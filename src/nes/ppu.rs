use std::{fmt::Write, time::Instant};

use thiserror::Error;

use crate::nes::{
    Nes,
    bus_cpu::BusCpuError,
    game_rom::MemoryMirroring,
    mappers::Mapper,
    ppu::{
        self,
        BusPpuTarget::{Memory, OpenBus},
    },
};

#[allow(unused)]
pub const ADDR_CHR_START: u16 = 0x0000;
pub const ADDR_CHR_END: u16 = 0x1FFF;

pub const ADDR_NAME_START: u16 = 0x2000;
pub const ADDR_NAME_END: u16 = 0x3EFF;
pub const SIZE_NAME_TBL: usize = 0x400;

pub const ADDR_PAL_START: u16 = 0x3F00;
pub const ADDR_PAL_END: u16 = 0x3FFF;
pub const SIZE_PALETTE: usize = 32;

pub struct BusPpu {
    pub address_bus: u16,
    chips: BusPpuChips,
    palettes: [u8; SIZE_PALETTE],
    open_bus_sim: u8,
    fake_value: u8,
}

pub struct BusPpuChips {
    nametables: [u8; 2 * SIZE_NAME_TBL],
}

pub enum BusPpuTarget<'a> {
    Memory(&'a mut u8),
    OpenBus,
}

impl BusPpu {
    pub fn new() -> Self {
        BusPpu {
            chips: BusPpuChips {
                nametables: [0; 2 * SIZE_NAME_TBL],
            },
            palettes: [0; SIZE_PALETTE],
            address_bus: 0,
            open_bus_sim: 0,
            fake_value: 0,
        }
    }

    fn resolve_addr<'a>(
        &'a mut self,
        addr: u16,
        data: u8,
        write: bool,
        mapper: &'a mut Box<dyn Mapper>,
        debug: bool,
        update_address_bus: bool,
    ) -> BusPpuTarget<'a> {
        let mut mapped_addr = addr & 0x3FFF;

        // Address bus is really just an emulation for the mapper.
        if mapped_addr <= ADDR_CHR_END && update_address_bus {
            self.address_bus = addr;
        }

        if mapped_addr <= ADDR_CHR_END && mapper.ppu_need_intercept(addr, debug) {
            return mapper.ppu_intercept(addr, &mut self.chips, data, write);
        }

        match mapped_addr {
            ADDR_NAME_START..=ADDR_NAME_END => {
                // Addresses 0x3000 - 0x3EFF are mirrored to 0x2000 - 0x2EFF
                if mapped_addr >= 0x3000 {
                    mapped_addr -= 0x1000;
                }
                let mut addr_byte = mapped_addr - 0x2000;
                let name_table_idx = addr_byte / 0x400;
                addr_byte &= (SIZE_NAME_TBL as u16) - 1;
                addr_byte += match mapper.mirroring() {
                    MemoryMirroring::Vertical => match name_table_idx {
                        0 => 0,
                        1 => 0x400,
                        2 => 0,
                        3 => 0x400,
                        _ => unreachable!(),
                    },
                    MemoryMirroring::Horizontal => match name_table_idx {
                        0 => 0,
                        1 => 0,
                        2 => 0x400,
                        3 => 0x400,
                        _ => panic!("{} {} {}", name_table_idx, addr_byte, addr),
                    },
                    _ => {
                        panic!("Mirroring not implemented: {:?}", mapper.mirroring())
                    }
                };
                return Memory(self.chips.nametables.get_mut(addr_byte as usize).unwrap());
            }
            ADDR_PAL_START..=ADDR_PAL_END => {
                mapped_addr &= (SIZE_PALETTE as u16) - 1;
                let mut palette_idx = mapped_addr & (SIZE_PALETTE as u16 - 1);
                if palette_idx == 0x0010
                    || palette_idx == 0x0014
                    || palette_idx == 0x0018
                    || palette_idx == 0x001C
                {
                    palette_idx &= 0x000F;
                }
                return Memory(self.palettes.get_mut(palette_idx as usize).unwrap());
            }
            _ => {}
        }
        unreachable!();
    }
    #[inline(always)]
    pub fn read(&mut self, addr: u16, mapper: &mut Box<dyn Mapper>) -> u8 {
        match self.resolve_addr(addr, 0, false, mapper, false, true) {
            Memory(r) => {
                let d = *r;
                self.open_bus_sim = d;
                return d;
            }
            OpenBus => return self.open_bus_sim,
        }
    }

    pub fn debug_read(&mut self, addr: u16, mapper: &mut Box<dyn Mapper>) -> u8 {
        match self.resolve_addr(addr, 0, false, mapper, true, false) {
            Memory(r) => {
                let d = *r;
                self.open_bus_sim = d;
                return d;
            }
            OpenBus => return 0,
        }
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8, mapper: &mut Box<dyn Mapper>) {
        self.open_bus_sim = data;
        match self.resolve_addr(addr, data, true, mapper, false, true) {
            Memory(r) => {
                *r = data;
            }
            OpenBus => {}
        }
    }
}

const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 240;
const SCREEN_SIZE: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

const CYCLE_WIDTH: usize = 340;
const SCANLINE_HEIGHT: isize = 260;

const DBG_CANVAS_WIDTH: usize = 128;
const DBG_CANVAS_HEIGHT: usize = 128;
const DBG_CANVAS_SIZE: usize = DBG_CANVAS_HEIGHT * DBG_CANVAS_WIDTH * 4;

pub struct Ppu {
    pub frame_complete: bool,
    pub nmi_signal: bool,
    pub is_odd_frame: bool,

    pub log: bool,

    pub scanline: isize,
    pub cycle: usize,

    t0: Instant,
    total: Instant,
    n_frames: f64,

    screen_data: [u8; SCREEN_SIZE],

    ctrl: u8,
    status: u8,
    mask: u8,
    fine_x: u8,
    vram: VRAM,
    oam: OAM,

    bg_regs: BGRegs,
    fg_regs: FGRegs,
}

#[derive(Default)]
struct BGRegs {
    shift_pat_low: u16,
    shift_pat_high: u16,

    shift_att_low: u16,
    shift_att_high: u16,

    latch_name: u8,
    latch_att: u8,
    latch_pat_lo: u8,
    latch_pat_hi: u8,
}

#[derive(Default, Clone, Copy)]
struct ActiveSprite {
    pattern_lo: u8,
    pattern_hi: u8,
    x_counter: u8,
    attribute: OAFlags,
}

#[derive(Default)]
struct FGRegs {
    sprites: [ActiveSprite; 8],
    active_sprites_count: usize,
    sprite0_possible: bool,
    sprite0_hit_scheduled: Option<usize>,
}

#[derive(Default)]
struct VRAM {
    addr: u16,
    t_addr: u16,
    // Set Low: High byte, Set High: Low Byte
    latch: bool,
    delayed_data: u8,
}

struct OAM {
    addr: usize,
    mem: [OAttribute; 64],

    secondary_mem: [OAttribute; 8],
}

#[derive(Clone, Copy)]
#[repr(packed)]
struct OAttribute {
    y_pos: u8,
    tile_id: u8,
    flags: OAFlags,
    x_pos: u8,
}

impl Default for OAttribute {
    fn default() -> Self {
        Self {
            y_pos: 0xFF,
            tile_id: 0xFF,
            flags: OAFlags { reg: 0xFF },
            x_pos: 0xFF,
        }
    }
}

#[derive(Copy, Clone, Default)]
#[repr(packed)]
struct OAFlags {
    reg: u8,
}

#[repr(u8)]
#[derive(Clone, Copy)]
enum OAFFields {
    SpritePalIndex = 0x3,
    #[allow(unused)]
    Unused = 0x1C,
    Priority = 0x20,
    HorizontalFlip = 0x40,
    VerticalFlip = 0x80,
}

use OAFFields::*;

impl OAFlags {
    #[inline(always)]
    fn get(&self, f: OAFFields) -> u8 {
        let v = self.reg & (f as u8);
        match f {
            SpritePalIndex => v,
            Unused => v >> 2,
            Priority => v >> 5,
            HorizontalFlip => v >> 6,
            VerticalFlip => v >> 7,
        }
    }
}

use PpuRegister::*;

use ppu_flags::*;

#[derive(Error, Debug)]
pub enum PpuError {
    #[error("Could not read/write to PPU registers on Cpu bus: {0}")]
    RegisterMemError(#[from] BusCpuError),
}

type Result<T> = std::result::Result<T, PpuError>;

// Main renderring implementation block
impl Ppu {
    pub fn clock(&mut self, nes: *mut Nes, mapper: &mut Box<dyn Mapper>) {
        if let Some(e) = mapper.on_ppu_clock() {
            let instance = unsafe { nes.as_mut().unwrap() };
            instance
                .trigger_event(e)
                .expect("Event failed on PPU Clock hook");
        }
        // let _span = tracy_client::span!("Clock PPU");
        // self.frame_complete = false;
        // Background register manipulation timeline.
        match self.scanline {
            -1..=239 => {
                self.scanline_bg_routine(nes);
                self.scanline_fg_routine(nes);
            }
            240 => {
                // Post render line
            }
            241 => {
                // V-Blank line
                if self.cycle == 1 {
                    self.status |= STATUS_VBLANK;

                    if (self.ctrl & CTRL_GENERATE_NMI) != 0 {
                        self.nmi_signal = true;
                    }
                }
            }
            242..=260 => {
                // V-Blank stage
                // Let the cpu do its thing
            }
            _ => unreachable!(),
        }

        let mut red_col = false;

        let (pal_idx, col_idx) = {
            let mut fg_pal = 0;
            let mut fg_col = 0;

            let mut bg_pal = 0;
            let mut bg_col = 0;

            let mut fg_priority = false;
            let mut sprite_zero_rendered = false;

            if (self.mask & MASK_RENDER_SPRITES) != 0 {
                (fg_pal, fg_col, fg_priority, sprite_zero_rendered) =
                    self.extract_foreground_color();
            }

            if (self.mask & MASK_RENDER_BG) != 0 {
                (bg_pal, bg_col) = self.extract_background_color();
            }

            _ = self.check_sprite0_hit(bg_col, fg_col, sprite_zero_rendered);

            self.blend_colors((fg_pal, fg_col), (bg_pal, bg_col), fg_priority)
        };

        let mut system_color_idx =
            self.read(ADDR_PAL_START + (4 * pal_idx as u16) + col_idx as u16, nes);
        // restraint to the 64 colors
        system_color_idx &= 0x3f;
        // FINALLY

        let rgb_color = NES_COLOR_PALETTE[system_color_idx as usize];

        self.set_pixel(self.cycle, self.scanline, rgb_color);

        self.cycle += 1;
        if self.cycle > CYCLE_WIDTH {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > SCANLINE_HEIGHT {
                self.scanline = -1;
                // self.n_frames += 1.0;
                // let elapsed = self.t0.elapsed().as_micros() as f64;
                // log::info!(
                //     "FPS delay: {:.4} ms ({:.4} FPS)",
                //     elapsed / 1_000.0,
                //     1_000_000.0 / elapsed
                // );
                // self.t0 = Instant::now();
                self.frame_complete = true;
                self.is_odd_frame ^= true;
            }
        }
    }

    #[inline(always)]
    fn blend_colors(
        &self,
        foreground: (u8, u8),
        background: (u8, u8),
        fg_priority: bool,
    ) -> (u8, u8) {
        match (foreground.1 != 0, background.1 != 0) {
            (false, false) => (0, 0),
            (false, true) => background,
            (true, false) => foreground,
            _ => {
                if fg_priority {
                    return foreground;
                }
                background
            }
        }
    }

    fn scanline_bg_routine(&mut self, nes: *mut Nes) {
        // Guard
        if !self.is_rendering_enabled() {
            return;
        }

        match self.cycle {
            0 => {
                // Odd frame skip
                if self.scanline == 0 && self.is_odd_frame {
                    self.cycle = 1;
                }
            }
            1..=256 => {
                if self.scanline == -1 && self.cycle == 1 {
                    self.status &= !(STATUS_VBLANK | STATUS_SPRITE_0_HIT | STATUS_SPRITE_OVERFLOW);
                }
                self.process_bg_step(nes);
                if self.cycle == 256 {
                    self.increment_scroll_y();
                }
            }
            257 => self.transfer_scroll_x(),
            258..=320 => {
                // Unused NT reads

                if self.scanline == -1 && self.cycle >= 280 && self.cycle <= 304 {
                    self.transfer_scroll_y();
                }
            }
            321..=336 => {
                self.process_bg_step(nes);
            }
            // Dummy NT reads
            337..=340 => {
                let dummy_nt_addr = 0x2000
                    | (self.vram.addr
                        & (VRAM_COARSE_X_MASK | VRAM_COARSE_Y_MASK | VRAM_NAMETABLE_MASK));
                // Although the data is discarded, mappers can syncronize stuff in the reads
                let _ = self.read(dummy_nt_addr, nes);
            }
            _ => unreachable!(),
        }
    }

    fn scanline_fg_routine(&mut self, nes: *mut Nes) {
        if !self.is_rendering_enabled() {
            return;
        }

        // Shift registers
        if self.cycle > 0 && self.cycle <= 256 {
            self.shift_fg_registers();
        }

        match self.cycle {
            0 => {
                #[cfg(feature = "log")]
                {
                    if self.log {
                        log::info!("New Scanline!. SC: {}, CYC: 0", self.scanline);
                    }
                }
            }
            // Clean secondary Mem if not in pre-render line
            1 => {
                if self.scanline != -1 {
                    self.oam.secondary_mem = [OAttribute::default(); 8];
                    self.fg_regs.active_sprites_count = 0;
                }
            }
            2..=64 => {
                // On real hardware cycle 1 to 64 are spent cleaning up seconday OAM
                // Since we can do it in one go lets just do nothing on the remaining cycles.
            }
            // Sprite evalutation for next line if not on pre-render
            // Again on real hardware this takes all cycles from 65 to 256 to do.
            // We can just get it over with in cycle 256.
            65..=255 => {}
            256 => {
                if self.scanline != -1 {
                    #[cfg(feature = "log")]
                    {
                        if self.log {
                            log::info!("Sprite Eval!CYC: {}", self.cycle)
                        }
                    }
                    self.sprite_eval();
                }
            }

            // Because of mappers it is best to trigger this some what at the beggining of the 257 to 320 window.
            // This is the best i found specially for SMB3 (mapper MMC3).
            // Probably the best would be to implement the cycle by cycle routine like the real hardware
            // instead of fetching it all at once.
            257..=260 => {}
            // I moved it to 261 to be more accurate
            261 => {
                #[cfg(feature = "log")]
                {
                    if self.log {
                        log::info!("Sprite fetch!, CYC: {}", self.cycle)
                    }
                }
                self.sprite_fetch(nes);
            }
            262..=320 => {}

            // Wait for next line
            321..=340 => {}

            _ => unreachable!(),
        }
    }

    fn sprite_eval(&mut self) {
        self.fg_regs.sprite0_possible = false;
        for (i, sprite) in self.oam.mem.iter().enumerate() {
            let sprite_height = if (self.ctrl & CTRL_SPRITE_SIZE) != 0 {
                16
            } else {
                8
            };

            // Check vertical intersection
            let sc = self.scanline as u8;
            if sc < sprite.y_pos || sc >= sprite.y_pos + sprite_height {
                continue;
            }

            if self.fg_regs.active_sprites_count >= 8 && self.scanline != -1 {
                self.status |= STATUS_SPRITE_OVERFLOW;
                break;
            }

            if i == 0 {
                self.fg_regs.sprite0_possible = true;
            }
            // Copy to seconday
            self.oam.secondary_mem[self.fg_regs.active_sprites_count] = *sprite;
            self.fg_regs.active_sprites_count += 1;
        }
    }

    fn sprite_fetch(&mut self, nes: *mut Nes) {
        for (i, active_sprite) in self.oam.secondary_mem.iter().enumerate() {
            let mut row = self.scanline as u8 - active_sprite.y_pos;

            let sprite_height = if (self.ctrl & CTRL_SPRITE_SIZE) != 0 {
                16
            } else {
                8
            };

            if active_sprite.flags.get(VerticalFlip) != 0 {
                row = (sprite_height - 1) - row;
            }

            // Fetch of CHR ROM Pattern tables
            let addr;
            if sprite_height == 8 {
                let base_addr = if (self.ctrl & CTRL_SPRITE_PATTERN) != 0 {
                    0x1000
                } else {
                    0x0000
                };
                addr = base_addr | ((active_sprite.tile_id as u16) << 4) | (row as u16 & 7);
            } else {
                let base_addr = if (active_sprite.tile_id & 1) != 0 {
                    0x1000
                } else {
                    0x0000
                };
                let mut tile_idx = (active_sprite.tile_id & 0xfe) as u16;
                if row >= 8 {
                    tile_idx += 1;
                }
                addr = base_addr | (tile_idx << 4) | (row as u16 & 7);
            }

            let mut pattern_lo = self.read(addr, nes);
            let mut pattern_hi = self.read(addr + 8, nes);

            if active_sprite.flags.get(HorizontalFlip) != 0 {
                pattern_lo = pattern_lo.reverse_bits();
                pattern_hi = pattern_hi.reverse_bits();
            }

            self.fg_regs.sprites[i] = ActiveSprite {
                pattern_lo,
                pattern_hi,
                x_counter: active_sprite.x_pos,
                attribute: active_sprite.flags,
            }
        }
        for s in &mut self.fg_regs.sprites[self.fg_regs.active_sprites_count..8] {
            *s = ActiveSprite::default();
        }
    }

    fn process_bg_step(&mut self, nes: *mut Nes) {
        self.shift_bg_registers();
        // Process each of the 8 sub-cycles
        let sub_cycle = (self.cycle - 1) % 8;
        match sub_cycle {
            0 => {}
            // Name table read
            1 => {
                let nt_addr = 0x2000
                    | (self.vram.addr
                        & (VRAM_COARSE_X_MASK | VRAM_COARSE_Y_MASK | VRAM_NAMETABLE_MASK));
                self.bg_regs.latch_name = self.read(nt_addr, nes)
            }
            2 => {}
            // Attribute table read
            3 => {
                // Grab attribute byte
                let v = self.vram.addr;
                // VRAM_ADDR (15 bits) (y: Nametable Y, Y: Coarse Y), same with x
                // _FFFyxYYYYYXXXXX
                // Attribute address to read (14 bits)
                // __10yx1111YYYXXX
                // __10000000000000 = 0x2000 (Nametable offset in memory)
                // __00001111000000 = 0x03C0 (Attribute offset inside nametable)
                // (YYY: Coarse Y upper 3 bits, essentially coarse_y // 4)
                // Same with Coarse X
                //

                let coarse_x = v & VRAM_COARSE_X_MASK;
                let coarse_y = (v & VRAM_COARSE_Y_MASK) >> 5;

                let attr_addr = 0x2000
                    | (v & VRAM_NAMETABLE_MASK)
                    | 0x03C0
                    | ((coarse_y >> 2) << 3)
                    | (coarse_x >> 2);
                let attr_byte = self.read(attr_addr, nes);

                let shift = ((coarse_y & 2) << 1) | (coarse_x & 2);
                self.bg_regs.latch_att = (attr_byte >> shift) & 0x03;
            }
            4 => {}
            // Pattern table reads
            // 5 Low
            // 7 High
            5 | 7 => {
                // Determine which pattern table, and then get the base address.
                let pattern_table_base = if (self.ctrl & CTRL_BG_PATTERN) != 0 {
                    0x1000
                } else {
                    0x0000
                };
                let fine_y = (self.vram.addr & VRAM_FINE_Y_MASK) >> 12;
                let tile_id = self.bg_regs.latch_name as u16;

                let pat_addr = pattern_table_base + (tile_id << 4) + fine_y;

                if sub_cycle == 5 {
                    self.bg_regs.latch_pat_lo = self.read(pat_addr, nes);
                } else {
                    self.bg_regs.latch_pat_hi = self.read(pat_addr + 8, nes);

                    // This is just something that happens periodically.
                    // Unrelated to reading from Pattern high table but it happens at the same
                    self.increment_scroll_x();
                    self.load_background_shift_registers();
                }
            }
            6 => {}
            _ => unreachable!(),
        }
    }

    fn extract_background_color(&self) -> (u8, u8) {
        let mux = 0x8000 >> self.fine_x;

        let pixel_bit0 = (self.bg_regs.shift_pat_low & mux) > 0;
        let pixel_bit1 = (self.bg_regs.shift_pat_high & mux) > 0;

        let mut pixel = (pixel_bit1 as u8) << 1 | (pixel_bit0 as u8);

        let pal_bit0 = (self.bg_regs.shift_att_low & mux) > 0;
        let pal_bit1 = (self.bg_regs.shift_att_high & mux) > 0;

        let mut palette = (pal_bit1 as u8) << 1 | (pal_bit0 as u8);

        if self.cycle < 8 && (self.mask & MASK_RENDER_LEFT_BG) == 0 {
            pixel = 0;
            // palette = 0;
        }

        (palette, pixel)
    }

    fn extract_foreground_color(&self) -> (u8, u8, bool, bool) {
        let mut pal_idx = 0;
        let mut col_idx = 0;
        let mut priority = false;
        let mut is_zero_rendered = false;

        for (i, spr) in self.fg_regs.sprites.iter().enumerate() {
            if spr.x_counter != 0 {
                continue;
            }
            let col_lo = (spr.pattern_lo & 0x80) != 0;
            let col_hi = (spr.pattern_hi & 0x80) != 0;
            col_idx = (col_hi as u8) << 1 | col_lo as u8;

            if col_idx == 0 {
                continue;
            }

            pal_idx = spr.attribute.get(SpritePalIndex) + 4;
            priority = spr.attribute.get(Priority) == 0;
            is_zero_rendered = i == 0;

            break;
        }
        if self.cycle < 8 && (self.mask & MASK_RENDER_LEFT_SPR) == 0 {
            // pal_idx = 0;
            col_idx = 0;
        }
        (pal_idx, col_idx, priority, is_zero_rendered)
    }

    #[inline(always)]
    fn set_pixel(&mut self, x: usize, y: isize, data: [u8; 3]) {
        if y < 0 {
            return;
        }
        let y = y as usize;
        if x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT {
            return;
        }
        let offset = 4 * (y * SCREEN_WIDTH + x);
        self.screen_data[offset..(offset + 3)].copy_from_slice(&data);
        self.screen_data[offset + 3] = 255;
    }
}

// Rendering helpers
impl Ppu {
    #[inline(always)]
    fn increment_scroll_y(&mut self) {
        // log::info!("inc vert(v) => SC: {}, CYC: {}", self.scanline, self.cycle);
        let mut fine_y = (self.vram.addr & VRAM_FINE_Y_MASK) >> 12;
        fine_y += 1;
        if fine_y > 7 {
            let mut coarse_y = (self.vram.addr & VRAM_COARSE_Y_MASK) >> 5;
            coarse_y += 1;
            if coarse_y > 29 {
                if coarse_y == 30 {
                    // Flip Name table Y Bit
                    self.vram.addr ^= VRAM_NAMETABLE_Y_MASK;
                }
                coarse_y = 0;
            }
            self.vram.addr = (self.vram.addr & !VRAM_COARSE_Y_MASK) | coarse_y << 5;
        }
        fine_y &= 0x7;
        self.vram.addr = (self.vram.addr & !VRAM_FINE_Y_MASK) | fine_y << 12
    }

    #[inline(always)]
    fn transfer_scroll_x(&mut self) {
        // log::info!("hori(v) =  hori(t) => SC: {}, CYC: {}", self.scanline, self.cycle);
        // Copy coarse x and nametable x
        let mask = VRAM_COARSE_X_MASK | VRAM_NAMETABLE_X_MASK;
        self.vram.addr = (self.vram.addr & !mask) | (self.vram.t_addr & mask);
    }

    #[inline(always)]
    fn transfer_scroll_y(&mut self) {
        // log::info!("vert(v) =  vert(t) => SC: {}, CYC: {}", self.scanline, self.cycle);
        // copy scroll y nametable y and fine y
        let mask = VRAM_COARSE_Y_MASK | VRAM_NAMETABLE_Y_MASK | VRAM_FINE_Y_MASK;
        self.vram.addr = (self.vram.addr & !mask) | (self.vram.t_addr & mask);
    }

    #[inline(always)]
    fn increment_scroll_x(&mut self) {
        //log::info!("inc hori(v) => SC: {}, CYC: {}", self.scanline, self.cycle);
        let mut coarse_x = self.vram.addr & VRAM_COARSE_X_MASK;
        coarse_x += 1;
        if coarse_x > 31 {
            coarse_x = 0;
            // Flip Name table X Bit
            self.vram.addr ^= VRAM_NAMETABLE_X_MASK;
        }
        coarse_x &= 0x1f;
        self.vram.addr = (self.vram.addr & !VRAM_COARSE_X_MASK) | coarse_x;
    }

    #[inline(always)]
    pub fn is_rendering_enabled(&self) -> bool {
        // Returns true if either the background bit (0x08) or sprite bit (0x10) is set
        (self.mask & (MASK_RENDER_BG | MASK_RENDER_SPRITES)) != 0
    }

    #[inline(always)]
    fn shift_bg_registers(&mut self) {
        if self.is_rendering_enabled() {
            self.bg_regs.shift_pat_low <<= 1;
            self.bg_regs.shift_pat_high <<= 1;
            self.bg_regs.shift_att_low <<= 1;
            self.bg_regs.shift_att_high <<= 1;
        }
    }

    #[inline(always)]
    fn shift_fg_registers(&mut self) {
        for spr in &mut self.fg_regs.sprites {
            if spr.x_counter != 0 {
                spr.x_counter -= 1;
            } else {
                spr.pattern_lo <<= 1;
                spr.pattern_hi <<= 1;
            }
        }
    }

    fn load_background_shift_registers(&mut self) {
        self.bg_regs.shift_pat_low =
            (self.bg_regs.shift_pat_low & 0xFF00) | (self.bg_regs.latch_pat_lo as u16);
        self.bg_regs.shift_pat_high =
            (self.bg_regs.shift_pat_high & 0xFF00) | (self.bg_regs.latch_pat_hi as u16);

        let inflated_att0 = if (self.bg_regs.latch_att & 0x01) != 0 {
            0xFF
        } else {
            0
        };
        let inflated_att1 = if (self.bg_regs.latch_att & 0x02) != 0 {
            0xFF
        } else {
            0
        };

        self.bg_regs.shift_att_low = (self.bg_regs.shift_att_low & 0xFF00) | inflated_att0;
        self.bg_regs.shift_att_high = (self.bg_regs.shift_att_high & 0xFF00) | inflated_att1;
    }

    #[inline(always)]
    fn check_sprite0_hit(&mut self, bg_col: u8, fg_col: u8, sprite_zero_rendered: bool) -> bool {
        if bg_col > 0 && fg_col > 0 && self.cycle <= 256 && self.cycle != 255 {
            if self.fg_regs.sprite0_possible && sprite_zero_rendered {
                if self.cycle < 8 {
                    // For a hit to happen here, BOTH the left BG and left Sprite rendering must be enabled!
                    let left_rendering_enabled = (self.mask & MASK_RENDER_LEFT_BG) != 0
                        && (self.mask & MASK_RENDER_LEFT_SPR) != 0;

                    if left_rendering_enabled {
                        self.status |= STATUS_SPRITE_0_HIT;
                        return true;
                    }
                } else if self.cycle >= 9 {
                    // Past pixel 8, the clipping mask no longer restricts hits
                    self.status |= STATUS_SPRITE_0_HIT;
                    return true;
                }
            }
        }
        return false;
    }
}

// Other Ppu functionalities

#[repr(u16)]
#[derive(Debug)]
pub enum PpuRegister {
    Ctrl = 0x2000,
    Mask = 0x2001,
    Status = 0x2002,
    OamAddr = 0x2003,
    OamData = 0x2004,
    Scroll = 0x2005,
    VRAMAddr = 0x2006,
    VRAMData = 0x2007,
}

impl PpuRegister {
    pub fn from_0_to_7(mut offset: u16) -> Self {
        offset &= 0x7;
        match offset {
            0 => Ctrl,
            1 => Mask,
            2 => Status,
            3 => OamAddr,
            4 => OamData,
            5 => Scroll,
            6 => VRAMAddr,
            7 => VRAMData,
            _ => unreachable!(),
        }
    }
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            screen_data: [0u8; SCREEN_SIZE],
            oam: OAM {
                mem: [OAttribute::default(); 64],
                addr: 0,
                secondary_mem: [OAttribute::default(); 8],
            },
            vram: VRAM::default(),
            bg_regs: BGRegs::default(),
            fg_regs: FGRegs::default(),
            frame_complete: false,
            nmi_signal: false,
            is_odd_frame: false,
            n_frames: 0.0,
            log: false,
            total: Instant::now(),
            t0: Instant::now(),
            ctrl: 0,
            mask: 0,
            fine_x: 0,
            status: 0,
            cycle: 0,
            scanline: -1,
        }
    }

    pub fn reset(&mut self) {
        self.cycle = 0;
        self.scanline = -1;
        self.frame_complete = false;
        self.is_odd_frame = false;

        self.ctrl = 0;
        self.mask = 0;
        // It doesnt actually reset
        self.status &= !STATUS_VBLANK;

        self.fine_x = 0;

        self.vram.delayed_data = 0;
        self.vram.latch = false;

        self.oam.addr;

        self.screen_data.fill(0);
    }

    pub fn toggle_log(&mut self) {
        self.log ^= true;
    }

    pub fn read(&self, addr: u16, nes: *mut Nes) -> u8 {
        let addr = addr & 0x3FFF;
        let nes = unsafe { nes.as_mut().unwrap() };
        nes.ppu_read(addr)
    }

    pub fn debug_read(&self, addr: u16, nes: *mut Nes) -> u8 {
        let addr = addr & 0x3FFF;
        let nes = unsafe { nes.as_mut().unwrap() };
        nes.ppu_debug_read(addr)
    }

    pub fn write(&self, addr: u16, data: u8, nes: *mut Nes) {
        let addr = addr & 0x3FFF;
        let nes = unsafe { nes.as_mut().unwrap() };
        nes.ppu_write(addr, data)
    }

    pub fn event_cpu_has_written(
        &mut self,
        reg: PpuRegister,
        data: u8,
        nes: *mut Nes,
    ) -> Result<()> {
        match reg {
            Ctrl => {
                self.ctrl = data;

                let name_table = data as u16 & 3;
                self.vram.t_addr = (self.vram.t_addr & !VRAM_NAMETABLE_MASK) | (name_table << 10);
            }
            Mask => {
                self.mask = data;
            }
            OamAddr => {
                self.oam.addr = data as usize;
            }
            OamData => {
                match self.oam.addr & 0x3 {
                    0 => self.oam.mem[self.oam.addr / 4].y_pos = data,
                    1 => self.oam.mem[self.oam.addr / 4].tile_id = data,
                    2 => self.oam.mem[self.oam.addr / 4].flags.reg = data,
                    3 => self.oam.mem[self.oam.addr / 4].x_pos = data,
                    _ => unreachable!(),
                }
                self.oam.addr += 1;
                self.oam.addr &= 0xFF;
            }
            Scroll => {
                if !self.vram.latch {
                    // First write: X-Scroll
                    // Coarse X (bits 3-7 of data) goes into bits 0-4 of t_addr
                    let coarse_x = ((data >> 3) & 0x1F) as u16;
                    self.vram.t_addr = (self.vram.t_addr & !VRAM_COARSE_X_MASK) | coarse_x;
                    self.fine_x = data & 0b0000_0111;
                } else {
                    // Second write: Y-Scroll
                    // Coarse Y (bits 3-7 of data) goes into bits 5-9 of t_addr
                    let coarse_y = ((data >> 3) & 0x1F) as u16;
                    self.vram.t_addr = (self.vram.t_addr & !VRAM_COARSE_Y_MASK) | (coarse_y << 5);
                    // Fine Y (bits 0-2 of data) goes into bits 12-14 of t_addr
                    let fine_y = data as u16 & 0x7;
                    self.vram.t_addr = (self.vram.t_addr & !VRAM_FINE_Y_MASK) | (fine_y << 12);
                }
                self.vram.latch ^= true;
            }
            VRAMAddr => {
                if !self.vram.latch {
                    self.vram.t_addr = (((data & 0x3f) as u16) << 8) | (self.vram.t_addr & 0x00FF);
                } else {
                    self.vram.t_addr = (self.vram.t_addr & 0x7F00) | data as u16;
                    self.vram.addr = self.vram.t_addr;
                }
                self.vram.latch ^= true;
            }
            VRAMData => {
                self.write(self.vram.addr, data, nes);

                // Increment VRAM Addr
                let increment = if (self.ctrl & 0x04) != 0 { 32 } else { 1u16 };
                self.vram.addr = self.vram.addr.wrapping_add(increment);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn event_cpu_has_read(&mut self, reg: PpuRegister, nes: *mut Nes) -> Result<u8> {
        match reg {
            Status => {
                let ret = self.status;

                // Side effects
                self.status &= !STATUS_VBLANK; // Clear V-blank flag
                self.vram.latch = false; // Reset latch

                return Ok(ret);
            }
            OamData => {
                return Ok(match self.oam.addr & 0x3 {
                    0 => self.oam.mem[self.oam.addr / 4].y_pos,
                    1 => self.oam.mem[self.oam.addr / 4].tile_id,
                    2 => self.oam.mem[self.oam.addr / 4].flags.reg,
                    3 => self.oam.mem[self.oam.addr / 4].x_pos,
                    _ => unreachable!(),
                });
            }
            VRAMData => {
                let mut fetch = self.vram.delayed_data;
                self.vram.delayed_data = self.read(self.vram.addr, nes);

                // Reading paletts bypass this delay
                if self.vram.addr >= ADDR_PAL_START {
                    fetch = self.vram.delayed_data;
                }

                // Increment address
                let increment = if (self.ctrl & ppu_flags::CTRL_VRAM_INCREMENT) != 0 {
                    32
                } else {
                    1u16
                };
                self.vram.addr = self.vram.addr.wrapping_add(increment);

                return Ok(fetch);
            }
            _ => {}
        }
        Ok(0)
    }

    pub fn event_oam_dma_transfer(&mut self, data: &[u8]) -> Result<()> {
        assert!(data.len() == 256);
        let slice = unsafe { std::mem::transmute(&data[..64]) };
        self.oam.mem.copy_from_slice(slice);
        Ok(())
    }

    #[inline(always)]
    pub fn get_screen(&self) -> (*const u8, usize) {
        (self.screen_data.as_ptr(), self.screen_data.len())
    }

    #[inline(always)]
    pub fn get_cycle_and_sc(&self) -> (isize, usize) {
        (self.scanline, self.cycle)
    }

    pub fn debug_format_registers(&self, out: &mut String) {
        write!(
            out,
            "STATUS: {:02X} CTRL: {:02X} MASK: {:02X} OAMAddr: {:02X} ADDR: {:04X}",
            self.status, self.ctrl, self.mask, self.oam.addr, self.vram.addr
        );
    }

    pub fn debug_extract_palettes(&mut self, out: &mut [[[u8; 4]; 4]; 8], nes: *mut Nes) {
        for palette_idx in 0..8 {
            for color_idx in 0..4 {
                let system_color_idx =
                    self.debug_read(ADDR_PAL_START + 4 * palette_idx + color_idx, nes) as usize;
                out[palette_idx as usize][color_idx as usize][0..3]
                    .copy_from_slice(NES_COLOR_PALETTE[system_color_idx].as_slice());
                out[palette_idx as usize][color_idx as usize][3] = 255;
            }
        }
    }

    pub fn debug_extract_sprite_canvas(
        &mut self,
        bank_num: u8,
        palette_idx: u16,
        overwritte_custom_palette: Option<[u8; 4]>,
        nes: *mut Nes,
        out: &mut [u8; DBG_CANVAS_SIZE],
    ) {
        let bank_offset = (bank_num as u16 * 0x1000) & 0x1FFF;
        let palette_idx = palette_idx & 0x03;
        for i in 0..16 {
            for j in 0..16 {
                let offset = (16 * (j * 16 + i)) + bank_offset;
                for row in 0..8 {
                    let tile_lsb = self.debug_read(offset + row, nes);
                    let tile_msb = self.debug_read(offset + row + 8, nes);

                    for col in 0..8 {
                        let bit_l = (tile_lsb >> (7 - col)) & 0x01;
                        let bit_h = (tile_msb >> (7 - col)) & 0x01;

                        let pattern_col = bit_h << 1 | bit_l;

                        let canvas_offset_x = (i * 8 + col) as usize;
                        let canvas_offset_y = (j * 8 + row) as usize;

                        let pixel_offset =
                            4 * (canvas_offset_y * DBG_CANVAS_WIDTH + canvas_offset_x);

                        let color_idx = if pattern_col != 0 {
                            self.debug_read(
                                ADDR_PAL_START + 4 * palette_idx + pattern_col as u16,
                                nes,
                            ) as usize
                        } else {
                            self.debug_read(ADDR_PAL_START, nes) as usize
                        };

                        match overwritte_custom_palette {
                            None => {
                                out[pixel_offset..(pixel_offset + 3)].copy_from_slice(
                                    NES_COLOR_PALETTE[color_idx & 0x3f].as_slice(),
                                );
                                out[pixel_offset + 3] = 255;
                            }
                            Some(p) => {
                                out[pixel_offset..(pixel_offset + 3)].copy_from_slice(
                                    NES_COLOR_PALETTE[p[(pattern_col as usize) & 3] as usize]
                                        .as_slice(),
                                );
                                out[pixel_offset + 3] = 255;
                            }
                        }
                    }
                }
            }
        }
    }
}

const NES_COLOR_PALETTE: [[u8; 3]; 64] = [
    [0x54, 0x54, 0x54],
    [0x00, 0x1E, 0x74],
    [0x08, 0x10, 0x90],
    [0x30, 0x00, 0x88],
    [0x44, 0x00, 0x64],
    [0x50, 0x00, 0x30],
    [0x40, 0x04, 0x00],
    [0x30, 0x18, 0x00],
    [0x12, 0x2C, 0x00],
    [0x00, 0x32, 0x00],
    [0x00, 0x38, 0x00],
    [0x00, 0x30, 0x4C],
    [0x00, 0x28, 0x64],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x98, 0x96, 0x98],
    [0x08, 0x4C, 0xC4],
    [0x30, 0x3C, 0xE4],
    [0x5C, 0x1E, 0xE4],
    [0x88, 0x14, 0xB4],
    [0xA0, 0x14, 0x68],
    [0x9E, 0x20, 0x10],
    [0x7A, 0x40, 0x00],
    [0x48, 0x5C, 0x00],
    [0x16, 0x68, 0x00],
    [0x00, 0x6C, 0x00],
    [0x00, 0x66, 0x44],
    [0x00, 0x5C, 0x94],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xEC, 0xEE, 0xEC],
    [0x4C, 0x9A, 0xEC],
    [0x78, 0x8C, 0xEC],
    [0xB4, 0x6C, 0xEC],
    [0xE4, 0x5C, 0xEC],
    [0xEC, 0x54, 0xB4],
    [0xEC, 0x6A, 0x56],
    [0xD6, 0x86, 0x10],
    [0xAB, 0xA2, 0x00],
    [0x72, 0xB4, 0x00],
    [0x38, 0xB8, 0x00],
    [0x1A, 0xB4, 0x5C],
    [0x1A, 0xAA, 0xB4],
    [0x40, 0x40, 0x40],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xEC, 0xEE, 0xEC],
    [0xA8, 0xCC, 0xEC],
    [0xBC, 0xBC, 0xEC],
    [0xD4, 0xB2, 0xEC],
    [0xEC, 0xAE, 0xEC],
    [0xEC, 0xAE, 0xD4],
    [0xEC, 0xB4, 0xB4],
    [0xE4, 0xC4, 0x90],
    [0xCC, 0xD2, 0x78],
    [0xB4, 0xDE, 0x78],
    [0x98, 0xE2, 0x90],
    [0x8A, 0xE2, 0xBC],
    [0x8C, 0xDA, 0xEC],
    [0x9C, 0x96, 0x9C],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
];

pub mod ppu_flags {
    // =========================================================================
    // 1. PPUCTRL ($2000) - Control Register Flags
    // =========================================================================
    /// Base nametable address (0 = $2000; 1 = $2400; 2 = $2800; 3 = $2C00)
    pub const CTRL_NAMETABLE_MASK: u8 = 0x03; // Bits 0-1
    /// VRAM address increment per CPU read/write of PPUDATA (0: add 1, going horizontally; 1: add 32, going vertically)
    pub const CTRL_VRAM_INCREMENT: u8 = 0x04; // Bit 2
    /// Sprite pattern table address for 8x8 sprites (0: $0000; 1: $1000; ignored in 8x16 mode)
    pub const CTRL_SPRITE_PATTERN: u8 = 0x08; // Bit 3
    /// Background pattern table address (0: $0000; 1: $1000)
    pub const CTRL_BG_PATTERN: u8 = 0x10; // Bit 4
    /// Sprite size (0: 8x8 pixels; 1: 8x16 pixels)
    pub const CTRL_SPRITE_SIZE: u8 = 0x20; // Bit 5
    /// PPU master/slave select (0: read backdrop from EXT pins; 1: output backdrop on EXT pins)
    pub const CTRL_MASTER_SLAVE: u8 = 0x40; // Bit 6
    /// Generate an NMI at the start of the vertical blanking interval (0: off; 1: on)
    pub const CTRL_GENERATE_NMI: u8 = 0x80; // Bit 7

    // =========================================================================
    // 2. PPUMASK ($2001) - Rendering Mask Register Flags
    // =========================================================================
    /// Grayscale mode (0: normal color; 1: produce a monochrome display)
    pub const MASK_GRAYSCALE: u8 = 0x01; // Bit 0
    /// 1: Show background in leftmost 8 pixels of screen, 0: Hide
    pub const MASK_RENDER_LEFT_BG: u8 = 0x02; // Bit 1
    /// 1: Show sprites in leftmost 8 pixels of screen, 0: Hide
    pub const MASK_RENDER_LEFT_SPR: u8 = 0x04; // Bit 2
    /// Background rendering visibility (0: off; 1: on)
    pub const MASK_RENDER_BG: u8 = 0x08; // Bit 3
    /// Sprite rendering visibility (0: off; 1: on)
    pub const MASK_RENDER_SPRITES: u8 = 0x10; // Bit 4
    /// Emphasize red color channels
    pub const MASK_EMPHASIZE_RED: u8 = 0x20; // Bit 5
    /// Emphasize green color channels
    pub const MASK_EMPHASIZE_GREEN: u8 = 0x40; // Bit 6
    /// Emphasize blue color channels
    pub const MASK_EMPHASIZE_BLUE: u8 = 0x80; // Bit 7

    // =========================================================================
    // 3. PPUSTATUS ($2002) - Status Register Flags
    // =========================================================================
    /// Sprite overflow flag (More than 8 sprites on a single scanline)
    pub const STATUS_SPRITE_OVERFLOW: u8 = 0x20; // Bit 5
    /// Sprite 0 Hit flag (Set when an opaque pixel of sprite 0 overlaps an opaque background pixel)
    pub const STATUS_SPRITE_0_HIT: u8 = 0x40; // Bit 6
    /// Vertical blank flag (0: not in vblank; 1: in vblank section)
    pub const STATUS_VBLANK: u8 = 0x80; // Bit 7
    /// Used to preserve the bottom 5 bits of status which contain open-bus stale data
    pub const STATUS_OPEN_BUS_MASK: u8 = 0x1F; // Bits 0-4

    // =========================================================================
    // 4. VRAM internal register sub-fields (`t_addr` and `v_addr` layout)
    // =========================================================================
    // Loopy Register Layout: yyy NN YYYYY XXXXX
    // ddd: fine Y scroll | NN: nametable select | YYYYY: coarse Y | XXXXX: coarse X

    pub const VRAM_COARSE_X_MASK: u16 = 0x001F; // Bits 0-4
    pub const VRAM_COARSE_Y_MASK: u16 = 0x03E0; // Bits 5-9
    pub const VRAM_NAMETABLE_MASK: u16 = 0x0C00; // Bits 10-11
    pub const VRAM_NAMETABLE_X_MASK: u16 = 0x0400; // Bit 10
    pub const VRAM_NAMETABLE_Y_MASK: u16 = 0x0800; // Bit 11
    pub const VRAM_FINE_Y_MASK: u16 = 0x7000; // Bits 12-14
}
