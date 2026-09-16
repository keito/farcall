use core::fmt::Debug;
use std::fs;
use std::io::{self, Read, Write};
use thiserror::Error;

use crate::cpu::{Cpu, CpuError, CpuStepResult, Flag};
use crate::memory::{
    MemoryError, PhysAddr, read_buffer, read_byte, validate_buffer, write_buffer, write_byte,
};
use crate::registers::{Reg8, Reg16};

mod console;
#[cfg(test)]
mod tests;

// Owns everything
struct Machine {
    cpu: Cpu,
    memory: Vec<u8>,
    // Handle table
    handles: Vec<Option<Resource>>,
}

#[derive(Clone, PartialEq)]
enum Resource {
    Stdin,
    Stdout,
    Console,
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
    #[error("Video Service Error: {0}")]
    VideoServiceError(String),
    #[error("CPU Error: {0}")]
    CpuError(#[from] CpuError),
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
        let Some(Some(resource)) = self.handles.get(0) else {
            return Err(MachineError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Stdin => io::stdin().read_exact(&mut buf)?,
            Resource::Console => self.read_console_byte(&mut buf)?,
            _ => {
                return Err(MachineError::ReadServiceError(
                    "Unsupported resource in handle 0".to_string(),
                ));
            }
        };
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

    /// Emulates the DOS output char service
    fn output_char_service(&mut self) -> Result<InterruptResult, MachineError> {
        let c = self.cpu.get_reg8(Reg8::Dl);
        let Some(Some(resource)) = self.handles.get(1) else {
            return Err(MachineError::WriteServiceError(
                "Handle 1 is not set".to_string(),
            ));
        };
        // TODO: Support other resources in the future
        if *resource != Resource::Stdout {
            return Err(MachineError::WriteServiceError(
                "Handle 1 must be stdout".to_string(),
            ));
        }
        io::stdout().write_all(&[c])?;
        io::stdout().flush()?;
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS read service
    fn read_service(&mut self) -> Result<InterruptResult, MachineError> {
        let dest = self.cpu.bx as usize;
        let Some(Some(resource)) = self.handles.get(dest) else {
            // Unknown handle
            self.cpu.ax = 6;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        if *resource != Resource::Stdin {
            return Err(MachineError::ReadServiceError(
                "Unsupported resource".to_string(),
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

    /// Emulates the DOS flush and read service
    fn flush_and_read_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Flush input if handle 0 is a console
        let Some(Some(resource)) = self.handles.get(0) else {
            return Err(MachineError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Console => self.flush_console_input()?,
            // TODO: handle STDIN if console-backed
            _ => {}
        };

        let al = self.cpu.get_reg8(Reg8::Al);
        return match al {
            0x01 => self.read_char_with_echo_service(),
            _ => Err(MachineError::ReadServiceError(
                "Unsupported mode in 0Ch service".to_string(),
            )),
        };
    }

    /// Emulates the DOS termination service
    fn termination_service(&mut self) -> Result<InterruptResult, MachineError> {
        Ok(InterruptResult::Exit((self.cpu.ax & 0xFF) as u8))
    }

    /// Emulates the DOS version service
    fn dos_version_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Report DOS version 2.0
        self.cpu.set_reg8(Reg8::Al, 2);
        self.cpu.set_reg8(Reg8::Ah, 0);
        self.cpu.bx = 0;
        self.cpu.cx = 0;
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the Video Service
    fn video_service(&mut self) -> Result<InterruptResult, MachineError> {
        if self.cpu.get_reg8(Reg8::Ah) == 0xf {
            // Get current video mode
            self.cpu.set_reg8(Reg8::Al, 0x3); // 80x25 color text mode
            self.cpu.set_reg8(Reg8::Ah, 80); // 80 columns in decimal
            self.cpu.set_reg8(Reg8::Bh, 0); // active display page
            Ok(InterruptResult::ResumeExecution)
        } else {
            Err(MachineError::VideoServiceError(
                "Unsupported command".to_string(),
            ))
        }
    }

    /// Emulates the Duplicate File Handle Service
    fn duplicate_file_handle_service(&mut self) -> Result<InterruptResult, MachineError> {
        let handle_num = self.cpu.get_reg16(Reg16::Bx);
        let Some(Some(resource)) = self.handles.get(handle_num as usize) else {
            // Unknown handle number
            self.cpu.ax = 6;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        let Some(pos) = self.handles.iter().position(|h| h.is_none()) else {
            // No open positions in table
            self.cpu.ax = 4;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        self.handles[pos] = Some(resource.clone());
        self.cpu.ax = pos as u16;
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the Close Handle Service
    fn close_handle_service(&mut self) -> Result<InterruptResult, MachineError> {
        let handle_num = self.cpu.get_reg16(Reg16::Bx);
        if !matches!(self.handles.get(handle_num as usize), Some(Some(_))) {
            // Handle is already closed or nonexistent
            self.cpu.ax = 6;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        self.handles[handle_num as usize] = None;
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    fn handle_guest_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        self.cpu.enter_interrupt(&mut self.memory, interrupt)?;
        Ok(InterruptResult::ResumeExecution)
    }

    fn handle_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        match interrupt {
            0x10 => self.video_service(),
            0x20 => Ok(InterruptResult::Exit(0)),
            // DOS services
            0x21 => {
                // AH contains the service ID
                match self.cpu.get_reg8(Reg8::Ah) {
                    0x01 => self.read_char_with_echo_service(),
                    0x02 => self.output_char_service(),
                    0x09 => self.print_string_service(),
                    0x0C => self.flush_and_read_service(),
                    0x30 => self.dos_version_service(),
                    0x3E => self.close_handle_service(),
                    0x3F => self.read_service(),
                    0x40 => self.write_service(),
                    0x45 => self.duplicate_file_handle_service(),
                    0x4C => self.termination_service(),
                    service => Err(MachineError::UnsupportedService(service)),
                }
            }
            0x60 => self.handle_guest_interrupt(interrupt),
            _ => Err(MachineError::UnsupportedInterrupt(interrupt)),
        }
    }

    fn step(&mut self) -> Result<MachineStepResult, MachineError> {
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

    fn load_com(&mut self, bytes: &[u8]) -> Result<(), MachineError> {
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

        let segment = 0x1_000; // Segment where we're loading our program
        let segment_size = 0x10_000; // COM segment size (64 KiB)

        // Set up the program segment prefix (PSP) as DOS would
        // INT 20h at 0000h - 0001h
        write_byte(&mut self.memory, PhysAddr::new(segment, 0x0000), 0xCD)?;
        write_byte(&mut self.memory, PhysAddr::new(segment, 0x0001), 0x20)?;

        // Zero word at FFFEh - FFFh
        write_byte(&mut self.memory, PhysAddr::new(segment, 0xFFFE), 0x00)?;
        write_byte(&mut self.memory, PhysAddr::new(segment, 0xFFFF), 0x00)?;
        let reserved_bytes = 2;

        // Initialize registers
        self.cpu.sp = 0xFFFE; // Make room for the 2 bytes above
        self.cpu.cs = segment;
        self.cpu.ds = segment;
        self.cpu.es = segment;
        self.cpu.ss = segment;

        // Load binary into segment at offset 0x100, by convention
        let prog_offset = 0x100;
        if prog_offset + bytes.len() + reserved_bytes > segment_size {
            return Err(MachineError::NotEnoughMemory(
                prog_offset + bytes.len() + reserved_bytes,
                segment_size,
            )
            .into());
        }
        let validated_buffer = validate_buffer(
            &mut self.memory,
            PhysAddr::new(segment, prog_offset as u16),
            bytes.len(),
        )?;
        write_buffer(validated_buffer, bytes)?;
        self.cpu.ip = prog_offset as u16;
        Ok(())
    }
}

pub fn main_loop(file_path: &str) -> anyhow::Result<u8> {
    let memory_size = 0x100_000; // Physical memory: 1 MiB

    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; memory_size],
        handles: vec![
            Some(Resource::Stdin),
            Some(Resource::Stdout),
            Some(Resource::Console),
        ],
    };
    machine.handles.resize(20, None);

    let data: Vec<u8> = fs::read(file_path)?;
    machine.load_com(&data)?;

    machine.cpu.trace();
    loop {
        let result = machine.step()?;
        if let MachineStepResult::Exit(code) = result {
            return Ok(code);
        }
    }
}
