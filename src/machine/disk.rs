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

pub(super) struct DiskGeometry {
    pub(super) cylinders: usize,
    pub(super) heads: usize,
    pub(super) sectors_per_track: usize,
}

impl DiskGeometry {
    pub(super) fn byte_offset(&self, cylinder: usize, head: usize, sector: usize) -> Option<usize> {
        if cylinder >= self.cylinders
            || head >= self.heads
            || sector == 0
            || sector > self.sectors_per_track
        {
            return None;
        }
        let index = (cylinder * self.heads + head) * self.sectors_per_track + sector - 1;
        Some(index * 512)
    }
}

pub(super) struct FloppyDisk {
    pub(super) bytes: Vec<u8>,
    pub(super) geometry: DiskGeometry,
}

pub struct DiskImageConfig {
    pub path: PathBuf,
    pub disk_type: DiskType,
}
