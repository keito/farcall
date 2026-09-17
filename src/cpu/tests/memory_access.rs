use super::{Cpu, CpuError, CpuStepResult};
use crate::memory::{MemoryError, PhysAddr};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn cpu_for_memory_access() -> Cpu {
    let mut cpu = Cpu::new();
    cpu.ax = 0xA55A;
    cpu.cx = 0x1234;
    cpu.dx = 0x5678;
    cpu.bx = 0x0300;
    cpu.bp = 0x0800;
    cpu.si = 0x0800;
    cpu.di = 0x0500;
    cpu.sp = 0x9000;
    cpu.es = 0xBEEF;
    cpu.ds = 0x1000;
    cpu.ss = 0x2000;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD7;
    cpu
}

#[test]
fn a0_reads_one_byte_from_ds_preserving_ah_flags_and_other_registers() {
    for (ds, offset, physical) in [
        (0x1000, 0x1234u16, 0x11234),
        (0xFFFF, 0x000F, 0xFFFFF), // Last byte on the 20-bit bus.
        (0xFFFF, 0x0020, 0x00010), // Segment plus offset wraps at 1 MiB.
    ] {
        for value in [0x00u8, 0x80, 0xFF] {
            for flags in [0x0000, 0x0FD7] {
                let mut cpu = cpu_for_memory_access();
                cpu.cs = 0x3000;
                cpu.ds = ds;
                cpu.flags = flags;
                let mut expected_registers = registers(&cpu);
                expected_registers[0] = 0xA500 | u16::from(value);
                let [low, high] = offset.to_le_bytes();
                // Keep both the code segment and the source byte accessible.
                let mut memory = vec![0xCC; (physical + 1).max(0x40000)];
                memory[0x30100..0x30103].copy_from_slice(&[0xA0, low, high]);
                memory[physical] = value;
                let original_memory = memory.clone();

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(registers(&cpu), expected_registers);
                assert_eq!(cpu.flags, flags);
                assert_eq!(cpu.ip, 0x0103);
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn a0_fetches_both_offset_bytes_across_ip_wrap() {
    for (ip, low_address, high_address, next_ip) in [
        (0xFFFE, 0x3FFFF, 0x30000, 0x0001),
        (0xFFFF, 0x30000, 0x30001, 0x0002),
    ] {
        let mut cpu = cpu_for_memory_access();
        cpu.cs = 0x3000;
        cpu.ip = ip;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0xA57E;
        let mut memory = vec![0xCC; 0x50000];
        memory[0x30000 + usize::from(ip)] = 0xA0;
        memory[low_address] = 0x34;
        memory[high_address] = 0x12;
        memory[0x11234] = 0x7E;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, next_ip);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
    }
}

#[test]
fn a0_accepts_the_last_available_byte_and_reports_missing_operands() {
    for offset in [0x01FFu16, 0x0200] {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0;
        let mut expected_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0103].copy_from_slice(&[0xA0, low, high]);
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        if offset == 0x01FF {
            assert!(matches!(result.unwrap(), CpuStepResult::Continue));
            expected_registers[0] = 0xA5CC;
            assert_eq!(cpu.ip, 0x0103);
        } else {
            assert!(matches!(
                result,
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ));
        }
        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn accumulator_stores_use_ds_and_preserve_registers_and_flags() {
    for opcode in [0xA2, 0xA3] {
        for (ax, low, high) in [
            (0xABCD, 0xCD, 0xAB),
            (0x0000, 0x00, 0x00),
            (0xFFFF, 0xFF, 0xFF),
        ] {
            for flags in [0x0000, 0x0FD7] {
                let mut cpu = cpu_for_memory_access();
                cpu.cs = 0x3000; // Instruction bytes come from CS, the destination uses DS.
                cpu.ax = ax;
                cpu.flags = flags;
                let original_registers = registers(&cpu);
                let mut memory = vec![0xCC; 0x40000];
                memory[0x30100..0x30103].copy_from_slice(&[opcode, 0x35, 0x12]);
                let mut expected_memory = memory.clone();
                // DS:1235 = 1000:1235 = physical 11235, an odd address.
                expected_memory[0x11235] = low;
                if opcode == 0xA3 {
                    expected_memory[0x11236] = high;
                }

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(registers(&cpu), original_registers);
                assert_eq!(cpu.flags, flags, "MOV must preserve every flag");
                assert_eq!(cpu.ip, 0x0103, "both instructions consume a word offset");
                assert!(
                    memory == expected_memory,
                    "opcode={opcode:02X}, AX={ax:04X}: wrong address, byte order, or write width"
                );
            }
        }
    }
}

#[test]
fn accumulator_stores_fetch_word_offsets_across_ip_wrap() {
    for opcode in [0xA2, 0xA3] {
        for (ip, low_address, high_address, next_ip) in [
            (0xFFFD, 0x3FFFE, 0x3FFFF, 0x0000),
            (0xFFFE, 0x3FFFF, 0x30000, 0x0001),
            (0xFFFF, 0x30000, 0x30001, 0x0002),
        ] {
            let mut cpu = cpu_for_memory_access();
            cpu.cs = 0x3000;
            cpu.ip = ip;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x50000];
            memory[0x30000 + usize::from(ip)] = opcode;
            memory[low_address] = 0x34;
            memory[high_address] = 0x12;
            let mut expected_memory = memory.clone();
            expected_memory[0x11234] = 0x5A;
            if opcode == 0xA3 {
                expected_memory[0x11235] = 0xA5;
            }

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(cpu.ip, next_ip);
            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, 0x0FD7);
            assert!(
                memory == expected_memory,
                "opcode={opcode:02X}, initial IP={ip:04X}"
            );
        }
    }
}

#[test]
fn accumulator_stores_wrap_at_the_twenty_bit_bus_boundary() {
    for opcode in [0xA2, 0xA3] {
        // Literal physical addresses keep expectations independent of PhysAddr.
        for (offset, low_address, high_address) in [
            (0x000Fu16, 0xFFFFF, 0x00000), // A word straddles the 1 MiB boundary.
            (0x0020u16, 0x00010, 0x00011), // The starting address itself wraps.
        ] {
            let mut cpu = cpu_for_memory_access();
            cpu.ds = 0xFFFF;
            let original_registers = registers(&cpu);
            let [low, high] = offset.to_le_bytes();
            let mut memory = vec![0xCC; 0x100000];
            memory[0x0100..0x0103].copy_from_slice(&[opcode, low, high]);
            let mut expected_memory = memory.clone();
            expected_memory[low_address] = 0x5A;
            if opcode == 0xA3 {
                expected_memory[high_address] = 0xA5;
            }

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, 0x0FD7);
            assert_eq!(cpu.ip, 0x0103);
            assert!(
                memory == expected_memory,
                "opcode={opcode:02X}, offset={offset:04X}"
            );
        }
    }
}

#[test]
fn a2_can_write_the_last_available_byte_without_accessing_a_second_byte() {
    let mut cpu = cpu_for_memory_access();
    cpu.ds = 0;
    let original_registers = registers(&cpu);
    let mut memory = vec![0xCC; 0x0200];
    memory[0x0100..0x0103].copy_from_slice(&[0xA2, 0xFF, 0x01]);
    let mut expected_memory = memory.clone();
    expected_memory[0x01FF] = 0x5A;

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));

    assert_eq!(registers(&cpu), original_registers);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(cpu.ip, 0x0103);
    assert_eq!(memory, expected_memory);
}

#[test]
fn accumulator_store_bounds_errors_preserve_memory_registers_and_flags() {
    for (opcode, offset) in [(0xA2, 0x0200u16), (0xA3, 0x01FF), (0xA3, 0x0200)] {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0103].copy_from_slice(&[opcode, low, high]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        // In particular, A3 must not write its low byte when the high byte is missing.
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn c7_writes_every_word_register_and_preserves_flags_and_memory() {
    // r/m field order: AX, CX, DX, BX, SP, BP, SI, DI.
    for destination in 0..8 {
        for immediate in [0x0000u16, 0x0080, 0x1234, 0x8000, 0xFFFF] {
            for flags in [0x0000, 0x0FD7] {
                let mut cpu = cpu_for_memory_access();
                cpu.flags = flags;
                let mut expected_registers = registers(&cpu);
                expected_registers[destination] = immediate;
                let [low, high] = immediate.to_le_bytes();
                let mut memory = vec![0xCC; 0x0200];
                memory[0x0100..0x0104].copy_from_slice(&[
                    0xC7,
                    0xC0 | destination as u8,
                    low,
                    high,
                ]);
                let original_memory = memory.clone();

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    registers(&cpu),
                    expected_registers,
                    "destination={destination}, immediate={immediate:04X}"
                );
                assert_eq!(cpu.flags, flags);
                assert_eq!(cpu.ip, 0x0104);
                assert_eq!(memory, original_memory);
            }
        }
    }
}

#[test]
fn c7_memory_consumes_displacement_before_immediate_and_uses_ds_or_ss() {
    // Each instruction stores ABCD. Destination addresses are literal expectations.
    for (instruction, physical) in [
        (&[0xC7, 0x06, 0x35, 0x12, 0xCD, 0xAB][..], 0x11235), // [DS:1235], odd address.
        (&[0xC7, 0x00, 0xCD, 0xAB][..], 0x10B00),             // [BX+SI], no displacement.
        (&[0xC7, 0x02, 0xCD, 0xAB][..], 0x21000),             // [BP+SI] selects SS.
        (&[0xC7, 0x46, 0xFE, 0xCD, 0xAB][..], 0x207FE),       // [BP-2], signed disp8.
        (&[0xC7, 0x47, 0x7F, 0xCD, 0xAB][..], 0x1037F),       // [BX+127], positive disp8.
        (&[0xC7, 0x80, 0x34, 0x12, 0xCD, 0xAB][..], 0x11D34), // [BX+SI+1234], disp16.
        (&[0xC7, 0x86, 0xFE, 0xFF, 0xCD, 0xAB][..], 0x207FE), // [BP+FFFE], disp16 wraps.
        (&[0xC7, 0x84, 0x00, 0xF8, 0xCD, 0xAB][..], 0x10000), // SI=0800; offset wraps to zero.
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = cpu_for_memory_access();
            cpu.flags = flags;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x30000];
            memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
            let mut expected_memory = memory.clone();
            expected_memory[physical] = 0xCD;
            expected_memory[physical + 1] = 0xAB;

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, flags);
            assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
            assert!(
                memory == expected_memory,
                "instruction={instruction:02X?}: only the destination word may change"
            );
        }
    }
}

#[test]
fn c7_fetches_mod_rm_displacement_and_immediate_across_ip_wrap_using_cs() {
    let instruction = [0xC7, 0x06, 0x35, 0x12, 0xCD, 0xAB]; // MOV word [DS:1235], ABCD.
    for (ip, next_ip) in [
        (0xFFFA, 0x0000),
        (0xFFFB, 0x0001),
        (0xFFFC, 0x0002),
        (0xFFFD, 0x0003),
        (0xFFFE, 0x0004),
        (0xFFFF, 0x0005),
    ] {
        let mut cpu = cpu_for_memory_access();
        cpu.cs = 0x3000;
        cpu.ip = ip;
        let original_registers = registers(&cpu);
        let mut memory = vec![0xCC; 0x50000];
        let before_wrap = 0x10000 - usize::from(ip);
        memory[0x30000 + usize::from(ip)..0x40000].copy_from_slice(&instruction[..before_wrap]);
        memory[0x30000..0x30000 + instruction.len() - before_wrap]
            .copy_from_slice(&instruction[before_wrap..]);
        let mut expected_memory = memory.clone();
        expected_memory[0x11235] = 0xCD;
        expected_memory[0x11236] = 0xAB;

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(cpu.ip, next_ip);
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == expected_memory, "initial IP={ip:04X}");
    }
}

#[test]
fn c7_memory_destination_wraps_at_the_twenty_bit_bus_boundary() {
    for (offset, low_address, high_address) in [
        (0x000Fu16, 0xFFFFF, 0x00000), // The word straddles 1 MiB.
        (0x0020u16, 0x00010, 0x00011), // The starting address wraps.
    ] {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0xFFFF;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x100000];
        memory[0x0100..0x0106].copy_from_slice(&[0xC7, 0x06, low, high, 0x34, 0x12]);
        let mut expected_memory = memory.clone();
        expected_memory[low_address] = 0x34;
        expected_memory[high_address] = 0x12;

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 0x0106);
        assert!(memory == expected_memory, "offset={offset:04X}");
    }
}

#[test]
fn c7_rejects_nonzero_opcode_extensions_without_writing() {
    for extension in 1..=7 {
        // Valid register, direct memory, disp8, and disp16 destinations expose
        // accidentally treating the reg field as a source register.
        for operand in [0xC0, 0x06, 0x46, 0x86] {
            let mut cpu = cpu_for_memory_access();
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x30000];
            memory[0x0100..0x0106].copy_from_slice(&[0xC7, operand | (extension << 3), 0, 0, 0, 0]);
            let original_memory = memory.clone();

            assert!(
                matches!(cpu.step(&mut memory), Err(CpuError::UnsupportedRegisterField(_, actual)) if actual == extension)
            );

            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, 0x0FD7);
            assert!(
                memory == original_memory,
                "extension={extension}, operand={operand:02X}"
            );
        }
    }
}

#[test]
fn c7_bounds_errors_preserve_registers_flags_and_both_destination_bytes() {
    for offset in [0x01FFu16, 0x0200] {
        // First the high byte is missing, then both destination bytes are missing.
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0106].copy_from_slice(&[0xC7, 0x06, low, high, 0x34, 0x12]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(
            memory, original_memory,
            "a missing high byte must not cause a partial write"
        );
    }
}

#[test]
fn c6_writes_each_byte_register_preserving_its_other_half_and_flags() {
    // Register field order: AL, CL, DL, BL, AH, CH, DH, BH.
    let expected_words = [
        0xA5E7, 0x12E7, 0x56E7, 0x03E7, 0xE75A, 0xE734, 0xE778, 0xE700,
    ];
    for (destination, expected_word) in expected_words.into_iter().enumerate() {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = cpu_for_memory_access();
            cpu.flags = flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[destination % 4] = expected_word;
            let mut memory = vec![0xCC; 0x0200];
            memory[0x0100..0x0103].copy_from_slice(&[0xC6, 0xC0 | destination as u8, 0xE7]);
            let original_memory = memory.clone();

            let result = cpu.step(&mut memory).expect("MOV r8, imm8 should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(
                registers(&cpu),
                expected_registers,
                "destination={destination}"
            );
            assert_eq!(cpu.ip, 0x0103);
            assert_eq!(cpu.flags, flags);
            assert_eq!(memory, original_memory);
        }
    }
}

#[test]
fn c6_memory_consumes_displacement_before_immediate_and_uses_default_segment() {
    for (instruction, physical) in [
        (&[0xC6, 0x06, 0xE5, 0x01, 25][..], 0x101E5), // MORE: [DS:01E5].
        (&[0xC6, 0x04, 0x80][..], 0x10800),           // [SI], no displacement.
        (&[0xC6, 0x46, 0xFE, 0x00][..], 0x207FE),     // [BP-2], signed disp8, SS.
        (&[0xC6, 0x80, 0x34, 0x12, 0xFF][..], 0x11D34), // [BX+SI+1234], disp16.
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = cpu_for_memory_access();
            cpu.flags = flags;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x30000];
            memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
            let mut expected_memory = memory.clone();
            expected_memory[physical] = *instruction.last().unwrap();

            let result = cpu.step(&mut memory).expect("MOV m8, imm8 should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
            assert_eq!(cpu.flags, flags);
            assert!(
                memory == expected_memory,
                "instruction={instruction:02X?}: only the destination byte should change"
            );
        }
    }
}

#[test]
fn c6_rejects_nonzero_opcode_extensions_without_writing() {
    for extension in 1..=7 {
        let mut cpu = cpu_for_memory_access();
        let original_registers = registers(&cpu);
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0103].copy_from_slice(&[0xC6, 0xC0 | (extension << 3), 0xE7]);
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        assert!(
            matches!(
                result,
                Err(CpuError::UnsupportedRegisterField(_, actual)) if actual == extension
            ),
            "must report the rejected extension {extension}"
        );
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}

fn assert_access(mut cpu: Cpu, opcode: u8, operand: &[u8], low: usize, high: usize) {
    let mut memory = vec![0xCC; 0x100000];
    memory[0x0100] = opcode;
    memory[0x0101..0x0101 + operand.len()].copy_from_slice(operand);
    memory[low] = 0x73;
    memory[high] = 0xE2;
    let mut expected_memory = memory.clone();
    let mut expected_registers = registers(&cpu);
    match opcode {
        0x8A => expected_registers[0] = 0xA573, // MOV AL, memory: preserve AH.
        0x88 => expected_memory[low] = 0x5A,    // MOV memory, AL: one byte only.
        0x8E => expected_registers[8] = 0xE273, // MOV ES, memory: little endian.
        0x8C => {
            expected_memory[low] = 0xEF; // MOV memory, ES: little endian.
            expected_memory[high] = 0xBE;
        }
        _ => unreachable!(),
    }

    let result = cpu
        .step(&mut memory)
        .unwrap_or_else(|error| panic!("opcode={opcode:02X}, operand={operand:02X?}: {error}"));

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(
        registers(&cpu),
        expected_registers,
        "only the destination changes"
    );
    assert_eq!(cpu.ip, 0x0101 + operand.len() as u16);
    assert_eq!(cpu.flags, 0x0FD7);
    // Avoid dumping two entire 1 MiB arrays on failure.
    assert!(
        memory == expected_memory,
        "incorrect write location, byte order, or width"
    );
}

fn assert_segment_selection(opcode: u8) {
    // SI and BP contain the same offset; DS and SS differ. Literal physical
    // addresses ensure these assertions do not reuse PhysAddr's calculation.
    for (operand, physical) in [
        (&[0x04][..], 0x10800),             // [SI] -> DS.
        (&[0x01][..], 0x10800),             // [BX+DI] -> DS.
        (&[0x46, 0x00][..], 0x20800),       // [BP+0] -> SS.
        (&[0x02][..], 0x21000),             // [BP+SI] -> SS.
        (&[0x06, 0x00, 0x08][..], 0x10800), // Direct [0800h] -> DS, not SS.
    ] {
        assert_access(
            cpu_for_memory_access(),
            opcode,
            operand,
            physical,
            physical + 1,
        );
    }
}

fn assert_bus_boundary(opcode: u8) {
    for (offset, low, high) in [
        (0x000E, 0xFFFFE, 0xFFFFF),
        (0x000F, 0xFFFFF, 0x00000), // Word's second byte crosses the bus boundary.
        (0x0010, 0x00000, 0x00001), // Segment + offset itself wraps to zero.
    ] {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0xFFFF;
        cpu.si = offset;
        assert_access(cpu, opcode, &[0x04], low, high);
    }
}

fn assert_bounds_error(opcode: u8) {
    let offsets: &[u16] = if matches!(opcode, 0x8C | 0x8E) {
        &[0x0100, 0x00FF] // Entire word missing, then only its high byte missing.
    } else {
        &[0x0100]
    };
    for &offset in offsets {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0x0010;
        cpu.si = offset;
        let original_registers = registers(&cpu);
        // DS:0100 -> physical 0200, just outside the host buffer.
        // DS:00FF -> physical 01FF, only one byte remains.
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0102].copy_from_slice(&[opcode, 0x04]);
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        assert!(
            matches!(
                result,
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ),
            "opcode={opcode:02X}, offset={offset:04X}: bounds must use the physical address"
        );
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(
            memory, original_memory,
            "failed writes must not partially modify memory"
        );
    }
}

// Separate reads/writes and boundary cases so one unfinished operation does
// not hide results for the others. Each test executes a bounded CPU step.
macro_rules! access_tests {
    ($name:ident, $opcode:expr) => {
        mod $name {
            #[test]
            fn selects_ds_or_ss_and_preserves_unrelated_state() {
                super::assert_segment_selection($opcode);
            }

            #[test]
            fn handles_the_20_bit_bus_boundary() {
                super::assert_bus_boundary($opcode);
            }

            #[test]
            fn bounds_errors_preserve_registers_and_memory() {
                super::assert_bounds_error($opcode);
            }
        }
    };
}

access_tests!(byte_read, 0x8A);
access_tests!(byte_write, 0x88);
access_tests!(word_read, 0x8E);
access_tests!(word_write, 0x8C);

#[test]
fn physical_address_translation_widens_wraps_and_allows_aliases() {
    for (segment, offset, expected) in [
        (0x0000, 0x1234, 0x01234),
        (0x1234, 0x0056, 0x12396),
        (0x1230, 0x0096, 0x12396), // Different segment:offset, same physical byte.
        (0xFFFF, 0x000F, 0xFFFFF),
        (0xFFFF, 0x0010, 0x00000),
        (0xFFFF, 0xFFFF, 0x0FFEF),
    ] {
        assert_eq!(PhysAddr::new(segment, offset).as_usize(), expected);
    }
}
