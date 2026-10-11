use std::io::Write;

use crossterm::{
    cursor,
    style::{Color, SetForegroundColor},
};

use crate::machine::TextScreen;

/// Render text-only output to the terminal.
pub(super) fn render(screen: &TextScreen, output: &mut impl Write) -> std::io::Result<()> {
    crossterm::execute!(output, cursor::Hide)?;

    let mut prev_fg_color: Option<u8> = None;
    for (row_index, row) in screen.cells.iter().enumerate() {
        output.write_all(format!("\x1b[{};{}H", row_index + 1, 1).as_bytes())?;
        for cell in row {
            // Print only printable ASCII characters, if video is enabled
            if screen.video_enabled && matches!(cell.character, 0x20..=0x7E) {
                let fg_color = cell.attribute & 0x0F;
                if Some(fg_color) != prev_fg_color {
                    crossterm::execute!(
                        output,
                        SetForegroundColor(attribute_color_to_crossterm_color(fg_color))
                    )?;
                    prev_fg_color = Some(fg_color);
                }
                output.write_all(&[cell.character])?;
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
    match color {
        0x00 => Color::Black,
        0x01 => Color::DarkBlue,
        0x02 => Color::DarkGreen,
        0x03 => Color::DarkCyan,
        0x04 => Color::DarkRed,
        0x05 => Color::DarkMagenta,
        0x06 => Color::DarkYellow, // Actually brown, but...
        0x07 => Color::Grey,
        0x08 => Color::DarkGrey,
        0x09 => Color::Blue,
        0x0A => Color::Green,
        0x0B => Color::Cyan,
        0x0C => Color::Red,
        0x0D => Color::Magenta,
        0x0E => Color::Yellow,
        _ => Color::White,
    }
}
