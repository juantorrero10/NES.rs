const PRG_BANK_SIZE: usize = 16384;
const CHR_BANK_SIZE: usize = 8192;

const NES_MAGIC: u32 = 0x1a53454e; // "NES\x1a"

pub struct GameROMArtifacts {
    pub cpu_mapper: Box<dyn CpuMapper>,
    pub ppu_mapper: Box<dyn PpuMapper>,
    pub mirroring: MemoryMirroring,
}

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
    #[error("{0} is not a NES ROM")]
    InvalidMagic(String),
    #[error("Mapper {0} is invalid or not supported by the emulator currently.")]
    MapperNotImplemented(u8),
}
use std::{
    fs::File,
    io::{Read, Seek},
    mem,
};

use thiserror::Error;

use crate::nes::{
    game_rom::ROMErr::MapperNotImplemented,
    mapper::{CpuMapper, CpuMapper0, PpuMapper, PpuMapper0},
};

pub fn load_rom(filepath: &str) -> Result<GameROMArtifacts> {
    let mut f = File::open(filepath)?;
    let mut header = [0u8; size_of::<NesGameHeader>()];
    f.read(header.as_mut_slice())?;
    let header: NesGameHeader = unsafe { mem::transmute(header) };
    log::info!("Loaded {}:\n{:#?}", filepath, header);

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

    let (cpu_mapper, ppu_mapper) = create_mappers(
        mapper_id,
        header.prg_rom_chunks as usize,
        header.prg_rom_chunks as usize,
        header.prg_ram_size as usize,
        prg_data,
        chr_data,
    )?;

    Ok(GameROMArtifacts {
        cpu_mapper,
        ppu_mapper,
        mirroring: mirror
    })
}

#[allow(unused_variables)]
fn create_mappers(
    mapper_id: u8,
    prg_banks: usize,
    chr_banks: usize,
    prg_ram_size: usize,
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
) -> Result<(Box<dyn CpuMapper>, Box<dyn PpuMapper>)> {
    match mapper_id {
        0 => Ok({
            let cpum = Box::new(CpuMapper0::new(prg_rom, prg_banks, prg_ram_size));
            let ppum = Box::new(PpuMapper0::new(chr_rom));
            (cpum, ppum)
        }),
        _ => Err(MapperNotImplemented(mapper_id)),
    }
}
