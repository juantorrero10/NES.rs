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

fn nes_loop(mut nes: Box<Nes>, mut ui: AppUI) -> Result<(), Box<dyn Error>> {
    let mut dissasembly = String::with_capacity(65536);
    let mut mem_page = [None; 256];
    let mut pfs = PersistentFrameState::default();
    let mut title = String::with_capacity(20);
    let mut sim = RunMode::Stopped;

    // Dissasembly
    let mut ins_mapping: HashMap<u16, (usize, usize, u16)> = HashMap::with_capacity(65536);
    let mut bkpt: Option<u16> = None;

    // PPU info upload data
    let mut chr_bank_0_canvas = [0u8; 128 * 128 * 4];
    let mut chr_bank_1_canvas = [0u8; 128 * 128 * 4];
    let mut selected_palette = 0;
    let mut palettes = [[[0u8; 4]; 4]; 8];
    let mut fps10_draw_ctrl = 0;

    // log
    let mut log = false;
    let mut log_string = String::with_capacity(128);

    let mut step_instruction = true;
    let mut frame_updated = true;
    let mut step_frame = false;

    let mut dev_mode = false;

    // let mut audio = vec![0f32; 2048];
    // let mut phase = 0.0f32;

    // It is a pointer+len pair because we can guarantee that this will be valid for ever.
    let (screen_ptr, screen_len) = nes.ppu.get_screen();

    while !ui.raylib_handle().window_should_close() {
        let t0 = Instant::now();

        match sim {
            RunMode::Sim => {
                step_frame = true;

                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_TAB) {
                    sim = RunMode::Stopped;
                }
            }
            RunMode::Stopped => {
                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_DOWN) {
                    //log::info!("Pressed S!");
                    step_instruction = true;
                }

                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_P) {
                    step_frame = true;
                } else if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_TAB) {
                    sim = RunMode::Sim;
                }
            }
        }

        if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_R) {
            nes.reset()?;
        }

        // Pressing V to jump a frame or Simulation Speed
        if step_frame {
            nes.controller
                .update_controller_status(ui.raylib_handle(), None);

            if nes.step_frame(bkpt)? {
                sim = RunMode::Stopped;
            }
            frame_updated = true;
            pfs.mem_edit_page_changed = true;
            // Stepping just one by pressing S
        } else if step_instruction {
            // log::info!("a");
            let mut c = 0;
            let mut started_next = nes.cpu.state.cycles_left > 0;
            loop {
                //log::info!("a CL{}", nes.cpu.state.cycles_left);
                if log {
                    nes.clock(Some(&mut log_string))?;
                } else {
                    nes.clock(None)?;
                }
                c += 1;
                // dbg!(nes.cpu.state.cycles_left, started_next);
                if !started_next && nes.cpu.state.cycles_left > 0 {
                    started_next = true;
                }

                if c >= 16 {
                    break;
                }

                if started_next && nes.cpu.state.cycles_left == 0 {
                    break;
                }
            }
            pfs.mem_edit_page_changed = true;
            frame_updated = true;
            fps10_draw_ctrl = 6;
        }

        if ui
            .raylib_handle()
            .is_key_pressed(raylib::ffi::KeyboardKey::KEY_F)
        {
            dev_mode ^= true;
        }

        if pfs.mem_edit_page_changed && dev_mode {
            save_page(
                pfs.mem_edit_current_page,
                &mut nes,
                &mut mem_page,
            );
        }

        // if log && !log_string.is_empty() {
        //     print!("{}\n", log_string);
        //     log_string.clear();
        // }

        if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_P) {
            selected_palette += 1;
            selected_palette &= 0x3;
            frame_updated = true;
            fps10_draw_ctrl = 6;
        }

        if frame_updated && dev_mode {
            if fps10_draw_ctrl == 6 {
                get_ppu_info(
                    &mut nes,
                    selected_palette,
                    &mut chr_bank_0_canvas,
                    &mut chr_bank_1_canvas,
                    &mut palettes,
                );
                fps10_draw_ctrl = 0;
            }
            fps10_draw_ctrl += 1;
            let pc = Some(nes.cpu.get_pc());
            get_dissasembly(
                &mut nes,
                &mut ins_mapping,
                &mut dissasembly,
                pc,
            );
        }

        let mut fs = FrameState {
            dev_mode,
            screen: GameScreen {
                update: frame_updated,
                video_output: (screen_ptr, screen_len),
            },
            memory_editor: MemEdit {
                paged_changed: false,
                current_page: pfs.mem_edit_current_page,
                search_byte_highlight: pfs.mem_edit_search_byte_highlight,
                page_data: &mem_page,
                modify_byte: None,
            },
            cpu_info: (&(nes.cpu)).into(),
            dissasembly: Dissassebly {
                update: step_instruction,
                text: &dissasembly,
                mapping: &ins_mapping,
                bkpt,
            },
            // Uploading the daat every frame
            game_audio: Apu {
                // data: audio.as_slice(),
            },
            ppu_info: PpuInfo {
                chr_bank_0: chr_bank_0_canvas.as_slice(),
                chr_bank_1: chr_bank_1_canvas.as_slice(),
                palettes,
                selected_palette,
            },
        };
        fs = ui.update(fs);
        bkpt = fs.dissasembly.bkpt;
        if let Some((a, v)) = fs.memory_editor.modify_byte {
            nes.write(a, v)?;
        }
        pfs = PersistentFrameState::save_persistent_info(fs);

        step_frame = false;
        step_instruction = false;
        frame_updated = false;

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
            match nes_loop(n, app_init()) {
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
