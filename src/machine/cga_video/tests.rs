use super::CgaVideo;

const VIDEO_BASE: usize = 0xB8000;

fn setup() -> (CgaVideo, Vec<u8>) {
    let mut video = CgaVideo::new();
    // These tests bypass BIOS initialization, so enable 80-column text mode.
    video.write(0x3D8, 0x29);
    let mut memory = vec![0; 0x100000];
    video.clear_text(&mut memory).unwrap();
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
    let mut frame = vec![0; 640 * 200];
    video.render(memory, &mut frame);
    let lit_pixels: Vec<_> = frame
        .iter()
        .enumerate()
        .filter_map(|(index, &color)| {
            if color == 0 {
                None
            } else {
                assert_eq!(color, 7);
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
    assert_eq!(video.get_bios_cursor(), (1, 0));
    assert_eq!(video.cursor_address, 0x0134);

    video.write_char(&mut memory, b'B').unwrap();
    assert_eq!(&memory[VIDEO_BASE..VIDEO_BASE + 4], b"A\x07B\x07");
    assert_eq!(memory[VIDEO_BASE + 2 * 0x0134], b' ');
    assert_eq!(video.get_bios_cursor(), (2, 0));
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
        assert_eq!(video.get_bios_cursor(), position);
        assert_eq!(video.cursor_address, address);
    }
    // BIOS backspace moves the cursor without erasing the character.
    assert_eq!(&memory[VIDEO_BASE + 160..VIDEO_BASE + 162], b"C\x07");
}

#[test]
fn wrapping_to_next_row_moves_the_rendered_cursor() {
    let (mut video, mut memory) = setup();
    write_spaces(&mut video, &mut memory, 79);
    assert_eq!(video.get_bios_cursor(), (79, 0));
    assert_rendered_cursor(&video, &memory, Some(79));
    video.write_char(&mut memory, b' ').unwrap();
    assert_eq!(video.get_bios_cursor(), (0, 1));
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
    assert_eq!(video.get_bios_cursor(), (0, 24));
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
    assert_eq!(video.get_bios_cursor(), (3, 24));
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
        assert_eq!(video.get_bios_cursor(), position);
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
    video.clear_text(&mut memory).unwrap();
    assert_eq!(video.get_bios_cursor(), (0, 0));
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
        assert_eq!(video.get_bios_cursor(), (3, 0));
        assert_rendered_cursor(&video, &memory, visible_cell);
    }
}

#[test]
fn high_resolution_graphics_unpacks_eight_pixels_most_significant_bit_first() {
    let mut video = CgaVideo::new();
    video.write(0x3D8, 0x1E);
    video.write(0x3D9, 0x0F);
    let mut memory = vec![0; 0x100000];
    let mut frame = vec![0xFF; 640 * 200];

    for (byte, pixels) in [
        (0x80, [15, 0, 0, 0, 0, 0, 0, 0]),
        (0x01, [0, 0, 0, 0, 0, 0, 0, 15]),
        (0xA5, [15, 0, 15, 0, 0, 15, 0, 15]),
        (0xFF, [15; 8]),
        (0x00, [0; 8]),
    ] {
        memory[VIDEO_BASE] = byte;
        video.render(&memory, &mut frame);
        let mut expected = vec![0; 640 * 200];
        expected[..8].copy_from_slice(&pixels);
        assert_eq!(frame, expected, "video byte {byte:02X}");
    }
}

#[test]
fn high_resolution_graphics_maps_both_scanline_banks_and_ignores_padding() {
    let mut video = CgaVideo::new();
    video.write(0x3D8, 0x1E);
    video.write(0x3D9, 0x07);
    let mut memory = vec![0; 0x100000];
    for (offset, byte) in [
        (0x0000, 0x80),
        (0x2000, 0x40),
        (0x0050, 0x20),
        (0x2050, 0x10),
        (0x1F3F, 0x02),
        (0x3F3F, 0x01),
    ] {
        memory[VIDEO_BASE + offset] = byte;
    }
    // Each 8 KiB bank has 192 bytes after its visible scanlines.
    memory[VIDEO_BASE + 0x1F40..VIDEO_BASE + 0x2000].fill(0xFF);
    memory[VIDEO_BASE + 0x3F40..VIDEO_BASE + 0x4000].fill(0xFF);
    memory[VIDEO_BASE + 0x4000] = 0xFF;

    let mut expected = vec![0; 640 * 200];
    for (x, y) in [(0, 0), (1, 1), (2, 2), (3, 3), (638, 198), (639, 199)] {
        expected[y * 640 + x] = 7;
    }
    let mut frame = vec![0xFF; 640 * 200];
    video.render(&memory, &mut frame);
    assert_eq!(frame, expected);
}

#[test]
fn high_resolution_graphics_uses_the_foreground_nibble_and_black_background() {
    let mut video = CgaVideo::new();
    video.write(0x3D8, 0x1E);
    let mut memory = vec![0; 0x100000];
    memory[VIDEO_BASE] = 0x81;
    let mut frame = vec![0xFF; 640 * 200];

    for foreground in 0..16 {
        let mut expected = vec![0; 640 * 200];
        expected[0] = foreground;
        expected[7] = foreground;
        for palette_bits in [0x00, 0x10, 0x20, 0x30] {
            let color_control = foreground | palette_bits;
            video.write(0x3D9, color_control);
            video.render(&memory, &mut frame);
            assert_eq!(frame, expected, "color control {color_control:02X}");
        }
    }
}

#[test]
fn disabled_video_uses_black_for_high_resolution_and_the_medium_resolution_background() {
    let mut video = CgaVideo::new();
    let mut memory = vec![0; 0x100000];
    memory[VIDEO_BASE..VIDEO_BASE + 0x4000].fill(0xFF);
    let mut frame = vec![0xFF; 640 * 200];

    for color_control in [0x00, 0x05, 0x1D, 0x3F] {
        video.write(0x3D9, color_control);
        // All modes have VIDEO clear. Bit 4 selects high resolution only in graphics.
        for (mode, expected_color) in [
            (0x02, color_control & 0x0F),
            (0x06, color_control & 0x0F),
            (0x12, 0),
            (0x16, 0),
            (0x21, 0),
            (0x31, 0),
        ] {
            video.write(0x3D8, mode);
            video.render(&memory, &mut frame);
            assert!(
                frame.iter().all(|&color| color == expected_color),
                "mode {mode:02X}, color control {color_control:02X}"
            );
        }
    }
}

#[test]
fn high_resolution_video_reenables_with_the_current_foreground_color() {
    let mut video = CgaVideo::new();
    let mut memory = vec![0; 0x100000];
    memory[VIDEO_BASE] = 0x80;
    video.write(0x3D8, 0x1E);
    video.write(0x3D9, 0x0F);
    let mut frame = vec![0xFF; 640 * 200];
    let mut expected = vec![0; 640 * 200];
    expected[0] = 15;
    video.render(&memory, &mut frame);
    assert_eq!(frame, expected);

    video.write(0x3D8, 0x16);
    video.write(0x3D9, 0x04);
    video.render(&memory, &mut frame);
    assert!(frame.iter().all(|&color| color == 0));

    video.write(0x3D8, 0x1E);
    expected[0] = 4;
    video.render(&memory, &mut frame);
    assert_eq!(frame, expected);
}

#[test]
fn graphics_mode_switching_changes_pixel_width_and_background() {
    let mut video = CgaVideo::new();
    video.write(0x3D9, 0x0F);
    let mut memory = vec![0; 0x100000];
    memory[VIDEO_BASE] = 0x1B;
    let mut frame = vec![0xFF; 640 * 200];

    for (mode, background, pixels) in [
        (0x1E, 0, [0, 0, 0, 15, 15, 0, 15, 15]),
        (0x0A, 15, [15, 15, 2, 2, 4, 4, 6, 6]),
        (0x1A, 0, [0, 0, 0, 15, 15, 0, 15, 15]),
    ] {
        video.write(0x3D8, mode);
        video.render(&memory, &mut frame);
        let mut expected = vec![background; 640 * 200];
        expected[..8].copy_from_slice(&pixels);
        assert_eq!(frame, expected, "mode control {mode:02X}");
    }
}

#[test]
fn high_resolution_bit_in_text_mode_keeps_text_rendering() {
    let (mut video, memory) = setup();
    video.write(0x3D8, 0x39);
    video.write(0x3D9, 0x0F);
    assert_rendered_cursor(&video, &memory, Some(0));
}
