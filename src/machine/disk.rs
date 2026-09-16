use crate::{
    cpu::Flag,
    memory::{PhysAddr, validate_buffer, write_buffer},
    registers::{Reg8, Reg16, SegmentReg},
};

use super::{InterruptResult, Machine, MachineError};

impl Machine {
    /// Emulates the BIOS disk service
    pub(super) fn disk_service(&mut self) -> Result<InterruptResult, MachineError> {
        // TODO: Support more drives
        let drive = self.cpu.get_reg8(Reg8::Dl);
        if drive != 0 {
            self.cpu.set_reg8(Reg8::Ah, 0x01);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        let operation = self.cpu.get_reg8(Reg8::Ah);
        match operation {
            0x00 => self.operation_reset(),
            0x02 => self.operation_read(),
            _ => {
                self.cpu.set_reg8(Reg8::Ah, 0x01);
                self.cpu.set_flag(Flag::Carry, true);
                return Ok(InterruptResult::ResumeExecution);
            }
        }
    }

    fn operation_reset(&mut self) -> Result<InterruptResult, MachineError> {
        self.cpu.set_reg8(Reg8::Ah, 0);
        self.cpu.set_flag(Flag::Carry, false);
        Ok(InterruptResult::ResumeExecution)
    }

    fn operation_read(&mut self) -> Result<InterruptResult, MachineError> {
        let sector_count = self.cpu.get_reg8(Reg8::Al) as usize;
        let cylinder = self.cpu.get_reg8(Reg8::Ch) as usize;
        let sector_num = self.cpu.get_reg8(Reg8::Cl) as usize;
        let head = self.cpu.get_reg8(Reg8::Dh) as usize;
        let dest = PhysAddr::new(
            self.cpu.get_segment_reg(SegmentReg::Es),
            self.cpu.get_reg16(Reg16::Bx),
        );

        let Some(image) = &self.floppy_image else {
            self.cpu.set_reg8(Reg8::Ah, 0x80);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };

        let Some(image_offset) = calc_360kib_disk_offset(cylinder, head, sector_num) else {
            self.cpu.set_reg8(Reg8::Ah, 0x04);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        };
        let num_bytes = 512 * sector_count;

        if sector_count == 0 || sector_count > 10 - sector_num {
            self.cpu.set_reg8(Reg8::Ah, 0x01);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        // Make sure the disk image is long enough
        if image_offset + num_bytes > image.len() {
            self.cpu.set_reg8(Reg8::Ah, 0x04);
            self.cpu.set_flag(Flag::Carry, true);
            return Ok(InterruptResult::ResumeExecution);
        }

        // Write sector to destination
        let validated_buffer = validate_buffer(&mut self.memory, dest, num_bytes)?;
        write_buffer(
            validated_buffer,
            &image[image_offset..image_offset + num_bytes],
        )?;

        self.cpu.set_reg8(Reg8::Ah, 0);
        self.cpu.set_flag(Flag::Carry, false);

        Ok(InterruptResult::ResumeExecution)
    }
}

// Calculate the offset in bytes for a 360KiB disk given CHS
pub(super) fn calc_360kib_disk_offset(
    cylinder: usize,
    head: usize,
    sector: usize,
) -> Option<usize> {
    // Floppy has 40 cylinders (0-index) 2 heads (0-index) 9 sectors (1-index)
    if cylinder > 39 || head > 1 || sector > 9 || sector == 0 {
        return None;
    }
    let sector_index = ((cylinder * 2) + head) * 9 + (sector - 1);
    Some(sector_index * 512)
}
