use std::{
    fs::File,
    io::{Read, Write, stdout},
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use rustix::termios::{OptionalActions, SpecialCodeIndex, Termios, tcgetattr, tcsetattr};

const UNSUPPORTED_HINT: &str = "\n\nGraphics Terminal mode requires a terminal emulator that supports the Kitty graphics protocol.";

pub(super) struct TerminalGuard {
    did_enable_enhanced_keyboard: bool,
    did_enter_alternate_screen: bool,
    did_use_graphics: bool,
    pub(super) terminal: File,
    original_settings: Option<Termios>,
    require_graphics: bool,
}

impl TerminalGuard {
    pub(super) fn new(require_graphics: bool) -> Result<Self, anyhow::Error> {
        // TODO: Add support for Windows
        let Ok(tty) = std::fs::File::open("/dev/tty") else {
            return Err(anyhow::anyhow!(
                "This program requires an interactive terminal."
            ));
        };

        let mut guard = Self {
            did_enable_enhanced_keyboard: false,
            did_enter_alternate_screen: false,
            did_use_graphics: false,
            terminal: tty,
            original_settings: None,
            require_graphics,
        };
        let mut attrs = tcgetattr(&guard.terminal).map_err(std::io::Error::from)?;
        guard.original_settings = Some(attrs.clone());

        // Configure raw mode
        attrs.make_raw();

        // Don't wait for bytes; we'll poll
        attrs.special_codes[SpecialCodeIndex::VTIME] = 0;
        attrs.special_codes[SpecialCodeIndex::VMIN] = 0;

        tcsetattr(&guard.terminal, OptionalActions::Now, &attrs).map_err(std::io::Error::from)?;

        // Enter alternate screen and hide cursor
        crossterm::execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
        guard.did_enter_alternate_screen = true;

        guard.check_graphics_capabilities()?;

        // Could be Ok(false), or an Err condition could be that it timed out
        if matches!(
            crossterm::terminal::supports_keyboard_enhancement(),
            Ok(true)
        ) {
            // Enable Kitty keyboard input
            crossterm::execute!(
                stdout(),
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                )
            )?;
            guard.did_enable_enhanced_keyboard = true;
        }

        Ok(guard)
    }

    pub(super) fn keyboard_enhancement_enabled(&self) -> bool {
        self.did_enable_enhanced_keyboard
    }

    fn check_graphics_capabilities(&mut self) -> anyhow::Result<()> {
        if !self.require_graphics {
            return Ok(());
        }

        // Send query
        stdout().write_all(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c")?;
        stdout().flush()?;

        // Wait for graphics reply, and identification reply, up to 1 second
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut response = Vec::new();
        while Instant::now() < deadline {
            let mut chunk = [0u8; 256];
            let count = self.terminal.read(&mut chunk)?;
            if count == 0 {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }

            response.extend_from_slice(&chunk[..count]);

            // Don't grow response more than 4KB
            if response.len() > 4096 {
                return Err(anyhow::anyhow!("Response too large"));
            }

            if has_identification_reply(&response) {
                break;
            }
        }

        // Terminal didn't respond with identification reply, so we can't tell
        // if it supports graphics.
        if !has_identification_reply(&response) {
            return Err(anyhow::anyhow!(
                "Your terminal did not respond to the identification query. {}",
                UNSUPPORTED_HINT
            ));
        }

        // Terminal didn't respond with expected graphics response
        let expected = b"\x1b_Gi=31;OK\x1b\\";
        let graphics_supported = response
            .windows(expected.len())
            .any(|bytes| bytes == expected);
        if !graphics_supported {
            return Err(anyhow::anyhow!(
                "Your terminal does not support Kitty graphics. {}",
                UNSUPPORTED_HINT
            ));
        }

        self.did_use_graphics = true;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.did_use_graphics {
            // Delete Kitty image
            if let Err(err) = stdout().write_all(b"\x1b_Ga=d,d=I,i=86,q=2\x1b\\") {
                eprintln!("Failed to restore terminal: {}", err);
            }
        }

        if self.did_enable_enhanced_keyboard {
            // Pop Kitty keyboard input flags
            if let Err(err) = crossterm::execute!(stdout(), PopKeyboardEnhancementFlags) {
                eprintln!("Failed to crossterm::execute!: {}", err);
            }
        }

        if self.did_enter_alternate_screen {
            // Show cursor and leave alternate screen
            if let Err(err) = crossterm::execute!(stdout(), cursor::Show, LeaveAlternateScreen) {
                eprintln!("Failed to crossterm::execute!: {}", err);
            }
        }

        if let Some(settings) = &self.original_settings {
            // Restore saved terminal settings
            if let Err(err) = tcsetattr(&self.terminal, OptionalActions::Now, settings) {
                eprintln!("Failed to restore terminal settings: {}", err);
            }
        }
    }
}

fn has_identification_reply(response: &[u8]) -> bool {
    response.windows(3).enumerate().any(|(start, prefix)| {
        if prefix != b"\x1b[?" {
            return false;
        }

        let rest = &response[start + 3..];
        let param_len = rest
            .iter()
            .take_while(|&&byte| byte.is_ascii_digit() || byte == b';')
            .count();

        param_len > 0 && rest.get(param_len) == Some(&b'c')
    })
}
