use std::{error::Error};

use thiserror::Error;

use crate::nes::{NesEvent, bus_cpu::IOResult::{OkRead, OkWrite, TriggerEvent}, mapper_base::Mapper, ppu::PpuRegister};

// RAM Constants
const ADDR_RAM_START: u16 = 0x0000;
const ADDR_RAM_END: u16 = 0x1FFF;
const SIZE_RAM: usize = 0x0800;

// PPU register constants
const ADDR_PPU_REG_START: u16 = 0x2000;
const ADDR_PPU_REG_END: u16 = 0x3FFF;
const SIZE_PPU_REG: usize = 0x8;
const ADDITIONAL_PPU_REG: u16 = 0x4014;

// Controler IO constants
const ADDR_IO_START: u16 = 0x4000;
const ADDR_IO_END: u16 = 0x401F;
const SIZE_IO: usize = 0x20;

const ADDR_OPEN_BUS_START: u16 = 0x4020;
const ADDR_OPEN_BUS_END: u16 = 0x5FFF;

const ADDR_MAYBE_SRAM_START: u16 = 0x6000;
const ADDR_MAYBE_SRAM_END: u16 = 0x7FFF;

const ADDR_ROM_START: u16 = 0x8000;
const ADDR_ROM_END: u16 = 0xFFFF;

#[derive(Error, Debug)]
pub enum BusCpuError {
    #[error("Tried to access unmirrored address inside region: ${0:04X}")]
    AddrUnMirrored(u16),
    #[error("Tried to access unimplemented memory range: ${0:04X} ")]
    AddrRangeNotImplemented(u16),
    #[error("{0}")]
    UnknownError(#[from] Box<dyn Error>),
    #[error("The Bus of the CPU couldn't talk to other components as they are not registered.")]
    CommunicationsNotConnected,
}

pub struct BusCpu {
    fake_value: u8,
    open_bus_sim_reg: u8,
    chips: BusCpuChips,
}

pub struct BusCpuChips {
    pub ram: [u8; SIZE_RAM],
    pub debug_ppu_reg: [u8; SIZE_PPU_REG],
    pub debug_controller_io: [u8; SIZE_IO],
    pub oam_transfer_buffer: [u8; 256],
}

type Result<T> = std::result::Result<T, BusCpuError>;

pub enum BusCpuTarget<'a> {
    Memory(&'a mut u8),
    HardwareIntercept {
        // Some memory accesses dont lie behind clean containters. Instead they need a hardware device manage it.
        event: NesEvent,
        // Debuggers should not trigger the events but at the same time have the data updated
        // All interceptions requiere a mirrored simulated memory read/write in order to maintain them fresh.
        debug_simulated_target: *mut u8,
    },
    OpenBus
}

pub enum IOResult {
    OkWrite,
    OkRead(u8),
    TriggerEvent(NesEvent, *mut u8),
}

impl<'a> BusCpuTarget<'a> {
    fn unwrap_memory(self) -> &'a mut u8 {
        if let Memory(re) = self {
            return re;
        }
        panic!("It is a hardware intercept");
    }
}

use BusCpuTarget::*;

impl BusCpu {
    pub fn new() -> BusCpu {
        // TODO: Cart
        BusCpu {
            chips: BusCpuChips {
                ram: [0u8; SIZE_RAM],
                // Just debug, for dissasembler and memory viewer
                debug_ppu_reg: [0u8; SIZE_PPU_REG],
                debug_controller_io: [0u8; SIZE_IO],
                oam_transfer_buffer: [0u8; 256],
            },

            open_bus_sim_reg: 0,

            fake_value: 0,
        }
    }

    fn bus_operation<'a>(
        &'a mut self,
        mut addr: u16,
        write: bool,
        data: u8,
        trigger_events: bool,
        mapper: &'a mut Box<dyn Mapper>
    ) -> Result<BusCpuTarget<'a>> {
        let input_addr = addr;
        // Map address

        if mapper.cpu_need_intercept(addr) {
            return Ok(mapper.cpu_intercept(addr, &mut self.chips, data, write));
        }

        match input_addr {
            ADDR_RAM_START..=ADDR_RAM_END => {
                addr &= SIZE_RAM as u16 - 1;
                return Ok(Memory(
                    self.chips
                        .ram
                        .get_mut(addr as usize)
                        .ok_or(BusCpuError::AddrUnMirrored(input_addr))?,
                ));
            }
            ADDR_PPU_REG_START..=ADDR_PPU_REG_END => {
                addr &= SIZE_PPU_REG as u16 - 1;

                let debug_simulated_target = self
                    .chips
                    .debug_ppu_reg
                    .get_mut(addr as usize)
                    .ok_or(BusCpuError::AddrUnMirrored(input_addr))?;

                if trigger_events {
                    return if write {
                        Ok(HardwareIntercept {
                            event: NesEvent::PpuRegWrittenTo(PpuRegister::from_0_to_7(addr), data),
                            debug_simulated_target,
                        })
                    } else {
                        Ok(HardwareIntercept {
                            event: NesEvent::PpuRegReadFrom(PpuRegister::from_0_to_7(addr)),
                            debug_simulated_target,
                        })
                    };
                }
                // Debug only
                Ok(Memory(debug_simulated_target))
            }
            ADDR_IO_START..=ADDR_IO_END => {
                macro_rules! get_simulated {
                    ($addr:expr) => {
                        self.chips
                            .debug_controller_io
                            .get_mut((addr & 0x1f) as usize)
                            .ok_or(BusCpuError::AddrUnMirrored(input_addr))?
                    };
                }
                if trigger_events {
                    if write {
                        return match addr {
                            0x4000..=0x4013 | 0x4015 | 0x4017 => Ok(HardwareIntercept {
                                event: NesEvent::ApuWrittenTo(addr, data),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            // OAM DMA transfer
                            0x4014 => {
                                Ok(HardwareIntercept {
                                    event: NesEvent::OAMDMATransfer(data),
                                    debug_simulated_target: get_simulated!(addr),
                                })
                            }
                            0x4016 => Ok(HardwareIntercept {
                                event: NesEvent::ControllerWrittenTo(data),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            _ => Ok(Memory(&mut self.open_bus_sim_reg)),
                        };
                    } else {
                        return match addr {
                            0x4000..=0x4014 => {
                                self.fake_value = 0;
                                Ok(Memory(&mut self.fake_value))
                            }
                            0x4015 => Ok(HardwareIntercept {
                                event: NesEvent::ApuReadFrom(addr),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            0x4016 | 0x4017 => Ok(HardwareIntercept {
                                event: NesEvent::ControllerReadFrom(addr),
                                debug_simulated_target: get_simulated!(addr),
                            }),
                            _ => Ok(Memory(&mut self.open_bus_sim_reg)),
                        };
                    }
                }

                Ok(Memory(get_simulated!(addr)))
            }
            _ => Ok(Memory(&mut self.open_bus_sim_reg)),
        }
    }

    #[inline(always)]
    pub fn read<'a>(&'a mut self, addr: u16, mapper: &'a mut Box<dyn Mapper>) -> Result<IOResult> {
        let data_ret;
        match self.bus_operation(addr, false, 0, true, mapper)? {
            // Handle it here if it doesnt need further intervention
            Memory(r) => {
                data_ret = *r;
                self.open_bus_sim_reg = data_ret;
                Ok(OkRead(data_ret))
            }
            HardwareIntercept {
                event,
                debug_simulated_target,
            } => {
                Ok(TriggerEvent(event, debug_simulated_target))
            }
            OpenBus => {
                Ok(OkRead(self.open_bus_sim_reg))
            }
        }
    }

    /// Read 16bit number (little endian, LSB first)
    #[inline(always)]
    pub fn read16<'a>(&'a mut self, addr: u16, mapper: &'a mut Box<dyn Mapper>) -> Result<(IOResult, IOResult)> {
        let result_lo = self.read(addr, mapper)?;
        let result_hi = self.read(addr + 1, mapper)?;
        Ok((result_lo, result_hi))
    }

    #[inline(always)]
    pub fn write<'a>(&'a mut self, addr: u16, data: u8, mapper: &'a mut Box<dyn Mapper>) -> Result<IOResult> {
        match self.bus_operation(addr, true, data, true, mapper)? {
            Memory(r) => {
                *r = data;
                self.open_bus_sim_reg = data;
                Ok(OkWrite)
            }
            HardwareIntercept {
                event,
                debug_simulated_target,
            } => {
                Ok(TriggerEvent(event, debug_simulated_target))
            }
            OpenBus => Ok(OkWrite)
        }
    }

    #[inline(always)]
    pub fn debug_read<'a>(&mut self, addr: u16, mapper: &'a mut Box<dyn Mapper>) -> Result<u8> {
        return Ok(*(self.bus_operation(addr, false, 0, false, mapper)?.unwrap_memory()));
    }

    #[inline(always)]
    pub fn debug_read16<'a>(&mut self, addr: u16, mapper: &'a mut Box<dyn Mapper>) -> Result<u16> {
        Ok((self.debug_read(addr, mapper)? as u16) | (self.debug_read(addr + 1, mapper)? as u16) << 8)
    }

    pub fn debug_write(&mut self, addr: u16, data: u8,  mapper: &mut Box<dyn Mapper>) {
        let _ = self.bus_operation(addr, true, data, false, mapper);
    }
}
