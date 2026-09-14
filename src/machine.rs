use core::fmt::{self, Debug};
use std::fs;
use std::io::{self, Write};
use thiserror::Error;

#[cfg(test)]
mod tests;

pub struct Cpu {
    ax: u16,
    bx: u16,
    cx: u16,
    dx: u16,
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

enum StepResult {
    Continue,
    Interrupt(u8),
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
    fn step(&mut self, memory: &mut [u8]) -> Result<StepResult, CpuError> {
        let byte = self.fetch_next_byte_and_advance_ip(memory);
        let mut result = StepResult::Continue;
        // Table 4-13
        match byte {
            // B0-B7: move an 8-bit immediate into an 8-bit register
            0xB0 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, "mov al,", operand);
                // Move the u8 operand into the low byte of AX
                self.ax = operand as u16 | (self.ax & 0xFF00);
            }
            0xB4 => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, "mov ah,", operand);
                // Move the u8 operand into the high byte of AX
                self.ax = ((operand as u16) << 8) | (self.ax & 0b11111111);
            }
            // B8-BF: move a 16-bit immediate into a 16-bit register
            0xBA => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, "mov dx,", operand);
                self.dx = operand;
            }
            0xBB => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, "mov bx,", operand);
                self.bx = operand;
            }
            0xB9 => {
                let operand = self.fetch_next_word_and_advance_ip(memory);
                print_instruction(byte, "mov cx,", operand);
                self.cx = operand;
            }
            // Interrupts
            0xCD => {
                let operand = self.fetch_next_byte_and_advance_ip(memory);
                print_instruction(byte, "int", operand);
                result = StepResult::Interrupt(operand);
            }
            _ => return Err(CpuError::UnsupportedInstruction(byte)),
        }
        self.trace();
        Ok(result)
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
        eprintln!("{:?}", self);
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

#[derive(Error, Debug)]
enum MachineError {
    #[error("Unsupported interrupt: {0:X}")]
    UnsupportedInterrupt(u8),
    #[error("Unsupported service: {0:X}")]
    UnsupportedService(u8),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Write Service error: {0}")]
    WriteServiceError(String),
}

enum InterruptResult {
    ResumeExecution,
    Exit(u8),
}

impl Machine {
    /// Emulates the DOS write service
    fn write_service(&mut self) -> Result<InterruptResult, MachineError> {
        let num_bytes = self.cpu.cx as usize;
        let start_pos = self.cpu.dx as usize;
        let dest = self.cpu.bx;
        if dest != 1 {
            return Err(MachineError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        if let Some(buffer) = self.memory.get(start_pos..start_pos + num_bytes) {
            io::stdout().write_all(b"\x1b[92m")?;
            io::stdout().write_all(buffer)?;
            io::stdout().write_all(b"\x1b[0m")?;
            // Successfully written bytes
            self.cpu.ax = num_bytes as u16;
            // Clear the carry flag (least significant bit of `flags`)
            self.cpu.flags = self.cpu.flags & !0x01;
            Ok(InterruptResult::ResumeExecution)
        } else {
            Err(MachineError::WriteServiceError(
                "Buffer out of bounds".to_string(),
            ))
        }
    }

    /// Emulates the DOS termination service
    fn termination_service(&mut self) -> Result<InterruptResult, MachineError> {
        Ok(InterruptResult::Exit((self.cpu.ax & 0xFF) as u8))
    }

    fn handle_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        match interrupt {
            0x21 => {
                // AH contains the service ID
                let service = ((self.cpu.ax & 0xFF00) >> 8) as u8;
                match service {
                    0x40 => self.write_service(),
                    0x4C => self.termination_service(),
                    _ => Err(MachineError::UnsupportedService(service)),
                }
            }
            _ => Err(MachineError::UnsupportedInterrupt(interrupt)),
        }
    }
}

pub fn main_loop(file_path: &str) -> anyhow::Result<u8> {
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
    loop {
        let result = machine.cpu.step(&mut machine.memory)?;
        match result {
            StepResult::Continue => continue,
            StepResult::Interrupt(interrupt) => {
                eprintln!("Interrupted: {interrupt:X}");
                let interrupt_result = machine.handle_interrupt(interrupt)?;
                machine.cpu.trace();
                match interrupt_result {
                    InterruptResult::ResumeExecution => continue,
                    InterruptResult::Exit(code) => return Ok(code),
                }
            }
        }
    }
}

fn print_instruction(op: u8, instruction: &str, operand: impl Into<u16>) {
    eprintln!(
        "{:#X} \x1b[96m({} {:X}h)\x1b[0m",
        op,
        instruction,
        operand.into()
    );
}
