use super::{Cpu, CpuStepResult};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

#[test]
fn rep_stosw_stores_separate_words_and_moves_di_by_two_in_both_directions() {
    for (direction, offsets, next_di) in [
        (0, [0x2000u16, 0x2002], 0x2004),
        (0x0400, [0x2000, 0x1FFE], 0x1FFC),
        (0, [0xFFFE, 0x0000], 0x0002),
        (0x0400, [0x0000, 0xFFFE], 0xFFFC),
    ] {
        for zf in [0, 0x0040] {
            // Segment overrides must not redirect the ES destination, in either order.
            for instruction in [&[0xF3, 0xAB][..], &[0x2E, 0xF3, 0xAB], &[0xF3, 0x3E, 0xAB]] {
                let mut cpu = Cpu::new();
                cpu.ax = 0x1234;
                cpu.cx = 2;
                cpu.di = offsets[0];
                [cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si] = [0x1234, 0x5678, 0x0800, 0x0900, 0x3000];
                [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0, 0x1000, 0x2000, 0x3000];
                cpu.flags = 0x0B97 | direction | zf;
                let flags = cpu.flags;
                let mut memory = vec![0xCC; 0x40000];
                memory[..instruction.len()].copy_from_slice(instruction);
                let mut expected_memory = memory.clone();
                for offset in offsets {
                    let address = 0x30000 + usize::from(offset);
                    expected_memory[address..address + 2].copy_from_slice(&[0x34, 0x12]);
                }
                let mut expected = registers(&cpu);
                expected[1] = 0;
                expected[7] = next_di;

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(registers(&cpu), expected);
                assert_eq!(cpu.flags, flags);
                assert_eq!(cpu.ip, instruction.len() as u16);
                assert!(
                    memory == expected_memory,
                    "only two complete, nonoverlapping words should be written"
                );
            }
        }
    }
}

#[test]
fn stosw_without_rep_stores_once_and_preserves_cx() {
    for (flags, next_di) in [(0x0BD7, 0x2002), (0x0FD7, 0x1FFE)] {
        for count in [0, 3] {
            let mut cpu = Cpu::new();
            cpu.ax = 0x1234;
            cpu.cx = count;
            cpu.es = 0x1000;
            cpu.di = 0x2000;
            cpu.flags = flags;
            let mut expected = registers(&cpu);
            expected[7] = next_di;
            let mut memory = vec![0xCC; 0x20000];
            memory[0] = 0xAB;
            let mut expected_memory = memory.clone();
            expected_memory[0x12000..0x12002].copy_from_slice(&[0x34, 0x12]);

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected);
            assert_eq!(cpu.flags, flags);
            assert_eq!(cpu.ip, 1);
            assert!(memory == expected_memory);
        }
    }
}

#[test]
fn rep_stosw_with_zero_cx_skips_the_write_and_preserves_state() {
    for flags in [0, 0x0FD7] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x1234;
        cpu.es = 0x4000;
        cpu.di = 0xFFFF;
        cpu.flags = flags;
        let original = registers(&cpu);
        // The destination is outside this slice: zero repetitions must not access it.
        let mut memory = [0xF3, 0xAB];

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), original);
        assert_eq!(cpu.flags, flags);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [0xF3, 0xAB]);
    }
}
