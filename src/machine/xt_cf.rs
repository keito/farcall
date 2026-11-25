use std::{
    fs::File,
    io::{Read, Seek},
};

use crate::machine::DiskGeometry;

pub(super) struct XtCf {
    cursor: Option<usize>,
    sectors_remaining: u16,
    file: Option<File>,
    geometry: DiskGeometry,
    identify: [u8; 512],
    transfer_buffer: [u8; 512],
    drive_head: u8,
    next_read_offset: u64,
    sector_count: u8,
    sector_number: u8,
    cylinder_low: u8,
    cylinder_high: u8,
    error: u8,
}

impl XtCf {
    pub(super) fn new(image: Option<File>) -> Self {
        let mut identify = [0u8; 512];
        identify[2..4].copy_from_slice(&32u16.to_le_bytes()); // Word 1: cylinders
        identify[6..8].copy_from_slice(&16u16.to_le_bytes()); // Word 3: heads
        identify[12..14].copy_from_slice(&63u16.to_le_bytes()); // Word 6: sectors per track

        XtCf {
            cursor: None,
            sectors_remaining: 0,
            file: image,
            geometry: DiskGeometry {
                cylinders: 32,
                heads: 16,
                sectors_per_track: 63,
            },
            identify,
            transfer_buffer: [0u8; 512],
            drive_head: 0,
            next_read_offset: 0,
            sector_count: 0,
            sector_number: 0,
            cylinder_low: 0,
            cylinder_high: 0,
            error: 0,
        }
    }

    fn fill_read_buffer(&mut self) {
        let Some(file) = self.file.as_mut() else {
            self.error = 0x10; // IDNF
            return;
        };
        let result = (|| -> Result<(), std::io::Error> {
            file.seek(std::io::SeekFrom::Start(self.next_read_offset as u64))?;
            file.read_exact(&mut self.transfer_buffer)?;
            Ok(())
        })();
        if result.is_err() {
            self.error = 0x10; // IDNF
            self.cursor = None;
            self.sectors_remaining = 0;
            return;
        }
        self.next_read_offset += 512;
        self.cursor = Some(0);
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x302 => {
                // Set the features register
                // TODO: Ignore 0x01 (use 8-bit transfers)
            }
            0x304 => self.sector_count = value,
            0x306 => self.sector_number = value,
            0x308 => self.cylinder_low = value,
            0x30A => self.cylinder_high = value,
            0x30C => self.drive_head = value,
            0x30E => {
                // Command
                if self.drive_head & 0x10 != 0 || self.file.is_none() {
                    return;
                }

                // Reset previous command state
                self.error = 0;
                self.cursor = None;
                self.sectors_remaining = 0;

                match value {
                    0xC4 => {
                        // Execute READ MULTIPLE

                        // Calculate offset to read from
                        let cylinder = u16::from_le_bytes([self.cylinder_low, self.cylinder_high]);
                        let head = self.drive_head & 0x0F;
                        let sector = self.sector_number;
                        let Some(offset) = self.geometry.byte_offset(
                            cylinder as usize,
                            head as usize,
                            sector as usize,
                        ) else {
                            self.error = 0x10; // IDNF: Invalid CHS address
                            return;
                        };
                        self.next_read_offset = offset as u64;
                        self.sectors_remaining = match self.sector_count {
                            0 => 256, // 0 means 256
                            count => u16::from(count),
                        };
                        self.fill_read_buffer();
                    }
                    0xEC => {
                        // Execute IDENTIFY DEVICE
                        self.transfer_buffer.copy_from_slice(&self.identify);
                        self.cursor = Some(0);
                        self.sectors_remaining = 1;
                    }
                    0xEF => {
                        // Execute SET FEATURES
                        // TODO: Ignore
                    }
                    _ => (),
                }
            }
            _ => (),
        }
    }

    pub(super) fn read(&mut self, port: u16) -> u8 {
        match port {
            0x300 => {
                // Read data
                if self.drive_head & 0x10 != 0 || self.file.is_none() {
                    return 0;
                }
                let Some(cursor) = self.cursor else {
                    return 0;
                };

                let byte = self.transfer_buffer[cursor as usize];

                // Reset cursor to None if done, or increment
                if cursor == self.transfer_buffer.len() - 1 {
                    self.sectors_remaining = self.sectors_remaining.saturating_sub(1);
                    if self.sectors_remaining == 0 {
                        self.cursor = None;
                    } else {
                        // Fill transfer buffer with next sector
                        self.fill_read_buffer();
                    }
                } else {
                    self.cursor = Some(cursor.wrapping_add(1));
                }

                byte
            }
            0x302 => self.error,
            // Status
            0x30E => {
                if self.drive_head & 0x10 == 0 && self.file.is_some() {
                    match self.cursor {
                        Some(_) => 0x48, // DRDY=0x40 | DRQ=0x08
                        None => 0x40 | (if self.error != 0 { 0x01 } else { 0 }),
                    }
                } else {
                    0x00
                }
            }
            _ => 0,
        }
    }
}
