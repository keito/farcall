use crate::{
    cpu::Flag,
    machine::{
        InterruptResult, Machine, MachineError,
        console::{PendingInput, PendingRead},
    },
    memory::{PhysAddr, read_buffer, read_byte, validate_buffer},
    registers::Reg8,
};

#[derive(Clone, PartialEq)]
pub(super) enum Resource {
    Stdin,
    Stdout,
    Console,
}

impl Machine {
    /// Emulates the read one character with echo service (INT 21h, AH=01h)
    pub(super) fn read_char_with_echo_service(&mut self) -> Result<InterruptResult, MachineError> {
        let Some(Some(resource)) = self.handles.get(0) else {
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

    /// Emulates the DOS output char service (INT 21h, AH=02h)
    pub(super) fn output_char_service(&mut self) -> Result<InterruptResult, MachineError> {
        let c = self.cpu.get_reg(Reg8::Dl);
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

        self.text_video.write_char(&mut self.memory, c)?;

        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS read service (INT21h, AH=3Fh)
    pub(super) fn read_service(&mut self) -> Result<InterruptResult, MachineError> {
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
        validate_buffer(&mut self.memory, dest_buffer, max_bytes)?;

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
        let Some(Some(resource)) = self.handles.get(0) else {
            return Err(MachineError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Stdin | Resource::Console => {
                self.keyboard_buffer.clear();
                self.pending_key = None;
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
        let num_bytes = self.cpu.cx as usize;
        let start_pos = PhysAddr::new(self.cpu.ds, self.cpu.dx);
        let dest = self.cpu.bx;
        if dest != 1 {
            return Err(MachineError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        let buffer = read_buffer(&self.memory, start_pos, num_bytes)?;
        for c in &buffer {
            self.text_video.write_char(&mut self.memory, *c)?;
        }

        // Successfully written bytes to AX and clear CF
        self.cpu.ax = num_bytes as u16;
        self.cpu.set_flag(Flag::Carry, false);

        Ok(InterruptResult::ResumeExecution)
    }
}
