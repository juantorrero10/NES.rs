use std::{cell::RefCell, error::Error, io::Read, rc::Rc};

use colored::Colorize;
use simple_logger::SimpleLogger;

use crate::nes::{Nes, bus_cpu::BusCpu, cpu::Cpu};


mod nes;

fn dump_and_abort(nes: Nes, e: Box<dyn Error>) -> Box<dyn Error> {
    log::warn!("Failure while clocking CPU, shuting down!");
    println!("{}:\n{:?}", ">>>CPU state dump on failure<<<".bright_black(), nes.cpu);
    println!("{}:\n{:?}", ">>>BUS state dump on failure<<<".bright_black(), nes.bus);
    return e;
}

fn nes_loop(mut nes: Nes) -> Result<Nes, Box<dyn Error>> {
    let stdin = std::io::stdin();
    let mut b = String::new();
    loop {
        b.clear();
        stdin.read_line(&mut b)?;
        if b.trim() == "exit" {break;}
        loop {
            if let Err(e) = nes.cpu.clock() {
                return Err(dump_and_abort(nes, Box::new(e)));
            };
            if nes.cpu.state.cycles_left == 0 {
                break; 
            }
        }
        dbg!(&nes.cpu);
    }
    Ok(nes)
}

fn init() -> Result<Nes, Box<dyn Error>> {
    let mut nes = Nes::new();
    nes.bus.borrow_mut().debug_load_prg(&[0xA2, 0x0A, 0x8E, 0x00, 0x00, 0xA2, 0x03, 0x8E, 0x01, 0x00, 0xAC, 0x00, 0x00, 0xA9, 0x00, 0x18, 0x6D, 0x01, 0x00, 0x88, 0xD0, 0xFA, 0x8D, 0x02, 0x00, 0xEA, 0xEA, 0xEA]);
    nes.cpu.reset()?;
    Ok(nes)
}

fn main() {
    SimpleLogger::new()
    .with_level(log::LevelFilter::Info)
    .with_timestamp_format(time::macros::format_description!("[[[year]-[month]-[day] [hour]:[minute]:[second]]"))
    .with_colors(true)
    .init()
    .expect("Failure: Logger failed");


    match init() {
        Err(e) =>
        eprintln!("{} {}: {}", "Start-up".blue().italic(), "NES Emulator panicked!".red().bold(), e),
        Ok(n) => {
            log::info!("Emulator initialized.");
            match nes_loop(n) {
                Err(e) =>
                println!("{} {}: {}", "Runtime".green().italic(), "NES Emulator panicked!".red().bold(), e),
                Ok(n) => { println!("{:?}", n.cpu) }
            }
        }
    }
}
