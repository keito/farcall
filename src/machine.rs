use core::fmt::{self, Debug};
use std::fs;
use thiserror::Error;

pub struct Cpu {
    ax: u16,
    bx: u16,
    cx: u16,
    dx: u16,
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
            bx: 0x1234,
            cx: 0x1234,
            dx: 0x1234,
            ip: 0x0,
            flags: 0,
        }
    }

    /// Fetch, decode, and execute one instruction
    fn step(&mut self, memory: &mut [u8]) -> Result<(), CpuError> {
        let byte = self.fetch_next_byte_and_advance_ip(memory);
        match byte {
            // B0-B7: move an 8-bit immediate into an 8-bit register
            0xB4 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                println!("0xB4 (mov ah, {:X}h)", operand);
                // Move the u8 operand into the high byte of AX
                self.ax = ((operand as u16) << 8) | (self.ax & 0b11111111);
            }
            // B8-BF: move a 16-bit immediate into a 16-bit register
            0xBA => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                println!("0xBA (mov dx, {:X}h)", operand);
                self.dx = operand;
            }
            0xBB => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                println!("0xBB (mov bx, {:X}h)", operand);
                self.bx = operand;
            }
            0xB9 => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                println!("0xB9 (mov cx, {:X}h)", operand);
                self.cx = operand;
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

    fn fetch_next_word_and_advance_ip(&mut self, memory: &[u8]) -> u16 {
        // The next byte is the low-order byte (little endian)
        let lob = self.fetch_next_byte_and_advance_ip(memory);
        // The next byte is the high-order byte
        let hob = self.fetch_next_byte_and_advance_ip(memory);
        // Combine the low and high bytes into a 16-bit value
        (lob as u16) | ((hob as u16) << 8)
    }

    fn trace(&self) {
        println!("{:?}", self);
    }
}

impl Debug for Cpu {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Cpu")
            .field("ax", &format_args!("{:#06X}", self.ax))
            .field("bx", &format_args!("{:#06X}", self.bx))
            .field("cx", &format_args!("{:#06X}", self.cx))
            .field("dx", &format_args!("{:#06X}", self.dx))
            .field("ip", &format_args!("{:#06X}", self.ip))
            .field("flags", &format_args!("{:#06X}", self.flags))
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
    machine.cpu.step(&mut machine.memory)?;
    machine.cpu.step(&mut machine.memory)?;
    machine.cpu.step(&mut machine.memory)?;

    Ok(())
}
