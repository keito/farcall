use std::io::Write;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustix::termios::Winsize;

use crate::{frontend::PALETTE, machine::Machine};

#[cfg(test)]
mod tests;

const MAX_DISPLAY_WIDTH: usize = 1280;
const PADDING: usize = 8;

pub(crate) struct Renderer {
    current_frame: [u8; 640 * 200],
    previous_frame: [u8; 640 * 200],
    previous_size: (usize, usize),
}

impl Renderer {
    pub(crate) fn new() -> Self {
        Self {
            current_frame: [0u8; 640 * 200],
            previous_frame: [0u8; 640 * 200],
            previous_size: (0, 0),
        }
    }

    pub(crate) fn render(
        &mut self,
        machine: &Machine,
        winsize: Winsize,
        stdout: &mut impl Write,
    ) -> anyhow::Result<()> {
        machine.render_video(&mut self.current_frame);
        let (display_width, display_height) = calc_display_size(winsize);

        // Skip output if frame and size are unchanged
        if self.current_frame == self.previous_frame
            && self.previous_size == (display_width, display_height)
        {
            return Ok(());
        }

        if display_width == 0 || display_height == 0 {
            anyhow::bail!("Terminal is too small");
        }

        let data = encode_image(&self.current_frame, 640, 200, display_width, display_height)?;
        stdout.write_all(b"\x1b[?2026h")?; // Begin synchronized update
        for line in data {
            stdout.write_all(&line.as_bytes())?;
        }
        stdout.write_all(b"\x1b[?2026l")?; // End synchronized update
        stdout.flush()?;

        std::mem::swap(&mut self.current_frame, &mut self.previous_frame);
        self.previous_size = (display_width, display_height);

        Ok(())
    }
}

/// Encodes the given image pixels into a Vec of Kitty commands
fn encode_image(
    pixels: &[u8],
    width: usize,
    height: usize,
    display_width: usize,
    display_height: usize,
) -> anyhow::Result<Vec<String>> {
    let out_w = display_width + PADDING * 2;
    let out_h = display_height + PADDING * 2;
    let row_bytes = out_w.div_ceil(2);
    let mut out_pixels = vec![0; row_bytes * out_h];

    // Optimization: precompute lookup
    let xmap = (0..display_width)
        .map(|x| x * width / display_width)
        .collect::<Vec<_>>();

    let mut prev_source_y: Option<usize> = None;
    for y in 0..display_height {
        let source_y = y * height / display_height;
        let start = (y + PADDING) * row_bytes;

        // See if we can just copy the previous row
        if prev_source_y == Some(source_y) {
            out_pixels.copy_within((start - row_bytes)..start, start);
        } else {
            // Copy the row
            let content_start = start + PADDING / 2;
            let source_row = &pixels[source_y * width..(source_y + 1) * width];
            for (out, pair) in out_pixels[content_start..content_start + display_width / 2]
                .iter_mut()
                .zip(xmap.chunks_exact(2))
            {
                *out = (source_row[pair[0]] << 4) | source_row[pair[1]];
            }

            // Handle the one remaining pixel for odd display widths
            if display_width % 2 != 0 {
                out_pixels[content_start + display_width / 2] =
                    source_row[xmap[display_width - 1]] << 4;
            }

            prev_source_y = Some(source_y);
        }
    }

    let mut png_bytes = Vec::new();

    let mut encoder = png::Encoder::new(&mut png_bytes, out_w as u32, out_h as u32);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_palette(PALETTE.as_flattened());
    encoder.set_depth(png::BitDepth::Four);
    encoder.set_compression(png::Compression::Fast);

    let mut writer = encoder.write_header()?;
    writer.write_image_data(&out_pixels)?;
    writer.finish()?;

    Ok(kitty_messages(&png_bytes))
}

fn kitty_messages(png_bytes: &[u8]) -> Vec<String> {
    let payload = STANDARD.encode(&png_bytes);
    let chunks = payload.as_bytes().chunks(4096);
    let count = chunks.len();
    chunks
        .enumerate()
        .map(|(i, chunk)| {
            let chunk = str::from_utf8(chunk).expect("base64 output is ASCII");
            let m = if count == i + 1 { 0 } else { 1 };
            if i == 0 {
                // a=T displays the pixels immediately
                // m is whether there is more
                // C=1 specifies no cursor movement
                // i=86 is the image ID
                // q=2 turns off responses
                format!("\x1b_Gi=86,p=1,q=2,C=1,a=T,f=100,m={m};{chunk}\x1b\\")
            } else {
                format!("\x1b_Gq=2,m={m};{chunk}\x1b\\")
            }
        })
        .collect()
}

fn calc_display_size(winsize: Winsize) -> (usize, usize) {
    // Reserve some margin
    let width_px = f64::from(winsize.ws_xpixel.saturating_sub(PADDING as u16 * 2));
    let height_px = f64::from(winsize.ws_ypixel.saturating_sub(PADDING as u16 * 2));

    let image_width = width_px
        .min(height_px * (4.0 / 3.0))
        // Ensure unbounded allocations don't happen
        .min(MAX_DISPLAY_WIDTH as f64);
    let image_height = image_width * (3.0 / 4.0);

    (image_width as usize, image_height as usize)
}
