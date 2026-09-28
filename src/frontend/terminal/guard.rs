use std::{fs::File, io::Write};

use crossterm::{
    cursor,
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
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

        // Enable Kitty keyboard input
        crossterm::execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            cursor::Hide,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        )?;

        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Delete Kitty image
        let mut output = std::io::stdout();
        if let Err(err) = output.write_all(b"\x1b_Ga=d,d=I,i=86,q=2\x1b\\") {
            eprintln!("Failed to restore terminal: {}", err);
        }

        // Pop Kitty keyboard input flags
        if let Err(err) = crossterm::execute!(
            std::io::stdout(),
            PopKeyboardEnhancementFlags,
            cursor::Show,
            LeaveAlternateScreen
        ) {
            eprintln!("Failed to crossterm::execute!: {}", err);
        }

        // Restore saved terminal settings
        if let Some(settings) = &self.original_settings {
            if let Err(err) = tcsetattr(&self.terminal, OptionalActions::Now, settings) {
                eprintln!("Failed to restore terminal settings: {}", err);
            }
        }
    }
}
