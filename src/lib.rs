#![feature(let_else)]

use std::collections::HashMap;

use containers::{LoadInfo, LoadType};
use once_cell::sync::Lazy;
use rand::{thread_rng, RngCore};
use skyline::hooks::InlineCtx;
use smash_arc::{ArcLookup, Hash40, PathListEntry, SearchLookup};
use types::{FilesystemInfo, LoadedDirectory, ResServiceNX};

mod containers;
mod lua;
mod search;
mod types;

pub struct StageAltFolder {
    alt_no: usize,
    base_path: Hash40,
    new_path: Hash40,
    files: Vec<Hash40>,
}

impl StageAltFolder {
    pub fn new<H: Into<Hash40>, H2: Into<Hash40>>(alt_no: usize, base: H, new: H2) -> Self {
        Self {
            alt_no,
            base_path: base.into(),
            new_path: new.into(),
            files: Vec::new(),
        }
    }

    pub fn add_file<H: Into<Hash40>>(&mut self, name: H) {
        self.files.push(name.into());
    }

    pub fn base_path(&self, name: Hash40) -> Hash40 {
        Hash40(
            smash::phx::Hash40::new_raw(self.base_path.0)
                .concat(smash::phx::Hash40::new("/"))
                .concat(smash::phx::Hash40::new_raw(name.0))
                .as_u64(),
        )
    }

    pub fn new_path(&self, name: Hash40) -> Hash40 {
        Hash40(
            smash::phx::Hash40::new_raw(self.new_path.0)
                .concat(smash::phx::Hash40::new("/"))
                .concat(smash::phx::Hash40::new_raw(name.0))
                .as_u64(),
        )
    }

    pub fn files(&self) -> &Vec<Hash40> {
        &self.files
    }
}

pub struct StageAlts {
    folders: HashMap<Hash40, Vec<StageAltFolder>>,
    available_normal: HashMap<Hash40, usize>,
    available_battle: HashMap<Hash40, usize>,
}

impl StageAlts {
    pub fn new() -> Self {
        Self {
            folders: HashMap::new(),
            available_normal: HashMap::new(),
            available_battle: HashMap::new(),
        }
    }

    pub fn add_alt(&mut self, base_path: Hash40, alt: StageAltFolder) {
        if let Some(alts) = self.folders.get_mut(&base_path) {
            alts.push(alt);
        } else {
            self.folders.insert(base_path, vec![alt]);
        }
    }

    pub fn get_alt(&self, path: Hash40, alt_no: usize) -> Option<&StageAltFolder> {
        let alts = self.folders.get(&path)?;

        for alt in alts.iter() {
            if alt.alt_no == alt_no {
                return Some(alt);
            }
        }

        None
    }

    pub fn set_available_normal(&mut self, path: Hash40, available: usize) {
        self.available_normal.insert(path, available);
    }

    pub fn is_available_normal(&self, path: Hash40, what: usize) -> bool {
        if let Some(max) = self.available_normal.get(&path) {
            what < *max
        } else {
            false
        }
    }

    pub fn set_available_battle(&mut self, path: Hash40, available: usize) {
        self.available_battle.insert(path, available);
    }

    pub fn is_available_battle(&self, path: Hash40, what: usize) -> bool {
        if let Some(max) = self.available_battle.get(&path) {
            what < *max
        } else {
            false
        }
    }

    pub fn max_normal(&self, path: Hash40) -> usize {
        self.available_normal.get(&path).copied().unwrap_or(0)
    }

    pub fn max_battle(&self, path: Hash40) -> usize {
        self.available_battle.get(&path).copied().unwrap_or(0)
    }
}

impl Default for StageAlts {
    fn default() -> Self {
        Self::new()
    }
}

static STAGE_ALT_LOOKUP: Lazy<StageAlts> = Lazy::new(search::collect_stage_alts);

extern "C" {
    fn res_loop_start(ctx: &InlineCtx);
    fn initial_loading(ctx: &InlineCtx);
}

#[skyline::hook(replace = initial_loading)]
unsafe fn initial_loading_hook(ctx: &InlineCtx) {
    call_original!(ctx);
    Lazy::force(&STAGE_ALT_LOOKUP);
    Lazy::force(&lua::UI_TO_HASH_LOOKUP);
    Lazy::force(&lua::UI_FILEPATH_INDICES);
}

#[skyline::from_offset(0x353fa20)]
pub unsafe fn refc(table: &'static FilesystemInfo, index: u32);

#[skyline::from_offset(0x353fb30)]
pub unsafe fn unrefc(table: &'static FilesystemInfo, index: u32);

static mut CURRENT_STAGE_INDEX: usize = 0;
static mut INCOMING_RANDOM: usize = 0;

#[skyline::hook(offset = 0x353fe30)]
unsafe fn init_loaded_dir(info: &'static FilesystemInfo, index: u32) -> *mut LoadedDirectory {
    let result: *mut LoadedDirectory = call_original!(info, index);

    if result.is_null() {
        return result;
    }

    let loaded_directory = &mut *result;

    let Some(dir) = info.arc().get_dir_infos().get(index as usize) else { return result; };

    let mut alt_no = lua::INCOMING_ALTS[CURRENT_STAGE_INDEX];

    if alt_no == usize::MAX || alt_no == usize::MAX - 1 {
        alt_no = INCOMING_RANDOM;
    }

    if let Some(alt) = STAGE_ALT_LOOKUP.get_alt(dir.path.hash40(), alt_no) {
        for child in loaded_directory.child_path_indices.iter() {
            unrefc(info, *child);
        }
        loaded_directory.child_path_indices.clear();

        for child in alt.files() {
            let child = alt.new_path(*child);

            println!("{:#X} child: {:#x}", dir.path.hash40().0, child.0);
            let path_index = info.arc().get_file_path_index_from_hash(child).unwrap().0;

            loaded_directory.child_path_indices.push(path_index);
            refc(info, path_index);
        }
    }

    result
}

#[skyline::hook(replace = res_loop_start)]
unsafe fn res_loop_start_hook(ctx: &InlineCtx) {
    let info = FilesystemInfo::instance().unwrap();
    let Some(arc) = FilesystemInfo::instance().map(|i| i.arc()) else { return; };
    let Some(service) = ResServiceNX::instance_mut() else { return; };

    let mut loads = [vec![], vec![], vec![], vec![], vec![]];

    for (list_idx, list) in service.res_lists.iter().enumerate() {
        for entry in list.iter() {
            if let LoadType::Directory = entry.ty {
                let loaded_dir = &info.get_loaded_directories()[entry.directory_index as usize];

                for file in loaded_dir.child_path_indices.iter() {
                    let info_index = arc.get_file_info_indices()
                        [arc.get_file_paths()[*file as usize].path.index() as usize]
                        .file_info_index;
                    let info = &arc.get_file_infos()[info_index];

                    if info.flags.unused4() & 1 != 0 {
                        loads[list_idx].push(*file);
                    }
                }
            }
        }
    }

    for (idx, load) in loads.into_iter().enumerate() {
        for load in load {
            service.res_lists[idx].insert(LoadInfo {
                ty: LoadType::File,
                filepath_index: load,
                directory_index: 0xFF_FFFF,
                files_to_load: 0,
            })
        }
    }

    call_original!(ctx)
}

#[skyline::hook(offset = 0x25fd2b8, inline)]
unsafe fn prepare_for_load(ctx: &skyline::hooks::InlineCtx) {
    if CURRENT_STAGE_INDEX == usize::MAX {
        CURRENT_STAGE_INDEX = 0;
    } else {
        CURRENT_STAGE_INDEX = (CURRENT_STAGE_INDEX + 1) % 3;
    }

    if lua::INCOMING_ALTS[CURRENT_STAGE_INDEX] == usize::MAX {
        let search = FilesystemInfo::instance().unwrap().search();

        let Ok(path) = search.get_path_list_entry_from_hash(*ctx.registers[8].x.as_ref()) else {
            return;
        };

        let Ok(path) = search.get_path_list_entry_from_hash(path.parent.hash40()) else {
            return;
        };

        let max = STAGE_ALT_LOOKUP.max_normal(path.file_name.hash40());
        INCOMING_RANDOM = if max == 0 {
            0
        } else {
            (thread_rng().next_u64() % (max as u64)) as usize
        };
    } else if lua::INCOMING_ALTS[CURRENT_STAGE_INDEX] == usize::MAX - 1 {
        let search = FilesystemInfo::instance().unwrap().search();

        let Ok(path) = search.get_path_list_entry_from_hash(*ctx.registers[8].x.as_ref()) else {
            return;
        };

        let Ok(path) = search.get_path_list_entry_from_hash(path.parent.hash40()) else {
            return;
        };

        let max = STAGE_ALT_LOOKUP.max_battle(path.file_name.hash40());
        INCOMING_RANDOM = if max == 0 {
            0
        } else {
            (thread_rng().next_u64() % (max as u64)) as usize
        };
    }
}

extern "C" {
    fn check_extension_eff_inline_hook(ctx: &InlineCtx);
}

unsafe fn check_extension_common(ctx: &mut InlineCtx) -> bool {
    let eff_hash = Hash40::from("eff");

    // get the path list entry from x8
    let path_list_entry: &PathListEntry = &*(*ctx.registers[8].x.as_ref() as *const PathListEntry);

    // satisfy post conditions before doing fighter only block
    *ctx.registers[8].x.as_mut() = if path_list_entry.ext.hash40() == eff_hash {
        0
    } else {
        1
    };
    *ctx.registers[9].x.as_mut() = eff_hash.as_u64();

    println!(
        "{:#x}: {:#X}",
        *ctx.registers[26].w.as_ref(),
        path_list_entry.path.hash40().0
    );

    // Don't bother checking if the extension is not eff
    if *ctx.registers[8].x.as_ref() != 0 {
        return true;
    }

    let Some(alt) = STAGE_ALT_LOOKUP.get_alt(path_list_entry.parent.hash40(), lua::INCOMING_ALTS[CURRENT_STAGE_INDEX]) else {
        return true;
    };

    if alt.files.contains(&path_list_entry.file_name.hash40()) {
        *ctx.registers[8].x.as_mut() = 0;
        false
    } else {
        *ctx.registers[8].x.as_mut() = 1;
        true
    }
}

#[skyline::hook(replace = check_extension_eff_inline_hook)]
unsafe fn check_extension_eff_inline_hook_hook(ctx: &mut InlineCtx) {
    if check_extension_common(ctx) {
        call_original!(ctx);
    }
}

#[skyline::hook(offset = 0x355f66c, inline)]
unsafe fn check_extension_hook(ctx: &mut InlineCtx) {
    check_extension_common(ctx);
}

const AARCh264_NOP: u32 = 0xD503201F;

#[skyline::main(name = "stage-alts")]
pub fn main() {
    skyline::install_hooks!(
        res_loop_start_hook,
        init_loaded_dir,
        initial_loading_hook,
        prepare_for_load,
    );

    let is_one_slot_eff_present = unsafe {
        let mut out = 0;
        skyline::nn::ro::LookupSymbol(&mut out, "check_extension_eff_inline_hook\0".as_ptr() as _);
        out != 0
    };

    if !is_one_slot_eff_present {
        unsafe {
            let _ = skyline::patching::patch_data(
                0x355f66c,
                &[
                    AARCh264_NOP,
                    AARCh264_NOP,
                    AARCh264_NOP,
                    AARCh264_NOP,
                    AARCh264_NOP,
                    AARCh264_NOP,
                ],
            );
        }
        skyline::install_hook!(check_extension_hook);
    } else {
        skyline::install_hook!(check_extension_eff_inline_hook_hook);
    }

    lua::install();
}
