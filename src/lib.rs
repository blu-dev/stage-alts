#![feature(let_else)]
#![feature(label_break_value)]
use std::{collections::HashMap, sync::atomic::Ordering};

use containers::{LoadInfo, LoadType};
use log::error;
use once_cell::sync::Lazy;
use rand::{thread_rng, RngCore};
use skyline::hooks::InlineCtx;
use smash_arc::{ArcLookup, FilePath, Hash40, PathListEntry, SearchLookup};
use types::{FilesystemInfo, LoadedDirectory, ResServiceNX};

mod alts;
mod containers;
#[cfg(feature = "logger")]
mod logger;
mod lua;
mod search;
mod types;

#[macro_export]
macro_rules! hash40_fmt {
    ($str:expr $(, $args:expr)*) => {
        Hash40::from(format!($str $(, $args)*).as_str())
    }
}

trait Hash40Ext: Sized {
    fn concat<H: Into<Self>>(self, other: H) -> Self;
    fn join_path<H: Into<Self>>(self, other: H) -> Self;
}

impl Hash40Ext for Hash40 {
    fn concat<H: Into<Self>>(self, other: H) -> Self {
        let raw = smash::phx::Hash40::new_raw(self.as_u64())
            .concat(smash::phx::Hash40::new_raw(other.into().as_u64()))
            .as_u64();

        Self(raw)
    }

    fn join_path<H: Into<Self>>(self, other: H) -> Self {
        self.concat("/").concat(other)
    }
}

extern "C" {
    fn res_loop_start(ctx: &InlineCtx);
    fn initial_loading(ctx: &InlineCtx);
}

#[skyline::from_offset(0x353fa20)]
pub unsafe fn refc(table: &'static FilesystemInfo, index: u32);

#[skyline::from_offset(0x353fb30)]
pub unsafe fn unrefc(table: &'static FilesystemInfo, index: u32);

#[skyline::from_offset(0x35455d0)]
pub unsafe fn add_to_res_list(res_service: &'static ResServiceNX, index: u32, list_index: u32);

static mut CURRENT_STAGE_INDEX: usize = 0;
static mut INCOMING_RANDOM: usize = 0;

#[skyline::hook(replace = initial_loading)]
unsafe fn initial_loading_hook(ctx: &InlineCtx) {
    call_original!(ctx);

    Lazy::force(&lua::UI_TO_HASH_LOOKUP);
    search::collect_alts();
}

#[skyline::hook(offset = 0x353fe30)]
unsafe fn init_loaded_dir(info: &'static FilesystemInfo, index: u32) -> *mut LoadedDirectory {
    let result: *mut LoadedDirectory = call_original!(info, index);

    if result.is_null() {
        return result;
    }

    let loaded_directory = &mut *result;

    let Some(dir) = info.arc().get_dir_infos().get(index as usize) else { return result; };

    let mgr = alts::get();

    if !mgr.does_current_alt_have_folder(dir.path.hash40()) {
        log::info!("Alt does not have folder {:#x}", dir.path.hash40().0);
        return result;
    }

    log::info!("Current alt has folder {:#x}!", dir.path.hash40().0);

    let Some(files) = mgr.get_files_for_alt_folder(dir.path.hash40()) else {
        error!("Current alt should have folder {:#x} but it was not found in the search section! Perhaps the config is incorrect?", dir.path.hash40().0);
        return result;
    };

    let arc = info.arc();
    let file_paths = arc.get_file_paths();

    let sharing_base = mgr
        .get_sharing_base_for_alt_folder(dir.path.hash40())
        .unwrap();

    for child in loaded_directory.child_path_indices.iter() {
        unrefc(info, *child);
        // if info.get_loaded_datas()
        //     [info.get_loaded_filepaths()[*child as usize].loaded_data_index as usize]
        //     .ref_count
        //     .load(Ordering::SeqCst)
        //     == 0
        // {
        //     if let Some((_, modded)) = sharing_base.get(&file_paths[*child as usize].path.hash40())
        //     {
        //         (*(arc.file_paths as *mut FilePath).add(*child as usize))
        //             .path
        //             .set_index(*modded);
        //     }
        // }
    }

    loaded_directory.child_path_indices.clear();

    for file in files {
        loaded_directory.child_path_indices.push(file.0);
        refc(info, file.0);
        add_to_res_list(ResServiceNX::instance().unwrap(), file.0, 0);
    }

    result
}

#[skyline::hook(offset = 0x353e5c0)]
unsafe fn uninit_loaded_dir(info: &'static FilesystemInfo, dir: *mut LoadedDirectory) {}

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

                // let Some(dir_info_name) = arc.get_dir_infos().get(loaded_dir.file_group_index as usize).map(|info| info.path.hash40()) else {
                //     continue;
                // };

                for file in loaded_dir.child_path_indices.iter() {
                    let info_index = arc.get_file_info_indices()
                        [arc.get_file_paths()[*file as usize].path.index() as usize]
                        .file_info_index;
                    let info = &arc.get_file_infos()[info_index];

                    if info.flags.unused4() & 5 != 0 {
                        loads[list_idx].push(info.file_path_index.0);
                    }
                }
            }
        }
    }

    for (idx, load) in loads.into_iter().enumerate() {
        for load in load {
            log::info!("Adding {:#x} to load list", load);
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
    let search = FilesystemInfo::instance().unwrap().search();
    let Ok(path) = search.get_path_list_entry_from_hash(*ctx.registers[8].x.as_ref()) else {
        error!("Failed to get the path list entry from {:#x}", *ctx.registers[8].x.as_ref());
        return;
    };

    let Ok(parent_path) = search.get_path_list_entry_from_hash(path.parent.hash40()) else {
        error!("Failed to get the parent of the path {:#x}", path.path.hash40().0);
        return;
    };

    let mut mgr = alts::get_mut();

    mgr.advance_alt(
        parent_path.file_name.hash40(),
        path.file_name.hash40() != Hash40::from("normal")
            && parent_path.file_name.hash40() != Hash40::from("end"),
    );
}

#[skyline::hook(offset = 0x22d91f0, inline)]
unsafe fn online_melee_any_scene_create(_: &InlineCtx) {
    let mut mgr = alts::get_mut();
    mgr.is_online = true;
}

#[skyline::hook(offset = 0x22d9120, inline)]
unsafe fn bg_matchmaking_seq(_: &InlineCtx) {
    let mut mgr = alts::get_mut();
    mgr.is_online = true;
}

#[skyline::hook(offset = 0x22d9050, inline)]
unsafe fn arena_seq(_: &InlineCtx) {
    let mut mgr = alts::get_mut();
    mgr.is_online = true;
}

#[skyline::hook(offset = 0x23599ac, inline)]
unsafe fn main_menu(_: &InlineCtx) {
    let mut mgr = alts::get_mut();
    mgr.selection = vec![];
    mgr.current_index = 0;
    mgr.is_online = false;
}

#[skyline::main(name = "stage-alts")]
pub fn main() {
    #[cfg(feature = "logger")]
    {
        logger::init();
    }

    skyline::install_hooks!(
        res_loop_start_hook,
        init_loaded_dir,
        initial_loading_hook,
        prepare_for_load,
        online_melee_any_scene_create,
        bg_matchmaking_seq,
        arena_seq,
        main_menu
    );

    lua::install();
}
