use base64::{Engine as _, engine::general_purpose::STANDARD};

pub(crate) fn image_testings() -> Vec<String> {
    let pixels: Vec<u8> = [
        0xFF, 0x55, 0x00, 0x00, 0xAA, 0x00, 0x00, 0x55, 0xFF, 0xAA, 0x00, 0x00,
    ]
    .repeat(512);
    encode_image(&pixels, 64, 32, 128, 32)
}

/// Encodes the given image pixels into a Vec of Kitty commands
pub(crate) fn encode_image(
    pixels: &[u8],
    width: usize,
    height: usize,
    display_columns: usize,
    display_rows: usize,
) -> Vec<String> {
    let payload = STANDARD.encode(pixels);
    let chunks = payload.as_bytes().chunks(4096);
    let count = chunks.len();
    chunks
        .enumerate()
        .map(|(i, chunk)| {
            let chunk = str::from_utf8(chunk).expect("base64 output is ASCII");
            let m = if count == i + 1 { 0 } else { 1 };
            if i == 0 {
                // a=T displays the pixels immediately
                // f is bits per pixel (3x8), s is width, v is height
                // c is display columns, r is display rows
                // m is whether there is more
                format!(
                    "\x1b_Ga=T,f=24,s={width},v={height},c={display_columns},r={display_rows},m={m};{chunk}\x1b\\"
                )
            } else {
                format!("\x1b_Gm={m};{chunk}\x1b\\")
            }
        })
        .collect()
}
