use std::path::PathBuf;

#[cfg(test)]
mod tests;

#[derive(Clone, clap::ValueEnum)]
pub enum DiskType {
    #[value(name = "180k")]
    Floppy180,
    #[value(name = "360k")]
    Floppy360,
    #[value(name = "720k")]
    Floppy720,
}

#[derive(Clone, Debug)]
pub struct DiskGeometry {
    pub(super) cylinders: usize,
    pub(super) heads: usize,
    pub(super) sectors_per_track: usize,
}

impl DiskGeometry {
    pub(super) fn byte_offset(&self, cylinder: usize, head: usize, sector: usize) -> Option<u64> {
        if cylinder >= self.cylinders
            || head >= self.heads
            || sector == 0
            || sector > self.sectors_per_track
        {
            return None;
        }
        let index = (cylinder * self.heads + head) * self.sectors_per_track + sector - 1;
        Some(index as u64 * 512)
    }
}

pub(super) struct FloppyDisk {
    pub(super) bytes: Vec<u8>,
    pub(super) geometry: DiskGeometry,
    pub(super) write_protected: bool,
}

pub struct DiskImageConfig {
    pub path: PathBuf,
    pub disk_type: DiskType,
    pub write_protected: bool,
}

pub fn parse_disk_geometry(text: &str) -> Result<DiskGeometry, String> {
    let parts = text.split(',').collect::<Vec<_>>();
    let [cylinders, heads, sectors] = parts.as_slice() else {
        return Err("Expected C,H,S values - e.g. 32,16,63".to_string());
    };
    Ok(DiskGeometry {
        cylinders: parse_chs_component(cylinders, "cylinders", 16383)?,
        heads: parse_chs_component(heads, "heads", 16)?,
        sectors_per_track: parse_chs_component(sectors, "sectors per track", 63)?,
    })
}

fn parse_chs_component(text: &str, name: &str, max: usize) -> Result<usize, String> {
    let value = text
        .parse::<usize>()
        .map_err(|_| format!("{name} must be an integer between 1 and {max}"))?;
    if value == 0 || value > max {
        return Err(format!("{name} must be between 1 and {max}"));
    }
    Ok(value)
}
