use font8x8::{BASIC_FONTS, UnicodeFonts};

use crate::memory::MemoryError;

const NUM_COLUMNS: usize = 80;
const NUM_ROWS: usize = 25;
const VIDEO_ADDR_BASE: usize = 0xB8000;
const GLYPH_WIDTH: usize = 8;
const GLYPH_HEIGHT: usize = 8;
const BYTES_PER_PIXEL: usize = 4;

pub(super) struct TextVideo {
    cursor_column: usize,
    cursor_row: usize,
}

impl TextVideo {
    pub(super) fn new() -> Self {
        Self {
            cursor_column: 0,
            cursor_row: 0,
        }
    }

    pub(super) fn clear(&mut self, memory: &mut [u8]) -> Result<(), MemoryError> {
        // fill all 2000 cells with a space and attribute 0x07 (light grey on black)
        for i in 0..NUM_COLUMNS * NUM_ROWS {
            memory[VIDEO_ADDR_BASE + 2 * i] = 0x20;
            memory[VIDEO_ADDR_BASE + 2 * i + 1] = 0x07;
        }

        // reset the cursor
        self.cursor_column = 0;
        self.cursor_row = 0;
        Ok(())
    }

    pub(super) fn write_char(&mut self, memory: &mut [u8], byte: u8) -> Result<(), MemoryError> {
        if byte == b'\r' {
            self.cursor_column = 0;
            return Ok(());
        } else if byte == b'\n' {
            self.cursor_row = self.cursor_row + 1;
        } else if byte.is_ascii_graphic() || byte == b' ' {
            let addr = VIDEO_ADDR_BASE + 2 * (self.cursor_row * NUM_COLUMNS + self.cursor_column);
            memory[addr] = byte;
            memory[addr + 1] = 0x07;
            self.cursor_column += 1;
        }

        if self.cursor_column == NUM_COLUMNS {
            self.cursor_column = 0;
            self.cursor_row = self.cursor_row + 1;
        }

        if self.cursor_row == NUM_ROWS {
            // scroll up by one row
            for r in 0..NUM_ROWS - 1 {
                for c in 0..NUM_COLUMNS {
                    memory[VIDEO_ADDR_BASE + 2 * (r * NUM_COLUMNS + c)] =
                        memory[VIDEO_ADDR_BASE + 2 * ((r + 1) * NUM_COLUMNS + c)];
                    memory[VIDEO_ADDR_BASE + 2 * (r * NUM_COLUMNS + c) + 1] =
                        memory[VIDEO_ADDR_BASE + 2 * ((r + 1) * NUM_COLUMNS + c) + 1];
                }
            }
            // clear the last row
            for c in 0..NUM_COLUMNS {
                memory[VIDEO_ADDR_BASE + 2 * ((NUM_ROWS - 1) * NUM_COLUMNS + c)] = 0x20;
                memory[VIDEO_ADDR_BASE + 2 * ((NUM_ROWS - 1) * NUM_COLUMNS + c) + 1] = 0x07;
            }
            self.cursor_row = NUM_ROWS - 1;
        }

        Ok(())
    }

    /// Renders video memory to the framebuffer
    pub(super) fn render(&self, memory: &[u8], frame: &mut [u8]) {
        for row in 0..NUM_ROWS {
            for column in 0..NUM_COLUMNS {
                let byte = memory[VIDEO_ADDR_BASE + 2 * (row * 80 + column)];
                let glyph = BASIC_FONTS.get(char::from(byte)).unwrap_or([0; 8]);
                for gy in 0..GLYPH_HEIGHT {
                    for gx in 0..GLYPH_WIDTH {
                        let lit = glyph[gy] & (1u8 << gx) != 0;
                        let x = column * GLYPH_WIDTH + gx;
                        let y = row * GLYPH_HEIGHT + gy;
                        let offset = BYTES_PER_PIXEL * (y * NUM_COLUMNS * GLYPH_WIDTH + x);
                        let brightness = if lit { 0xAA } else { 0x00 };
                        frame[offset] = brightness; // red
                        frame[offset + 1] = brightness; // green
                        frame[offset + 2] = brightness; // blue
                        frame[offset + 3] = 0xFF; // alpha
                    }
                }
            }
        }
    }
}
