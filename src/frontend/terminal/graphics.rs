use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Encodes the given image pixels into a Vec of Kitty commands
pub(crate) fn encode_image(
    pixels: &[u8],
    width: usize,
    height: usize,
    display_width: usize,
    display_height: usize,
) -> anyhow::Result<Vec<String>> {
    let mut new_pixels: Vec<u8> = Vec::with_capacity(display_width * display_height * 3);
    let mut prev_source_y: Option<usize> = None;
    for y in 0..display_height {
        let source_y = y * height / display_height;

        // See if we can just copy the previous row
        if prev_source_y == Some(source_y) {
            let row_offset = (y - 1) * display_width * 3;
            new_pixels.extend_from_within(row_offset..row_offset + 3 * display_width);
            continue;
        }

        // Copy the row
        for x in 0..display_width {
            let source_x = x * width / display_width;
            let source_offset = (source_y * width + source_x) * 3;
            new_pixels.extend_from_slice(&pixels[source_offset..source_offset + 3]);
        }

        prev_source_y = Some(source_y);
    }

    let mut png_bytes = Vec::new();

    let mut encoder =
        png::Encoder::new(&mut png_bytes, display_width as u32, display_height as u32);
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
                format!("\x1b_Gi=86,q=1,C=1,a=T,f=100,m={m};{chunk}\x1b\\")
            } else {
                format!("\x1b_Gm={m};{chunk}\x1b\\")
            }
        })
        .collect())
}
