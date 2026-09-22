use crate::memory::MemoryError;

#[cfg(test)]
mod tests;

const NUM_COLUMNS: usize = 80;
const NUM_ROWS: usize = 25;
const VIDEO_ADDR_BASE: usize = 0xB8000;
const GLYPH_WIDTH: usize = 8;
const GLYPH_HEIGHT: usize = 8;
const BYTES_PER_PIXEL: usize = 4;

pub(super) struct TextVideo {
    /// Hardware cursor address
    cursor_address: u16,
    /// BIOS emulation cursor position: column
    cursor_column: usize,
    /// BIOS emulation cursor position: row
    cursor_row: usize,
    /// Which register will be written to with port 0x3D4 writes
    cursor_write_selector: Option<CursorRegister>,
    /// Cursor end scan line
    end_scan_line: usize,
    /// Cursor start scan line
    start_scan_line: usize,
    status: u8,
}

enum CursorRegister {
    CursorStartScanLine,
    CursorEndScanLine,
    CursorAddressHigh,
    CursorAddressLow,
}

const GLYPH_BYTES: &[u8; 2048] = include_bytes!("cga_8x8.dat");

impl TextVideo {
    pub(super) fn new() -> Self {
        Self {
            cursor_address: 0,
            cursor_column: 0,
            cursor_row: 0,
            cursor_write_selector: None,
            end_scan_line: 7,
            start_scan_line: 6,
            status: 0,
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x3D4 => match value {
                0x0A => self.cursor_write_selector = Some(CursorRegister::CursorStartScanLine),
                0x0B => self.cursor_write_selector = Some(CursorRegister::CursorEndScanLine),
                0x0E => self.cursor_write_selector = Some(CursorRegister::CursorAddressHigh),
                0x0F => self.cursor_write_selector = Some(CursorRegister::CursorAddressLow),
                _ => self.cursor_write_selector = None,
            },
            0x3D5 => match self.cursor_write_selector {
                Some(CursorRegister::CursorStartScanLine) => self.start_scan_line = value as usize,
                Some(CursorRegister::CursorEndScanLine) => self.end_scan_line = value as usize,
                Some(CursorRegister::CursorAddressHigh) => {
                    self.cursor_address = ((value as u16) << 8) | (self.cursor_address & 0x00FF);
                }
                Some(CursorRegister::CursorAddressLow) => {
                    self.cursor_address = (self.cursor_address & 0xFF00) | (value as u16);
                }
                None => (),
            },
            _ => (),
        }
    }

    pub(super) fn set_scan_lines(&mut self, start_scan_line: usize, end_scan_line: usize) {
        self.start_scan_line = start_scan_line;
        self.end_scan_line = end_scan_line;
    }

    pub(super) fn get_scan_lines(&self) -> (usize, usize) {
        (self.start_scan_line, self.end_scan_line)
    }

    pub(super) fn get_cursor(&self) -> (usize, usize) {
        (self.cursor_column, self.cursor_row)
    }

    /// Call to sync cursor_address with cursor column/row
    fn sync_cursor_address(&mut self) {
        self.cursor_address = (self.cursor_row * NUM_COLUMNS + self.cursor_column) as u16;
    }

    /// Used by BIOS emulation to clear video memory
    pub(super) fn clear(&mut self, memory: &mut [u8]) -> Result<(), MemoryError> {
        // fill all 2000 cells with a space and attribute 0x07 (light grey on black)
        for i in 0..NUM_COLUMNS * NUM_ROWS {
            memory[VIDEO_ADDR_BASE + 2 * i] = 0x20;
            memory[VIDEO_ADDR_BASE + 2 * i + 1] = 0x07;
        }

        // reset the cursor
        self.cursor_column = 0;
        self.cursor_row = 0;
        self.sync_cursor_address();
        Ok(())
    }

    /// Used by BIOS emulation to write a charcater to video memory
    pub(super) fn write_char(&mut self, memory: &mut [u8], byte: u8) -> Result<(), MemoryError> {
        if byte == b'\r' {
            self.cursor_column = 0;
            self.sync_cursor_address();
            return Ok(());
        } else if byte == b'\n' {
            self.cursor_row = self.cursor_row + 1;
        } else if byte.is_ascii_graphic() || byte == b' ' {
            let addr = VIDEO_ADDR_BASE + 2 * (self.cursor_row * NUM_COLUMNS + self.cursor_column);
            memory[addr] = byte;
            memory[addr + 1] = 0x07;
            self.cursor_column += 1;
        } else if byte == 0x08 {
            // Backspace
            self.cursor_column = self.cursor_column.saturating_sub(1);
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

        self.sync_cursor_address();
        Ok(())
    }

    /// Used by DOS emulation to emulate a backspace
    pub(super) fn backspace(&mut self, memory: &mut [u8]) -> Result<(), MemoryError> {
        if self.cursor_column == 0 {
            // Go back one row, to the end unless we're at the top
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
                self.cursor_column = NUM_COLUMNS - 1;
            }
        } else {
            self.cursor_column -= 1;
        }

        // Delete the character that was there
        let addr = VIDEO_ADDR_BASE + 2 * (self.cursor_row * NUM_COLUMNS + self.cursor_column);
        memory[addr] = b' ';
        memory[addr + 1] = 0x07;
        self.sync_cursor_address();
        Ok(())
    }

    /// Renders video memory to the framebuffer
    pub(super) fn render(&self, memory: &[u8], frame: &mut [u8]) {
        for row in 0..NUM_ROWS {
            for column in 0..NUM_COLUMNS {
                let text_byte = memory[VIDEO_ADDR_BASE + 2 * (row * 80 + column)];
                let attributes = memory[VIDEO_ADDR_BASE + 2 * (row * 80 + column) + 1];
                let offset = usize::from(text_byte) * 8;
                let glyph = GLYPH_BYTES.get(offset..offset + 8).unwrap_or(&[0; 8]);

                let (fg_red, fg_green, fg_blue) = attribute_color_to_rgb(attributes & 0x0F);
                // TODO: Support blinking, change to 0x0F for blink disabled
                let (bg_red, bg_green, bg_blue) = attribute_color_to_rgb((attributes >> 4) & 0x07);

                // Maybe draw cursor
                let cursor_cell = row * NUM_COLUMNS + column == self.cursor_address as usize;
                let cursor_enabled = (self.start_scan_line & 0x60) != 0x20;
                let cursor_start = self.start_scan_line & 0x1F;
                let cursor_end = self.end_scan_line & 0x1F;

                for gy in 0..GLYPH_HEIGHT {
                    // Is this a cursor line?
                    let cursor_line =
                        cursor_cell && cursor_enabled && gy >= cursor_start && gy <= cursor_end;

                    for gx in 0..GLYPH_WIDTH {
                        let glyph_pixel = glyph[gy] & (0x80u8 >> gx) != 0;
                        let lit = glyph_pixel || cursor_line;
                        let x = column * GLYPH_WIDTH + gx;
                        let y = row * GLYPH_HEIGHT + gy;
                        let offset = BYTES_PER_PIXEL * (y * NUM_COLUMNS * GLYPH_WIDTH + x);

                        frame[offset] = if lit { fg_red } else { bg_red }; // red
                        frame[offset + 1] = if lit { fg_green } else { bg_green }; // green
                        frame[offset + 2] = if lit { fg_blue } else { bg_blue }; // blue
                        frame[offset + 3] = 0xFF; // alpha
                    }
                }
            }
        }
    }

    pub(super) fn read_status(&mut self) -> u8 {
        // TODO: Do proper status
        self.status = !self.status;
        self.status
    }
}

pub fn attribute_color_to_rgb(color: u8) -> (u8, u8, u8) {
    match color {
        0x00 => (0x00, 0x00, 0x00), // black
        0x01 => (0x00, 0x00, 0xAA), // blue
        0x02 => (0x00, 0xAA, 0x00), // green
        0x03 => (0x00, 0xAA, 0xAA), // cyan
        0x04 => (0xAA, 0x00, 0x00), // red
        0x05 => (0xAA, 0x00, 0xAA), // magenta
        0x06 => (0xAA, 0x55, 0x00), // brown
        0x07 => (0xAA, 0xAA, 0xAA), // light gray
        0x08 => (0x55, 0x55, 0x55), // dark gray
        0x09 => (0x55, 0x55, 0xFF), // light blue
        0x0A => (0x55, 0xFF, 0x55), // light green
        0x0B => (0x55, 0xFF, 0xFF), // light cyan
        0x0C => (0xFF, 0x55, 0x55), // light red
        0x0D => (0xFF, 0x55, 0xFF), // light magenta
        0x0E => (0xFF, 0xFF, 0x55), // yellow
        0x0F => (0xFF, 0xFF, 0xFF), // bright white
        _ => (255, 255, 255),       // white
    }
}
