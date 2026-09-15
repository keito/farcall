use super::{Cpu, CpuError, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn assert_flags(actual: u16, initial: u16, expected: u16) {
    // XOR defines CF/PF/ZF/SF/OF, preserves control flags, and leaves AF undefined.
    // Deliberately do not require any particular value for AF.
    assert_eq!(actual & !0x0010, (initial & !0x08D5) | expected);
}

#[test]
fn xor_word_results_set_flags_and_preserve_the_source() {
    for (destination, source, expected, flags) in [
        (0x1234, 0x1234, 0x0000, 0x0044), // Zero: ZF and PF.
        (0x1234, 0x0234, 0x1000, 0x0004), // PF examines low byte, not whole word.
        (0x1234, 0x1335, 0x0101, 0x0000), // Low byte has odd parity, word has even.
        (0xA55A, 0x255A, 0x8000, 0x0084), // Sign is bit 15; low byte is zero.
        (0x0080, 0x0000, 0x0080, 0x0000), // Bit 7 alone must not set SF.
        (0xAAAA, 0x5555, 0xFFFF, 0x0084), // Exercise all result bits.
    ] {
        for initial_flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = source;
            cpu.bx = destination;
            cpu.flags = initial_flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[3] = expected;
            let mut memory = [0x31, 0xC3]; // XOR BX, AX.

            let result = cpu.step(&mut memory).expect("XOR BX, AX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0x31, 0xC3]);
            assert_flags(cpu.flags, initial_flags, flags);
        }
    }
}

#[test]
fn xor_register_encoding_covers_every_source_destination_and_self_xor() {
    let initial = [
        0x1234, 0x5678, 0x9ABC, 0xDEF0, 0x1357, 0x2468, 0xA55A, 0xFFFF,
    ];
    for source in 0..8 {
        for destination in 0..8 {
            let mut cpu = Cpu::new();
            [
                cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
            ] = initial;
            cpu.flags = 0x0FD7;
            let mut expected_registers = registers(&cpu);
            expected_registers[destination] = initial[destination] ^ initial[source];
            let mut memory = [0x31, 0xC0 | ((source as u8) << 3) | destination as u8];
            let original_memory = memory;

            let result = cpu.step(&mut memory).expect("register XOR should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(
                registers(&cpu),
                expected_registers,
                "source={source}, destination={destination}"
            );
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, original_memory);
            if source == destination {
                // Includes MORE's 31 DB: XOR BX, BX.
                assert_flags(cpu.flags, 0x0FD7, 0x0044);
            }
        }
    }
}

#[test]
fn xor_memory_writes_a_little_endian_word_using_the_decoded_segment() {
    // SI=BP=0800. Different DS/SS values expose incorrect segment selection.
    // Addresses are literal expectations, independent of PhysAddr helpers.
    for (instruction, ds, low, high) in [
        (&[0x31, 0x04][..], 0x1000, 0x10800, 0x10801), // [SI], AX.
        (&[0x31, 0x46, 0xFE][..], 0x1000, 0x207FE, 0x207FF), // [BP-2], AX.
        (&[0x31, 0x84, 0x34, 0x12][..], 0x1000, 0x11A34, 0x11A35), // [SI+1234], AX.
        (&[0x31, 0x06, 0xFF, 0x00][..], 0xFFFF, 0x000EF, 0x000F0), // Address wraps at 1 MiB.
        (&[0x31, 0x06, 0x0F, 0x00][..], 0xFFFF, 0xFFFFF, 0x00000), // Word straddles 1 MiB.
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
        memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
        memory[low] = 0x34;
        memory[high] = 0x12; // 1234 XOR A55A = B76E.
        let mut expected_memory = memory.clone();
        expected_memory[low] = 0x6E;
        expected_memory[high] = 0xB7;

        let result = cpu.step(&mut memory).expect("memory XOR should succeed");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert_flags(cpu.flags, 0x0FD7, 0x0080); // Negative, nonzero, low byte has odd parity.
        assert!(
            memory == expected_memory,
            "only the destination word may change: {instruction:02X?}"
        );
    }
}

#[test]
fn xor_out_of_bounds_word_returns_error_without_partial_write_or_flag_changes() {
    for offset in [0x01FFu16, 0x0200] {
        // Missing high byte, then missing both bytes.
        let mut cpu = Cpu::new();
        cpu.ax = 0xFFFF;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x0200];
        memory[..4].copy_from_slice(&[0x31, 0x06, low, high]);
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
