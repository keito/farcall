use crossterm::event::{
    KeyCode as CTKeyCode, KeyEvent as CTKeyEvent, KeyEventState, KeyModifiers, ModifierKeyCode,
};

use crate::machine::bios_keyboard::BiosKeyStroke;

#[cfg(test)]
mod tests;

enum KeyMap {
    Override(u8, u8), // Override ASCII code; provide (scan code, ascii code)
    Ignore,           // Ignores the key
}

pub(crate) fn crossterm_to_bios_keystroke(event: &CTKeyEvent) -> Option<BiosKeyStroke> {
    if !event.is_press() && !event.is_repeat() {
        return None;
    }

    let (normal, shift, ctrl, alt) = match event.code {
        CTKeyCode::Char('a') => (
            KeyMap::Override(0x1E, 'a' as u8),
            KeyMap::Override(0x1E, 0x41),
            KeyMap::Override(0x1E, 0x01),
            KeyMap::Override(0x1E, 0x00),
        ),
        CTKeyCode::Char('b') => (
            KeyMap::Override(0x30, 'b' as u8),
            KeyMap::Override(0x30, 0x42),
            KeyMap::Override(0x30, 0x02),
            KeyMap::Override(0x30, 0x00),
        ),
        CTKeyCode::Char('c') => (
            KeyMap::Override(0x2E, 'c' as u8),
            KeyMap::Override(0x2E, 0x43),
            KeyMap::Override(0x2E, 0x03),
            KeyMap::Override(0x2E, 0x00),
        ),
        CTKeyCode::Char('d') => (
            KeyMap::Override(0x20, 'd' as u8),
            KeyMap::Override(0x20, 0x44),
            KeyMap::Override(0x20, 0x04),
            KeyMap::Override(0x20, 0x00),
        ),
        CTKeyCode::Char('e') => (
            KeyMap::Override(0x12, 'e' as u8),
            KeyMap::Override(0x12, 0x45),
            KeyMap::Override(0x12, 0x05),
            KeyMap::Override(0x12, 0x00),
        ),
        CTKeyCode::Char('f') => (
            KeyMap::Override(0x21, 'f' as u8),
            KeyMap::Override(0x21, 0x46),
            KeyMap::Override(0x21, 0x06),
            KeyMap::Override(0x21, 0x00),
        ),
        CTKeyCode::Char('g') => (
            KeyMap::Override(0x22, 'g' as u8),
            KeyMap::Override(0x22, 0x47),
            KeyMap::Override(0x22, 0x07),
            KeyMap::Override(0x22, 0x00),
        ),
        CTKeyCode::Char('h') => (
            KeyMap::Override(0x23, 'h' as u8),
            KeyMap::Override(0x23, 0x48),
            KeyMap::Override(0x23, 0x08),
            KeyMap::Override(0x23, 0x00),
        ),
        CTKeyCode::Char('i') => (
            KeyMap::Override(0x17, 'i' as u8),
            KeyMap::Override(0x17, 0x49),
            KeyMap::Override(0x17, 0x09),
            KeyMap::Override(0x17, 0x00),
        ),
        CTKeyCode::Char('j') => (
            KeyMap::Override(0x24, 'j' as u8),
            KeyMap::Override(0x24, 0x4A),
            KeyMap::Override(0x24, 0x0A),
            KeyMap::Override(0x24, 0x00),
        ),
        CTKeyCode::Char('k') => (
            KeyMap::Override(0x25, 'k' as u8),
            KeyMap::Override(0x25, 0x4B),
            KeyMap::Override(0x25, 0x0B),
            KeyMap::Override(0x25, 0x00),
        ),
        CTKeyCode::Char('l') => (
            KeyMap::Override(0x26, 'l' as u8),
            KeyMap::Override(0x26, 0x4C),
            KeyMap::Override(0x26, 0x0C),
            KeyMap::Override(0x26, 0x00),
        ),
        CTKeyCode::Char('m') => (
            KeyMap::Override(0x32, 'm' as u8),
            KeyMap::Override(0x32, 0x4D),
            KeyMap::Override(0x32, 0x0D),
            KeyMap::Override(0x32, 0x00),
        ),
        CTKeyCode::Char('n') => (
            KeyMap::Override(0x31, 'n' as u8),
            KeyMap::Override(0x31, 0x4E),
            KeyMap::Override(0x31, 0x0E),
            KeyMap::Override(0x31, 0x00),
        ),
        CTKeyCode::Char('o') => (
            KeyMap::Override(0x18, 'o' as u8),
            KeyMap::Override(0x18, 0x4F),
            KeyMap::Override(0x18, 0x0F),
            KeyMap::Override(0x18, 0x00),
        ),
        CTKeyCode::Char('p') => (
            KeyMap::Override(0x19, 'p' as u8),
            KeyMap::Override(0x19, 0x50),
            KeyMap::Override(0x19, 0x10),
            KeyMap::Override(0x19, 0x00),
        ),
        CTKeyCode::Char('q') => (
            KeyMap::Override(0x10, 'q' as u8),
            KeyMap::Override(0x10, 0x51),
            KeyMap::Override(0x10, 0x11),
            KeyMap::Override(0x10, 0x00),
        ),
        CTKeyCode::Char('r') => (
            KeyMap::Override(0x13, 'r' as u8),
            KeyMap::Override(0x13, 0x52),
            KeyMap::Override(0x13, 0x12),
            KeyMap::Override(0x13, 0x00),
        ),
        CTKeyCode::Char('s') => (
            KeyMap::Override(0x1F, 's' as u8),
            KeyMap::Override(0x1F, 0x53),
            KeyMap::Override(0x1F, 0x13),
            KeyMap::Override(0x1F, 0x00),
        ),
        CTKeyCode::Char('t') => (
            KeyMap::Override(0x14, 't' as u8),
            KeyMap::Override(0x14, 0x54),
            KeyMap::Override(0x14, 0x14),
            KeyMap::Override(0x14, 0x00),
        ),
        CTKeyCode::Char('u') => (
            KeyMap::Override(0x16, 'u' as u8),
            KeyMap::Override(0x16, 0x55),
            KeyMap::Override(0x16, 0x15),
            KeyMap::Override(0x16, 0x00),
        ),
        CTKeyCode::Char('v') => (
            KeyMap::Override(0x2F, 'v' as u8),
            KeyMap::Override(0x2F, 0x56),
            KeyMap::Override(0x2F, 0x16),
            KeyMap::Override(0x2F, 0x00),
        ),
        CTKeyCode::Char('w') => (
            KeyMap::Override(0x11, 'w' as u8),
            KeyMap::Override(0x11, 0x57),
            KeyMap::Override(0x11, 0x17),
            KeyMap::Override(0x11, 0x00),
        ),
        CTKeyCode::Char('x') => (
            KeyMap::Override(0x2D, 'x' as u8),
            KeyMap::Override(0x2D, 0x58),
            KeyMap::Override(0x2D, 0x18),
            KeyMap::Override(0x2D, 0x00),
        ),
        CTKeyCode::Char('y') => (
            KeyMap::Override(0x15, 'y' as u8),
            KeyMap::Override(0x15, 0x59),
            KeyMap::Override(0x15, 0x19),
            KeyMap::Override(0x15, 0x00),
        ),
        CTKeyCode::Char('z') => (
            KeyMap::Override(0x2C, 'z' as u8),
            KeyMap::Override(0x2C, 0x5A),
            KeyMap::Override(0x2C, 0x1A),
            KeyMap::Override(0x2C, 0x00),
        ),
        CTKeyCode::Char('1') => (
            KeyMap::Override(0x02, '1' as u8),
            KeyMap::Override(0x02, 0x21),
            KeyMap::Ignore,
            KeyMap::Override(0x78, 0x00),
        ),
        CTKeyCode::Char('2') => (
            KeyMap::Override(0x03, '2' as u8),
            KeyMap::Override(0x03, 0x40),
            KeyMap::Override(0x03, 0x00),
            KeyMap::Override(0x79, 0x00),
        ),
        CTKeyCode::Char('3') => (
            KeyMap::Override(0x04, '3' as u8),
            KeyMap::Override(0x04, 0x23),
            KeyMap::Ignore,
            KeyMap::Override(0x7A, 0x00),
        ),
        CTKeyCode::Char('4') => (
            KeyMap::Override(0x05, '4' as u8),
            KeyMap::Override(0x05, 0x24),
            KeyMap::Ignore,
            KeyMap::Override(0x7B, 0x00),
        ),
        CTKeyCode::Char('5') => (
            KeyMap::Override(0x06, '5' as u8),
            KeyMap::Override(0x06, 0x25),
            KeyMap::Ignore,
            KeyMap::Override(0x7C, 0x00),
        ),
        CTKeyCode::Char('6') => (
            KeyMap::Override(0x07, '6' as u8),
            KeyMap::Override(0x07, 0x5E),
            KeyMap::Override(0x07, 0x1E),
            KeyMap::Override(0x7D, 0x00),
        ),
        CTKeyCode::Char('7') => (
            KeyMap::Override(0x08, '7' as u8),
            KeyMap::Override(0x08, 0x26),
            KeyMap::Ignore,
            KeyMap::Override(0x7E, 0x00),
        ),
        CTKeyCode::Char('8') => (
            KeyMap::Override(0x09, '8' as u8),
            KeyMap::Override(0x09, 0x2A),
            KeyMap::Ignore,
            KeyMap::Override(0x7F, 0x00),
        ),
        CTKeyCode::Char('9') => (
            KeyMap::Override(0x0A, '9' as u8),
            KeyMap::Override(0x0A, 0x28),
            KeyMap::Ignore,
            KeyMap::Override(0x80, 0x00),
        ),
        CTKeyCode::Char('0') => (
            KeyMap::Override(0x0B, '0' as u8),
            KeyMap::Override(0x0B, 0x29),
            KeyMap::Ignore,
            KeyMap::Override(0x81, 0x00),
        ),
        CTKeyCode::Char('-') => (
            KeyMap::Override(0x0C, '-' as u8),
            KeyMap::Override(0x0C, 0x5F),
            KeyMap::Override(0x0C, 0x1F),
            KeyMap::Override(0x82, 0x00),
        ),
        CTKeyCode::Char('=') => (
            KeyMap::Override(0x0D, '=' as u8),
            KeyMap::Override(0x0D, 0x2B),
            KeyMap::Ignore,
            KeyMap::Override(0x83, 0x00),
        ),
        CTKeyCode::Enter => (
            KeyMap::Override(0x1C, 0x0D),
            KeyMap::Override(0x1C, 0x0D),
            KeyMap::Override(0x1C, 0x0A),
            KeyMap::Ignore,
        ),
        CTKeyCode::Backspace => (
            KeyMap::Override(0x0E, 0x08),
            KeyMap::Override(0x0E, 0x08),
            KeyMap::Override(0x0E, 0x7F),
            KeyMap::Ignore,
        ),
        CTKeyCode::Char(' ') => (
            KeyMap::Override(0x39, ' ' as u8),
            KeyMap::Override(0x39, 0x20),
            KeyMap::Override(0x39, 0x20),
            KeyMap::Override(0x39, 0x20),
        ),
        CTKeyCode::Esc => (
            KeyMap::Override(0x01, 0x1B),
            KeyMap::Override(0x01, 0x1B),
            KeyMap::Override(0x01, 0x1B),
            KeyMap::Ignore,
        ),
        CTKeyCode::Tab => (
            KeyMap::Override(0x0F, 0x09),
            KeyMap::Override(0x0F, 0x00),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::BackTab => (
            KeyMap::Override(0x0F, 0x00),
            KeyMap::Override(0x0F, 0x00),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::Char('/') => (
            KeyMap::Override(0x35, '/' as u8),
            KeyMap::Override(0x35, 0x3F),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::Char('\'') => (
            KeyMap::Override(0x28, '\'' as u8),
            KeyMap::Override(0x28, '"' as u8),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::Char('`') => (
            KeyMap::Override(0x29, '`' as u8),
            KeyMap::Override(0x29, 0x7E),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::Char('\\') => (
            KeyMap::Override(0x2B, '\\' as u8),
            KeyMap::Override(0x2B, 0x7C),
            KeyMap::Override(0x2B, 0x1C),
            KeyMap::Ignore,
        ),
        CTKeyCode::Char('[') => (
            KeyMap::Override(0x1A, '[' as u8),
            KeyMap::Override(0x1A, 0x7B),
            KeyMap::Override(0x1A, 0x1B),
            KeyMap::Ignore,
        ),
        CTKeyCode::Char(']') => (
            KeyMap::Override(0x1B, ']' as u8),
            KeyMap::Override(0x1B, 0x7D),
            KeyMap::Override(0x1B, 0x1D),
            KeyMap::Ignore,
        ),
        CTKeyCode::Char(';') => (
            KeyMap::Override(0x27, ';' as u8),
            KeyMap::Override(0x27, 0x3A),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::Char(',') => (
            KeyMap::Override(0x33, ',' as u8),
            KeyMap::Override(0x33, 0x3C),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::Char('.') => (
            KeyMap::Override(0x34, '.' as u8),
            KeyMap::Override(0x34, 0x3E),
            KeyMap::Ignore,
            KeyMap::Ignore,
        ),
        CTKeyCode::F(n @ 1..=10) => (
            KeyMap::Override(0x3B + (n - 1), 0x00),
            KeyMap::Override(0x54 + (n - 1), 0x00),
            KeyMap::Override(0x5E + (n - 1), 0x00),
            KeyMap::Override(0x68 + (n - 1), 0x00),
        ),
        _ => return None,
    };

    let map = if event.modifiers.contains(KeyModifiers::ALT) {
        alt
    } else if event.modifiers.contains(KeyModifiers::CONTROL) {
        ctrl
    } else if event.modifiers.contains(KeyModifiers::SHIFT) {
        shift
    } else {
        normal
    };
    match map {
        KeyMap::Override(scan_code, ascii) => Some(BiosKeyStroke {
            scan_code,
            ascii: Some(ascii),
        }),
        KeyMap::Ignore => None,
    }
}

pub(crate) fn crossterm_to_pc_scan_code(event: &CTKeyEvent) -> Option<u8> {
    let keypad = event.state.contains(KeyEventState::KEYPAD);
    let scan_code = match (event.code, keypad) {
        (CTKeyCode::Char('a'), _) => 0x1E,
        (CTKeyCode::Char('b'), _) => 0x30,
        (CTKeyCode::Char('c'), _) => 0x2E,
        (CTKeyCode::Char('d'), _) => 0x20,
        (CTKeyCode::Char('e'), _) => 0x12,
        (CTKeyCode::Char('f'), _) => 0x21,
        (CTKeyCode::Char('g'), _) => 0x22,
        (CTKeyCode::Char('h'), _) => 0x23,
        (CTKeyCode::Char('i'), _) => 0x17,
        (CTKeyCode::Char('j'), _) => 0x24,
        (CTKeyCode::Char('k'), _) => 0x25,
        (CTKeyCode::Char('l'), _) => 0x26,
        (CTKeyCode::Char('m'), _) => 0x32,
        (CTKeyCode::Char('n'), _) => 0x31,
        (CTKeyCode::Char('o'), _) => 0x18,
        (CTKeyCode::Char('p'), _) => 0x19,
        (CTKeyCode::Char('q'), _) => 0x10,
        (CTKeyCode::Char('r'), _) => 0x13,
        (CTKeyCode::Char('s'), _) => 0x1F,
        (CTKeyCode::Char('t'), _) => 0x14,
        (CTKeyCode::Char('u'), _) => 0x16,
        (CTKeyCode::Char('v'), _) => 0x2F,
        (CTKeyCode::Char('w'), _) => 0x11,
        (CTKeyCode::Char('x'), _) => 0x2D,
        (CTKeyCode::Char('y'), _) => 0x15,
        (CTKeyCode::Char('z'), _) => 0x2C,
        (CTKeyCode::Char('1'), false) => 0x02,
        (CTKeyCode::Char('2'), false) => 0x03,
        (CTKeyCode::Char('3'), false) => 0x04,
        (CTKeyCode::Char('4'), false) => 0x05,
        (CTKeyCode::Char('5'), false) => 0x06,
        (CTKeyCode::Char('6'), false) => 0x07,
        (CTKeyCode::Char('7'), false) => 0x08,
        (CTKeyCode::Char('8'), false) => 0x09,
        (CTKeyCode::Char('9'), false) => 0x0A,
        (CTKeyCode::Char('0'), false) => 0x0B,
        (CTKeyCode::Char('-'), false) => 0x0C,
        (CTKeyCode::Char('='), _) => 0x0D,
        (CTKeyCode::Enter, _) => 0x1C,
        (CTKeyCode::Backspace, _) => 0x0E,
        (CTKeyCode::Char(' '), _) => 0x39,
        (CTKeyCode::Esc, _) => 0x01,
        (CTKeyCode::Tab, _) => 0x0F,
        (CTKeyCode::Char('/'), _) => 0x35,
        (CTKeyCode::Char('\''), _) => 0x28,
        (CTKeyCode::Char('`'), _) => 0x29,
        (CTKeyCode::Char('\\'), _) => 0x2B,
        (CTKeyCode::Char('['), _) => 0x1A,
        (CTKeyCode::Char(']'), _) => 0x1B,
        (CTKeyCode::Char(';'), _) => 0x27,
        (CTKeyCode::Char(','), _) => 0x33,
        (CTKeyCode::Char('.'), false) => 0x34,
        (CTKeyCode::F(1), _) => 0x3B,
        (CTKeyCode::F(2), _) => 0x3C,
        (CTKeyCode::F(3), _) => 0x3D,
        (CTKeyCode::F(4), _) => 0x3E,
        (CTKeyCode::F(5), _) => 0x3F,
        (CTKeyCode::F(6), _) => 0x40,
        (CTKeyCode::F(7), _) => 0x41,
        (CTKeyCode::F(8), _) => 0x42,
        (CTKeyCode::F(9), _) => 0x43,
        (CTKeyCode::F(10), _) => 0x44,
        (CTKeyCode::Modifier(ModifierKeyCode::LeftShift), _) => 0x2A,
        (CTKeyCode::Modifier(ModifierKeyCode::RightShift), _) => 0x36,
        (CTKeyCode::Modifier(ModifierKeyCode::LeftControl), _) => 0x1D,
        (CTKeyCode::Modifier(ModifierKeyCode::RightControl), _) => 0x1D, // Alias
        (CTKeyCode::Modifier(ModifierKeyCode::LeftAlt), _) => 0x38,
        (CTKeyCode::Modifier(ModifierKeyCode::RightAlt), _) => 0x38, // Alias
        (CTKeyCode::CapsLock, _) => 0x3A,
        (CTKeyCode::NumLock, _) => 0x45,
        (CTKeyCode::ScrollLock, _) => 0x46,
        (CTKeyCode::Char('7'), true) => 0x47,
        (CTKeyCode::Char('8'), true) => 0x48,
        (CTKeyCode::Char('9'), true) => 0x49,
        (CTKeyCode::Char('-'), true) => 0x4A,
        (CTKeyCode::Char('4'), true) => 0x4B,
        (CTKeyCode::Char('5'), true) => 0x4C,
        (CTKeyCode::Char('6'), true) => 0x4D,
        (CTKeyCode::Char('+'), true) => 0x4E,
        (CTKeyCode::Char('1'), true) => 0x4F,
        (CTKeyCode::Char('2'), true) => 0x50,
        (CTKeyCode::Char('3'), true) => 0x51,
        (CTKeyCode::Char('0'), true) => 0x52,
        (CTKeyCode::Char('.'), true) => 0x53,
        (CTKeyCode::Char('*'), true) => 0x37,
        (CTKeyCode::Up, _) => 0x48,       // Numpad up
        (CTKeyCode::Down, _) => 0x50,     // Numpad down
        (CTKeyCode::Left, _) => 0x4B,     // Numpad left
        (CTKeyCode::Right, _) => 0x4D,    // Numpad right
        (CTKeyCode::PageUp, _) => 0x49,   // Numpad page up
        (CTKeyCode::PageDown, _) => 0x51, // Numpad page down
        (CTKeyCode::Home, _) => 0x47,     // Numpad home
        (CTKeyCode::End, _) => 0x4F,      // Numpad end
        (CTKeyCode::Insert, _) => 0x52,   // Numpad insert
        (CTKeyCode::Delete, _) => 0x53,   // Numpad delete
        _ => return None,
    };
    Some(scan_code as u8)
}

pub(crate) fn normalize_basic_crossterm_key_event(key_event: CTKeyEvent) -> CTKeyEvent {
    fn shift_event(key_event: CTKeyEvent, key_char: char) -> CTKeyEvent {
        CTKeyEvent {
            code: CTKeyCode::Char(key_char),
            modifiers: key_event.modifiers | KeyModifiers::SHIFT,
            kind: key_event.kind,
            state: key_event.state,
        }
    }

    match key_event.code {
        CTKeyCode::Char(char @ 'A'..='Z') => shift_event(key_event, char.to_ascii_lowercase()),
        CTKeyCode::Char('~') => shift_event(key_event, '`'),
        CTKeyCode::Char('!') => shift_event(key_event, '1'),
        CTKeyCode::Char('@') => shift_event(key_event, '2'),
        CTKeyCode::Char('#') => shift_event(key_event, '3'),
        CTKeyCode::Char('$') => shift_event(key_event, '4'),
        CTKeyCode::Char('%') => shift_event(key_event, '5'),
        CTKeyCode::Char('^') => shift_event(key_event, '6'),
        CTKeyCode::Char('&') => shift_event(key_event, '7'),
        CTKeyCode::Char('*') => shift_event(key_event, '8'),
        CTKeyCode::Char('(') => shift_event(key_event, '9'),
        CTKeyCode::Char(')') => shift_event(key_event, '0'),
        CTKeyCode::Char('_') => shift_event(key_event, '-'),
        CTKeyCode::Char('+') => shift_event(key_event, '='),
        CTKeyCode::Char('{') => shift_event(key_event, '['),
        CTKeyCode::Char('}') => shift_event(key_event, ']'),
        CTKeyCode::Char('|') => shift_event(key_event, '\\'),
        CTKeyCode::Char(':') => shift_event(key_event, ';'),
        CTKeyCode::Char('"') => shift_event(key_event, '\''),
        CTKeyCode::Char('<') => shift_event(key_event, ','),
        CTKeyCode::Char('>') => shift_event(key_event, '.'),
        CTKeyCode::Char('?') => shift_event(key_event, '/'),
        _ => key_event,
    }
}
