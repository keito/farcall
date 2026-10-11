use std::io::Write;

use crossterm::{
    cursor,
    style::{Color, SetBackgroundColor, SetForegroundColor},
};

use crate::machine::{TextScreen, attribute_color_to_rgb};

#[cfg(test)]
mod tests;

/// Render text-only output to the terminal.
pub(super) fn render(screen: &TextScreen, output: &mut impl Write) -> std::io::Result<()> {
    crossterm::execute!(output, cursor::Hide)?;

    let mut prev_fg_color: Option<u8> = None;
    let mut prev_bg_color: Option<u8> = None;
    for (row_index, row) in screen.cells.iter().enumerate() {
        output.write_all(format!("\x1b[{};{}H", row_index + 1, 1).as_bytes())?;
        for cell in row {
            if screen.video_enabled {
                let bg_color =
                    (cell.attribute >> 4) & (if screen.blink_enabled { 0x07 } else { 0x0F });
                if Some(bg_color) != prev_bg_color {
                    crossterm::execute!(
                        output,
                        SetBackgroundColor(attribute_color_to_crossterm_color(bg_color))
                    )?;
                    prev_bg_color = Some(bg_color);
                }

                let fg_color = cell.attribute & 0x0F;
                if Some(fg_color) != prev_fg_color {
                    crossterm::execute!(
                        output,
                        SetForegroundColor(attribute_color_to_crossterm_color(fg_color))
                    )?;
                    prev_fg_color = Some(fg_color);
                }
            }

            if screen.video_enabled {
                let mut utf8_bytes = [0; 4];
                let encoded = text_character_to_char(cell.character).encode_utf8(&mut utf8_bytes);
                output.write_all(encoded.as_bytes())?;
            } else {
                output.write_all(&[b' '])?;
            }
        }
    }

    crossterm::execute!(output, crossterm::style::ResetColor)?;

    if let Some(cursor) = screen.cursor {
        crossterm::execute!(
            output,
            cursor::MoveTo(cursor.column as u16, cursor.row as u16),
            cursor::Show
        )?;
    }

    Ok(())
}

fn attribute_color_to_crossterm_color(color: u8) -> Color {
    // TODO: Support fallback to 256 colors using available_color_count()
    let (r, g, b) = attribute_color_to_rgb(color);
    Color::Rgb { r, g, b }
}

// Map CP437 characters to Unicode character
fn text_character_to_char(byte: u8) -> char {
    match byte {
        // Video-memory glyphs in the ASCII control-code positions
        0x01 => '☺',
        0x02 => '☻',
        0x03 => '♥',
        0x04 => '♦',
        0x05 => '♣',
        0x06 => '♠',
        0x07 => '•',
        0x08 => '◘',
        0x09 => '○',
        0x0A => '◙',
        0x0B => '♂',
        0x0C => '♀',
        0x0D => '♪',
        0x0E => '♫',
        0x0F => '☼',
        0x10 => '►',
        0x11 => '◄',
        0x12 => '↕',
        0x13 => '‼',
        0x14 => '¶',
        0x15 => '§',
        0x16 => '▬',
        0x17 => '↨',
        0x18 => '↑',
        0x19 => '↓',
        0x1A => '→',
        0x1B => '←',
        0x1C => '∟',
        0x1D => '↔',
        0x1E => '▲',
        0x1F => '▼',
        0x7F => '⌂',
        0x80 => 'Ç',
        0x81 => 'ü',
        0x82 => 'é',
        0x83 => 'â',
        0x84 => 'ä',
        0x85 => 'à',
        0x86 => 'å',
        0x87 => 'ç',
        0x88 => 'ê',
        0x89 => 'ë',
        0x8A => 'è',
        0x8B => 'ï',
        0x8C => 'î',
        0x8D => 'ì',
        0x8E => 'Ä',
        0x8F => 'Å',
        0x90 => 'É',
        0x91 => 'æ',
        0x92 => 'Æ',
        0x93 => 'ô',
        0x94 => 'ö',
        0x95 => 'ò',
        0x96 => 'û',
        0x97 => 'ù',
        0x98 => 'ÿ',
        0x99 => 'Ö',
        0x9A => 'Ü',
        0x9B => '¢',
        0x9C => '£',
        0x9D => '¥',
        0x9E => '₧',
        0x9F => 'ƒ',
        0xA0 => 'á',
        0xA1 => 'í',
        0xA2 => 'ó',
        0xA3 => 'ú',
        0xA4 => 'ñ',
        0xA5 => 'Ñ',
        0xA6 => 'ª',
        0xA7 => 'º',
        0xA8 => '¿',
        0xA9 => '⌐',
        0xAA => '¬',
        0xAB => '½',
        0xAC => '¼',
        0xAD => '¡',
        0xAE => '«',
        0xAF => '»',
        0xB0 => '░',
        0xB1 => '▒',
        0xB2 => '▓',
        0xB3 => '│',
        0xB4 => '┤',
        0xB5 => '╡',
        0xB6 => '╢',
        0xB7 => '╖',
        0xB8 => '╕',
        0xB9 => '╣',
        0xBA => '║',
        0xBB => '╗',
        0xBC => '╝',
        0xBD => '╜',
        0xBE => '╛',
        0xBF => '┐',
        0xC0 => '└',
        0xC1 => '┴',
        0xC2 => '┬',
        0xC3 => '├',
        0xC4 => '─',
        0xC5 => '┼',
        0xC6 => '╞',
        0xC7 => '╟',
        0xC8 => '╚',
        0xC9 => '╔',
        0xCA => '╩',
        0xCB => '╦',
        0xCC => '╠',
        0xCD => '═',
        0xCE => '╬',
        0xCF => '╧',
        0xD0 => '╨',
        0xD1 => '╤',
        0xD2 => '╥',
        0xD3 => '╙',
        0xD4 => '╘',
        0xD5 => '╒',
        0xD6 => '╓',
        0xD7 => '╫',
        0xD8 => '╪',
        0xD9 => '┘',
        0xDA => '┌',
        0xDB => '█',
        0xDC => '▄',
        0xDD => '▌',
        0xDE => '▐',
        0xDF => '▀',
        0xE0 => 'α',
        0xE1 => 'ß',
        0xE2 => 'Γ',
        0xE3 => 'π',
        0xE4 => 'Σ',
        0xE5 => 'σ',
        0xE6 => 'µ',
        0xE7 => 'τ',
        0xE8 => 'Φ',
        0xE9 => 'Θ',
        0xEA => 'Ω',
        0xEB => 'δ',
        0xEC => '∞',
        0xED => 'φ',
        0xEE => 'ε',
        0xEF => '∩',
        0xF0 => '≡',
        0xF1 => '±',
        0xF2 => '≥',
        0xF3 => '≤',
        0xF4 => '⌠',
        0xF5 => '⌡',
        0xF6 => '÷',
        0xF7 => '≈',
        0xF8 => '°',
        0xF9 => '∙',
        0xFA => '·',
        0xFB => '√',
        0xFC => 'ⁿ',
        0xFD => '²',
        0xFE => '■',
        0xFF => '\u{a0}',
        // Printable ASCII goes straight through
        0x20..=0x7E => char::from(byte),
        // Other chars are replaced with a space
        _ => ' ',
    }
}
