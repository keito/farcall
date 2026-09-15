use super::{Cpu, CpuError, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn assert_flags(actual: u16, initial: u16, expected: u16) {
    // CF/OF clear; SF/ZF/PF depend on the result. AF is undefined, so ignore it.
    // All other bits, including control flags, must be preserved.
    assert_eq!(actual & !0x0010, (initial & !0x08D5) | expected);
}

#[test]
fn or_byte_sets_flags_and_preserves_source_and_other_register_halves() {
    for (destination, source, expected, flags) in [
        (0x00, 0x00, 0x00, 0x0044), // Zero, even parity.
        (0x12, 0x24, 0x36, 0x0004), // Both operands contribute bits.
        (0x55, 0x55, 0x55, 0x0004), // OR must not behave like XOR.
        (0x80, 0x00, 0x80, 0x0080), // Byte sign, odd parity.
        (0x80, 0x01, 0x81, 0x0084), // Byte sign, even parity.
        (0x00, 0x01, 0x01, 0x0000), // Clear stale ZF/SF/PF.
    ] {
        for initial_flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xA500 | source;
            cpu.bx = 0x5A00 | destination;
            cpu.flags = initial_flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[3] = 0x5A00 | expected;
            let mut memory = [0x08, 0xC3]; // OR BL, AL.

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0x08, 0xC3]);
            assert_flags(cpu.flags, initial_flags, flags);
        }
    }
}

#[test]
fn or_byte_handles_high_registers_and_aliasing_within_ax() {
    for (mod_rm, expected_ax) in [
        (0xE0, 0x1236), // OR AL, AH.
        (0xC4, 0x3624), // OR AH, AL.
        (0xE4, 0x1224), // OR AH, AH leaves the value unchanged.
    ] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x1224;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = expected_ax;
        let mut memory = [0x08, mod_rm];

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [0x08, mod_rm]);
        assert_flags(cpu.flags, 0x0FD7, 0x0004);
    }
}

#[test]
fn or_word_sets_word_sign_and_low_byte_parity_preserving_source() {
    for (destination, source, expected, flags) in [
        (0x0000, 0x0000, 0x0000, 0x0044),
        (0x1234, 0xA55A, 0xB77E, 0x0084), // Full-word OR, not XOR or addition.
        (0x0000, 0x8000, 0x8000, 0x0084), // Sign bit 15, low byte even parity.
        (0x0080, 0x0000, 0x0080, 0x0000), // Bit 7 isn't the word sign.
        (0x0000, 0x0100, 0x0100, 0x0004), // Whole word odd parity, low byte even.
        (0x0100, 0x0001, 0x0101, 0x0000), // Whole word even parity, low byte odd.
    ] {
        for initial_flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = source;
            cpu.bx = destination;
            cpu.flags = initial_flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[3] = expected;
            let mut memory = [0x09, 0xC3]; // OR BX, AX.

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0x09, 0xC3]);
            assert_flags(cpu.flags, initial_flags, flags);
        }
    }
}

#[test]
fn or_ax_ax_checks_mores_read_count_without_changing_it() {
    for (ax, flags) in [(0, 0x0044), (7, 0x0000), (0x1000, 0x0004)] {
        let mut cpu = Cpu::new();
        cpu.ax = ax;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let mut memory = [0x09, 0xC0]; // MORE: OR AX, AX.

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [0x09, 0xC0]);
        assert_flags(cpu.flags, 0x0FD7, flags);
    }
}

#[test]
fn or_memory_uses_the_correct_width_segment_displacement_and_bus_wrapping() {
    for opcode in [0x08, 0x09] {
        for (operand, ds, low, high) in [
            (&[0x04][..], 0x1000, 0x10800, 0x10801),       // [SI], DS.
            (&[0x46, 0xFE][..], 0x1000, 0x207FE, 0x207FF), // [BP-2], SS.
            (&[0x84, 0x34, 0x12][..], 0x1000, 0x11A34, 0x11A35), // [SI+1234].
            (&[0x06, 0xFF, 0x00][..], 0xFFFF, 0x000EF, 0x000F0), // Address wraps.
            (&[0x06, 0x0F, 0x00][..], 0xFFFF, 0xFFFFF, 0x00000), // Word wraps.
        ] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xA55A;
            cpu.si = 0x0800;
            cpu.bp = 0x0800;
            cpu.ds = ds;
            cpu.ss = 0x2000;
            cpu.ip = 0x0100;
            cpu.flags = 0x0FD7;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x100000];
            memory[0x0100] = opcode;
            memory[0x0101..0x0101 + operand.len()].copy_from_slice(operand);
            memory[low] = 0x34;
            memory[high] = 0x12;
            let mut expected_memory = memory.clone();
            expected_memory[low] = 0x7E;
            if opcode == 0x09 {
                expected_memory[high] = 0xB7;
            }

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.ip, 0x0101 + operand.len() as u16);
            assert_flags(
                cpu.flags,
                0x0FD7,
                if opcode == 0x08 { 0x0004 } else { 0x0084 },
            );
            assert!(
                memory == expected_memory,
                "opcode={opcode:02X}, operand={operand:02X?}: only destination bytes change"
            );
        }
    }
}

#[test]
fn or_invalid_memory_returns_error_without_partial_write_or_flag_changes() {
    for (opcode, offset) in [(0x08, 0x0200u16), (0x09, 0x0200), (0x09, 0x01FF)] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xFFFF;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0x12; 0x0200];
        memory[..4].copy_from_slice(&[opcode, 0x06, low, high]);
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        assert!(matches!(
            result,
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}
