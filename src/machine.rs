use core::fmt::{self, Debug};
use std::fs;
use thiserror::Error;

pub struct Cpu {
    ax: u16,
    // Other registers...
    ip: u16,
    flags: u16,
}

// Owns everything
pub struct Machine {
    cpu: Cpu,
    memory: Vec<u8>,
}

#[derive(Error, Debug)]
enum CpuError {
    #[error("Unsupported instruction: {0:X}")]
    UnsupportedInstruction(u8),
}

impl Cpu {
    fn new() -> Self {
        Cpu {
            ax: 0x1234,
            ip: 0x0,
            flags: 0,
        }
    }

    /// Fetch, decode, and execute one instruction
    fn step(&mut self, memory: &mut [u8]) -> Result<(), CpuError> {
        let byte = self.fetch_next_byte_and_advance_ip(memory);
        match byte {
            0xB4 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                println!("0xB4 (mov ah, {:X}h)", operand);
                self.ax = ((operand as u16) << 8) | (self.ax & 0b11111111);
            }
            _ => return Err(CpuError::UnsupportedInstruction(byte)),
        }
        self.trace();
        Ok(())
    }

    fn fetch_next_byte_and_advance_ip(&mut self, memory: &[u8]) -> u8 {
        let byte = memory[self.ip as usize];
        self.ip += 1;
        byte
    }

    fn trace(&self) {
        println!("{:?}", self);
    }
}

impl Debug for Cpu {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Cpu")
            .field("ax", &format_args!("0x{:X}", self.ax))
            .field("ip", &format_args!("0x{:X}", self.ip))
            .field("flags", &format_args!("0x{:X}", self.flags))
            .finish()
    }
}

pub fn main_loop(file_path: &str) -> anyhow::Result<()> {
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; 512],
    };

    let data: Vec<u8> = fs::read(file_path)?;
    // Load binary into memory at offset 0x100
    // TODO: Check machine.memory is long enough
    machine.memory[0x100..0x100 + data.len()].copy_from_slice(&data);
    // Start executing at 0x100
    machine.cpu.ip = 0x100;

    machine.cpu.trace();
    machine.cpu.step(&mut machine.memory)?;

    Ok(())
}
