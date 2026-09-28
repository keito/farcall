mod bios;
mod pc;
#[cfg(test)]
mod tests;

pub(crate) use bios::{ModifierState, winit_to_bios_key_stroke};
pub(crate) use pc::winit_to_pc_scan_code;
