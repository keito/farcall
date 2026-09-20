use winit::keyboard::{KeyCode, SmolStr};

use crate::machine::KeyPress;

pub(crate) struct ModifierState {
    pub(crate) shift: bool,
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
}

pub(crate) fn translate_winit_key(
    key: KeyCode,
    text: Option<SmolStr>,
    modifiers: &ModifierState,
) -> Option<KeyPress> {
    // Shift-Tab has no ASCII text
    if key == KeyCode::Tab && modifiers.shift {
        return Some(KeyPress {
            ascii: None,
            scan_code: 0x0F,
        });
    }

    let (scan_code, scan_code_with_shift) = match key {
        KeyCode::KeyA => (0x1E, 0x1E),
        KeyCode::KeyB => (0x30, 0x30),
        KeyCode::KeyC => (0x2E, 0x2E),
        KeyCode::KeyD => (0x20, 0x20),
        KeyCode::KeyE => (0x12, 0x12),
        KeyCode::KeyF => (0x21, 0x21),
        KeyCode::KeyG => (0x22, 0x22),
        KeyCode::KeyH => (0x23, 0x23),
        KeyCode::KeyI => (0x17, 0x17),
        KeyCode::KeyJ => (0x24, 0x24),
        KeyCode::KeyK => (0x25, 0x25),
        KeyCode::KeyL => (0x26, 0x26),
        KeyCode::KeyM => (0x32, 0x32),
        KeyCode::KeyN => (0x31, 0x31),
        KeyCode::KeyO => (0x18, 0x18),
        KeyCode::KeyP => (0x19, 0x19),
        KeyCode::KeyQ => (0x10, 0x10),
        KeyCode::KeyR => (0x13, 0x13),
        KeyCode::KeyS => (0x1F, 0x1F),
        KeyCode::KeyT => (0x14, 0x14),
        KeyCode::KeyU => (0x16, 0x16),
        KeyCode::KeyV => (0x2F, 0x2F),
        KeyCode::KeyW => (0x11, 0x11),
        KeyCode::KeyX => (0x2D, 0x2D),
        KeyCode::KeyY => (0x15, 0x15),
        KeyCode::KeyZ => (0x2C, 0x2C),
        KeyCode::Digit1 => (0x02, 0x02),
        KeyCode::Digit2 => (0x03, 0x03),
        KeyCode::Digit3 => (0x04, 0x04),
        KeyCode::Digit4 => (0x05, 0x05),
        KeyCode::Digit5 => (0x06, 0x06),
        KeyCode::Digit6 => (0x07, 0x07),
        KeyCode::Digit7 => (0x08, 0x08),
        KeyCode::Digit8 => (0x09, 0x09),
        KeyCode::Digit9 => (0x0A, 0x0A),
        KeyCode::Digit0 => (0x0B, 0x0B),
        KeyCode::Minus => (0x0C, 0x0C),
        KeyCode::Equal => (0x0D, 0x0D),
        KeyCode::Enter => (0x1C, 0x1C),
        KeyCode::Backspace => (0x0E, 0x0E),
        KeyCode::Space => (0x39, 0x39),
        KeyCode::Escape => (0x01, 0x01),
        KeyCode::Tab => (0x0F, 0x0F),
        KeyCode::F1 => (0x3B, 0x54),
        KeyCode::F2 => (0x3C, 0x55),
        KeyCode::F3 => (0x3D, 0x56),
        KeyCode::F4 => (0x3E, 0x57),
        KeyCode::F5 => (0x3F, 0x58),
        KeyCode::F6 => (0x40, 0x59),
        KeyCode::F7 => (0x41, 0x5A),
        KeyCode::F8 => (0x42, 0x5B),
        KeyCode::F9 => (0x43, 0x5C),
        KeyCode::F10 => (0x44, 0x5D),
        _ => return None,
    };

    // Send the right ASCII code between 0x01 and 0x1A for Ctrl+a through z
    if modifiers.ctrl && key >= KeyCode::KeyA && key <= KeyCode::KeyZ {
        return Some(KeyPress {
            ascii: Some(key as u8 - KeyCode::KeyA as u8 + 1),
            scan_code,
        });
    }

    // Make sure dead keys don't register
    let ascii = match text {
        Some(t) if t.is_ascii() && t.len() == 1 => Some(t.as_bytes()[0]),
        None if matches!(
            key,
            KeyCode::F1
                | KeyCode::F2
                | KeyCode::F3
                | KeyCode::F4
                | KeyCode::F5
                | KeyCode::F6
                | KeyCode::F7
                | KeyCode::F8
                | KeyCode::F9
                | KeyCode::F10
        ) =>
        {
            None
        }
        _ => return None,
    };

    let scan_code = if modifiers.shift {
        scan_code_with_shift
    } else {
        scan_code
    };

    Some(KeyPress { ascii, scan_code })
}
