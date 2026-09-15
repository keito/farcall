#[derive(Copy, Clone)]
pub(crate) struct PhysAddr(u32);

impl PhysAddr {
    pub(crate) fn new(segment: u16, offset: u16) -> Self {
        Self((((segment as u32) << 4) + (offset as u32)) & 0xFFFFF)
    }

    pub(crate) fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

impl std::fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::UpperHex::fmt(&self.0, f)
    }
}
