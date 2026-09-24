use super::CgaVideo;

const VIDEO_BASE: usize = 0xB8000;

fn setup() -> (CgaVideo, Vec<u8>) {
    let mut video = CgaVideo::new();
    let mut memory = vec![0; 0x100000];
    video.clear(&mut memory).unwrap();
    (video, memory)
}

fn write_register(video: &mut CgaVideo, register: u8, value: u8) {
    video.write(0x3D4, register);
    video.write(0x3D5, value);
}

fn write_spaces(video: &mut CgaVideo, memory: &mut [u8], count: usize) {
    for _ in 0..count {
        video.write_char(memory, b' ').unwrap();
    }
}

// On a blank screen, only the default two-scan-line cursor should be lit.
fn assert_rendered_cursor(video: &CgaVideo, memory: &[u8], cell: Option<usize>) {
    let mut frame = vec![0; 640 * 200 * 4];
    video.render(memory, &mut frame);
    let lit_pixels: Vec<_> = frame
        .chunks_exact(4)
        .enumerate()
        .filter_map(|(index, rgba)| {
            assert_eq!(rgba[3], 0xFF);
            if rgba[..3] == [0, 0, 0] {
                None
            } else {
                assert_eq!(&rgba[..3], &[0xAA; 3]);
                Some(index)
            }
        })
        .collect();
    let mut expected = Vec::new();
    if let Some(cell) = cell {
        let x = (cell % 80) * 8;
        let y = (cell / 80) * 8;
        for scan_line in 6..=7 {
            for column in 0..8 {
                expected.push((y + scan_line) * 640 + x + column);
            }
        }
    }
    assert_eq!(lit_pixels, expected);
}

#[test]
fn high_byte_writes_replace_old_bits_and_preserve_low_byte() {
    let mut video = CgaVideo::new();
    write_register(&mut video, 0x0F, 0x34);
    video.write(0x3D4, 0x0E);
    for (value, expected) in [(1, 0x0134), (0, 0x0034), (1, 0x0134), (2, 0x0234)] {
        video.write(0x3D5, value);
        assert_eq!(video.cursor_address, expected);
    }
}

#[test]
fn low_byte_writes_replace_old_bits_and_preserve_high_byte() {
    let mut video = CgaVideo::new();
    write_register(&mut video, 0x0E, 2);
    video.write(0x3D4, 0x0F);
    for (value, expected) in [(0xFF, 0x02FF), (0, 0x0200), (1, 0x0201), (2, 0x0202)] {
        video.write(0x3D5, value);
        assert_eq!(video.cursor_address, expected);
    }
}

#[test]
fn hardware_cursor_moves_without_changing_bios_output_position() {
    let (mut video, mut memory) = setup();
    video.write_char(&mut memory, b'A').unwrap();
    write_register(&mut video, 0x0E, 1);
    write_register(&mut video, 0x0F, 0x34);
    assert_eq!(video.get_cursor(), (1, 0));
    assert_eq!(video.cursor_address, 0x0134);

    video.write_char(&mut memory, b'B').unwrap();
    assert_eq!(&memory[VIDEO_BASE..VIDEO_BASE + 4], b"A\x07B\x07");
    assert_eq!(memory[VIDEO_BASE + 2 * 0x0134], b' ');
    assert_eq!(video.get_cursor(), (2, 0));
    assert_eq!(video.cursor_address, 2);
}

#[test]
fn character_output_and_control_characters_update_both_cursors() {
    let (mut video, mut memory) = setup();
    for (byte, position, address) in [
        (b'A', (1, 0), 1),
        (b'B', (2, 0), 2),
        (b'\n', (2, 1), 82), // LF preserves the column.
        (b'\r', (0, 1), 80), // CR preserves the row.
        (0x08, (0, 1), 80),  // BIOS backspace stops at the left edge.
        (b'C', (1, 1), 81),
        (0x08, (0, 1), 80),
    ] {
        video.write_char(&mut memory, byte).unwrap();
        assert_eq!(video.get_cursor(), position);
        assert_eq!(video.cursor_address, address);
    }
    // BIOS backspace moves the cursor without erasing the character.
    assert_eq!(&memory[VIDEO_BASE + 160..VIDEO_BASE + 162], b"C\x07");
}

#[test]
fn wrapping_to_next_row_moves_the_rendered_cursor() {
    let (mut video, mut memory) = setup();
    write_spaces(&mut video, &mut memory, 79);
    assert_eq!(video.get_cursor(), (79, 0));
    assert_rendered_cursor(&video, &memory, Some(79));
    video.write_char(&mut memory, b' ').unwrap();
    assert_eq!(video.get_cursor(), (0, 1));
    assert_eq!(video.cursor_address, 80);
    assert_rendered_cursor(&video, &memory, Some(80));
}

#[test]
fn bottom_right_output_scrolls_before_updating_cursor() {
    let (mut video, mut memory) = setup();
    for row in 0..25 {
        for _ in 0..80 {
            video.write_char(&mut memory, b'A' + row).unwrap();
        }
    }
    assert_eq!(video.get_cursor(), (0, 24));
    assert_eq!(video.cursor_address, 1920);
    for row in 0..24 {
        for column in 0..80 {
            let address = VIDEO_BASE + 2 * (row * 80 + column);
            assert_eq!(&memory[address..address + 2], &[b'B' + row as u8, 7]);
        }
    }
    for cell in memory[VIDEO_BASE + 3840..VIDEO_BASE + 4000].chunks_exact(2) {
        assert_eq!(cell, b" \x07");
    }
}

#[test]
fn line_feed_at_bottom_scrolls_and_preserves_column() {
    let (mut video, mut memory) = setup();
    write_spaces(&mut video, &mut memory, 1923);
    video.write_char(&mut memory, b'\n').unwrap();
    assert_eq!(video.get_cursor(), (3, 24));
    assert_eq!(video.cursor_address, 1923);
    assert_rendered_cursor(&video, &memory, Some(1923));
}

#[test]
fn dos_backspace_erases_and_updates_cursor_across_row_boundary() {
    for (count, position, address) in [(0, (0, 0), 0), (1, (0, 0), 0), (80, (79, 0), 79)] {
        let (mut video, mut memory) = setup();
        for _ in 0..count {
            video.write_char(&mut memory, b'X').unwrap();
        }
        video.backspace(&mut memory).unwrap();
        assert_eq!(video.get_cursor(), position);
        assert_eq!(video.cursor_address, address);
        let offset = VIDEO_BASE + 2 * usize::from(address);
        assert_eq!(&memory[offset..offset + 2], b" \x07");
    }
}

#[test]
fn clear_resets_both_cursor_positions_and_screen() {
    let (mut video, mut memory) = setup();
    video.write_char(&mut memory, b'A').unwrap();
    write_register(&mut video, 0x0E, 1);
    write_register(&mut video, 0x0F, 0x34);
    video.clear(&mut memory).unwrap();
    assert_eq!(video.get_cursor(), (0, 0));
    assert_eq!(video.cursor_address, 0);
    for cell in memory[VIDEO_BASE..VIDEO_BASE + 4000].chunks_exact(2) {
        assert_eq!(cell, b" \x07");
    }
    assert_rendered_cursor(&video, &memory, Some(0));
}

#[test]
fn rendering_uses_hardware_address_and_hides_cursor_outside_visible_cells() {
    let (mut video, mut memory) = setup();
    write_spaces(&mut video, &mut memory, 3);
    assert_rendered_cursor(&video, &memory, Some(3));
    for (high, low, visible_cell) in [(1, 0x34, Some(308)), (0, 0x34, Some(52)), (7, 0xD0, None)] {
        write_register(&mut video, 0x0E, high);
        write_register(&mut video, 0x0F, low);
        assert_eq!(video.get_cursor(), (3, 0));
        assert_rendered_cursor(&video, &memory, visible_cell);
    }
}
