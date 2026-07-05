use std::{
    error::Error, fmt::Write, os::raw::c_void, sync::{
        Arc, atomic::{AtomicBool, AtomicPtr, Ordering},
    }, time::{Duration, Instant},
};

use raylib::ffi::SetAudioStreamCallback;
use ringbuf::{SharedRb, storage::Heap, traits::Consumer, wrap::caching::Caching};

use crate::{
    FRAME_ELAPSED_NANOS, nes::{Nes, NesError}, ui::{
        AppUI, framestate::{FileAction, FrameState, MemPage},
    }, utils,
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
}

static NES_CTX: AtomicPtr<AppCtx> = AtomicPtr::new(std::ptr::null_mut());

pub fn nes_loop(mut ctx: Box<AppCtx>) -> Result<(), Box<dyn Error>> {
    let mut state = FrameState::new_boxed();

    let mut frame_count = 0;

    let mut next_frame = Instant::now();

    let mut window_title = String::new();

    let mut frame_times= Duration::from_secs(0);

    let mut t0 = Instant::now();

    unsafe {
        SetAudioStreamCallback(*ctx.ui.raylib_audio_stream(), Some(nes_audio_callback));
        NES_CTX.store(std::ptr::from_mut(ctx.as_mut()), Ordering::Release);
    }

    while !ctx.ui.raylib_handle().window_should_close() {

        let mut update_info = false;

        next_frame += Duration::from_nanos(FRAME_ELAPSED_NANOS as u64);

        if ctx.is_running.load(Ordering::Acquire) {
            if ctx.nes.step_frame(state.dissasembly.bkpt)? {
                ctx.is_running.store(false, Ordering::Release);
            }
            update_info = true;
        } else if ctx.step_instruction.load(Ordering::Acquire) {
            ctx.step_instruction.store(false, Ordering::Release);
            ctx.nes.step_instruction()?;
            update_info = true;
        }


        // Update and draw frame
        ctx.nes
            .controller
            .update_keys_pressed(ctx.ui.raylib_handle(), None);

        let mut update_canvas = frame_count % 6 == 0;

        utils::handle_keys_presses(
            &mut ctx,
            &mut state.dev_mode,
            &mut state.ppu_info.selected_palette,
            &mut update_info,
            &mut update_canvas,
            &mut state.ppu_info.toggle_mapper
        )?;

        update_state(&mut ctx, &mut state, update_info, update_canvas);

        let old_page = state.memory_editor.current_page;

        frame_times += t0.elapsed();
        
        if frame_count % 6 == 0 {
            window_title.clear();
            write!(
                window_title,
                "{:.2} FPS",
                6.0 / frame_times.as_secs_f32()
            )?;
            frame_times = Duration::ZERO;
            //log::info!("{}", window_title);
            ctx.ui.raylib_handle().set_window_title(ctx.ui.raylib_thread(), &window_title);
        }
        //write!(window_title, "{:.2} FPS", (1_000_000.0 / t0.elapsed().as_micros() as f32))?;

        t0 = Instant::now();

        ctx.ui.update(&mut state);



        // log::info!("Drawn!, t0: {:.2} ms", (last_frame_drawn.elapsed().as_nanos() as f32) / 1_000_000.0);

        let changed = old_page != state.memory_editor.current_page;

        apply_changes(&mut ctx, &mut state, changed)?;

        frame_count += 1;

        let now = Instant::now();

        if let Some(late) = now.checked_duration_since(next_frame) {
            let frame_elapsed = Duration::from_nanos_u128(FRAME_ELAPSED_NANOS);
            if late > frame_elapsed {
                next_frame = now + frame_elapsed;
            }
        }

        let premature_wakeup  = Duration::from_micros(500);

        if now + premature_wakeup < next_frame {
            std::thread::sleep(next_frame - now - premature_wakeup);
        }

        while Instant::now() < next_frame {
            std::hint::spin_loop();
        }
    }

    Ok(())
}

fn update_state(
    ctx: &mut AppCtx,
    state: &mut FrameState,
    frame_new_info: bool,
    update_canvas: bool,
) {
    if frame_new_info {
        ctx.ui.update_game_screen(ctx.nes.ppu.get_screen());
        state.show_press_start_msg = !ctx.nes.ppu.screen_not_null();
    }

    state.clear();

    if ctx.ui.nametable_window_shown() {
        let nes_ptr = std::ptr::from_mut(&mut ctx.nes);
        ctx.nes.ppu.debug_update_nametables(&mut state.ppu_info.nametables.0, nes_ptr);
        ctx.ui.update_nametables(&state.ppu_info.nametables.0);
    }    

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
            state.ppu_info.display_mapper_info = ctx
                .nes
                .mapper
                .debug_information(&mut state.ppu_info.mapper_info);
        } else {
            ctx.nes
                .ppu
                .debug_format_registers(&mut state.ppu_info.ppu_regs);
        }
    }
}

fn apply_changes(
    ctx: &mut AppCtx,
    state: &mut FrameState,
    mut memory_editor_page_changed: bool,
) -> Result<(), NesError> {
    if let Some((a, v)) = state.memory_editor.modify_byte {
        match state.memory_editor.current_page {
            MemPage::Cpu(_) => {
                ctx.nes.write(a, v)?;
            }
            MemPage::Ppu(_) => {
                ctx.nes.ppu_write(a, v);
            }
        }
        memory_editor_page_changed = true;
    }

    if memory_editor_page_changed {
        utils::get_mem_page(
            state.memory_editor.current_page,
            &mut ctx.nes,
            &mut state.memory_editor.page_data,
        );
    }

    ctx.nes.apu.options = state.audio.clone();
    ctx.nes.ppu.options.hide_overscan_region = state.video.hide_overscan_region;

    ctx.nes.handle_file_action(&state.file_action)?;
    if let FileAction::LoadROM(path) = &state.file_action {
        ctx.ui
            .raylib_handle()
            .set_window_title(ctx.ui.raylib_thread(), &utils::get_title(path)?);
    }
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

    if !ctx.is_running.load(Ordering::Acquire) {
        unsafe {
            std::ptr::write_bytes(buffer as *mut f32, 0, sample_count);
            return;
        }
    }

    let mut n_sample = 0;
    let mut last_sample = 0.0;

    while n_sample < sample_count {

        if let Some(sample) = ctx.audio_consumer.try_pop() {
            last_sample = sample;
            out_buffer[n_sample] = sample;
        } else {
            out_buffer[n_sample] = last_sample;
        }
        n_sample += 1;
    }
}
