pub(super) struct Ppi {
    port_b: u8,
    sw1: u8,
    sw2: u8,
}

impl Ppi {
    pub(super) fn new() -> Self {
        Self {
            port_b: 0,
            // 1 floppy, 80 cols CGA, no coprocessor, 64KB motherboard memory
            sw1: 0b0010_1101,
            // 18 x 32 KB (576 KB) beyond the first 64 KB (i.e., 640 KB)
            sw2: 18,
        }
    }

    pub(super) fn read(&self, port: u16) -> u8 {
        match port {
            0x60 => {
                if self.port_b & 0x80 != 0 {
                    self.sw1
                } else {
                    // TODO: keyboard data
                    0
                }
            }
            0x61 => self.port_b,
            0x62 => {
                if self.port_b & 0x04 != 0 {
                    self.sw2 & 0x0F
                } else {
                    (self.sw2 >> 4) & 1
                }
            }
            _ => 0,
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x61 => self.port_b = value,
            0x63 => (), // Recognize the BIOS's 99h init command
            _ => (),
        }
    }
}
