use std::fs::File;
use std::io::Read;

use rustix::termios::{LocalModes, QueueSelector, SpecialCodeIndex, Termios, tcflush};
use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};

use super::Machine;
use super::MachineError;

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

    pub(super) fn flush_console_input(&self) -> Result<(), MachineError> {
        let tty = std::fs::File::open("/dev/tty")?;
        tcflush(&tty, QueueSelector::IFlush).map_err(std::io::Error::from)?;
        Ok(())
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
    // TODO: For now, we'll just support alphabet and Enter and backspace
    if (byte >= 0x61 && byte <= 0x7a) || (byte >= 0x41 && byte <= 0x5a) {
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
        return Some((byte as u8, scan_code));
    }

    if byte == b'\r' || byte == b'\n' {
        return Some((0x0D, 0x1C));
    }

    if byte == 0x08 || byte == 0x7f {
        return Some((0x08, 0x0E));
    }

    None
}
