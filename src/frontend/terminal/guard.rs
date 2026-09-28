use std::fs::File;

use rustix::termios::{OptionalActions, SpecialCodeIndex, Termios, tcgetattr, tcsetattr};

pub(super) struct TerminalGuard {
    pub(super) terminal: File,
    original_settings: Option<Termios>,
}

impl TerminalGuard {
    pub(super) fn new() -> Result<Self, anyhow::Error> {
        // TODO: Add support for Windows
        let Ok(tty) = std::fs::File::open("/dev/tty") else {
            return Err(anyhow::anyhow!(
                "This program requires an interactive terminal."
            ));
        };

        let mut guard = Self {
            terminal: tty,
            original_settings: None,
        };
        let mut attrs = tcgetattr(&guard.terminal).map_err(std::io::Error::from)?;
        guard.original_settings = Some(attrs.clone());

        // Configure raw mode
        attrs.make_raw();

        // Don't wait for bytes; we'll poll
        attrs.special_codes[SpecialCodeIndex::VTIME] = 0;
        attrs.special_codes[SpecialCodeIndex::VMIN] = 0;

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
