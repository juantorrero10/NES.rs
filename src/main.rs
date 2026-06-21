#![windows_subsystem = "windows"]

use std::{
    ffi::OsString, path::Path, sync::{Arc, atomic::AtomicBool}
};

use colored::Colorize;
use ringbuf::{SharedRb, storage::Heap, traits::Split, wrap::caching::Caching};
use simple_logger::SimpleLogger;

use crate::{
    nes::{Nes, NesError},
    ui::{
        AppUI, Theme,
    },
};

pub mod utils;
pub mod ui;
mod emu_loop;
mod nes;

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 240;
pub const SCREEN_SIZE: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;
pub const AUDIO_BUFFER_SIZE: usize = 733;
pub const FRAME_ELAPSED: u128 = 16_639_267;
pub const MAX_SLEEP_TIME: u128 = 10_000_000;
pub const EMU_SAMPLE_RATE: u32 = 44100;


fn nes_init() -> Result<Box<emu_loop::AppCtx>, NesError> {
    let mut args = std::env::args();
    while let Some(s) = args.next()
        && s.starts_with("--")
    {}

    let rom_path = match args.next() {
        Some(f) => OsString::from(&f),
        _ => match utils::load_rom_dialog() {
            Some(s) => s,
            None => std::process::exit(1),
        },
    };
    let audio: SharedRb<ringbuf::storage::Heap<f32>> = SharedRb::new(AUDIO_BUFFER_SIZE);
    let (tx, rx) = audio.split();
    let screen_title = utils::get_title(&rom_path)?;
    let mut nes_ctx = Box::new(emu_loop::AppCtx {
        ui: app_init(),
        nes: Nes::new(&rom_path, tx)?,
        audio_consumer: rx,
        is_running: AtomicBool::new(false),
        update_info: AtomicBool::new(false),
        step_instruction: AtomicBool::new(false),
        breakpoint: None,
    });
    nes_ctx.ui.raylib_handle().set_window_title(nes_ctx.ui.raylib_thread(), &screen_title);
    nes_ctx.nes.try_load_save_file(None)?;
    nes_ctx.nes.reset()?;
    Ok(nes_ctx)
}

fn app_init() -> AppUI {
    let mut light_mode = false;
    for s in std::env::args() {
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
    #[cfg(target_os = "windows")]
    utils::attach_console();

    std::env::set_current_dir(utils::get_correct_cwd()).expect("Could not set the correct CWD");

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
            match emu_loop::nes_loop(n) {
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
}
