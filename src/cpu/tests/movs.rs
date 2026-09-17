use super::{Cpu, CpuStepResult};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup(instruction: &[u8]) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new();
    [cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp] = [0xA55A, 2, 0x1234, 0x5678, 0x0800, 0x0900];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.si = 0x2000;
    cpu.di = 0x3000;
    cpu.ip = 0x0100;
    cpu.flags = 0x0BD7;
    let mut memory = vec![0xCC; 0x50000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn movsw_copies_one_word_in_either_direction_without_changing_ax_cx_or_flags() {
    for (flags, next_si, next_di) in [(0x0BD7, 0x2002, 0x3002), (0x0FD7, 0x1FFE, 0x2FFE)] {
        for count in [0, 3] {
            let (mut cpu, mut memory) = setup(&[0xA5]);
            cpu.cx = count;
            cpu.flags = flags;
            memory[0x22000..0x22002].copy_from_slice(&[0x34, 0x12]);
            let mut expected_memory = memory.clone();
            expected_memory[0x43000..0x43002].copy_from_slice(&[0x34, 0x12]);
            let mut expected = registers(&cpu);
            expected[6] = next_si;
            expected[7] = next_di;

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected);
            assert_eq!(cpu.flags, flags);
            assert_eq!(cpu.ip, 0x0101);
            assert!(memory == expected_memory);
        }
    }
}

#[test]
fn rep_movsw_copies_full_cx_words_in_both_directions_regardless_of_zf() {
    for count in [2u16, 0x0101] {
        for flags in [0x0B97, 0x0BD7, 0x0F97, 0x0FD7] {
            let (mut cpu, mut memory) = setup(&[0xF3, 0xA5]);
            cpu.cx = count;
            cpu.flags = flags;
            let backwards = flags & 0x0400 != 0;
            let mut expected_memory = memory.clone();
            for index in 0..usize::from(count) {
                let source = if backwards {
                    0x22000 - 2 * index
                } else {
                    0x22000 + 2 * index
                };
                let dest = if backwards {
                    0x43000 - 2 * index
                } else {
                    0x43000 + 2 * index
                };
                let bytes = [index as u8, 0xA5];
                memory[source..source + 2].copy_from_slice(&bytes);
                expected_memory[source..source + 2].copy_from_slice(&bytes);
                expected_memory[dest..dest + 2].copy_from_slice(&bytes);
            }
            let mut expected = registers(&cpu);
            expected[1] = 0;
            expected[6] = if backwards {
                0x2000 - 2 * count
            } else {
                0x2000 + 2 * count
            };
            expected[7] = if backwards {
                0x3000 - 2 * count
            } else {
                0x3000 + 2 * count
            };

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected);
            assert_eq!(cpu.flags, flags);
            assert_eq!(cpu.ip, 0x0102);
            assert!(memory == expected_memory, "CX={count}, flags={flags:04X}");
        }
    }
}

#[test]
fn rep_movsw_wraps_both_indexes_within_their_segments() {
    for (flags, offsets, next_index) in [
        (0x0BD7, [0xFFFEu16, 0x0000], 0x0002),
        (0x0FD7, [0x0000, 0xFFFE], 0xFFFC),
    ] {
        let (mut cpu, mut memory) = setup(&[0xF3, 0xA5]);
        cpu.si = offsets[0];
        cpu.di = offsets[0];
        cpu.flags = flags;
        for (offset, bytes) in offsets.into_iter().zip([[0x34, 0x12], [0xCD, 0xAB]]) {
            let source = 0x20000 + usize::from(offset);
            memory[source..source + 2].copy_from_slice(&bytes);
        }
        let mut expected_memory = memory.clone();
        for (offset, bytes) in offsets.into_iter().zip([[0x34, 0x12], [0xCD, 0xAB]]) {
            let dest = 0x40000 + usize::from(offset);
            expected_memory[dest..dest + 2].copy_from_slice(&bytes);
        }
        let mut expected = registers(&cpu);
        expected[1] = 0;
        expected[6] = next_index;
        expected[7] = next_index;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.flags, flags);
        assert_eq!(cpu.ip, 0x0102);
        assert!(memory == expected_memory);
    }
}

#[test]
fn rep_movsw_segment_prefixes_override_only_the_source_in_either_order() {
    for (prefix, source) in [
        (0x26, 0x42000),
        (0x2E, 0x12000),
        (0x36, 0x32000),
        (0x3E, 0x22000),
    ] {
        for prefixes in [[0xF3, prefix], [prefix, 0xF3]] {
            let (mut cpu, mut memory) = setup(&[prefixes[0], prefixes[1], 0xA5, 0xA5]);
            memory[source..source + 4].copy_from_slice(&[0x34, 0x12, 0xCD, 0xAB]);
            memory[0x22004..0x22006].copy_from_slice(&[0x78, 0x56]);
            let mut expected_memory = memory.clone();
            expected_memory[0x43000..0x43004].copy_from_slice(&[0x34, 0x12, 0xCD, 0xAB]);
            let mut expected = registers(&cpu);
            expected[1] = 0;
            expected[6] = 0x2004;
            expected[7] = 0x3004;

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected);
            assert_eq!(cpu.flags, 0x0BD7);
            assert_eq!(cpu.ip, 0x0103);
            assert!(memory == expected_memory);

            // Prefixes expire: copy once from DS even though REP left CX=0.
            expected_memory[0x43004..0x43006].copy_from_slice(&[0x78, 0x56]);
            expected[6] = 0x2006;
            expected[7] = 0x3006;
            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected);
            assert_eq!(cpu.flags, 0x0BD7);
            assert_eq!(cpu.ip, 0x0104);
            assert!(memory == expected_memory);
        }
    }
}

#[test]
fn rep_movsw_zero_count_does_not_access_either_operand() {
    for flags in [0, 0x0FD7] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xA55A;
        [cpu.ds, cpu.es, cpu.si, cpu.di] = [0x2000, 0x4000, 0xFFFF, 0xFFFF];
        cpu.flags = flags;
        let original = registers(&cpu);
        // Neither operand is inside this slice, but CX=0 must skip both accesses.
        let mut memory = [0xF3, 0xA5];

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), original);
        assert_eq!(cpu.flags, flags);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [0xF3, 0xA5]);
    }
}

#[test]
fn rep_movsw_reads_each_word_after_the_previous_write_when_ranges_overlap() {
    for (flags, si, di, next_si, next_di, bytes) in [
        (0x0BD7, 0x2000, 0x2002, 0x2006, 0x2008, [0x11, 0x12]),
        (0x0FD7, 0x2006, 0x2004, 0x2000, 0x1FFE, [0x41, 0x42]),
    ] {
        let (mut cpu, mut memory) = setup(&[0xF3, 0xA5]);
        cpu.es = cpu.ds;
        cpu.cx = 3;
        cpu.si = si;
        cpu.di = di;
        cpu.flags = flags;
        memory[0x22000..0x22008].copy_from_slice(&[0x11, 0x12, 0x21, 0x22, 0x31, 0x32, 0x41, 0x42]);
        let mut expected_memory = memory.clone();
        // Each write overwrites the next source, propagating one word through all four.
        expected_memory[0x22000..0x22008].copy_from_slice(&bytes.repeat(4));
        let mut expected = registers(&cpu);
        expected[1] = 0;
        expected[6] = next_si;
        expected[7] = next_di;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.flags, flags);
        assert_eq!(cpu.ip, 0x0102);
        assert!(memory == expected_memory);
    }
}
