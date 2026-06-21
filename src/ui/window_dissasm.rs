use imgui::StyleColor;

use crate::ui::{Fonts, PanelProportions, Theme, framestate::FrameState};

pub fn update(
    ui: &imgui::Ui,
    dims: &PanelProportions,
    theme: &Theme,
    fonts: &Fonts,
    addr_buffer: &mut String,
    text_buffer: &mut String,
    st: &mut FrameState,
) {
    macro_rules! fmt_text_colored {
            ($ui:expr, $col:expr, $fmt:literal, $($arg:tt)*) => {
                text_buffer.clear();
                let _ = std::fmt::Write::write_fmt(text_buffer, format_args!($fmt, $($arg)*)).expect("Fmt failed");
                $ui.text_colored($col, &text_buffer)
            };
    }

    let options = &mut st.dissasembly.options;

    ui.dummy([0.0, dims.window_h * 0.004]);
    ui.indent_by(dims.window_w * 0.007);
    ui.text_colored(theme.title, "DISASSEMBLY");
    ui.separator();
    ui.dummy([0.0, dims.window_h * 0.01]);
    let c_bg = if !options.focus_pc {
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
        .flags(imgui::InputTextFlags::ENTER_RETURNS_TRUE | imgui::InputTextFlags::CHARS_HEXADECIMAL)
        .build()
    {
        match u16::from_str_radix(&addr_buffer, 16) {
            Ok(a) => {
                target_addr = a;
                options.focus_pc = false;
                if a >= 0x8000 {
                    let diff = std::cmp::min(options.show / 2, 0xFFFF - a);
                    options.from = a - options.show + diff;
                    options.to = a + diff;
                } else {
                    let diff = std::cmp::min(options.show / 2, a);
                    options.from = a - diff;
                    options.to = a + options.show - diff;
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
        options.focus_pc = !options.focus_pc;
    }
    ui.same_line();
    _t.pop();
    let _t = ui.push_style_color(StyleColor::Button, theme.button);
    if ui.button("bkpt") {
        if addr_buffer.is_empty() {
            st.dissasembly.bkpt = None
        }
        else if let Ok(a) = u16::from_str_radix(&addr_buffer, 16) {
            st.dissasembly.bkpt = Some(a);
            addr_buffer.clear();
        }
    }
    _t.pop();
    let allow_scroll = if options.focus_pc {
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
                if current_scroll <= 0.0 && options.from != 0 {
                    let diff = std::cmp::min(options.show / 4, options.from);
                    options.from = options.from.saturating_sub(diff);
                    options.to = options.to.saturating_sub(diff);
                    ui.set_scroll_y(diff as f32 * line_height);
                }

                if current_scroll >= max_scroll - 1.0 && options.to != 0xFFFF {
                    let diff = std::cmp::min(options.show / 4, 0xFFFF - options.to);
                    options.from = options.from.saturating_add(diff);
                    options.to = options.to.saturating_add(diff);

                    // Move camera up to maintain relative position
                    ui.set_scroll_y(current_scroll - (diff as f32 * line_height));
                }
            }

            if options.focus_pc {
                options.from = st.cpu_info.regs.pc - (options.show / 2) as u16;
                options.to = st.cpu_info.regs.pc + (options.show / 2) as u16;
            }
            // Update line height
            let line_height = ui.text_line_height_with_spacing();
            while i <= options.show as u32 {
                let addr = options.from + i as u16;
                if addr == st.cpu_info.regs.pc {
                    pc_line_index = current_line_count;
                    padding_text = " > ";
                    line_color = theme.highlight;
                } else {
                    padding_text = "   ";
                    line_color = theme.value;
                }
                if let Some(bkpt) = st.dissasembly.bkpt && addr == bkpt {
                    line_color = theme.signal_low;
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
                        fmt_text_colored!(ui, theme.label, "${:04X}:   <unmapped>", addr);
                    }
                }
                if addr == 0xFFFF {
                    break;
                }
                current_line_count += 1;
            }
            if has_jumped && !options.focus_pc {
                let target_y = (target_addr - options.from) as f32 * line_height;
                let center_offset = target_y - (ui.window_size()[1] / 2.0) + (line_height / 2.0);
                ui.set_scroll_y(center_offset);
            }
            if options.focus_pc {
                let target_y = pc_line_index as f32 * line_height;
                let center_offset = target_y - (ui.window_size()[1] / 2.0) + (line_height / 2.0);
                ui.set_scroll_y(center_offset);
            }
            f.pop();
        });
}
