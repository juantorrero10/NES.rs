use std::fmt::Write;

use imgui::{FontId, StyleColor, StyleVar, WindowFlags, sys::ImGuiCond_FirstUseEver};
use log::info;
use raylib::{
    RaylibHandle, RaylibThread,
    color::Color,
    drawing::{RaylibDraw, RaylibTextureModeExt},
    texture::{RaylibRenderTexture2D, RaylibTexture2D, RenderTexture2D},
};
use raylib_imgui_rs::Renderer;

use crate::{nes::cpu::Flag6502, ui::framestate::FrameState};

pub mod framestate;
mod window_dissasm;
mod window_memory;
mod window_ppu;

struct Panel {
    pos: [f32; 2],
    size: [f32; 2],
}

pub struct PanelProportions {
    pub window_w: f32,
    pub window_h: f32,

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
        mut game_win_w: f32,
        mut game_win_h: f32,
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
        return [self.left_w, self.lower_h + 0.5];
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

struct RaylibInfo {
    handle: RaylibHandle,
    thread: RaylibThread,
    game_texture: RenderTexture2D,
    chr_bank0: RenderTexture2D,
    chr_bank1: RenderTexture2D,
}

impl RaylibInfo {
    fn new(
        handle: RaylibHandle,
        thread: RaylibThread,
        texture: RenderTexture2D, /*, screen_data: Vec<u8>*/
        c0: RenderTexture2D,
        c1: RenderTexture2D,
    ) -> Self {
        Self {
            handle,
            thread,
            game_texture: texture,
            chr_bank0: c0,
            chr_bank1: c1,
        }
    }
}

pub struct Options {
    disassm_focus_pc: bool,
    dissasm_from: u16,
    dissasm_to: u16,
    dissasm_show: u16,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            disassm_focus_pc: true,
            dissasm_from: 0,
            dissasm_to: 200,
            dissasm_show: 200,
        }
    }
}

type Fonts = [FontId; 3];
pub struct Theme {
    pub bg: [f32; 4],
    pub darker_bg: [f32; 4],
    pub game_screen_bg: [f32; 4],
    pub label: [f32; 4],
    pub title: [f32; 4],
    pub signal_low: [f32; 4],
    pub signal_high: [f32; 4],
    pub highlight: [f32; 4],
    pub highlight2: [f32; 4],
    pub value: [f32; 4],
    pub button: [f32; 4],
}

/**
*      let label_color = [0.75, 0.8, 0.82, 0.6];
       let window_title_color = [0.72, 0.8, 0.82, 0.9];
       let signal_low_color = [1.0, 0.3, 0.3, 1.0];
       let signal_high_color = [0.3, 1.0, 0.3, 1.0];
       let highlight_color = [0.25, 0.75, 0.87, 0.9];
       let highlight2_color = [0.98, 0.902, 0.337, 0.9];
       let value_color = [1.0, 1.0, 1.0, 0.9];
       let button_bg_color = [0.369, 0.361, 0.431, 1.0];
       let darker_bg_color = [0.049, 0.065, 0.092, 1.0f32];
*/

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: [0.1, 0.12, 0.15, 1.0],
            darker_bg: [0.049, 0.065, 0.092, 1.0f32],
            game_screen_bg: [0.0, 0.0, 0.0, 1.0f32],
            label: [0.75, 0.8, 0.82, 0.6],
            title: [0.72, 0.8, 0.82, 0.9],
            signal_low: [1.0, 0.3, 0.3, 1.0],
            signal_high: [0.3, 1.0, 0.3, 1.0],
            highlight: [0.25, 0.75, 0.87, 0.9],
            highlight2: [0.98, 0.902, 0.337, 0.9],
            value: [1.0, 1.0, 1.0, 0.9],
            button: [0.369, 0.361, 0.431, 1.0],
        }
    }
}

pub struct AppUI {
    pub ctx: imgui::Context,
    pub renderer: Renderer,
    pub raylib: RaylibInfo,
    pub dev_mode: bool,
    theme: Theme,
    fonts: Fonts,
    options: Options,
    text_buffer: String,
    addr_disassm_buffer: String,
    addr_memedit_buffer: String,
}

impl AppUI {
    pub fn new() -> Self {
        AppUI::initialize(Theme::default())
    }
    pub fn with_theme(t: Theme) -> Self {
        AppUI::initialize(t)
    }
    fn initialize(t: Theme) -> Self {
        let (mut rl, thread) = raylib::init()
            .size(1280, 720)
            .title("Nes")
            .resizable()
            .build();
        let mut ctx = imgui::Context::create();
        ctx.io_mut().config_flags |= imgui::ConfigFlags::DOCKING_ENABLE;
        let style = ctx.style_mut();
        style.window_rounding = 0.0;
        style.colors[imgui::StyleColor::WindowBg as usize] = t.bg; // Darker background
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

        let f2 = ctx.fonts().add_font(&[imgui::FontSource::TtfData {
            data: include_bytes!("C:\\Windows\\Fonts\\consolai.ttf"), // 'consolai' is the italic version
            size_pixels: font_size,
            config: None,
        }]);

        // Re-build the font atlas (required after adding fonts)
        let mut renderer = Renderer::create(&mut ctx, &mut rl, &thread);
        renderer.reload_fonts(&mut ctx, &mut rl, &thread);

        let screen = rl
            .load_render_texture(&thread, 256, 240)
            .expect("Couldn't make texture");
        let c0 = rl
            .load_render_texture(&thread, 128, 128)
            .expect("Couldnt make texture");
        let c1 = rl
            .load_render_texture(&thread, 128, 128)
            .expect("Couldnt make texture");

        screen.set_texture_filter(&thread, raylib::ffi::TextureFilter::TEXTURE_FILTER_POINT);
        c0.set_texture_filter(&thread, raylib::ffi::TextureFilter::TEXTURE_FILTER_POINT);
        c1.set_texture_filter(&thread, raylib::ffi::TextureFilter::TEXTURE_FILTER_POINT);
        Self {
            options: Options::default(),
            ctx,
            renderer,
            raylib: RaylibInfo::new(rl, thread, screen, c0, c1),
            dev_mode: true,
            text_buffer: String::with_capacity(128),
            addr_disassm_buffer: String::with_capacity(5),
            addr_memedit_buffer: "0000".to_owned(),
            fonts: [f0, f1, f2],
            theme: t,
        }
    }

    pub fn update(&mut self, st: &mut FrameState) {
        self.text_buffer.clear();
        let buffer = &mut self.text_buffer;

        macro_rules! fmt_text {
            ($ui:expr, $fmt:literal, $($arg:tt)*) => {
                buffer.clear();
                let _ = std::fmt::Write::write_fmt(buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
                $ui.text_colored(self.theme.value, &buffer)
            };
        }

        macro_rules! fmt_text_colored {
            ($ui:expr, $col:expr, $fmt:literal, $($arg:tt)*) => {
                buffer.clear();
                let _ = std::fmt::Write::write_fmt(buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
                $ui.text_colored($col, &buffer)
            };
        }

        self.renderer.update(&mut self.ctx, &mut self.raylib.handle);

        if st.screen.update {
            unsafe {
                self.raylib
                    .game_texture
                    .texture_mut()
                    .update_texture(std::slice::from_raw_parts(
                        st.screen.video_output.0,
                        st.screen.video_output.1,
                    ))
                    .expect("Couldnt swap buffer");
            }
        }

        let ui = self.ctx.new_frame();

        ui.dockspace_over_main_viewport();
        let window = [
            self.raylib.handle.get_screen_width() as f32,
            self.raylib.handle.get_screen_height() as f32,
        ]; // Get the window size;
        let dims;
        if self.dev_mode {
            dims = PanelProportions::new(window, 0.7, 0.7, 0.25, 0.33);
        } else {
            dims = PanelProportions::new(window, 1.0, 1.0, 0.25, 0.33);
        }

        if self
            .raylib
            .handle
            .is_key_pressed(raylib::ffi::KeyboardKey::KEY_F)
            && !ui.io().want_capture_keyboard
        {
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

        let _t = ui.push_style_color(StyleColor::WindowBg, self.theme.game_screen_bg);
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

                imgui::Image::new(
                    imgui::TextureId::new(self.raylib.game_texture.texture().id as usize),
                    [draw_w, draw_h],
                )
                .uv0([0.0, 1.0])
                .uv1([1.0, 0.0])
                .build(ui);
            });
        _t.pop();
        ui.window("ppu stuff")
            .flags(panel_flag)
            .position(dims.position_right_middle(), imgui::Condition::Always)
            .size(dims.size_right_middle(), imgui::Condition::Always)
            .build(|| {
                window_ppu::update(
                    ui,
                    &dims,
                    &mut self.raylib.chr_bank0,
                    &mut self.raylib.chr_bank1,
                    &self.theme,
                    &self.fonts,
                    &mut self.options,
                    buffer,
                    st,
                );
            });
        ui.window("CPU")
            .position(dims.position_right_upper(), imgui::Condition::Always)
            .flags(panel_flag)
            .size(dims.size_right_upper(), imgui::Condition::Always)
            .build(|| {
                ui.dummy([0.0, dims.window_h * 0.004]);
                ui.indent_by(dims.window_w * 0.007);
                ui.text_colored(self.theme.title, "CPU INFO");
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
                        ui.text_colored(self.theme.label, "PC: ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "${:04X}", st.cpu_info.regs.pc);
                        ui.text_colored(self.theme.label, "SP: ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "${:02X}", st.cpu_info.regs.sp);
                        ui.text_colored(self.theme.label, "A:  ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "${:02X}", st.cpu_info.regs.a);
                        ui.same_line();
                        ui.text_colored(self.theme.label, "  X: ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "${:02X}", st.cpu_info.regs.x);
                        ui.same_line();
                        ui.text_colored(self.theme.label, "  Y: ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "${:02X}", st.cpu_info.regs.y);
                        ui.text_colored(self.theme.label, "FLAGS: ");
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
                                ui.text_colored(self.theme.signal_high, s);
                            } else {
                                ui.text_colored(self.theme.signal_low, s);
                            }
                        }
                        ui.separator();
                        ui.text_colored(self.theme.label, "Cycles left: ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "{}", st.cpu_info.cycles_left);
                        ui.text_colored(self.theme.label, "Cycle counter: ");
                        ui.same_line();
                        fmt_text_colored!(ui, self.theme.value, "{}", st.cpu_info.global);
                        f.pop();
                    });
            });

        ui.window("Memory")
            .position(dims.position_lower(), imgui::Condition::Always)
            .size(dims.size_lower(), imgui::Condition::Always)
            .flags(panel_flag)
            .build(|| {
                window_memory::update(
                    ui,
                    &dims,
                    &self.theme,
                    &self.fonts,
                    &mut self.options,
                    &mut self.addr_memedit_buffer,
                    &mut self.text_buffer,
                    st,
                );
            });
        ui.window("Disassemble")
            .position(dims.position_right_lower(), imgui::Condition::Always)
            .size(dims.size_right_lower(), imgui::Condition::Always)
            .flags(panel_flag)
            .build(|| {
                window_dissasm::update(
                    ui,
                    &dims,
                    &self.theme,
                    &self.fonts,
                    &mut self.options,
                    &mut self.addr_disassm_buffer,
                    &mut self.text_buffer,
                    &st,
                )
            });

        let mut d = self.raylib.handle.begin_drawing(&self.raylib.thread);
        d.clear_background(raylib::prelude::Color::DARKGRAY);
        self.renderer.render(&mut self.ctx, &mut d);
    }

    pub fn raylib(&self) -> &RaylibInfo {
        return &self.raylib;
    }
    pub fn raylib_handle(&self) -> &RaylibHandle {
        return &self.raylib.handle;
    }
    pub fn raylib_thread(&self) -> &RaylibThread {
        return &self.raylib.thread;
    }
}
