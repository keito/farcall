use super::{Cpu, CpuStepResult};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

// Operand, imm16, initial CF, result, literal CF/PF/AF/ZF/SF/OF expectations.
type WordCase = (u16, u16, bool, u16, u16);

const ADD_CASES: &[WordCase] = &[
    (0x1234, 0x0102, true, 0x1336, 0x0004), // ADD ignores incoming carry.
    (0x0000, 0x0000, false, 0x0000, 0x0044),
    (0x00FF, 0x0001, false, 0x0100, 0x0014),
    (0x7FFF, 0x0001, false, 0x8000, 0x0894),
    (0x8000, 0x8000, false, 0x0000, 0x0845),
    (0xFFFF, 0x0001, false, 0x0000, 0x0055),
    (0xFFFF, 0xFFFF, false, 0xFFFE, 0x0091),
];

const OR_CASES: &[WordCase] = &[
    (0x1234, 0x0102, true, 0x1336, 0x0004),
    (0xF0F0, 0x0F0F, false, 0xFFFF, 0x0084),
    (0x0000, 0x0000, true, 0x0000, 0x0044),
    (0x8000, 0x0001, false, 0x8001, 0x0080),
];

const ADC_CASES: &[WordCase] = &[
    (0x1234, 0x0102, true, 0x1337, 0x0000),
    (0x1234, 0x0102, false, 0x1336, 0x0004),
    (0xFFFF, 0x0000, true, 0x0000, 0x0055),
    (0x0000, 0xFFFF, true, 0x0000, 0x0055), // RHS + carry must not truncate.
    (0x7FFF, 0x0000, true, 0x8000, 0x0894),
    (0x0000, 0x7FFF, true, 0x8000, 0x0894),
    (0x8000, 0x7FFF, true, 0x0000, 0x0055),
    (0x8000, 0xFFFF, true, 0x8000, 0x0095),
    (0x8000, 0x8000, true, 0x0001, 0x0801),
    (0x000F, 0x0000, true, 0x0010, 0x0010),
];

const SBB_CASES: &[WordCase] = &[
    (0x1234, 0x0102, true, 0x1131, 0x0000),
    (0x1234, 0x0102, false, 0x1132, 0x0000),
    (0x0000, 0x0000, true, 0xFFFF, 0x0095),
    (0x0000, 0xFFFF, true, 0x0000, 0x0055), // Borrow even when result equals LHS.
    (0xFFFF, 0xFFFF, true, 0xFFFF, 0x0095),
    (0x8000, 0x7FFF, true, 0x0000, 0x0854),
    (0x7FFF, 0xFFFF, true, 0x7FFF, 0x0015),
    (0x8000, 0x0000, true, 0x7FFF, 0x0814),
    (0x0000, 0x7FFF, true, 0x8000, 0x0095),
    (0x0000, 0x8000, true, 0x7FFF, 0x0015),
    (0x7FFF, 0x8000, true, 0xFFFE, 0x0881),
];

const AND_CASES: &[WordCase] = &[
    (0xA55A, 0x0FF0, true, 0x0550, 0x0004),
    (0xF0F0, 0x0F0F, false, 0x0000, 0x0044),
    (0xFFFF, 0x8001, true, 0x8001, 0x0080),
    (0xFFFF, 0xFFFF, false, 0xFFFF, 0x0084),
];

const SUB_CASES: &[WordCase] = &[
    (0x1234, 0x0102, true, 0x1132, 0x0000), // SUB/CMP ignore incoming carry.
    (0x1234, 0x1234, false, 0x0000, 0x0044),
    (0x0000, 0x0001, false, 0xFFFF, 0x0095),
    (0x0100, 0x0001, false, 0x00FF, 0x0014),
    (0x8000, 0x0001, false, 0x7FFF, 0x0814),
    (0x7FFF, 0xFFFF, false, 0x8000, 0x0885),
    (0x0000, 0xFFFF, false, 0x0001, 0x0011),
];

const XOR_CASES: &[WordCase] = &[
    (0xA55A, 0x0FF0, true, 0xAAAA, 0x0084),
    (0x1234, 0x1234, false, 0x0000, 0x0044),
    (0xFFFF, 0xFFFE, true, 0x0001, 0x0000),
    (0x1234, 0x0000, false, 0x1234, 0x0000), // Parity uses only the low byte.
];

fn assert_81_flags(actual: u16, initial: u16, arithmetic: u16, extension: u8) {
    // AF is undefined for OR/AND/XOR. Control flags and all other bits survive.
    let ignored = if matches!(extension, 1 | 4 | 6) {
        0x0010
    } else {
        0
    };
    assert_eq!(actual & !ignored, (initial & !0x08D5) | arithmetic);
}

fn check_81_register_cases(extension: u8, cases: &[WordCase]) {
    for reg in 0..8 {
        for &(operand, immediate, carry, result, arithmetic) in cases {
            for base_flags in [0, 0x0FD6] {
                let flags = base_flags | u16::from(carry);
                let mut cpu = Cpu::new();
                let mut values = [
                    0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A, 0x89AB,
                ];
                values[reg] = operand;
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = values;
                [cpu.es, cpu.ss, cpu.ds] = [0x1000, 0x2000, 0x3000];
                cpu.flags = flags;
                let mut expected = registers(&cpu);
                if extension != 7 {
                    expected[reg] = result;
                }
                let [low, high] = immediate.to_le_bytes();
                let instruction = [0x81, 0xC0 | (extension << 3) | reg as u8, low, high];
                let mut memory = instruction;

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "/{extension}, reg={reg}, lhs={operand:04X}, rhs={immediate:04X}, carry={carry}"
                );
                assert_81_flags(cpu.flags, flags, arithmetic, extension);
                assert_eq!(cpu.ip, 4, "81 consumes a full little-endian immediate word");
                assert_eq!(memory, instruction);
            }
        }
    }
}

#[test]
fn add_81_sets_word_arithmetic_flags_and_ignores_incoming_carry() {
    check_81_register_cases(0, ADD_CASES);
}

#[test]
fn or_81_sets_word_logic_flags() {
    check_81_register_cases(1, OR_CASES);
}

#[test]
fn adc_81_includes_carry_without_truncating_the_sum() {
    check_81_register_cases(2, ADC_CASES);
}

#[test]
fn sbb_81_includes_borrow_without_truncating_the_subtrahend() {
    check_81_register_cases(3, SBB_CASES);
}

#[test]
fn and_81_sets_word_logic_flags() {
    check_81_register_cases(4, AND_CASES);
}

#[test]
fn sub_81_sets_word_arithmetic_flags_and_ignores_incoming_carry() {
    check_81_register_cases(5, SUB_CASES);
}

#[test]
fn xor_81_sets_word_logic_flags() {
    check_81_register_cases(6, XOR_CASES);
}

#[test]
fn cmp_81_sets_subtraction_flags_without_changing_any_register() {
    check_81_register_cases(7, SUB_CASES);
}

#[test]
fn group_81_memory_forms_decode_displacements_before_immediates_and_honor_segments() {
    let operations = [
        ADD_CASES, OR_CASES, ADC_CASES, SBB_CASES, AND_CASES, SUB_CASES, XOR_CASES, SUB_CASES,
    ];
    for (extension, cases) in operations.into_iter().enumerate() {
        let (operand, immediate, carry, result, arithmetic) = cases[0];
        for (mod_rm, displacement, default_base) in [
            (0x06, &[0x01, 0x02][..], 0x20000), // Direct [0201h], DS.
            (0x46, &[0xFE][..], 0x30000),       // [BP-2], SS.
            (0x80, &[0x01, 0x00][..], 0x20000), // [BX+SI+0001h], DS.
        ] {
            for (prefix, segment_base) in [
                (None, default_base),
                (Some(0x26), 0x40000),
                (Some(0x2E), 0x10000),
                (Some(0x36), 0x30000),
                (Some(0x3E), 0x20000),
            ] {
                let mut cpu = Cpu::new();
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = [
                    0xA55A, 0x1234, 0x5678, 0x0100, 0x0800, 0x0203, 0x0100, 0x0900,
                ];
                [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
                cpu.ip = 0x0100;
                cpu.flags = 0x0FD6 | u16::from(carry);
                let flags = cpu.flags;
                let expected_registers = registers(&cpu);
                let mut instruction = Vec::new();
                if let Some(prefix) = prefix {
                    instruction.push(prefix);
                }
                instruction.extend_from_slice(&[0x81, mod_rm | ((extension as u8) << 3)]);
                instruction.extend_from_slice(displacement);
                instruction.extend_from_slice(&immediate.to_le_bytes());
                let mut memory = vec![0xCC; 0x50000];
                memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
                let address = segment_base + 0x0201; // Deliberately unaligned word.
                memory[address..address + 2].copy_from_slice(&operand.to_le_bytes());
                let mut expected_memory = memory.clone();
                if extension != 7 {
                    expected_memory[address..address + 2].copy_from_slice(&result.to_le_bytes());
                }

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(registers(&cpu), expected_registers);
                assert_81_flags(cpu.flags, flags, arithmetic, extension as u8);
                assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
                assert!(
                    memory == expected_memory,
                    "/{extension}, ModR/M={mod_rm:02X}, prefix={prefix:02X?}"
                );
            }
        }
    }
}

#[test]
fn group_81_fetches_both_immediate_bytes_across_ip_wrap() {
    for ip in [0xFFFCu16, 0xFFFD, 0xFFFE, 0xFFFF] {
        let mut cpu = Cpu::new();
        cpu.cs = 0x1000;
        cpu.ip = ip;
        cpu.ax = 0x1234;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; 0x20000];
        for (index, byte) in [0x81, 0xC0, 0x02, 0x01].into_iter().enumerate() {
            memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
        }
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[0] = 0x1336;

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected);
        assert_81_flags(cpu.flags, 0x0FD7, 0x0004, 0);
        assert_eq!(cpu.ip, ip.wrapping_add(4));
        assert!(memory == original_memory);
    }
}

#[test]
fn add_83_sign_extends_the_byte_and_updates_the_full_word_register_and_flags() {
    // Operand, encoded immediate, result, literal CF/PF/AF/ZF/SF/OF bits.
    let cases = [
        (0x0000, 0x00, 0x0000, 0x0044),
        (0x0000, 0x7F, 0x007F, 0x0000),
        (0x0000, 0x80, 0xFF80, 0x0080),
        (0x0100, 0xFF, 0x00FF, 0x0005),
        (0x00FF, 0x01, 0x0100, 0x0014),
        (0x7FFF, 0x01, 0x8000, 0x0894),
        (0x8000, 0xFF, 0x7FFF, 0x0805),
        (0xFFFF, 0x01, 0x0000, 0x0055),
        (0x0080, 0x80, 0x0000, 0x0045),
    ];
    for reg in 0..8 {
        for (operand, immediate, result, arithmetic_flags) in cases {
            for flags in [0, 0x0FD7] {
                let mut cpu = Cpu::new();
                let mut values = [
                    0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A, 0x89AB,
                ];
                values[reg] = operand;
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = values;
                [cpu.es, cpu.ss, cpu.ds] = [0x1000, 0x2000, 0x3000];
                cpu.flags = flags;
                let mut expected = registers(&cpu);
                expected[reg] = result;
                let instruction = [0x83, 0xC0 | reg as u8, immediate];
                let mut memory = instruction;

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "register={reg}, operand={operand:04X}, immediate={immediate:02X}"
                );
                assert_eq!(cpu.flags, (flags & !0x08D5) | arithmetic_flags);
                assert_eq!(cpu.ip, 3, "consume only one immediate byte");
                assert_eq!(memory, instruction);
            }
        }
    }
}

#[test]
fn add_83_updates_both_memory_bytes_and_fetches_the_immediate_after_displacement() {
    for (instruction, address, operand, result, arithmetic_flags) in [
        // ADD word [DS:2000h], 1: carry must reach the high byte.
        (
            &[0x83, 0x06, 0x00, 0x20, 0x01][..],
            0x12000,
            0x00FFu16,
            0x0100u16,
            0x0014,
        ),
        // ADD word [SS:BP-2], -1: BP=2002h and both bytes are signed independently.
        (
            &[0x83, 0x46, 0xFE, 0xFF][..],
            0x22000,
            0x0100,
            0x00FF,
            0x0005,
        ),
        // The same BP address with an ES override.
        (
            &[0x26, 0x83, 0x46, 0xFE, 0xFF][..],
            0x32000,
            0x0100,
            0x00FF,
            0x0005,
        ),
    ] {
        for flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xA55A;
            cpu.bp = 0x2002;
            [cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000];
            cpu.flags = flags;
            let expected_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x40000];
            memory[..instruction.len()].copy_from_slice(instruction);
            memory[address..address + 2].copy_from_slice(&operand.to_le_bytes());
            let mut expected_memory = memory.clone();
            expected_memory[address..address + 2].copy_from_slice(&result.to_le_bytes());

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.flags, (flags & !0x08D5) | arithmetic_flags);
            assert_eq!(cpu.ip, instruction.len() as u16);
            assert!(
                memory == expected_memory,
                "only the destination word should change"
            );
        }
    }
}
