use std::{
    error::Error,
    os::raw::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
    time::{Duration, Instant},
};

use raylib::ffi::SetAudioStreamCallback;
use ringbuf::{SharedRb, storage::Heap, traits::Consumer, wrap::caching::Caching};

use crate::{
    FRAME_ELAPSED, MAX_SLEEP_TIME,
    nes::{Nes, NesError},
    ui::{AppUI, framestate::{FileAction, FrameState}},
    utils,
};

pub type AudioProducer = Caching<Arc<SharedRb<Heap<f32>>>, true, false>;
pub type AudioConsumer = Caching<Arc<SharedRb<Heap<f32>>>, false, true>;

pub struct AppCtx {
    pub ui: AppUI,
    pub nes: Nes,
    pub audio_consumer: AudioConsumer,
    pub step_instruction: AtomicBool,
    pub is_running: AtomicBool,
    pub update_info: AtomicBool,
    pub breakpoint: Option<u16>,
}

static NES_CTX: AtomicPtr<AppCtx> = AtomicPtr::new(std::ptr::null_mut());

pub fn nes_loop(mut ctx: Box<AppCtx>) -> Result<(), Box<dyn Error>> {
    let mut state = FrameState::new_boxed();

    let mut frame_count = 0;

    let mut last_frame_drawn = Instant::now();

    unsafe {
        SetAudioStreamCallback(*ctx.ui.raylib_audio_stream(), Some(nes_audio_callback));
        NES_CTX.store(std::ptr::from_mut(ctx.as_mut()), Ordering::Release);
    }

    while !ctx.ui.raylib_handle().window_should_close() {
        let nanos_since_last_frame = last_frame_drawn.elapsed().as_nanos();

        // Theres a lot of time to spare until next frame -> SLEEP
        // Otherwise spin loop until frame
        if nanos_since_last_frame < MAX_SLEEP_TIME {
            let sleep_nanos = MAX_SLEEP_TIME - nanos_since_last_frame;
            std::thread::sleep(Duration::from_nanos(sleep_nanos as u64));
            continue;
        }

        // Update and draw frame
        if nanos_since_last_frame >= FRAME_ELAPSED {
            last_frame_drawn += Duration::from_nanos(FRAME_ELAPSED as u64);
            // Update controllers
            ctx.nes
                .controller
                .update_keys_pressed(ctx.ui.raylib_handle(), None);

            let mut frame_new_info = ctx.update_info.load(Ordering::Acquire);
            ctx.update_info.store(false, Ordering::Release);
            let mut update_canvas = frame_count % 6 == 0;

            utils::handle_keys_presses(
                &mut ctx,
                &mut state.dev_mode,
                &mut state.ppu_info.selected_palette,
                &mut frame_new_info,
                &mut update_canvas,
                &mut state.ppu_info.toggle_mapper,
            )?;

            update_state(&mut ctx,  &mut state, frame_new_info, update_canvas);

            let old_page = state.memory_editor.current_page;

            ctx.ui.update(&mut state);
            
            let changed = old_page != state.memory_editor.current_page;
            
            apply_changes(&mut ctx, &mut state, changed)?;

            frame_count += 1;
        }
    }

    Ok(())
}

fn update_state(ctx: &mut AppCtx, state: &mut FrameState, frame_new_info: bool, update_canvas: bool) {
    if frame_new_info {
            ctx.ui.update_game_screen(ctx.nes.ppu.get_front_screen());
            state.show_press_start_msg = !ctx.nes.ppu.screen_not_null();
        }

    state.clear();

    if frame_new_info && state.dev_mode {
        state.clear_debug();
        if update_canvas && !state.ppu_info.toggle_mapper {
            utils::get_ppu_info(
                &mut ctx.nes,
                state.ppu_info.selected_palette,
                &mut state.ppu_info.chr_banks.0,
                &mut state.ppu_info.chr_banks.1,
                &mut state.ppu_info.palettes,
            );
        }
        let pc = Some(ctx.nes.cpu.registers.pc);
        utils::get_dissasembly(
            &mut ctx.nes,
            &mut state.dissasembly.mapping,
            &mut state.dissasembly.text,
            pc,
        );
        state.dissasembly.update = true;
        state.cpu_info = (&(ctx.nes.cpu)).into();
        utils::get_mem_page(
            state.memory_editor.current_page,
            &mut ctx.nes,
            &mut state.memory_editor.page_data,
        );
        state.memory_editor.paged_changed = true;
        if state.ppu_info.toggle_mapper {
            state.ppu_info.display_mapper_info = ctx.nes.mapper.debug_information(&mut state.ppu_info.mapper_info);
        } else {
            ctx.nes.ppu.debug_format_registers(&mut state.ppu_info.ppu_regs);
        }
    }
}

fn apply_changes(ctx: &mut AppCtx, state: &mut FrameState, memory_editor_page_changed: bool) -> Result<(), NesError> {
    if memory_editor_page_changed {
        utils::get_mem_page(
            state.memory_editor.current_page,
            &mut ctx.nes,
            &mut state.memory_editor.page_data,
        );
    }
    
    if let Some((a, v)) = state.memory_editor.modify_byte {
        ctx.nes.ppu_write(a, v);
    }

    ctx.nes.apu.options = state.audio.clone();
    ctx.nes.ppu.options.hide_overscan_region = state.video.hide_overscan_region;

    ctx.nes.handle_file_action(&state.file_action)?;
    if let FileAction::LoadROM(path) = &state.file_action {
        ctx.ui.raylib_handle().set_window_title(ctx.ui.raylib_thread(), &utils::get_title(path)?);  
    }

    ctx.breakpoint = state.dissasembly.bkpt;
    Ok(())
}

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

    if ctx.step_instruction.load(Ordering::Acquire) {
        loop {
            ctx.nes.clock(None).expect("Failure while stepping");
            let _ = ctx.audio_consumer.try_pop();
            if ctx.nes.cpu.state.cycles_left == 0 {
                break;
            }
        }
        ctx.step_instruction.store(false, Ordering::Release);
        ctx.update_info.store(true, Ordering::Release);
    }

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

        if let Some(b) = ctx.breakpoint {
            if ctx.nes.cpu.registers.pc == b {
                ctx.is_running.store(false, Ordering::Release);
                out_buffer[n_sample..].fill(0.0);
                break;
            }
        }

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
            ctx.nes.ppu.swap_buffers();
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
