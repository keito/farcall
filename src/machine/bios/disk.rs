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
        floppy_image: &Option<FloppyDisk>,
    ) -> Result<InterruptResult, BiosError> {
        // TODO: Support more drives
        let drive = cpu.get_reg(Reg8::Dl);
        if drive != 0 {
            cpu.set_reg(Reg8::Ah, 0x01);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        let operation = cpu.get_reg(Reg8::Ah);
        match operation {
            0x00 => self.operation_reset(cpu),
            0x02 => self.operation_read(cpu, memory, floppy_image),
            _ => {
                cpu.set_reg(Reg8::Ah, 0x01);
                cpu.set_flag(Flag::Carry, true);
                return Ok(InterruptResult::ResumeExecution);
            }
        }
    }

    fn operation_reset(&self, cpu: &mut Cpu) -> Result<InterruptResult, BiosError> {
        cpu.set_reg(Reg8::Ah, 0);
        cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    fn operation_read(
        &self,
        cpu: &mut Cpu,
        memory: &mut [u8],
        floppy_image: &Option<FloppyDisk>,
    ) -> Result<InterruptResult, BiosError> {
        let sector_count = cpu.get_reg(Reg8::Al) as usize;
        let cylinder = cpu.get_reg(Reg8::Ch) as usize;
        let sector_num = cpu.get_reg(Reg8::Cl) as usize;
        let head = cpu.get_reg(Reg8::Dh) as usize;
        let dest = SegmentedAddr::new(cpu.get_segment_reg(SegmentReg::Es), cpu.get_reg(Reg16::Bx))
            .to_physical();

        let Some(disk) = floppy_image else {
            cpu.set_reg(Reg8::Ah, 0x80);
            cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        let geometry = &disk.geometry;
        let Some(image_offset) = geometry.byte_offset(cylinder, head, sector_num) else {
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
