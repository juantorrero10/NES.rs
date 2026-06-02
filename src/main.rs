//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//use tracy_client::ProfiledAllocator;

//#[global_allocator]
//static GLOBAL: ProfiledAllocator<std::alloc::System> = ProfiledAllocator::new(std::alloc::System, 0);

use std::{
    collections::HashMap,
    error::Error,
    ffi::OsString,
    fmt::Write,
    thread::sleep,
    time::{Duration, Instant},
};

use colored::Colorize;
use log::info;
use raylib::ffi::KeyboardKey;
use simple_logger::SimpleLogger;

use crate::{
    nes::{Nes, NesError, bus_cpu::BusCpu, instruction_set::Instruction6502},
    ui::{
        AppUI, Theme,
        framestate::{
            Apu, Dissassebly, FrameState, GameScreen, MemEdit, PersistentFrameState, PpuInfo,
        },
    },
};

mod nes;
mod ui;
mod main_loop;

const FPS60_ELAPSED: u128 = 16667;

enum RunMode {
    Stopped,
    Sim,
}

fn get_dissasembly(
    nes: &mut Nes,
    hm: &mut HashMap<u16, (usize, usize, u16)>,
    buff: &mut String,
    force_syncronize: Option<u16>,
) {
    buff.clear();
    hm.clear();
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

fn save_page(page_num: u8, nes: &mut Nes, out_buff: &mut [Option<u8>]) {
    for j in 0..256 {
        out_buff[j] = nes.debug_read((page_num as u16) << 8 | j as u16).ok()
    }
}

fn get_ppu_info(
    nes: &mut Nes,
    selected_palette: u16,
    out_chr0: &mut [u8; 128 * 128 * 4],
    out_chr1: &mut [u8; 128 * 128 * 4],
    out_pal: &mut [[[u8; 4]; 4]; 8],
) {
    let nes_ptr = std::ptr::from_mut(nes);
    nes.ppu.debug_extract_sprite_canvas(0, selected_palette, None, nes_ptr, out_chr0);
    nes.ppu.debug_extract_sprite_canvas(1, selected_palette, None, nes_ptr, out_chr1);
    nes.ppu.debug_extract_palettes(&mut *out_pal, nes_ptr);
}

// CRASH JUST AS I ENTER
fn nes_init() -> Result<Box<Nes>, NesError> {
    let mut args = std::env::args();
    while let Some(s) = args.next() && s.starts_with("--") {}

    let rom_path = match args.next() {
        Some(f) => OsString::from(&f),
        _ => {
            let file = rfd::FileDialog::new()
                .add_filter("NES ROM", &["nes", "rom"])
                .add_filter("any file", &["*"])
                .pick_file();
            match file {
                Some(p) => p.as_os_str().to_owned(),
                None => {
                    panic!("a");
                }
            }
        }
    };
    let mut nes = Box::new(Nes::new(rom_path)?);
    nes.reset();
    Ok(nes)
}

fn app_init() -> AppUI {
    let mut light_mode = false;
    for (i, s) in std::env::args().enumerate() {
        if s == "--light" {
            light_mode = true;
        }
    }

    let app_ui = if light_mode {
        AppUI::with_theme(Theme {
            bg: [0.95, 0.95, 0.9, 1.0],
            darker_bg: [0.73, 0.73, 0.7, 1.0f32],
            game_screen_bg: [0.95, 0.95, 0.9, 1.0],
            label: [0.35, 0.35, 0.35, 1.0],
            title: [0.25, 0.25, 0.25, 1.0],
            signal_low: [0.8, 0.17, 0.17, 1.0],
            signal_high: [0.17, 0.8, 0.17, 1.0],
            highlight: [0.15, 0.65, 0.67, 0.9],
            highlight2: [0.98, 0.902, 0.337, 0.9],
            value: [0.0, 0.0, 0.0, 1.0],
            button: [0.369, 0.361, 0.431, 1.0],
        })
    } else {
        AppUI::new()
    };
    app_ui

}

fn main() {
    dbg!(size_of::<Nes>());
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .with_timestamp_format(time::macros::format_description!(
            "[[[year]-[month]-[day] [hour]:[minute]:[second]]"
        ))
        .with_colors(true)
        .init()
        .expect("Failure: Logger failed");

    match nes_init() {
        Err(e) => eprintln!(
            "{} {}: {}",
            "Start-up".blue().italic(),
            "NES Emulator panicked!".red().bold(),
            e
        ),
        Ok(n) => {
            log::info!("Emulator initialized.");
            match main_loop::nes_loop(n, app_init()) {
                Err(e) => println!(
                    "{} {}: {}",
                    "Runtime".green().italic(),
                    "NES Emulator panicked!".red().bold(),
                    e
                ),
                Ok(_) => {
                    return ();
                }
            }
        }
    }
}
