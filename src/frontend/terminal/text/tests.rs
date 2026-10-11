use crossterm::style::{Color, SetBackgroundColor, SetForegroundColor};

use crate::machine::{TextCell, TextCursor, TextScreen};

use super::render;

fn blank_screen() -> TextScreen {
    TextScreen {
        blink_enabled: false,
        cells: [[TextCell {
            character: b' ',
            attribute: 0x07,
        }; 80]; 25],
        video_enabled: true,
        cursor: None,
    }
}

fn render_output(screen: &TextScreen) -> String {
    let mut output = Vec::new();
    render(screen, &mut output).expect("rendering to a Vec should succeed");
    assert!(
        !output.contains(&0),
        "rendered output must not contain NUL padding"
    );
    String::from_utf8(output).expect("rendered output must be valid UTF-8")
}

fn first_row(screen: &TextScreen) -> String {
    let output = render_output(screen);
    let (_, rows) = output.split_once("\x1b[1;1H").expect("first row position");
    let (mut row, _) = rows.split_once("\x1b[2;1H").expect("second row position");

    // The fixture has uniform attributes, so color commands precede the cells.
    // Strip those commands without depending on whether NO_COLOR is set.
    while let Some(command) = row.strip_prefix("\x1b[") {
        let (parameters, rest) = command.split_once('m').expect("color command terminator");
        assert!(
            parameters
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b';')
        );
        row = rest;
    }

    row.to_owned()
}

#[test]
fn printable_ascii_renders_without_nul_padding() {
    let characters: Vec<u8> = (0x20..=0x7E).collect();
    for chunk in characters.chunks(80) {
        let mut screen = blank_screen();
        for (cell, character) in screen.cells[0].iter_mut().zip(chunk) {
            cell.character = *character;
        }

        let mut expected: String = chunk.iter().copied().map(char::from).collect();
        expected.extend(std::iter::repeat_n(' ', 80 - chunk.len()));
        assert_eq!(first_row(&screen), expected);
    }
}

#[test]
fn box_drawing_characters_render_as_utf8_between_ascii_characters() {
    let mut screen = blank_screen();
    for (cell, character) in screen.cells[0]
        .iter_mut()
        .zip([b'A', 0xDA, 0xC4, 0xBF, 0xB3, 0xC0, 0xD9, b'Z'])
    {
        cell.character = character;
    }

    let mut expected = String::from("A┌─┐│└┘Z");
    expected.extend(std::iter::repeat_n(' ', 72));
    let row = first_row(&screen);
    assert_eq!(row, expected);
    assert_eq!(row.chars().count(), 80);
}

#[test]
fn screen_bytes_render_as_cp437_glyphs_instead_of_terminal_controls() {
    let mut screen = blank_screen();
    let characters = [
        b'A', 0x00, 0x01, 0x02, 0x07, 0x09, 0x0A, 0x0D, 0x1B, 0x1F, 0x7F, 0x80, 0xFF, b'Z',
    ];
    for (cell, character) in screen.cells[0].iter_mut().zip(characters) {
        cell.character = character;
    }

    // NUL is blank; the other low bytes are screen symbols, not controls.
    // CP437 0xFF is a non-breaking space, distinct from an ordinary space.
    let mut expected = String::from("A ☺☻•○◙♪←▼⌂Ç\u{a0}Z");
    expected.extend(std::iter::repeat_n(' ', 80 - characters.len()));
    let row = first_row(&screen);
    assert_eq!(row, expected);
    assert_eq!(row.chars().count(), 80);
    assert!(!row.chars().any(char::is_control));
}

#[test]
fn blink_transitions_preserve_colors_and_leave_following_cells_steady() {
    let mut screen = blank_screen();
    screen.blink_enabled = true;
    for cell in &mut screen.cells[0] {
        cell.attribute = 0x1F; // White on dark blue, steady.
    }
    for (cell, (character, attribute)) in
        screen.cells[0]
            .iter_mut()
            .zip([(b'A', 0x9F), (b'B', 0x9F), (b'C', 0x1F), (b'D', 0x9F)])
    {
        cell.character = character;
        cell.attribute = attribute;
    }

    let output = render_output(&screen);
    // Attribute bit 7 selects blink here, so all four letters keep dark blue.
    // Build color commands via Crossterm so the test also works with NO_COLOR.
    let background = SetBackgroundColor(Color::Rgb { r: 0, g: 0, b: 170 });
    let foreground = SetForegroundColor(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    });
    let expected = format!(
        "\x1b[1;1H{background}{foreground}\x1b[5mAB\x1b[25mC\x1b[5mD\x1b[25m{}",
        " ".repeat(76),
    );
    assert!(
        output.contains(&expected),
        "unexpected blink/color transitions: {output:?}"
    );
    assert_eq!(output.matches("\x1b[5m").count(), 2);
    assert_eq!(output.matches("\x1b[25m").count(), 2);
    assert!(
        output.ends_with("\x1b[0m"),
        "frame must reset its attributes"
    );
}

#[test]
fn blink_disabled_uses_bright_backgrounds_without_enabling_blink() {
    let mut screen = blank_screen();
    for (cell, (character, attribute)) in
        screen.cells[0]
            .iter_mut()
            .zip([(b'A', 0x9F), (b'B', 0x1F), (b'C', 0x9F), (b'D', 0x1F)])
    {
        cell.character = character;
        cell.attribute = attribute;
    }

    let output = render_output(&screen);
    let bright = SetBackgroundColor(Color::Rgb {
        r: 85,
        g: 85,
        b: 255,
    });
    let dark = SetBackgroundColor(Color::Rgb { r: 0, g: 0, b: 170 });
    let foreground = SetForegroundColor(Color::Rgb {
        r: 255,
        g: 255,
        b: 255,
    });
    let expected = format!("\x1b[1;1H{bright}{foreground}\x1b[25mA{dark}B{bright}C{dark}D");
    assert!(
        output.contains(&expected),
        "unexpected background colors: {output:?}"
    );
    assert!(!output.contains("\x1b[5m"), "CGA blink mode is disabled");
    assert_eq!(output.matches("\x1b[25m").count(), 1);
    assert!(
        output.ends_with("\x1b[0m"),
        "frame must reset its attributes"
    );
}

#[test]
fn visible_cursor_is_positioned_and_shown_after_resetting_attributes() {
    let mut screen = blank_screen();
    screen.blink_enabled = true;
    // Leave blink enabled on the final cell to exercise cleanup before Show.
    screen.cells[24][79] = TextCell {
        character: b'X',
        attribute: 0x87,
    };

    for (column, row) in [(0, 0), (7, 11), (79, 24)] {
        screen.cursor = Some(TextCursor { column, row });
        let output = render_output(&screen);

        assert!(
            output.starts_with("\x1b[?25l"),
            "hide cursor before drawing"
        );
        assert_eq!(output.matches("\x1b[?25l").count(), 1);
        assert_eq!(output.matches("\x1b[?25h").count(), 1);
        // Screen coordinates are zero-based; ANSI positions are one-based,
        // with the row before the column.
        let expected = format!("\x1b[5mX\x1b[0m\x1b[{};{}H\x1b[?25h", row + 1, column + 1,);
        assert!(
            output.ends_with(&expected),
            "incorrect cursor position or cleanup order at ({column}, {row})"
        );
    }
}

#[test]
fn absent_cursor_stays_hidden_after_rendering() {
    let mut screen = blank_screen();
    screen.blink_enabled = true;
    screen.cells[24][79] = TextCell {
        character: b'X',
        attribute: 0x87,
    };

    let output = render_output(&screen);

    assert!(
        output.starts_with("\x1b[?25l"),
        "hide cursor before drawing"
    );
    assert_eq!(output.matches("\x1b[?25l").count(), 1);
    assert!(!output.contains("\x1b[?25h"), "cursor must remain hidden");
    assert!(
        output.ends_with("\x1b[5mX\x1b[0m"),
        "reset attributes even when no cursor is shown"
    );
}
