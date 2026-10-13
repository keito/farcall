use super::{CgaVideo, GLYPH_HEIGHT, NUM_COLUMNS, NUM_ROWS, VIDEO_ADDR_BASE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextCell {
    pub character: u8,
    pub attribute: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextCursor {
    pub column: usize,
    pub row: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextScreen {
    pub blink_enabled: bool,
    pub cells: [[TextCell; NUM_COLUMNS]; NUM_ROWS],
    pub video_enabled: bool,
    pub cursor: Option<TextCursor>,
}

impl CgaVideo {
    /// Renders video memory to a TextScreen, if we can. This is for the text-only frontend.
    pub(crate) fn render_text_screen(&self, memory: &[u8]) -> Option<TextScreen> {
        if self.is_graphics() {
            return None;
        }

        let is_video = self.is_video();

        let cursor_enabled = (self.cursor_start_reg & 0x60) != 0x20;
        let cursor_addr = usize::from(self.cursor_address);
        let cursor_start = self.cursor_start_reg & 0x1F;
        let cursor_end = self.cursor_end_reg & 0x1F;
        let cursor = (cursor_enabled
            && is_video
            && cursor_addr < NUM_COLUMNS * NUM_ROWS
            && cursor_start <= cursor_end
            && cursor_start < GLYPH_HEIGHT as u8)
            .then(|| TextCursor {
                column: cursor_addr % NUM_COLUMNS,
                row: cursor_addr / NUM_COLUMNS,
            });

        let mut screen = TextScreen {
            blink_enabled: self.mode & 0x20 != 0,
            cells: [[TextCell {
                character: 0,
                attribute: 0,
            }; NUM_COLUMNS]; NUM_ROWS],
            video_enabled: is_video,
            cursor,
        };

        for row in 0..NUM_ROWS {
            for column in 0..NUM_COLUMNS {
                let character = memory[VIDEO_ADDR_BASE + 2 * (row * NUM_COLUMNS + column)];
                let attribute = memory[VIDEO_ADDR_BASE + 2 * (row * NUM_COLUMNS + column) + 1];

                screen.cells[row][column] = TextCell {
                    character,
                    attribute,
                };
            }
        }

        Some(screen)
    }
}
