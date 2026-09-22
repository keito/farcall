use core::fmt::Debug;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use thiserror::Error;

use crate::cpu::{Cpu, CpuError, CpuStepResult, Flag};
use crate::machine::bios_keyboard::BiosKeyStroke;
use crate::memory::{
    MemoryError, PhysAddr, SegmentedAddr, validate_buffer, write_buffer, write_byte, write_word,
};
use crate::registers::Reg8;
use bios_keyboard::BiosKeyboard;
use disk::{DiskGeometry, FloppyDisk};
use dma::Dma;
use dos::Dos;
use fdc::{Fdc, FdcReadData};
use pc_keyboard::PcKeyboard;
use pic::Pic;
use pit::Pit;
use ppi::Ppi;
use video::TextVideo;

pub use disk::DiskType;

pub mod bios_keyboard;
mod disk;
mod dma;
mod dos;
mod fdc;
mod pc_keyboard;
mod pic;
mod pit;
mod ppi;
#[cfg(test)]
mod tests;
mod video;

// Owns everything
pub struct Machine {
    bios_keyboard: BiosKeyboard,
    bios_pending_read: bool,
    boot_time: Instant,
    cpu: Cpu,
    dma: Dma,
    dos: Dos,
    fdc: Fdc,
    floppy_image: Option<FloppyDisk>,
    keyboard_input_mode: KeyboardInputMode,
    last_step_time: Instant,
    load_mode: LoadMode,
    memory: Vec<u8>,
    pc_keyboard: PcKeyboard,
    pic: Pic,
    pit: Pit,
    ppi: Ppi,
    /// Used to directly raise INT 09h when emulating a BIOS
    synthetic_int09_pending: bool,
    text_video: TextVideo,
    /// Fraction of a PIT tick remaining after a step
    tick_remainder: u128,
    verbosity: u8,
}

#[derive(Error, Debug)]
pub enum MachineError {
    #[error("Not enough memory; offset {0} exceeds {1} bytes")]
    NotEnoughMemory(usize, usize),
    #[error("Unsupported interrupt: {0:X}h")]
    UnsupportedInterrupt(u8),
    #[error("Unsupported service: {0:X}h")]
    UnsupportedService(u8),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Read Service error: {0}")]
    ReadServiceError(String),
    #[error("Write Service error: {0}")]
    WriteServiceError(String),
    #[error("Print String Service error: {0}")]
    PrintStringServiceError(String),
    #[error("Memory Error: {0}")]
    MemoryError(#[from] MemoryError),
    #[error("Video Service error: {0}")]
    VideoServiceError(String),
    #[error("CPU Error: {0}")]
    CpuError(#[from] CpuError),
    #[error("Boot Sector error: {0}")]
    BootSectorError(String),
    #[error("Time of Day Service error: {0}")]
    TimeOfDayServiceError(String),
    #[error("Keyboard Service error: {0}")]
    KeyboardServiceError(String),
    #[error("BIOS System Service error: {0}")]
    BiosSystemServiceError(String),
    #[error("PIC error: {0}")]
    PicError(String),
    #[error("Disk error: {0}")]
    DiskError(String),
}

enum InterruptResult {
    ResumeExecution,
    WaitingForInput,
    Exit(u8),
}

pub enum MachineStepResult {
    Continue,
    WaitingForInput,
    Exit(u8),
}

enum BytePhase {
    Low,
    High,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum KeyboardInputMode {
    BiosKeystrokes,
    PcHardware,
}

impl Machine {
    pub fn new(
        file_path: &str,
        load_mode: LoadMode,
        image_type: Option<DiskType>,
        verbosity: u8,
        rom: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let memory_size = 0x100_000; // Physical memory: 1 MiB

        let mut machine = Machine {
            bios_keyboard: BiosKeyboard::new(),
            bios_pending_read: false,
            boot_time: Instant::now(),
            last_step_time: Instant::now(),
            cpu: Cpu::new(verbosity),
            dma: Dma::new(),
            dos: Dos::new(),
            fdc: Fdc::new(),
            floppy_image: None,
            pc_keyboard: PcKeyboard::new(),
            synthetic_int09_pending: false,
            keyboard_input_mode: match load_mode {
                LoadMode::Rom => KeyboardInputMode::PcHardware,
                _ => KeyboardInputMode::BiosKeystrokes,
            },
            load_mode: load_mode.clone(),
            memory: vec![0; memory_size],
            pic: Pic::new(),
            pit: Pit::new(),
            ppi: Ppi::new(),
            text_video: TextVideo::new(),
            tick_remainder: 0,
            verbosity,
        };
        machine.text_video.clear(&mut machine.memory)?;

        let data: Vec<u8> = fs::read(file_path)?;
        match load_mode {
            LoadMode::Com => machine.load_com(&data)?,
            LoadMode::Boot => machine.load_boot_sector(data, image_type)?,
            LoadMode::Rom => {
                machine.load_boot_sector(data, image_type)?;
                machine.load_rom(rom.unwrap())?;
            }
        }

        if load_mode != LoadMode::Rom {
            // Create a stub entry in the Interrupt Vector Table for INT 09h
            // TODO: Do this properly
            let iret_addr = PhysAddr::new(0xF000, 0x0100);
            write_byte(&mut machine.memory, iret_addr, 0xCF)?; // IRET
            // Put that memory address into IVT
            write_word(&mut machine.memory, SegmentedAddr::new(0x0, 9 * 4), 0x0100)?; // Offset
            write_word(
                &mut machine.memory,
                SegmentedAddr::new(0x0, 9 * 4 + 2),
                0xF000,
            )?; // Segment
        }

        Ok(machine)
    }

    pub fn render_video(&self, frame: &mut [u8]) {
        self.text_video.render(&self.memory, frame);
    }

    /// Used for COM and Boot modes
    pub fn enqueue_bios_keystroke(&mut self, key: BiosKeyStroke) {
        self.bios_keyboard.enqueue(key);
        self.synthetic_int09_pending = true;
    }

    /// Used for ROM mode
    pub fn enqueue_pc_keyboard_event(&mut self, scan_code: u8, pressed: bool) {
        self.pc_keyboard.enqueue(scan_code, pressed);
    }

    /// Emulates the Video Service (INT 10h)
    fn video_service(&mut self) -> Result<InterruptResult, MachineError> {
        let op = self.cpu.get_reg(Reg8::Ah);
        match op {
            // Set video mode
            0x00 => {
                let mode = self.cpu.get_reg(Reg8::Al);
                if mode != 0x03 {
                    return Err(MachineError::VideoServiceError(format!(
                        "Terribly sorry, we only support 03h; you want {:#X}h",
                        mode
                    )));
                }
                self.text_video.clear(&mut self.memory)?;
                self.text_video.set_scan_lines(6, 7);
                Ok(InterruptResult::ResumeExecution)
            }
            // Set text cursor shape
            0x01 => {
                self.text_video.set_scan_lines(
                    self.cpu.get_reg(Reg8::Ch) as usize,
                    self.cpu.get_reg(Reg8::Cl) as usize,
                );
                Ok(InterruptResult::ResumeExecution)
            }
            // Read Cursor Position, Size, and Shape
            0x03 => {
                let display_page = self.cpu.get_reg(Reg8::Bh);
                if display_page != 0 {
                    return Err(MachineError::VideoServiceError(format!(
                        "Unsupported display page: {:#X}h",
                        display_page
                    )));
                }

                let scan_lines = self.text_video.get_scan_lines();
                self.cpu.set_reg(Reg8::Ch, scan_lines.0 as u8);
                self.cpu.set_reg(Reg8::Cl, scan_lines.1 as u8);

                let (column, row) = self.text_video.get_cursor();
                self.cpu.set_reg(Reg8::Dh, row as u8);
                self.cpu.set_reg(Reg8::Dl, column as u8);

                Ok(InterruptResult::ResumeExecution)
            }
            // Select active display page
            0x05 => {
                let display_page = self.cpu.get_reg(Reg8::Al);
                if display_page != 0 {
                    return Err(MachineError::VideoServiceError(format!(
                        "Unsupported display page: {:#X}h",
                        display_page
                    )));
                }
                Ok(InterruptResult::ResumeExecution)
            }
            // BIOS character output
            0x0E => {
                let c = self.cpu.get_reg(Reg8::Al);
                self.text_video.write_char(&mut self.memory, c)?;
                Ok(InterruptResult::ResumeExecution)
            }
            // Get current video mode
            0x0F => {
                self.cpu.set_reg(Reg8::Al, 0x3); // 80x25 color text mode
                self.cpu.set_reg(Reg8::Ah, 80); // 80 columns in decimal
                self.cpu.set_reg(Reg8::Bh, 0); // active display page
                Ok(InterruptResult::ResumeExecution)
            }
            0x10..=0xFF => {
                // Original PC BIOS ignores video functions beyond 0Fh
                Ok(InterruptResult::ResumeExecution)
            }
            _ => Err(MachineError::VideoServiceError(format!(
                "Unsupported command: {:#X}h",
                op
            ))),
        }
    }

    /// Emulates the Equipment List Service
    fn equipment_list_service(&mut self) -> Result<InterruptResult, MachineError> {
        // One floppy drive, 64 KiB motherboard RAM, 80x25 color, and nothing else
        self.cpu.ax = 0b0000_0000_0010_1101;
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the Serial Port Service
    fn serial_port_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Serial port unavailable
        self.cpu.ax = 0x8000;
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the Printer Service
    fn printer_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Unavailable or timed-out hardware
        self.cpu.set_reg(Reg8::Ah, 0x01);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the Amount Conventional Ram Service
    fn amount_conventional_ram_service(&mut self) -> Result<InterruptResult, MachineError> {
        self.cpu.ax = 640; // decimal
        Ok(InterruptResult::ResumeExecution)
    }

    fn time_of_day_service(&mut self) -> Result<InterruptResult, MachineError> {
        let operation = self.cpu.get_reg(Reg8::Ah);
        match operation {
            0x00 => {
                // Read the clock (divide by 55 for approximate tick rate)
                let ticks = self.boot_time.elapsed().as_millis() as u32 / 55;
                self.cpu.cx = (ticks >> 16) as u16;
                self.cpu.dx = ticks as u16;
                // TODO: Support day rollover
                self.cpu.set_reg(Reg8::Al, 0);
                Ok(InterruptResult::ResumeExecution)
            }
            0x02 => {
                // Read the real-time clock (not supported by the 8088)
                Ok(InterruptResult::ResumeExecution)
            }
            _ => Err(MachineError::TimeOfDayServiceError(format!(
                "Unsupported operation: {operation:X}h"
            ))),
        }
    }

    // Keyboard Service (INT 16h)
    fn keyboard_service(&mut self) -> Result<InterruptResult, MachineError> {
        let operation = self.cpu.get_reg(Reg8::Ah);
        if self.verbosity > 0 {
            eprintln!("\x1b[1;93mKeyboard Service op {:X}h\x1b[0m", operation);
        }
        match operation {
            0x00 => {
                // Read key
                if let Some(first_key) = self.bios_keyboard.pop() {
                    self.cpu.set_reg(Reg8::Al, first_key.ascii.unwrap_or(0x0));
                    self.cpu.set_reg(Reg8::Ah, first_key.scan_code);
                    Ok(InterruptResult::ResumeExecution)
                } else {
                    self.bios_pending_read = true;
                    Ok(InterruptResult::WaitingForInput)
                }
            }
            0x01 => {
                // Check key
                if let Some(first_key) = self.bios_keyboard.peek() {
                    self.cpu.set_reg(Reg8::Al, first_key.ascii.unwrap_or(0x0));
                    self.cpu.set_reg(Reg8::Ah, first_key.scan_code);
                    self.cpu.set_flag(Flag::Zero, false);
                } else {
                    self.cpu.set_flag(Flag::Zero, true);
                }
                Ok(InterruptResult::ResumeExecution)
            }
            // 0x02 => {
            //     // Read shift status
            //     Ok(InterruptResult::ResumeExecution)
            // }
            _ => Err(MachineError::KeyboardServiceError(
                "Unsupported operation".to_string(),
            )),
        }
    }

    // INT 15h
    fn bios_system_services(&mut self) -> Result<InterruptResult, MachineError> {
        let op = self.cpu.get_reg(Reg8::Ah);
        match op {
            0x41 => {
                // Wait for External Event (IBM PC Convertible)
                // TODO: We don't support it.
                self.cpu.set_reg(Reg8::Ah, 0x80);
                self.cpu.set_flag(Flag::Carry, true);
            }
            0x88 => {
                // Extended Memory Size Determine (RAM above 1 MiB)
                // TODO: We don't support it. IBM PC/PCjr return 80h; PC/XT returns 86h
                self.cpu.set_reg(Reg8::Ah, 0x80);
                self.cpu.set_flag(Flag::Carry, true);
            }
            0xC0 => {
                // Return System Configuration Parameters (not supported on PC/PCjr)
                self.cpu.set_reg(Reg8::Ah, 0x80);
                self.cpu.set_flag(Flag::Carry, true);
            }
            _ => {
                return Err(MachineError::BiosSystemServiceError(format!(
                    "Unsupported operation: {:#X}",
                    op
                )));
            }
        }
        Ok(InterruptResult::ResumeExecution)
    }

    fn handle_guest_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        self.cpu.enter_interrupt(&mut self.memory, interrupt)?;
        Ok(InterruptResult::ResumeExecution)
    }

    fn handle_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        // With a BIOS loaded, send all interrupts to the guest BIOS
        if self.load_mode == LoadMode::Rom {
            return self.handle_guest_interrupt(interrupt);
        }
        match interrupt {
            // BIOS services
            0x10 => self.video_service(),
            0x11 => self.equipment_list_service(),
            0x12 => self.amount_conventional_ram_service(),
            0x13 => self.disk_service(),
            0x14 => self.serial_port_service(),
            0x15 => self.bios_system_services(),
            0x16 => self.keyboard_service(),
            0x17 => self.printer_service(),
            0x1A => self.time_of_day_service(),
            0x20 => {
                // In boot mode, send to the guest
                if self.load_mode == LoadMode::Boot {
                    return self.handle_guest_interrupt(interrupt);
                }
                Ok(InterruptResult::Exit(0))
            }
            0x21 => {
                // DOS services
                // In boot mode, send to the guest
                if self.load_mode == LoadMode::Boot {
                    return self.handle_guest_interrupt(interrupt);
                }
                // AH contains the service ID
                match self.cpu.get_reg(Reg8::Ah) {
                    0x01 => self.dos.read_char_with_echo_service(),
                    0x02 => self.dos.output_char_service(
                        &self.cpu,
                        &mut self.memory,
                        &mut self.text_video,
                    ),
                    0x09 => self.dos.print_string_service(
                        &mut self.cpu,
                        &mut self.memory,
                        &mut self.text_video,
                    ),
                    0x0C => self
                        .dos
                        .flush_and_read_service(&mut self.cpu, &mut self.bios_keyboard),
                    0x30 => self.dos.dos_version_service(&mut self.cpu),
                    0x3E => self.dos.close_handle_service(&mut self.cpu),
                    0x3F => self.dos.read_service(&mut self.cpu, &mut self.memory),
                    0x40 => self.dos.write_service(
                        &mut self.cpu,
                        &mut self.memory,
                        &mut self.text_video,
                    ),
                    0x45 => self.dos.duplicate_file_handle_service(&mut self.cpu),
                    0x4C => self.dos.termination_service(&self.cpu),
                    service => Err(MachineError::UnsupportedService(service)),
                }
            }
            0x28 => {
                // DOS idle
                // In boot mode, send to the guest
                if self.load_mode == LoadMode::Boot {
                    return self.handle_guest_interrupt(interrupt);
                }
                // In COM mode, noop for now
                // TODO: support guest-installed handlers
                Ok(InterruptResult::ResumeExecution)
            }
            0x29 => {
                // Fast console output
                // In boot mode, send to the guest
                if self.load_mode == LoadMode::Boot {
                    return self.handle_guest_interrupt(interrupt);
                }
                // TODO: Emulate in COM mode
                Err(MachineError::UnsupportedInterrupt(interrupt))
            }
            // By default, send interrupts to the guest
            _ => self.handle_guest_interrupt(interrupt),
        }
    }

    fn advance_devices(&mut self) {
        let now = Instant::now();
        let ns_elapsed = now.duration_since(self.last_step_time).as_nanos();
        let scaled = ns_elapsed * 1_193_182 + self.tick_remainder;
        let ticks = scaled / 1_000_000_000;
        self.tick_remainder = scaled % 1_000_000_000;
        self.last_step_time = now;

        for _ in 0..ticks {
            self.pit.advance(1);
            self.pic.set_irq_line(0, self.pit.output0());
        }
    }

    pub fn step(&mut self) -> Result<MachineStepResult, MachineError> {
        if self.bios_pending_read {
            if let Some(key) = self.bios_keyboard.pop() {
                self.cpu.set_reg(Reg8::Al, key.ascii.unwrap_or(0x0));
                self.cpu.set_reg(Reg8::Ah, key.scan_code);
                self.bios_pending_read = false;
                return Ok(MachineStepResult::Continue);
            }
            return Ok(MachineStepResult::WaitingForInput);
        }

        if self.dos.has_pending_input() {
            return self.dos.handle_pending_input(
                &mut self.cpu,
                &mut self.memory,
                &mut self.bios_keyboard,
                &mut self.text_video,
            );
        }

        // Advance non-CPU devices
        self.advance_devices();

        if self.keyboard_input_mode == KeyboardInputMode::PcHardware {
            if let Some(scan_code) = self.pc_keyboard.peek() {
                if self.ppi.try_receive_scan_code(scan_code) {
                    self.pc_keyboard.pop_front();
                }
                // Synchronize PIC with PPI
                self.pic.set_irq_line(1, self.ppi.irq_output());
            }
        }

        // Check PIC for pending interrupts
        if self.pic.interrupt_pending() && self.cpu.get_flag(Flag::InterruptEnable) {
            let vector = self.pic.acknowledge()?;
            self.cpu.enter_interrupt(&mut self.memory, vector)?;
        }

        // Deliver keyboard IRQ if emulating BIOS keystrokes
        if self.keyboard_input_mode == KeyboardInputMode::BiosKeystrokes
            && self.synthetic_int09_pending
            && self.cpu.get_flag(Flag::InterruptEnable)
        {
            self.synthetic_int09_pending = false;
            self.cpu.enter_interrupt(&mut self.memory, 0x09)?;
        }

        let result = self.cpu.step(&mut self.memory)?;
        let outcome = match result {
            CpuStepResult::Continue => Ok(MachineStepResult::Continue),
            CpuStepResult::Interrupt(interrupt) => {
                if self.verbosity > 0 {
                    eprintln!("\x1b[1;91mInterrupt {interrupt:X}h\x1b[0m");
                }
                let interrupt_result = self.handle_interrupt(interrupt)?;
                match interrupt_result {
                    InterruptResult::ResumeExecution => Ok(MachineStepResult::Continue),
                    InterruptResult::WaitingForInput => Ok(MachineStepResult::WaitingForInput),
                    InterruptResult::Exit(code) => Ok(MachineStepResult::Exit(code)),
                }
            }
            CpuStepResult::PortRead8 { port } => {
                match port {
                    0x20 | 0x21 => {
                        let result = self.pic.read(port);
                        self.cpu.set_reg(Reg8::Al, result);
                    }
                    0x40..=0x43 => {
                        let result = self.pit.read(port);
                        self.cpu.set_reg(Reg8::Al, result);
                    }
                    0x3DA => {
                        // Video Service
                        self.cpu.set_reg(Reg8::Al, self.text_video.read_status());
                    }
                    0x3F4 | 0x3F5 => {
                        let result = self.fdc.read(port);
                        self.cpu.set_reg(Reg8::Al, result);
                        // Synchronize the PIC
                        self.pic.set_irq_line(6, self.fdc.irq_output());
                    }
                    0x60..=0x63 => {
                        let result = self.ppi.read(port);
                        self.cpu.set_reg(Reg8::Al, result);
                    }
                    _ => {
                        self.cpu.set_reg(Reg8::Al, 0xFF);
                    }
                };
                Ok(MachineStepResult::Continue)
            }
            CpuStepResult::PortRead16 { .. } => {
                self.cpu.ax = 0xFFFF;
                Ok(MachineStepResult::Continue)
            }
            CpuStepResult::PortWrite8 { port, value } => {
                self.write_port8(port, value)?;
                Ok(MachineStepResult::Continue)
            }
            CpuStepResult::PortWrite16 { port, value } => {
                self.write_port8(port, (value & 0xFF) as u8)?;
                self.write_port8(port.wrapping_add(1), (value >> 8) as u8)?;
                Ok(MachineStepResult::Continue)
            }
        };
        self.cpu.trace();
        outcome
    }

    fn write_port8(&mut self, port: u16, value: u8) -> Result<(), MachineError> {
        match port {
            0x04 | 0x05 | 0x81 | 0x0A | 0x0B | 0x0C | 0x0D => {
                self.dma.write(port, value);
            }
            0x20 | 0x21 => {
                self.pic.write(port, value);
            }
            0x40..=0x43 => {
                self.pit.write(port, value);
                // Synchronize PIC's IRQ lines with the PIT's output
                self.pic.set_irq_line(0, self.pit.output0());
            }
            0x60..=0x63 => {
                self.ppi.write(port, value);
                self.pic.set_irq_line(1, self.ppi.irq_output());
            }
            0x3D4 | 0x3D5 => {
                self.text_video.write(port, value);
            }
            0x3F2 | 0x3F5 => {
                self.fdc.write(port, value);
                if let Some(read_request) = self.fdc.take_read_request() {
                    self.handle_floppy_read(read_request)?;
                }
                // Synchronize PIC's IRQ line with the FDC's output
                self.pic.set_irq_line(6, self.fdc.irq_output());
            }
            _ => (),
        };
        Ok(())
    }

    fn handle_floppy_read(&mut self, read_request: FdcReadData) -> Result<(), MachineError> {
        let [
            drive_head,
            cylinder,
            head,
            sector,
            size_code,
            end_sector,
            _gap,
            _data_length,
        ] = read_request.parameters;

        let drive = drive_head & 0x03;
        let physical_head = (drive_head >> 2) & 1;

        if drive != 0 || size_code != 2 || physical_head != head {
            return Err(MachineError::DiskError("Unsupported disk".to_string()));
        }

        let count_bytes = self.dma.count_bytes();

        // Require DMA to be unmasked, mode 0x46
        if self.dma.is_masked() || self.dma.mode() != 0x46 || count_bytes % 512 != 0 {
            return Err(MachineError::DiskError(
                "Unsupported DMA parameters".to_string(),
            ));
        }

        let dest_addr = self.dma.physical_address();
        let dma_offset = (dest_addr & 0xFFFF) as usize;

        if dma_offset + count_bytes > 0x10000 {
            return Err(MachineError::DiskError(
                "DMA transfer crosses 64 KiB boundary".to_string(),
            ));
        }

        let Some(floppy_image) = self.floppy_image.as_ref() else {
            return Err(MachineError::DiskError(
                "No floppy image inserted".to_string(),
            ));
        };

        let offset = floppy_image
            .geometry
            .byte_offset(cylinder as usize, head as usize, sector as usize)
            .ok_or_else(|| MachineError::DiskError("Invalid disk geometry".to_string()))?;

        let Some(bytes) = floppy_image.bytes.get(offset..offset + count_bytes) else {
            return Err(MachineError::DiskError("Bytes out of bounds".to_string()));
        };

        let sectors_read = count_bytes / 512;
        let last_sector = (usize::from(sector) + sectors_read - 1) as u8;
        if last_sector > end_sector || last_sector > floppy_image.geometry.sectors_per_track as u8 {
            return Err(MachineError::DiskError(
                "Disk read attempted beyond end of disk".to_string(),
            ));
        }

        let validated_buffer =
            validate_buffer(&mut self.memory, PhysAddr::from_u32(dest_addr), count_bytes)?;
        write_buffer(validated_buffer, bytes)?;

        self.dma.complete_transfer(count_bytes as u16);

        // Normal completion, drive A, head 0
        // No errors
        // No errors
        // C
        // H
        // R
        // N: 2 means 512-byte sectors
        let mt = read_request.command & 0x80 != 0;
        let cylinder_result = if end_sector == last_sector && (!mt || head == 1) {
            cylinder + 1
        } else {
            cylinder
        };
        let head_result = if end_sector == last_sector && mt && (head == 0) {
            1
        } else if end_sector == last_sector && mt && (head == 1) {
            0
        } else {
            head
        };
        let st0 = drive | (head_result << 2);
        let r_result = if last_sector < end_sector {
            last_sector + 1
        } else {
            1
        };
        let results = [st0, 00, 00, cylinder_result, head_result, r_result, 2];
        self.fdc.complete_read(results);

        Ok(())
    }

    fn load_com(&mut self, bytes: &[u8]) -> Result<(), MachineError> {
        /*
        Offset within segment
        0000
              PSP: 256 bytes
        00FF
        ...
        0100
              Contents of COM file
              ...unused space...
              ^ Stack grows this way
        FFFE Zero return address
        FFFF Zero return address
        */

        let segment = 0x1_000; // Segment where we're loading our program
        let segment_size = 0x10_000; // COM segment size (64 KiB)

        // Set up the program segment prefix (PSP) as DOS would
        // INT 20h at 0000h - 0001h
        write_byte(&mut self.memory, PhysAddr::new(segment, 0x0000), 0xCD)?;
        write_byte(&mut self.memory, PhysAddr::new(segment, 0x0001), 0x20)?;

        // Zero word at FFFEh - FFFh
        write_byte(&mut self.memory, PhysAddr::new(segment, 0xFFFE), 0x00)?;
        write_byte(&mut self.memory, PhysAddr::new(segment, 0xFFFF), 0x00)?;
        let reserved_bytes = 2;

        // Initialize registers
        self.cpu.sp = 0xFFFE; // Make room for the 2 bytes above
        self.cpu.cs = segment;
        self.cpu.ds = segment;
        self.cpu.es = segment;
        self.cpu.ss = segment;

        // Load binary into segment at offset 0x100, by convention
        let prog_offset = 0x100;
        if prog_offset + bytes.len() + reserved_bytes > segment_size {
            return Err(MachineError::NotEnoughMemory(
                prog_offset + bytes.len() + reserved_bytes,
                segment_size,
            )
            .into());
        }
        let validated_buffer = validate_buffer(
            &mut self.memory,
            SegmentedAddr::new(segment, prog_offset as u16).to_physical(),
            bytes.len(),
        )?;
        write_buffer(validated_buffer, bytes)?;
        self.cpu.ip = prog_offset as u16;
        Ok(())
    }

    fn load_boot_sector(
        &mut self,
        bytes: Vec<u8>,
        image_type: Option<DiskType>,
    ) -> Result<(), MachineError> {
        if bytes.len() < 512 {
            return Err(MachineError::BootSectorError(
                "Boot sector must be at least 512 bytes long".to_string(),
            ));
        }
        // Copy first 512 bytes of the boot sector to 0000:7C00h
        let validated_buffer = validate_buffer(&mut self.memory, PhysAddr::new(0, 0x7C00), 512)?;
        write_buffer(validated_buffer, &bytes[0..512])?;
        self.cpu.cs = 0;
        self.cpu.ip = 0x7C00;
        // Pretend we loaded the boot sector from the first floppy drive
        self.cpu.set_reg(Reg8::Dl, 0);

        // Keep the floppy image in machine
        let geometry = match image_type {
            Some(DiskType::Floppy360) => DiskGeometry {
                cylinders: 40,
                heads: 2,
                sectors_per_track: 9,
            },
            None | Some(DiskType::Floppy180) => DiskGeometry {
                cylinders: 40,
                heads: 1,
                sectors_per_track: 9,
            },
        };
        self.floppy_image = Some(FloppyDisk {
            bytes: bytes,
            geometry: geometry,
        });

        Ok(())
    }

    fn load_rom(&mut self, path: PathBuf) -> Result<(), MachineError> {
        let bytes: Vec<u8> = fs::read(path)?;
        if bytes.len() != 8192 {
            return Err(MachineError::BootSectorError(
                "BIOS ROM must be 8192 bytes long".to_string(),
            ));
        }
        // Copy 8192 bytes of the ROM to 0000h:FE000h
        let validated_buffer = validate_buffer(&mut self.memory, PhysAddr::new(0xFE00, 0), 8192)?;
        write_buffer(validated_buffer, &bytes)?;
        self.cpu.cs = 0xFFFF;
        self.cpu.ip = 0x0000;

        Ok(())
    }

    pub(crate) fn keyboard_input_mode(&self) -> KeyboardInputMode {
        self.keyboard_input_mode
    }
}

#[derive(Copy, Clone, PartialEq)]
pub(crate) enum LoadMode {
    Com,
    Boot,
    Rom,
}
