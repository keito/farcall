use crate::cpu::Flag;
use crate::machine::MachineStepResult;
use crate::memory::{SegmentedAddr, validate_buffer, write_buffer};
use crate::registers::Reg8;

use super::Machine;
use super::MachineError;

pub(super) enum PendingInput {
    DosRead(PendingRead),
    DosReadCharWithEcho,
    KeyboardService,
}

pub(super) struct PendingRead {
    pub(super) destination: SegmentedAddr,
    pub(super) max_bytes: usize,
    pub(super) bytes: Vec<u8>,
}

impl Machine {
    pub(super) fn handle_pending_input(&mut self) -> Result<MachineStepResult, MachineError> {
        let Some(pending_input) = &mut self.pending_input else {
            return Ok(MachineStepResult::Continue);
        };
        match pending_input {
            PendingInput::DosRead(read) => {
                if self.console_read_buffer.is_empty() {
                    // Consume keystrokes until Enter. Move completed line into buffer.
                    while let Some(key) = self.bios_key_queue.pop_front() {
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
                let buffer =
                    validate_buffer(&mut self.memory, read.destination.to_physical(), num_bytes)?;
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
            PendingInput::DosReadCharWithEcho => {
                if let Some(key) = self.bios_key_queue.pop_front() {
                    if let Some(char) = key.ascii {
                        self.text_video.write_char(&mut self.memory, char)?;
                        self.cpu.set_reg(Reg8::Al, char);
                        self.pending_input = None;
                        return Ok(MachineStepResult::Continue);
                    }
                }
                return Ok(MachineStepResult::WaitingForInput);
            }
            PendingInput::KeyboardService => {
                if let Some(key) = self.bios_key_queue.pop_front() {
                    self.cpu.set_reg(Reg8::Al, key.ascii.unwrap_or(0x0));
                    self.cpu.set_reg(Reg8::Ah, key.scan_code);
                    self.pending_input = None;
                    return Ok(MachineStepResult::Continue);
                }
                return Ok(MachineStepResult::WaitingForInput);
            }
        }
    }
}
