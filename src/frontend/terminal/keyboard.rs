use super::TerminalGuard;

use std::io::Read;

pub(super) fn read_input_byte(guard: &mut TerminalGuard) -> Result<Option<u8>, anyhow::Error> {
    let mut buf = [0u8; 1];
    let result = guard.terminal.read(&mut buf)?;
    if result == 0 {
        Ok(None)
    } else {
        Ok(Some(buf[0]))
    }
}

/// Translates a byte from the console into its ASCII char and BIOS scan code
pub(crate) fn translate_key(byte: u8) -> Option<(u8, u8)> {
    // TODO: Only supports the printable characters + a few
    match byte {
        // TODO: Move escape into a parser for escape sequences
        0x1b => Some((0x1B, 0x01)), // Escape
        b'!' => Some((byte, 0x02)),
        b'@' => Some((byte, 0x03)),
        b'#' => Some((byte, 0x04)),
        b'$' => Some((byte, 0x05)),
        b'%' => Some((byte, 0x06)),
        b'^' => Some((byte, 0x07)),
        b'&' => Some((byte, 0x08)),
        b'*' => Some((byte, 0x09)),
        b'(' => Some((byte, 0x0a)),
        b')' => Some((byte, 0x0b)),
        b'_' => Some((byte, 0x0c)),
        b'+' => Some((byte, 0x0d)),
        b'1'..=b'9' => Some((byte, byte - b'1' + 0x02)),
        b'0' => Some((byte, 0x0b)),
        b'-' => Some((byte, 0x0c)),
        b'=' => Some((byte, 0x0d)),
        0x08 | 0x7f => Some((0x08, 0x0e)),   // Backspace
        0x09 => Some((byte, 0x0f)),          // Tab
        b'\r' | b'\n' => Some((0x0d, 0x1c)), // Return
        b'[' => Some((byte, 0x1a)),
        b'{' => Some((byte, 0x1a)),
        b']' => Some((byte, 0x1b)),
        b'}' => Some((byte, 0x1b)),
        b':' => Some((byte, 0x27)),
        b';' => Some((byte, 0x27)),
        b'\'' => Some((byte, 0x28)),
        b'"' => Some((byte, 0x28)),
        b'~' => Some((byte, 0x29)),
        b'`' => Some((byte, 0x29)),
        b'\\' => Some((byte, 0x2b)),
        b'|' => Some((byte, 0x2b)),
        b',' => Some((byte, 0x33)),
        b'<' => Some((byte, 0x33)),
        b'.' => Some((byte, 0x34)),
        b'>' => Some((byte, 0x34)),
        b'/' => Some((byte, 0x35)),
        b'?' => Some((byte, 0x35)),
        b' ' => Some((byte, 0x39)),
        0x61..=0x7a | 0x41..=0x5a => {
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
            Some((byte as u8, scan_code))
        }
        _ => None,
    }
}
