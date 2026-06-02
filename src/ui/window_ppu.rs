use raylib::texture::{RaylibRenderTexture2D, RaylibTexture2D, RenderTexture2D};

use crate::ui::{Fonts, Options, PanelProportions, Theme, framestate::FrameState};



pub fn update(
    ui: &imgui::Ui,
    dims: &PanelProportions,
    cb0: &mut RenderTexture2D, 
    cb1: &mut RenderTexture2D, 
    theme: &Theme,
    fonts: &Fonts,
    options: &mut Options,
    text_buffer: &mut String,
    st: &mut FrameState,
) {


    if !st.ppu_info.toggle_mapper {
        ui.dummy([0.0, dims.window_h * 0.004]);
        ui.indent_by(dims.window_w * 0.007);
        ui.text_colored(theme.title, "PPU INFO");
        ui.separator();
        cb0.update_texture(st.ppu_info.chr_bank_0).expect("Coulnt swap buffer");
        cb1.update_texture(st.ppu_info.chr_bank_1).expect("Coulnt swap buffer");
    
        ui.dummy([0.0, dims.window_h * 0.004]);
        let curr_pos = ui.cursor_pos();
    
        let draw_n = dims.right_middle_h * 0.7;
    
        imgui::Image::new(
            imgui::TextureId::new(cb0.texture().id as usize),
            [draw_n, draw_n],
        )
        .build(ui); 
    
        ui.set_cursor_pos(curr_pos);
        ui.indent_by(ui.window_size()[0] * 0.49);
    
        imgui::Image::new(
            imgui::TextureId::new(cb1.texture().id as usize),
            [draw_n, draw_n],
        )
        .build(ui);

        ui.set_cursor_pos([ui.window_size()[0] * 0.02, ui.window_size()[1] * 0.9]);
        ui.set_window_font_scale(0.8);
        ui.text_colored(theme.value, st.ppu_info.ppu_regs);
        ui.set_window_font_scale(1.0);
    } else {
        ui.dummy([0.0, dims.window_h * 0.004]);
        ui.indent_by(dims.window_w * 0.007);
        ui.text_colored(theme.title, "MAPPER INFO");
        ui.separator();
        if st.ppu_info.display_mapper_info {
            ui.text_colored(theme.value, st.ppu_info.mapper_info);
        } else {
            let _pi = ui.push_font(fonts[2]);
            ui.text_colored(theme.label, "Unavailable");
            _pi.pop();
        }
        ui.set_window_font_scale(1.0);
    }

}