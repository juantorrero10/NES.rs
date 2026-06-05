use std::{
    ffi::c_void,
    sync::atomic::{AtomicPtr, Ordering},
};

use raylib::ffi::SetAudioStreamCallback;
use ringbuf::traits::Consumer;

use super::*;

static NES_CTX: AtomicPtr<AppCtx> = AtomicPtr::new(std::ptr::null_mut());

pub fn nes_loop(mut ctx: Box<AppCtx>) -> Result<(), Box<dyn Error>> {
    let mut dissasembly = String::with_capacity(65536);
    let mut mem_page = [None; 256];
    let mut pfs = PersistentFrameState::default();
    let mut title = String::with_capacity(20);

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
    let mut frame_has_changed = true; // We may have new info so we fetch it from NES
    let mut new_frame_generated = true; // PPU has generated another frame, force App update

    let mut dev_mode = false;

    let mut mapper_info = String::with_capacity(512);
    let mut display_mapper = false;
    let mut toggle_mapper = false;

    let mut ppu_regs = String::with_capacity(128);

    let mut micros_since_last_frame;

    let mut t0 = Instant::now();

    unsafe {
        SetAudioStreamCallback(*ctx.ui.raylib_audio_stream(), Some(nes_audio_callback));
        NES_CTX.store(std::ptr::from_mut(ctx.as_mut()), Ordering::Release);
    }

    // It is a pointer+len pair because we can guarantee that this will be valid for ever.
    let (screen_ptr, screen_len) = ctx.nes.ppu.get_screen();

    log::info!("Starting main loop");

    while !ctx.ui.raylib_handle().window_should_close() {
        if ctx.is_running.load(Ordering::Acquire) {
            ctx.nes
                .controller
                .update_controller_status(ctx.ui.raylib_handle(), None);
        }

        if ctx.update_info.load(Ordering::Acquire) {
            pfs.mem_edit_page_changed = true;
            frame_has_changed = true;
            ctx.update_info.store(false, Ordering::Release);
            ctx.ui.update_game_screen(screen_ptr, screen_len);
        }

        #[cfg(feature = "log")]
        {
            if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_L) {
                ctx.nes.mapper.toggle_log();
                ctx.nes.ppu.toggle_log();
                ctx.nes.toggle_log();
                ctx.nes.cpu.toggle_log();
            }
        }

        if pfs.mem_edit_page_changed && dev_mode {
            save_page(pfs.mem_edit_current_page, &mut ctx.nes, &mut mem_page);
        }

        // if log && !log_string.is_empty() {
        //     print!("{}\n", log_string);
        //     log_string.clear();
        // }

        if frame_has_changed && dev_mode {
            if fps10_draw_ctrl == 6 && !toggle_mapper {
                get_ppu_info(
                    &mut ctx.nes,
                    selected_palette,
                    &mut chr_bank_0_canvas,
                    &mut chr_bank_1_canvas,
                    &mut palettes,
                );
                fps10_draw_ctrl = 0;
            }
            fps10_draw_ctrl += 1;
            let pc = Some(ctx.nes.cpu.get_pc());
            get_dissasembly(&mut ctx.nes, &mut ins_mapping, &mut dissasembly, pc);
            if toggle_mapper {
                mapper_info.clear();
                display_mapper = ctx.nes.mapper.debug_information(&mut mapper_info);
            } else {
                ppu_regs.clear();
                ctx.nes.ppu.debug_format_registers(&mut ppu_regs);
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
            cpu_info: (&(ctx.nes.cpu)).into(),
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

        if micros_since_last_frame >= FPS60_ELAPSED {
            handle_keys(&mut ctx, &mut step_instruction, &mut dev_mode, &mut selected_palette, &mut frame_has_changed, &mut fps10_draw_ctrl, &mut toggle_mapper)?;
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
            ctx.ui
                .raylib_handle()
                .set_window_title(ctx.ui.raylib_thread(), &title);

            fs = ctx.ui.update(fs);
            t0 = Instant::now();

            bkpt = fs.dissasembly.bkpt;
            if let Some((a, v)) = fs.memory_editor.modify_byte {
                ctx.nes.write(a, v)?;
            }
            pfs = PersistentFrameState::save_persistent_info(fs);
        } else {
            std::thread::yield_now();
        }

        // real_time = false;
        // log::info!("real_time = {}", real_time);
        step_instruction = false;
        frame_has_changed = false;
        new_frame_generated = false;
    }
    Ok(())
}

fn handle_keys(
    ctx: &mut AppCtx,
    step_instruction: &mut bool,
    dev_mode: &mut bool,
    selected_palette: &mut u16,
    frame_has_changed: &mut bool,
    fps10_draw_ctrl: &mut i32,
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
                *step_instruction = true;
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
    }
    if ctx
        .ui
        .raylib_handle()
        .is_key_pressed(KeyboardKey::KEY_COMMA)
    {
        *selected_palette += 1;
        *selected_palette &= 0x7;
        *frame_has_changed = true;
        *fps10_draw_ctrl = 6;
    }

    if ctx.ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_M) {
        *toggle_mapper ^= true;
    }
    Ok(())
}

pub const AUDIO_BUFFER_SIZE: usize = 727;
pub const FPS60_ELAPSED: u128 = 16667;
pub const EMU_SAMPLE_RATE: u32 = 43653;

unsafe extern "C" fn nes_audio_callback(buffer: *mut c_void, sample_count: u32) {
    let ctx_ptr = NES_CTX.load(Ordering::Acquire);
    let sample_count = sample_count as usize;
    if ctx_ptr.is_null() {
        unsafe {
            std::ptr::write_bytes(buffer as *mut f32, 0, sample_count);
            return;
        }
    }

    let out_buffer = unsafe { std::slice::from_raw_parts_mut(buffer as *mut f32, sample_count) };

    let ctx = unsafe { ctx_ptr.as_mut().unwrap() };

    if !ctx.is_running.load(Ordering::Acquire) {
        unsafe {
            std::ptr::write_bytes(buffer as *mut f32, 0, sample_count);
            return;
        }
    }

    let mut n_sample = 0;
    let mut timeout = 0;
    while n_sample < sample_count && timeout < sample_count * 10 {
        ctx.nes.clock(None).expect("Failure during nes clock");

        if let Some(sample) = ctx.audio_consumer.try_pop() {
            out_buffer[n_sample] = sample;
            n_sample += 1;
            timeout = 0;
        } else {
            timeout += 1;
        }
        // Update at the exact moment of a new frame to avoid frame tearing
        if ctx.nes.ppu.frame_complete {
            ctx.nes.ppu.frame_complete = false;
            ctx.update_info.store(true, Ordering::Release)
        }

        // Breakpoint reached
        if let Some(b) = ctx.breakpoint
            && ctx.nes.cpu.get_pc() == b
        {
            ctx.is_running.store(false, Ordering::Release);
        }
    }
}

/*
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
    ctx.ui: &mut AppUI,
    break_point: Option<u16>,
) -> Result<LoopBroken, Box<dyn Error>> {
    if unsafe { !IsAudioStreamProcessed(*ctx.ui.raylib_audio_stream()) } {
        return Ok(LoopBroken::PollingFailed);
    }
    // A new audio sample is needed
    nes.controller
        .update_controller_status(ctx.ui.raylib_handle(), None);

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
            *ctx.ui.raylib_audio_stream(),
            audio_buffer.as_ptr() as *const _,
            AUDIO_BUFFER_SIZE as i32,
        );
    }
    return Ok(LoopBroken::AudioBufferPopulated);
}*/
