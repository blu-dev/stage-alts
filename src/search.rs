use smash_arc::{
    serde::Hash40String, ArcFile, ArcLookup, FolderPathListEntry, Hash40, HashToIndex,
    LoadedSearchSection, LookupError, PathListEntry, SearchLookup,
};
use std::collections::HashMap;

use crate::{types::FilesystemInfo, StageAltFolder, StageAlts};

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

macro_rules! hash40_fmt {
    ($str:expr $(, $args:expr)*) => {
        Hash40::from(format!($str $(, $args)*).as_str())
    }
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

fn diff_folders(
    search: &LoadedSearchSection,
    arc: &ArcFile,
    src: Hash40,
    dst: Hash40,
    dst_parent: Folder,
) -> HashMap<Hash40String, NewFile> {
    // we begin diffing by seeing if the dst folder *even exists*
    // if it does not exist, then we are basically just gonna copy everything from src to our output
    let children = walk_search_section(search, src, 1);

    let path_index = search
        .get_path_list_index_from_hash(dst)
        .unwrap_or(0xFF_FFFF);

    let mut new_files = HashMap::new();

    for child in children {
        match child {
            SearchEntry::File(index) => {
                // check if it is a vanilla file, if so then we can continue, otherwise
                // we aren't going to try sharing to it
                if arc
                    .get_file_path_index_from_hash(
                        search.get_path_list()[index as usize].path.hash40(),
                    )
                    .is_err()
                {
                    continue;
                }

                // get the path hash for this and see if it exists in dst
                let name = search.get_path_list()[index as usize].file_name.hash40();

                let exists = if path_index != 0xFF_FFFF {
                    get_direct_child(search, path_index, name).is_some()
                } else {
                    false
                };

                if !exists {
                    new_files.insert(
                        Hash40String(search.get_path_list()[index as usize].path.hash40()),
                        NewFile {
                            full_path: Hash40String(join_path(dst, name)),
                            file_name: Hash40String(name),
                            parent: dst_parent.clone(),
                            extension: Hash40String(
                                search.get_path_list()[index as usize].ext.hash40(),
                            ),
                        },
                    );
                }
            }
            SearchEntry::Folder { index, .. } => {
                let name = search.get_path_list()[index as usize].file_name.hash40();

                // let exists = if path_index != 0xFF_FFFF {
                //     println!("getting child");
                //     std::thread::sleep(std::time::Duration::from_millis(100));
                //     let child = get_direct_child(search, path_index, name).is_some();
                //     println!("gotten child");
                //     child
                // } else {
                //     false
                // };

                // if !exists {
                let new_parent = Folder {
                    full_path: Hash40String(join_path(dst, name)),
                    name: Some(Hash40String(name)),
                    parent: Some(Box::new(dst_parent.clone())),
                };

                new_files.extend(diff_folders(
                    search,
                    arc,
                    search.get_path_list()[index as usize].path.hash40(),
                    join_path(dst, name),
                    new_parent,
                ));

                // }
            }
        }
    }

    new_files
}

fn collect_alts(
    search: &LoadedSearchSection,
    base_path: Hash40,
    index: u32,
    alt_no: usize,
) -> Vec<StageAltFolder> {
    let hash = search.get_path_list()[index as usize].path.hash40();
    let walked = walk_search_section(search, hash, 1);

    let mut self_alt = StageAltFolder::new(alt_no, base_path, hash);

    let mut alts = vec![];

    for child in walked {
        match child {
            SearchEntry::File(index) => {
                let child = &search.get_path_list()[index as usize];
                self_alt.add_file(child.file_name.hash40());
            }
            SearchEntry::Folder { index, .. } => {
                let child = &search.get_path_list()[index as usize];
                let new_base_path = Hash40(
                    smash::phx::Hash40::new_raw(base_path.0)
                        .concat(smash::phx::Hash40::new("/"))
                        .concat(smash::phx::Hash40::new_raw(child.file_name.hash40().0))
                        .as_u64(),
                );

                alts.extend(collect_alts(search, new_base_path, index, alt_no));
            }
        }
    }

    if !self_alt.files().is_empty() {
        alts.push(self_alt);
    }

    alts
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

fn fix_order_specific_files(search: &mut LoadedSearchSection) {
    let mut counter = 1;
    let index = search
        .get_path_list_index_from_hash("stage/poke_stadium2")
        .unwrap();
    while let Some(alt_index) =
        get_direct_child(search, index, hash40_fmt!("normal_s{:02}", counter))
    {
        let Some(param_folder) = get_direct_child(search, alt_index, Hash40::from("param")) else { 
            counter += 1;
            continue;
        };
        let Some(referencing) = get_child_referencing(search, param_folder, Hash40::from("xstadium_00.lvd")) else {
            counter += 1;
            continue;
        };

        if referencing == 0xFF_FFFF {
            counter += 1;
            continue;
        }

        let lvd_index = search.get_path_list()[referencing as usize].path.index();
        let lvd_next = search.get_path_list()[lvd_index as usize].path.index();

        let first_child_index = {
            let parent_hash = search.get_path_list()[param_folder as usize].path.hash40();
            let folder = search
                .get_folder_path_entry_from_hash_mut(parent_hash)
                .unwrap();
            let prev = folder.get_first_child_index();
            folder.set_first_child_index(lvd_index);
            prev as u32
        };
        search.get_path_list_mut()[referencing as usize]
            .path
            .set_index(lvd_next);
        search.get_path_list_mut()[lvd_index as usize]
            .path
            .set_index(first_child_index);
        counter += 1;
    }
}

fn get_eff_folder_from_stage(stage: Hash40) -> Hash40 {
    join_path("effect/stage", stage)
}

fn get_eff_name_from_stage(stage: Hash40, alt: usize) -> Hash40 {
    if alt == 0 {
        concat(concat("ef_", stage), ".eff")
    } else {
        concat(concat("ef_", stage), hash40_fmt!("_s{:02}.eff", alt))
    }
}

fn get_eff_path_from_stage(stage: Hash40, alt: usize) -> Hash40 {
    join_path(
        get_eff_folder_from_stage(stage),
        get_eff_name_from_stage(stage, alt),
    )
}

pub fn collect_stage_alts() -> StageAlts {
    fix_order_specific_files(FilesystemInfo::instance_mut().unwrap().search_mut());

    let search = FilesystemInfo::instance().unwrap().search();
    let search_mut = FilesystemInfo::instance_mut().unwrap().search_mut();

    let stage = walk_search_section(search, Hash40::from("stage"), 1);

    let mut missing_files: HashMap<Hash40String, Vec<NewFile>> = HashMap::new();
    let mut alts = StageAlts::new();

    let arc = ArcFile::open("rom:/data.arc").unwrap();

    for child in stage {
        let SearchEntry::Folder { index, .. } = child else { continue; };

        let path = &search.get_path_list()[index as usize];
        if path.file_name.hash40() == Hash40::from("common") {
            continue;
        }

        if let Some(normal) = get_direct_child(search, index, Hash40::from("normal")) {
            let mut counter = 1;
            while let Some(alt) =
                get_direct_child(search, index, hash40_fmt!("normal_s{:02}", counter))
            {
                let name = search.get_path_list()[index as usize].file_name.hash40();
                {
                    let parent_base = Folder {
                        full_path: Hash40String(search.get_path_list()[alt as usize].path.hash40()),
                        name: None,
                        parent: None,
                    };

                    let missing = diff_folders(
                        search,
                        &arc,
                        search.get_path_list()[normal as usize].path.hash40(),
                        parent_base.full_path.0,
                        parent_base,
                    );

                    // only do this if missing is empty, since otherwise we are restarting and
                    // this is a pointless step
                    if missing.is_empty() {
                        file_order_fix(
                            search_mut,
                            search.get_path_list()[normal as usize].path.hash40(),
                            search.get_path_list()[alt as usize].path.hash40(),
                        );
                    }

                    'outer: for (k, v) in missing {
                        if let Some(list) = missing_files.get_mut(&k) {
                            for file in list.iter() {
                                if file.full_path == v.full_path {
                                    continue 'outer;
                                }
                            }
                            list.push(v);
                        } else {
                            missing_files.insert(k, vec![v]);
                        }
                    }
                }

                let base_path = search.get_path_list()[normal as usize].path.hash40();
                let mut alt_list = collect_alts(search, base_path, alt, counter);

                let mut effect_alt = StageAltFolder::new(
                    counter as usize,
                    get_eff_folder_from_stage(name),
                    get_eff_folder_from_stage(name),
                );

                effect_alt.add_file(get_eff_name_from_stage(name, counter));

                alt_list.push(effect_alt);

                for alt in alt_list {
                    alts.add_alt(alt.base_path, alt);
                }

                counter += 1;
            }
            alts.set_available_normal(path.file_name.hash40(), counter);
        }

        if let Some(battle) = get_direct_child(search, index, Hash40::from("battle")) {
            let mut counter = 1;
            while let Some(alt) =
                get_direct_child(search, index, hash40_fmt!("battle_s{:02}", counter))
            {
                let name = search.get_path_list()[index as usize].file_name.hash40();
                {
                    let parent_base = Folder {
                        full_path: Hash40String(search.get_path_list()[alt as usize].path.hash40()),
                        name: None,
                        parent: None,
                    };

                    let missing = diff_folders(
                        search,
                        &arc,
                        search.get_path_list()[battle as usize].path.hash40(),
                        parent_base.full_path.0,
                        parent_base,
                    );

                    // only do this if missing is empty, since otherwise we are restarting and
                    // this is a pointless step
                    if missing.is_empty() {
                        file_order_fix(
                            search_mut,
                            search.get_path_list()[battle as usize].path.hash40(),
                            search.get_path_list()[alt as usize].path.hash40(),
                        );
                    }

                    'outer: for (k, v) in missing {
                        if let Some(list) = missing_files.get_mut(&k) {
                            for file in list.iter() {
                                if file.full_path == v.full_path {
                                    continue 'outer;
                                }
                            }
                            list.push(v);
                        } else {
                            missing_files.insert(k, vec![v]);
                        }
                    }
                }

                let base_path = search.get_path_list()[battle as usize].path.hash40();
                let mut alt_list = collect_alts(search, base_path, alt, counter);

                let mut effect_alt = StageAltFolder::new(
                    counter as usize,
                    get_eff_folder_from_stage(name),
                    get_eff_folder_from_stage(name),
                );

                effect_alt.add_file(get_eff_name_from_stage(name, counter));

                if alts
                    .get_alt(effect_alt.base_path, effect_alt.alt_no)
                    .is_none()
                {
                    alt_list.push(effect_alt);
                }

                for alt in alt_list {
                    alts.add_alt(alt.base_path, alt);
                }

                counter += 1;
            }
            alts.set_available_battle(path.file_name.hash40(), counter);
        }
    }

    let effect_dir = arc.get_dir_info_from_hash("effect/stage").unwrap();
    let children = &arc.file_system.folder_child_hashes[effect_dir.children_range()];

    for effect_child in children {
        let Ok(path) = search.get_path_list_entry_from_hash(effect_child.hash40()) else {
            continue;
        };

        let max = alts
            .max_normal(path.file_name.hash40())
            .max(alts.max_battle(path.file_name.hash40()));

        for counter in 1..max {
            let new_path = join_path(
                path.parent.hash40(),
                concat(path.file_name.hash40(), hash40_fmt!("_s{:02}", counter)),
            );
            let missing = diff_folders(
                search,
                &arc,
                path.path.hash40(),
                new_path,
                Folder {
                    full_path: Hash40String(new_path),
                    name: Some(Hash40String(concat(
                        path.file_name.hash40(),
                        hash40_fmt!("_s{:02}", counter),
                    ))),
                    parent: Some(Box::new(Folder {
                        full_path: Hash40String(path.parent.hash40()),
                        name: None,
                        parent: None,
                    })),
                },
            );

            if missing.is_empty() {
                file_order_fix(search_mut, path.path.hash40(), new_path);
            } else {
                println!("{:#x?}", missing);
            }

            if let Ok(index) = search.get_path_list_index_from_hash(new_path) {
                let alt_paths = collect_alts(search, path.path.hash40(), index, counter);
                for alt in alt_paths {
                    for file in alt.files() {
                        println!("{:#x}", alt.new_path(*file).0);
                    }
                    println!("----------");
                    alts.add_alt(alt.base_path, alt);
                }
            }

            'outer: for (k, v) in missing {
                if let Some(list) = missing_files.get_mut(&k) {
                    for file in list.iter() {
                        if file.full_path == v.full_path {
                            continue 'outer;
                        }
                    }
                    list.push(v);
                } else {
                    missing_files.insert(k, vec![v]);
                }
            }
        }
    }

    if !missing_files.is_empty() {
        let mut auto_cfg = config::AutoCfg::open().unwrap();

        for (k, v) in missing_files.into_iter() {
            for file in v {
                match auto_cfg.merge(k.0, file) {
                    MergeResult::AlreadyThere => {
                        skyline_web::DialogOk::ok(
                            "Please make sure that Stage-Alts-Auto-Cfg is enabled via ARCropolis",
                        );
                        unsafe {
                            skyline::nn::oe::RequestToRelaunchApplication();
                        }
                    }
                    MergeResult::Merged => {}
                }
            }
        }

        auto_cfg.save().unwrap();

        skyline_web::DialogOk::ok(
            "The stage alts auto config has been updated, please restart your game!",
        );
        unsafe {
            skyline::nn::oe::RequestToRelaunchApplication();
        }

        // let string = serde_json::to_string_pretty(&SimpleConfigJson { new_shared_files: missing_files }).unwrap();

        // std::fs::write("sd:/ultimate/mods/Stage-Alts-Auto-Cfg/config.json", string).unwrap();
    }

    // println!("{:#x?}", missing_files);

    alts
}
