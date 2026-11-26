use crate::{
    cpu::{Cpu, Flag},
    machine::{Bios, FloppyDisk, bios::BiosError},
    memory::{SegmentedAddr, validate_buffer, write_buffer},
    registers::{Reg8, Reg16, SegmentReg},
};

use super::InterruptResult;

impl Bios {
    /// Emulates the BIOS disk service
    pub(in crate::machine) fn disk_service(
        &self,
        cpu: &mut Cpu,
        memory: &mut [u8],
        floppy_images: &[Option<FloppyDisk>; 2],
    ) -> Result<InterruptResult, BiosError> {
        let drive = cpu.get_reg(Reg8::Dl);
        if drive > 1 {
            cpu.set_reg(Reg8::Ah, 0x01);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        let operation = cpu.get_reg(Reg8::Ah);
        match operation {
            0x00 => self.operation_reset(drive, cpu),
            0x02 => self.operation_read(drive, cpu, memory, floppy_images),
            _ => {
                cpu.set_reg(Reg8::Ah, 0x01);
                cpu.set_flag(Flag::Carry, true);
                return Ok(InterruptResult::ResumeExecution);
            }
        }
    }

    fn operation_reset(&self, _drive: u8, cpu: &mut Cpu) -> Result<InterruptResult, BiosError> {
        cpu.set_reg(Reg8::Ah, 0);
        cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    fn operation_read(
        &self,
        drive: u8,
        cpu: &mut Cpu,
        memory: &mut [u8],
        floppy_images: &[Option<FloppyDisk>; 2],
    ) -> Result<InterruptResult, BiosError> {
        let sector_count = cpu.get_reg(Reg8::Al) as usize;
        let cylinder = cpu.get_reg(Reg8::Ch) as usize;
        let sector_num = cpu.get_reg(Reg8::Cl) as usize;
        let head = cpu.get_reg(Reg8::Dh) as usize;
        let dest = SegmentedAddr::new(cpu.get_segment_reg(SegmentReg::Es), cpu.get_reg(Reg16::Bx))
            .to_physical();

        let Some(disk) = &floppy_images[drive as usize] else {
            cpu.set_reg(Reg8::Ah, 0x80);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        let geometry = &disk.geometry;
        let Some(image_offset) = geometry
            .byte_offset(cylinder, head, sector_num)
            .and_then(|offset| usize::try_from(offset).ok())
        else {
            cpu.set_reg(Reg8::Ah, 0x04);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        let num_bytes = 512 * sector_count;

        if sector_count == 0 || sector_count > geometry.sectors_per_track - sector_num + 1 {
            cpu.set_reg(Reg8::Ah, 0x01);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        // Make sure the disk image is long enough
        if image_offset + num_bytes > disk.bytes.len() {
            cpu.set_reg(Reg8::Ah, 0x04);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        // Write sector to destination
        let validated_buffer = validate_buffer(memory, dest, num_bytes)?;
        write_buffer(
            validated_buffer,
            &disk.bytes[image_offset..image_offset + num_bytes],
        )?;

        cpu.set_reg(Reg8::Ah, 0);
        cpu.set_flag(Flag::Carry, false);

        Ok(InterruptResult::ResumeExecution)
    }
}
