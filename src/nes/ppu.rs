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

    fn resolve_addr(&mut self, addr: u16) -> Result<&mut u8> {
        let mut mapped_addr = addr & 0x3FFF;
        // Mapper only acts on CHR-ROM data
        if mapped_addr <= ADDR_CHR_END {
            mapped_addr = self.mapper.ppu_map_addr(mapped_addr);
            return self
                .chr_data
                .get_mut(mapped_addr as usize)
                .ok_or(PpuError::AddrUnMirrored(addr));
        }
        match mapped_addr {
            ADDR_NAME_START..=ADDR_NAME_END => {
                // Addresses 0x3000 - 0x3EFF are mirrored to 0x2000 - 0x2EFF
                if mapped_addr > 0x3000 {
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
                        _ => unreachable!(),
                    },
                    _ => {
                        panic!("Mirroring not implemented: {:?}", self.mirroring)
                    }
                };
                return self
                    .nametables
                    .get_mut(addr_byte as usize)
                    .ok_or(PpuError::AddrUnMirrored(addr));
            }
            ADDR_PAL_START..=ADDR_PAL_END => {
                mapped_addr &= (SIZE_PALETTE as u16) - 1;
                return self
                    .palettes
                    .get_mut(mapped_addr as usize)
                    .ok_or(PpuError::AddrUnMirrored(addr));
            }
            _ => {}
        }
        Err(PpuError::UnimplementedAddr(addr))
    }
    #[inline(always)]
    pub fn read(&mut self, addr: u16) -> Result<u8> {
        Ok(*self.resolve_addr(addr)?)
    }

    #[inline(always)]
    pub fn write(&mut self, addr: u16, data: u8) -> Result<()> {
        *self.resolve_addr(addr)? = data;
        Ok(())
    }
}

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 240;
pub const SCREEN_SIZE: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

pub const CYCLE_WIDTH: usize = 340;
pub const SCANLINE_HEIGHT: isize = 260;

pub struct Ppu {
    pub frame_complete: bool,

    scanline: isize,
    cycle: usize,

    screen_data: [u8; SCREEN_SIZE],

    ctrl: u8,
    mask: u8,
    status: u8,
    vram: IVRAM,
    oam: IOAM,

    bus_cpu: Rc<RefCell<BusCpu>>,
    bus_ppu: BusPpu,
}

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

use PpuRegister::*;

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

impl Ppu {
    pub fn new(
        bus_cpu: Rc<RefCell<BusCpu>>,
        mapper: Box<dyn PpuMapper>,
        chr_data: Vec<u8>,
        mirroring: MemoryMirroring,
    ) -> Self {
        Ppu {
            bus_cpu,
            bus_ppu: BusPpu::new(mapper, chr_data, mirroring),
            screen_data: [0u8; SCREEN_SIZE],
            oam: IOAM {
                mem: [0u8; 256],
                offset: 0,
            },
            vram: IVRAM {
                addr: 0,
                latch: false,
                delayed_data: 0,
                t_addr: 0,
            },
            frame_complete: false,
            ctrl: 0,
            mask: 0,
            status: 0,
            cycle: 0,
            scanline: 0,
        }
    }

    pub fn event_cpu_has_written(&mut self, reg: PpuRegister) -> Result<()> {
        match reg {
            Ctrl => {
                self.ctrl = bus!(self).read(Ctrl as u16)?;
            }
            Mask => {
                self.mask = bus!(self).read(Mask as u16)?;
            }
            OamAddr => {
                self.oam.offset = bus!(self).read(OamAddr as u16)? as usize;
            }
            OamData => {
                self.oam.mem[self.oam.offset] = bus!(self).read(OamData as u16)?;
            }
            VRAMAddr => {
                let b = bus!(self).read(VRAMAddr as u16)?;
                if !self.vram.latch {
                    self.vram.t_addr = (((b & 0x3f) as u16) << 8) | (self.vram.t_addr & 0x00FF);
                } else {
                    self.vram.t_addr = (self.vram.t_addr & 0xFF00) | b as u16;
                    self.vram.addr = self.vram.t_addr;
                }
                self.vram.latch ^= true;
            }
            VRAMData => {
                let data = bus!(self).read(VRAMData as u16)?;
                self.bus_ppu.write(self.vram.addr, data)?;

                // Increment VRAM Addr
                let increment = if (self.status & 0x04) != 0 { 32 } else { 1u16 };
                self.vram.addr = self.vram.addr.wrapping_add(increment);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn event_cpu_has_read(&mut self, reg: PpuRegister) -> Result<()> {
        match reg {
            Status => {
                bus!(self).write(Status as u16, self.status)?;

                // Side effects
                self.status &= 0x7F; // Clear V-blank flag
                self.vram.latch = false; // Reset latch
            }
            OamData => {
                bus!(self).write(OamData as u16, self.oam.mem[self.oam.offset])?;
            }
            VRAMData => {
                let mut fetch = self.vram.delayed_data;
                self.vram.delayed_data = self.bus_ppu.read(self.vram.addr)?;

                // Reading paletts bypass this delay
                if self.vram.addr > ADDR_PAL_START {
                    fetch = self.vram.delayed_data;
                }

                bus!(self).write(VRAMData as u16, fetch)?;

                // Increment address
                let increment = if (self.status & 0x04) != 0 { 32 } else { 1u16 };
                self.vram.addr = self.vram.addr.wrapping_add(increment);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn clock(&mut self) {
        let lum = rand::random::<u8>();
        self.set_pixel(self.cycle - 1, self.scanline, [lum, lum, lum]);

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

    #[inline(always)]
    fn set_pixel(&mut self, x: usize, y: isize, data: [u8; 3]) {
        if y < 0 {
            return;
        }
        let y = y as usize;
        if x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT {
            return;
        }
        let offset = 3 * (y * SCREEN_WIDTH + x);
        self.screen_data[offset..(offset + 3)].copy_from_slice(&data);
        self.screen_data[offset + 3] = 255;
    }

    #[inline(always)]
    pub fn get_screen_ptr(&self) -> (*const u8, usize) {
        return (self.screen_data.as_ptr(), SCREEN_SIZE);
    }
}
