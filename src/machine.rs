use core::fmt::Debug;
use std::collections::VecDeque;
use std::fs;
use std::time::Instant;
use thiserror::Error;

use crate::cpu::{Cpu, CpuError, CpuStepResult, Flag};
use crate::machine::console::PendingInput;
use crate::memory::{
    MemoryError, PhysAddr, SegmentedAddr, validate_buffer, write_buffer, write_byte, write_word,
};
use crate::registers::{Reg8, Reg16};
use disk::{DiskGeometry, FloppyDisk};
use dos::Resource;
use video::TextVideo;

pub use disk::DiskType;

mod console;
mod disk;
mod dos;
#[cfg(test)]
mod tests;
mod video;

// Owns everything
pub struct Machine {
    boot_time: Instant,
    /// Read line buffer (DOS mode only)
    console_read_buffer: VecDeque<u8>,
    cpu: Cpu,
    floppy_image: Option<FloppyDisk>,
    /// Resource handles (DOS mode only)
    handles: Vec<Option<Resource>>,
    keyboard_buffer: VecDeque<KeyPress>,
    keyboard_irq_pending: bool,
    load_mode: LoadMode,
    memory: Vec<u8>,
    /// What kind of input we're pending
    pending_input: Option<PendingInput>,
    text_video: TextVideo,
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

pub struct KeyPress {
    pub ascii: Option<u8>,
    pub scan_code: u8,
}

impl Machine {
    pub fn new(
        file_path: &str,
        load_mode: LoadMode,
        image_type: Option<DiskType>,
        verbosity: u8,
    ) -> anyhow::Result<Self> {
        let memory_size = 0x100_000; // Physical memory: 1 MiB

        let mut machine = Machine {
            boot_time: Instant::now(),
            console_read_buffer: VecDeque::new(),
            cpu: Cpu::new(verbosity),
            handles: vec![
                Some(Resource::Stdin),
                Some(Resource::Stdout),
                Some(Resource::Console),
            ],
            floppy_image: None,
            keyboard_buffer: VecDeque::new(),
            keyboard_irq_pending: false,
            load_mode: load_mode.clone(),
            memory: vec![0; memory_size],
            pending_input: None,
            text_video: TextVideo::new(),
            verbosity,
        };
        machine.handles.resize(20, None);
        machine.text_video.clear(&mut machine.memory)?;

        let data: Vec<u8> = fs::read(file_path)?;
        match load_mode {
            LoadMode::Com => machine.load_com(&data)?,
            LoadMode::Boot => machine.load_boot_sector(data, image_type)?,
        }

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

        Ok(machine)
    }

    pub fn render_video(&self, frame: &mut [u8]) {
        self.text_video.render(&self.memory, frame);
    }

    pub fn enqueue_input_character(&mut self, key: KeyPress) {
        self.keyboard_buffer.push_back(key);
        self.keyboard_irq_pending = true;
    }

    /// Emulates the DOS termination service
    fn termination_service(&mut self) -> Result<InterruptResult, MachineError> {
        Ok(InterruptResult::Exit((self.cpu.ax & 0xFF) as u8))
    }

    /// Emulates the DOS version service
    fn dos_version_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Report DOS version 2.0
        self.cpu.set_reg(Reg8::Al, 2);
        self.cpu.set_reg(Reg8::Ah, 0);
        self.cpu.bx = 0;
        self.cpu.cx = 0;
        Ok(InterruptResult::ResumeExecution)
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
                if let Some(first_key) = self.keyboard_buffer.pop_front() {
                    self.cpu.set_reg(Reg8::Al, first_key.ascii.unwrap_or(0x0));
                    self.cpu.set_reg(Reg8::Ah, first_key.scan_code);
                    Ok(InterruptResult::ResumeExecution)
                } else {
                    self.pending_input = Some(PendingInput::KeyboardService);
                    Ok(InterruptResult::WaitingForInput)
                }
            }
            0x01 => {
                // Check key
                if let Some(first_key) = self.keyboard_buffer.front() {
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

    /// Emulates the Duplicate File Handle Service
    fn duplicate_file_handle_service(&mut self) -> Result<InterruptResult, MachineError> {
        let handle_num = self.cpu.get_reg(Reg16::Bx);
        let Some(Some(resource)) = self.handles.get(handle_num as usize) else {
            // Unknown handle number
            self.cpu.ax = 6;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        let Some(pos) = self.handles.iter().position(|h| h.is_none()) else {
            // No open positions in table
            self.cpu.ax = 4;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        self.handles[pos] = Some(resource.clone());
        self.cpu.ax = pos as u16;
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the Close Handle Service
    fn close_handle_service(&mut self) -> Result<InterruptResult, MachineError> {
        let handle_num = self.cpu.get_reg(Reg16::Bx);
        if !matches!(self.handles.get(handle_num as usize), Some(Some(_))) {
            // Handle is already closed or nonexistent
            self.cpu.ax = 6;
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        self.handles[handle_num as usize] = None;
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    fn handle_guest_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
        self.cpu.enter_interrupt(&mut self.memory, interrupt)?;
        Ok(InterruptResult::ResumeExecution)
    }

    fn handle_interrupt(&mut self, interrupt: u8) -> Result<InterruptResult, MachineError> {
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
                    0x01 => self.read_char_with_echo_service(),
                    0x02 => self.output_char_service(),
                    0x09 => self.print_string_service(),
                    0x0C => self.flush_and_read_service(),
                    0x30 => self.dos_version_service(),
                    0x3E => self.close_handle_service(),
                    0x3F => self.read_service(),
                    0x40 => self.write_service(),
                    0x45 => self.duplicate_file_handle_service(),
                    0x4C => self.termination_service(),
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

    pub fn step(&mut self) -> Result<MachineStepResult, MachineError> {
        if self.pending_input.is_some() {
            return self.handle_pending_input();
        }

        // Deliver keyboard IRQ
        if self.keyboard_irq_pending && self.cpu.get_flag(Flag::InterruptEnable) {
            self.keyboard_irq_pending = false;
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
                if port == 0x3DA {
                    // Video Service
                    self.cpu.set_reg(Reg8::Al, self.text_video.read_status());
                } else {
                    self.cpu.set_reg(Reg8::Al, 0xFF);
                }
                Ok(MachineStepResult::Continue)
            }
            CpuStepResult::PortRead16 { .. } => {
                self.cpu.ax = 0xFFFF;
                Ok(MachineStepResult::Continue)
            }
            CpuStepResult::PortWrite8 { .. } => Ok(MachineStepResult::Continue),
            CpuStepResult::PortWrite16 { .. } => Ok(MachineStepResult::Continue),
        };
        self.cpu.trace();
        outcome
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
}

#[derive(Clone, PartialEq)]
pub(crate) enum LoadMode {
    Com,
    Boot,
}
