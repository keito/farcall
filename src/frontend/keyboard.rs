use winit::keyboard::KeyCode;

use crate::machine::KeyPress;

pub(crate) struct ModifierState {
    pub(crate) shift: bool,
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
    pub(crate) caps_lock: bool,
}

pub(crate) fn translate_winit_key(key: KeyCode, shift: bool, caps_lock: bool) -> Option<KeyPress> {
    let (scan_code, normal, shifted) = match key {
        KeyCode::KeyA => (0x1E, b'a', b'A'),
        KeyCode::KeyB => (0x30, b'b', b'B'),
        KeyCode::KeyC => (0x2E, b'c', b'C'),
        KeyCode::KeyD => (0x20, b'd', b'D'),
        KeyCode::KeyE => (0x12, b'e', b'E'),
        KeyCode::KeyF => (0x21, b'f', b'F'),
        KeyCode::KeyG => (0x22, b'g', b'G'),
        KeyCode::KeyH => (0x23, b'h', b'H'),
        KeyCode::KeyI => (0x17, b'i', b'I'),
        KeyCode::KeyJ => (0x24, b'j', b'J'),
        KeyCode::KeyK => (0x25, b'k', b'K'),
        KeyCode::KeyL => (0x26, b'l', b'L'),
        KeyCode::KeyM => (0x32, b'm', b'M'),
        KeyCode::KeyN => (0x31, b'n', b'N'),
        KeyCode::KeyO => (0x18, b'o', b'O'),
        KeyCode::KeyP => (0x19, b'p', b'P'),
        KeyCode::KeyQ => (0x10, b'q', b'Q'),
        KeyCode::KeyR => (0x13, b'r', b'R'),
        KeyCode::KeyS => (0x1F, b's', b'S'),
        KeyCode::KeyT => (0x14, b't', b'T'),
        KeyCode::KeyU => (0x16, b'u', b'U'),
        KeyCode::KeyV => (0x2F, b'v', b'V'),
        KeyCode::KeyW => (0x11, b'w', b'W'),
        KeyCode::KeyX => (0x2D, b'x', b'X'),
        KeyCode::KeyY => (0x15, b'y', b'Y'),
        KeyCode::KeyZ => (0x2C, b'z', b'Z'),
        KeyCode::Digit1 => (0x02, b'1', b'!'),
        KeyCode::Digit2 => (0x03, b'2', b'@'),
        KeyCode::Digit3 => (0x04, b'3', b'#'),
        KeyCode::Digit4 => (0x05, b'4', b'$'),
        KeyCode::Digit5 => (0x06, b'5', b'%'),
        KeyCode::Digit6 => (0x07, b'6', b'^'),
        KeyCode::Digit7 => (0x08, b'7', b'&'),
        KeyCode::Digit8 => (0x09, b'8', b'*'),
        KeyCode::Digit9 => (0x0A, b'9', b'('),
        KeyCode::Digit0 => (0x0B, b'0', b')'),
        KeyCode::Minus => (0x0C, b'-', b'_'),
        KeyCode::Equal => (0x0D, b'=', b'+'),
        KeyCode::Enter => (0x1C, b'\r', b'\r'),
        KeyCode::Backspace => (0x0E, 0x08, 0x08),
        KeyCode::Space => (0x39, b' ', b' '),
        KeyCode::Escape => (0x01, 0x1B, 0x1B),
        KeyCode::Tab => (0x0F, 0x09, 0x09),
        _ => return None,
    };

    let use_shifted = if normal.is_ascii_alphabetic() {
        shift ^ caps_lock
    } else {
        shift
    };

    Some(KeyPress {
        ascii: Some(if use_shifted { shifted } else { normal }),
        scan_code,
    })
}
