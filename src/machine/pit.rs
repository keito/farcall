/// Emulates the Programmable Interval Timer (8253)
pub(super) struct Pit {}

impl Pit {
    pub(super) fn new() -> Self {
        Self {}
    }

    pub(super) fn read(&self, port: u16) -> u8 {
        match port {
            _ => todo!(),
        }
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            _ => todo!(),
        }
    }
}
