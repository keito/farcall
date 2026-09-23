use std::time::Instant;

mod disk;

use crate::{
    cpu::{Cpu, Flag},
    machine::{BiosKeyboard, FloppyDisk, InterruptResult, MachineStepResult, TextVideo},
    memory::MemoryError,
    registers::Reg8,
};

pub(super) struct Bios {
    boot_time: Instant,
    pending_keyboard_read: bool,
}

impl Bios {
    pub(super) fn new() -> Self {
        Self {
            boot_time: Instant::now(),
            pending_keyboard_read: false,
        }
    }

    /// Emulates the Video Service (INT 10h)
    pub(super) fn video_service(
        &self,
        cpu: &mut Cpu,
        memory: &mut [u8],
        video: &mut TextVideo,
    ) -> Result<InterruptResult, BiosError> {
        let op = cpu.get_reg(Reg8::Ah);
        match op {
            // Set video mode
            0x00 => {
                let mode = cpu.get_reg(Reg8::Al);
                if mode != 0x03 {
                    return Err(BiosError::VideoServiceError(format!(
                        "Terribly sorry, we only support 03h; you want {:#X}h",
                        mode
                    )));
                }
                video.clear(memory)?;
                video.set_scan_lines(6, 7);
                Ok(InterruptResult::ResumeExecution)
            }
            // Set text cursor shape
            0x01 => {
                video.set_scan_lines(
                    cpu.get_reg(Reg8::Ch) as usize,
                    cpu.get_reg(Reg8::Cl) as usize,
                );
                Ok(InterruptResult::ResumeExecution)
            }
            // Read Cursor Position, Size, and Shape
            0x03 => {
                let display_page = cpu.get_reg(Reg8::Bh);
                if display_page != 0 {
                    return Err(BiosError::VideoServiceError(format!(
                        "Unsupported display page: {:#X}h",
                        display_page
                    )));
                }

                let scan_lines = video.get_scan_lines();
                cpu.set_reg(Reg8::Ch, scan_lines.0 as u8);
                cpu.set_reg(Reg8::Cl, scan_lines.1 as u8);

                let (column, row) = video.get_cursor();
                cpu.set_reg(Reg8::Dh, row as u8);
                cpu.set_reg(Reg8::Dl, column as u8);

                Ok(InterruptResult::ResumeExecution)
            }
            // Select active display page
            0x05 => {
                let display_page = cpu.get_reg(Reg8::Al);
                if display_page != 0 {
                    return Err(BiosError::VideoServiceError(format!(
                        "Unsupported display page: {:#X}h",
                        display_page
                    )));
                }
                Ok(InterruptResult::ResumeExecution)
            }
            // BIOS character output
            0x0E => {
                let c = cpu.get_reg(Reg8::Al);
                video.write_char(memory, c)?;
                Ok(InterruptResult::ResumeExecution)
            }
            // Get current video mode
            0x0F => {
                cpu.set_reg(Reg8::Al, 0x3); // 80x25 color text mode
                cpu.set_reg(Reg8::Ah, 80); // 80 columns in decimal
                cpu.set_reg(Reg8::Bh, 0); // active display page
                Ok(InterruptResult::ResumeExecution)
            }
            0x10..=0xFF => {
                // Original PC BIOS ignores video functions beyond 0Fh
                Ok(InterruptResult::ResumeExecution)
            }
            _ => Err(BiosError::VideoServiceError(format!(
                "Unsupported command: {:#X}h",
                op
            ))),
        }
    }

    /// INT 11h: Emulates the Equipment List Service
    pub(super) fn equipment_list_service(
        &self,
        cpu: &mut Cpu,
    ) -> Result<InterruptResult, BiosError> {
        // 2 floppy drives, 64 KiB motherboard RAM, 80x25 color, and nothing else
        cpu.ax = 0b0000_0000_0110_1101;
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

    /// INT 16h: Keyboard Service
    pub(super) fn keyboard_service(
        &mut self,
        cpu: &mut Cpu,
        bios_keyboard: &mut BiosKeyboard,
        verbosity: u8,
    ) -> Result<InterruptResult, BiosError> {
        let operation = cpu.get_reg(Reg8::Ah);
        if verbosity > 0 {
            eprintln!("\x1b[1;93mKeyboard Service op {:X}h\x1b[0m", operation);
        }
        match operation {
            0x00 => {
                // Read key
                if let Some(first_key) = bios_keyboard.pop() {
                    cpu.set_reg(Reg8::Al, first_key.ascii.unwrap_or(0x0));
                    cpu.set_reg(Reg8::Ah, first_key.scan_code);
                    Ok(InterruptResult::ResumeExecution)
                } else {
                    self.pending_keyboard_read = true;
                    Ok(InterruptResult::WaitingForInput)
                }
            }
            0x01 => {
                // Check key
                if let Some(first_key) = bios_keyboard.peek() {
                    cpu.set_reg(Reg8::Al, first_key.ascii.unwrap_or(0x0));
                    cpu.set_reg(Reg8::Ah, first_key.scan_code);
                    cpu.set_flag(Flag::Zero, false);
                } else {
                    cpu.set_flag(Flag::Zero, true);
                }
                Ok(InterruptResult::ResumeExecution)
            }
            // 0x02 => {
            //     // Read shift status
            //     Ok(InterruptResult::ResumeExecution)
            // }
            _ => Err(BiosError::KeyboardServiceError(
                "Unsupported operation".to_string(),
            )),
        }
    }

    pub(super) fn pending_keyboard_read(&self) -> bool {
        self.pending_keyboard_read
    }

    pub(super) fn handle_pending_keyboard_read(
        &mut self,
        cpu: &mut Cpu,
        bios_keyboard: &mut BiosKeyboard,
    ) -> Result<MachineStepResult, BiosError> {
        if let Some(key) = bios_keyboard.pop() {
            cpu.set_reg(Reg8::Al, key.ascii.unwrap_or(0x0));
            cpu.set_reg(Reg8::Ah, key.scan_code);
            self.pending_keyboard_read = false;
            return Ok(MachineStepResult::Continue);
        }
        Ok(MachineStepResult::WaitingForInput)
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

    pub(super) fn handle_bios_service(
        &mut self,
        interrupt: u8,
        cpu: &mut Cpu,
        memory: &mut [u8],
        bios_keyboard: &mut BiosKeyboard,
        floppy_images: &[Option<FloppyDisk>; 2],
        video: &mut TextVideo,
        verbosity: u8,
    ) -> Result<InterruptResult, BiosError> {
        match interrupt {
            0x10 => self.video_service(cpu, memory, video),
            0x11 => self.equipment_list_service(cpu),
            0x12 => self.amount_conventional_ram_service(cpu),
            0x13 => self.disk_service(cpu, memory, floppy_images),
            0x14 => self.serial_port_service(cpu),
            0x15 => self.bios_system_services(cpu),
            0x16 => self.keyboard_service(cpu, bios_keyboard, verbosity),
            0x17 => self.printer_service(cpu),
            0x1A => self.time_of_day_service(cpu),
            _ => unreachable!("Interrupts should be filtered earlier"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BiosError {
    #[error("BIOS Keyboard Service error: {0}")]
    KeyboardServiceError(String),
    #[error(transparent)]
    Memory(#[from] MemoryError),
    #[error("BIOS System Service error: {0}")]
    SystemServiceError(String),
    #[error("BIOS Time of Day Service error: {0}")]
    TimeOfDayServiceError(String),
    #[error("BIOS Video Service error: {0}")]
    VideoServiceError(String),
}
