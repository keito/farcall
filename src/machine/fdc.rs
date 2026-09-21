use std::collections::VecDeque;

/// Emulates a Floppy Disk Controller
pub(super) struct Fdc {
    /// Digital Output Register
    dor: u8,
    interrupt_request: bool,
    result_bytes: VecDeque<u8>,
}

impl Fdc {
    pub(super) fn new() -> Self {
        Self {
            dor: 0,
            interrupt_request: false,
            result_bytes: VecDeque::new(),
        }
    }

    pub(super) fn read(&mut self, port: u16) -> u8 {
        match port {
            // Idle vs result bytes available status
            0x3F4 => {
                if self.result_bytes.is_empty() {
                    0x80
                } else {
                    0xD0
                }
            }
            // Result bytes from commands
            0x3F5 => self.result_bytes.pop_front().unwrap_or(0),
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
                    self.result_bytes.clear();
                    self.interrupt_request = false;
                }
                self.dor = value;
            }
            0x3F5 => {
                // Command bytes
                if value == 0x08 {
                    // TODO: Fix the check here
                    if self.interrupt_request {
                        // Sense Interrupt Status
                        self.interrupt_request = false;
                        // Ready-change status for drive A (ST0)
                        self.result_bytes.push_back(0xC0);
                        // Cylinder 0
                        self.result_bytes.push_back(0x00);
                    } else {
                        // Invalid command
                        self.result_bytes.push_back(0x80);
                    }
                }
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
