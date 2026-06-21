use raylib::texture::{RaylibRenderTexture2D, RaylibTexture2D, RenderTexture2D};

use crate::ui::{Fonts, PanelProportions, Theme, framestate::FrameState};



pub fn update(
    ui: &imgui::Ui,
    dims: &PanelProportions,
    cb0: &mut RenderTexture2D, 
    cb1: &mut RenderTexture2D, 
    theme: &Theme,
    fonts: &Fonts,
    text_buffer: &mut String,
    st: &mut FrameState,
) {


    if !st.ppu_info.toggle_mapper {
        ui.dummy([0.0, dims.window_h * 0.004]);
        ui.indent_by(dims.window_w * 0.007);
        ui.text_colored(theme.title, "PPU INFO");
        ui.separator();
        cb0.update_texture(&st.ppu_info.chr_banks.0).expect("Coulnt swap buffer");
        cb1.update_texture(&st.ppu_info.chr_banks.1).expect("Coulnt swap buffer");
    
        ui.dummy([0.0, dims.window_h * 0.004]);
        let curr_pos = ui.cursor_pos();
    
        let draw_n = dims.right_middle_h * 0.7;
    
        imgui::Image::new(
            imgui::TextureId::new(cb0.texture().id as usize),
            [draw_n, draw_n],
        )
        .build(ui);

        ui.set_cursor_pos(curr_pos);
        ui.indent_by(ui.window_size()[0] * 0.50);

        if st.ppu_info.chr_banks.0[0..=3] != [0, 0, 0, 0] {
            draw_palettes(&curr_pos, ui, st, draw_n);
        }
        
    
        ui.set_cursor_pos(curr_pos);
        ui.indent_by(ui.window_size()[0] * 0.02);
    
        imgui::Image::new(
            imgui::TextureId::new(cb1.texture().id as usize),
            [draw_n, draw_n],
        )
        .build(ui);

        ui.set_cursor_pos([ui.window_size()[0] * 0.02, ui.window_size()[1] * 0.9]);
        ui.set_window_font_scale(0.8);
        ui.text_colored(theme.value, &st.ppu_info.ppu_regs);
        ui.set_window_font_scale(1.0);
    } else {
        ui.dummy([0.0, dims.window_h * 0.004]);
        ui.indent_by(dims.window_w * 0.007);
        ui.text_colored(theme.title, "MAPPER INFO");
        ui.separator();
        if st.ppu_info.display_mapper_info {
            ui.text_colored(theme.value, &st.ppu_info.mapper_info);
        } else {
            let _pi = ui.push_font(fonts[2]);
            ui.text_colored(theme.label, "Unavailable");
            _pi.pop();
        }
        ui.set_window_font_scale(1.0);
    }

}


fn draw_palettes(curr_pos: &[f32; 2], ui: &imgui::Ui, st: &FrameState, draw_n: f32) {
    let draw_list = ui.get_window_draw_list();
        let window_pos = ui.window_pos();

        let left_texture_end_x = curr_pos[0] + draw_n;
        let right_texture_start_x = ui.window_size()[0] * 0.55; 
        let center_gap_width = right_texture_start_x - left_texture_end_x;

        let box_dim = ui.window_size()[0] * 0.015; 
        let total_bar_width = box_dim * 4.0; 

        let bar_start_x = window_pos[0] + left_texture_end_x + ((center_gap_width - total_bar_width) * 0.5);

        let vertical_spacing = ui.window_size()[1] * 0.06; 
        let mut current_y = window_pos[1] + curr_pos[1] + vertical_spacing; 

        // 2. Loop through all 8 palettes
        for palette in st.ppu_info.palettes.iter() {
            
            // Loop through the 4 colors inside this individual palette entry
            for (color_idx, color) in palette.iter().enumerate() {
                let x_offset = (color_idx as f32) * box_dim;

                // Calculate rectangle corners
                let rect_start = [bar_start_x + x_offset, current_y];
                let rect_end = [rect_start[0] + box_dim, rect_start[1] + box_dim];

                // Convert u8 0..255 colors to f32 0.0..1.0
                let imgui_color = [
                    color[0] as f32 / 255.0,
                    color[1] as f32 / 255.0,
                    color[2] as f32 / 255.0,
                    1.0,
                ];

                // CORRECTED IMGUI-RS API: Draw the filled color square
                draw_list.add_rect(rect_start, rect_end, imgui_color)
                    .filled(true)
                    .build();
            }

            // Move down to prepare the screen coordinate for the next row entry
            current_y += box_dim + vertical_spacing;
        }
}