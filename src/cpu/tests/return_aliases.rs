use super::{Cpu, CpuModel, CpuStepResult};

// On 8088, C0 aliases C2 (RET imm16) and C1 aliases C3 (RET).
// Microcode analysis: https://www.righto.com/2023/07/undocumented-8086-instructions.html
// C2/C3 remain ordinary near returns on the 286, where C0/C1 are shifts/rotates.

#[test]
fn i8088_c0_alias_pops_ip_then_discards_imm16_bytes() {
    check_ret_immediate(false, 0xC0);
}

#[test]
fn i8088_c1_alias_pops_only_ip() {
    check_plain_ret(false, 0xC1);
}

#[test]
fn i286_c2_still_pops_ip_then_discards_imm16_bytes() {
    check_ret_immediate(true, 0xC2);
}

#[test]
fn i286_c3_still_pops_only_ip() {
    check_plain_ret(true, 0xC3);
}

fn check_ret_immediate(is_286: bool, opcode: u8) {
    // Literal final SP values independently cover pop and cleanup wrapping.
    for (sp, immediate, final_sp) in [
        (0x1000, 0x0000u16, 0x1002),
        (0x1000, 0x0004, 0x1006),
        (0x1000, 0x1234, 0x2236),
        // On 286, C0 E0 21 means SHL AL,33; on 8088 the immediate is 21E0h.
        (0x1000, 0x21E0, 0x31E2),
        (0xFFFE, 0x0004, 0x0004),
        (0x0000, 0xFFFF, 0x0001),
    ] {
        let [low, high] = immediate.to_le_bytes();
        check_return(is_286, &[opcode, low, high], sp, final_sp);
    }
    if !is_286 {
        // The 8088 also wraps the stack word itself across SS:FFFF.
        check_return(false, &[opcode, 0x04, 0x00], 0xFFFF, 0x0005);
    }
}

fn check_plain_ret(is_286: bool, opcode: u8) {
    for (sp, final_sp) in [(0x1000, 0x1002), (0xFFFE, 0x0000), (0x0000, 0x0002)] {
        check_return(is_286, &[opcode], sp, final_sp);
    }
    if !is_286 {
        check_return(false, &[opcode], 0xFFFF, 0x0001);
    }
}

fn other_registers(cpu: &Cpu) -> [u16; 11] {
    [
        cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es, cpu.ss,
    ]
}

fn check_return(is_286: bool, instruction: &[u8], sp: u16, final_sp: u16) {
    let ips: &[u16] = if is_286 {
        &[0x0100]
    } else {
        &[0x0100, 0xFFFE, 0xFFFF]
    };
    for &ip in ips {
        for pattern in [0x0000, 0x0FD5] {
            let model = if is_286 {
                CpuModel::I80286
            } else {
                CpuModel::I8088
            };
            let mut cpu = Cpu::new(model, 0);
            [cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.bp, cpu.si, cpu.di] =
                [0xA55A, 0x2345, 0xC533, 0x4567, 0x5678, 0x6789, 0x789A];
            [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
            cpu.ip = ip;
            cpu.sp = sp;
            cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | pattern;
            let flags = cpu.flags;
            let saved_registers = other_registers(&cpu);
            let mut memory = vec![0xCC; 0x50000];
            for (index, &byte) in instruction.iter().enumerate() {
                memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
            }
            // Near return target is 2479h, always read from SS regardless of DS/ES.
            memory[0x30000 + usize::from(sp)] = 0x79;
            memory[0x30000 + usize::from(sp.wrapping_add(1))] = 0x24;
            let original_memory = memory.clone();

            let result = cpu.step(&mut memory).expect("near return should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(
                cpu.ip, 0x2479,
                "return to the popped IP, opcode={:02X}",
                instruction[0]
            );
            assert_eq!(
                cpu.sp, final_sp,
                "opcode={:02X}, initial SP={sp:04X}",
                instruction[0]
            );
            assert_eq!(cpu.flags, flags, "RET preserves FLAGS");
            assert_eq!(
                other_registers(&cpu),
                saved_registers,
                "near RET preserves CS and data registers"
            );
            assert!(
                memory == original_memory,
                "RET must not erase its stack word"
            );
        }
    }
}

#[test]
fn i8088_c1_does_not_fetch_a_modrm_or_immediate_byte() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ip = 0x3FFF;
    cpu.sp = 0x0100;
    let flags = cpu.flags;
    let mut memory = vec![0xCC; 0x4000];
    memory[0x3FFF] = 0xC1; // Last available byte: fetching a ModR/M byte would panic.
    memory[0x0100..0x0102].copy_from_slice(&[0x42, 0x00]);
    let original_memory = memory.clone();

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));

    assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0, 0x0042, 0x0102));
    assert_eq!(cpu.flags, flags);
    assert!(memory == original_memory);
}
