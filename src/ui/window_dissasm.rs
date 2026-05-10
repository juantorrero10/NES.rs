use imgui::StyleColor;

use crate::ui::{Fonts, Options, Theme, framestate::FrameState};

pub fn update(ui: &imgui::Ui, theme: &Theme, fonts: &Fonts, options: &mut Options, addr_buffer: &mut String, text_buffer: &mut String, st: &FrameState) {
    
    macro_rules! fmt_text_colored {
            ($ui:expr, $col:expr, $fmt:literal, $($arg:tt)*) => {
                text_buffer.clear();
                let _ = std::fmt::Write::write_fmt(text_buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
                $ui.text_colored($col, &text_buffer)
            };
    }
    
    
    ui.dummy([0.0, ui.window_size()[1] * 0.02]);
    ui.indent_by(ui.window_size()[0] * 0.025);
    ui.text_colored(theme.title, "DISASSEMBLY");
    ui.separator();
    ui.dummy([0.0, ui.window_size()[1] * 0.01]);
    let c_bg = if !options.disassm_focus_pc {
        theme.button
    } else {
        theme.highlight
    };
    ui.set_next_item_width(ui.window_size()[0] * 0.6);
    let _h = ui.push_style_color(StyleColor::FrameBg, theme.darker_bg);
    let _p = ui.push_style_color(StyleColor::TextDisabled, theme.label);
    let _pi = ui.push_font(fonts[2]);
    let mut has_jumped = false;
    let mut target_addr = 0;
    if ui
        .input_text("##address_input", addr_buffer)
        .hint("Address to jump to (hex4)")
        .flags(
            imgui::InputTextFlags::ENTER_RETURNS_TRUE
                | imgui::InputTextFlags::CHARS_HEXADECIMAL,
        )
        .build()
    {
        match u16::from_str_radix(&addr_buffer, 16) {
            Ok(a) => {
                target_addr = a;
                options.disassm_focus_pc = false;
                if a >= 0x8000 {
                    let diff = std::cmp::min(options.dissasm_show / 2, 0xFFFF - a);
                    options.dissasm_from = a - options.dissasm_show + diff;
                    options.dissasm_to = a + diff;
                } else {
                    let diff = std::cmp::min(options.dissasm_show / 2, a);
                    options.dissasm_from = a - diff;
                    options.dissasm_to = a + options.dissasm_show - diff;
                };
                has_jumped = true;
            }
            Err(e) => log::error!("Could convert jump address: {}", e),
        }
    }
    _h.pop();
    _p.pop();
    _pi.pop();
    ui.same_line();
    let _t = ui.push_style_color(StyleColor::Button, c_bg);
    if ui.button("Lock") {
        options.disassm_focus_pc = !options.disassm_focus_pc;
    }
    _t.pop();
    let allow_scroll = if options.disassm_focus_pc {
        imgui::WindowFlags::NO_SCROLL_WITH_MOUSE
    } else {
        imgui::WindowFlags::empty()
    };
    ui.child_window("dissasm_scroll")
        .size([0.0, ui.window_size()[1] * 0.75])
        .flags(allow_scroll)
        .border(false)
        .build(|| {
            let line_height = ui.text_line_height_with_spacing();
            let mut i = 0u32;
            let mut current_line_count = 0;
            let mut pc_line_index = 0;
            let f = ui.push_font(fonts[1]);
            ui.indent_by(3.0);
            let mut padding_text;
            let mut line_color;
            let current_scroll = ui.scroll_y();
            let max_scroll = ui.scroll_max_y();
            if max_scroll != 0.0 {
                if current_scroll <= 0.0 && options.dissasm_from != 0 {
                    let diff = std::cmp::min(
                        options.dissasm_show / 4,
                        options.dissasm_from,
                    );
                    options.dissasm_from =
                        options.dissasm_from.saturating_sub(diff);
                    options.dissasm_to =
                        options.dissasm_to.saturating_sub(diff);
                    ui.set_scroll_y(diff as f32 * line_height);
                }

                if current_scroll >= max_scroll - 1.0
                    && options.dissasm_to != 0xFFFF
                {
                    let diff = std::cmp::min(
                        options.dissasm_show / 4,
                        0xFFFF - options.dissasm_to,
                    );
                    options.dissasm_from =
                        options.dissasm_from.saturating_add(diff);
                    options.dissasm_to =
                        options.dissasm_to.saturating_add(diff);

                    // Move camera up to maintain relative position
                    ui.set_scroll_y(current_scroll - (diff as f32 * line_height));
                }
            }

            if options.disassm_focus_pc {
                options.dissasm_from =
                    st.cpu_info.regs.pc - (options.dissasm_show / 2) as u16;
                options.dissasm_to =
                    st.cpu_info.regs.pc + (options.dissasm_show / 2) as u16;
            }
            // Update line height
            let line_height = ui.text_line_height_with_spacing();
            while i <= options.dissasm_show as u32 {
                let addr = options.dissasm_from + i as u16;
                if addr == st.cpu_info.regs.pc {
                    pc_line_index = current_line_count;
                    padding_text = " > ";
                    line_color = theme.highlight;
                } else {
                    padding_text = "   ";
                    line_color = theme.value;
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
                        fmt_text_colored!(ui, theme.label, "${:04X} <unmapped>", addr);
                    }
                }
                if addr == 0xFFFF {
                    break;
                }
                current_line_count += 1;
            }
            if has_jumped && !options.disassm_focus_pc {
                let target_y =
                    (target_addr - options.dissasm_from) as f32 * line_height;
                let center_offset =
                    target_y - (ui.window_size()[1] / 2.0) + (line_height / 2.0);
                ui.set_scroll_y(center_offset);
            }
            if options.disassm_focus_pc {
                let target_y = pc_line_index as f32 * line_height;
                let center_offset =
                    target_y - (ui.window_size()[1] / 2.0) + (line_height / 2.0);
                ui.set_scroll_y(center_offset);
            }
            f.pop();
        });
}