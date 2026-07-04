use crate::{nes::cpu::Cpu, ui::framestate::MemPage};

use super::*;

enum PageType {
    Cpu,
    Ppu
}

pub fn update(
    ui: &imgui::Ui,
    dims: &PanelProportions,
    theme: &Theme,
    fonts: &Fonts,
    addr_buffer: &mut String,
    text_buffer: &mut String,
    cmd_buffer: &mut String,
    st: &mut FrameState,
) {
    macro_rules! fmt_text_colored {
        ($ui:expr, $col:expr, $fmt:literal, $($arg:tt)*) => {
            text_buffer.clear();
            let _ = std::fmt::Write::write_fmt(text_buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
            $ui.text_colored($col, &text_buffer)
        };
    }
    let mut page_type = if let MemPage::Cpu(_) = st.memory_editor.current_page {
        PageType::Cpu
    } else {
        PageType::Ppu
    };

    let page_num = match st.memory_editor.current_page {
        MemPage::Cpu(p) => p,
        MemPage::Ppu(p) => p,
    };

    ui.dummy([0.0, dims.window_h * 0.004]);
    ui.indent_by(dims.window_w * 0.007);
    ui.text_colored(theme.title, "MEMORY EDITOR");
    ui.separator();

    ui.text_colored(theme.label, "Address: ");
    ui.same_line();
    ui.set_next_item_width(ui.window_size()[0] * 0.15); // Make it small since it's only 2 characters
    let _h = ui.push_style_color(StyleColor::FrameBg, theme.darker_bg);
    let _p = ui.push_style_color(StyleColor::TextDisabled, theme.label);
    let _pi = ui.push_font(fonts[2]);
    let mut has_jumped = false;
    let mut target_addr = 0;
    if ui
        .input_text("##page_input", addr_buffer)
        .hint("Page")
        .flags(imgui::InputTextFlags::ENTER_RETURNS_TRUE | imgui::InputTextFlags::CHARS_HEXADECIMAL)
        .build()
    {
        // When the user presses Enter, parse the hex string
        st.memory_editor.paged_changed = true;
        match u16::from_str_radix(addr_buffer, 16) {
            Ok(address) => {
                st.memory_editor.current_page = if let PageType::Cpu = page_type {
                    MemPage::Cpu((address >> 8) as u8)
                } else {
                    MemPage::Ppu((address >> 8) as u8)
                };
                st.memory_editor.search_byte_highlight = Some((address & 0x00FF) as u8)
            }
            Err(_) => {
                // Revert the text buffer back to the valid page if parsing fails
                addr_buffer.clear();
                write!(addr_buffer, "{:04X}", (page_num as u16) << 8).expect("Fmt failed");
            }
        }
    }
    _h.pop();
    _p.pop();
    _pi.pop();
    ui.same_line();
    ui.set_next_item_width(ui.window_size()[0] * 0.07);
    let _t = ui.push_style_color(StyleColor::Button, theme.button);
    if ui.button("<") {
        if page_num > 0 {
            st.memory_editor.current_page.dec();
            // Directly update the text field's buffer with the new address page
            addr_buffer.clear();
            st.memory_editor.search_byte_highlight = None;
            write!(
                addr_buffer,
                "{:04X}",
                ((page_num - 1) as u16) << 8
            )
            .expect("Fmt failed");
            st.memory_editor.paged_changed = true;
        }
    }
    ui.set_next_item_width(ui.window_size()[0] * 0.07);
    ui.same_line();
    if ui.button(">") {
        let max = match page_type {
            PageType::Cpu => 0xFF,
            PageType::Ppu => 0x3F
        };
        if page_num < max {
            st.memory_editor.current_page.inc();
            // Directly update the text field's buffer with the new address page
            addr_buffer.clear();
            st.memory_editor.search_byte_highlight = None;
            write!(
                addr_buffer,
                "{:04X}",
                ((page_num + 1) as u16) << 8
            )
            .expect("Fmt failed");
            st.memory_editor.paged_changed = true;
        }
    }
    _t.pop();

    ui.same_line();
    //ui.indent_by(ui.window_pos()[0] * 0.05);

    ui.set_next_item_width(ui.window_size()[0] * 0.15); // Make it small since it's only 2 characters
    let _h = ui.push_style_color(StyleColor::FrameBg, theme.darker_bg);
    let _p = ui.push_style_color(StyleColor::TextDisabled, theme.label);
    let _pi = ui.push_font(fonts[2]);
    let mut has_jumped = false;
    let mut target_addr = 0;
    if ui
        .input_text("##cmd_input", cmd_buffer)
        .hint("Command...")
        .flags(imgui::InputTextFlags::ENTER_RETURNS_TRUE)
        .build()
    {
        // When the user presses Enter, parse the hex string
        st.memory_editor.paged_changed = true;

        if cmd_buffer.starts_with('$') {
            match u16::from_str_radix(&cmd_buffer[1..5], 16) {
                Ok(addr) => {
                    if cmd_buffer.chars().nth(5) == Some('=') {
                        match u8::from_str_radix(&cmd_buffer[6..], 16) {
                            Ok(v) => {
                                st.memory_editor.modify_byte = Some((addr, v));
                            }
                            Err(_) => {}
                        }
                    }
                }
                _ => {}
            }
        }

        cmd_buffer.clear();
    }
    _h.pop();
    _p.pop();
    _pi.pop();

    ui.same_line();
    let mut cursor_pos = ui.cursor_pos();
    cursor_pos[0] += ui.content_region_avail()[0] * 0.2;
    ui.set_cursor_pos(cursor_pos);

    {
        let _t = ui.push_style_var(imgui::StyleVar::ItemSpacing([0.0, 0.0]));
        let button_size = [50.0, 20.0];
        let cols = match page_type {
            PageType::Cpu => (theme.highlight, theme.darker_bg),
            PageType::Ppu => (theme.darker_bg, theme.highlight),
        };

        let _t1 = ui.push_style_color(StyleColor::Button, cols.0);
        if ui.button_with_size("CPU", button_size) {
            st.memory_editor.current_page = MemPage::Cpu(0);
            addr_buffer.clear();
            write!(addr_buffer, "0000").expect("Fmt failed");
        }
        _t1.end();
        let _t1 = ui.push_style_color(StyleColor::Button, cols.1);
        ui.same_line();
        if ui.button_with_size("PPU", button_size) {
            st.memory_editor.current_page = MemPage::Ppu(0);
            addr_buffer.clear();
            write!(addr_buffer, "0000").expect("Fmt failed");
        }
        _t1.end()
        
    }

    ui.dummy([0.0, ui.content_region_avail()[1] * 0.05]);


    ui.child_window("mem_page_scroll")
        .size([0.0, ui.window_size()[1] * 0.95])
        .flags(WindowFlags::HORIZONTAL_SCROLLBAR)
        .border(false)
        .build(|| {
            fmt_text_colored!(ui, theme.label, "       00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F 10 11 12 13 14 15 16 17 18 19 1A 1B 1C 1D 1E 1F",);
            for i in 0..8 {
                fmt_text_colored!(ui, theme.label, "${:04X}: ", ((page_num as u16) << 8) + (i as u16) * 32);
                let _no_spacing;
                unsafe { _no_spacing = ui.push_style_var(StyleVar::ItemSpacing([0.0, ui.style().item_spacing[1]])); }
                ui.same_line();
                text_buffer.clear();
                for j in 0..32 {
                    let idx = i * 32 + j;
                    if let Some(b) = st.memory_editor.search_byte_highlight {
                        if b == idx as u8 {
                            // Flush what we have now.
                            if !text_buffer.is_empty() {
                                ui.text_colored(theme.value, &text_buffer);
                                ui.same_line();
                                text_buffer.clear();
                            }
                            match st.memory_editor.page_data[idx] {
                                Some(b) => {
                                    write!(text_buffer, "{:02X}", b).expect("Fmt failed");
                                    draw_highlighted_text(ui, text_buffer, theme.value, theme.highlight);
                                },
                                None => {
                                    text_buffer.push_str("??");
                                    draw_highlighted_text(ui, text_buffer, theme.label, theme.highlight);
                                }
                            }
                            
                            text_buffer.clear();
                            text_buffer.push(' ');
                            ui.same_line();
                            continue;
                        }
                    }
                    match st.memory_editor.page_data[idx] {
                        Some(b) => {
                            write!(text_buffer, "{:02X} ", b).expect("Fmt failed");
                        },
                        None => {
                            text_buffer.push_str("?? ");
                        }
                    }
                }
                ui.text_colored(theme.value, &text_buffer);
                _no_spacing.pop();
            }
        });

    if false {
        // 1. Calculate how wide the text is with the current font
        let text_width = ui.calc_text_size("Game is running...")[0];
        let f = ui.current_font_size();

        // 2. Get the current window width
        let window_width = ui.window_size()[0];
        let window_height = ui.window_size()[1];

        // 3. Set the cursor to the middle (X offset)
        // We keep the Y position the same as it currently is
        let center_x = (window_width - text_width) * 0.5;
        let center_y = (window_height - f) * 0.5;
        ui.set_cursor_pos([center_x, center_y]);

        // 4. Render
        ui.text_colored(theme.label, "Game is running...");
    }
}


fn draw_highlighted_text(ui: &imgui::Ui, text: &str, text_color: [f32; 4], bg_color: [f32; 4]) {
    // 1. Get the starting screen coordinate where the cursor currently sits
    let mut min_pos = ui.cursor_screen_pos();

    // 2. Calculate how large the text block will be on the screen
    let text_size = ui.calc_text_size(text);
    
    // 3. Compute the bottom-right corner bounding box coordinates
    let mut max_pos = [min_pos[0] + text_size[0], min_pos[1] + text_size[1]];

    // Make the rectangle a bit bigger
    min_pos[0] *= 0.996;
    min_pos[1] *= 0.999;
    max_pos[0] *= 1.004;
    max_pos[1] *= 1.001;
    
    // 4. Get the draw list for the current window and paint the background rectangle
    let draw_list = ui.get_window_draw_list();
    draw_list.add_rect(min_pos, max_pos, bg_color).filled(true).build();
    
    // 5. Draw the text overlay on top of the newly rendered rectangle
    ui.text_colored(text_color, text);
}