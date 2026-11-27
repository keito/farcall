use super::{Cpu, CpuModel, CpuStepResult};

// Intel SDM, Architecture Compatibility, PUSH SP: the 8088 pushes the
// decremented SP; the 286 pushes the original SP. Both decrement SP by two.
// https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366821.pdf
// Each case is (initial SP, final SP, expected little-endian stack bytes).
const I8088_CASES: &[(u16, u16, [u8; 2])] = &[
    (0x1000, 0x0FFE, [0xFE, 0x0F]),
    (0x0000, 0xFFFE, [0xFE, 0xFF]),
    (0x0002, 0x0000, [0x00, 0x00]),
];
const I80286_CASES: &[(u16, u16, [u8; 2])] = &[
    (0x1000, 0x0FFE, [0x00, 0x10]),
    (0x0000, 0xFFFE, [0x00, 0x00]),
    (0x0002, 0x0000, [0x02, 0x00]),
];

#[test]
fn i8088_register_encoding_pushes_decremented_sp() {
    check_push_sp(CpuModel::I8088, &[0x54], 0x0101, I8088_CASES);
}

#[test]
fn i8088_modrm_encoding_pushes_decremented_sp() {
    check_push_sp(CpuModel::I8088, &[0xFF, 0xF4], 0x0102, I8088_CASES);
}

#[test]
fn i80286_register_encoding_pushes_original_sp() {
    check_push_sp(CpuModel::I80286, &[0x54], 0x0101, I80286_CASES);
}

#[test]
fn i80286_modrm_encoding_pushes_original_sp() {
    check_push_sp(CpuModel::I80286, &[0xFF, 0xF4], 0x0102, I80286_CASES);
}

fn check_push_sp(
    model: CpuModel,
    instruction: &[u8],
    expected_ip: u16,
    cases: &[(u16, u16, [u8; 2])],
) {
    let mut cpu = Cpu::new(model, 0);
    for &(initial_sp, expected_sp, stack_bytes) in cases {
        for initial_flags in [0x0002, 0x0FD7] {
            [cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di] =
                [0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A];
            [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
            cpu.sp = initial_sp;
            cpu.ip = 0x0100;
            cpu.flags = initial_flags;
            let other_registers = [
                cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss,
                cpu.es,
            ];
            let mut memory = vec![0xCC; 0x50000];
            memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
            let mut expected_memory = memory.clone();
            let address = 0x30000 + usize::from(expected_sp);
            expected_memory[address..address + 2].copy_from_slice(&stack_bytes);

            let result = cpu.step(&mut memory).expect("PUSH SP should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.sp, expected_sp);
            assert_eq!(cpu.ip, expected_ip, "consume only the instruction bytes");
            assert_eq!(cpu.flags, initial_flags, "PUSH preserves every flag");
            assert_eq!(
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss,
                    cpu.es,
                ],
                other_registers
            );
            assert_eq!(
                &memory[address..address + 2],
                &stack_bytes,
                "PUSH SP {instruction:02X?}, initial SP={initial_sp:04X}"
            );
            assert!(memory == expected_memory, "write only the two stack bytes");
        }
    }
}
