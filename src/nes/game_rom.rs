const PRG_BANK_SIZE: usize = 16384;
const CHR_BANK_SIZE: usize = 8192;

const NES_MAGIC: u32 = 0x1a53454e; // "NES\x1a"

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum MemoryMirroring {
    Horizontal,
    Vertical,
    OnScreenLO,
    OnScreenHI,
}

#[repr(packed)]
#[derive(Debug)]
struct NesGameHeader {
    magic: u32,
    prg_rom_chunks: u8,
    chr_rom_chunks: u8,
    flag6: u8,
    flag7: u8,
    prg_ram_size: u8,
    tv_system1: u8,
    tv_system2: u8,
    unused: [u8; 5],
}

type Result<T> = std::result::Result<T, ROMErr>;

#[derive(Error, Debug)]
pub enum ROMErr {
    #[error("Could not open file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("{0:?} is not a NES ROM")]
    InvalidMagic(OsString),
    #[error("Mapper {0} is invalid or not supported by the emulator currently.")]
    MapperNotImplemented(u8),
}
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Seek},
    mem,
};

use thiserror::Error;

use crate::nes::{
    game_rom::ROMErr::MapperNotImplemented,
    mapper::{MMC0, MMC3, Mapper},
};

pub fn load_rom(filepath: OsString) -> Result<Box<dyn Mapper>> {
    let mut f = File::open(&filepath)?;
    let mut header = [0u8; size_of::<NesGameHeader>()];
    f.read(header.as_mut_slice())?;
    let header: NesGameHeader = unsafe { mem::transmute(header) };
    log::info!("Loaded {:?}:\n{:#?}", filepath, header);

    if header.magic != NES_MAGIC {
        return Err(ROMErr::InvalidMagic(filepath.to_owned()));
    }

    // Skip trainer if present
    if (header.flag6 & 0b0000_0100) != 0 {
        // Advance cursor by 512
        f.seek(std::io::SeekFrom::Current(512))?;
    }
    let mapper_id = (header.flag6 >> 4) | (header.flag7 & 0x0F);
    let mirror = if (header.flag6 & 1) != 0 {
        MemoryMirroring::Vertical
    } else {
        MemoryMirroring::Horizontal
    };
    log::info!("Mapper_id: {mapper_id}");
    log::info!("Mirroring: {mirror:?}");

    let mut prg_data = Vec::with_capacity(header.prg_rom_chunks as usize * PRG_BANK_SIZE);
    let mut chr_data = Vec::with_capacity(header.chr_rom_chunks as usize * CHR_BANK_SIZE);

    unsafe {
        prg_data.set_len(header.prg_rom_chunks as usize * PRG_BANK_SIZE);
        chr_data.set_len(header.chr_rom_chunks as usize * CHR_BANK_SIZE);
    }

    f.read_exact(prg_data.as_mut_slice())?;
    f.read_exact(chr_data.as_mut_slice())?;

    Ok(create_mapper(
        mapper_id,
        header.prg_rom_chunks as usize,
        header.prg_rom_chunks as usize,
        header.prg_ram_size as usize,
        prg_data,
        chr_data,
        mirror,
    )?)
}

#[allow(unused_variables)]
fn create_mapper(
    mapper_id: u8,
    prg_banks: usize,
    chr_banks: usize,
    prg_ram_size: usize,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: MemoryMirroring,
) -> Result<Box<dyn Mapper>> {
    match mapper_id {
        0 => Ok(Box::new(MMC0::new(
            prg_rom,
            prg_banks,
            prg_ram_size,
            chr_rom,
            mirroring,
        ))),
        4 => Ok(Box::new(MMC3::new(
            prg_rom,
            prg_banks,
            chr_banks,
            prg_ram_size,
            chr_rom,
            mirroring
        ))),
        _ => Err(MapperNotImplemented(mapper_id)),
    }
}
