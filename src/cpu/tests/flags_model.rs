use super::{Cpu, CpuModel, CpuStepResult};

// Intel SDM, Processor Identification: FLAGS bits 12..15 read as ones on
// 8088, and zeros on 286 in real mode. Bit 1 is fixed at one on both.
// https://www.intel.com/content/dam/support/us/en/documents/processors/pentium4/sb/25366521.pdf
// (stack word, expected 8088 FLAGS, expected 286 real-mode FLAGS).
const FLAG_CASES: &[(u16, u16, u16)] = &[
    (0x0000, 0xF002, 0x0002),
    (0xFFFF, 0xFFD7, 0x0FD7),
    (0x0002, 0xF002, 0x0002), // Bit 1 stays set.
    (0x0028, 0xF002, 0x0002), // Reserved bits 3 and 5 stay clear.
    (0xF000, 0xF002, 0x0002), // Bits 12..15 cannot be changed.
    (0x0FD5, 0xFFD7, 0x0FD7), // All nine writable flags.
    (0x0001, 0xF003, 0x0003), // CF.
    (0x0004, 0xF006, 0x0006), // PF.
    (0x0010, 0xF012, 0x0012), // AF.
    (0x0040, 0xF042, 0x0042), // ZF.
    (0x0080, 0xF082, 0x0082), // SF.
    (0x0100, 0xF102, 0x0102), // TF.
    (0x0200, 0xF202, 0x0202), // IF.
    (0x0400, 0xF402, 0x0402), // DF.
    (0x0800, 0xF802, 0x0802), // OF.
];

fn registers(cpu: &Cpu) -> [u16; 10] {
    [
        cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.bp, cpu.si, cpu.di, cpu.ds, cpu.es, cpu.ss,
    ]
}

fn prepare(cpu: &mut Cpu, instruction: &[u8], sp: u16) -> Vec<u8> {
    [cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.bp, cpu.si, cpu.di] =
        [0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
    cpu.ip = 0x0100;
    cpu.sp = sp;
    let mut memory = vec![0xCC; 0x50000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    memory
}

#[test]
fn i8088_constructor_and_pushf_expose_f002() {
    check_initial_pushf(Cpu::new(CpuModel::I8088, 0), 0xF002);
}

#[test]
fn i80286_constructor_and_pushf_expose_0002() {
    check_initial_pushf(Cpu::new(CpuModel::I80286, 0), 0x0002);
}

fn check_initial_pushf(mut cpu: Cpu, expected_flags: u16) {
    assert_eq!(cpu.flags, expected_flags, "initial FLAGS");
    for (sp, final_sp, address) in [(0x1000, 0x0FFE, 0x30FFE), (0x0000, 0xFFFE, 0x3FFFE)] {
        let mut memory = prepare(&mut cpu, &[0x9C], sp);
        let saved_registers = registers(&cpu);
        let mut expected_memory = memory.clone();
        expected_memory[address..address + 2].copy_from_slice(&expected_flags.to_le_bytes());

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(cpu.flags, expected_flags, "PUSHF preserves FLAGS");
        assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x1000, 0x0101, final_sp));
        assert_eq!(registers(&cpu), saved_registers);
        assert!(
            memory == expected_memory,
            "PUSHF writes exactly one stack word"
        );
    }
}

#[test]
fn i8088_popf_pushf_restore_each_writable_flag_and_reserved_bits() {
    for &(stack_word, expected, _) in FLAG_CASES {
        check_popf_pushf(
            Cpu::new(CpuModel::I8088, 0),
            stack_word,
            expected,
            [0xF002, 0xFFD7],
        );
    }
}

#[test]
fn i80286_popf_pushf_restore_each_writable_flag_and_reserved_bits() {
    for &(stack_word, _, expected) in FLAG_CASES {
        check_popf_pushf(
            Cpu::new(CpuModel::I80286, 0),
            stack_word,
            expected,
            [0x0002, 0x0FD7],
        );
    }
}

fn check_popf_pushf(mut cpu: Cpu, stack_word: u16, expected_flags: u16, initial_flags: [u16; 2]) {
    for flags in initial_flags {
        for (sp, popped_sp, address) in [(0x1000, 0x1002, 0x31000), (0xFFFE, 0x0000, 0x3FFFE)] {
            let mut memory = prepare(&mut cpu, &[0x9D, 0x9C], sp);
            cpu.flags = flags;
            memory[address..address + 2].copy_from_slice(&stack_word.to_le_bytes());
            let saved_registers = registers(&cpu);
            let mut expected_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(
                cpu.flags, expected_flags,
                "POPF stack word={stack_word:04X}"
            );
            assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x1000, 0x0101, popped_sp));
            assert_eq!(registers(&cpu), saved_registers);
            assert!(memory == expected_memory, "POPF leaves memory intact");

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            expected_memory[address..address + 2].copy_from_slice(&expected_flags.to_le_bytes());
            assert_eq!(cpu.flags, expected_flags, "PUSHF preserves restored FLAGS");
            assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x1000, 0x0102, sp));
            assert_eq!(registers(&cpu), saved_registers);
            assert!(memory == expected_memory, "PUSHF exposes normalized FLAGS");
        }
    }
}

#[test]
fn i8088_iret_restores_model_specific_flags_from_a_frame() {
    for &(stack_word, expected, _) in FLAG_CASES {
        check_iret(
            Cpu::new(CpuModel::I8088, 0),
            stack_word,
            expected,
            [0xF002, 0xFFD7],
        );
    }
}

#[test]
fn i80286_iret_restores_real_mode_flags_from_a_frame() {
    for &(stack_word, _, expected) in FLAG_CASES {
        check_iret(
            Cpu::new(CpuModel::I80286, 0),
            stack_word,
            expected,
            [0x0002, 0x0FD7],
        );
    }
}

fn check_iret(mut cpu: Cpu, stack_word: u16, expected_flags: u16, initial_flags: [u16; 2]) {
    for flags in initial_flags {
        for (sp, final_sp, address) in [(0x1000, 0x1006, 0x31000), (0xFFFA, 0x0000, 0x3FFFA)] {
            let mut memory = prepare(&mut cpu, &[0xCF], sp);
            cpu.flags = flags;
            let [low, high] = stack_word.to_le_bytes();
            memory[address..address + 6].copy_from_slice(&[
                0x68, 0x24, // Return IP=2468h.
                0x78, 0x56, // Return CS=5678h.
                low, high,
            ]);
            let saved_registers = registers(&cpu);
            let original_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(
                cpu.flags, expected_flags,
                "IRET stack word={stack_word:04X}"
            );
            assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x5678, 0x2468, final_sp));
            assert_eq!(registers(&cpu), saved_registers);
            assert!(memory == original_memory, "IRET leaves its frame intact");
        }
    }
}

#[test]
fn guest_flags_probe_identifies_8088() {
    check_guest_probe(Cpu::new(CpuModel::I8088, 0), 0xF000);
}

#[test]
fn guest_flags_probe_identifies_286() {
    check_guest_probe(Cpu::new(CpuModel::I80286, 0), 0x0000);
}

fn check_guest_probe(mut cpu: Cpu, expected_ax: u16) {
    let mut memory = prepare(
        &mut cpu,
        &[
            0x31, 0xC0, // XOR AX,AX.
            0x50, // PUSH AX.
            0x9D, // POPF.
            0x9C, // PUSHF.
            0x58, // POP AX.
            0x25, 0x00, 0xF0, // AND AX,F000h.
        ],
        0x1000,
    );
    let saved_registers = registers(&cpu);
    let mut expected_registers = saved_registers;
    expected_registers[0] = expected_ax;

    for _ in 0..6 {
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
    }

    assert_eq!(cpu.ax, expected_ax);
    assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x1000, 0x0109, 0x1000));
    assert_eq!(registers(&cpu), expected_registers);
}
