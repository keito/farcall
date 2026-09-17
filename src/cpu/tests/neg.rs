use super::{Cpu, CpuError, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

#[test]
fn neg_word_updates_every_register_and_all_arithmetic_flags() {
    // Operand, result, literal CF/PF/AF/ZF/SF/OF bits.
    let cases = [
        (0x0000, 0x0000, 0x0044), // Only zero clears CF; AF and OF clear too.
        (0x0001, 0xFFFF, 0x0095),
        (0x0002, 0xFFFE, 0x0091), // Odd low-byte parity.
        (0x0010, 0xFFF0, 0x0085), // No borrow across the low nibble.
        (0x0100, 0xFF00, 0x0085), // PF uses the low byte, not the whole word.
        (0x7FFF, 0x8001, 0x0091),
        (0x8000, 0x8000, 0x0885), // -32768 cannot become +32768: OF is set.
        (0x8001, 0x7FFF, 0x0015),
        (0xFFF0, 0x0010, 0x0001),
        (0xFFFF, 0x0001, 0x0011),
    ];
    for reg in 0..8 {
        for (operand, result, arithmetic_flags) in cases {
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
                let instruction = [0xF7, 0xD8 | reg as u8]; // NEG r16, no immediate.
                let mut memory = instruction;

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "reg={reg}, operand={operand:04X}"
                );
                assert_eq!(cpu.flags, (flags & !0x08D5) | arithmetic_flags);
                assert_eq!(cpu.ip, 2);
                assert_eq!(memory, instruction);
            }
        }
    }
}

#[test]
fn neg_word_memory_uses_default_and_overridden_segments_and_writes_only_two_bytes() {
    for (operand, offset, default_base) in [
        (&[0x18][..], 0x0240, 0x20000),             // BX+SI.
        (&[0x1A][..], 0x0340, 0x30000),             // BP+SI uses SS.
        (&[0x1E, 0x35, 0x12][..], 0x1235, 0x20000), // Direct disp16.
        (&[0x5E, 0xFE][..], 0x02FE, 0x30000),       // BP-2: signed disp8.
        (&[0x9E, 0xFE, 0xFF][..], 0x02FE, 0x30000), // BP+FFFE: disp16.
        (&[0x9C, 0xC0, 0xFF][..], 0x0000, 0x20000), // SI+FFC0 wraps.
    ] {
        for (prefix, base) in [
            (None, default_base),
            (Some(0x26), 0x40000),
            (Some(0x2E), 0x10000),
            (Some(0x36), 0x30000),
            (Some(0x3E), 0x20000),
        ] {
            let mut cpu = Cpu::new();
            [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
            [cpu.bx, cpu.bp, cpu.si] = [0x0200, 0x0300, 0x0040];
            cpu.ax = 0xA55A;
            cpu.ip = 0x0100;
            cpu.flags = 0x0FD7;
            let original_registers = registers(&cpu);
            let mut instruction: Vec<u8> = prefix.into_iter().collect();
            instruction.push(0xF7);
            instruction.extend_from_slice(operand);
            let mut memory = vec![0xCC; 0x50000];
            memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
            memory[base + offset..base + offset + 2].copy_from_slice(&[1, 0]);
            let mut expected_memory = memory.clone();
            expected_memory[base + offset..base + offset + 2].copy_from_slice(&[0xFF, 0xFF]);

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, (0x0FD7 & !0x08D5) | 0x0095);
            assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
            assert!(memory == expected_memory, "instruction={instruction:02X?}");
        }
    }
}

#[test]
fn neg_word_reads_and_writes_across_the_twenty_bit_bus_boundary() {
    let mut cpu = Cpu::new();
    cpu.ds = 0xFFFF;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD7;
    let original_registers = registers(&cpu);
    let mut memory = vec![0xCC; 0x100000];
    memory[0x100..0x104].copy_from_slice(&[0xF7, 0x1E, 0x0F, 0]);
    memory[0xFFFFF] = 1;
    memory[0] = 0;
    let mut expected_memory = memory.clone();
    expected_memory[0xFFFFF] = 0xFF;
    expected_memory[0] = 0xFF;

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));

    assert_eq!(registers(&cpu), original_registers);
    assert_eq!(cpu.flags, (0x0FD7 & !0x08D5) | 0x0095);
    assert_eq!(cpu.ip, 0x0104);
    assert!(memory == expected_memory);
}

#[test]
fn neg_word_fetches_prefix_opcode_mod_rm_and_displacement_across_ip_wrap() {
    let instruction = [0x26, 0xF7, 0x1E, 0x34, 0x12]; // NEG word [ES:1234].
    for ip in 0xFFFBu16..=0xFFFF {
        let mut cpu = Cpu::new();
        cpu.cs = 0x3000;
        cpu.es = 0x1000;
        cpu.ip = ip;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let mut memory = vec![0xCC; 0x40000];
        for (index, byte) in instruction.into_iter().enumerate() {
            memory[0x30000 + usize::from(ip.wrapping_add(index as u16))] = byte;
        }
        memory[0x11234..0x11236].copy_from_slice(&[0, 0]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.ip, ip.wrapping_add(5));
        assert_eq!(cpu.flags, (0x0FD7 & !0x08D5) | 0x0044);
        assert!(memory == original_memory);
    }
}

#[test]
fn neg_word_memory_errors_preserve_registers_flags_and_memory() {
    for offset in [0x01FFu16, 0x0200] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x1234;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x0200];
        memory[..4].copy_from_slice(&[0xF7, 0x1E, low, high]);
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
