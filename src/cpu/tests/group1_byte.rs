use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// Expected flags are literal x86 flag bits, not computed by CPU helpers.
// (initial operand, immediate, result, CF/PF/AF/ZF/SF/OF)
type Case = (u8, u8, u8, u16);

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn assert_flags(actual: u16, initial: u16, expected: u16, extension: u8) {
    // AF is undefined for logical operations. All other non-arithmetic bits
    // (including control flags) must remain unchanged.
    let ignored = if matches!(extension, 1 | 4 | 6) {
        0x0010
    } else {
        0
    };
    assert_eq!(actual & !ignored, (initial & !0x08D5) | expected);
}

fn check_cases(extension: u8, cases: &[Case]) {
    check_cases_with_initial_flags(extension, cases, &[0, 0x0FD7]);
}

fn check_cases_with_initial_flags(extension: u8, cases: &[Case], initial_flags: &[u16]) {
    for &(lhs, rhs, result, flags) in cases {
        for &initial_flags in initial_flags {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.ax = 0xA500 | u16::from(lhs);
            cpu.flags = initial_flags;
            let mut expected_registers = registers(&cpu);
            if extension != 7 {
                expected_registers[0] = 0xA500 | u16::from(result);
            }
            let mut memory = [0x80, 0xC0 | (extension << 3), rhs];
            let original_memory = memory;

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));
            assert_eq!(
                registers(&cpu),
                expected_registers,
                "/{extension}: {lhs:02X}, {rhs:02X}"
            );
            assert_eq!(cpu.ip, 3);
            assert_eq!(memory, original_memory);
            assert_flags(cpu.flags, initial_flags, flags, extension);
        }
    }
}

#[test]
fn add_immediate_byte_sets_carry_overflow_auxiliary_carry_and_result_flags() {
    check_cases(
        0,
        &[
            (0x00, 0x00, 0x00, 0x0044),
            (0x00, 0x01, 0x01, 0x0000), // Clears stale arithmetic flags.
            (0x0F, 0x01, 0x10, 0x0010), // Nibble carry only.
            (0x7F, 0x01, 0x80, 0x0890), // Positive signed overflow, no carry.
            (0x80, 0x80, 0x00, 0x0845), // Negative signed overflow and carry.
            (0xFF, 0x01, 0x00, 0x0055), // Carry without signed overflow.
            (0xFF, 0xFF, 0xFE, 0x0091),
            (0x80, 0x01, 0x81, 0x0084),
            (0x7E, 0x01, 0x7F, 0x0000),
        ],
    );
}

#[test]
fn or_immediate_byte_sets_logic_flags() {
    check_cases(
        1,
        &[
            (0x00, 0x00, 0x00, 0x0044),
            (0x12, 0x24, 0x36, 0x0004),
            (0x55, 0x55, 0x55, 0x0004), // Distinguishes OR from ADD/XOR.
            (0x80, 0x00, 0x80, 0x0080),
            (0x80, 0x01, 0x81, 0x0084),
            (0x00, 0x01, 0x01, 0x0000),
        ],
    );
}

#[test]
fn and_immediate_byte_sets_logic_flags() {
    check_cases(
        4,
        &[
            (0xF0, 0x0F, 0x00, 0x0044), // Catches accidentally calling ADD.
            (0xA5, 0x3C, 0x24, 0x0004),
            (0xFF, 0x80, 0x80, 0x0080),
            (0xFF, 0x81, 0x81, 0x0084),
            (0xFF, 0x01, 0x01, 0x0000),
        ],
    );
}

const SUBTRACTION_CASES: &[Case] = &[
    (0x42, 0x42, 0x00, 0x0044),
    (0x02, 0x01, 0x01, 0x0000),
    (0x00, 0x01, 0xFF, 0x0095), // Unsigned and nibble borrow.
    (0x10, 0x01, 0x0F, 0x0014), // Nibble borrow without unsigned borrow.
    (0x80, 0x01, 0x7F, 0x0810), // Negative signed overflow.
    (0x7F, 0xFF, 0x80, 0x0881), // Positive signed overflow and borrow.
    (0x80, 0x00, 0x80, 0x0080),
    (0x00, 0x80, 0x80, 0x0881),
];

#[test]
fn sub_immediate_byte_writes_result_and_sets_arithmetic_flags() {
    check_cases(5, SUBTRACTION_CASES);
}

#[test]
fn xor_immediate_byte_sets_logic_flags() {
    check_cases(
        6,
        &[
            (0x55, 0x55, 0x00, 0x0044),
            (0xA5, 0x3C, 0x99, 0x0084),
            (0xFF, 0x7F, 0x80, 0x0080),
            (0x81, 0x80, 0x01, 0x0000),
            (0x12, 0x24, 0x36, 0x0004),
        ],
    );
}

#[test]
fn cmp_immediate_byte_sets_subtraction_flags_without_writing_result() {
    check_cases(7, SUBTRACTION_CASES);
}

// Each operation applied to 12h and immediate 03h. CMP keeps 12h.
const OPERATIONS: [(u8, u8, u16); 6] = [
    (0, 0x15, 0x0000),
    (1, 0x13, 0x0000),
    (4, 0x02, 0x0000),
    (5, 0x0F, 0x0014),
    (6, 0x11, 0x0004),
    (7, 0x12, 0x0014),
];

#[test]
fn every_implemented_operation_handles_all_eight_byte_registers() {
    for (extension, expected, flags) in OPERATIONS {
        for rm in 0u8..8 {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = [0xA1B1, 0xA2B2, 0xA3B3, 0xA4B4];
            let mut words = [cpu.ax, cpu.cx, cpu.dx, cpu.bx];
            let index = usize::from(rm & 3);
            let shift = if rm < 4 { 0 } else { 8 };
            let preserved_half = words[index] & !(0xFF << shift);
            words[index] = preserved_half | (0x12 << shift);
            [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = words;
            cpu.flags = 0x0FD7;
            let mut expected_registers = registers(&cpu);
            expected_registers[index] = preserved_half | (u16::from(expected) << shift);
            let mut memory = [0x80, 0xC0 | (extension << 3) | rm, 0x03];
            let original_memory = memory;

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));
            assert_eq!(
                registers(&cpu),
                expected_registers,
                "/{extension}, r/m={rm}"
            );
            assert_eq!(memory, original_memory);
            assert_eq!(cpu.ip, 3);
            assert_flags(cpu.flags, 0x0FD7, flags, extension);
        }
    }
}

#[test]
fn immediate_follows_displacement_and_memory_uses_segments_and_wrapping() {
    for (extension, expected, flags) in OPERATIONS {
        for (operand, ds, address) in [
            (&[0x04][..], 0x1000, 0x10800),             // [SI], DS.
            (&[0x46, 0xFE][..], 0x1000, 0x2FFFF),       // [BP-2], SS, offset wraps.
            (&[0x84, 0x34, 0x12][..], 0x1000, 0x11A34), // [SI+1234h].
            (&[0x84, 0x00, 0xF9][..], 0x1000, 0x10100), // [SI+F900h], offset wraps.
            (&[0x06, 0xFF, 0x00][..], 0xFFFF, 0x000EF), // Direct DS address, bus wraps.
        ] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.ip = 0x0100;
            cpu.si = 0x0800;
            cpu.bp = 1;
            cpu.ds = ds;
            cpu.ss = 0x2000;
            cpu.flags = 0x0FD7;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x100000];
            memory[0x0100] = 0x80;
            memory[0x0101..0x0101 + operand.len()].copy_from_slice(operand);
            memory[0x0101] |= extension << 3;
            memory[0x0101 + operand.len()] = 0x03;
            memory[address] = 0x12;
            let mut expected_memory = memory.clone();
            expected_memory[address] = expected;

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));
            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.ip, 0x0102 + operand.len() as u16);
            assert_flags(cpu.flags, 0x0FD7, flags, extension);
            assert!(
                memory == expected_memory,
                "/{extension}, operand={operand:02X?}: only destination may change"
            );
        }
    }
}

#[test]
fn group1_byte_fetches_immediate_across_ip_wrap_using_cs() {
    for (extension, expected, flags) in OPERATIONS {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.cs = 0x1000;
        cpu.ip = 0xFFFE;
        cpu.ax = 0xA512;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0xA500 | u16::from(expected);
        let mut memory = vec![0xCC; 0x20000];
        memory[0x1FFFE] = 0x80;
        memory[0x1FFFF] = 0xC0 | (extension << 3);
        memory[0x10000] = 0x03;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
        assert_eq!(cpu.ip, 1);
        assert_eq!(registers(&cpu), expected_registers);
        assert_flags(cpu.flags, 0x0FD7, flags, extension);
        assert!(memory == original_memory);
    }
}

#[test]
fn invalid_memory_operand_returns_error_without_changing_data_or_flags() {
    for (extension, _, _) in OPERATIONS {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.ax = 0xA512;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let mut memory = [0x80, 0x06 | (extension << 3), 0x00, 0x02, 0x03];
        let original_memory = memory;

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn adc_without_incoming_carry_behaves_like_add() {
    check_cases_with_initial_flags(
        2,
        &[
            (0x00, 0x00, 0x00, 0x0044),
            (0x00, 0x01, 0x01, 0x0000),
            (0x0F, 0x01, 0x10, 0x0010),
            (0x7F, 0x01, 0x80, 0x0890),
            (0x80, 0x80, 0x00, 0x0845),
            (0xFF, 0x01, 0x00, 0x0055),
        ],
        &[0, 0x0FD6], // CF clear, other flags initially clear or set.
    );
}

#[test]
fn adc_includes_incoming_carry_in_result_and_all_arithmetic_flags() {
    check_cases_with_initial_flags(
        2,
        &[
            (0x00, 0x00, 0x01, 0x0000), // Carry is consumed and then cleared.
            (0x0F, 0x00, 0x10, 0x0010), // Carry-in alone causes nibble carry.
            (0x7F, 0x00, 0x80, 0x0890), // Carry-in alone causes signed overflow.
            (0x80, 0x7F, 0x00, 0x0055), // Opposite signs: no signed overflow.
            (0xFF, 0x00, 0x00, 0x0055), // Carry-in causes unsigned carry.
            (0x80, 0x80, 0x01, 0x0801), // Both signed overflow and carry.
            (0xFF, 0xFF, 0xFF, 0x0095), // Full sum is 1FFh.
            (0x00, 0xFF, 0x00, 0x0055), // Do not wrap rhs + carry before adding.
        ],
        &[0x0001, 0x0FD7],
    );
}

#[test]
fn sbb_without_incoming_borrow_behaves_like_sub() {
    check_cases_with_initial_flags(3, SUBTRACTION_CASES, &[0, 0x0FD6]);
}

#[test]
fn sbb_includes_incoming_borrow_in_result_and_all_arithmetic_flags() {
    // Intel SBB: DEST := DEST - (SRC + CF). Expected flags describe the
    // complete subtraction, including cases where SRC + CF is 100h.
    // https://pdos.csail.mit.edu/6.828/2004/readings/i386/SBB.htm
    check_cases_with_initial_flags(
        3,
        &[
            (0x00, 0x00, 0xFF, 0x0095), // Incoming borrow causes underflow.
            (0x01, 0x00, 0x00, 0x0044), // Incoming borrow is consumed, CF clears.
            (0x10, 0x00, 0x0F, 0x0014), // Nibble borrow without unsigned borrow.
            (0x80, 0x00, 0x7F, 0x0810), // Incoming borrow causes signed overflow.
            (0x7F, 0xFF, 0x7F, 0x0011), // Signed result fits despite unsigned borrow.
            (0xFF, 0xFF, 0xFF, 0x0095), // FFh - 100h requires a borrow.
            (0x00, 0xFF, 0x00, 0x0055), // Zero result can still have CF set.
            (0x80, 0x7F, 0x00, 0x0854), // Signed overflow without unsigned borrow.
            (0x80, 0x80, 0xFF, 0x0095),
            (0x0F, 0x0F, 0xFF, 0x0095),
        ],
        &[0x0001, 0x0FD7],
    );
}

fn check_carry_destinations(extension: u8) {
    for incoming in [0u16, 1] {
        // Literal outcomes for 12h +/- 03h +/- incoming CF.
        let (expected, flags) = match (extension, incoming) {
            (2, 0) => (0x15, 0x0000),
            (2, 1) => (0x16, 0x0000),
            (3, 0) => (0x0F, 0x0014),
            (3, 1) => (0x0E, 0x0010),
            _ => unreachable!("test helper only accepts ADC and SBB"),
        };
        for (operand, ds, address) in [
            (&[0xC0][..], 0x1000, None),                      // AL
            (&[0xC1][..], 0x1000, None),                      // CL
            (&[0xC2][..], 0x1000, None),                      // DL
            (&[0xC3][..], 0x1000, None),                      // BL
            (&[0xC4][..], 0x1000, None),                      // AH
            (&[0xC5][..], 0x1000, None),                      // CH
            (&[0xC6][..], 0x1000, None),                      // DH
            (&[0xC7][..], 0x1000, None),                      // BH
            (&[0x04][..], 0x1000, Some(0x10800)),             // [SI], DS
            (&[0x46, 0xFE][..], 0x1000, Some(0x2FFFF)),       // [BP-2], SS, offset wraps
            (&[0x84, 0x34, 0x12][..], 0x1000, Some(0x11A34)), // disp16
            (&[0x84, 0x00, 0xF9][..], 0x1000, Some(0x10100)), // disp16 wraps
            (&[0x06, 0xFF, 0x00][..], 0xFFFF, Some(0x000EF)), // 20-bit wrap
        ] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = [0xA1B1, 0xA2B2, 0xA3B3, 0xA4B4];
            cpu.ip = 0x0100;
            cpu.si = 0x0800;
            cpu.bp = 1;
            cpu.ds = ds;
            cpu.ss = 0x2000;
            let initial_flags = 0x0FD6 | incoming;
            cpu.flags = initial_flags;
            let mut memory = vec![0xCC; 0x100000];
            memory[0x0100] = 0x80;
            memory[0x0101..0x0101 + operand.len()].copy_from_slice(operand);
            memory[0x0101] |= extension << 3;
            memory[0x0101 + operand.len()] = 0x03;
            let mut expected_registers = registers(&cpu);
            let mut expected_memory = memory.clone();

            if let Some(address) = address {
                memory[address] = 0x12;
                expected_memory[address] = expected;
            } else {
                let rm = operand[0] & 7;
                let index = usize::from(rm & 3);
                let shift = if rm < 4 { 0 } else { 8 };
                let mut words = [cpu.ax, cpu.cx, cpu.dx, cpu.bx];
                let other_half = words[index] & !(0xFF << shift);
                words[index] = other_half | (0x12 << shift);
                [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = words;
                expected_registers[index] = other_half | (u16::from(expected) << shift);
            }

            let context = format!("/{extension}, incoming CF={incoming}, operand={operand:02X?}");
            let outcome = cpu.step(&mut memory).expect(&context);
            assert!(matches!(outcome, CpuStepResult::Continue), "{context}");
            assert_eq!(registers(&cpu), expected_registers, "{context}");
            assert_eq!(cpu.ip, 0x0102 + operand.len() as u16, "{context}");
            assert_flags(cpu.flags, initial_flags, flags, extension);
            assert!(
                memory == expected_memory,
                "{context}: only destination may change"
            );
        }
    }
}

#[test]
fn adc_handles_all_byte_registers_and_segmented_memory_with_both_carry_inputs() {
    check_carry_destinations(2);
}

#[test]
fn sbb_handles_all_byte_registers_and_segmented_memory_with_both_borrow_inputs() {
    check_carry_destinations(3);
}

#[test]
fn adc_propagates_carry_from_low_byte_into_high_byte() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ax = 0x00FF;
    cpu.flags = 0x0FD7;
    let mut expected_registers = registers(&cpu);
    let mut memory = [0x80, 0xD0, 0, 0x80, 0xD4, 0]; // ADC AL,0; ADC AH,0.
    let original_memory = memory;

    for (expected_ax, flags, next_ip) in [(0x0000, 0x0055, 3), (0x0100, 0x0000, 6)] {
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
        expected_registers[0] = expected_ax;
        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, next_ip);
        assert_flags(cpu.flags, 0x0FD7, flags, 2);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn sbb_propagates_borrow_from_low_byte_into_high_byte() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ax = 0x0100;
    cpu.flags = 0x0FD7;
    let mut expected_registers = registers(&cpu);
    let mut memory = [0x80, 0xD8, 0, 0x80, 0xDC, 0]; // SBB AL,0; SBB AH,0.
    let original_memory = memory;

    for (expected_ax, flags, next_ip) in [(0x01FF, 0x0095, 3), (0x00FF, 0x0044, 6)] {
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
        expected_registers[0] = expected_ax;
        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, next_ip);
        assert_flags(cpu.flags, 0x0FD7, flags, 3);
        assert_eq!(memory, original_memory);
    }
}
