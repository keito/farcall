use super::BytePhase;

/// Emulates the DMA controller
pub(super) struct Dma {
    address: u16,
    count: u16,
    page: u8,
    mode: u8,
    masked: bool,
    byte_phase: BytePhase,
    terminal_count: bool,
}

impl Dma {
    pub(super) fn new() -> Self {
        Self {
            address: 0,
            count: 0,
            page: 0,
            mode: 0,
            masked: true,
            byte_phase: BytePhase::Low,
            terminal_count: false,
        }
    }

    #[allow(dead_code)]
    pub(super) fn read(&mut self, port: u16) -> u8 {
        match port {
            _ => todo!("Strange things have happened"),
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x04 => {
                // Write channel 2 address, low byte then high
                Self::write_register(&mut self.byte_phase, &mut self.address, value);
            }
            0x05 => {
                // Write channel 2 count, low byte then high
                Self::write_register(&mut self.byte_phase, &mut self.count, value);
            }
            0x81 => {
                // Store channel 2's page byte
                self.page = value;
            }
            0x0A => {
                // When value & 3 == 2, set masked from bit 2
                if value & 3 == 2 {
                    self.masked = value & 4 != 0;
                }
            }
            0x0B => {
                // Store mode when value & 3 == 2
                if value & 3 == 2 {
                    self.mode = value;
                }
            }
            0x0C => {
                // Reset byte phase to low; ignore the value
                self.byte_phase = BytePhase::Low;
            }
            0x0D => {
                // Master reset: reset byte phase and mask the channel;
                // further reset state will follow as we add it
                self.byte_phase = BytePhase::Low;
                self.masked = true;
                self.terminal_count = false;
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn physical_address(&self) -> u32 {
        (u32::from(self.page & 0x0F) << 16) | u32::from(self.address)
    }

    pub(super) fn count_bytes(&self) -> usize {
        usize::from(self.count) + 1
    }

    pub(super) fn is_masked(&self) -> bool {
        self.masked
    }

    pub(super) fn mode(&self) -> u8 {
        self.mode
    }

    pub(super) fn complete_transfer(&mut self, transferred_bytes: u16) {
        self.address = self.address.wrapping_add(transferred_bytes);
        self.count = self.count.wrapping_sub(transferred_bytes);
        self.masked = true;
        self.terminal_count = true;
    }

    pub(super) fn write_register(byte_phase: &mut BytePhase, register: &mut u16, value: u8) {
        match *byte_phase {
            BytePhase::Low => {
                *register = (*register & 0xFF00) | u16::from(value);
                *byte_phase = BytePhase::High;
            }
            BytePhase::High => {
                *register = (*register & 0x00FF) | (u16::from(value) << 8);
                *byte_phase = BytePhase::Low;
            }
        }
    }
}
