use std::collections::VecDeque;

use crate::{
    cpu::{Cpu, Flag},
    machine::{BiosKeyboard, CgaVideo, InterruptResult, MachineStepResult},
    memory::{MemoryError, SegmentedAddr, read_buffer, read_byte, validate_buffer, write_buffer},
    registers::{Reg8, Reg16},
};

pub(super) struct Dos {
    /// Read line buffer
    pub(crate) console_read_buffer: VecDeque<u8>,
    /// Resource handles
    handles: Vec<Option<Resource>>,
    /// What kind of input we're pending
    pending_input: Option<PendingInput>,
}

impl Dos {
    pub(super) fn new() -> Self {
        let mut dos = Self {
            console_read_buffer: VecDeque::new(),
            pending_input: None,
            handles: vec![
                Some(Resource::Stdin),
                Some(Resource::Stdout),
                Some(Resource::Console),
            ],
        };
        dos.handles.resize(20, None);
        dos
    }

    pub(super) fn has_pending_input(&self) -> bool {
        self.pending_input.is_some()
    }

    pub(super) fn handle_pending_input(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut [u8],
        bios_keyboard: &mut BiosKeyboard,
        video: &mut CgaVideo,
    ) -> Result<MachineStepResult, DosError> {
        let Some(pending_input) = &mut self.pending_input else {
            return Ok(MachineStepResult::Continue);
        };
        match pending_input {
            PendingInput::DosRead(read) => {
                if self.console_read_buffer.is_empty() {
                    // Consume keystrokes until Enter. Move completed line into buffer.
                    while let Some(key) = bios_keyboard.pop() {
                        if key.ascii == Some(b'\r') {
                            // We finished the line
                            read.bytes.push(b'\r');
                            read.bytes.push(b'\n');
                            video.write_char(memory, b'\r')?;
                            video.write_char(memory, b'\n')?;

                            // Write those bytes to the console read buffer
                            self.console_read_buffer.extend(read.bytes.drain(..));
                            break;
                        } else if key.ascii == Some(0x08) {
                            // Backspace
                            if read.bytes.pop().is_some() {
                                video.backspace(memory)?;
                            }
                        } else if let Some(char) = key.ascii {
                            // TODO: ignore non-ASCII characters for now
                            read.bytes.push(char);
                            video.write_char(memory, char)?;
                        }
                    }
                }

                // If it's still empty, wait for more input/an Enter.
                if self.console_read_buffer.is_empty() {
                    return Ok(MachineStepResult::WaitingForInput);
                }

                // Dequeue at most max_bytes bytes from the read buffer to guest mem
                let num_bytes = self.console_read_buffer.len().min(read.max_bytes);
                let buffer = validate_buffer(memory, read.destination.to_physical(), num_bytes)?;
                write_buffer(
                    buffer,
                    &self.console_read_buffer.make_contiguous()[..num_bytes],
                )?;
                self.console_read_buffer.drain(..num_bytes);

                // Write actual bytes read to AX and clear CF on success
                cpu.ax = num_bytes as u16;
                cpu.set_flag(Flag::Carry, false);

                self.pending_input = None;
                return Ok(MachineStepResult::Continue);
            }
            PendingInput::DosReadCharWithEcho => {
                if let Some(key) = bios_keyboard.pop() {
                    if let Some(char) = key.ascii {
                        video.write_char(memory, char)?;
                        cpu.set_reg(Reg8::Al, char);
                        self.pending_input = None;
                        return Ok(MachineStepResult::Continue);
                    }
                }
                return Ok(MachineStepResult::WaitingForInput);
            }
        }
    }

    /// Handles DOS services
    pub(super) fn handle_interrupt(
        &mut self,
        interrupt: u8,
        cpu: &mut Cpu,
        memory: &mut [u8],
        bios_keyboard: &mut BiosKeyboard,
        video: &mut CgaVideo,
    ) -> Result<InterruptResult, DosError> {
        match interrupt {
            // Terminate program service
            0x20 => Ok(InterruptResult::Exit(0)),
            0x21 => match cpu.get_reg(Reg8::Ah) {
                // AH contains the service ID
                0x01 => self.read_char_with_echo_service(),
                0x02 => self.output_char_service(cpu, memory, video),
                0x09 => self.print_string_service(cpu, memory, video),
                0x0C => self.flush_and_read_service(cpu, bios_keyboard),
                0x30 => self.dos_version_service(cpu),
                0x3E => self.close_handle_service(cpu),
                0x3F => self.read_service(cpu, memory),
                0x40 => self.write_service(cpu, memory, video),
                0x45 => self.duplicate_file_handle_service(cpu),
                0x4C => self.termination_service(cpu),
                service => Err(DosError::UnsupportedInt21hService(service)),
            },
            0x28 => self.idle_service(),
            _ => Err(DosError::UnsupportedInterrupt(interrupt)),
        }
    }

    /// Handles DOS Idle Service (INT 28h)
    pub(super) fn idle_service(&self) -> Result<InterruptResult, DosError> {
        // Noop for now
        // TODO: support guest-installed handlers
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT 21h, AH=01h: Emulates the read one character with echo service
    pub(super) fn read_char_with_echo_service(&mut self) -> Result<InterruptResult, DosError> {
        let Some(Some(resource)) = self.handles.get(0) else {
            return Err(DosError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Stdin | Resource::Console => {
                self.pending_input = Some(PendingInput::DosReadCharWithEcho);
                Ok(InterruptResult::WaitingForInput)
            }
            _ => Err(DosError::ReadServiceError(
                "Unsupported resource in handle 0".to_string(),
            )),
        }
    }

    /// INT 21h, AH=02h: Emulates the DOS output char service
    pub(super) fn output_char_service(
        &self,
        cpu: &Cpu,
        memory: &mut [u8],
        video: &mut CgaVideo,
    ) -> Result<InterruptResult, DosError> {
        let c = cpu.get_reg(Reg8::Dl);
        let Some(Some(resource)) = self.handles.get(1) else {
            return Err(DosError::WriteServiceError(
                "Handle 1 is not set".to_string(),
            ));
        };
        // TODO: Support other resources in the future
        if *resource != Resource::Stdout {
            return Err(DosError::WriteServiceError(
                "Handle 1 must be stdout".to_string(),
            ));
        }

        video.write_char(memory, c)?;

        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS print string service (INT 21h, AH=09h)
    pub(super) fn print_string_service(
        &self,
        cpu: &mut Cpu,
        memory: &mut [u8],
        video: &mut CgaVideo,
    ) -> Result<InterruptResult, DosError> {
        let mut segmented_addr = SegmentedAddr::new(cpu.ds, cpu.dx);
        let mut buf = vec![];
        let mut found_dollar = false;
        for _ in 0..memory.len() {
            let byte = read_byte(memory, segmented_addr.to_physical())?;
            if byte == b'$' {
                found_dollar = true;
                break;
            }
            buf.push(byte);
            segmented_addr = segmented_addr.wrapping_add(1);
        }
        if !found_dollar {
            return Err(DosError::PrintStringServiceError(
                "$ symbol not found in memory".to_string(),
            ));
        }

        for c in buf {
            video.write_char(memory, c)?;
        }

        // Observed in real DOS
        cpu.set_reg(Reg8::Al, 0x24);

        return Ok(InterruptResult::ResumeExecution);
    }

    /// Emulates the DOS flush and read service (INT 21h, AH=0Ch)
    pub(super) fn flush_and_read_service(
        &mut self,
        cpu: &mut Cpu,
        bios_keyboard: &mut BiosKeyboard,
    ) -> Result<InterruptResult, DosError> {
        // Flush input if handle 0 is a console
        let Some(Some(resource)) = self.handles.get(0) else {
            return Err(DosError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Stdin | Resource::Console => {
                bios_keyboard.clear();
            }
            _ => {}
        };

        let al = cpu.get_reg(Reg8::Al);
        return match al {
            0x01 => self.read_char_with_echo_service(),
            _ => Err(DosError::ReadServiceError(
                "Unsupported mode in 0Ch service".to_string(),
            )),
        };
    }

    /// INT21h, AH=30h: Emulates the DOS version service
    pub(super) fn dos_version_service(&self, cpu: &mut Cpu) -> Result<InterruptResult, DosError> {
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
    ) -> Result<InterruptResult, DosError> {
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

    /// INT21h, AH=3Fh: Emulates the DOS read service
    pub(super) fn read_service(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut [u8],
    ) -> Result<InterruptResult, DosError> {
        let dest = cpu.bx as usize;
        let Some(Some(resource)) = self.handles.get(dest) else {
            // Unknown handle
            cpu.ax = 6;
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        if *resource != Resource::Stdin {
            return Err(DosError::ReadServiceError(
                "Unsupported resource".to_string(),
            ));
        }
        let dest_buffer = normalize_addr(SegmentedAddr::new(cpu.ds, cpu.dx));
        // Limit size of write to 0x10000 - offset
        let max_bytes = (cpu.cx as usize).min(0x10000 - dest_buffer.offset as usize);
        validate_buffer(memory, dest_buffer.to_physical(), max_bytes)?;

        if max_bytes == 0 {
            // Nothing to read
            cpu.ax = 0;
            cpu.set_flag(Flag::Carry, false);
            return Ok(InterruptResult::ResumeExecution);
        }

        self.pending_input = Some(PendingInput::DosRead(PendingRead {
            destination: dest_buffer,
            max_bytes,
            bytes: Vec::new(),
        }));
        Ok(InterruptResult::WaitingForInput)
    }

    /// Emulates the DOS write service (INT 21h, AH=40h)
    pub(super) fn write_service(
        &self,
        cpu: &mut Cpu,
        memory: &mut [u8],
        video: &mut CgaVideo,
    ) -> Result<InterruptResult, DosError> {
        let start_pos = normalize_addr(SegmentedAddr::new(cpu.ds, cpu.dx));
        // Limit size of write to 0x10000 - offset
        let num_bytes = (cpu.cx as usize).min(0x10000 - start_pos.offset as usize);
        let dest = cpu.bx;
        if dest != 1 {
            return Err(DosError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        let buffer = read_buffer(memory, start_pos.to_physical(), num_bytes)?;
        for c in &buffer {
            video.write_char(memory, *c)?;
        }

        // Successfully written bytes to AX and clear CF
        cpu.ax = num_bytes as u16;
        cpu.set_flag(Flag::Carry, false);

        Ok(InterruptResult::ResumeExecution)
    }

    /// INT21h, AH=45h: Emulates the Duplicate File Handle Service
    pub(super) fn duplicate_file_handle_service(
        &mut self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, DosError> {
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
    pub(super) fn termination_service(&self, cpu: &Cpu) -> Result<InterruptResult, DosError> {
        Ok(InterruptResult::Exit((cpu.ax & 0xFF) as u8))
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum Resource {
    Stdin,
    Stdout,
    Console,
}

enum PendingInput {
    DosRead(PendingRead),
    DosReadCharWithEcho,
}

struct PendingRead {
    pub(super) destination: SegmentedAddr,
    pub(super) max_bytes: usize,
    pub(super) bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum DosError {
    #[error(transparent)]
    Memory(#[from] MemoryError),
    #[error("DOS Print String Service error: {0}")]
    PrintStringServiceError(String),
    #[error("DOS Read Service error: {0}")]
    ReadServiceError(String),
    #[error("Unsupported DOS service: INT {0:X}h")]
    UnsupportedInterrupt(u8),
    #[error("Unsupported DOS service: INT 21h AH={0:X}h")]
    UnsupportedInt21hService(u8),
    #[error("DOS Write Service error: {0}")]
    WriteServiceError(String),
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
