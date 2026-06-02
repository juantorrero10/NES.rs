use super::*;

pub fn nes_loop(mut nes: Box<Nes>, mut ui: AppUI) -> Result<(), Box<dyn Error>> {
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

    let mut mapper_info = String::with_capacity(512);
    let mut display_mapper = false;
    let mut toggle_mapper =  false;

    let mut ppu_regs = String::with_capacity(128);

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

        // if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_P) {
        //     selected_palette += 1;
        //     selected_palette &= 0x3;
        //     frame_updated = true;
        //     fps10_draw_ctrl = 6;
        // }

        if ui.raylib_handle().is_key_pressed(KeyboardKey::KEY_M) {
            toggle_mapper ^= true;
        }

        if frame_updated && dev_mode {
            if false {
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
                display_mapper_info: display_mapper,
                mapper_info: &mapper_info,
                ppu_regs: &ppu_regs,
                toggle_mapper
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
