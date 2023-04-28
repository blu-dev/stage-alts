use log::error;
use smash_arc::{
    serde::Hash40String, ArcFile, ArcLookup, FolderPathListEntry, Hash40, HashToIndex, LoadedArc,
    LoadedSearchSection, LookupError, PathListEntry, SearchLookup,
};
use std::collections::HashMap;

use crate::{
    alts::{StageAlt, StageAltInfo},
    hash40_fmt,
    types::FilesystemInfo,
    Hash40Ext,
};

pub trait SearchEx: SearchLookup {
    fn get_folder_path_to_index_mut(&mut self) -> &mut [HashToIndex];
    fn get_folder_path_list_mut(&mut self) -> &mut [FolderPathListEntry];
    fn get_path_to_index_mut(&mut self) -> &mut [HashToIndex];
    fn get_path_list_indices_mut(&mut self) -> &mut [u32];
    fn get_path_list_mut(&mut self) -> &mut [PathListEntry];

    fn get_folder_path_index_from_hash_mut(
        &mut self,
        hash: impl Into<Hash40>,
    ) -> Result<&mut HashToIndex, LookupError> {
        let folder_path_to_index = self.get_folder_path_to_index_mut();
        match folder_path_to_index.binary_search_by_key(&hash.into(), |h| h.hash40()) {
            Ok(idx) => Ok(&mut folder_path_to_index[idx]),
            Err(_) => Err(LookupError::Missing),
        }
    }

    fn get_folder_path_entry_from_hash_mut(
        &mut self,
        hash: impl Into<Hash40>,
    ) -> Result<&mut FolderPathListEntry, LookupError> {
        let index = *self.get_folder_path_index_from_hash(hash)?;
        if index.index() != 0xFF_FFFF {
            Ok(&mut self.get_folder_path_list_mut()[index.index() as usize])
        } else {
            Err(LookupError::Missing)
        }
    }

    fn get_path_index_from_hash_mut(
        &mut self,
        hash: impl Into<Hash40>,
    ) -> Result<&mut HashToIndex, LookupError> {
        let path_to_index = self.get_path_to_index_mut();
        match path_to_index.binary_search_by_key(&hash.into(), |h| h.hash40()) {
            Ok(idx) => Ok(&mut path_to_index[idx]),
            Err(_) => Err(LookupError::Missing),
        }
    }

    fn get_path_list_index_from_hash_mut(
        &mut self,
        hash: impl Into<Hash40>,
    ) -> Result<&mut u32, LookupError> {
        let index = *self.get_path_index_from_hash(hash)?;
        if index.index() != 0xFF_FFFF {
            Ok(&mut self.get_path_list_indices_mut()[index.index() as usize])
        } else {
            Err(LookupError::Missing)
        }
    }

    fn get_path_list_entry_from_hash_mut(
        &mut self,
        hash: impl Into<Hash40>,
    ) -> Result<&mut PathListEntry, LookupError> {
        let index = self.get_path_list_index_from_hash(hash)?;
        if index != 0xFF_FFFF {
            Ok(&mut self.get_path_list_mut()[index as usize])
        } else {
            Err(LookupError::Missing)
        }
    }

    fn get_first_child_in_folder_mut(
        &mut self,
        hash: impl Into<Hash40>,
    ) -> Result<&mut PathListEntry, LookupError> {
        let folder_path = self.get_folder_path_entry_from_hash(hash)?;
        let index_idx = folder_path.get_first_child_index();

        if index_idx == 0xFF_FFFF {
            return Err(LookupError::Missing);
        }

        let path_entry_index = self.get_path_list_indices()[index_idx];
        if path_entry_index != 0xFF_FFFF {
            Ok(&mut self.get_path_list_mut()[path_entry_index as usize])
        } else {
            Err(LookupError::Missing)
        }
    }

    fn get_next_child_in_folder_mut(
        &mut self,
        current_child: &PathListEntry,
    ) -> Result<&mut PathListEntry, LookupError> {
        let index_idx = current_child.path.index() as usize;
        if index_idx == 0xFF_FFFF {
            return Err(LookupError::Missing);
        }

        let path_entry_index = self.get_path_list_indices()[index_idx];
        if path_entry_index != 0xFF_FFFF {
            Ok(&mut self.get_path_list_mut()[path_entry_index as usize])
        } else {
            Err(LookupError::Missing)
        }
    }
}

impl SearchEx for LoadedSearchSection {
    fn get_folder_path_to_index_mut(&mut self) -> &mut [HashToIndex] {
        unsafe {
            let table_size = (*self.body).folder_path_count;
            std::slice::from_raw_parts_mut(self.folder_path_index as _, table_size as usize)
        }
    }

    fn get_folder_path_list_mut(&mut self) -> &mut [FolderPathListEntry] {
        unsafe {
            let table_size = (*self.body).folder_path_count;
            std::slice::from_raw_parts_mut(self.folder_path_list as _, table_size as usize)
        }
    }

    fn get_path_to_index_mut(&mut self) -> &mut [HashToIndex] {
        unsafe {
            let table_size = (*self.body).path_indices_count;
            std::slice::from_raw_parts_mut(self.path_index as _, table_size as usize)
        }
    }

    fn get_path_list_indices_mut(&mut self) -> &mut [u32] {
        unsafe {
            let table_size = (*self.body).path_indices_count;
            std::slice::from_raw_parts_mut(self.path_list_indices as _, table_size as usize)
        }
    }

    fn get_path_list_mut(&mut self) -> &mut [PathListEntry] {
        unsafe {
            let table_size = (*self.body).path_count;
            std::slice::from_raw_parts_mut(self.path_list as _, table_size as usize)
        }
    }
}

pub enum SearchEntry {
    File(u32),
    Folder {
        index: u32,
        children: Vec<SearchEntry>,
    },
}

pub trait FlattenVec {
    fn flatten(self) -> Self;
}

impl FlattenVec for Vec<SearchEntry> {
    fn flatten(self) -> Self {
        let mut out = vec![];

        for child in self {
            match child {
                SearchEntry::Folder { children, .. } => out.extend(children.flatten()),
                other => out.push(other),
            }
        }

        out
    }
}

pub fn walk_search_section(
    search: &LoadedSearchSection,
    hash: Hash40,
    depth: isize,
) -> Vec<SearchEntry> {
    if depth == 0 {
        return vec![];
    }

    let Ok(folder) = search.get_folder_path_entry_from_hash(hash) else { return vec![] };

    let mut child_index = folder.get_first_child_index() as u32;

    let mut children = vec![];

    let path_indices = search.get_path_list_indices();
    let paths = search.get_path_list();

    loop {
        if child_index == 0xFF_FFFF {
            break;
        }

        let path_index = path_indices[child_index as usize];
        if path_index == 0xFF_FFFF {
            break;
        }

        let path = &paths[path_index as usize];

        if path.is_directory() {
            children.push(SearchEntry::Folder {
                index: path_index,
                children: walk_search_section(search, path.path.hash40(), depth - 1),
            })
        } else {
            children.push(SearchEntry::File(path_index))
        }

        child_index = path.path.index() as u32;
    }

    children
}

fn get_direct_child(search: &LoadedSearchSection, index: u32, child: Hash40) -> Option<u32> {
    let parent = search.get_path_list().get(index as usize)?;
    let folder = search
        .get_folder_path_entry_from_hash(parent.path.hash40())
        .ok()?;

    let mut child_index = folder.get_first_child_index() as u32;

    let path_indices = search.get_path_list_indices();
    let paths = search.get_path_list();

    loop {
        if child_index == 0xFF_FFFF {
            break;
        }

        let path_index = path_indices[child_index as usize];
        if path_index == 0xFF_FFFF {
            break;
        }

        let path = &paths[path_index as usize];

        if path.file_name.hash40() == child {
            return Some(path_index);
        }

        child_index = path.path.index() as u32;
    }

    None
}

fn get_child_referencing(search: &LoadedSearchSection, index: u32, child: Hash40) -> Option<u32> {
    let parent = search.get_path_list().get(index as usize)?;
    let folder = search
        .get_folder_path_entry_from_hash(parent.path.hash40())
        .ok()?;

    let mut child_index = folder.get_first_child_index() as u32;

    let path_indices = search.get_path_list_indices();
    let paths = search.get_path_list();

    let mut prev = 0xFF_FFFF;

    loop {
        if child_index == 0xFF_FFFF {
            break;
        }

        let path_index = path_indices[child_index as usize];
        if path_index == 0xFF_FFFF {
            break;
        }

        let path = &paths[path_index as usize];

        if path.file_name.hash40() == child {
            return Some(prev);
        }

        prev = child_index;
        child_index = path.path.index() as u32;
    }

    None
}

fn join_path<H1: Into<Hash40>, H2: Into<Hash40>>(parent: H1, child: H2) -> Hash40 {
    let parent = parent.into();
    let child = child.into();
    Hash40(
        smash::phx::Hash40::new_raw(parent.as_u64())
            .concat(smash::phx::Hash40::new("/"))
            .concat(smash::phx::Hash40::new_raw(child.0))
            .as_u64(),
    )
}

fn concat<H1: Into<Hash40>, H2: Into<Hash40>>(first: H1, second: H2) -> Hash40 {
    let first = first.into();
    let second = second.into();

    Hash40(
        smash::phx::Hash40::new_raw(first.as_u64())
            .concat(smash::phx::Hash40::new_raw(second.as_u64()))
            .as_u64(),
    )
}

fn file_order_fix(search: &mut LoadedSearchSection, src: Hash40, dst: Hash40) {
    // as a preliminary step, we should get the path index of the destination folder, and leave
    // if it is not a folder or it does not exist
    let Ok(dst_index) = search.get_path_list_index_from_hash(dst) else {
        return;
    };

    if !search.get_path_list()[dst_index as usize].is_directory() {
        return;
    }

    // to begin, we do a 1-depth walk on the source directory. This will give us the list of entries in
    // the order that they appear in the search section
    let source_entries = walk_search_section(search, src, 1);

    // we check if there are zero entries, and then dip since there is no ordering to do!
    if source_entries.is_empty() {
        return;
    }

    // then, we want to iterate through each of the entries, grab their name, and call `get_direct_child`
    // on them. this will allow us to accumulate a vector of the new files in their proper order
    let mut ordered_children = vec![];
    for entry in source_entries {
        let index = match entry {
            SearchEntry::File(index) => index,
            SearchEntry::Folder { index, .. } => index,
        };

        let src_child_path = search.get_path_list()[index as usize];

        let file_name = src_child_path.file_name.hash40();

        if let Some(dst_child) = get_direct_child(search, dst_index, file_name) {
            ordered_children.push(dst_child);

            let dst_child_path = search.get_path_list()[dst_child as usize];

            if dst_child_path.is_directory() {
                file_order_fix(
                    search,
                    src_child_path.path.hash40(),
                    dst_child_path.path.hash40(),
                );
            }
        }
    }

    // we have now acquired a fixed list of all of our entries **as they appear on the base slot**
    // so now what we need to do is get the rest of what is *not* in that list and tack it on to the end
    // the order in which they are placed on the end is not gauranteed to be valid
    let dst_entries = walk_search_section(search, dst, 1);

    for entry in dst_entries {
        let index = match entry {
            SearchEntry::File(index) => index,
            SearchEntry::Folder { index, .. } => index,
        };

        if !ordered_children.contains(&index) {
            ordered_children.push(index);
        }
    }

    // we get the folder path index ahead of time to simplify our code later
    let Ok(folder_path_index) = search.get_folder_path_index_from_hash(dst) else {
        return;
    };

    let folder_path_index = folder_path_index.index();

    // finally, we should now recreate the file listing in the search section.
    // we check if there are zero entries, and if so we set the first child
    // to the invalid index and leave
    if ordered_children.is_empty() {
        search.get_folder_path_list_mut()[folder_path_index as usize]
            .set_first_child_index(0xFF_FFFF);
        return;
    }

    // we have confirmed that there is at least one entry, so we then set the first child
    // index to that entry, and then we can loop through the rest of them since it is in the search section
    search.get_folder_path_list_mut()[folder_path_index as usize]
        .set_first_child_index(ordered_children[0]);

    for x in 1..ordered_children.len() {
        search.get_path_list_mut()[ordered_children[x - 1] as usize]
            .path
            .set_index(ordered_children[x]);
    }

    // finish by setting the last path's next path to invalid
    search.get_path_list_mut()[*ordered_children.last().unwrap() as usize]
        .path
        .set_index(0xFF_FFFF);
}

fn collect_folders(search: &LoadedSearchSection, path: Hash40, mut base: Hash40) -> Vec<Hash40> {
    let children = walk_search_section(search, path, 1);

    if base != Hash40(0) {
        base = base.concat("/");
    }

    let mut out = vec![];
    for child in children {
        let SearchEntry::Folder { index, .. } = child else {
            continue;
        };

        let path = search.get_path_list()[index as usize];
        let next_path = base.concat(path.file_name.hash40());
        out.push(next_path);

        out.extend(collect_folders(search, path.path.hash40(), next_path));
    }

    out
}

fn get_ui_files(stage_name: Hash40, alt_no: usize) -> [Hash40; 5] {
    let replace_path = if crate::alts::StageAltManager::is_dlc_stage(stage_name) {
        Hash40::from("ui/replace_patch/stage")
    } else {
        Hash40::from("ui/replace/stage")
    };

    let stage_name = if stage_name == Hash40::from("battlefield_s") {
        Hash40::from("battlefields")
    } else if stage_name == Hash40::from("battlefield_l") {
        Hash40::from("battlefieldl")
    } else {
        stage_name
    };

    let suffix = if alt_no == 0 {
        Hash40::from(".bntx")
    } else {
        hash40_fmt!("_s{:02}.bntx", alt_no)
    };

    [
        replace_path
            .join_path("stage_0")
            .join_path("stage_0_")
            .concat(stage_name)
            .concat(suffix),
        replace_path
            .join_path("stage_1")
            .join_path("stage_1_")
            .concat(stage_name)
            .concat(suffix),
        replace_path
            .join_path("stage_2")
            .join_path("stage_2_")
            .concat(stage_name)
            .concat(suffix),
        replace_path
            .join_path("stage_3")
            .join_path("stage_3_")
            .concat(stage_name)
            .concat(suffix),
        replace_path
            .join_path("stage_4")
            .join_path("stage_4_")
            .concat(stage_name)
            .concat(suffix),
    ]
}

fn collect_sharing_base(
    arc: &LoadedArc,
    search: &LoadedSearchSection,
    folder_lookup: &HashMap<Hash40, Hash40>,
) -> HashMap<Hash40, (u32, u32)> {
    let mut out = HashMap::new();
    for (base, modded) in folder_lookup.iter() {
        let files: Vec<u32> = walk_search_section(search, *modded, 1)
            .into_iter()
            .filter_map(|entry| {
                if let SearchEntry::File(idx) = entry {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect();

        for file in files {
            let name = search.get_path_list()[file as usize].file_name.hash40();
            let base_path = base.join_path(name);
            let modded_path = modded.join_path(name);

            let Ok(base_fp_index) = arc.get_file_path_index_from_hash(base_path) else {
                continue;
            };
            let modded_fp_index = arc.get_file_path_index_from_hash(modded_path).unwrap();

            let file_paths = arc.get_file_paths();
            if file_paths[base_fp_index].path.index() != file_paths[modded_fp_index].path.index() {
                out.insert(
                    base_path,
                    (
                        file_paths[base_fp_index].path.index(),
                        file_paths[modded_fp_index].path.index(),
                    ),
                );
            }
        }
    }
    out
}

pub fn collect_alts() {
    // Acquire a mutable and immutable reference
    // This is a really bad practice but if we are careful it's fine
    let search = FilesystemInfo::instance().unwrap().search();
    let search_mut = FilesystemInfo::instance_mut().unwrap().search_mut();

    // Also acquire a reference to the arc
    // We use this to do unsharing and resharing on the fly
    let arc = FilesystemInfo::instance().unwrap().arc();

    // Collect all of the stage folders in the stage directory, we are going to check them on a case by case basis for stage alts
    let stage_folders = walk_search_section(search, Hash40::from("stage"), 1);

    let mut alt_infos = HashMap::new();
    let mut total_alts = vec![];

    for stage in stage_folders {
        // stage folder entries must be folders
        let SearchEntry::Folder { index: stage_folder_index, .. } = stage else {
            error!("File encountered in stage folder!");
            continue;
        };

        // go ahead and get the actual path entry since we are going to be using it a bit
        let stage_path = search.get_path_list()[stage_folder_index as usize];

        // move on to the next folder if this one is common, there are no alts to be had on common
        if stage_path.file_name.hash40() == Hash40::from("common") {
            continue;
        }

        // We attempt to get the normal path. This one is unconditional because every stage must have a normal folder, even battlefield
        let Some(normal_path) = get_direct_child(search, stage_folder_index, Hash40::from("normal")).map(|index| search.get_path_list()[index as usize]) else {
            error!("Stage {:#x} did not have normal folder!", stage_path.file_name.hash40().0);
            continue;
        };

        // We get the battle path if it exists, it does not exist for boss stages (iirc?) and small bf, big bf, and fd
        let battle_path = get_direct_child(search, stage_folder_index, Hash40::from("battle"))
            .map(|index| search.get_path_list()[index as usize]);

        let mut alts = vec![];
        for x in 0..100 {
            // If there is no normal alt then there definitely won't be a battlefield alt
            // Even for battlefield form only mods there will still be a normal alt it will just be the vanilla stage
            let Some(normal_alt_index) = get_direct_child(search, stage_folder_index, hash40_fmt!("normal_s{:02}", x)) else {
                continue;
            };

            let normal_alt = search.get_path_list()[normal_alt_index as usize];

            let is_normal_ws =
                get_direct_child(search, normal_alt_index, Hash40::from("wifi-safe.flag"))
                    .is_some();
            let is_normal_ignore =
                get_direct_child(search, normal_alt_index, Hash40::from("wifi-ignore.flag"))
                    .is_some();

            // When we get here, we know that we have an alt. So we are going to first collect the UI paths and the effect folder. Makes the most sense to do these
            // as they don't require any discovery -- they are static paths.

            // The effect path is handled on a folder basis to ensure that in the off-chance that any stage effect folder has more than just a .eff
            // that it's going to be handled, but even if that isn't the case it enables not colliding with one-slot effects.
            let mut folder_lookup = HashMap::new();
            folder_lookup.insert(
                Hash40::from("effect/stage").join_path(stage_path.file_name.hash40()),
                Hash40::from("effect/stage")
                    .join_path(stage_path.file_name.hash40())
                    .concat(hash40_fmt!("_s{:02}", x)),
            );

            // The UI files are also static and can just be generated.
            let ui_files = get_ui_files(stage_path.file_name.hash40(), x);

            // Perform the file order fix on the normal section
            // This is a very important step, as often the search section will walk through the children and find the first file with a certain extension. In vanilla, these are all formatted
            // via alphabetical order, so a file like `poke_stadium2_00.lvd` will be detected before `poke_stadium2_01.lvd`
            // SAFETY: Upon reaching this point, having any immutable references to the search section is considered invalid, so don't do that. All of our indices/state
            // at this point is managed via indices.
            file_order_fix(
                search_mut,
                normal_path.path.hash40(),
                normal_alt.path.hash40(),
            );

            // We are using the regular stage normal path here to collect these because we can use that to detect
            // if there is something missing when loading the stage alt. Allows us to display a panic error message instead
            // of just crashing a little bit later, or the user having unintended effects
            //
            // Note: Each of these paths are relative to the normal path, meaning we will get paths like `model/floating_plate_set`, which is dope
            // because it means we can join it against our roots separately without having to rediscover
            let folders = collect_folders(search, normal_path.path.hash40(), Hash40::from(""));

            // We create our folder lookup here, this is going to become part of the stage alt.
            for folder in folders {
                folder_lookup.insert(
                    normal_path.path.hash40().join_path(folder),
                    normal_alt.path.hash40().join_path(folder),
                );
            }

            // do the same thing for the battle paths if they exist
            let mut is_battle_ws = false;
            let mut is_battle_ignore = false;
            'battle: {
                let Some(battle_path) = battle_path else {
                    break 'battle;
                };

                let Some(battle_alt_index) = get_direct_child(search, stage_folder_index, hash40_fmt!("battle_s{:02}", x)) else {
                    error!("The battlefield form alt for {:#x} was not discovered even thought it is erquired.", stage_path.file_name.hash40().0);
                    break 'battle;
                };

                let battle_alt = search.get_path_list()[battle_alt_index as usize];

                is_battle_ws =
                    get_direct_child(search, battle_alt_index, Hash40::from("wifi-safe.flag"))
                        .is_some();
                is_battle_ignore =
                    get_direct_child(search, battle_alt_index, Hash40::from("wifi-ignore.flag"))
                        .is_some();

                file_order_fix(
                    search_mut,
                    battle_path.path.hash40(),
                    battle_alt.path.hash40(),
                );

                let folders = collect_folders(search, battle_path.path.hash40(), Hash40::from(""));

                for folder in folders {
                    folder_lookup.insert(
                        battle_path.path.hash40().join_path(folder),
                        battle_alt.path.hash40().join_path(folder),
                    );
                }
            }

            let sharing_base = collect_sharing_base(arc, search, &folder_lookup);

            alts.push(std::sync::Arc::new(StageAlt {
                alt_folders: folder_lookup,
                sharing_base,
                ui_paths: ui_files,
                is_normal_ws,
                is_normal_ignore,
                is_battle_ws,
                is_battle_ignore,
            }));
        }

        if !alts.is_empty() {
            let mut folder_lookup = HashMap::new();
            folder_lookup.insert(
                Hash40::from("effect/stage").join_path(stage_path.file_name.hash40()),
                Hash40::from("effect/stage").join_path(stage_path.file_name.hash40()),
            );

            let ui_files = get_ui_files(stage_path.file_name.hash40(), 0);
            let folders = collect_folders(search, normal_path.path.hash40(), Hash40::from(""));

            folder_lookup.extend(folders.into_iter().map(|path| {
                (
                    normal_path.path.hash40().join_path(path),
                    normal_path.path.hash40().join_path(path),
                )
            }));

            if let Some(battle_path) = battle_path {
                let folders = collect_folders(search, battle_path.path.hash40(), Hash40::from(""));

                folder_lookup.extend(folders.into_iter().map(|path| {
                    (
                        battle_path.path.hash40().join_path(path),
                        battle_path.path.hash40().join_path(path),
                    )
                }));
            }

            alts.insert(
                0,
                std::sync::Arc::new(StageAlt {
                    alt_folders: folder_lookup,
                    sharing_base: HashMap::new(),
                    ui_paths: ui_files,
                    is_normal_ws: true,
                    is_normal_ignore: false,
                    is_battle_ws: true,
                    is_battle_ignore: false,
                }),
            );
        }

        let stage_name = stage_path.file_name.hash40();

        let mut alt_info = StageAltInfo {
            stage_name,
            stage_folder: stage_path.path.hash40(),
            normal_folder: stage_path.path.hash40().join_path("normal"),
            battle_folder: stage_path.path.hash40().join_path("battle"),
            alts_found: vec![],
        };

        for alt in alts {
            alt_info.alts_found.push(alt.clone());
            total_alts.push(alt);
        }

        alt_infos.insert(alt_info.stage_name, alt_info);
    }

    let mut mgr = crate::alts::get_mut();
    mgr.alt_infos = alt_infos;
    mgr.alts = total_alts;
}
