use std::io::Write;

use crossterm::{
    cursor,
    style::{Color, SetBackgroundColor, SetForegroundColor},
};

use crate::machine::{TextScreen, attribute_color_to_rgb};

/// Render text-only output to the terminal.
pub(super) fn render(screen: &TextScreen, output: &mut impl Write) -> std::io::Result<()> {
    crossterm::execute!(output, cursor::Hide)?;

    let mut prev_fg_color: Option<u8> = None;
    let mut prev_bg_color: Option<u8> = None;
    for (row_index, row) in screen.cells.iter().enumerate() {
        output.write_all(format!("\x1b[{};{}H", row_index + 1, 1).as_bytes())?;
        for cell in row {
            if screen.video_enabled {
                let bg_color = (cell.attribute >> 4) & 0x07;
                if Some(bg_color) != prev_bg_color {
                    crossterm::execute!(
                        output,
                        SetBackgroundColor(attribute_color_to_crossterm_color(bg_color))
                    )?;
                    prev_bg_color = Some(bg_color);
                }
            }

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
    // TODO: Support fallback to 256 colors using available_color_count()
    let (r, g, b) = attribute_color_to_rgb(color);
    Color::Rgb { r, g, b }
}
