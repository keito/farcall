use crate::{
    cpu::Flag,
    machine::{InterruptResult, Machine, MachineError, MachineStepResult},
    memory::{PhysAddr, validate_buffer, write_buffer},
};

#[derive(Clone, PartialEq)]
pub(super) enum Resource {
    Stdin,
    Stdout,
    Console,
}

pub(super) enum PendingInput {
    DosRead(PendingRead),
}

pub(super) struct PendingRead {
    destination: PhysAddr,
    max_bytes: usize,
    bytes: Vec<u8>,
}

impl Machine {
    /// Emulates the DOS read service (INT21h, service 3Fh)
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

    pub(super) fn handle_pending_input(&mut self) -> Result<MachineStepResult, MachineError> {
        let Some(pending_input) = &mut self.pending_input else {
            return Ok(MachineStepResult::Continue);
        };
        match pending_input {
            PendingInput::DosRead(read) => {
                if self.console_read_buffer.is_empty() {
                    // Consume keystrokes until Enter. Move completed line into buffer.
                    while let Some(key) = self.input_queue.pop_front() {
                        if key.ascii == Some(b'\r') {
                            // We finished the line
                            read.bytes.push(b'\r');
                            read.bytes.push(b'\n');
                            self.text_video.write_char(&mut self.memory, b'\r')?;
                            self.text_video.write_char(&mut self.memory, b'\n')?;

                            // Write those bytes to the console read buffer
                            self.console_read_buffer.extend(read.bytes.drain(..));
                            break;
                        } else if key.ascii == Some(0x08) {
                            // Backspace
                            if read.bytes.pop().is_some() {
                                self.text_video.backspace(&mut self.memory)?;
                            }
                        } else if let Some(char) = key.ascii {
                            // TODO: ignore non-ASCII characters for now
                            read.bytes.push(char);
                            self.text_video.write_char(&mut self.memory, char)?;
                        }
                    }
                }

                // If it's still empty, wait for more input/an Enter.
                if self.console_read_buffer.is_empty() {
                    return Ok(MachineStepResult::WaitingForInput);
                }

                // Dequeue at most max_bytes bytes from the read buffer to guest mem
                let num_bytes = self.console_read_buffer.len().min(read.max_bytes);
                let buffer = validate_buffer(&mut self.memory, read.destination, num_bytes)?;
                write_buffer(
                    buffer,
                    &self.console_read_buffer.make_contiguous()[..num_bytes],
                )?;
                self.console_read_buffer.drain(..num_bytes);

                // Write actual bytes read to AX and clear CF on success
                self.cpu.ax = num_bytes as u16;
                self.cpu.set_flag(Flag::Carry, false);

                self.pending_input = None;
                return Ok(MachineStepResult::Continue);
            }
        }
    }
}
