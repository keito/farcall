use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};

use crate::machine::MachineError;

use super::DiskGeometry;

enum TransferDirection {
    Read,
    Write,
}

struct Transfer {
    buffer: [u8; 512],
    cursor: usize,
    direction: TransferDirection,
    next_offset: u64,
    sectors_remaining: u16,
}

pub(super) struct XtCf {
    file: Option<File>,
    geometry: Option<DiskGeometry>,
    identify: [u8; 512],
    drive_head: u8,
    sector_count: u8,
    sector_number: u8,
    cylinder_low: u8,
    cylinder_high: u8,
    error: u8,
    transfer: Option<Transfer>,
}

impl XtCf {
    /// Creates a new XtCf adapter, optionally with an image attached.
    /// Geometry is autodetected if none is provided, and ignored if no image is attached.
    pub(super) fn new(
        image: Option<File>,
        geometry: Option<DiskGeometry>,
    ) -> Result<Self, MachineError> {
        let geometry = match (image.as_ref(), geometry) {
            (Some(_), Some(geometry)) => Some(geometry),
            (Some(file), None) => Some(Self::detect_geometry(file)?),
            (None, _) => None,
        };

        let mut identify = [0u8; 512];
        if let Some(geometry) = geometry.as_ref() {
            // Validate geometry
            let expected_len = geometry.cylinders as u64
                * geometry.heads as u64
                * geometry.sectors_per_track as u64
                * 512;
            if expected_len != image.as_ref().unwrap().metadata()?.len()
                || !(1..=16383).contains(&geometry.cylinders)
            {
                return Err(MachineError::DiskError(
                    "Invalid disk geometry for the hard disk image.".to_string(),
                ));
            }

            identify[2..4].copy_from_slice(&(geometry.cylinders as u16).to_le_bytes()); // Word 1: cylinders
            identify[6..8].copy_from_slice(&(geometry.heads as u16).to_le_bytes()); // Word 3: heads
            identify[12..14].copy_from_slice(&(geometry.sectors_per_track as u16).to_le_bytes()); // Word 6: sectors per track
        }

        Ok(XtCf {
            file: image,
            geometry,
            identify,
            drive_head: 0,
            transfer: None,
            cylinder_low: 0,
            cylinder_high: 0,
            sector_count: 0,
            sector_number: 0,
            error: 0,
        })
    }

    /// Autodetects geometry given the file
    fn detect_geometry(image: &File) -> Result<DiskGeometry, MachineError> {
        let image_len = image.metadata()?.len();
        let bytes_per_cylinder = 16 * 63 * 512;
        let cylinders = image_len / bytes_per_cylinder;
        Ok(DiskGeometry {
            cylinders: cylinders as usize,
            heads: 16,
            sectors_per_track: 63,
        })
    }

    fn fill_read_buffer(&mut self) {
        let Some(file) = self.file.as_mut() else {
            self.error = 0x10; // IDNF
            return;
        };
        let Some(transfer) = self.transfer.as_mut() else {
            return;
        };
        let result = (|| -> Result<(), std::io::Error> {
            file.seek(SeekFrom::Start(transfer.next_offset))?;
            file.read_exact(&mut transfer.buffer)?;
            Ok(())
        })();
        if result.is_err() {
            self.error = 0x10; // IDNF
            self.transfer = None;
            return;
        }
        transfer.next_offset += 512;
        transfer.cursor = 0;
    }

    /// Calculate offset to read or write from
    fn calculate_offset(&mut self) -> Option<u64> {
        let cylinder = u16::from_le_bytes([self.cylinder_low, self.cylinder_high]);
        let head = self.drive_head & 0x0F;
        let sector = self.sector_number;
        let Some(offset) = self.geometry.as_ref().and_then(|geometry| {
            geometry.byte_offset(cylinder as usize, head as usize, sector as usize)
        }) else {
            self.error = 0x10; // IDNF: Invalid CHS address
            return None;
        };
        Some(offset)
    }

    pub(super) fn write(&mut self, port: u16, value: u8) {
        match port {
            0x300 => {
                // Write data
                if self.drive_head & 0x10 != 0 {
                    return;
                }
                let Some(transfer) = self.transfer.as_mut() else {
                    return;
                };
                if !matches!(transfer.direction, TransferDirection::Write) {
                    return;
                }

                let Some(file) = self.file.as_mut() else {
                    return;
                };

                let cursor = transfer.cursor;
                transfer.buffer[cursor] = value;

                if cursor == transfer.buffer.len() - 1 {
                    transfer.cursor = 0;
                    let result = (|| -> Result<(), std::io::Error> {
                        // TODO: When we support any sized images, get and store the size once
                        let capacity = file.metadata()?.len();
                        let end = transfer.next_offset + transfer.buffer.len() as u64;
                        if end > capacity {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "Transfer exceeds file capacity",
                            ));
                        }
                        file.seek(SeekFrom::Start(transfer.next_offset))?;
                        file.write_all(&transfer.buffer)?;
                        Ok(())
                    })();
                    if result.is_err() {
                        self.error = 0x10; // IDNF
                        self.transfer = None;
                        return;
                    }

                    transfer.sectors_remaining = transfer.sectors_remaining.saturating_sub(1);
                    if transfer.sectors_remaining == 0 {
                        self.transfer = None;
                    } else {
                        transfer.next_offset += 512;
                        transfer.cursor = 0;
                    }
                } else {
                    transfer.cursor = cursor.wrapping_add(1);
                }
            }
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
                self.transfer = None;

                match value {
                    0xC4 => {
                        // Execute READ MULTIPLE

                        // Calculate offset to read from
                        let Some(offset) = self.calculate_offset() else {
                            return;
                        };
                        self.transfer = Some(Transfer {
                            buffer: [0u8; 512],
                            cursor: 0,
                            direction: TransferDirection::Read,
                            next_offset: offset,
                            sectors_remaining: match self.sector_count {
                                0 => 256, // 0 means 256
                                count => u16::from(count),
                            },
                        });
                        self.fill_read_buffer();
                    }
                    0xC5 => {
                        // Execute WRITE MULTIPLE

                        // Calculate offset to write to
                        let Some(offset) = self.calculate_offset() else {
                            return;
                        };
                        self.transfer = Some(Transfer {
                            buffer: [0u8; 512],
                            cursor: 0,
                            direction: TransferDirection::Write,
                            next_offset: offset,
                            sectors_remaining: match self.sector_count {
                                0 => 256, // 0 means 256
                                count => u16::from(count),
                            },
                        });
                    }
                    0xEC => {
                        // Execute IDENTIFY DEVICE
                        self.transfer = Some(Transfer {
                            buffer: self.identify,
                            cursor: 0,
                            direction: TransferDirection::Read,
                            next_offset: 0,
                            sectors_remaining: 1,
                        });
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
                let Some(transfer) = self.transfer.as_mut() else {
                    return 0;
                };
                if !matches!(transfer.direction, TransferDirection::Read) {
                    return 0;
                }

                let cursor = transfer.cursor;
                let byte = transfer.buffer[cursor as usize];

                // Reset cursor to None if done, or increment
                if cursor == transfer.buffer.len() - 1 {
                    transfer.sectors_remaining = transfer.sectors_remaining.saturating_sub(1);
                    if transfer.sectors_remaining == 0 {
                        self.transfer = None;
                    } else {
                        // Fill transfer buffer with next sector
                        self.fill_read_buffer();
                    }
                } else {
                    transfer.cursor = cursor.wrapping_add(1);
                }

                byte
            }
            0x302 => self.error,
            // Status
            0x30E => {
                if self.drive_head & 0x10 == 0 && self.file.is_some() {
                    match self.transfer {
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
