pub(super) struct Ppi {
    keyboard_scan_code: Option<u8>,
    port_b: u8,
    sw1: u8,
    sw2: u8,
}

impl Ppi {
    pub(super) fn new() -> Self {
        Self {
            keyboard_scan_code: None,
            port_b: 0,
            // 2 floppies, 80 cols CGA, no coprocessor, 64KB motherboard memory
            sw1: 0b0110_1101,
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
                    self.keyboard_scan_code.unwrap_or(0)
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
            0x61 => {
                self.port_b = value;
                if value & 0x80 != 0 {
                    self.keyboard_scan_code = None;
                }
            }
            0x63 => (), // Recognize the BIOS's 99h init command
            _ => (),
        }
    }

    pub(super) fn try_receive_scan_code(&mut self, scan_code: u8) -> bool {
        // Make sure we can accept a scan code
        if self.keyboard_scan_code.is_some() || self.port_b & 0x80 != 0 || self.port_b & 0x40 == 0 {
            return false;
        }

        self.keyboard_scan_code = Some(scan_code);
        true
    }

    pub(super) fn irq_output(&self) -> bool {
        self.keyboard_scan_code.is_some()
    }
}
