use crate::{
    cpu::Flag,
    memory::{PhysAddr, validate_buffer, write_buffer},
    registers::{Reg8, Reg16, SegmentReg},
};

use super::{InterruptResult, Machine, MachineError};

#[cfg(test)]
mod tests;

#[derive(Clone, clap::ValueEnum)]
pub enum DiskType {
    #[value(name = "180k")]
    Floppy180,
    #[value(name = "360k")]
    Floppy360,
}

pub(super) struct DiskGeometry {
    pub(super) cylinders: usize,
    pub(super) heads: usize,
    pub(super) sectors_per_track: usize,
}

impl DiskGeometry {
    fn byte_offset(&self, cylinder: usize, head: usize, sector: usize) -> Option<usize> {
        if cylinder >= self.cylinders
            || head >= self.heads
            || sector == 0
            || sector > self.sectors_per_track
        {
            return None;
        }
        let index = (cylinder * self.heads + head) * self.sectors_per_track + sector - 1;
        Some(index * 512)
    }
}

pub(super) struct FloppyDisk {
    pub(super) bytes: Vec<u8>,
    pub(super) geometry: DiskGeometry,
}

impl Machine {
    /// Emulates the BIOS disk service
    pub(super) fn disk_service(&mut self) -> Result<InterruptResult, MachineError> {
        // TODO: Support more drives
        let drive = self.cpu.get_reg(Reg8::Dl);
        if drive != 0 {
            self.cpu.set_reg(Reg8::Ah, 0x01);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        let operation = self.cpu.get_reg(Reg8::Ah);
        match operation {
            0x00 => self.operation_reset(),
            0x02 => self.operation_read(),
            _ => {
                self.cpu.set_reg(Reg8::Ah, 0x01);
                self.cpu.set_flag(Flag::Carry, true);
                return Ok(InterruptResult::ResumeExecution);
            }
        }
    }

    fn operation_reset(&mut self) -> Result<InterruptResult, MachineError> {
        self.cpu.set_reg(Reg8::Ah, 0);
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    fn operation_read(&mut self) -> Result<InterruptResult, MachineError> {
        let sector_count = self.cpu.get_reg(Reg8::Al) as usize;
        let cylinder = self.cpu.get_reg(Reg8::Ch) as usize;
        let sector_num = self.cpu.get_reg(Reg8::Cl) as usize;
        let head = self.cpu.get_reg(Reg8::Dh) as usize;
        let dest = PhysAddr::new(
            self.cpu.get_segment_reg(SegmentReg::Es),
            self.cpu.get_reg(Reg16::Bx),
        );

        let Some(disk) = &self.floppy_image else {
            self.cpu.set_reg(Reg8::Ah, 0x80);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        let geometry = &disk.geometry;
        let Some(image_offset) = geometry.byte_offset(cylinder, head, sector_num) else {
            self.cpu.set_reg(Reg8::Ah, 0x04);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        let num_bytes = 512 * sector_count;

        if sector_count == 0 || sector_count > geometry.sectors_per_track - sector_num + 1 {
            self.cpu.set_reg(Reg8::Ah, 0x01);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        // Make sure the disk image is long enough
        if image_offset + num_bytes > disk.bytes.len() {
            self.cpu.set_reg(Reg8::Ah, 0x04);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        // Write sector to destination
        let validated_buffer = validate_buffer(&mut self.memory, dest, num_bytes)?;
        write_buffer(
            validated_buffer,
            &disk.bytes[image_offset..image_offset + num_bytes],
        )?;

        self.cpu.set_reg(Reg8::Ah, 0);
        self.cpu.set_flag(Flag::Carry, false);

        Ok(InterruptResult::ResumeExecution)
    }
}
