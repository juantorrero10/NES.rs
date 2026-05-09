use std::fmt::Write;

use imgui::{FontId, StyleVar, WindowFlags, sys::ImGuiCond_FirstUseEver};
use log::info;
use raylib::{RaylibHandle, RaylibThread, color::Color, drawing::{RaylibDraw, RaylibTextureModeExt}, texture::{RaylibRenderTexture2D, RaylibTexture2D, RenderTexture2D}};
use raylib_imgui_rs::Renderer;

use crate::{nes::cpu::Flag6502, ui::framestate::FrameState};

pub mod framestate;



struct Panel {
    pos: [f32; 2],
    size: [f32; 2],
}

struct PanelProportions {
    window_w: f32,
    window_h: f32,

    // They sum 1
    upper_h: f32,
    lower_h: f32,

    // They sum 1
    left_w: f32,
    right_w: f32,

    // They sum 1
    right_upper_h: f32,
    right_middle_h: f32,
    right_lower_h: f32,
}

impl PanelProportions {
    fn new(
        window_size: [f32; 2],
        mut game_win_h: f32,
        mut game_win_w: f32,
        mut right_upper_h: f32,
        mut right_middle_h: f32,
    ) -> Self {
        debug_assert!(
            game_win_h >= 0.0 && game_win_w >= 0.0 && right_upper_h >= 0.0 && right_middle_h >= 0.0
        );
        debug_assert!(
            game_win_h <= 1.0 && game_win_w <= 1.0 && right_upper_h <= 1.0 && right_middle_h <= 1.0
        );
        game_win_w *= window_size[0];
        game_win_h *= window_size[1];
        right_upper_h *= window_size[1];
        right_middle_h *= window_size[1];
        Self {
            window_w: window_size[0],
            window_h: window_size[1],
            upper_h: game_win_h,
            lower_h: window_size[1] - game_win_h + 0.5,
            left_w: game_win_w,
            right_w: window_size[0] - game_win_w,
            right_upper_h: right_upper_h,
            right_middle_h: right_middle_h,
            right_lower_h: (window_size[1] - right_middle_h - right_upper_h) + 0.5,
        }
    }

    fn position_game_win(&self) -> [f32; 2] {
        return [0.0, 0.0];
    }
    fn position_lower(&self) -> [f32; 2] {
        return [0.0, self.upper_h];
    }
    fn position_right_upper(&self) -> [f32; 2] {
        return [self.left_w, 0.0];
    }
    fn position_right_middle(&self) -> [f32; 2] {
        return [self.left_w, self.right_upper_h];
    }
    fn position_right_lower(&self) -> [f32; 2] {
        return [self.left_w, self.right_middle_h + self.right_upper_h];
    }

    fn size_game_win(&self) -> [f32; 2] {
        return [self.left_w, self.upper_h];
    }
    fn size_lower(&self) -> [f32; 2] {
        return [self.left_w, self.lower_h];
    }
    fn size_right_upper(&self) -> [f32; 2] {
        return [self.right_w, self.right_upper_h];
    }
    fn size_right_middle(&self) -> [f32; 2] {
        return [self.right_w, self.right_middle_h];
    }
    fn size_right_lower(&self) -> [f32; 2] {
        return [self.right_w, self.right_lower_h + 0.5];
    }
}

pub struct RaylibInfo(RaylibHandle, RaylibThread, RenderTexture2D, Vec<u8>);

pub struct AppUI {
    pub ctx: imgui::Context,
    pub renderer: Renderer,
    pub raylib: RaylibInfo,
    pub dev_mode: bool,
    pub fonts: [FontId; 2],
    text_buffer: String,
}

impl AppUI {
    pub fn new() -> Self {
        let (mut rl, thread) = raylib::init()
            .size(1280, 720)
            .title("Nes")
            .resizable()
            .build();
        let mut ctx = imgui::Context::create();
        ctx.io_mut().config_flags |= imgui::ConfigFlags::DOCKING_ENABLE;
        let style = ctx.style_mut();
        style.window_rounding = 0.0;
        style.colors[imgui::StyleColor::WindowBg as usize] = [0.1, 0.12, 0.15, 1.0]; // Darker background
        let font_size = 18.0; // Increase this for larger text
        let f0 = ctx.fonts().add_font(&[imgui::FontSource::TtfData {
            data: include_bytes!("C:\\Windows\\Fonts\\consola.ttf"), // This embeds it into the binary
            size_pixels: font_size,
            config: Some(imgui::FontConfig {
                rasterizer_multiply: 1.2, // Makes the font a bit "thicker" and clearer
                ..imgui::FontConfig::default()
            }),
        }]);
        let f1 = ctx.fonts().add_font(&[imgui::FontSource::TtfData {
            data: include_bytes!("C:\\Windows\\Fonts\\CascadiaMono.ttf"), // This embeds it into the binary
            size_pixels: font_size * 1.1,
            config: Some(imgui::FontConfig {
                rasterizer_multiply: 1.2, // Makes the font a bit "thicker" and clearer
                ..imgui::FontConfig::default()
            }),
        }]);
        
        // Re-build the font atlas (required after adding fonts)
        let mut renderer = Renderer::create(&mut ctx, &mut rl, &thread);
        renderer.reload_fonts(&mut ctx, &mut rl, &thread);

        let screen = rl.load_render_texture(&thread, 256, 240).expect("Couldn't make texture");
        screen.set_texture_filter(&thread, raylib::ffi::TextureFilter::TEXTURE_FILTER_POINT);
        Self {
            ctx,
            renderer,
            raylib: RaylibInfo(rl, thread, screen, vec![0u8; 256 * 240 * 4]),
            dev_mode: true,
            text_buffer: String::with_capacity(128),
            fonts: [f0, f1]
        }
    }

    pub fn update(&mut self, st: FrameState) {
        self.text_buffer.clear();
        let buffer = &mut self.text_buffer;

        macro_rules! fmt_text {
            ($ui:expr, $fmt:literal, $($arg:tt)*) => {
                buffer.clear();
                let _ = std::fmt::Write::write_fmt(buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
                $ui.text(&buffer)
            };
        }

        macro_rules! fmt_text_colored {
            ($ui:expr, $col:expr, $fmt:literal, $($arg:tt)*) => {
                buffer.clear();
                let _ = std::fmt::Write::write_fmt(buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
                $ui.text_colored($col, &buffer)
            };
        }

        self.renderer.update(&mut self.ctx, &mut self.raylib.0);

         {
            for i in (0..self.raylib.3.len()).step_by(4) {
                let lum = rand::random::<u8>();
                self.raylib.3[i]     = lum; // R
                self.raylib.3[i + 1] = lum; // G
                self.raylib.3[i + 2] = lum; // B
                self.raylib.3[i + 3] = 255; // A
            }
            self.raylib.2.texture_mut().update_texture(&self.raylib.3).expect("Couldnt swap buffer");
        }


        let ui = self.ctx.new_frame();

        ui.dockspace_over_main_viewport();
        let window = [
            self.raylib.0.get_screen_width() as f32,
            self.raylib.0.get_screen_height() as f32,
        ]; // Get the window size;
        let dims;
        if self.dev_mode {
            dims = PanelProportions::new(window, 0.6, 0.6, 0.25, 0.33);
        } else {
            dims = PanelProportions::new(window, 1.0, 1.0, 0.25, 0.33);
        }

        if self.raylib.0.is_key_pressed(raylib::ffi::KeyboardKey::KEY_F) {
            self.dev_mode = match self.dev_mode {
                true => false,
                false => true,
            }
        }

        let panel_flag = imgui::WindowFlags::NO_TITLE_BAR
            | imgui::WindowFlags::NO_RESIZE
            | imgui::WindowFlags::NO_MOVE
            | imgui::WindowFlags::NO_COLLAPSE
            | imgui::WindowFlags::NO_SCROLLBAR
            | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;

        let label_color = [0.75, 0.8, 0.82, 0.6];
        let highlight_color = [0.2, 0.7, 0.82, 1.0];
        let value_color = [1.0, 1.0, 1.0, 0.9];

        ui.window("Game")
            .flags(panel_flag)
            .position(dims.position_game_win(), imgui::Condition::Always)
            .size(dims.size_game_win(), imgui::Condition::Always)
            .build(|| {
                let content_size = ui.content_region_avail();
                let nes_aspect = 256.0 / 240.0;
                let window_aspect = content_size[0] / content_size[1];

                let (draw_w, draw_h) = if window_aspect > nes_aspect {
                    // Window is wider than NES - fit to height
                    (content_size[1] * nes_aspect, content_size[1])
                } else {
                    // Window is taller than NES - fit to width
                    (content_size[0], content_size[0] / nes_aspect)
                };

                // Center the image (the "Black Bands" logic)
                let x_offset = (content_size[0] - draw_w) / 2.0;
                let y_offset = (content_size[1] - draw_h) / 2.0;
                ui.set_cursor_pos([ui.cursor_pos()[0] + x_offset, ui.cursor_pos()[1] + y_offset]);
                
                imgui::Image::new(imgui::TextureId::new(self.raylib.2.texture().id as usize), [draw_w, draw_h])
                    .uv0([0.0, 1.0])
                    .uv1([1.0, 0.0])
                    .build(ui);
            });
        ui.window("Console")
            .flags(panel_flag)
            .position(dims.position_lower(), imgui::Condition::Always)
            .size(dims.size_lower(), imgui::Condition::Always)
            .build(|| {
                ui.text("Console output will go here...");
            });
        ui.window("CPU")
            .position(dims.position_right_upper(), imgui::Condition::Always)
            .flags(panel_flag)
            .size(dims.size_right_upper(), imgui::Condition::Always)
            .build(|| {
                ui.dummy([0.0, dims.size_right_upper()[1] * 0.01]);
                ui.indent_by(dims.size_right_upper()[0] * 0.025);
                ui.text_colored(label_color, "CPU INFO");
                ui.separator();
                ui.dummy([0.0, dims.size_right_upper()[1] * 0.01]);
                ui.child_window("info")
                    .size([
                        dims.size_right_upper()[0] * 0.92,
                        dims.size_right_upper()[1] * 0.65,
                    ])
                    .border(true)
                    .build(|| {
                        let f = ui.push_font(self.fonts[1]);
                        ui.text_colored(label_color, "PC: ");
                        ui.same_line();
                        fmt_text!(ui, "${:04X}", st.cpu_info.regs.pc);
                        ui.text_colored(label_color, "SP: ");
                        ui.same_line();
                        fmt_text!(ui, "${:02X}", st.cpu_info.regs.sp);
                        ui.text_colored(label_color, "A:  ");
                        ui.same_line();
                        fmt_text!(ui, "${:02X}", st.cpu_info.regs.a);
                        ui.same_line();
                        ui.text_colored(label_color, "  X: ");
                        ui.same_line();
                        fmt_text!(ui, "${:02X}", st.cpu_info.regs.x);
                        ui.same_line();
                        ui.text_colored(label_color, "  Y: ");
                        ui.same_line();
                        fmt_text!(ui, "${:02X}", st.cpu_info.regs.y);
                        ui.text_colored(label_color, "FLAGS: ");
                        for i in [
                            Flag6502::C,
                            Flag6502::Z,
                            Flag6502::I,
                            Flag6502::D,
                            Flag6502::B,
                            Flag6502::U,
                            Flag6502::V,
                            Flag6502::N,
                        ] {
                            ui.same_line();
                            let s: &str = i.into();
                            if i.reg_has(st.cpu_info.regs.sr) {
                                ui.text_colored([0.3, 1.0, 0.3, 1.0], s);
                            } else {
                                ui.text_colored([1.0, 0.3, 0.3, 1.0], s);
                            }
                        }
                        ui.separator();
                        ui.text_colored(label_color, "Cycles left: ");
                        ui.same_line();
                        fmt_text!(ui, "{}", st.cpu_info.cycles_left);
                        ui.text_colored(label_color, "Cycle counter: ");
                        ui.same_line();
                        fmt_text!(ui, "{}", st.cpu_info.global);
                        f.pop();
                    });
            });

        ui.window("Memory")
            .position(dims.position_right_middle(), imgui::Condition::Always)
            .size(dims.size_right_middle(), imgui::Condition::Always)
            .flags(panel_flag)
            .build(|| {
                ui.text("Memory Hex View Placeholder");
            });
        ui.window("Disassemble")
            .position(dims.position_right_lower(), imgui::Condition::Always)
            .size(dims.size_right_lower(), imgui::Condition::Always)
            .flags(panel_flag)
            .build(|| {
                ui.dummy([0.0, dims.size_right_lower()[1] * 0.02]);
                ui.indent_by(dims.size_right_lower()[0] * 0.025);
                ui.text_colored(label_color, "DISASSEMBLY");
                ui.separator();
                let showing = 100u32;
                let mut i = 0u32;
                let mut current_line_count = 0;
                let mut pc_line_index = 0;
                ui.child_window("dissasm_scroll")
                    .size([0.0, 0.0])
                    .border(false)
                    .build(|| {
                        let f = ui.push_font(self.fonts[1]);
                        ui.indent_by(3.0);
                        let mut padding_text;
                        let mut line_color;
                        while i <= showing {
                            let addr =
                                (st.cpu_info.regs.pc + i as u16).wrapping_sub((showing / 2) as u16);

                            if addr == st.cpu_info.regs.pc {
                                pc_line_index = current_line_count;
                                padding_text = " > ";
                                line_color = highlight_color;
                            } else {
                                padding_text = "   ";
                                line_color = value_color;
                            }
                            match st.dissasembly.mapping.get(&addr) {
                                Some((start, end, jump)) => {
                                    i += *jump as u32;
                                    ui.text_colored(line_color, padding_text);
                                    ui.same_line();
                                    ui.text_colored(line_color, &st.dissasembly.text[*start..*end]);
                                }
                                None => {
                                    i += 1;
                                    ui.text(padding_text);
                                    ui.same_line();
                                    fmt_text_colored!(ui, label_color, "${:04X} <unmapped>", addr);
                                }
                            }
                            current_line_count += 1;
                        }
                        if st.dissasembly.new_info {
                            let line_height = ui.text_line_height_with_spacing();
                            let target_y = pc_line_index as f32 * line_height;
                            let center_offset =
                                target_y - (ui.window_size()[1] / 2.0) + (line_height / 2.0);
                            ui.set_scroll_y(center_offset);
                        }
                        f.pop();
                    });
            });

        let mut d = self.raylib.0.begin_drawing(&self.raylib.1);
        d.clear_background(raylib::prelude::Color::DARKGRAY);
        self.renderer.render(&mut self.ctx, &mut d);
    }

    pub fn raylib(&self) -> &RaylibInfo {
        return &self.raylib;
    }
    pub fn raylib_handle(&self) -> &RaylibHandle {
        return &self.raylib.0;
    }
    pub fn raylib_thread(&self) -> &RaylibThread {
        return &self.raylib.1;
    }
}
