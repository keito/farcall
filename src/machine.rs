use core::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;

use crate::cpu::{Cpu, CpuError, CpuStepResult, Flag};
use crate::memory::{
    MemoryError, PhysAddr, SegmentedAddr, validate_buffer, write_buffer, write_byte, write_word,
};
use crate::registers::Reg8;
use bios::{Bios, BiosError};
use bios_keyboard::{BiosKeyStroke, BiosKeyboard};
use disk::{DiskGeometry, FloppyDisk};
use dma::Dma;
use dos::{Dos, DosError};
use fdc::{Fdc, FdcReadData};
use pc_keyboard::PcKeyboard;
use pic::Pic;
use pit::Pit;
use ppi::Ppi;
use video::TextVideo;

pub use disk::{DiskImageConfig, DiskType};

mod bios;
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
    bios: Option<Bios>,
    bios_keyboard: BiosKeyboard,
    cpu: Cpu,
    dma: Dma,
    dos: Option<Dos>,
    fdc: Fdc,
    floppy_images: [Option<FloppyDisk>; 2],
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
    #[error(transparent)]
    Bios(#[from] BiosError),
    #[error("Boot Sector error: {0}")]
    BootSectorError(String),
    #[error("CPU Error: {0}")]
    CpuError(#[from] CpuError),
    #[error("Disk error: {0}")]
    DiskError(String),
    #[error(transparent)]
    Dos(#[from] DosError),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Not enough memory; offset {0} exceeds {1} bytes")]
    NotEnoughMemory(usize, usize),
    #[error("Memory Error: {0}")]
    MemoryError(#[from] MemoryError),
    #[error("PIC error: {0}")]
    PicError(String),
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
    pub fn new(boot_mode: BootMode, verbosity: u8) -> anyhow::Result<Self> {
        let memory_size = 0x100_000; // Physical memory: 1 MiB

        let load_mode = if boot_mode.is_rom() {
            LoadMode::Rom
        } else if boot_mode.is_com() {
            LoadMode::Com
        } else {
            LoadMode::Boot
        };

        let mut machine = Machine {
            bios: (!boot_mode.is_rom()).then(|| Bios::new()),
            bios_keyboard: BiosKeyboard::new(),
            last_step_time: Instant::now(),
            cpu: Cpu::new(verbosity),
            dma: Dma::new(),
            dos: boot_mode.is_com().then(|| Dos::new()),
            fdc: Fdc::new(),
            floppy_images: [None, None],
            keyboard_input_mode: if boot_mode.is_rom() {
                KeyboardInputMode::PcHardware
            } else {
                KeyboardInputMode::BiosKeystrokes
            },
            load_mode,
            memory: vec![0; memory_size],
            pc_keyboard: PcKeyboard::new(),
            pic: Pic::new(),
            pit: Pit::new(),
            ppi: Ppi::new(),
            synthetic_int09_pending: false,
            text_video: TextVideo::new(),
            tick_remainder: 0,
            verbosity,
        };
        machine.text_video.clear(&mut machine.memory)?;

        match boot_mode {
            BootMode::Com { program } => {
                let data = fs::read(program)?;
                machine.load_com(&data)?;
            }
            BootMode::Floppy {
                drive_a,
                drive_b,
                rom,
            } => {
                let data = fs::read(drive_a.path)?;
                machine.load_boot_sector(data, drive_a.disk_type)?;

                if let Some(rom_path) = rom {
                    machine.load_rom(&rom_path)?;
                }

                if let Some(drive_b) = drive_b {
                    let data = fs::read(drive_b.path)?;
                    machine.load_floppy_image(1, data, drive_b.disk_type);
                }
            }
        }

        if machine.keyboard_input_mode == KeyboardInputMode::BiosKeystrokes {
            // Create a stub IRET entry in the Interrupt Vector Table for INT 09h
            // so that applications have a default handler to hook into.
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
            0x10..=0x17 | 0x1A => {
                // BIOS services - in COM/Boot mode, send to our emulated BIOS
                if let Some(bios) = self.bios.as_mut() {
                    Ok(bios.handle_bios_service(
                        interrupt,
                        &mut self.cpu,
                        &mut self.memory,
                        &mut self.bios_keyboard,
                        &self.floppy_images,
                        &mut self.text_video,
                        self.verbosity,
                    )?)
                } else {
                    unreachable!("This should be ROM mode, which is handled above");
                }
            }
            0x20 | 0x21 | 0x28 | 0x29 => {
                // DOS services - in COM mode, send to our emulated DOS
                if let Some(dos) = self.dos.as_mut() {
                    Ok(dos.handle_interrupt(
                        interrupt,
                        &mut self.cpu,
                        &mut self.memory,
                        &mut self.bios_keyboard,
                        &mut self.text_video,
                    )?)
                } else {
                    self.handle_guest_interrupt(interrupt)
                }
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
        if let Some(bios) = self.bios.as_mut()
            && bios.pending_keyboard_read()
        {
            return Ok(bios.handle_pending_keyboard_read(&mut self.cpu, &mut self.bios_keyboard)?);
        }

        if let Some(dos) = self.dos.as_mut()
            && dos.has_pending_input()
        {
            return Ok(dos.handle_pending_input(
                &mut self.cpu,
                &mut self.memory,
                &mut self.bios_keyboard,
                &mut self.text_video,
            )?);
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
            size_code,  // sector size code (bytes = 128 << size_code except 0)
            end_sector, // EOT: final sector ID on a track for this command
            _gap,
            _data_length,
        ] = read_request.parameters;

        let drive = drive_head & 0x03; // Extract Unit Select bits
        let physical_head = (drive_head >> 2) & 1; // Extract HD bit

        // Ensure drive A or B and sectors are 512 bytes, and physical head agrees with head ID.
        if (drive != 0 && drive != 1) || size_code != 2 || physical_head != head {
            return Err(MachineError::DiskError("Unsupported disk".to_string()));
        }

        // See how many bytes DMA is configured to receive
        let count_bytes = self.dma.count_bytes();

        // Require DMA to be unmasked, mode 0x46, and byte count divisible by 512
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

        // Handle disk image not provided with a MA (Missing Address Mark) status
        // TODO: more accurately simulate empty disk
        let Some(floppy_image) = self.floppy_images[drive as usize].as_ref() else {
            let st0 = 0x40 | (physical_head << 2) | drive;
            let results = [st0, 0x01, 0x00, cylinder, head, sector, size_code];
            self.fdc.complete_read(results);
            return Ok(());
        };

        let multi_track = read_request.command & 0x80 != 0;
        let mut current_head = head;
        let mut current_sector = sector;
        let mut buffer: Vec<u8> = Vec::with_capacity(count_bytes);

        // For each sector...
        for _ in 0..count_bytes / 512 {
            let offset = floppy_image
                .geometry
                .byte_offset(
                    cylinder as usize,
                    current_head as usize,
                    current_sector as usize,
                )
                .ok_or_else(|| MachineError::DiskError("Invalid disk geometry".to_string()))?;

            let Some(bytes) = floppy_image.bytes.get(offset..offset + 512) else {
                return Err(MachineError::DiskError("Bytes out of bounds".to_string()));
            };

            buffer.extend_from_slice(bytes);

            if buffer.len() == count_bytes {
                break;
            }

            if current_sector != end_sector {
                // Not yet at end of track
                current_sector = current_sector.wrapping_add(1);
            } else if current_sector == end_sector && multi_track && current_head == 0 {
                // At end of track, multi-track enabled, and we're on the first head
                current_head = 1;
                current_sector = 1;
            } else if current_sector == end_sector {
                return Err(MachineError::DiskError(
                    "Cannot read past end of track without multi-track".to_string(),
                ));
            }
        }

        // Write buffer to DMA-requested memory address
        let validated_buffer =
            validate_buffer(&mut self.memory, PhysAddr::from_u32(dest_addr), count_bytes)?;
        write_buffer(validated_buffer, &buffer)?;

        self.dma.complete_transfer(count_bytes as u16);

        // Construct position after transfer
        let reached_end_of_track = current_sector == end_sector;
        let (result_cylinder, result_head, result_sector) =
            match (reached_end_of_track, multi_track, current_head) {
                // Continue within current track
                (false, _, _) => (cylinder, current_head, current_sector + 1),
                // Continue onto head 1 of this cylinder
                (true, true, 0) => (cylinder, 1, 1),
                // Finished the last head
                (true, true, _) => (cylinder + 1, 0, 1),
                // Single-track mode: go to next cylinder on current head
                (true, false, _) => (cylinder + 1, current_head, 1),
            };

        let results = [
            drive | (current_head << 2), // ST0: Normal completion,
            00,                          // ST1: no errors
            00,                          // ST2: no errors
            result_cylinder,
            result_head,
            result_sector,
            size_code,
        ];
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
        image_type: DiskType,
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
        self.load_floppy_image(0, bytes, image_type);

        Ok(())
    }

    /// Loads the bytes/geometry of the given floppy image into the given slot
    fn load_floppy_image(&mut self, drive: u8, bytes: Vec<u8>, image_type: DiskType) {
        let geometry = match image_type {
            DiskType::Floppy180 => DiskGeometry {
                cylinders: 40,
                heads: 1,
                sectors_per_track: 9,
            },
            DiskType::Floppy360 => DiskGeometry {
                cylinders: 40,
                heads: 2,
                sectors_per_track: 9,
            },
            DiskType::Floppy720 => DiskGeometry {
                cylinders: 80,
                heads: 2,
                sectors_per_track: 9,
            },
        };
        self.floppy_images[drive as usize] = Some(FloppyDisk {
            bytes: bytes,
            geometry: geometry,
        });
    }

    fn load_rom(&mut self, path: &Path) -> Result<(), MachineError> {
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
enum LoadMode {
    Com,
    Boot,
    Rom,
}

pub(crate) enum BootMode {
    Com {
        program: PathBuf,
    },
    Floppy {
        drive_a: DiskImageConfig,
        drive_b: Option<DiskImageConfig>,
        rom: Option<PathBuf>,
    },
}

impl BootMode {
    pub fn is_com(&self) -> bool {
        matches!(self, BootMode::Com { .. })
    }

    pub fn is_rom(&self) -> bool {
        match self {
            BootMode::Com { .. } => false,
            BootMode::Floppy {
                drive_a: _,
                drive_b: _,
                rom,
            } => rom.is_some(),
        }
    }
}
