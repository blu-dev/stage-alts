use std::{collections::HashMap, path::Path};

use once_cell::sync::Lazy;
use prc::ParamKind;
use rlua_lua53_sys as lua;
use smash_arc::{Hash40, ArcLookup, FilePathIdx};

use parking_lot::Mutex;

use crate::{STAGE_ALT_LOOKUP, types::FilesystemInfo};

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

static PANEL_TO_HASH_LOOKUP2: Lazy<Mutex<HashMap<usize, Hash40>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub static UI_FILEPATH_INDICES: Lazy<Vec<u32>> = Lazy::new(|| {
    let hashes = UI_TO_HASH_LOOKUP
        .iter()
        .map(|(_, place)| *place);

    let info = FilesystemInfo::instance().unwrap();
    let arc = info.arc();
    let mut indices = vec![];
    for hash in hashes {
        // normal
        let mut counter = 1;
        while let Ok(index) = arc.get_file_path_index_from_hash(format_path_for_ui(hash, 0, counter)) {
            indices.push(index.0);
            counter += 1;
        }
        // battle
        let mut counter = 1;
        while let Ok(index) = arc.get_file_path_index_from_hash(format_path_for_ui(hash, 2, counter)) {
            indices.push(index.0);
            counter += 1;
        }
        // omega
        let mut counter = 1;
        while let Ok(index) = arc.get_file_path_index_from_hash(format_path_for_ui(hash, 1, counter)) {
            indices.push(index.0);
            counter += 1;
        }
    }

    indices
});

// pub static mut INCOMING_ALT_NO: usize = 0;
pub static mut INCOMING_ALTS: [usize; 3] = [0; 3];

extern "C" fn register_alt(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let mut alt_no = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as usize;
        lua::lua_pop(state, 1);

        let panel_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let preview_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let ui_hash = PANEL_TO_HASH_LOOKUP2.lock().get(&(panel_id as usize)).copied();
        println!("{:#x?}", ui_hash);
        if let Some(ui_hash) = ui_hash {
            if ui_hash == Hash40::from("ui_stage_random")
            || ui_hash == Hash40::from("ui_stage_random_normal") {
                alt_no = usize::MAX;
            } else if ui_hash == Hash40::from("ui_stage_random_battle")
            || ui_hash == Hash40::from("ui_stage_random_end") {
                alt_no = usize::MAX - 1;
            }
        }

        INCOMING_ALTS[preview_id as usize] = alt_no;

        0
    }
}

extern "C" fn get_next_alt(state: *mut lua::lua_State) -> i32 {
    unsafe {
        let alt_no = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let mut stage_form = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);
        let panel_id = lua::lua_tointegerx(state, -1, std::ptr::null_mut()) as i32;
        lua::lua_pop(state, 1);

        let panel_id = panel_id as usize;

        println!("alt: {}, form: {}, panel: {}", alt_no, stage_form, panel_id);

        let panel_lookup = PANEL_TO_HASH_LOOKUP2.lock();

        let stage_hash = panel_lookup
            .get(&panel_id)
            .and_then(|ui_hash| UI_TO_HASH_LOOKUP.get(ui_hash))
            .copied();

        if let Some(mut hash) = stage_hash {
            if hash == Hash40::from("stage/end") {
                stage_form = 0;
            }

            if (hash == Hash40::from("stage/battlefield_s") || hash == Hash40::from("stage/battlefield_l"))
            && stage_form != 0 {
                hash = Hash40::from("stage/battlefield");
                stage_form = 0;
            }
            if (stage_form == 0 && STAGE_ALT_LOOKUP.is_available_normal(hash, 1 + alt_no as usize))
            || (stage_form != 0 && STAGE_ALT_LOOKUP.is_available_battle(hash, 1 + alt_no as usize)) {
                lua::lua_pushinteger(state, 1 + alt_no as i64);
            } else {
                lua::lua_pushinteger(state, 0);
            }
        } else {
            lua::lua_pushinteger(state, -1);
        }

        1
    }
}

fn format_path_for_ui(stage: Hash40, form: usize, alt: usize) -> Hash40 {
    let dlc = &[
        Hash40::from("battlefields"),
        Hash40::from("brave_altar"),
        Hash40::from("buddy_spiral"),
        Hash40::from("demon_dojo"),
        Hash40::from("dolly_stadium"),
        Hash40::from("fe_shrine"),
        Hash40::from("ff_cave"),
        Hash40::from("jack_mementoes"),
        Hash40::from("pickel_world"),
        Hash40::from("tantan_spring"),
        Hash40::from("xeno_alst"),
        Hash40::from("trail_castle"),
    ];

    let stage = if stage == Hash40::from("battlefield_l") {
        Hash40::from("battlefieldl")
    } else if stage == Hash40::from("battlefield_s") {
        Hash40::from("battlefields")
    } else {
        stage
    };

    let ui_folder = if !dlc.contains(&stage) {
        match form {
            0 => "ui/replace/stage/stage_2/stage_2_",
            1 => "ui/replace/stage/stage_4/stage_4_",
            2 => "ui/replace/stage/stage_3/stage_3_",
            _ => return Hash40::from("")
        }
    } else {
        match form {
            0 => "ui/replace_patch/stage/stage_2/stage_2_",
            1 => "ui/replace_patch/stage/stage_4/stage_4_",
            2 => "ui/replace_patch/stage/stage_3/stage_3_",
            _ => return Hash40::from("")
        }
    };

    let suffix = if alt == 0 {
        ".bntx".into()
    } else {
        format!("_s{:02}.bntx", alt)
    };


    let full_path = smash::phx::Hash40::new(ui_folder)
        .concat(smash::phx::Hash40::new_raw(stage.0))
        .concat(smash::phx::Hash40::new(suffix));

    Hash40(full_path.as_u64())
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

        let stage_hash = panel_lookup
            .get(&panel_id)
            .and_then(|ui_hash| UI_TO_HASH_LOOKUP.get(ui_hash))
            .copied();

        let index = if let Some(mut hash) = stage_hash {
            if (hash == Hash40::from("battlefield_s") || hash == Hash40::from("battlefield_l"))
            && stage_form != 0 {
                hash = Hash40::from("battlefield");
            }
            arc
                .get_file_path_index_from_hash(format_path_for_ui(hash, stage_form as _, alt_no as _))
                .or_else(|_| arc.get_file_path_index_from_hash(format_path_for_ui(hash, stage_form as _, 0)))
                .unwrap_or(FilePathIdx(0xFF_FFFF))
        } else {
            FilePathIdx(0xFF_FFFF)
        };

        lua::lua_pushinteger(state, index.0 as _);

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
        let info = FilesystemInfo::instance().unwrap();
        for index in UI_FILEPATH_INDICES.iter() {
            super::refc(info, *index);
        }
        0
    }
}

extern "C" fn off_load(_state: *mut lua::lua_State) -> i32 {
    unsafe {
        let info = FilesystemInfo::instance().unwrap();
        for index in UI_FILEPATH_INDICES.iter() {
            super::unrefc(info, *index);
        }
        super::CURRENT_STAGE_INDEX = usize::MAX;
        0
    }
}

unsafe fn push_new_singleton(lua_state: *mut lua::lua_State, name: &'static str, registry: &[lua::luaL_Reg]) {
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
            func: Some(get_next_alt)
        },
        lua::luaL_Reg {
            name: "send_message\0".as_ptr() as _,
            func: Some(send_message)
        },
        lua::luaL_Reg {
            name: "register_alt\0".as_ptr() as _,
            func: Some(register_alt)
        },
        lua::luaL_Reg {
            name: "get_index_for_texture\0".as_ptr() as _,
            func: Some(get_index_for_texture)
        },
        lua::luaL_Reg {
            name: "on_load\0".as_ptr() as _,
            func: Some(on_load)
        },
        lua::luaL_Reg {
            name: "off_load\0".as_ptr() as _,
            func: Some(off_load)
        },
        lua::luaL_Reg {
            name: std::ptr::null(),
            func: None
        }
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
    params: [f32; 4]
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
    unsafe {
        let _ = skyline::patching::nop_data(0x1d34e4c);
    }
    skyline::install_hooks!(
        add_to_key_context,
        replace_texture,
        is_valid_entrance_param
    );
}