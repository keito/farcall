use std::time::Instant;

use crate::{
    cpu::{Cpu, Flag},
    machine::InterruptResult,
    registers::Reg8,
};

pub(super) struct Bios {
    boot_time: Instant,
}

impl Bios {
    pub(super) fn new() -> Self {
        Self {
            boot_time: Instant::now(),
        }
    }

    /// INT 11h: Emulates the Equipment List Service
    pub(super) fn equipment_list_service(
        &self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, BiosError> {
        // One floppy drive, 64 KiB motherboard RAM, 80x25 color, and nothing else
        cpu.ax = 0b0000_0000_0010_1101;
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT 12h: Emulates the Amount Conventional Ram Service
    pub(super) fn amount_conventional_ram_service(
        &self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, BiosError> {
        cpu.ax = 640; // decimal
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT 14h: Emulates the Serial Port Service
    pub(super) fn serial_port_service(&self, cpu: &mut Cpu) -> Result<InterruptResult, BiosError> {
        // Serial port unavailable
        cpu.ax = 0x8000;
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT 15h: BIOS system services
    pub(super) fn bios_system_services(&self, cpu: &mut Cpu) -> Result<InterruptResult, BiosError> {
        let op = cpu.get_reg(Reg8::Ah);
        match op {
            0x41 => {
                // Wait for External Event (IBM PC Convertible)
                // TODO: We don't support it.
                cpu.set_reg(Reg8::Ah, 0x80);
                cpu.set_flag(Flag::Carry, true);
            }
            0x88 => {
                // Extended Memory Size Determine (RAM above 1 MiB)
                // TODO: We don't support it. IBM PC/PCjr return 80h; PC/XT returns 86h
                cpu.set_reg(Reg8::Ah, 0x80);
                cpu.set_flag(Flag::Carry, true);
            }
            0xC0 => {
                // Return System Configuration Parameters (not supported on PC/PCjr)
                cpu.set_reg(Reg8::Ah, 0x80);
                cpu.set_flag(Flag::Carry, true);
            }
            _ => {
                return Err(BiosError::SystemServiceError(format!(
                    "Unsupported operation: {:#X}",
                    op
                )));
            }
        }
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT 17h: Emulates the Printer Service
    pub(super) fn printer_service(&self, cpu: &mut Cpu) -> Result<InterruptResult, BiosError> {
        // Unavailable or timed-out hardware
        cpu.set_reg(Reg8::Ah, 0x01);
        Ok(InterruptResult::ResumeExecution)
    }

    /// INT 1Ah: Time of Day Service
    pub(super) fn time_of_day_service(&self, cpu: &mut Cpu) -> Result<InterruptResult, BiosError> {
        let operation = cpu.get_reg(Reg8::Ah);
        match operation {
            0x00 => {
                // Read the clock (divide by 55 for approximate tick rate)
                let ticks = self.boot_time.elapsed().as_millis() as u32 / 55;
                cpu.cx = (ticks >> 16) as u16;
                cpu.dx = ticks as u16;
                // TODO: Support day rollover
                cpu.set_reg(Reg8::Al, 0);
                Ok(InterruptResult::ResumeExecution)
            }
            0x02 => {
                // Read the real-time clock (not supported by the 8088)
                Ok(InterruptResult::ResumeExecution)
            }
            _ => Err(BiosError::TimeOfDayServiceError(format!(
                "Unsupported operation: {operation:X}h"
            ))),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BiosError {
    #[error("BIOS System Service error: {0}")]
    SystemServiceError(String),
    #[error("Time of Day Service error: {0}")]
    TimeOfDayServiceError(String),
}
