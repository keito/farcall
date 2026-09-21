/// Emulates a Floppy Disk Controller
pub(super) struct Fdc {
    /// Digital Output Register
    dor: u8,
    interrupt_request: bool,
}

impl Fdc {
    pub(super) fn new() -> Self {
        Self {
            dor: 0,
            interrupt_request: false,
        }
    }

    pub(super) fn read(&self, port: u16) -> u8 {
        match port {
            _ => unreachable!(),
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x3F2 => {
                if (self.dor & 0x4 == 0) && (value & 0x4 != 0) {
                    // Leaving Reset
                    self.interrupt_request = true;
                } else if (self.dor & 0x4 != 0) && (value & 0x4 == 0) {
                    // Entering Reset
                    self.interrupt_request = false;
                }
                self.dor = value;
            }
            _ => unreachable!(),
        }
    }

    /// The FDC's interrupt output pin
    pub(super) fn irq_output(&self) -> bool {
        // If we have an interrupt request and the DOR bit 3 (interrupt/DMA interface) is set
        self.interrupt_request && (self.dor & 0x08 != 0)
    }
}
