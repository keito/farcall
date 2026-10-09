use std::{
    fs::File,
    io::{Read, Write},
    time::{Duration, Instant},
};

use crossterm::{
    cursor,
    event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use rustix::termios::{OptionalActions, SpecialCodeIndex, Termios, tcgetattr, tcsetattr};

const UNSUPPORTED_HINT: &str =
    "\n\npc86 requires a terminal emulator that supports the Kitty keyboard and graphics protocols. Supported emulators include:
    - Kitty
    - Ghostty
    - iTerm2 (>= 3.6, with \"Apps can change how keys are reported\" enabled under Profiles -> Keys -> General)
    - WezTerm (with Kitty graphics and keyboard enabled in the Lua config)
    - Warp";

pub(super) struct TerminalGuard {
    pub(super) terminal: File,
    original_settings: Option<Termios>,
    did_enable_keyboard: bool,
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
            did_enable_keyboard: false,
        };
        let mut attrs = tcgetattr(&guard.terminal).map_err(std::io::Error::from)?;
        guard.original_settings = Some(attrs.clone());

        // Configure raw mode
        attrs.make_raw();

        // Don't wait for bytes; we'll poll
        attrs.special_codes[SpecialCodeIndex::VTIME] = 0;
        attrs.special_codes[SpecialCodeIndex::VMIN] = 0;

        tcsetattr(&guard.terminal, OptionalActions::Now, &attrs).map_err(std::io::Error::from)?;

        // Check if we have Kitty graphics and keyboard
        guard.check_capabilities()?;

        // Enter alternate screen and enable Kitty keyboard input
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
        guard.did_enable_keyboard = true;

        Ok(guard)
    }

    fn check_capabilities(&mut self) -> anyhow::Result<()> {
        if !crossterm::terminal::supports_keyboard_enhancement()? {
            return Err(anyhow::anyhow!(
                "Your terminal does not support keyboard enhancements. {}",
                UNSUPPORTED_HINT
            ));
        }

        // Send query
        std::io::stdout().write_all(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c")?;
        std::io::stdout().flush()?;

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

        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.did_enable_keyboard {
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
        }

        // Restore saved terminal settings
        if let Some(settings) = &self.original_settings {
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

        param_len > 0
            && rest[..param_len]
                .split(|&byte| byte == b';')
                .all(|param| !param.is_empty() && param.iter().all(|byte| byte.is_ascii_digit()))
            && rest.get(param_len) == Some(&b'c')
    })
}
