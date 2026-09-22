use winit::keyboard::{KeyCode, SmolStr};

use crate::machine::bios_keyboard::BiosKeyStroke;

#[cfg(test)]
mod tests;

pub(crate) struct ModifierState {
    pub(crate) shift: bool,
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
}

enum KeyMap {
    HostText(u8),     // Uses the host's ASCII code; provide scan code
    Override(u8, u8), // Override ASCII code; provide (scan code, ascii code)
    Ignore,           // Ignores the key
}

pub(crate) fn translate_winit_key(
    key: KeyCode,
    text: Option<SmolStr>,
    modifiers: &ModifierState,
) -> Option<BiosKeyStroke> {
    let (normal, shift, ctrl, alt) = match key {
        KeyCode::KeyA => (
            KeyMap::HostText(0x1E),
            KeyMap::Override(0x1E, 0x41),
            KeyMap::Override(0x1E, 0x01),
            KeyMap::Override(0x1E, 0x00),
        ),
        KeyCode::KeyB => (
            KeyMap::HostText(0x30),
            KeyMap::Override(0x30, 0x42),
            KeyMap::Override(0x30, 0x02),
            KeyMap::Override(0x30, 0x00),
        ),
        KeyCode::KeyC => (
            KeyMap::HostText(0x2E),
            KeyMap::Override(0x2E, 0x43),
            KeyMap::Override(0x2E, 0x03),
            KeyMap::Override(0x2E, 0x00),
        ),
        KeyCode::KeyD => (
            KeyMap::HostText(0x20),
            KeyMap::Override(0x20, 0x44),
            KeyMap::Override(0x20, 0x04),
            KeyMap::Override(0x20, 0x00),
        ),
        KeyCode::KeyE => (
            KeyMap::HostText(0x12),
            KeyMap::Override(0x12, 0x45),
            KeyMap::Override(0x12, 0x05),
            KeyMap::Override(0x12, 0x00),
        ),
        KeyCode::KeyF => (
            KeyMap::HostText(0x21),
            KeyMap::Override(0x21, 0x46),
            KeyMap::Override(0x21, 0x06),
            KeyMap::Override(0x21, 0x00),
        ),
        KeyCode::KeyG => (
            KeyMap::HostText(0x22),
            KeyMap::Override(0x22, 0x47),
            KeyMap::Override(0x22, 0x07),
            KeyMap::Override(0x22, 0x00),
        ),
        KeyCode::KeyH => (
            KeyMap::HostText(0x23),
            KeyMap::Override(0x23, 0x48),
            KeyMap::Override(0x23, 0x08),
            KeyMap::Override(0x23, 0x00),
        ),
        KeyCode::KeyI => (
            KeyMap::HostText(0x17),
            KeyMap::Override(0x17, 0x49),
            KeyMap::Override(0x17, 0x09),
            KeyMap::Override(0x17, 0x00),
        ),
        KeyCode::KeyJ => (
            KeyMap::HostText(0x24),
            KeyMap::Override(0x24, 0x4A),
            KeyMap::Override(0x24, 0x0A),
            KeyMap::Override(0x24, 0x00),
        ),
        KeyCode::KeyK => (
            KeyMap::HostText(0x25),
            KeyMap::Override(0x25, 0x4B),
            KeyMap::Override(0x25, 0x0B),
            KeyMap::Override(0x25, 0x00),
        ),
        KeyCode::KeyL => (
            KeyMap::HostText(0x26),
            KeyMap::Override(0x26, 0x4C),
            KeyMap::Override(0x26, 0x0C),
            KeyMap::Override(0x26, 0x00),
        ),
        KeyCode::KeyM => (
            KeyMap::HostText(0x32),
            KeyMap::Override(0x32, 0x4D),
            KeyMap::Override(0x32, 0x0D),
            KeyMap::Override(0x32, 0x00),
        ),
        KeyCode::KeyN => (
            KeyMap::HostText(0x31),
            KeyMap::Override(0x31, 0x4E),
            KeyMap::Override(0x31, 0x0E),
            KeyMap::Override(0x31, 0x00),
        ),
        KeyCode::KeyO => (
            KeyMap::HostText(0x18),
            KeyMap::Override(0x18, 0x4F),
            KeyMap::Override(0x18, 0x0F),
            KeyMap::Override(0x18, 0x00),
        ),
        KeyCode::KeyP => (
            KeyMap::HostText(0x19),
            KeyMap::Override(0x19, 0x50),
            KeyMap::Override(0x19, 0x10),
            KeyMap::Override(0x19, 0x00),
        ),
        KeyCode::KeyQ => (
            KeyMap::HostText(0x10),
            KeyMap::Override(0x10, 0x51),
            KeyMap::Override(0x10, 0x11),
            KeyMap::Override(0x10, 0x00),
        ),
        KeyCode::KeyR => (
            KeyMap::HostText(0x13),
            KeyMap::Override(0x13, 0x52),
            KeyMap::Override(0x13, 0x12),
            KeyMap::Override(0x13, 0x00),
        ),
        KeyCode::KeyS => (
            KeyMap::HostText(0x1F),
            KeyMap::Override(0x1F, 0x53),
            KeyMap::Override(0x1F, 0x13),
            KeyMap::Override(0x1F, 0x00),
        ),
        KeyCode::KeyT => (
            KeyMap::HostText(0x14),
            KeyMap::Override(0x14, 0x54),
            KeyMap::Override(0x14, 0x14),
            KeyMap::Override(0x14, 0x00),
        ),
        KeyCode::KeyU => (
            KeyMap::HostText(0x16),
            KeyMap::Override(0x16, 0x55),
            KeyMap::Override(0x16, 0x15),
            KeyMap::Override(0x16, 0x00),
        ),
        KeyCode::KeyV => (
            KeyMap::HostText(0x2F),
            KeyMap::Override(0x2F, 0x56),
            KeyMap::Override(0x2F, 0x16),
            KeyMap::Override(0x2F, 0x00),
        ),
        KeyCode::KeyW => (
            KeyMap::HostText(0x11),
            KeyMap::Override(0x11, 0x57),
            KeyMap::Override(0x11, 0x17),
            KeyMap::Override(0x11, 0x00),
        ),
        KeyCode::KeyX => (
            KeyMap::HostText(0x2D),
            KeyMap::Override(0x2D, 0x58),
            KeyMap::Override(0x2D, 0x18),
            KeyMap::Override(0x2D, 0x00),
        ),
        KeyCode::KeyY => (
            KeyMap::HostText(0x15),
            KeyMap::Override(0x15, 0x59),
            KeyMap::Override(0x15, 0x19),
            KeyMap::Override(0x15, 0x00),
        ),
        KeyCode::KeyZ => (
            KeyMap::HostText(0x2C),
            KeyMap::Override(0x2C, 0x5A),
            KeyMap::Override(0x2C, 0x1A),
            KeyMap::Override(0x2C, 0x00),
        ),
        KeyCode::Digit1 => (
            KeyMap::HostText(0x02),
            KeyMap::Override(0x02, 0x21),
            KeyMap::Ignore,
            KeyMap::Override(0x78, 0x00),
        ),
        KeyCode::Digit2 => (
            KeyMap::HostText(0x03),
            KeyMap::Override(0x03, 0x40),
            KeyMap::Override(0x03, 0x00),
            KeyMap::Override(0x79, 0x00),
        ),
        KeyCode::Digit3 => (
            KeyMap::HostText(0x04),
            KeyMap::Override(0x04, 0x23),
            KeyMap::Ignore,
            KeyMap::Override(0x7A, 0x00),
        ),
        KeyCode::Digit4 => (
            KeyMap::HostText(0x05),
            KeyMap::Override(0x05, 0x24),
            KeyMap::Ignore,
            KeyMap::Override(0x7B, 0x00),
        ),
        KeyCode::Digit5 => (
            KeyMap::HostText(0x06),
            KeyMap::Override(0x06, 0x25),
            KeyMap::Ignore,
            KeyMap::Override(0x7C, 0x00),
        ),
        KeyCode::Digit6 => (
            KeyMap::HostText(0x07),
            KeyMap::Override(0x07, 0x5E),
            KeyMap::Override(0x07, 0x1E),
            KeyMap::Override(0x7D, 0x00),
        ),
        KeyCode::Digit7 => (
            KeyMap::HostText(0x08),
            KeyMap::Override(0x08, 0x26),
            KeyMap::Ignore,
            KeyMap::Override(0x7E, 0x00),
        ),
        KeyCode::Digit8 => (
            KeyMap::HostText(0x09),
            KeyMap::Override(0x09, 0x2A),
            KeyMap::Ignore,
            KeyMap::Override(0x7F, 0x00),
        ),
        KeyCode::Digit9 => (
            KeyMap::HostText(0x0A),
            KeyMap::Override(0x0A, 0x28),
            KeyMap::Ignore,
            KeyMap::Override(0x80, 0x00),
        ),
        KeyCode::Digit0 => (
            KeyMap::HostText(0x0B),
            KeyMap::Override(0x0B, 0x29),
            KeyMap::Ignore,
            KeyMap::Override(0x81, 0x00),
        ),
        KeyCode::Minus => (
            KeyMap::HostText(0x0C),
            KeyMap::Override(0x0C, 0x5F),
            KeyMap::Override(0x0C, 0x1F),
            KeyMap::Override(0x82, 0x00),
        ),
        KeyCode::Equal => (
            KeyMap::HostText(0x0D),
            KeyMap::Override(0x0D, 0x2B),
            KeyMap::Ignore,
            KeyMap::Override(0x83, 0x00),
        ),
        KeyCode::Enter => (
            KeyMap::HostText(0x1C),
            KeyMap::Override(0x1C, 0x0D),
            KeyMap::Override(0x1C, 0x0A),
            KeyMap::Ignore,
        ),
        KeyCode::Backspace => (
            KeyMap::HostText(0x0E),
            KeyMap::Override(0x0E, 0x08),
            KeyMap::Override(0x0E, 0x7F),
            KeyMap::Ignore,
        ),
        KeyCode::Space => (
            KeyMap::HostText(0x39),
            KeyMap::Override(0x39, 0x20),
            KeyMap::Override(0x39, 0x20),
            KeyMap::Override(0x39, 0x20),
        ),
        KeyCode::Escape => (
            KeyMap::HostText(0x01),
            KeyMap::Override(0x01, 0x1B),
            KeyMap::Override(0x01, 0x1B),
            KeyMap::Ignore,
        ),
        KeyCode::Tab => (
            KeyMap::HostText(0x0F),
            KeyMap::Override(0x0F, 0x00),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::Slash => (
            KeyMap::HostText(0x35),
            KeyMap::Override(0x35, 0x3F),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::Quote => (
            KeyMap::HostText(0x28),
            KeyMap::Override(0x28, 0x22),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::Backquote => (
            KeyMap::HostText(0x29),
            KeyMap::Override(0x29, 0x7E),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::Backslash => (
            KeyMap::HostText(0x2B),
            KeyMap::Override(0x2B, 0x7C),
            KeyMap::Override(0x2B, 0x1C),
            KeyMap::Ignore,
        ),
        KeyCode::BracketLeft => (
            KeyMap::HostText(0x1A),
            KeyMap::Override(0x1A, 0x7B),
            KeyMap::Override(0x1A, 0x1B),
            KeyMap::Ignore,
        ),
        KeyCode::BracketRight => (
            KeyMap::HostText(0x1B),
            KeyMap::Override(0x1B, 0x7D),
            KeyMap::Override(0x1B, 0x1D),
            KeyMap::Ignore,
        ),
        KeyCode::Semicolon => (
            KeyMap::HostText(0x27),
            KeyMap::Override(0x27, 0x3A),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::Comma => (
            KeyMap::HostText(0x33),
            KeyMap::Override(0x33, 0x3C),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::Period => (
            KeyMap::HostText(0x34),
            KeyMap::Override(0x34, 0x3E),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        KeyCode::F1 => (
            KeyMap::Override(0x3B, 0x00),
            KeyMap::Override(0x54, 0x00),
            KeyMap::Override(0x5E, 0x00),
            KeyMap::Override(0x68, 0x00),
        ),
        KeyCode::F2 => (
            KeyMap::Override(0x3C, 0x00),
            KeyMap::Override(0x55, 0x00),
            KeyMap::Override(0x5F, 0x00),
            KeyMap::Override(0x69, 0x00),
        ),
        KeyCode::F3 => (
            KeyMap::Override(0x3D, 0x00),
            KeyMap::Override(0x56, 0x00),
            KeyMap::Override(0x60, 0x00),
            KeyMap::Override(0x6A, 0x00),
        ),
        KeyCode::F4 => (
            KeyMap::Override(0x3E, 0x00),
            KeyMap::Override(0x57, 0x00),
            KeyMap::Override(0x61, 0x00),
            KeyMap::Override(0x6B, 0x00),
        ),
        KeyCode::F5 => (
            KeyMap::Override(0x3F, 0x00),
            KeyMap::Override(0x58, 0x00),
            KeyMap::Override(0x62, 0x00),
            KeyMap::Override(0x6C, 0x00),
        ),
        KeyCode::F6 => (
            KeyMap::Override(0x40, 0x00),
            KeyMap::Override(0x59, 0x00),
            KeyMap::Override(0x63, 0x00),
            KeyMap::Override(0x6D, 0x00),
        ),
        KeyCode::F7 => (
            KeyMap::Override(0x41, 0x00),
            KeyMap::Override(0x5A, 0x00),
            KeyMap::Override(0x64, 0x00),
            KeyMap::Override(0x6E, 0x00),
        ),
        KeyCode::F8 => (
            KeyMap::Override(0x42, 0x00),
            KeyMap::Override(0x5B, 0x00),
            KeyMap::Override(0x65, 0x00),
            KeyMap::Override(0x6F, 0x00),
        ),
        KeyCode::F9 => (
            KeyMap::Override(0x43, 0x00),
            KeyMap::Override(0x5C, 0x00),
            KeyMap::Override(0x66, 0x00),
            KeyMap::Override(0x70, 0x00),
        ),
        KeyCode::F10 => (
            KeyMap::Override(0x44, 0x00),
            KeyMap::Override(0x5D, 0x00),
            KeyMap::Override(0x67, 0x00),
            KeyMap::Override(0x71, 0x00),
        ),
        _ => return None,
    };

    let map = if modifiers.alt {
        alt
    } else if modifiers.ctrl {
        ctrl
    } else if modifiers.shift {
        shift
    } else {
        normal
    };
    match map {
        KeyMap::HostText(scan_code) => text.map_or(None, |text| {
            if text.is_ascii() && text.len() == 1 {
                Some(BiosKeyStroke {
                    scan_code,
                    ascii: Some(text.as_bytes()[0]),
                })
            } else {
                None
            }
        }),
        KeyMap::Override(scan_code, ascii) => Some(BiosKeyStroke {
            scan_code,
            ascii: Some(ascii),
        }),
        KeyMap::Ignore => None,
    }
}

/// Returns the base BIOS scan code for the raw KeyCode, used for ROM mode when a BIOS
/// is maintaining modifier keys.
pub(super) fn translate_raw_key(key: KeyCode) -> Option<u8> {
    let scan_code = match key {
        KeyCode::KeyA => 0x1E,
        KeyCode::KeyB => 0x30,
        KeyCode::KeyC => 0x2E,
        KeyCode::KeyD => 0x20,
        KeyCode::KeyE => 0x12,
        KeyCode::KeyF => 0x21,
        KeyCode::KeyG => 0x22,
        KeyCode::KeyH => 0x23,
        KeyCode::KeyI => 0x17,
        KeyCode::KeyJ => 0x24,
        KeyCode::KeyK => 0x25,
        KeyCode::KeyL => 0x26,
        KeyCode::KeyM => 0x32,
        KeyCode::KeyN => 0x31,
        KeyCode::KeyO => 0x18,
        KeyCode::KeyP => 0x19,
        KeyCode::KeyQ => 0x10,
        KeyCode::KeyR => 0x13,
        KeyCode::KeyS => 0x1F,
        KeyCode::KeyT => 0x14,
        KeyCode::KeyU => 0x16,
        KeyCode::KeyV => 0x2F,
        KeyCode::KeyW => 0x11,
        KeyCode::KeyX => 0x2D,
        KeyCode::KeyY => 0x15,
        KeyCode::KeyZ => 0x2C,
        KeyCode::Digit1 => 0x02,
        KeyCode::Digit2 => 0x03,
        KeyCode::Digit3 => 0x04,
        KeyCode::Digit4 => 0x05,
        KeyCode::Digit5 => 0x06,
        KeyCode::Digit6 => 0x07,
        KeyCode::Digit7 => 0x08,
        KeyCode::Digit8 => 0x09,
        KeyCode::Digit9 => 0x0A,
        KeyCode::Digit0 => 0x0B,
        KeyCode::Minus => 0x0C,
        KeyCode::Equal => 0x0D,
        KeyCode::Enter => 0x1C,
        KeyCode::Backspace => 0x0E,
        KeyCode::Space => 0x39,
        KeyCode::Escape => 0x01,
        KeyCode::Tab => 0x0F,
        KeyCode::Slash => 0x35,
        KeyCode::Quote => 0x28,
        KeyCode::Backquote => 0x29,
        KeyCode::Backslash => 0x2B,
        KeyCode::BracketLeft => 0x1A,
        KeyCode::BracketRight => 0x1B,
        KeyCode::Semicolon => 0x27,
        KeyCode::Comma => 0x33,
        KeyCode::Period => 0x34,
        KeyCode::F1 => 0x3B,
        KeyCode::F2 => 0x3C,
        KeyCode::F3 => 0x3D,
        KeyCode::F4 => 0x3E,
        KeyCode::F5 => 0x3F,
        KeyCode::F6 => 0x40,
        KeyCode::F7 => 0x41,
        KeyCode::F8 => 0x42,
        KeyCode::F9 => 0x43,
        KeyCode::F10 => 0x44,
        KeyCode::ShiftLeft => 0x2A,
        KeyCode::ShiftRight => 0x36,
        KeyCode::ControlLeft => 0x1D,
        KeyCode::ControlRight => 0x1D, // Alias
        KeyCode::AltLeft => 0x38,
        KeyCode::AltRight => 0x38, // Alias
        KeyCode::CapsLock => 0x3A,
        KeyCode::NumLock => 0x45,
        KeyCode::ScrollLock => 0x46,
        KeyCode::Numpad7 => 0x47,
        KeyCode::Numpad8 => 0x48,
        KeyCode::Numpad9 => 0x49,
        KeyCode::NumpadSubtract => 0x4A,
        KeyCode::Numpad4 => 0x4B,
        KeyCode::Numpad5 => 0x4C,
        KeyCode::Numpad6 => 0x4D,
        KeyCode::NumpadAdd => 0x4E,
        KeyCode::Numpad1 => 0x4F,
        KeyCode::Numpad2 => 0x50,
        KeyCode::Numpad3 => 0x51,
        KeyCode::Numpad0 => 0x52,
        KeyCode::NumpadDecimal => 0x53,
        KeyCode::NumpadMultiply => 0x37,
        KeyCode::ArrowUp => 0x48,    // Numpad up
        KeyCode::ArrowDown => 0x50,  // Numpad down
        KeyCode::ArrowLeft => 0x4B,  // Numpad left
        KeyCode::ArrowRight => 0x4D, // Numpad right
        KeyCode::PageUp => 0x49,     // Numpad page up
        KeyCode::PageDown => 0x51,   // Numpad page down
        KeyCode::Home => 0x47,       // Numpad home
        KeyCode::End => 0x4F,        // Numpad end
        KeyCode::Insert => 0x52,     // Numpad insert
        KeyCode::Delete => 0x53,     // Numpad delete
        _ => return None,
    };
    Some(scan_code as u8)
}
