use std::{collections::HashMap, path::Path};

use log::{error, info};
use once_cell::sync::Lazy;
use prc::ParamKind;
use rlua_lua53_sys as lua;
use smash_arc::{ArcLookup, FilePathIdx, Hash40};

use parking_lot::Mutex;

use crate::{
    alts::{self, Selection, STAGE_ALT_MANAGER},
    types::FilesystemInfo,
};

pub static UI_TO_HASH_LOOKUP: Lazy<HashMap<Hash40, Hash40>> = Lazy::new(|| {
    let data = if Path::new("mods:/ui/param/database/ui_stage_db.prc").exists() {
        std::fs::read("mods:/ui/param/database/ui_stage_db.prc").unwrap()
    } else {
        std::fs::read("arc:/ui/param/database/ui_stage_db.prc").unwrap()
    };

    let mut reader = std::io::Cursor::new(data);

    let param_data = prc::read_stream(&mut reader).unwrap();

    let (_, main_list) = &param_data.0[0];

    let ParamKind::List(list) = main_list else { unreachable!() };

    let mut map = HashMap::new();

    for param in list.0.iter() {
        let ParamKind::Struct(param) = param else { continue; };

        let mut ui_stage_id = Hash40(u64::MAX);
        let mut stage_place_id = Hash40(u64::MAX);

        for (k, v) in param.0.iter() {
            let k = Hash40(k.0);
            if k == Hash40::from("ui_stage_id") {
                let ParamKind::Hash(v) = v else { continue; };

                ui_stage_id = Hash40(v.0);
            } else if k == Hash40::from("stage_place_id") {
                let ParamKind::Hash(v) = v else { continue; };

                stage_place_id = Hash40(v.0);
            }
        }

        map.insert(ui_stage_id, stage_place_id);
    }

    map
});

static PANEL_TO_HASH_LOOKUP2: Lazy<Mutex<HashMap<usize, Hash40>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// pub static mut INCOMING_ALT_NO: usize = 0;
pub static mut INCOMING_ALTS: [usize; 3] = [0; 3];

extern "C" fn register_alt(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let alt_no = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as usize;
        lua::lua_pop(state, 1);

        let panel_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let preview_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let ui_hash = PANEL_TO_HASH_LOOKUP2
            .lock()
            .get(&(panel_id as usize))
            .copied();

        let Some(ui_hash) = ui_hash else {
            error!("There is no UI hash for the panel id {:#x}", panel_id);
            return 0;
        };

        let mut mgr = alts::get_mut();

        if [
            Hash40::from("ui_stage_random"),
            Hash40::from("ui_stage_random_normal"),
            Hash40::from("ui_stage_random_battle"),
            Hash40::from("ui_stage_random_end"),
        ]
        .contains(&ui_hash)
        {
            info!(
                "Setting stage selection for preview id {} to random!",
                preview_id
            );
            mgr.set_stage_selection(preview_id as usize, Selection::Random);
        } else if let Some(stage_name) = UI_TO_HASH_LOOKUP.get(&ui_hash) {
            info!(
                "Setting stage selection for preview id {} to {:#x} @ {}!",
                preview_id, stage_name.0, alt_no
            );
            mgr.set_stage_selection(
                preview_id as usize,
                Selection::Regular {
                    name: *stage_name,
                    alt: alt_no,
                },
            );
        } else {
            error!(
                "Unable to get the stage name from the UI hash: {:#x}",
                ui_hash.0
            );
        }

        0
    }
}

extern "C" fn get_next_alt(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let stage_form = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let alt_no = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let panel_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let panel_id = panel_id as usize;

        let panel_lookup = PANEL_TO_HASH_LOOKUP2.lock();

        let Some(ui_hash) = panel_lookup.get(&panel_id).copied() else {
            error!("Failed to get the UI hash from panel id {:#x}", panel_id);
            lua::lua_pushinteger(state, 0);
            return 1;
        };

        let Some(stage_hash) = UI_TO_HASH_LOOKUP.get(&ui_hash).copied() else {
            error!("Failed to get the stage name from the UI hash {:#x}", ui_hash.0);
            lua::lua_pushinteger(state, 0);
            return 1;
        };

        let mgr = alts::get();

        info!("Getting the next alt for {:#x} @ {}", stage_hash.0, alt_no);
        let next = mgr.get_next_alt(stage_hash, alt_no as usize, stage_form != 0);

        lua::lua_pushinteger(state, next as i64);

        1
    }
}

extern "C" fn get_prev_alt(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let stage_form = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let alt_no = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let panel_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let panel_id = panel_id as usize;

        let panel_lookup = PANEL_TO_HASH_LOOKUP2.lock();

        let Some(ui_hash) = panel_lookup.get(&panel_id).copied() else {
            error!("Failed to get the UI hash from panel id {:#x}", panel_id);
            lua::lua_pushinteger(state, 0);
            return 1;
        };

        let Some(stage_hash) = UI_TO_HASH_LOOKUP.get(&ui_hash).copied() else {
            error!("Failed to get the stage name from the UI hash {:#x}", ui_hash.0);
            lua::lua_pushinteger(state, 0);
            return 1;
        };

        let mgr = alts::get();

        info!("Getting the prev alt for {:#x} @ {}", stage_hash.0, alt_no);
        let next = mgr.get_prev_alt(stage_hash, alt_no as usize, stage_form != 0);

        lua::lua_pushinteger(state, next as i64);

        1
    }
}

extern "C" fn get_index_for_texture(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let alt_no = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let stage_form = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let panel_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let panel_id = panel_id as usize;

        let arc = FilesystemInfo::instance().unwrap().arc();
        let panel_lookup = PANEL_TO_HASH_LOOKUP2.lock();

        let default_index = arc
            .get_file_path_index_from_hash(Hash40::from(
                "ui/replace/chara/chara_1/chara_1_wario_04.bntx",
            ))
            .unwrap();

        let Some(ui_hash) = panel_lookup.get(&panel_id).copied() else {
            error!("Failed to get UI hash for panel id {}", panel_id);
            lua::lua_pushinteger(state, default_index.0 as i64);
            return 1;
        };

        let Some(stage_name) = UI_TO_HASH_LOOKUP.get(&ui_hash).copied() else {
            error!("Failed to get stage name from UI hash {:#x}", ui_hash.0);
            lua::lua_pushinteger(state, default_index.0 as i64);
            return 1;
        };

        let mgr = alts::get();

        let path_hash = match stage_form {
            0 => mgr.get_normal_ui_path(stage_name, alt_no as usize),
            1 => mgr.get_battle_ui_path(stage_name, alt_no as usize),
            2 => mgr.get_end_ui_path(stage_name, alt_no as usize),
            form => {
                error!(
                    "Stage form {} is invalid in this context, unable to get UI index",
                    form
                );
                lua::lua_pushinteger(state, default_index.0 as i64);
                return 1;
            }
        };

        let path_index = arc.get_file_path_index_from_hash(path_hash).map_or_else(
            |_| {
                error!(
                    "Failed to get file path index from hash UI path hash {:#x}",
                    path_hash.0
                );
                default_index
            },
            |index| {
                info!(
                    "Using file path index {:#x} for {:#x} @ alt#{} + form#{}",
                    index.0, stage_name.0, alt_no, stage_form
                );
                index
            },
        );

        lua::lua_pushinteger(state, path_index.0 as i64);
        1
    }
}

extern "C" fn send_message(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let value = skyline::from_c_str(lua::lua_tostring(state, -1) as _);
        println!("{}", value);
        lua::lua_pop(state, 1);
        0
    }
}

extern "C" fn on_load(_state: *mut lua::lua_State) -> i32 {
    unsafe {
        // let info = FilesystemInfo::instance().unwrap();
        // for index in UI_FILEPATH_INDICES.iter() {
        //     super::refc(info, *index);
        // }
        let mut mgr = alts::get_mut();
        mgr.current_index = 0;
        0
    }
}

extern "C" fn off_load(_state: *mut lua::lua_State) -> i32 {
    unsafe {
        // let info = FilesystemInfo::instance().unwrap();
        // for index in UI_FILEPATH_INDICES.iter() {
        //     super::unrefc(info, *index);
        // }
        let mgr = alts::get_mut();
        super::CURRENT_STAGE_INDEX = usize::MAX;
        0
    }
}

extern "C" fn set_stage_use_num(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let use_num = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        alts::get_mut().set_stage_use_count(use_num as _);
        0
    }
}

unsafe fn push_new_singleton(
    lua_state: *mut lua::lua_State,
    name: &'static str,
    registry: &[lua::luaL_Reg],
) {
    let real_name = format!("{}\0", name);
    let meta_name = format!("Metatable{}\0", name);
    lua::luaL_newmetatable(lua_state, meta_name.as_ptr() as _);
    lua::lua_pushvalue(lua_state, -1);
    lua::lua_setfield(lua_state, -2, "__index\0".as_ptr() as _);

    lua::luaL_setfuncs(lua_state, registry.as_ptr(), 0);
    lua::lua_pop(lua_state, 1);

    lua::lua_newtable(lua_state);
    lua::lua_getfield(lua_state, lua::LUA_REGISTRYINDEX, meta_name.as_ptr() as _);
    lua::lua_setmetatable(lua_state, -2);

    let global_table = lua::bindings::index2addr(lua_state, lua::LUA_REGISTRYINDEX);
    let table = (*global_table).value.ptr;
    let value = if *(table as *mut u32).add(3) < 2 {
        todo!()
    } else {
        (*(table as *mut *mut lua::bindings::TValue).add(2)).add(1)
    };
    lua::bindings::auxsetstr(lua_state, value, real_name.as_ptr() as _);
}

#[skyline::hook(offset = 0x3373048, inline)]
unsafe fn add_to_key_context(ctx: &skyline::hooks::InlineCtx) {
    let lua_state: *mut lua::lua_State = *ctx.registers[19].x.as_ref() as _;
    let registry = &[
        lua::luaL_Reg {
            name: "get_next_alt\0".as_ptr() as _,
            func: Some(get_next_alt),
        },
        lua::luaL_Reg {
            name: "get_prev_alt\0".as_ptr() as _,
            func: Some(get_prev_alt),
        },
        lua::luaL_Reg {
            name: "send_message\0".as_ptr() as _,
            func: Some(send_message),
        },
        lua::luaL_Reg {
            name: "register_alt\0".as_ptr() as _,
            func: Some(register_alt),
        },
        lua::luaL_Reg {
            name: "get_index_for_texture\0".as_ptr() as _,
            func: Some(get_index_for_texture),
        },
        lua::luaL_Reg {
            name: "on_load\0".as_ptr() as _,
            func: Some(on_load),
        },
        lua::luaL_Reg {
            name: "off_load\0".as_ptr() as _,
            func: Some(off_load),
        },
        lua::luaL_Reg {
            name: "set_stage_use_num\0".as_ptr() as _,
            func: Some(set_stage_use_num),
        },
        lua::luaL_Reg {
            name: std::ptr::null(),
            func: None,
        },
    ];
    push_new_singleton(lua_state, "StageAltManager", registry);
}

#[skyline::hook(offset = 0x33590a0)]
unsafe fn replace_texture(state: *mut lua::lua_State) -> i32 {
    if dbg!(lua::lua_isinteger(state, -1)) == 1 {
        let index = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        lua::lua_pushlightuserdata(state, &index as *const i32 as _);
        call_original!(state)
    } else {
        call_original!(state)
    }
}

#[repr(C)]
struct StageEntry {
    key: u64,
    params: [f32; 4],
}

#[skyline::hook(offset = 0x1b31ca0)]
unsafe fn is_valid_entrance_param(arg: u64, arg2: i32) -> bool {
    let vec = &mut *((arg + 0x168) as *mut smash::cpp::Vector<StageEntry>);

    let mut map = PANEL_TO_HASH_LOOKUP2.lock();

    for (idx, entry) in vec.iter().enumerate() {
        map.insert(idx, Hash40(entry.key & 0xFF_FFFF_FFFF));
    }

    call_original!(arg, arg2)
}

pub fn install() {
    // unsafe {
    //     let _ = skyline::patching::nop_data(0x1d34e4c);
    // }
    skyline::install_hooks!(add_to_key_context, replace_texture, is_valid_entrance_param);
}
