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
    for y in 0..display_height {
        for x in 0..display_width {
            let source_x = x * width / display_width;
            let source_y = y * height / display_height;
            let source_offset = (source_y * width + source_x) * 3;
            new_pixels.push(pixels[source_offset]);
            new_pixels.push(pixels[source_offset + 1]);
            new_pixels.push(pixels[source_offset + 2]);
        }
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
                // c is display columns, r is display rows
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
