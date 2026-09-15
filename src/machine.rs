use core::fmt::Debug;
use std::fs;
use std::io::{self, Read, Write};
use thiserror::Error;

use crate::cpu::{Cpu, CpuStepResult, Flag};
use crate::memory::{
    MemoryError, PhysAddr, read_buffer, read_byte, validate_buffer, write_buffer, write_byte,
};
use crate::registers::Reg8;

#[cfg(test)]
mod tests;

// Owns everything
struct Machine {
    cpu: Cpu,
    memory: Vec<u8>,
}

#[derive(Error, Debug)]
enum MachineError {
    #[error("Not enough memory; offset {0} exceeds {1} bytes")]
    NotEnoughMemory(usize, usize),
    #[error("Unsupported interrupt: {0:X}")]
    UnsupportedInterrupt(u8),
    #[error("Unsupported service: {0:X}")]
    UnsupportedService(u8),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Read Service error: {0}")]
    ReadServiceError(String),
    #[error("Write Service error: {0}")]
    WriteServiceError(String),
    #[error("Print String Service error: {0}")]
    PrintStringServiceError(String),
    #[error("Memory Error: {0}")]
    MemoryError(#[from] MemoryError),
}

enum InterruptResult {
    ResumeExecution,
    Exit(u8),
}

enum MachineStepResult {
    Continue,
    Exit(u8),
}

impl Machine {
    /// Emulates the read one character with echo service
    fn read_char_with_echo_service(&mut self) -> Result<InterruptResult, MachineError> {
        let mut buf = [0u8; 1];
        io::stdin().read_exact(&mut buf)?;
        io::stdout().write_all(&buf)?;
        io::stdout().flush()?;
        self.cpu.set_reg8(Reg8::Al, buf[0]);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS write service
    fn write_service(&mut self) -> Result<InterruptResult, MachineError> {
        let num_bytes = self.cpu.cx as usize;
        let start_pos = PhysAddr::new(self.cpu.ds, self.cpu.dx);
        let dest = self.cpu.bx;
        if dest != 1 {
            return Err(MachineError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        let buffer = read_buffer(&self.memory, start_pos, num_bytes)?;
        io::stdout().write_all(b"\x1b[92m")?;
        io::stdout().write_all(&buffer)?;
        io::stdout().write_all(b"\x1b[0m")?;
        io::stdout().flush()?;
        // Successfully written bytes
        self.cpu.ax = num_bytes as u16;
        // Clear the carry flag
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS print string service
    fn print_string_service(&mut self) -> Result<InterruptResult, MachineError> {
        let mut phys_addr = PhysAddr::new(self.cpu.ds, self.cpu.dx);
        let mut buf = vec![];
        let mut found_dollar = false;
        for _ in 0..self.memory.len() {
            let byte = read_byte(&self.memory, phys_addr)?;
            if byte == b'$' {
                found_dollar = true;
                break;
            }
            buf.push(byte);
            phys_addr = phys_addr.wrapping_add(1);
        }
        if !found_dollar {
            return Err(MachineError::PrintStringServiceError(
                "$ symbol not found in memory".to_string(),
            ));
        }

        io::stdout().write_all(b"\x1b[92m")?;
        io::stdout().write_all(&buf)?;
        io::stdout().write_all(b"\x1b[0m")?;
        io::stdout().flush()?;

        // Observed in real DOS
        self.cpu.set_reg8(Reg8::Al, 0x24);

        return Ok(InterruptResult::ResumeExecution);
    }

    /// Emulates the DOS read service
    fn read_service(&mut self) -> Result<InterruptResult, MachineError> {
        let dest = self.cpu.bx;
        if dest != 0 {
            return Err(MachineError::ReadServiceError(
                "Unsupported source in bx".to_string(),
            ));
        }
        let max_bytes = self.cpu.cx as usize;
        let dest_buffer = PhysAddr::new(self.cpu.ds, self.cpu.dx);
        let validated_buffer = validate_buffer(&mut self.memory, dest_buffer, max_bytes)?;

        let mut temp_buf = vec![0u8; max_bytes];
        let num_bytes = io::stdin().read(&mut temp_buf)?;
        temp_buf.truncate(num_bytes);
        write_buffer(validated_buffer, &temp_buf)?;

        // Write actual bytes read to AX on success
        self.cpu.ax = num_bytes as u16;
        // Clear carry on success
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS termination service
    fn termination_service(&mut self) -> Result<InterruptResult, MachineError> {
        Ok(InterruptResult::Exit((self.cpu.ax & 0xFF) as u8))
    }

    fn handle_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        match interrupt {
            0x20 => Ok(InterruptResult::Exit(0)),
            0x21 => {
                // AH contains the service ID
                let service = ((self.cpu.ax & 0xFF00) >> 8) as u8;
                match service {
                    0x01 => self.read_char_with_echo_service(),
                    0x09 => self.print_string_service(),
                    0x3F => self.read_service(),
                    0x40 => self.write_service(),
                    0x4C => self.termination_service(),
                    _ => Err(MachineError::UnsupportedService(service)),
                }
            }
            _ => Err(MachineError::UnsupportedInterrupt(interrupt)),
        }
    }

    fn step(&mut self) -> anyhow::Result<MachineStepResult> {
        let result = self.cpu.step(&mut self.memory)?;
        match result {
            CpuStepResult::Continue => Ok(MachineStepResult::Continue),
            CpuStepResult::Interrupt(interrupt) => {
                eprintln!("Interrupted: {interrupt:X}");
                let interrupt_result = self.handle_interrupt(interrupt)?;
                self.cpu.trace();
                match interrupt_result {
                    InterruptResult::ResumeExecution => Ok(MachineStepResult::Continue),
                    InterruptResult::Exit(code) => Ok(MachineStepResult::Exit(code)),
                }
            }
        }
    }
}

pub fn main_loop(file_path: &str) -> anyhow::Result<u8> {
    /*
    Offset within segment
    0000
          PSP: 256 bytes
    00FF
    ...
    0100
          Contents of COM file
          ...unused space...
          ^ Stack grows this way
    FFFE Zero return address
    FFFF Zero return address
    */
    let memory_size = 0x100_000; // Physical memory: 1 MiB
    let segment = 0x1_000; // Segment where we're loading our program
    let segment_size = 0x10_000; // COM segment size (64 KiB)

    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; memory_size],
    };

    // Set up the program segment prefix (PSP) as DOS would
    // INT 20h at 0000h - 0001h
    write_byte(&mut machine.memory, PhysAddr::new(segment, 0x0000), 0xCD)?;
    write_byte(&mut machine.memory, PhysAddr::new(segment, 0x0001), 0x20)?;

    // Zero word at FFFEh - FFFh
    write_byte(&mut machine.memory, PhysAddr::new(segment, 0xFFFE), 0x00)?;
    write_byte(&mut machine.memory, PhysAddr::new(segment, 0xFFFF), 0x00)?;
    let reserved_bytes = 2;

    // Initialize registers
    machine.cpu.sp = 0xFFFE; // Make room for the 2 bytes above
    machine.cpu.cs = segment;
    machine.cpu.ds = segment;
    machine.cpu.es = segment;
    machine.cpu.ss = segment;

    // Load binary into segment at offset 0x100, by convention
    let data: Vec<u8> = fs::read(file_path)?;
    let prog_offset = 0x100;
    if prog_offset + data.len() + reserved_bytes > segment_size {
        return Err(MachineError::NotEnoughMemory(
            prog_offset + data.len() + reserved_bytes,
            segment_size,
        )
        .into());
    }
    let validated_buffer = validate_buffer(
        &mut machine.memory,
        PhysAddr::new(segment, prog_offset as u16),
        data.len(),
    )?;
    write_buffer(validated_buffer, &data)?;
    machine.cpu.ip = prog_offset as u16;

    machine.cpu.trace();
    loop {
        let result = machine.step()?;
        if let MachineStepResult::Exit(code) = result {
            return Ok(code);
        }
    }
}
