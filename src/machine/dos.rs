use std::collections::VecDeque;

use crate::{
    cpu::{Cpu, Flag},
    machine::{
        InterruptResult, Machine, MachineError, TextVideo,
        console::{PendingInput, PendingRead},
    },
    memory::{SegmentedAddr, read_buffer, read_byte, validate_buffer},
    registers::{Reg8, Reg16},
};

pub(super) struct Dos {
    /// Read line buffer
    pub(crate) console_read_buffer: VecDeque<u8>,
    /// Resource handles
    handles: Vec<Option<Resource>>,
}

impl Dos {
    pub(super) fn new() -> Self {
        let mut dos = Self {
            console_read_buffer: VecDeque::new(),
            handles: vec![
                Some(Resource::Stdin),
                Some(Resource::Stdout),
                Some(Resource::Console),
            ],
        };
        dos.handles.resize(20, None);
        dos
    }

    /// INT 21h, AH=02h: Emulates the DOS output char service
    pub(super) fn output_char_service(
        &self,
        cpu: &Cpu,
        memory: &mut [u8],
        video: &mut TextVideo,
    ) -> Result<InterruptResult, MachineError> {
        let c = cpu.get_reg(Reg8::Dl);
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

        video.write_char(memory, c)?;

        Ok(InterruptResult::ResumeExecution)
    }

    /// INT21h, AH=30h: Emulates the DOS version service
    pub(super) fn dos_version_service(
        &self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, MachineError> {
        // Report DOS version 2.0
        cpu.set_reg(Reg8::Al, 2);
        cpu.set_reg(Reg8::Ah, 0);
        cpu.bx = 0;
        cpu.cx = 0;
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT21h, AH=3Eh: Emulates the Close Handle Service
    pub(super) fn close_handle_service(
        &mut self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, MachineError> {
        let handle_num = cpu.get_reg(Reg16::Bx);
        if !matches!(self.handles.get(handle_num as usize), Some(Some(_))) {
            // Handle is already closed or nonexistent
            cpu.ax = 6;
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        self.handles[handle_num as usize] = None;
        cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT21h, AH=45h: Emulates the Duplicate File Handle Service
    pub(super) fn duplicate_file_handle_service(
        &mut self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, MachineError> {
        let handle_num = cpu.get_reg(Reg16::Bx);
        let Some(Some(resource)) = self.handles.get(handle_num as usize) else {
            // Unknown handle number
            cpu.ax = 6;
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        let Some(pos) = self.handles.iter().position(|h| h.is_none()) else {
            // No open positions in table
            cpu.ax = 4;
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        self.handles[pos] = Some(resource.clone());
        cpu.ax = pos as u16;
        cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT21h, AH=4Ch: Emulates the DOS termination service
    pub(super) fn termination_service(&self, cpu: &Cpu) -> Result<InterruptResult, MachineError> {
        Ok(InterruptResult::Exit((cpu.ax & 0xFF) as u8))
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum Resource {
    Stdin,
    Stdout,
    Console,
}

impl Machine {
    /// Emulates the read one character with echo service (INT 21h, AH=01h)
    pub(super) fn read_char_with_echo_service(&mut self) -> Result<InterruptResult, MachineError> {
        let Some(Some(resource)) = self.dos.handles.get(0) else {
            return Err(MachineError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Stdin | Resource::Console => {
                self.pending_input = Some(PendingInput::DosReadCharWithEcho);
                Ok(InterruptResult::WaitingForInput)
            }
            _ => Err(MachineError::ReadServiceError(
                "Unsupported resource in handle 0".to_string(),
            )),
        }
    }

    /// Emulates the DOS read service (INT21h, AH=3Fh)
    pub(super) fn read_service(&mut self) -> Result<InterruptResult, MachineError> {
        let dest = self.cpu.bx as usize;
        let Some(Some(resource)) = self.dos.handles.get(dest) else {
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
        let dest_buffer = normalize_addr(SegmentedAddr::new(self.cpu.ds, self.cpu.dx));
        // Limit size of write to 0x10000 - offset
        let max_bytes = (self.cpu.cx as usize).min(0x10000 - dest_buffer.offset as usize);
        validate_buffer(&mut self.memory, dest_buffer.to_physical(), max_bytes)?;

        if max_bytes == 0 {
            // Nothing to read
            self.cpu.ax = 0;
            self.cpu.set_flag(Flag::Carry, false);
            return Ok(InterruptResult::ResumeExecution);
        }

        self.pending_input = Some(PendingInput::DosRead(PendingRead {
            destination: dest_buffer,
            max_bytes,
            bytes: Vec::new(),
        }));
        Ok(InterruptResult::WaitingForInput)
    }

    /// Emulates the DOS print string service (INT 21h, AH=09h)
    pub(super) fn print_string_service(&mut self) -> Result<InterruptResult, MachineError> {
        let mut segmented_addr = SegmentedAddr::new(self.cpu.ds, self.cpu.dx);
        let mut buf = vec![];
        let mut found_dollar = false;
        for _ in 0..self.memory.len() {
            let byte = read_byte(&self.memory, segmented_addr.to_physical())?;
            if byte == b'$' {
                found_dollar = true;
                break;
            }
            buf.push(byte);
            segmented_addr = segmented_addr.wrapping_add(1);
        }
        if !found_dollar {
            return Err(MachineError::PrintStringServiceError(
                "$ symbol not found in memory".to_string(),
            ));
        }

        for c in buf {
            self.text_video.write_char(&mut self.memory, c)?;
        }

        // Observed in real DOS
        self.cpu.set_reg(Reg8::Al, 0x24);

        return Ok(InterruptResult::ResumeExecution);
    }

    /// Emulates the DOS flush and read service (INT 21h, AH=0Ch)
    pub(super) fn flush_and_read_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Flush input if handle 0 is a console
        let Some(Some(resource)) = self.dos.handles.get(0) else {
            return Err(MachineError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Stdin | Resource::Console => {
                self.bios_keyboard.clear();
            }
            _ => {}
        };

        let al = self.cpu.get_reg(Reg8::Al);
        return match al {
            0x01 => self.read_char_with_echo_service(),
            _ => Err(MachineError::ReadServiceError(
                "Unsupported mode in 0Ch service".to_string(),
            )),
        };
    }

    /// Emulates the DOS write service (INT 21h, AH=40h)
    pub(super) fn write_service(&mut self) -> Result<InterruptResult, MachineError> {
        let start_pos = normalize_addr(SegmentedAddr::new(self.cpu.ds, self.cpu.dx));
        // Limit size of write to 0x10000 - offset
        let num_bytes = (self.cpu.cx as usize).min(0x10000 - start_pos.offset as usize);
        let dest = self.cpu.bx;
        if dest != 1 {
            return Err(MachineError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        let buffer = read_buffer(&self.memory, start_pos.to_physical(), num_bytes)?;
        for c in &buffer {
            self.text_video.write_char(&mut self.memory, *c)?;
        }

        // Successfully written bytes to AX and clear CF
        self.cpu.ax = num_bytes as u16;
        self.cpu.set_flag(Flag::Carry, false);

        Ok(InterruptResult::ResumeExecution)
    }
}

/// Normalize a segmented address, similar to what DOS 2.0 does, by moving
/// multiples of 16 from the offset to the segment. This allows more room
/// for higher offsets in segmented instructions.
fn normalize_addr(addr: SegmentedAddr) -> SegmentedAddr {
    SegmentedAddr {
        segment: addr.segment.wrapping_add(addr.offset >> 4),
        offset: addr.offset & 0x000F,
    }
}
