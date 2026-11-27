use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup(instruction: &[u8], ip: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [0xA510, 3, 0x1234, 0x5678, 0x0800, 0x0900, 0x2000, 0x3000];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = ip;
    cpu.flags = 0xFBD7;
    let mut memory = vec![0xCC; 0x60000];
    for (index, &byte) in instruction.iter().enumerate() {
        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
    }
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn assert_flags(cpu: &Cpu, initial: u16, arithmetic: u16) {
    assert_eq!(cpu.flags, (initial & !0x08D5) | arithmetic);
}

fn put_operand(memory: &mut [u8], opcode: u8, address: usize, value: u16) {
    let [low, high] = value.to_le_bytes();
    memory[address] = low;
    if opcode == 0xAF {
        memory[(address + 1) & 0xFFFFF] = high;
    }
}

fn check_single_comparisons(opcode: u8, cases: &[(u16, u16, u16)]) {
    let width = if opcode == 0xAE { 1 } else { 2 };
    for &(accumulator, operand, arithmetic) in cases {
        for flags in [0xF002, 0xFBD7, 0xF402, 0xFFD7] {
            for cx in [0, 0x1234] {
                let (mut cpu, mut memory) = setup(&[opcode], 0x0100);
                cpu.ax = if opcode == 0xAE {
                    0xA500 | accumulator
                } else {
                    accumulator
                };
                cpu.cx = cx;
                cpu.flags = flags;
                put_operand(&mut memory, opcode, 0x43000, operand);
                let original_memory = memory.clone();
                let mut expected = registers(&cpu);
                expected[7] = if flags & 0x0400 == 0 {
                    0x3000 + width
                } else {
                    0x3000 - width
                };

                step(&mut cpu, &mut memory);

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "opcode={opcode:02X}, AX={accumulator:04X}, operand={operand:04X}"
                );
                assert_flags(&cpu, flags, arithmetic);
                assert_eq!(cpu.ip, 0x0101, "SCAS has no encoded operands");
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn scasb_compares_al_minus_memory_and_preserves_ax_si_cx_and_memory() {
    check_single_comparisons(
        0xAE,
        &[
            (0x42, 0x42, 0x0044),
            (0x00, 0x01, 0x0095), // FF: unsigned borrow, nibble borrow, sign, parity.
            (0x10, 0x01, 0x0014),
            (0x80, 0x01, 0x0810), // Signed overflow without unsigned borrow.
            (0x7F, 0xFF, 0x0881), // Signed overflow with unsigned borrow.
            (0xFF, 0x00, 0x0084),
            (0x01, 0x00, 0x0000), // Clear previous arithmetic flags; ignore incoming CF.
        ],
    );
}

#[test]
fn scasw_compares_the_full_ax_and_uses_low_byte_parity() {
    check_single_comparisons(
        0xAF,
        &[
            (0x1234, 0x1234, 0x0044),
            (0x0000, 0x0001, 0x0095),
            (0x0010, 0x0001, 0x0014),
            (0x8000, 0x0001, 0x0814),
            (0x7FFF, 0xFFFF, 0x0885),
            (0xFFFF, 0x0000, 0x0084),
            (0x0001, 0x0000, 0x0000),
            (0x0100, 0x0000, 0x0004), // Low bytes equal, full words unequal; even low-byte parity.
            (0x0000, 0xFFFF, 0x0011),
        ],
    );
}

#[test]
fn scas_wraps_di_in_both_directions_and_reads_unaligned_words() {
    for (opcode, backwards, di, next_di) in [
        (0xAE, false, 0xFFFF, 0x0000),
        (0xAE, true, 0x0000, 0xFFFF),
        (0xAF, false, 0xFFFE, 0x0000),
        (0xAF, false, 0xFFFF, 0x0001),
        (0xAF, true, 0x0000, 0xFFFE),
        (0xAF, true, 0x0001, 0xFFFF),
    ] {
        let (mut cpu, mut memory) = setup(&[opcode], 0x0100);
        cpu.ax = 0x0010;
        cpu.di = di;
        cpu.flags = if backwards { 0xFFD7 } else { 0xFBD7 };
        let initial_flags = cpu.flags;
        // The high byte wraps to ES:0000 when the word starts at ES:FFFF.
        memory[0x40000 + usize::from(di)] = 1;
        if opcode == 0xAF {
            memory[0x40000 + usize::from(di.wrapping_add(1))] = 0;
        }
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[7] = next_di;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_flags(&cpu, initial_flags, 0x0014);
        assert!(memory == original_memory);
    }
}

#[test]
fn scas_uses_es_even_with_segment_override_prefixes() {
    for opcode in [0xAE, 0xAF] {
        for prefix in [0x26, 0x2E, 0x36, 0x3E] {
            let (mut cpu, mut memory) = setup(&[prefix, opcode], 0x0100);
            cpu.ax = 0x1234;
            put_operand(&mut memory, opcode, 0x43000, 0x1234);
            // Other segments and ES:SI still contain CC, which cannot compare equal.
            let original_memory = memory.clone();
            let mut expected = registers(&cpu);
            expected[7] = if opcode == 0xAE { 0x3001 } else { 0x3002 };

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected);
            assert_flags(&cpu, 0xFBD7, 0x0044);
            assert_eq!(cpu.ip, 0x0102);
            assert!(memory == original_memory);
        }
    }
}

fn check_repeat_stops(prefix: u8) {
    for opcode in [0xAE, 0xAF] {
        let width = if opcode == 0xAE { 1u16 } else { 2 };
        for stop_at in [0u16, 1, 2] {
            for flags in [0xFB97, 0xFBD7, 0xFF97, 0xFFD7] {
                let (mut cpu, mut memory) = setup(&[prefix, opcode], 0x0100);
                cpu.ax = 0x0010;
                cpu.flags = flags;
                let backwards = flags & 0x0400 != 0;
                for index in 0..3 {
                    let equal = if prefix == 0xF2 {
                        index == stop_at
                    } else {
                        index != stop_at
                    };
                    let offset = if backwards {
                        0x3000 - index * width
                    } else {
                        0x3000 + index * width
                    };
                    put_operand(
                        &mut memory,
                        opcode,
                        0x40000 + usize::from(offset),
                        if equal { 0x10 } else { 0x11 },
                    );
                }
                let original_memory = memory.clone();
                let mut expected = registers(&cpu);
                expected[1] = 2 - stop_at;
                expected[7] = if backwards {
                    0x3000 - (stop_at + 1) * width
                } else {
                    0x3000 + (stop_at + 1) * width
                };

                step(&mut cpu, &mut memory);

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "opcode={opcode:02X}, prefix={prefix:02X}, stop={stop_at}, flags={flags:04X}"
                );
                assert_flags(&cpu, flags, if prefix == 0xF2 { 0x0044 } else { 0x0095 });
                assert_eq!(cpu.ip, 0x0102);
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn repne_scas_stops_on_first_match_including_the_last_allowed_element() {
    check_repeat_stops(0xF2);
}

#[test]
fn repe_scas_stops_on_first_mismatch_including_the_last_allowed_element() {
    check_repeat_stops(0xF3);
}

#[test]
fn repeated_scas_exhausts_full_cx_and_wraps_di_between_elements() {
    for opcode in [0xAE, 0xAF] {
        let width = if opcode == 0xAE { 1u16 } else { 2 };
        for prefix in [0xF2, 0xF3] {
            for count in [1u16, 3, 0x0101] {
                for backwards in [false, true] {
                    let (mut cpu, mut memory) = setup(&[prefix, opcode], 0x0100);
                    cpu.ax = 0x0010;
                    cpu.cx = count;
                    cpu.di = if backwards { 0 } else { 0xFFFE };
                    cpu.flags = if backwards { 0xFFD7 } else { 0xFBD7 };
                    let initial_flags = cpu.flags;
                    let mut offset = cpu.di;
                    for _ in 0..count {
                        put_operand(
                            &mut memory,
                            opcode,
                            0x40000 + usize::from(offset),
                            if prefix == 0xF3 { 0x10 } else { 1 },
                        );
                        offset = if backwards {
                            offset.wrapping_sub(width)
                        } else {
                            offset.wrapping_add(width)
                        };
                    }
                    let original_memory = memory.clone();
                    let mut expected = registers(&cpu);
                    expected[1] = 0;
                    expected[7] = if backwards {
                        0u16.wrapping_sub(count * width)
                    } else {
                        0xFFFEu16.wrapping_add(count * width)
                    };

                    step(&mut cpu, &mut memory);

                    assert_eq!(registers(&cpu), expected);
                    assert_flags(
                        &cpu,
                        initial_flags,
                        if prefix == 0xF3 { 0x0044 } else { 0x0014 },
                    );
                    assert_eq!(cpu.ip, 0x0102);
                    assert!(memory == original_memory);
                }
            }
        }
    }
}

#[test]
fn zero_count_repeated_scas_skips_memory_access_and_preserves_all_flags() {
    for opcode in [0xAE, 0xAF] {
        for prefix in [0xF2, 0xF3] {
            for flags in [0xF002, 0xFFD7] {
                let mut cpu = Cpu::new(CpuModel::I8088, 0);
                cpu.ax = 0x1234;
                cpu.es = 0xFFFF;
                cpu.di = 0xFFFF;
                cpu.si = 0xABCD;
                cpu.flags = flags;
                let original = registers(&cpu);
                let mut memory = [prefix, opcode]; // ES:DI is outside this memory.
                let original_memory = memory;

                step(&mut cpu, &mut memory);

                assert_eq!(registers(&cpu), original);
                assert_eq!(cpu.flags, flags);
                assert_eq!(cpu.ip, 2);
                assert_eq!(memory, original_memory);
            }
        }
    }
}

#[test]
fn scas_accepts_prefix_orders_across_ip_wrap_and_repeat_expires_after_one_instruction() {
    for opcode in [0xAE, 0xAF] {
        let width = if opcode == 0xAE { 1 } else { 2 };
        for prefix in [0xF2, 0xF3] {
            for prefixes in [[prefix, 0x2E], [0x2E, prefix]] {
                for ip in [0xFFFD, 0xFFFE, 0xFFFF] {
                    let (mut cpu, mut memory) =
                        setup(&[prefixes[0], prefixes[1], opcode, opcode], ip);
                    cpu.ax = 0x10;
                    cpu.cx = 4;
                    for index in 0..4 {
                        let equal = if prefix == 0xF2 {
                            index % 2 == 1
                        } else {
                            index % 2 == 0
                        };
                        put_operand(
                            &mut memory,
                            opcode,
                            0x43000 + index * width,
                            if equal { 0x10 } else { 0x11 },
                        );
                    }
                    let original_memory = memory.clone();
                    let mut expected = registers(&cpu);
                    expected[1] = 2;
                    expected[7] += 2 * width as u16;

                    step(&mut cpu, &mut memory);

                    assert_eq!(registers(&cpu), expected);
                    assert_eq!(cpu.ip, ip.wrapping_add(3));
                    step(&mut cpu, &mut memory);

                    expected[7] += width as u16;
                    assert_eq!(
                        registers(&cpu),
                        expected,
                        "plain SCAS must leave CX unchanged"
                    );
                    assert_flags(&cpu, 0xFBD7, if prefix == 0xF2 { 0x0095 } else { 0x0044 });
                    assert_eq!(cpu.ip, ip.wrapping_add(4));
                    assert!(memory == original_memory);
                }
            }
        }
    }
}

#[test]
fn scas_reads_across_the_twenty_bit_physical_address_boundary() {
    for opcode in [0xAE, 0xAF] {
        let (mut cpu, mut memory) = setup(&[opcode], 0x0100);
        cpu.es = 0xFFFF;
        cpu.di = 0x000F;
        cpu.ax = 0x1234;
        memory.resize(0x100000, 0xCC);
        memory[0xFFFFF] = 0x34;
        memory[0] = 0x12;
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[7] = if opcode == 0xAE { 0x0010 } else { 0x0011 };

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_flags(&cpu, 0xFBD7, 0x0044);
        assert!(memory == original_memory);
    }
}

#[test]
fn scas_reports_missing_operands_without_changing_flags_or_registers() {
    for (opcode, di) in [(0xAE, 0x0200), (0xAF, 0x01FF), (0xAF, 0x0200)] {
        for prefix in [None, Some(0xF2), Some(0xF3)] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.ax = 0xCC;
            cpu.di = di;
            cpu.cx = 3;
            cpu.flags = 0xFFD7;
            let mut instruction = Vec::new();
            if let Some(prefix) = prefix {
                instruction.push(prefix);
            }
            instruction.push(opcode);
            let mut memory = vec![0xCC; 0x0200];
            memory[..instruction.len()].copy_from_slice(&instruction);
            let original = registers(&cpu);
            let original_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory),
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ));

            assert_eq!(registers(&cpu), original);
            assert_eq!(cpu.flags, 0xFFD7);
            assert_eq!(cpu.ip, instruction.len() as u16);
            assert!(memory == original_memory);
        }
    }
}
