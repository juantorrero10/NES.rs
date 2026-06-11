#![windows_subsystem = "windows"]

#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

use std::{
    collections::HashMap, error::Error, ffi::OsString, fmt::Write, path::PathBuf, sync::{Arc, atomic::AtomicBool}, time::Instant
};

use colored::Colorize;
use raylib::ffi::KeyboardKey;
use ringbuf::{SharedRb, storage::Heap, traits::Split, wrap::caching::Caching};
use simple_logger::SimpleLogger;

use crate::{
    main_loop::AUDIO_BUFFER_SIZE,
    nes::{Nes, NesError, instruction_set::Instruction6502},
    ui::{
        AppUI, Theme,
        framestate::{Dissassebly, FrameState, MemEdit, PersistentFrameState, PpuInfo},
    },
};

mod main_loop;
mod nes;
mod ui;

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
    nes.ppu
        .debug_extract_sprite_canvas(0, selected_palette, None, nes_ptr, out_chr0);
    nes.ppu
        .debug_extract_sprite_canvas(1, selected_palette, None, nes_ptr, out_chr1);
    nes.ppu.debug_extract_palettes(&mut *out_pal, nes_ptr);
}

type AudioProducer = Caching<Arc<SharedRb<Heap<f32>>>, true, false>;
type AudioConsumer = Caching<Arc<SharedRb<Heap<f32>>>, false, true>;

pub struct AppCtx {
    ui: AppUI,
    nes: Nes,
    audio_consumer: AudioConsumer,
    is_running: AtomicBool,
    update_info: AtomicBool,
    breakpoint: Option<u16>,
}

fn load_rom_dialog() -> Option<OsString> {
    let file = rfd::FileDialog::new()
        .add_filter("NES ROM", &["nes", "rom"])
        .add_filter("any file", &["*"])
        .pick_file();
    match file {
        Some(p) => Some(p.as_os_str().to_owned()),
        None => None,
    }
}

fn load_sram_dialog() -> Option<OsString> {
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

fn store_emu_snapshot_dialog() -> Option<OsString> {
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

fn load_emu_snapshot_dialog() -> Option<OsString> {
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

fn nes_init() -> Result<Box<AppCtx>, NesError> {
    let mut args = std::env::args();
    while let Some(s) = args.next()
        && s.starts_with("--")
    {}

    let rom_path = match args.next() {
        Some(f) => OsString::from(&f),
        _ => match load_rom_dialog() {
            Some(s) => s,
            None => std::process::exit(1),
        },
    };
    let audio: SharedRb<ringbuf::storage::Heap<f32>> = SharedRb::new(AUDIO_BUFFER_SIZE);
    let (tx, rx) = audio.split();
    let mut nes_ctx = Box::new(AppCtx {
        ui: app_init(),
        nes: Nes::new(&rom_path, tx)?,
        audio_consumer: rx,
        is_running: AtomicBool::new(false),
        update_info: AtomicBool::new(false),
        breakpoint: None,
    });
    nes_ctx.nes.try_load_save_file(None)?;
    nes_ctx.nes.reset()?;
    Ok(nes_ctx)
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

#[cfg(target_os = "windows")]
fn attach_console() {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            let _ = std::io::stdout();
            let _ = std::io::stderr();
        }
    }
}

fn get_correct_cwd() -> PathBuf {
    // 1. Check if we are running through Cargo
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        return PathBuf::from(manifest_dir);
    }

    // 2. Fallback for production: Anchor strictly to where the .exe file sits
    let mut exe_path = std::env::current_exe().expect("Failed to get executable directory");
    exe_path.pop(); // Remove the executable name to get the directory
    exe_path
}

fn main() {
    #[cfg(target_os = "windows")]
    attach_console();

    std::env::set_current_dir(get_correct_cwd()).expect("Could not set the correct CWD");

    SimpleLogger::new()
    .with_level(log::LevelFilter::Info)
    .with_timestamp_format(time::macros::format_description!(
        "[[[year]-[month]-[day] [hour]:[minute]:[second]]"
    ))
    .with_colors(true)
    .init()
    .expect("Failure: Logger failed");

    log::info!("CWD: {:?}", std::env::current_dir().unwrap());
    match nes_init() {
        Err(e) => eprintln!(
            "{} {}: {}",
            "Start-up".blue().italic(),
            "NES Emulator panicked!".red().bold(),
            e
        ),
        Ok(n) => {
            log::info!("Emulator initialized.");
            match main_loop::nes_loop(n) {
                Err(e) => {
                    println!(
                        "{} {}: {}",
                        "Runtime".green().italic(),
                        "NES Emulator panicked!".red().bold(),
                        e
                    )
                }
                Ok(_) => {
                    return;
                }
            }
        }
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}
