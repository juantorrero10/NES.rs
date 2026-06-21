use std::ops::{Index, IndexMut};

use raylib::{RaylibHandle, ffi::KeyboardKey};

#[derive(Default)]
pub struct Controller {
    button_state: ButtonState,
    shift_register: u8,
    polling_set_high: bool,
    open_bus: u8,
}

#[derive(Default, Debug)]
struct ButtonState {
    a: bool,
    b: bool,
    select: bool,
    start: bool,
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

impl Index<usize> for ButtonState {
    type Output = bool;
    fn index(&self, index: usize) -> &Self::Output {
        match index & 0x07 {
            0 => &self.a,
            1 => &self.b,
            2 => &self.select,
            3 => &self.start,
            4 => &self.up,
            5 => &self.down,
            6 => &self.left,
            7 => &self.right,
            _ => unreachable!(),
        }
    }
}

impl IndexMut<usize> for ButtonState {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match index & 0x07 {
            0 => &mut self.a,
            1 => &mut self.b,
            2 => &mut self.select,
            3 => &mut self.start,
            4 => &mut self.up,
            5 => &mut self.down,
            6 => &mut self.left,
            7 => &mut self.right,
            _ => unreachable!(),
        }
    }
}

pub struct KeyboardMappings([KeyboardKey; 8]);

impl Default for KeyboardMappings {
    fn default() -> Self {
        Self {
            0: [
                KeyboardKey::KEY_SPACE,
                KeyboardKey::KEY_LEFT_SHIFT,
                KeyboardKey::KEY_BACKSPACE,
                KeyboardKey::KEY_ENTER,
                KeyboardKey::KEY_W,
                KeyboardKey::KEY_S,
                KeyboardKey::KEY_A,
                KeyboardKey::KEY_D,
            ],
        }
    }
}

impl ButtonState {
    fn as_shift_register(&self) -> u8 {
        let mut ret = 0;
        (0..8u8).for_each(|i| ret |= (self[i as usize] as u8) << i);
        ret
    }
}

impl Controller {
    pub fn update_keys_pressed(
        &mut self,
        rl: &RaylibHandle,
        mappings: Option<KeyboardMappings>,
    ) {
        let mappings = mappings.unwrap_or_default();
        for (i, key) in mappings.0.into_iter().enumerate() {
            self.button_state[i] = rl.is_key_down(key);
        }
    }

    pub fn event_cpu_has_read(&mut self, addr: u16) -> u8 {
        if addr != 0x4016 {
            return 0;
        }
        if self.polling_set_high {
            self.shift_register = self.button_state.as_shift_register();
            return self.button_state.a as u8 | 0x40;
        }

        let mut bit = self.shift_register & 1;
        self.shift_register >>= 1;
        self.shift_register |= 0x80;

        bit |= 0x40;

        return bit;
    }

    // No addr argument since it just can be 0x4016
    pub fn event_cpu_has_written(&mut self, data: u8) {
        self.polling_set_high = (data & 1) != 0;
        if self.polling_set_high {
            self.shift_register = self.button_state.as_shift_register();
        }
    }
}
