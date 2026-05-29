use std::{cell::RefCell, rc::Rc};

use raylib::ffi::GuiListViewProperty::SCROLLBAR_WIDTH;
use thiserror::Error;

use crate::{
    bus,
    nes::{
        NesEvent,
        bus_cpu::{BusCpu, BusCpuError},
        game_rom::MemoryMirroring,
        mapper::PpuMapper,
    },
};

const ADDR_CHR_START: u16 = 0x0000;
const ADDR_CHR_END: u16 = 0x1FFF;

const ADDR_NAME_START: u16 = 0x2000;
const ADDR_NAME_END: u16 = 0x3EFF;
const SIZE_NAME_TBL: usize = 0x400;

const ADDR_PAL_START: u16 = 0x3F00;
const ADDR_PAL_END: u16 = 0x3FFF;
const SIZE_PALETTE: usize = 32;

struct BusPpu {
    mapper: Box<dyn PpuMapper>,
    chr_data: Vec<u8>,
    nametables: [u8; 2 * SIZE_NAME_TBL],
    palettes: [u8; SIZE_PALETTE],
    mirroring: MemoryMirroring,
    fake_value: u8,
}

impl BusPpu {
    fn new(mapper: Box<dyn PpuMapper>, chr_data: Vec<u8>, mirroring: MemoryMirroring) -> Self {
        BusPpu {
            mapper,
            chr_data,
            nametables: [0; 2 * SIZE_NAME_TBL],
            palettes: [0; SIZE_PALETTE],
            mirroring,
            fake_value: 0,
        }
    }

    fn resolve_addr(&mut self, addr: u16) -> &mut u8 {
        let mut mapped_addr = addr & 0x3FFF;
        // Mapper only acts on CHR-ROM data
        if mapped_addr <= ADDR_CHR_END {
            mapped_addr = self.mapper.ppu_map_addr(mapped_addr);
            return self.chr_data.get_mut(mapped_addr as usize).unwrap();
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
                addr_byte += match self.mirroring {
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
                        panic!("Mirroring not implemented: {:?}", self.mirroring)
                    }
                };
                return self.nametables.get_mut(addr_byte as usize).unwrap();
            }
            ADDR_PAL_START..=ADDR_PAL_END => {
                mapped_addr &= (SIZE_PALETTE as u16) - 1;
                let mut palette_idx = mapped_addr & (SIZE_PALETTE as u16- 1);
                if palette_idx == 0x0010 || palette_idx == 0x0014 || palette_idx == 0x0018 || palette_idx == 0x001C {
                    palette_idx &= 0x000F;
                }
                return self.palettes.get_mut(palette_idx as usize).unwrap();
            }
            _ => {}
        }
        unreachable!();
    }
    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> u8 {
        *self.resolve_addr(addr)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) {
        *self.resolve_addr(addr) = data;
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
    
    scanline: isize,
    cycle: usize,
    
    screen_data: [u8; SCREEN_SIZE],
    
    ctrl: u8,
    status: u8,
    mask: u8,
    fine_x: u8,
    vram: IVRAM,
    oam: IOAM,

    bg_regs: BGSRegs,

    bus_ppu: BusPpu,
}

#[derive(Default)]
struct BGSRegs {
    shift_pat_low: u16,
    shift_pat_high: u16,

    shift_att_low: u16,
    shift_att_high: u16,

    latch_name: u8,
    latch_att: u8,
    latch_pat_lo: u8,
    latch_pat_hi: u8,
}

#[derive(Default)]
struct IVRAM {
    addr: u16,
    t_addr: u16,
    // Set Low: High byte, Set High: Low Byte
    latch: bool,
    delayed_data: u8,
}

struct IOAM {
    offset: usize,
    mem: [u8; 256],
}

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

    OAMDMACopy = 0x4014,
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

use PpuRegister::*;

use ppu_flags::*;

#[derive(Error, Debug)]
pub enum PpuError {
    #[error("Could not read/write to PPU registers on Cpu bus: {0}")]
    RegisterMemError(#[from] BusCpuError),

    #[error("Address ${0:04X} inside region was not mirrored")]
    AddrUnMirrored(u16),

    #[error("Address ${0:04X} was not mapped to any device.")]
    UnimplementedAddr(u16),
}

type Result<T> = std::result::Result<T, PpuError>;

// Main renderring implementation block
impl Ppu {
    pub fn clock(&mut self) {
        self.frame_complete = false;
        // Background register manipulation timeline.
        match self.scanline {
            -1..=239 => self.general_line_routine(),
            240 => {
                // Post render line
            }
            241 => {
                // V-Blank line
                if self.cycle == 1 {
                    self.status |= STATUS_VBLANK;
                    
                    if (self.ctrl & 0x80) != 0 { 
                        self.nmi_signal = true;
                    }
                }

            },
            242..=260 => {
                // V-Blank stage
                // Let the cpu do its thing
            }
            _ => unreachable!(),
        }


        // Get colors from registers
        /*if (self.mask & MASK_RENDER_BG) != 0 */{
            let (pal_idx, col_idx) = self.extract_colors_from_registers();
            let system_color_idx = self.bus_ppu.read(ADDR_PAL_START + (4 * pal_idx as u16) + col_idx as u16);
            // FINALLY 
            let rbga_color = NES_COLOR_PALETTE[system_color_idx as usize];
            self.set_pixel(self.cycle, self.scanline, rbga_color);
        }


        self.cycle += 1;
        if self.cycle > CYCLE_WIDTH {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > SCANLINE_HEIGHT {
                self.scanline = -1;
                self.frame_complete = true;
            }
        }
    }

    fn general_line_routine(&mut self) {
        // Guard
        if !self.is_rendering_enabled() {
            return;
        }

        match self.cycle {
            0 => {}
            1..=256 => {
                if self.scanline == -1 && self.cycle == 1 {
                    self.status &= !(STATUS_VBLANK | STATUS_SPRITE_0_HIT | STATUS_SPRITE_OVERFLOW);
                }
                self.process_bg_step();
                if self.cycle == 256 {
                    self.increment_scroll_y();
                }
            }
            257 => self.copy_scroll_x(),
            258..=320 => {
                // Unused NT reads

                if self.scanline == -1 && self.cycle >= 280 && self.cycle <= 304 {
                    self.copy_scroll_y();
                }
            }
            321..=336 => {
                self.process_bg_step();
            }
            // Dummy NT reads
            337..=340 => {
                let dummy_nt_addr = 0x2000 | (self.vram.addr & 0x0FFF);
                // Although the data is discarded, mappers can syncronize stuff through it 
                let _ = self.bus_ppu.read(dummy_nt_addr); 
            }
            _ => unreachable!(),
        }
    }

    fn process_bg_step(&mut self) {
        self.shift_bg_registers();
        // Process each of the 8 sub-cycles
        let sub_cycle = (self.cycle - 1) % 8;
        match sub_cycle {
            0 => {}
            // Name table read
            1 => {
                let nt_addr = 0x2000
                    | (self.vram.addr & (VRAM_COARSE_X_MASK
                        | VRAM_COARSE_Y_MASK
                        | VRAM_NAMETABLE_MASK));
                self.bg_regs.latch_name = self.bus_ppu.read(nt_addr)
            }
            2 => {}
            // Attribute table read
            3 => {
                // Grab attribute byte
                let v = self.vram.addr;
                // v & 0xC00 -> Name table
                // v >> 4 & 0x38 => Coarse_y / 4
                // v >> 2 & 0x7  => Coarse_x / 4
                let attr_addr =
                    0x23C0 | (v & VRAM_NAMETABLE_MASK) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
                let attr_byte = self.bus_ppu.read(attr_addr);

                let coarse_x = v & VRAM_COARSE_X_MASK;
                let coarse_y = (v & VRAM_COARSE_Y_MASK) >> 5;

                let shift = ((coarse_y & 2) << 1) | (coarse_x & 2);
                self.bg_regs.latch_att = (attr_byte >> shift) & 0x03;
            }
            4 => {}
            // Pattern low read
            5 => {
                let pattern_table_base = if (self.ctrl & CTRL_BG_PATTERN) != 0 {
                    0x1000
                } else {
                    0x0000
                };
                let fine_y = (self.vram.addr & VRAM_FINE_Y_MASK) >> 12;
                let tile_id = self.bg_regs.latch_name as u16;

                let pat_low_addr = pattern_table_base + (tile_id << 4) + fine_y;
                self.bg_regs.latch_pat_lo = self.bus_ppu.read(pat_low_addr);
            }
            6 => {}
            // Pattern hi read
            7 => {
                // Repeat lo but for high
                let pattern_table_base = if (self.ctrl & CTRL_BG_PATTERN) != 0 {
                    0x1000
                } else {
                    0x0000
                };
                let fine_y = (self.vram.addr & VRAM_FINE_Y_MASK) >> 12;
                let tile_id = self.bg_regs.latch_name as u16;

                let pat_high_addr = pattern_table_base + (tile_id << 4) + fine_y;
                self.bg_regs.latch_pat_hi = self.bus_ppu.read(pat_high_addr);

                // finally
                self.load_background_shift_registers();
                self.increment_scroll_x();
            }
            _ => unreachable!(),
        }
    }

    fn extract_colors_from_registers(&self) -> (u8, u8) {
        let mux = 0x8000 >> self.fine_x;

        let pixel_bit0 = (self.bg_regs.shift_pat_low & mux) > 0;
        let pixel_bit1 = (self.bg_regs.shift_pat_high & mux) > 0;

        let pixel = (pixel_bit1 as u8) << 1 | (pixel_bit0 as u8);

        let pal_bit0 = (self.bg_regs.shift_att_low & mux) > 0;
        let pal_bit1 = (self.bg_regs.shift_att_high & mux) > 0;

        let palette = (pal_bit1 as u8) << 1 | (pal_bit0 as u8);

        (pixel, palette)
    }
}

// Rendering helpers
impl Ppu {
    #[inline(always)]
    fn increment_scroll_y(&mut self) {
        let mut fine_y = ((self.vram.addr & VRAM_FINE_Y_MASK) >> 12) + 1;
        if fine_y > 7 {
            let mut coarse_y = (self.vram.addr & VRAM_COARSE_Y_MASK) >> 5;
            coarse_y += 1;
            if coarse_y == 30 {
                coarse_y = 0;
                // Flip Name table Y Bit
                self.vram.addr ^= VRAM_NAMETABLE_Y_MASK;
            }
            coarse_y &= 0x1f;
            self.vram.addr = (self.vram.addr & !VRAM_COARSE_Y_MASK) | coarse_y << 5;
        }
        fine_y &= 0x7;
        self.vram.addr = (self.vram.addr & !VRAM_FINE_Y_MASK) | fine_y << 12
    }

    #[inline(always)]
    fn copy_scroll_x(&mut self) {
        // Copy coarse x and nametable x
        let mask = VRAM_COARSE_X_MASK | VRAM_NAMETABLE_X_MASK;
        self.vram.addr = (self.vram.addr & !mask) | (self.vram.t_addr & mask);
    }

    #[inline(always)]
    fn copy_scroll_y(&mut self) {
        // copy scroll y nametable y and fine y
        let mask = VRAM_COARSE_Y_MASK | VRAM_NAMETABLE_Y_MASK | VRAM_FINE_Y_MASK;
        self.vram.addr = (self.vram.addr & !mask) | (self.vram.t_addr & mask);
    }

    #[inline(always)]
    fn increment_scroll_x(&mut self) {
        let mut coarse_x = (self.vram.addr & VRAM_COARSE_X_MASK);
        coarse_x += 1;
        if coarse_x > 31 {
            coarse_x = 0;
            // Flip Name table Y Bit
            self.vram.addr ^= VRAM_NAMETABLE_X_MASK;
        }
        coarse_x &= 0x1f;
        self.vram.addr = (self.vram.addr & !VRAM_COARSE_X_MASK) | coarse_x;
    }

    #[inline(always)]
    pub fn is_rendering_enabled(&self) -> bool {
        // Returns true if either the background bit (0x08) or sprite bit (0x10) is set
        (self.mask & 0x18) != 0
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

// Other Ppu functionalities
impl Ppu {
    pub fn new(mapper: Box<dyn PpuMapper>, chr_data: Vec<u8>, mirroring: MemoryMirroring) -> Self {
        Ppu {
            bus_ppu: BusPpu::new(mapper, chr_data, mirroring),
            screen_data: [0u8; SCREEN_SIZE],
            oam: IOAM {
                mem: [0u8; 256],
                offset: 0,
            },
            vram: IVRAM::default(),
            bg_regs: BGSRegs::default(),
            frame_complete: false,
            nmi_signal: false,
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

        self.ctrl = 0;
        self.mask = 0;
        // It doesnt actually reset
        self.status &= !STATUS_VBLANK;

        self.fine_x = 0;

        self.vram.addr = 0;
        self.vram.t_addr = 0;
        self.vram.delayed_data = 0;
        self.vram.latch = false;

        self.oam.offset;
    }

    pub fn event_cpu_has_written(&mut self, reg: PpuRegister, data: u8) -> Result<()> {
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
                self.oam.offset = data as usize;
            }
            OamData => {
                self.oam.mem[self.oam.offset] = data;
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
                self.bus_ppu.write(self.vram.addr, data);

                // Increment VRAM Addr
                let increment = if (self.ctrl & 0x04) != 0 { 32 } else { 1u16 };
                self.vram.addr = self.vram.addr.wrapping_add(increment);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn event_cpu_has_read(&mut self, reg: PpuRegister) -> Result<u8> {
        match reg {
            Status => {
                let ret = self.status;

                // Side effects
                self.status &= !STATUS_VBLANK; // Clear V-blank flag
                self.vram.latch = false; // Reset latch

                return Ok(ret);
            }
            OamData => {
                return Ok(self.oam.mem[self.oam.offset]);
            }
            VRAMData => {
                let mut fetch = self.vram.delayed_data;
                self.vram.delayed_data = self.bus_ppu.read(self.vram.addr);

                // Reading paletts bypass this delay
                if self.vram.addr > ADDR_PAL_START {
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

    #[inline(always)]
    pub fn get_screen(&self) -> (*const u8, usize) {
        (self.screen_data.as_ptr(), self.screen_data.len())
    }

    #[inline(always)]
    pub fn get_cycle_and_sc(&self) -> (isize, usize) {
        (self.scanline, self.cycle)
    }

    pub fn debug_extract_palettes(&mut self, out: &mut [[[u8; 4]; 4]; 8]) {
        for palette_idx in 0..8 {
            for color_idx in 0..4 {
                let system_color_idx = self
                    .bus_ppu
                    .read(ADDR_PAL_START + 4 * palette_idx + color_idx)
                    as usize;
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
        out: &mut [u8; DBG_CANVAS_SIZE],
    ) {
        let bank_offset = (bank_num as u16 * 0x1000) & 0x1FFF;
        let palette_idx = palette_idx & 0x03;
        for i in 0..16 {
            for j in 0..16 {
                let offset = (16 * (j * 16 + i)) + bank_offset;
                for row in 0..8 {
                    let tile_lsb = self.bus_ppu.read(offset + row);
                    let tile_msb = self.bus_ppu.read(offset + row + 8);

                    for col in 0..8 {
                        let bit_l = (tile_lsb >> (7 - col)) & 0x01;
                        let bit_h = (tile_msb >> (7 - col)) & 0x01;

                        let pattern_col = bit_h << 1 | bit_l;

                        let canvas_offset_x = (i * 8 + col) as usize;
                        let canvas_offset_y = (j * 8 + row) as usize;

                        let pixel_offset =
                            4 * (canvas_offset_y * DBG_CANVAS_WIDTH + canvas_offset_x);

                        let color_idx = if pattern_col != 0 {
                            self.bus_ppu
                                .read(ADDR_PAL_START + 4 * palette_idx + pattern_col as u16)
                                as usize
                        } else {
                            self.bus_ppu.read(ADDR_PAL_START) as usize
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
