use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn assert_control_flags(actual: u16, initial: u16) {
    // DIV/IDIV leave CF/PF/AF/ZF/SF/OF undefined. Only check the other bits.
    assert_eq!(actual & !0x08D5, initial & !0x08D5);
}

#[test]
fn div_word_uses_the_full_unsigned_dividend_and_returns_quotient_and_remainder() {
    for (dx, ax, divisor, quotient, remainder) in [
        (0x0000, 0x0000, 0x0001, 0x0000, 0x0000),
        (0x0000, 0x000A, 0x0003, 0x0003, 0x0001),
        (0x0000, 0x0010, 0x0002, 0x0008, 0x0000), // Catches shifting by (16 + AX).
        (0x0000, 0xFFFF, 0x0001, 0xFFFF, 0x0000), // Largest valid quotient.
        (0x0001, 0x0001, 0x0002, 0x8000, 0x0001), // Nonzero high half.
        (0xFFFE, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFE), // Large unsigned values.
        (0x0000, 0xFFFF, 0x8000, 0x0001, 0x7FFF),
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.dx = dx;
            cpu.ax = ax;
            cpu.bx = divisor;
            cpu.flags = flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[0] = quotient;
            expected_registers[2] = remainder;
            let mut memory = [0xF7, 0xF3]; // DIV BX; no immediate follows.

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(
                registers(&cpu),
                expected_registers,
                "{dx:04X}:{ax:04X} / {divisor:04X}"
            );
            assert_control_flags(cpu.flags, flags);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0xF7, 0xF3]);
        }
    }
}

#[test]
fn div_word_register_sources_include_ax_and_preserve_unrelated_registers() {
    // DX is tested separately: DIV DX cannot produce a fitting quotient.
    for (source, quotient, remainder) in [
        (0, 1, 0),
        (1, 33, 1),
        (3, 16, 4),
        (4, 14, 2),
        (5, 12, 4),
        (6, 11, 1),
        (7, 10, 0),
    ] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        [
            cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
        ] = [100, 3, 0, 6, 7, 8, 9, 10];
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = quotient;
        expected_registers[2] = remainder;
        let mut memory = [0xF7, 0xF0 | source];
        let original_memory = memory;

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers, "source={source}");
        assert_control_flags(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn div_word_zero_divisor_and_oversized_quotient_report_interrupt_zero() {
    for (dx, ax, divisor) in [
        (0x0000, 0x0000, 0x0000),
        (0x0000, 0x000A, 0x0000),
        (0xFFFF, 0xFFFF, 0x0000),
        (0x0001, 0x0000, 0x0001), // Quotient 10000h must not truncate to zero.
        (0x0002, 0x0000, 0x0002),
        (0xFFFF, 0xFFFF, 0xFFFF),
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.ax = ax;
            cpu.dx = dx;
            cpu.bx = divisor;
            cpu.sp = 0x0800;
            cpu.flags = flags;
            let original_registers = registers(&cpu);
            let mut memory = [0xF7, 0xF3];

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Interrupt(0)
            ));

            // On an 8086 divide error, quotient/remainder are undefined.
            // Do not require a particular AX/DX value; other registers must survive.
            let mut actual_registers = registers(&cpu);
            actual_registers[0] = original_registers[0];
            actual_registers[2] = original_registers[2];
            assert_eq!(actual_registers, original_registers);
            assert_control_flags(cpu.flags, flags);
            assert_eq!(cpu.ip, 2, "8086 return IP points after DIV");
            assert_eq!(
                memory,
                [0xF7, 0xF3],
                "Machine, not CPU decoding, enters the handler"
            );
        }
    }
}

#[test]
fn div_dx_reports_zero_divisor_or_overflow_instead_of_using_a_modified_dx() {
    for dx in [0, 1, 0x8000, 0xFFFF] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.dx = dx;
        cpu.ax = 0x1234;
        let mut memory = [0xF7, 0xF2]; // DIV DX.

        // With nonzero DX, (DX * 65536 + AX) / DX is at least 65536.
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Interrupt(0)
        ));
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [0xF7, 0xF2]);
    }
}

#[test]
fn div_word_reads_segmented_memory_with_displacements_and_bus_wrapping() {
    for (instruction, ds, low, high) in [
        (&[0xF7, 0x34][..], 0x1000, 0x10800, 0x10801), // [SI].
        (&[0xF7, 0x76, 0xFE][..], 0x1000, 0x207FE, 0x207FF), // [BP-2], SS.
        (&[0xF7, 0xB4, 0x34, 0x12][..], 0x1000, 0x11A34, 0x11A35), // [SI+1234].
        (&[0xF7, 0xB4, 0x00, 0xF8][..], 0x1000, 0x10000, 0x10001), // Offset wraps to zero.
        (&[0xF7, 0x36, 0x20, 0x00][..], 0xFFFF, 0x00010, 0x00011),
        (&[0xF7, 0x36, 0x0F, 0x00][..], 0xFFFF, 0xFFFFF, 0x00000),
    ] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.ax = 0x0203;
        cpu.dx = 1;
        cpu.si = 0x0800;
        cpu.bp = 0x0800;
        cpu.ds = ds;
        cpu.ss = 0x2000;
        cpu.ip = 0x0100;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0x0100;
        expected_registers[2] = 3; // 00010203h / 0102h = 0100h remainder 3.
        let mut memory = vec![0xCC; 0x100000];
        memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
        memory[low] = 0x02;
        memory[high] = 0x01;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_control_flags(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert!(memory == original_memory, "instruction={instruction:02X?}");
    }
}

#[test]
fn div_word_fetches_mod_rm_and_displacement_across_ip_wrap_using_cs() {
    let instruction = [0xF7, 0x36, 0x34, 0x12];
    for (ip, next_ip) in [(0xFFFC, 0), (0xFFFD, 1), (0xFFFE, 2), (0xFFFF, 3)] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.cs = 0x3000;
        cpu.ds = 0x1000;
        cpu.ip = ip;
        cpu.ax = 10;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 3;
        expected_registers[2] = 1;
        let mut memory = vec![0xCC; 0x50000];
        let before_wrap = 0x10000 - usize::from(ip);
        memory[0x30000 + usize::from(ip)..0x40000].copy_from_slice(&instruction[..before_wrap]);
        memory[0x30000..0x30000 + instruction.len() - before_wrap]
            .copy_from_slice(&instruction[before_wrap..]);
        memory[0x11234..0x11236].copy_from_slice(&[3, 0]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, next_ip);
        assert_control_flags(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
    }
}

#[test]
fn div_word_missing_memory_is_a_memory_error_not_a_divide_error() {
    for offset in [0x01FFu16, 0x0200] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.ax = 0x1234;
        cpu.dx = 0x5678;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0; 0x0200];
        memory[..4].copy_from_slice(&[0xF7, 0x36, low, high]);
        let original_memory = memory.clone();

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
fn idiv_byte_signed_quotients_remainders_and_8086_boundaries() {
    // Quotient truncates toward zero; a nonzero remainder has the dividend's sign.
    for (ax, divisor, quotient, remainder) in [
        (0x0007, 0x03, 0x02, 0x01),
        (0xFFF9, 0x03, 0xFE, 0xFF),
        (0x0007, 0xFD, 0xFE, 0x01),
        (0xFFF9, 0xFD, 0x02, 0xFF),
        (0x0001, 0xFE, 0x00, 0x01),
        (0xFFFF, 0x02, 0x00, 0xFF),
        (0x0000, 0x80, 0x00, 0x00),
        (0x007F, 0x01, 0x7F, 0x00),
        (0xFF81, 0x01, 0x81, 0x00), // -127 is the 8086 minimum quotient.
        (0xFF80, 0x02, 0xC0, 0x00), // -128 is fine as a dividend.
        (0x0100, 0x04, 0x40, 0x00), // All of AX participates.
        (0x3FFF, 0x80, 0x81, 0x7F), // +16383 / -128 = -127 remainder 127.
        (0xC001, 0x80, 0x7F, 0x81),
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.ax = ax;
            cpu.bx = 0x5500 | divisor;
            cpu.dx = 0xBEEF;
            cpu.flags = flags;
            let mut expected = registers(&cpu);
            expected[0] = (remainder << 8) | quotient;
            let mut memory = [0xF6, 0xFB]; // IDIV BL.

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected, "AX={ax:04X}, BL={divisor:02X}");
            assert_control_flags(cpu.flags, flags);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0xF6, 0xFB]);
        }
    }
}

#[test]
fn idiv_word_signed_quotients_remainders_and_8086_boundaries() {
    for (dx, ax, divisor, quotient, remainder) in [
        (0x0000, 0x0007, 0x0003, 0x0002, 0x0001),
        (0xFFFF, 0xFFF9, 0x0003, 0xFFFE, 0xFFFF),
        (0x0000, 0x0007, 0xFFFD, 0xFFFE, 0x0001),
        (0xFFFF, 0xFFF9, 0xFFFD, 0x0002, 0xFFFF),
        (0x0000, 0x0001, 0xFFFE, 0x0000, 0x0001),
        (0xFFFF, 0xFFFF, 0x0002, 0x0000, 0xFFFF),
        (0x0000, 0x0000, 0x8000, 0x0000, 0x0000),
        (0x0000, 0x7FFF, 0x0001, 0x7FFF, 0x0000),
        (0xFFFF, 0x8001, 0x0001, 0x8001, 0x0000),
        (0xFFFF, 0x8000, 0x0002, 0xC000, 0x0000),
        (0x0000, 0xFFFF, 0x0003, 0x5555, 0x0000), // AX is not separately signed.
        (0x0001, 0x0001, 0x0003, 0x5555, 0x0002), // Include the high half.
        (0xFFFF, 0x0000, 0x0003, 0xAAAB, 0xFFFF), // -65536 / 3.
        (0x3FFF, 0xFFFF, 0x8000, 0x8001, 0x7FFF),
        (0xC000, 0x0001, 0x8000, 0x7FFF, 0x8001),
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.dx = dx;
            cpu.ax = ax;
            cpu.bx = divisor;
            cpu.flags = flags;
            let mut expected = registers(&cpu);
            expected[0] = quotient;
            expected[2] = remainder;
            let mut memory = [0xF7, 0xFB]; // IDIV BX.

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(
                registers(&cpu),
                expected,
                "{dx:04X}:{ax:04X} / {divisor:04X}"
            );
            assert_control_flags(cpu.flags, flags);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0xF7, 0xFB]);
        }
    }
}

#[test]
fn idiv_zero_divisors_and_signed_overflow_report_interrupt_zero_without_panicking() {
    // Original 8086/8088 IDIV rejects -128/-32768 as quotients, unlike later CPUs.
    for (opcode, dx, ax, divisor) in [
        (0xF6, 0xBEEF, 0x0000, 0x00),
        (0xF6, 0xBEEF, 0x8000, 0x00),
        (0xF6, 0xBEEF, 0x0080, 0x01), // +128.
        (0xF6, 0xBEEF, 0xFF80, 0x01), // -128: 8086 quirk.
        (0xF6, 0xBEEF, 0xFF7F, 0x01), // -129.
        (0xF6, 0xBEEF, 0x0100, 0x01), // Must not truncate quotient to a byte.
        (0xF6, 0xBEEF, 0x8000, 0xFF), // Minimum signed dividend / -1.
        (0xF6, 0xBEEF, 0x7FFF, 0x01),
        (0xF7, 0x0000, 0x0000, 0x0000),
        (0xF7, 0x8000, 0xFFFF, 0x0000),
        (0xF7, 0x0000, 0x8000, 0x0001), // +32768.
        (0xF7, 0xFFFF, 0x8000, 0x0001), // -32768: 8086 quirk.
        (0xF7, 0xFFFF, 0x7FFF, 0x0001), // -32769.
        (0xF7, 0x0001, 0x0000, 0x0001), // Must not truncate quotient to a word.
        (0xF7, 0x8000, 0xFFFF, 0x0001), // Previously panicked assembling DX:AX.
        (0xF7, 0x8000, 0x0000, 0xFFFF), // i32::MIN / -1 must not panic.
        (0xF7, 0x7FFF, 0xFFFF, 0x0001),
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.dx = dx;
            cpu.ax = ax;
            cpu.bx = divisor;
            cpu.flags = flags;
            let original = registers(&cpu);
            let mut memory = [opcode, 0xFB];

            assert!(
                matches!(cpu.step(&mut memory).unwrap(), CpuStepResult::Interrupt(0)),
                "opcode={opcode:02X}, {dx:04X}:{ax:04X} / {divisor:04X}"
            );

            // Error quotient/remainder are undefined, but unrelated registers survive.
            let mut actual = registers(&cpu);
            actual[0] = original[0];
            if opcode == 0xF7 {
                actual[2] = original[2];
            }
            assert_eq!(actual, original);
            assert_control_flags(cpu.flags, flags);
            assert_eq!(cpu.ip, 2, "8086 saves the address after IDIV");
            assert_eq!(memory, [opcode, 0xFB]);
        }
    }
}

#[test]
fn idiv_reads_overlapping_source_registers_before_writing_results() {
    for (opcode, mod_rm, ax, dx, expected_ax, expected_dx) in [
        (0xF6, 0xF8, 0xFFF9, 0xBEEF, 0x0001, 0xBEEF), // -7 / AL (-7).
        (0xF6, 0xFC, 0xFFF9, 0xBEEF, 0x0007, 0xBEEF), // -7 / AH (-1).
        (0xF7, 0xF8, 0xFFF9, 0xFFFF, 0x0001, 0x0000), // -7 / AX (-7).
        (0xF7, 0xFA, 0xFFF9, 0xFFFF, 0x0007, 0x0000), // -7 / DX (-1).
    ] {
        let mut cpu = Cpu::new(CpuModel::I8088, 0);
        cpu.ax = ax;
        cpu.dx = dx;
        cpu.flags = 0x0FD7;
        let mut expected = registers(&cpu);
        expected[0] = expected_ax;
        expected[2] = expected_dx;
        let mut memory = [opcode, mod_rm];

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected);
        assert_control_flags(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [opcode, mod_rm]);
    }
}

#[test]
fn idiv_memory_operands_use_the_right_width_segment_and_wrapping() {
    for opcode in [0xF6, 0xF7] {
        for (instruction, ds, address) in [
            (vec![opcode, 0x3C], 0x1000, 0x10800),             // [SI], DS.
            (vec![opcode, 0x7E, 0xFE], 0x1000, 0x207FE),       // [BP-2], SS.
            (vec![0x26, opcode, 0x7E, 0xFE], 0x1000, 0x307FE), // ES overrides SS.
            (vec![opcode, 0xBC, 0x00, 0xF8], 0x1000, 0x10000), // SI+F800 wraps.
            (vec![opcode, 0x3E, 0x0F, 0x00], 0xFFFF, 0xFFFFF), // Bus boundary.
        ] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.ax = 0xFFF9; // -7.
            cpu.dx = if opcode == 0xF6 { 0xBEEF } else { 0xFFFF };
            cpu.ds = ds;
            cpu.ss = 0x2000;
            cpu.es = 0x3000;
            cpu.si = 0x0800;
            cpu.bp = 0x0800;
            cpu.ip = 0x0100;
            cpu.flags = 0x0FD7;
            let mut expected = registers(&cpu);
            expected[0] = if opcode == 0xF6 { 0xFF02 } else { 0x0000 };
            if opcode == 0xF7 {
                expected[2] = 0xFFF9; // -7 / +253 = 0 remainder -7.
            }
            let mut memory = vec![0xCC; 0x100000];
            memory[0x100..0x100 + instruction.len()].copy_from_slice(&instruction);
            memory[address] = 0xFD; // Byte -3, but word +253.
            memory[(address + 1) & 0xFFFFF] = 0;
            let original = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected, "{instruction:02X?}");
            assert_control_flags(cpu.flags, 0x0FD7);
            assert_eq!(cpu.ip, 0x100 + instruction.len() as u16);
            assert!(memory == original);
        }
    }
}
