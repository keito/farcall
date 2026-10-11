use std::io::Write;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rustix::termios::Winsize;

use crate::machine::Machine;

const PADDING: usize = 8;

pub(crate) struct Renderer {
    current_frame: [u8; 640 * 200 * 3],
    previous_frame: [u8; 640 * 200 * 3],
    previous_size: (usize, usize),
}

impl Renderer {
    pub(crate) fn new() -> Self {
        Self {
            current_frame: [0u8; 640 * 200 * 3],
            previous_frame: [0u8; 640 * 200 * 3],
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
        for line in data {
            stdout.write_all(&line.as_bytes())?;
        }
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
    let mut new_pixels: Vec<u8> =
        Vec::with_capacity((PADDING * 2 + display_width) * (display_height + PADDING * 2) * 3);

    let stride = (display_width + PADDING * 2) * 3;
    let vert_padding_bytes = PADDING * stride;

    // Append top padding
    new_pixels.resize(vert_padding_bytes, 0);

    let mut prev_source_y: Option<usize> = None;
    for y in 0..display_height {
        let source_y = y * height / display_height;

        // See if we can just copy the previous row
        if prev_source_y == Some(source_y) {
            let row_offset = new_pixels.len() - stride;
            new_pixels.extend_from_within(row_offset..row_offset + stride);
            continue;
        }

        // Add left margin
        new_pixels.extend_from_slice(&[0; PADDING * 3]);

        // Copy the row
        for x in 0..display_width {
            let source_x = x * width / display_width;
            let source_offset = (source_y * width + source_x) * 3;
            new_pixels.extend_from_slice(&pixels[source_offset..source_offset + 3]);
        }

        // Add right margin
        new_pixels.extend_from_slice(&[0; PADDING * 3]);

        prev_source_y = Some(source_y);
    }

    // Append bottom padding
    new_pixels.resize(new_pixels.len() + vert_padding_bytes, 0);

    let mut png_bytes = Vec::new();

    let mut encoder = png::Encoder::new(
        &mut png_bytes,
        (display_width + PADDING * 2) as u32,
        (display_height + PADDING * 2) as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);

    let mut writer = encoder.write_header()?;
    writer.write_image_data(&new_pixels)?;
    writer.finish()?;

    let payload = STANDARD.encode(&png_bytes);
    let chunks = payload.as_bytes().chunks(4096);
    let count = chunks.len();
    Ok(chunks
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
                format!("\x1b_Gi=86,q=2,C=1,a=T,f=100,m={m};{chunk}\x1b\\")
            } else {
                format!("\x1b_Gq=2,m={m};{chunk}\x1b\\")
            }
        })
        .collect())
}

fn calc_display_size(winsize: Winsize) -> (usize, usize) {
    // Reserve some margin
    let width_px = f64::from(winsize.ws_xpixel.saturating_sub(PADDING as u16 * 2));
    let height_px = f64::from(winsize.ws_ypixel.saturating_sub(PADDING as u16 * 2));

    let image_width = width_px.min(height_px * (4.0 / 3.0));
    let image_height = image_width * (3.0 / 4.0);

    (image_width as usize, image_height as usize)
}
