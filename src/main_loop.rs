use std::thread;

use raylib::{
    RaylibHandle,
    ffi::{IsAudioStreamProcessed, UpdateAudioStream},
};
use ringbuf::{
    SharedRb,
    traits::{Consumer, Observer},
};

use crate::main_loop::LoopBroken::PollingFailed;

use super::*;

pub fn nes_loop(
    mut nes: Box<Nes>,
    mut audio_consumer: AudioConsumer,
    mut ui: AppUI,
) -> Result<(), Box<dyn Error>> {
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

    let mut step_instruction = true; // Step through next instruction
    let mut real_time = false; // Poll audio every iteration.
    let mut frame_has_changed = true; // We may have new info so we fetch it from NES
    let mut new_frame_generated = true; // PPU has generated another frame, force App update

    let mut resume_audio_at = 0;

    let mut audio_buffer = [0f32; AUDIO_BUFFER_SIZE];

    let mut dev_mode = false;

    let mut mapper_info = String::with_capacity(512);
    let mut display_mapper = false;
    let mut toggle_mapper = false;

    let mut ppu_regs = String::with_capacity(128);

    let mut micros_since_last_frame;
    let mut micros_since_last_game_frame: u128 = 0;

    let mut t0 = Instant::now();
    let mut t0_game = Instant::now();

    // It is a pointer+len pair because we can guarantee that this will be valid for ever.
    let (screen_ptr, screen_len) = nes.ppu.get_screen();

    while !ui.raylib_handle().window_should_close() {
        match sim {
            RunMode::Sim => {
                real_time = true;
                // log::info!("real_time = {}", real_time);

                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_TAB) {
                    sim = RunMode::Stopped;
                }
            }
            RunMode::Stopped => {
                real_time = false;
                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_DOWN) {
                    step_instruction = true;
                }

                if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_TAB) {
                    real_time = true;
                    sim = RunMode::Sim;
                }
            }
        }

        if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_R) {
            nes.reset()?;
        }

        // Pressing V to jump a frame or Simulation Speed
        if real_time {
            match poll_real_time(
                &mut nes,
                &mut audio_consumer,
                audio_buffer.as_mut_slice(),
                resume_audio_at,
                &mut ui,
                bkpt,
            )? {
                // Just wait until next iteration
                PollingFailed => {}
                LoopBroken::AudioBufferPopulated => {
                    frame_has_changed = true;
                    pfs.mem_edit_page_changed = true;
                    resume_audio_at = 0;
                }
                LoopBroken::NewFrameGenerated(r) => {
                    resume_audio_at = r;
                    frame_has_changed = true;
                    pfs.mem_edit_page_changed = true;
                    new_frame_generated = true;
                    // log::info!("Ppu has new frame");
                }
                LoopBroken::Breakpoint => {
                    sim = RunMode::Stopped;
                    pfs.mem_edit_page_changed = true;
                    frame_has_changed = true;
                }
            }
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
            frame_has_changed = true;
            fps10_draw_ctrl = 6;
        }

        if ui
            .raylib_handle()
            .is_key_pressed(raylib::ffi::KeyboardKey::KEY_F11)
        {
            dev_mode ^= true;
        }

        #[cfg(feature = "log")]
        {
            if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_L) {
                nes.mapper.toggle_log();
                nes.ppu.toggle_log();
                nes.toggle_log();
                nes.cpu.toggle_log();
            }
        }

        if pfs.mem_edit_page_changed && dev_mode {
            save_page(pfs.mem_edit_current_page, &mut nes, &mut mem_page);
        }

        // if log && !log_string.is_empty() {
        //     print!("{}\n", log_string);
        //     log_string.clear();
        // }

        if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_COMMA) {
            selected_palette += 1;
            selected_palette &= 0x7;
            frame_has_changed = true;
            fps10_draw_ctrl = 6;
        }

        if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_M) {
            toggle_mapper ^= true;
        }

        if frame_has_changed && dev_mode {
            if fps10_draw_ctrl == 6 && !toggle_mapper {
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
            get_dissasembly(&mut nes, &mut ins_mapping, &mut dissasembly, pc);
            if toggle_mapper {
                mapper_info.clear();
                display_mapper = nes.mapper.debug_information(&mut mapper_info);
            } else {
                ppu_regs.clear();
                nes.ppu.debug_format_registers(&mut ppu_regs);
            }
        }

        let mut fs = FrameState {
            dev_mode,
            save_signal: false,
            load_signal: false,
            screen: GameScreen {
                update: new_frame_generated,
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
                display_mapper_info: display_mapper,
                mapper_info: &mapper_info,
                ppu_regs: &ppu_regs,
                toggle_mapper,
            },
        };

        micros_since_last_frame = t0.elapsed().as_micros();
        // micros_since_last_game_frame = t0_game.elapsed().as_micros();

        // log::info!("Time last frame: {}", micros_since_last_frame as f64 / 1_000_000.0);

        /*if !real_time {
            if micros_since_last_frame < FPS60_ELAPSED {
                sleep(Duration::from_micros((micros_since_last_frame - FPS60_ELAPSED) as u64));
            }
        }*/

        if  micros_since_last_frame >= FPS60_ELAPSED || new_frame_generated {
            // log::info!("New frame");
            let us = micros_since_last_frame as f64;
            // if false {
            //     log::info!(
            //         "FPS: {}, Delay: {} ms, New frame? {}",
            //         1_000_000.0 / us,
            //         us / 1000.0,
            //         new_frame_generated
            //     );
            // }
            title.clear();
            let version_tag;
            version_tag = match cfg!(debug_assertions) {
                true => "Debug",
                false => "Release",
            };
            let _ = write!(
                title,
                "ShittyEmu.rs [{version_tag}] (FPS:{:.2})",
                (1_000_000.0 / (us))
            );
            // log::info!("FPS: {}, Delay: {} ms, New frame? {}", 1_000_000.0 / us, us / 1000.0, new_frame_generated);
            ui.raylib_handle()
                .set_window_title(ui.raylib_thread(), &title);

            fs = ui.update(fs);
            t0 = Instant::now();
            if new_frame_generated {
                // thread::sleep(Duration::from_micros(FPS60_ELAPSED as u64/ 2))
            }
        }

        bkpt = fs.dissasembly.bkpt;
        if let Some((a, v)) = fs.memory_editor.modify_byte {
            nes.write(a, v)?;
        }

        pfs = PersistentFrameState::save_persistent_info(fs);

        // real_time = false;
        // log::info!("real_time = {}", real_time);
        step_instruction = false;
        frame_has_changed = false;
        new_frame_generated = false;
    }
    Ok(())
}

pub const AUDIO_BUFFER_SIZE: usize = 800;

enum LoopBroken {
    PollingFailed,
    AudioBufferPopulated,
    NewFrameGenerated(usize),
    Breakpoint,
}

fn poll_real_time(
    nes: &mut Nes,
    audio_consumer: &mut AudioConsumer,
    audio_buffer: &mut [f32],
    resume_audio_at: usize,
    ui: &mut AppUI,
    break_point: Option<u16>,
) -> Result<LoopBroken, Box<dyn Error>> {
    if unsafe { !IsAudioStreamProcessed(*ui.raylib_audio_stream()) } {
        return Ok(LoopBroken::PollingFailed);
    }
    // A new audio sample is needed
    nes.controller
        .update_controller_status(ui.raylib_handle(), None);

    let mut n_sample = resume_audio_at;
    while n_sample < AUDIO_BUFFER_SIZE {
        nes.clock(None)?;

        if let Some(sample) = audio_consumer.try_pop() {
            audio_buffer[n_sample] = sample;
            n_sample += 1;
        }
        // Exit at the exact moment of a new frame to avoid frame tearing
        // We need to return the last position of the buffer as well.
        if nes.ppu.frame_complete {
            nes.ppu.frame_complete = false;
            return Ok(LoopBroken::NewFrameGenerated(n_sample));
        }

        // Breakpoint reached
        if let Some(b) = break_point
            && nes.cpu.get_pc() == b
        {
            return Ok(LoopBroken::Breakpoint);
        }
    }

    unsafe {
        UpdateAudioStream(
            *ui.raylib_audio_stream(),
            audio_buffer.as_ptr() as *const _,
            AUDIO_BUFFER_SIZE as i32,
        );
    }
    return Ok(LoopBroken::AudioBufferPopulated);
}
