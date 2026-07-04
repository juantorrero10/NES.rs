use std::{collections::HashMap, error::Error, ffi::OsString, path::{Path, PathBuf}, sync::atomic::Ordering};

use raylib::ffi::KeyboardKey;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

use crate::{emu_loop::AppCtx, nes::{Nes, NesError, instruction_set::Instruction6502}, ui::framestate::MemPage};


pub fn load_rom_dialog() -> Option<OsString> {
    let file = rfd::FileDialog::new()
        .add_filter("NES ROM", &["nes", "rom"])
        .add_filter("any file", &["*"])
        .pick_file();
    match file {
        Some(p) => Some(p.as_os_str().to_owned()),
        None => None,
    }
}

pub fn load_sram_dialog() -> Option<OsString> {
    let file = rfd::FileDialog::new()
        .add_filter("NES Game SRAM Snapshot", &["sram"])
        .add_filter("any file", &["*"])
        .set_directory("savefiles")
        .pick_file();
    match file {
        Some(p) => Some(p.as_os_str().to_owned()),
        None => None,
    }
}

pub fn store_emu_snapshot_dialog() -> Option<OsString> {
    let file = rfd::FileDialog::new()
        .add_filter("NES Emulator Snapshot", &["snap"])
        .add_filter("any file", &["*"])
        .set_directory("savefiles")
        .save_file();
    match file {
        Some(p) => Some(p.as_os_str().to_owned()),
        None => None,
    }
}

pub fn load_emu_snapshot_dialog() -> Option<OsString> {
    let file = rfd::FileDialog::new()
        .add_filter("NES Emulator Snapshot", &["snap"])
        .add_filter("any file", &["*"])
        .set_directory("savefiles")
        .pick_file();
    match file {
        Some(p) => Some(p.as_os_str().to_owned()),
        None => None,
    }
}

#[cfg(target_os = "windows")]
pub fn attach_console() {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            let _ = std::io::stdout();
            let _ = std::io::stderr();
        }
    }
}

pub fn get_correct_cwd() -> PathBuf {
    // 1. Check if we are running through Cargo
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        return PathBuf::from(manifest_dir);
    }

    // 2. Fallback for production: Anchor strictly to where the .exe file sits
    let mut exe_path = std::env::current_exe().expect("Failed to get executable directory");
    exe_path.pop(); // Remove the executable name to get the directory
    exe_path
}

pub fn get_dissasembly(
    nes: &mut Nes,
    hm: &mut HashMap<u16, (usize, usize, u16)>,
    buff: &mut String,
    force_syncronize: Option<u16>,
) {
    let mut old_len;
    let mut new_len;
    let mut jump: u16;
    let mut i = 0u32;
    while i <= 0xFFFF {
        let addr = i as u16;
        old_len = buff.len();
        match Instruction6502::dissasemble(addr, nes, buff) {
            Ok(v) => {
                jump = v as u16;
                if let Some(a) = force_syncronize {
                    if addr < a && addr + v as u16 > a {
                        i = a as u32;
                        new_len = buff.len();
                        hm.insert(addr as u16, (old_len, new_len, (a - addr)));
                        continue;
                    }
                }
                i += v as u32;
            }
            Err(_) => {
                buff.truncate(old_len);
                i += 1;
                continue;
            }
        }
        new_len = buff.len();
        hm.insert(addr as u16, (old_len, new_len, jump));
    }
}

pub fn get_mem_page(page: MemPage, nes: &mut Nes, out_buff: &mut [Option<u8>]) {
    let page_iter = (0..256).into_iter();
    match page {
        MemPage::Cpu(p) => {
            page_iter.for_each(|i| out_buff[i as usize] = nes.debug_read((p as u16) << 8 | i).ok());
        },
        MemPage::Ppu(p) => {
            page_iter.for_each(|i| out_buff[i as usize] = Some(nes.ppu_debug_read((p as u16) << 8 | i)));
        },
    };
}

pub fn get_ppu_info(
    nes: &mut Nes,
    selected_palette: u16,
    out_chr0: &mut [u8; 128 * 128 * 4],
    out_chr1: &mut [u8; 128 * 128 * 4],
    out_pal: &mut [[[u8; 4]; 4]; 8],
) {
    let nes_ptr = std::ptr::from_mut(nes);
    nes.ppu
        .debug_extract_sprite_canvas(0, selected_palette, None, nes_ptr, out_chr0);
    nes.ppu
        .debug_extract_sprite_canvas(1, selected_palette, None, nes_ptr, out_chr1);
    nes.ppu.debug_extract_palettes(&mut *out_pal, nes_ptr);
}

pub fn handle_keys_presses(
    ctx: &mut AppCtx,
    dev_mode: &mut bool,
    selected_palette: &mut u16,
    frame_has_changed: &mut bool,
    update_canvas: &mut bool,
    toggle_mapper: &mut bool,
) -> Result<(), Box<dyn Error>> {
    match ctx.is_running.load(Ordering::Acquire) {
        true => {
            // log::info!("real_time = {}", real_time);

            if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_TAB) {
                ctx.is_running.store(false, Ordering::Release);
            }
        }
        false => {
            if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_DOWN) {
                ctx.step_instruction.store(true, Ordering::Release);
            }

            if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_TAB) {
                ctx.is_running.store(true, Ordering::Release);
            }
        }
    }

    if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_R) {
        ctx.nes.reset()?;
    }

    if ctx
        .ui
        .raylib_handle()
        .is_key_pressed(raylib::ffi::KeyboardKey::KEY_F11)
    {
        *dev_mode ^= true;
        ctx.step_instruction.store(true, Ordering::Release);
    }
    if ctx
        .ui
        .raylib_handle()
        .is_key_pressed(KeyboardKey::KEY_COMMA)
    {
        *selected_palette += 1;
        *selected_palette %= 8;
        *frame_has_changed = true;
        *update_canvas = true;
    }

    if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_M) {
        *toggle_mapper ^= true;
    }
    Ok(())
}

#[inline]
pub fn get_title(path: &OsString) -> Result<String, NesError> {
    let file_name = Path::new(&path).file_name().ok_or_else(|| NesError::InvalidPath(path.clone()))?;
    Ok(format!("ShittyEmu: {:?}", file_name))
}