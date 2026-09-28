use base64::{Engine as _, engine::general_purpose::STANDARD};

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
                // C=1 specifies no cursor movement
                // i=86 is the image ID
                format!(
                    "\x1b_Gi=86,q=1,C=1,a=T,f=24,s={width},v={height},c={display_columns},r={display_rows},m={m};{chunk}\x1b\\"
                )
            } else {
                format!("\x1b_Gm={m};{chunk}\x1b\\")
            }
        })
        .collect()
}
