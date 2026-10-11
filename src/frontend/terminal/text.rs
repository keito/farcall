use std::io::Write;

use crossterm::cursor;

use crate::machine::TextScreen;

/// Render text-only output to the terminal.
pub(super) fn render(screen: &TextScreen, output: &mut impl Write) -> std::io::Result<()> {
    crossterm::execute!(output, cursor::Hide)?;

    for (row_index, row) in screen.cells.iter().enumerate() {
        output.write_all(format!("\x1b[{};{}H", row_index + 1, 1).as_bytes())?;
        for cell in row {
            // Print only printable ASCII characters, if video is enabled
            if screen.video_enabled && matches!(cell.character, 0x20..=0x7E) {
                output.write_all(&[cell.character])?;
            } else {
                output.write_all(&[b' '])?;
            }
        }
    }

    if let Some(cursor) = screen.cursor {
        crossterm::execute!(
            output,
            cursor::MoveTo(cursor.column as u16, cursor.row as u16),
            cursor::Show
        )?;
    }

    Ok(())
}
