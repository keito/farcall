use std::fs::File;
use std::io::Read;

use rustix::termios::{LocalModes, QueueSelector, SpecialCodeIndex, Termios, tcflush};
use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};

use super::Machine;
use super::MachineError;

impl Machine {
    pub(super) fn read_console_byte(&self, buf: &mut [u8]) -> Result<(), MachineError> {
        // TODO: Add support for Windows
        let mut guard = TerminalGuard {
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
        // Wait for at least one byte
        attrs.special_codes[SpecialCodeIndex::VMIN] = 1;
        attrs.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(&guard.terminal, OptionalActions::Now, &attrs).map_err(std::io::Error::from)?;

        guard.terminal.read_exact(buf)?;

        Ok(())
    }

    pub(super) fn flush_console_input(&self) -> Result<(), MachineError> {
        let tty = std::fs::File::open("/dev/tty")?;
        tcflush(&tty, QueueSelector::IFlush).map_err(std::io::Error::from)?;
        Ok(())
    }
}

struct TerminalGuard {
    terminal: File,
    original_settings: Option<Termios>,
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
