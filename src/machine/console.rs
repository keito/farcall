use std::fs::File;
use std::io::Read;

use rustix::termios::{LocalModes, SpecialCodeIndex, Termios};
use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};

use crate::cpu::Flag;
use crate::machine::MachineStepResult;
use crate::memory::{PhysAddr, validate_buffer, write_buffer};
use crate::registers::Reg8;

use super::Machine;
use super::MachineError;

pub(super) enum PendingInput {
    DosRead(PendingRead),
    DosReadCharWithEcho,
    KeyboardService,
}

pub(super) struct PendingRead {
    pub(super) destination: PhysAddr,
    pub(super) max_bytes: usize,
    pub(super) bytes: Vec<u8>,
}

impl Machine {
    pub(super) fn read_console_byte(&mut self, buf: &mut [u8]) -> Result<(), MachineError> {
        let Some(ref mut terminal) = self.terminal else {
            return Err(MachineError::TerminalError(
                "Terminal not initialized".to_string(),
            ));
        };
        let mut attrs = tcgetattr(&terminal.terminal).map_err(std::io::Error::from)?;
        // Wait for at least one byte
        attrs.special_codes[SpecialCodeIndex::VMIN] = 1;
        tcsetattr(&terminal.terminal, OptionalActions::Now, &attrs)
            .map_err(std::io::Error::from)?;

        terminal.terminal.read_exact(buf)?;
        Ok(())
    }

    pub(super) fn try_read_console_byte(&mut self) -> Result<Option<u8>, MachineError> {
        let Some(ref mut terminal) = self.terminal else {
            return Err(MachineError::TerminalError(
                "Terminal not initialized".to_string(),
            ));
        };
        let mut attrs = tcgetattr(&terminal.terminal).map_err(std::io::Error::from)?;
        // Don't wait for bytes
        attrs.special_codes[SpecialCodeIndex::VMIN] = 0;
        tcsetattr(&terminal.terminal, OptionalActions::Now, &attrs)
            .map_err(std::io::Error::from)?;

        let mut buf = [0u8; 1];
        let result = terminal.terminal.read(&mut buf)?;
        if result == 0 {
            Ok(None)
        } else {
            Ok(Some(buf[0]))
        }
    }

    pub(super) fn handle_pending_input(&mut self) -> Result<MachineStepResult, MachineError> {
        let Some(pending_input) = &mut self.pending_input else {
            return Ok(MachineStepResult::Continue);
        };
        match pending_input {
            PendingInput::DosRead(read) => {
                if self.console_read_buffer.is_empty() {
                    // Consume keystrokes until Enter. Move completed line into buffer.
                    while let Some(key) = self.keyboard_buffer.pop_front() {
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
            PendingInput::DosReadCharWithEcho => {
                if let Some(key) = self.keyboard_buffer.pop_front() {
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
                if let Some(key) = self.keyboard_buffer.pop_front() {
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

pub(super) struct TerminalGuard {
    terminal: File,
    original_settings: Option<Termios>,
}

impl TerminalGuard {
    pub(super) fn new() -> Result<Self, MachineError> {
        // TODO: Add support for Windows
        let mut guard = Self {
            terminal: std::fs::File::open("/dev/tty")?,
            original_settings: None,
        };
        let mut attrs = tcgetattr(&guard.terminal).map_err(std::io::Error::from)?;
        guard.original_settings = Some(attrs.clone());

        // Deliver input without waiting for Enter
        attrs.local_modes.remove(LocalModes::ICANON);
        // DOS service handles echoing
        attrs.local_modes.remove(LocalModes::ECHO);
        attrs.local_modes.remove(LocalModes::ECHONL);

        attrs.special_codes[SpecialCodeIndex::VTIME] = 0;

        tcsetattr(&guard.terminal, OptionalActions::Now, &attrs).map_err(std::io::Error::from)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Restore saved terminal settings on drop
        if let Some(settings) = &self.original_settings {
            let result = tcsetattr(&self.terminal, OptionalActions::Now, settings)
                .map_err(std::io::Error::from);
            if let Err(err) = result {
                eprintln!("Failed to restore terminal settings: {}", err);
            }
        }
    }
}

/// Translates a byte from the console into its ASCII char and BIOS scan code
pub(crate) fn translate_key(byte: u8) -> Option<(u8, u8)> {
    // TODO: Only supports the printable characters + a few
    match byte {
        // TODO: Move escape into a parser for escape sequences
        0x1b => Some((0x1B, 0x01)), // Escape
        b'!' => Some((byte, 0x02)),
        b'@' => Some((byte, 0x03)),
        b'#' => Some((byte, 0x04)),
        b'$' => Some((byte, 0x05)),
        b'%' => Some((byte, 0x06)),
        b'^' => Some((byte, 0x07)),
        b'&' => Some((byte, 0x08)),
        b'*' => Some((byte, 0x09)),
        b'(' => Some((byte, 0x0a)),
        b')' => Some((byte, 0x0b)),
        b'_' => Some((byte, 0x0c)),
        b'+' => Some((byte, 0x0d)),
        b'1'..=b'9' => Some((byte, byte - b'1' + 0x02)),
        b'0' => Some((byte, 0x0b)),
        b'-' => Some((byte, 0x0c)),
        b'=' => Some((byte, 0x0d)),
        0x08 | 0x7f => Some((0x08, 0x0e)),   // Backspace
        0x09 => Some((byte, 0x0f)),          // Tab
        b'\r' | b'\n' => Some((0x0d, 0x1c)), // Return
        b'[' => Some((byte, 0x1a)),
        b'{' => Some((byte, 0x1a)),
        b']' => Some((byte, 0x1b)),
        b'}' => Some((byte, 0x1b)),
        b':' => Some((byte, 0x27)),
        b';' => Some((byte, 0x27)),
        b'\'' => Some((byte, 0x28)),
        b'"' => Some((byte, 0x28)),
        b'~' => Some((byte, 0x29)),
        b'`' => Some((byte, 0x29)),
        b'\\' => Some((byte, 0x2b)),
        b'|' => Some((byte, 0x2b)),
        b',' => Some((byte, 0x33)),
        b'<' => Some((byte, 0x33)),
        b'.' => Some((byte, 0x34)),
        b'>' => Some((byte, 0x34)),
        b'/' => Some((byte, 0x35)),
        b'?' => Some((byte, 0x35)),
        b' ' => Some((byte, 0x39)),
        0x61..=0x7a | 0x41..=0x5a => {
            let scan_code = match byte.to_ascii_lowercase() {
                b'a' => 0x1e,
                b'b' => 0x30,
                b'c' => 0x2e,
                b'd' => 0x20,
                b'e' => 0x12,
                b'f' => 0x21,
                b'g' => 0x22,
                b'h' => 0x23,
                b'i' => 0x17,
                b'j' => 0x24,
                b'k' => 0x25,
                b'l' => 0x26,
                b'm' => 0x32,
                b'n' => 0x31,
                b'o' => 0x18,
                b'p' => 0x19,
                b'q' => 0x10,
                b'r' => 0x13,
                b's' => 0x1f,
                b't' => 0x14,
                b'u' => 0x16,
                b'v' => 0x2f,
                b'w' => 0x11,
                b'x' => 0x2d,
                b'y' => 0x15,
                b'z' => 0x2c,
                _ => unreachable!("Should not reach this point"),
            };
            Some((byte as u8, scan_code))
        }
        _ => None,
    }
}
