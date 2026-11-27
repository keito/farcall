use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn assert_flags(actual: u16, initial: u16, expected: u16, count: u8) {
    if count == 0 {
        assert_eq!(actual, initial, "zero count preserves every flag");
    } else {
        // AF is undefined for nonzero counts; OF is undefined for counts > 1.
        // Preserve control flags and check only defined arithmetic flags.
        let ignored = 0x0010 | if count == 1 { 0 } else { 0x0800 };
        assert_eq!(actual & !ignored, (initial & !0x08D5) | expected);
    }
}

// Literal expectations: operand, count, result, CF/PF/ZF/SF/OF.
fn check_dh_cases(cases: &[(u8, u8, u8, u16)]) {
    for &(operand, count, result, flags) in cases {
        for initial_flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.dx = (u16::from(operand) << 8) | 0x5A;
            cpu.cx = 0xA500 | u16::from(count);
            cpu.flags = initial_flags;
            let mut expected = registers(&cpu);
            expected[2] = (u16::from(result) << 8) | 0x5A;
            let mut memory = [0xD2, 0xE6]; // SHL DH, CL; no immediate byte.

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected, "DH={operand:02X}, CL={count}");
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0xD2, 0xE6]);
            assert_flags(cpu.flags, initial_flags, flags, count);
        }
    }
}

#[test]
fn one_bit_shifts_set_carry_overflow_sign_zero_and_parity() {
    check_dh_cases(&[
        (0x00, 1, 0x00, 0x0044), // Zero cannot produce carry or overflow.
        (0x01, 1, 0x02, 0x0000), // Odd parity.
        (0x03, 1, 0x06, 0x0004), // Even parity.
        (0x40, 1, 0x80, 0x0880), // Signed overflow without carry.
        (0x7F, 1, 0xFE, 0x0880),
        (0x80, 1, 0x00, 0x0845), // Carry and signed overflow.
        (0xC0, 1, 0x80, 0x0081), // Carry without signed overflow.
        (0xFF, 1, 0xFE, 0x0081),
    ]);
}

#[test]
fn multi_bit_shifts_take_carry_from_the_last_bit_shifted_out() {
    check_dh_cases(&[
        (0x80, 2, 0x00, 0x0044), // First bit out is 1, last bit out is 0.
        (0x40, 2, 0x00, 0x0045), // Original bit 6 becomes carry.
        (0x21, 3, 0x08, 0x0001),
        (0x01, 7, 0x80, 0x0080),
        (0x02, 7, 0x00, 0x0045), // Original bit 1 becomes carry.
        (0x03, 7, 0x80, 0x0081),
    ]);
}

#[test]
fn count_eight_clears_the_byte_and_moves_original_bit_zero_to_carry() {
    check_dh_cases(&[
        (0x00, 8, 0x00, 0x0044),
        (0x01, 8, 0x00, 0x0045),
        (0x80, 8, 0x00, 0x0044),
        (0xFF, 8, 0x00, 0x0045),
    ]);
}

#[test]
fn large_8086_counts_are_not_masked_and_do_not_panic() {
    // Original 8086 shifts all CL times. After more than eight one-bit
    // shifts, the byte and the last bit shifted out are both zero.
    for count in [9, 15, 16, 31, 32, 33, 63, 64, 127, 128, 255] {
        check_dh_cases(&[(0xFF, count, 0x00, 0x0044)]);
    }
}

#[test]
fn zero_count_preserves_every_register_and_flag() {
    for rm in 0..8 {
        for flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = [0xA55A, 0x8100, 0x407F, 0xFF01];
            cpu.flags = flags;
            let original = registers(&cpu);
            let instruction = [0xD2, 0xE0 | rm];
            let mut memory = instruction;

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), original, "r/m={rm}");
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, instruction);
            assert_flags(cpu.flags, flags, 0, 0);
        }
    }
}

#[test]
fn register_destinations_preserve_the_other_byte_and_unrelated_registers() {
    for rm in [0u8, 2, 3, 4, 5, 6, 7] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = [0xA55A, 0xB501, 0xC56A, 0xD57A];
        cpu.flags = 0x0FD7;
        let index = usize::from(rm & 3);
        let shift = if rm < 4 { 0 } else { 8 };
        let mut words = [cpu.ax, cpu.cx, cpu.dx, cpu.bx];
        words[index] = (words[index] & !(0xFF << shift)) | (0x80 << shift);
        [cpu.ax, cpu.cx, cpu.dx, cpu.bx] = words;
        let mut expected = registers(&cpu);
        expected[index] &= !(0xFF << shift);
        let instruction = [0xD2, 0xE0 | rm];
        let mut memory = instruction;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected, "r/m={rm}");
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, instruction);
        assert_flags(cpu.flags, 0x0FD7, 0x0845, 1);
    }
}

#[test]
fn shifting_cl_uses_the_original_count_even_though_cl_is_the_destination() {
    for (count, result, flags) in [(1, 2, 0), (2, 8, 0), (3, 24, 4), (8, 0, 0x44)] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.cx = 0xA500 | count;
        cpu.flags = 0x0FD7;
        let mut expected = registers(&cpu);
        expected[1] = 0xA500 | result;
        let mut memory = [0xD2, 0xE1]; // SHL CL, CL

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.ip, 2);
        assert_flags(cpu.flags, 0x0FD7, flags, count as u8);
    }
}

#[test]
fn memory_operands_use_the_default_or_overridden_segment_and_write_one_byte() {
    for (operand, offset, default_base) in [
        (&[0x20][..], 0x0240, 0x20000),             // BX+SI
        (&[0x22][..], 0x0340, 0x30000),             // BP+SI: SS
        (&[0x26, 0x35, 0x12][..], 0x1235, 0x20000), // direct disp16
        (&[0x66, 0xFE][..], 0x02FE, 0x30000),       // BP-2: signed disp8
        (&[0xA6, 0xFE, 0xFF][..], 0x02FE, 0x30000), // BP+FFFE: disp16
        (&[0xA4, 0xC0, 0xFF][..], 0x0000, 0x20000), // SI+FFC0 wraps
    ] {
        for (prefix, base) in [
            (None, default_base),
            (Some(0x26), 0x40000),
            (Some(0x2E), 0x10000),
            (Some(0x36), 0x30000),
            (Some(0x3E), 0x20000),
        ] {
            // Count zero still consumes the ModR/M displacement and prefix.
            for count in [0, 1] {
                let mut cpu = Cpu::new(CpuModel::I8088, 0);
                [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
                [cpu.bx, cpu.bp, cpu.si] = [0x0200, 0x0300, 0x0040];
                cpu.cx = 0xA500 | count;
                cpu.ip = 0x0100;
                cpu.flags = 0x0FD7;
                let original_registers = registers(&cpu);
                let mut instruction: Vec<u8> = prefix.into_iter().collect();
                instruction.push(0xD2);
                instruction.extend_from_slice(operand);
                let mut memory = vec![0xCC; 0x50000];
                memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
                memory[base + offset] = 0x80;
                let mut expected_memory = memory.clone();
                if count != 0 {
                    expected_memory[base + offset] = 0;
                }

                step(&mut cpu, &mut memory);

                assert_eq!(registers(&cpu), original_registers);
                assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
                assert!(
                    memory == expected_memory,
                    "instruction={instruction:02X?}, CL={count}"
                );
                assert_flags(cpu.flags, 0x0FD7, 0x0845, count as u8);
            }
        }
    }
}

#[test]
fn instruction_fetch_wraps_ip_between_prefix_opcode_and_mod_rm() {
    for (ip, next) in [(0xFFFDu16, 0), (0xFFFE, 1), (0xFFFF, 2)] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.cs = 0x1000;
        cpu.ip = ip;
        cpu.dx = 0x805A;
        cpu.cx = 1;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; 0x20000];
        for (index, byte) in [0x2E, 0xD2, 0xE6].into_iter().enumerate() {
            memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
        }
        let original = memory.clone();

        step(&mut cpu, &mut memory);

        assert_eq!(cpu.dx, 0x005A);
        assert_eq!(cpu.cx, 1);
        assert_eq!(cpu.cs, 0x1000);
        assert_eq!(cpu.ip, next);
        assert!(memory == original);
        assert_flags(cpu.flags, 0x0FD7, 0x0845, 1);
    }
}

#[test]
fn memory_operand_wraps_at_the_twenty_bit_bus_boundary() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ds = 0xFFFF;
    cpu.bx = 0x0020;
    cpu.cx = 1;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x100000];
    memory[0x0100..0x0102].copy_from_slice(&[0xD2, 0x27]); // SHL byte [BX], CL
    memory[0x00010] = 0x80; // FFFF:0020 wraps to physical 00010.
    let mut expected = memory.clone();
    expected[0x00010] = 0;
    let original_registers = registers(&cpu);

    step(&mut cpu, &mut memory);

    assert!(memory == expected);
    assert_eq!(registers(&cpu), original_registers);
    assert_eq!(cpu.ip, 0x0102);
    assert_flags(cpu.flags, 0x0FD7, 0x0845, 1);
}

#[test]
fn missing_memory_operand_reports_an_error_without_changing_flags_or_registers() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ds = 0x1000;
    cpu.cx = 1;
    cpu.flags = 0x0FD7;
    let original = registers(&cpu);
    let instruction = [0xD2, 0x26, 0x00, 0x02]; // SHL byte [DS:0200], CL
    let mut memory = instruction;

    assert!(matches!(
        cpu.step(&mut memory),
        Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
    ));

    assert_eq!(registers(&cpu), original);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(cpu.ip, 4);
    assert_eq!(memory, instruction);
}
