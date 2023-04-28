use std::{
    collections::{BTreeMap, HashMap},
    ops::{Deref, DerefMut},
    sync::Arc,
};

use log::{error, info};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use smash_arc::{ArcLookup, FilePath, FilePathIdx, Hash40, HashToIndex, SearchLookup};

use crate::search::{FlattenVec, SearchEntry, SearchEx};

use crate::{search::walk_search_section, types::FilesystemInfo, Hash40Ext};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Selection {
    Regular { name: Hash40, alt: usize },
    Random,
    Invalid,
}

impl std::fmt::Display for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regular { name, alt } => {
                write!(f, "{:#x} @ Alt #{}", name.0, alt)
            }
            Self::Random => {
                write!(f, "Random")
            }
            Self::Invalid => {
                write!(f, "Invalid")
            }
        }
    }
}

/// The main structure to represent information for a specific stage alt
pub struct StageAlt {
    /// The mapping of vanilla folder paths to the alt folder paths.
    ///
    /// An example of this would be for a user who has an alt on `s01` for Battlefield, you would be able to lookup the hash for
    /// `"stage/battlefield/normal/model"` and receive the hash for `"stage/battlefield/normal_s01/model"`.
    ///
    /// This is intended to be used in the replacement of the file list to load.
    pub alt_folders: HashMap<Hash40, Hash40>,

    /// The mapping of non-shared files to their base file link index
    /// This is used to unshare and reshare files at runtime to prevent weird, inconsistent issues (such as that with
    /// Final Heaven)
    pub sharing_base: HashMap<Hash40, (u32, u32)>,

    /// The paths to each of the `stage_x` UI paths. For DLC stages these are in `replace_patch` and for non-DLC these are in `replace`
    pub ui_paths: [Hash40; 5],

    pub is_normal_ws: bool,
    pub is_normal_ignore: bool,
    pub is_battle_ws: bool,
    pub is_battle_ignore: bool,
}

pub struct StageAltInfo {
    pub stage_name: Hash40,
    pub stage_folder: Hash40,
    pub normal_folder: Hash40,
    pub battle_folder: Hash40,
    pub alts_found: Vec<Arc<StageAlt>>,
}

pub struct StageAltManager {
    pub filepath_backup: BTreeMap<Hash40, u32>,
    pub path_backup: BTreeMap<Hash40, u32>,
    pub folder_backup: BTreeMap<Hash40, u32>,
    pub alt_infos: HashMap<Hash40, StageAltInfo>,
    pub alts: Vec<Arc<StageAlt>>,
    pub selection: Vec<Selection>,
    pub current_index: usize,
    pub current_alt: Option<Arc<StageAlt>>,
    pub is_online: bool,
}

impl StageAltManager {
    pub fn is_dlc_stage(stage_name: Hash40) -> bool {
        [
            Hash40::from("battlefield_s"),
            Hash40::from("brave_altar"),
            Hash40::from("buddy_spiral"),
            Hash40::from("demon_dojo"),
            Hash40::from("dolly_stadium"),
            Hash40::from("fe_shrine"),
            Hash40::from("ff_cave"),
            Hash40::from("jack_mementoes"),
            Hash40::from("pickel_world"),
            Hash40::from("tantan_spring"),
            Hash40::from("trail_castle"),
            Hash40::from("xeno_alst"),
        ]
        .contains(&stage_name)
    }

    // fn get_backup_path_index(&self, hash: Hash40) -> Option<u32> {
    //     self.path_lookup_backup
    //         .binary_search_by_key(&hash, |index| index.hash40())
    //         .ok()
    //         .map(|index| index as u32)
    // }

    fn vanilla_normal_ui_path(stage_name: Hash40) -> Hash40 {
        let replace_folder = if Self::is_dlc_stage(stage_name) {
            Hash40::from("replace_patch")
        } else {
            Hash40::from("replace")
        };

        let stage_name = if stage_name == Hash40::from("battlefield_s") {
            Hash40::from("battlefields")
        } else if stage_name == Hash40::from("battlefield_l") {
            Hash40::from("battlefieldl")
        } else {
            stage_name
        };

        let file_name = Hash40::from("stage_2_").concat(stage_name).concat(".bntx");

        Hash40::from("ui")
            .join_path(replace_folder)
            .join_path("stage")
            .join_path("stage_2")
            .join_path(file_name)
    }

    fn vanilla_battle_ui_path(stage_name: Hash40) -> Hash40 {
        let replace_folder =
            if Self::is_dlc_stage(stage_name) && stage_name != Hash40::from("battlefield_s") {
                Hash40::from("replace_patch")
            } else {
                Hash40::from("replace")
            };

        let stage_name = if stage_name == Hash40::from("battlefield_s")
            || stage_name == Hash40::from("battlefield_l")
        {
            Hash40::from("battlefield")
        } else {
            stage_name
        };

        let file_name = Hash40::from("stage_4_").concat(stage_name).concat(".bntx");

        Hash40::from("ui")
            .join_path(replace_folder)
            .join_path("stage")
            .join_path("stage_4")
            .join_path(file_name)
    }

    fn vanilla_end_ui_path(stage_name: Hash40) -> Hash40 {
        let replace_folder =
            if Self::is_dlc_stage(stage_name) && stage_name != Hash40::from("battlefield_s") {
                Hash40::from("replace_patch")
            } else {
                Hash40::from("replace")
            };

        let stage_name = if stage_name == Hash40::from("battlefield_s")
            || stage_name == Hash40::from("battlefield_l")
        {
            Hash40::from("battlefield")
        } else {
            stage_name
        };

        let file_name = Hash40::from("stage_3_").concat(stage_name).concat(".bntx");

        Hash40::from("ui")
            .join_path(replace_folder)
            .join_path("stage")
            .join_path("stage_3")
            .join_path(file_name)
    }

    pub fn get_next_alt(&self, stage_name: Hash40, current_index: usize, is_battle: bool) -> usize {
        let Some(info) = self.alt_infos.get(&stage_name) else {
            info!("Stage {:#x} has no alts, resorting to default", stage_name.0);
            return 0;
        };

        if let Some(alt) = info.alts_found.get(current_index + 1) {
            if self.is_online
                && ((is_battle && !alt.is_battle_ws) || (!is_battle && !alt.is_normal_ws))
            {
                info!(
                    "Skipping alt {} for stage {:#x} because it is not wifi-safe",
                    current_index + 1,
                    stage_name.0
                );
                self.get_next_alt(stage_name, current_index + 1, is_battle)
            } else {
                info!(
                    "Stage {:#x} has alt {}, using that one",
                    stage_name.0,
                    current_index + 1
                );
                current_index + 1
            }
        } else {
            info!(
                "Stage {:#x} did not have alt {}, using default",
                stage_name.0,
                current_index + 1
            );
            0
        }
    }

    pub fn get_prev_alt(&self, stage_name: Hash40, current_index: usize, is_battle: bool) -> usize {
        let Some(info) = self.alt_infos.get(&stage_name) else {
            info!("Stage {:#x} has no alts, resorting to default", stage_name.0);
            return 0;
        };

        if let Some(alt) = info.alts_found.get(current_index - 1) {
            if self.is_online
                && ((is_battle && !alt.is_battle_ws) || (!is_battle && !alt.is_normal_ws))
            {
                info!(
                    "Skipping alt {} for stage {:#x} because it is not wifi-safe",
                    current_index - 1,
                    stage_name.0
                );
                self.get_prev_alt(stage_name, current_index - 1, is_battle)
            } else {
                info!(
                    "Stage {:#x} has alt {}, using that one",
                    stage_name.0,
                    current_index - 1
                );
                current_index - 1
            }
        } else {
            info!(
                "Stage {:#x} did not have alt {}, using default",
                stage_name.0,
                current_index - 1
            );
            self.get_prev_alt(stage_name, info.alts_found.len(), is_battle)
        }
    }

    pub fn get_random_alt(&self, stage_name: Hash40, is_battle: bool) -> usize {
        let Some(info) = self.alt_infos.get(&stage_name) else {
            info!("Stage {:#x} has no alts, so no random alt will be picked", stage_name.0);
            return 0;
        };

        if info.alts_found.is_empty() {
            error!("Stage {:#x} has no alts, despite having alt information. No random alt will be picked", stage_name.0);
            return 0;
        }

        let alt = loop {
            let alt = rand::random::<usize>() % info.alts_found.len();
            if is_battle {
                if self.is_online && !info.alts_found[alt].is_battle_ws {
                    info!(
                        "Skipping random alt {} for {:#x} because it is not wifi safe!",
                        alt, stage_name.0
                    );
                    continue;
                }

                if info.alts_found[alt].is_battle_ignore {
                    info!(
                        "Skipping random alt {} for {:#x} because it should be ignored!",
                        alt, stage_name.0
                    );
                    continue;
                }
            } else {
                if self.is_online && !info.alts_found[alt].is_normal_ws {
                    info!(
                        "Skipping random alt {} for {:#x} because it is not wifi safe!",
                        alt, stage_name.0
                    );
                    continue;
                }

                if info.alts_found[alt].is_normal_ignore {
                    info!(
                        "Skipping random alt {} for {:#x} because it should be ignored!",
                        alt, stage_name.0
                    );
                    continue;
                }
            }

            break alt;
        };

        info!("Using random alt for stage {:#x}: {}", stage_name.0, alt);
        alt
    }

    pub fn get_normal_ui_path(&self, stage_name: Hash40, alt: usize) -> Hash40 {
        if alt == 0 {
            let path = Self::vanilla_normal_ui_path(stage_name);
            info!(
                "Getting default UI path for {:#x} @ normal: {:#x}",
                stage_name.0, path.0
            );
            return path;
        }

        let Some(info) = self.alt_infos.get(&stage_name) else {
            let path = Self::vanilla_normal_ui_path(stage_name);
            error!("There is no stage alt information for {:#x}, using vanilla file path for normal: {:#x}", stage_name.0, path.0);
            return path;
        };

        if info.alts_found.is_empty() {
            let path = Self::vanilla_normal_ui_path(stage_name);
            error!("There are no stage alts for the stage {:#x}, using vanilla file path for normal: {:#x}", stage_name.0, path.0);
            return path;
        }

        if let Some(stage_alt) = info.alts_found.get(alt) {
            info!(
                "Getting alt #{} UI path for {:#x} @ normal: {:#x}",
                alt, stage_name.0, stage_alt.ui_paths[2].0
            );
            stage_alt.ui_paths[2]
        } else {
            let path = Self::vanilla_normal_ui_path(stage_name);
            error!(
                "There is no stage alt #{} for {:#x}, using vanilla file path for normal: {:#x}",
                alt, stage_name.0, path.0
            );
            path
        }
    }

    pub fn get_battle_ui_path(&self, stage_name: Hash40, alt: usize) -> Hash40 {
        if alt == 0 {
            let path = Self::vanilla_battle_ui_path(stage_name);
            info!(
                "Getting default UI path for {:#x} @ battle: {:#x}",
                stage_name.0, path.0
            );
            return path;
        }

        let Some(info) = self.alt_infos.get(&stage_name) else {
            let path = Self::vanilla_battle_ui_path(stage_name);
            error!("There is no stage alt information for {:#x}, using vanilla file path for battle: {:#x}", stage_name.0, path.0);
            return path;
        };

        if info.alts_found.is_empty() {
            let path = Self::vanilla_battle_ui_path(stage_name);
            error!("There are no stage alts for the stage {:#x}, using vanilla file path for battle: {:#x}", stage_name.0, path.0);
            return path;
        }

        if let Some(stage_alt) = info.alts_found.get(alt) {
            info!(
                "Getting alt #{} UI path for {:#x} @ battle: {:#x}",
                alt, stage_name.0, stage_alt.ui_paths[4].0
            );
            stage_alt.ui_paths[4]
        } else {
            let path = Self::vanilla_battle_ui_path(stage_name);
            error!(
                "There is no stage alt #{} for {:#x}, using vanilla file path for battle: {:#x}",
                alt, stage_name.0, path.0
            );
            path
        }
    }

    pub fn get_end_ui_path(&self, stage_name: Hash40, alt: usize) -> Hash40 {
        if alt == 0 {
            let path = Self::vanilla_end_ui_path(stage_name);
            info!(
                "Getting default UI path for {:#x} @ end: {:#x}",
                stage_name.0, path.0
            );
            return path;
        }

        let Some(info) = self.alt_infos.get(&stage_name) else {
            let path = Self::vanilla_end_ui_path(stage_name);
            error!("There is no stage alt information for {:#x}, using vanilla file path for end: {:#x}", stage_name.0, path.0);
            return path;
        };

        if info.alts_found.is_empty() {
            let path = Self::vanilla_end_ui_path(stage_name);
            error!("There are no stage alts for the stage {:#x}, using vanilla file path for end: {:#x}", stage_name.0, path.0);
            return path;
        }

        if let Some(stage_alt) = info.alts_found.get(alt) {
            info!(
                "Getting alt #{} UI path for {:#x} @ end: {:#x}",
                alt, stage_name.0, stage_alt.ui_paths[3].0
            );
            stage_alt.ui_paths[3]
        } else {
            let path = Self::vanilla_end_ui_path(stage_name);
            error!(
                "There is no stage alt #{} for {:#x}, using vanilla file path for end: {:#x}",
                alt, stage_name.0, path.0
            );
            path
        }
    }

    pub fn set_stage_use_count(&mut self, count: usize) {
        info!("Setting selection count to {}", count);
        self.selection = vec![Selection::Invalid; count];
    }

    pub fn set_stage_selection(&mut self, index: usize, selection: Selection) {
        info!("Setting stage selection #{} to {}", index, selection);
        if index >= self.selection.len() {
            error!("Provided index is greater than the set stage use count!");
            return;
        }

        self.selection[index] = selection;
    }

    fn change_alt(&mut self, new_alt: Option<Arc<StageAlt>>) {
        // let arc = FilesystemInfo::instance().unwrap().arc();
        // if let Some(alt) = self.current_alt.take() {
        //     for (hash, (base, _)) in alt.sharing_base.iter() {
        //         let filepath_index = arc.get_file_path_index_from_hash(*hash).unwrap();
        //         unsafe {
        //             (*(arc.file_paths as *mut FilePath).add(usize::from(filepath_index)))
        //                 .path
        //                 .set_index(*base);
        //         }
        //     }
        // }
        self.unhack_lookups_for_alt();
        self.current_alt = new_alt;
        self.hack_lookups_for_alt();
    }

    pub fn advance_alt(&mut self, incoming: Hash40, is_battle: bool) {
        info!("Advancing the alt to the next selection");
        let sel = if self.selection.is_empty() {
            info!("The selection list is empty, a random alt will be selected!");
            Selection::Random
        } else if incoming == Hash40::from("resultstage") {
            info!("The result stage is incoming, selecting a random alt!");
            Selection::Random
        } else if self.current_index == usize::MAX {
            self.current_index = 0;
            self.selection[self.current_index]
        } else {
            self.current_index = (self.current_index + 1) % self.selection.len();
            self.selection[self.current_index]
        };

        match sel {
            Selection::Invalid => {
                error!(
                    "Invalid selection encountered when advancing alt with incoming {:#x}",
                    incoming.0
                );
                self.change_alt(None);
            }
            Selection::Random => {
                let alt_id = self.get_random_alt(incoming, is_battle);
                info!(
                    "Randomly selected alt id {} for stage {:#x}",
                    alt_id, incoming.0
                );
                if alt_id == 0 {
                    info!("Since the alt id is 0, there will be no alt");
                    self.change_alt(None);
                    return;
                }

                self.change_alt(
                    self.alt_infos
                        .get(&incoming)
                        .and_then(|info| info.alts_found.get(alt_id))
                        .cloned(),
                );

                if self.current_alt.is_none() {
                    error!("Unable to use alt {} for stage {:#x}", alt_id, incoming.0);
                }
            }
            Selection::Regular { name, alt } => {
                if name != incoming {
                    error!(
                        "The incoming stage {:#x} did not match the reserved stage name {:#x}",
                        incoming.0, name.0
                    );
                }

                info!("Selecting alt {} for stage {:#x}", alt, name.0);
                if alt == 0 {
                    info!("Since the alt id is 0, there will be no alt");
                    self.change_alt(None);
                }

                self.change_alt(
                    self.alt_infos
                        .get(&name)
                        .and_then(|info| info.alts_found.get(alt))
                        .cloned(),
                );

                if self.current_alt.is_none() {
                    error!("Unable to use alt {} for stage {:#x}", alt, name.0);
                }
            }
        }
    }

    pub fn does_current_alt_have_folder(&self, folder: Hash40) -> bool {
        let Some(alt) = self.current_alt.clone() else {
            return false;
        };

        info!("Loading folder {:#x}", folder.0);

        alt.alt_folders.contains_key(&folder)
    }

    pub fn get_sharing_base_for_alt_folder(
        &self,
        folder: Hash40,
    ) -> Option<&HashMap<Hash40, (u32, u32)>> {
        self.current_alt.as_ref().map(|alt| &alt.sharing_base)
    }

    pub fn get_files_for_alt_folder(&self, folder: Hash40) -> Option<Vec<FilePathIdx>> {
        let alt = self.current_alt.clone()?;

        let base_folder = folder;
        let Some(folder) = alt.alt_folders.get(&folder).copied() else {
            error!("Could not get the folder {:#x} for the current alt!", folder.0);
            return None;
        };

        let instance = FilesystemInfo::instance().unwrap();
        let search = instance.search();
        let search_mut = FilesystemInfo::instance_mut().unwrap().search_mut();
        let arc = instance.arc();

        if search.get_folder_path_entry_from_hash(folder).is_err() {
            error!(
                "Could not find the folder path entry for folder {:#x}",
                folder.0
            );
            return None;
        }

        let children = walk_search_section(search, folder, 1);
        let files = children.flatten();

        let files = files
            .into_iter()
            .filter_map(|file| {
                let SearchEntry::File(index) = file else {
                    error!("Folder encountered in flattened children of {:#x}", folder.0);
                    return None;
                };

                let path = search.get_path_list()[index as usize];

                if path.ext == Hash40::from("flag") {
                    return None;
                }

                match arc.get_file_path_index_from_hash(path.path.hash40()) {
                    Ok(index) => {
                        info!(
                            "Retrieving file {:#x} with index {:#x}",
                            path.path.hash40().0,
                            index.0
                        );
                        Some(index)
                    }
                    Err(_) => {
                        error!("FilePathIdx for {:#x} was not found!", path.path.hash40().0);
                        None
                    }
                }
            })
            .collect();

        Some(files)
    }

    pub fn hack_lookups_for_alt(&self) {
        let folder_lookup = if let Some(alt) = self.current_alt.as_ref() {
            &alt.alt_folders
        } else {
            return;
        };

        let mut file_lookup = HashMap::new();

        let info = FilesystemInfo::instance_mut().unwrap();

        let search = info.search();

        for (base, modded) in folder_lookup.iter() {
            for file in walk_search_section(search, *base, 1).flatten() {
                let SearchEntry::File(index) = file else {
                    continue;
                };

                let name = search.get_path_list()[index as usize].file_name.hash40();

                file_lookup.insert(base.join_path(name), modded.join_path(name));
            }
        }

        let arc = info.arc_mut();

        for (base, modded) in file_lookup.iter() {
            let Ok(file_path_index) = arc.get_file_path_index_from_hash(*modded) else {
                continue;
            };
            let bucket = arc.get_bucket_for_hash(*base);
            let index_index = bucket
                .binary_search_by_key(base, |index| index.hash40())
                .map(|index| unsafe {
                    (&bucket[index] as *const HashToIndex).offset_from(arc.file_hash_to_path_index)
                })
                .unwrap() as usize;

            unsafe {
                (*(arc.file_hash_to_path_index as *mut HashToIndex).add(index_index))
                    .set_index(file_path_index.0);
            }
        }

        let search_mut = info.search_mut();

        for (base, modded) in file_lookup.iter() {
            let Ok(index) = search_mut
                .get_path_index_from_hash(*modded)
                .map(|index| index.index()) else {
                    continue;
                };
            search_mut
                .get_path_index_from_hash_mut(dbg!(*base))
                .unwrap()
                .set_index(index);
        }
    }

    pub fn unhack_lookups_for_alt(&self) {
        let folder_lookup = if let Some(alt) = self.current_alt.as_ref() {
            &alt.alt_folders
        } else {
            return;
        };

        let mut file_lookup = HashMap::new();

        let info = FilesystemInfo::instance_mut().unwrap();

        let search = info.search();

        for (base, modded) in folder_lookup.iter() {
            for file in walk_search_section(search, *base, 1).flatten() {
                let SearchEntry::File(index) = file else {
                    continue;
                };

                let name = search.get_path_list()[index as usize].file_name.hash40();

                file_lookup.insert(base.join_path(name), modded.join_path(name));
            }
        }

        let arc = info.arc_mut();

        for (base, modded) in file_lookup.iter() {
            let file_path_index = self.filepath_backup.get(base).copied().unwrap();
            let bucket = arc.get_bucket_for_hash(*base);
            let index_index = bucket
                .binary_search_by_key(base, |index| index.hash40())
                .map(|index| unsafe {
                    (&bucket[index] as *const HashToIndex).offset_from(arc.file_hash_to_path_index)
                })
                .unwrap() as usize;

            unsafe {
                (*(arc.file_hash_to_path_index as *mut HashToIndex).add(index_index))
                    .set_index(file_path_index);
            }
        }

        let search = info.search_mut();

        for (base, modded) in file_lookup.iter() {
            search
                .get_path_index_from_hash_mut(*base)
                .unwrap()
                .set_index(self.path_backup.get(base).copied().unwrap());
        }
    }
}

pub static STAGE_ALT_MANAGER: Lazy<RwLock<StageAltManager>> = Lazy::new(|| {
    let search = FilesystemInfo::instance().unwrap().search();
    let arc = FilesystemInfo::instance().unwrap().arc();
    RwLock::new(StageAltManager {
        filepath_backup: BTreeMap::from_iter(
            arc.get_file_hash_to_path_index()
                .iter()
                .map(|index| (index.hash40(), index.index())),
        ),
        path_backup: BTreeMap::from_iter(
            search
                .get_path_to_index()
                .iter()
                .map(|index| (index.hash40(), index.index())),
        ),
        folder_backup: BTreeMap::from_iter(
            search
                .get_path_to_index()
                .iter()
                .map(|index| (index.hash40(), index.index())),
        ),
        alt_infos: HashMap::new(),
        alts: vec![],
        selection: vec![],
        current_index: usize::MAX,
        current_alt: None,
        is_online: false,
    })
});

pub fn get() -> impl Deref<Target = StageAltManager> {
    STAGE_ALT_MANAGER.read()
}

pub fn get_mut() -> impl DerefMut<Target = StageAltManager> {
    STAGE_ALT_MANAGER.write()
}
