use core::fmt::Debug;
use std::collections::VecDeque;
use std::fs;
use std::io::{self, Write};
use std::time::Instant;
use thiserror::Error;

use crate::cpu::{Cpu, CpuError, CpuStepResult, Flag};
use crate::memory::{
    MemoryError, PhysAddr, read_buffer, validate_buffer, write_buffer, write_byte,
};
use crate::registers::{Reg8, Reg16};
use console::{TerminalGuard, translate_key};
use disk::{DiskGeometry, FloppyDisk};
use dos::{PendingInput, Resource};
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
    console_read_buffer: VecDeque<u8>,
    cpu: Cpu,
    floppy_image: Option<FloppyDisk>,
    handles: Vec<Option<Resource>>,
    input_queue: VecDeque<KeyPress>,
    load_mode: LoadMode,
    memory: Vec<u8>,
    pending_key: Option<(u8, u8)>,
    pending_input: Option<PendingInput>,
    terminal: Option<TerminalGuard>,
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
    #[error("Terminal error: {0}")]
    TerminalError(String),
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
            input_queue: VecDeque::new(),
            load_mode: load_mode.clone(),
            memory: vec![0; memory_size],
            pending_input: None,
            pending_key: None,
            terminal: Some(TerminalGuard::new()?),
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

        Ok(machine)
    }

    pub fn render_video(&self, frame: &mut [u8]) {
        self.text_video.render(&self.memory, frame);
    }

    pub fn enqueue_input_character(&mut self, key: KeyPress) {
        self.input_queue.push_back(key);
    }

    /// Emulates the DOS write service
    fn write_service(&mut self) -> Result<InterruptResult, MachineError> {
        let num_bytes = self.cpu.cx as usize;
        let start_pos = PhysAddr::new(self.cpu.ds, self.cpu.dx);
        let dest = self.cpu.bx;
        if dest != 1 {
            return Err(MachineError::WriteServiceError(
                "Unsupported destination in bx".to_string(),
            ));
        }

        let buffer = read_buffer(&self.memory, start_pos, num_bytes)?;

        for c in &buffer {
            self.text_video.write_char(&mut self.memory, *c)?;
        }

        io::stdout().write_all(b"\x1b[92m")?;
        io::stdout().write_all(&buffer)?;
        io::stdout().write_all(b"\x1b[0m")?;
        io::stdout().flush()?;
        // Successfully written bytes
        self.cpu.ax = num_bytes as u16;
        // Clear the carry flag
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    /// Emulates the DOS flush and read service
    fn flush_and_read_service(&mut self) -> Result<InterruptResult, MachineError> {
        // Flush input if handle 0 is a console
        let Some(Some(resource)) = self.handles.get(0) else {
            return Err(MachineError::ReadServiceError(
                "Handle 0 is not set".to_string(),
            ));
        };
        match *resource {
            Resource::Console => self.flush_console_input()?,
            // TODO: handle STDIN if console-backed
            _ => {}
        };

        let al = self.cpu.get_reg(Reg8::Al);
        return match al {
            0x01 => self.read_char_with_echo_service(),
            _ => Err(MachineError::ReadServiceError(
                "Unsupported mode in 0Ch service".to_string(),
            )),
        };
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

    /// Emulates the Video Service
    fn video_service(&mut self) -> Result<InterruptResult, MachineError> {
        let op = self.cpu.get_reg(Reg8::Ah);
        match op {
            // Set text cursor shape
            0x1 => {
                // TODO: Noop for now
                Ok(InterruptResult::ResumeExecution)
            }
            // BIOS character output
            0xe => {
                let c = self.cpu.get_reg(Reg8::Al);
                io::stdout().write_all(b"\x1b[92m")?;
                io::stdout().write_all(&[c])?;
                io::stdout().write_all(b"\x1b[0m")?;
                io::stdout().flush()?;
                Ok(InterruptResult::ResumeExecution)
            }
            // Get current video mode
            0xf => {
                self.cpu.set_reg(Reg8::Al, 0x3); // 80x25 color text mode
                self.cpu.set_reg(Reg8::Ah, 80); // 80 columns in decimal
                self.cpu.set_reg(Reg8::Bh, 0); // active display page
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
            _ => Err(MachineError::TimeOfDayServiceError(
                "Unsupported operation".to_string(),
            )),
        }
    }

    fn keyboard_service(&mut self) -> Result<InterruptResult, MachineError> {
        let operation = self.cpu.get_reg(Reg8::Ah);
        if self.verbosity > 0 {
            eprintln!("\x1b[1;93mKeyboard Service op {:X}h\x1b[0m", operation);
        }
        match operation {
            0x00 => {
                // Read key
                let key: Option<(u8, u8)> = self.pending_key.take();
                let (ascii_key, scan_code) = match key {
                    Some(key) => key,
                    None => loop {
                        let mut buf = [0u8; 1];
                        self.read_console_byte(&mut buf)?;
                        if let Some(key) = translate_key(buf[0]) {
                            break key;
                        }
                    },
                };
                self.cpu.set_reg(Reg8::Al, ascii_key);
                self.cpu.set_reg(Reg8::Ah, scan_code);
                Ok(InterruptResult::ResumeExecution)
            }
            0x01 => {
                // Check key
                let mut key: Option<(u8, u8)> = self.pending_key;
                if key.is_none() {
                    // Try reading host input without waiting
                    if let Some(byte) = self.try_read_console_byte()? {
                        // Key arrived; translate and cache it
                        key = translate_key(byte); // Might still return None
                        self.pending_key = key;
                    }
                }
                // If we have a key
                if let Some((ascii_key, scan_code)) = key {
                    self.cpu.set_reg(Reg8::Al, ascii_key);
                    self.cpu.set_reg(Reg8::Ah, scan_code);
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

    fn bios_system_service(&mut self) -> Result<InterruptResult, MachineError> {
        let op = self.cpu.get_reg(Reg8::Ah);
        match op {
            0x88 => {
                // Get extended memory size (RAM above 1 MiB)
                // TODO: We don't support it. IBM PC/PCjr return 80h; PC/XT returns 86h
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
            0x15 => self.bios_system_service(),
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
            CpuStepResult::PortRead8 { .. } => {
                self.cpu.set_reg(Reg8::Al, 0xFF);
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
            PhysAddr::new(segment, prog_offset as u16),
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
