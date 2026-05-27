use std::{
    collections::HashMap,
    error::Error,
    fmt::Write,
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use colored::Colorize;
use log::info;
use raylib::ffi::KeyboardKey;
use simple_logger::SimpleLogger;

use crate::{
    nes::{Nes, bus_cpu::BusCpu, instruction_set::Instruction6502},
    ui::{
        AppUI, Theme,
        framestate::{Dissassebly, FrameState, GameScreen, MemEdit, PersistentFrameState},
    },
};

mod nes;
mod ui;

fn dump_and_abort(nes: Nes, e: Box<dyn Error>) -> Box<dyn Error> {
    log::warn!("Failure while clocking CPU, shuting down!");
    println!(
        "{}:\n{:?}",
        ">>>CPU state dump on failure<<<".bright_black(),
        nes.cpu
    );
    println!(
        "{}:\n{:?}",
        ">>>BUS state dump on failure<<<".bright_black(),
        nes.bus
    );
    return e;
}

const FPS60_ELAPSED: u128 = 16667;

enum RunMode {
    WaitRelease,
    CanStep,
    Sim,
}

fn get_dissasembly(
    bus: &mut BusCpu,
    hm: &mut HashMap<u16, (usize, usize, u16)>,
    buff: &mut String,
) {
    buff.clear();
    hm.clear();
    let mut old_len = 0usize;
    let mut new_len = 0usize;
    let mut jump: u16;
    let mut i = 0u32;
    while i <= 0xFFFF {
        let addr = i as u16;
        old_len = buff.len();
        match Instruction6502::dissasemble(addr, bus, buff) {
            Ok(v) => {
                i += v as u32;
                jump = v as u16;
            }
            Err(e) => {
                buff.truncate(old_len);
                i += 1;
                continue;
            }
        }
        new_len = buff.len();
        hm.insert(addr as u16, (old_len, new_len, jump));
    }
}

fn save_page(page_num: u8, bus: &mut BusCpu, out_buff: &mut [Option<u8>]) {
    for j in 0..256 {
        out_buff[j] = bus.read((page_num as u16) << 8 | j as u16).ok()
    }
}

fn nes_loop(mut nes: Nes, mut ui: AppUI) -> Result<(), Box<dyn Error>> {
    let mut dissasembly = String::with_capacity(65536);
    let mut mem_page = [None; 256];
    let mut pfs = PersistentFrameState::default();
    let mut title = String::with_capacity(20);
    let mut sim = RunMode::CanStep;
    let mut ins_mapping: HashMap<u16, (usize, usize, u16)> = HashMap::with_capacity(65536);
    let mut step = true;
    let mut frame_updated = true;

    while !ui.raylib_handle().window_should_close() {
        let t0 = Instant::now();

        match sim {
            RunMode::Sim => {
                step = true;
                frame_updated = true;

                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_SPACE) {
                    sim = RunMode::CanStep;
                }
            }
            RunMode::CanStep => {
                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_S) {
                    sim = RunMode::WaitRelease;
                    step = true;
                    frame_updated = true;
                }
                else if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_SPACE) {
                    sim = RunMode::Sim;
                }
            }
            RunMode::WaitRelease => {
                if ui.raylib_handle().is_key_released(KeyboardKey::KEY_S) {
                    sim = RunMode::CanStep;
                }
            }
        }

        if step {
            loop {
                if let Err(e) = nes.cpu.clock() {
                    return Err(dump_and_abort(nes, Box::new(e)));
                };
                if nes.cpu.state.cycles_left == 0 {
                    break;
                }
            }
            get_dissasembly(
                &mut nes.bus.borrow_mut(),
                &mut ins_mapping,
                &mut dissasembly,
            );
            pfs.mem_edit_page_changed = true;
        }

        if pfs.mem_edit_page_changed {
            save_page(
                pfs.mem_edit_current_page,
                &mut nes.bus.borrow_mut(),
                &mut mem_page,
            );
        }
        let mut fs = FrameState {
            screen: GameScreen {
                update: frame_updated,
            },
            memory_editor: MemEdit {
                paged_changed: false,
                current_page: pfs.mem_edit_current_page,
                search_byte_highlight: pfs.mem_edit_search_byte_highlight,
                page_data: &mem_page,
            },
            cpu_info: (&(nes.cpu)).into(),
            dissasembly: Dissassebly {
                update: step,
                text: &dissasembly,
                mapping: &ins_mapping,
            },
        };
        step = false;
        frame_updated = false;
        ui.update(&mut fs);

        pfs = PersistentFrameState::save_persistent_info(fs);
        let dt = t0.elapsed().as_micros();
        // V-Sync logic
        if dt < FPS60_ELAPSED {
            sleep(Duration::from_micros((FPS60_ELAPSED - dt) as u64));
        }
        title.clear();
        let version_tag;
        version_tag = match cfg!(debug_assertions) {
            true => "Debug",
            false => "Release",
        };
        let _ = write!(
            title,
            "ShittyEmu.rs [{version_tag}] (FPS:{:.2})",
            (1000000.0 / (std::cmp::max(dt, FPS60_ELAPSED) as f32))
        );
        ui.raylib_handle()
            .set_window_title(ui.raylib_thread(), &title);
    }
    Ok(())
}

fn init() -> Result<(Nes, AppUI), Box<dyn Error>> {
    let mut nes = Nes::new();
    /*
        *=$8000
        LDX #10
        STX $0000
        LDX #3
        STX $0001
        LDY $0000
        LDA #0
        CLC
        loop
        ADC $0001
        DEY
        BNE loop
        STA $0002
        NOP
        NOP
        NOP
    */
    nes.bus.borrow_mut().debug_load_prg(&[
        0xA2, 0x0A, 0x8E, 0x00, 0x00, 0xA2, 0x03, 0x8E, 0x01, 0x00, 0xAC, 0x00, 0x00, 0xA9, 0x00,
        0x18, 0x6D, 0x01, 0x00, 0x88, 0xD0, 0xFA, 0x8D, 0x02, 0x00, 0xA2, 0x21, 0x8E, 0x00, 0x80,
        0xEA,
    ]);
    nes.cpu.reset()?;

    let app_ui =
        if std::env::args().len() > 1 && std::env::args().skip(1).next().unwrap() == "--light" {
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
    Ok((nes, app_ui))
}

fn main() {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .with_timestamp_format(time::macros::format_description!(
            "[[[year]-[month]-[day] [hour]:[minute]:[second]]"
        ))
        .with_colors(true)
        .init()
        .expect("Failure: Logger failed");

    match init() {
        Err(e) => eprintln!(
            "{} {}: {}",
            "Start-up".blue().italic(),
            "NES Emulator panicked!".red().bold(),
            e
        ),
        Ok((n, ui)) => {
            log::info!("Emulator initialized.");
            match nes_loop(n, ui) {
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
