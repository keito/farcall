use super::{Cpu, CpuStepResult};

fn other_registers(cpu: &Cpu) -> [u16; 11] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss, cpu.es,
    ]
}

fn setup(instruction: &[u8], sp: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(0);
    [cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di] =
        [0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = 0x0100;
    cpu.sp = sp;
    let mut memory = vec![0xCC; 0x50000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    (cpu, memory)
}

#[test]
fn pushf_exposes_initial_8086_reserved_bits_and_wraps_sp() {
    for (initial_sp, expected_sp, address) in [(0x1000, 0x0FFE, 0x30FFE), (0x0000, 0xFFFE, 0x3FFFE)]
    {
        let (mut cpu, mut memory) = setup(&[0x9C], initial_sp);
        // Leave FLAGS exactly as Cpu::new initialized it; no preceding POPF.
        let original_registers = other_registers(&cpu);
        let mut expected_memory = memory.clone();
        expected_memory[address..address + 2].copy_from_slice(&[0x02, 0xF0]);

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(cpu.flags, 0xF002, "PUSHF preserves the initial flags");
        assert_eq!(cpu.sp, expected_sp);
        assert_eq!(cpu.ip, 0x0101, "PUSHF has only one opcode byte");
        assert_eq!(other_registers(&cpu), original_registers);
        assert!(memory == expected_memory, "write exactly one word at SS:SP");
    }
}

#[test]
fn popf_restores_writable_flags_and_pushf_exposes_fixed_reserved_bits() {
    // Explicit hardware values rather than calling the normalization helper.
    // Each individual flag case also checks its bit position survives POPF.
    for (stack_word, expected_flags) in [
        (0x0000u16, 0xF002u16),
        (0xFFFF, 0xFFD7),
        (0x0028, 0xF002), // Reserved bits 3 and 5 cannot be set.
        (0xF000, 0xF002), // Reserved bits 12..15 and bit 1 read as ones.
        (0x0FD5, 0xFFD7), // All nine writable flags.
        (0x0001, 0xF003), // CF.
        (0x0004, 0xF006), // PF.
        (0x0010, 0xF012), // AF.
        (0x0040, 0xF042), // ZF.
        (0x0080, 0xF082), // SF.
        (0x0100, 0xF102), // TF.
        (0x0200, 0xF202), // IF.
        (0x0400, 0xF402), // DF.
        (0x0800, 0xF802), // OF.
    ] {
        for initial_flags in [0xF002, 0xFFD7] {
            for (initial_sp, popped_sp, address) in
                [(0x1000, 0x1002, 0x31000), (0xFFFE, 0x0000, 0x3FFFE)]
            {
                let (mut cpu, mut memory) = setup(&[0x9D, 0x9C], initial_sp);
                cpu.flags = initial_flags;
                memory[address..address + 2].copy_from_slice(&stack_word.to_le_bytes());
                let original_registers = other_registers(&cpu);
                let mut expected_memory = memory.clone();

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(cpu.flags, expected_flags, "stack word={stack_word:04X}");
                assert_eq!(cpu.sp, popped_sp);
                assert_eq!(cpu.ip, 0x0101, "POPF has no operand bytes");
                assert_eq!(other_registers(&cpu), original_registers);
                assert!(memory == expected_memory, "POPF does not erase its source");

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                expected_memory[address..address + 2]
                    .copy_from_slice(&expected_flags.to_le_bytes());
                assert!(memory == expected_memory, "PUSHF exposes normalized flags");
                assert_eq!(cpu.flags, expected_flags, "PUSHF preserves flags");
                assert_eq!(cpu.sp, initial_sp);
                assert_eq!(cpu.ip, 0x0102);
                assert_eq!(other_registers(&cpu), original_registers);
            }
        }
    }
}
