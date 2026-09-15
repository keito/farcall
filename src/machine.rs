use core::fmt::Debug;
use std::fs;
use std::io::{self, Write};
use thiserror::Error;

use crate::cpu::{Cpu, CpuStepResult, Flag};

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
    #[error("Write Service error: {0}")]
    WriteServiceError(String),
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
            // Clear the carry flag
            self.cpu.set_flag(Flag::Carry, false);
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
    let memory_size = 512;
    let begin_offset = 0x100;
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; memory_size],
    };

    let data: Vec<u8> = fs::read(file_path)?;
    // Load binary into memory at offset 0x100
    if begin_offset + data.len() > memory_size {
        return Err(MachineError::NotEnoughMemory(begin_offset + data.len(), memory_size).into());
    }
    machine.memory[begin_offset..begin_offset + data.len()].copy_from_slice(&data);
    // Start executing at begin_offset
    machine.cpu.ip = begin_offset as u16;

    machine.cpu.trace();
    loop {
        let result = machine.step()?;
        if let MachineStepResult::Exit(code) = result {
            return Ok(code);
        }
    }
}
