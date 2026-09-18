use crate::memory::MemoryError;

const NUM_COLUMNS: usize = 80;
const NUM_ROWS: usize = 25;
const VIDEO_ADDR_BASE: usize = 0xB8000;

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
}
