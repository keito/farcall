use super::{Cpu, CpuStepResult};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup(instruction: &[u8], ip: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new();
    cpu.ax = 0xA55A;
    cpu.cx = 3;
    cpu.dx = 0x1234;
    cpu.bx = 0x5678;
    cpu.sp = 0x0800;
    cpu.bp = 0x0900;
    cpu.si = 0x2000;
    cpu.di = 0x3000;
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = ip;
    cpu.flags = 0x0BD7; // DF clear; other defined flags set.
    let mut memory = vec![0xCC; 0x50000];
    for (index, &byte) in instruction.iter().enumerate() {
        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
    }
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn rep_lodsb_uses_full_cx_and_leaves_the_last_byte_in_al_regardless_of_zf() {
    for (count, forward_si, backward_si) in [
        (1, 0x2001, 0x1FFF),
        (3, 0x2003, 0x1FFD),
        (0x0101, 0x2101, 0x1EFF), // Use all of CX, not just CL.
    ] {
        for (flags, backwards) in [
            (0x0B97, false), // ZF clear, DF clear.
            (0x0BD7, false), // ZF set, DF clear.
            (0x0F97, true),  // ZF clear, DF set.
            (0x0FD7, true),  // ZF set, DF set.
        ] {
            let (mut cpu, mut memory) = setup(&[0xF3, 0xAC], 0x0100);
            cpu.cx = count;
            cpu.flags = flags;
            for index in 0..count {
                let offset = if backwards {
                    0x2000 - index
                } else {
                    0x2000 + index
                };
                memory[0x20000 + usize::from(offset)] =
                    if index == count - 1 { 0x73 } else { 0x11 };
            }
            let original_memory = memory.clone();
            let mut expected = registers(&cpu);
            expected[0] = 0xA573; // AH survives; only the final loaded byte remains.
            expected[1] = 0;
            expected[6] = if backwards { backward_si } else { forward_si };

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected, "CX={count}, flags={flags:04X}");
            assert_eq!(cpu.ip, 0x0102, "decode REP and LODSB only once");
            assert_eq!(cpu.flags, flags);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn rep_lodsb_zero_count_does_not_access_memory_or_change_state() {
    for flags in [0, 0x0FD7] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xA55A;
        cpu.ds = 0x2000;
        cpu.si = 0xFFFF;
        cpu.di = 0x1234;
        cpu.flags = flags;
        let original = registers(&cpu);
        // No source data exists in this slice. CX=0 must skip the load entirely.
        let mut memory = [0xF3, 0xAC];

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), original);
        assert_eq!(cpu.flags, flags);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, [0xF3, 0xAC]);
    }
}

#[test]
fn rep_lodsb_wraps_si_in_both_directions_within_the_source_segment() {
    for (flags, offsets, next_si) in [
        (0x0BD7, [0xFFFFu16, 0x0000, 0x0001], 0x0002),
        (0x0FD7, [0x0000, 0xFFFF, 0xFFFE], 0xFFFD),
    ] {
        let (mut cpu, mut memory) = setup(&[0xF3, 0xAC], 0x0100);
        cpu.si = offsets[0];
        cpu.flags = flags;
        for (offset, value) in offsets.into_iter().zip([0x11, 0x22, 0x73]) {
            memory[0x20000 + usize::from(offset)] = value;
        }
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[0] = 0xA573;
        expected[1] = 0;
        expected[6] = next_si;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.ip, 0x0102);
        assert_eq!(cpu.flags, flags);
        assert!(memory == original_memory);
    }
}

#[test]
fn rep_lodsb_accepts_both_prefix_orders_and_expires_before_the_next_instruction() {
    for (prefix, source) in [
        (0x26, 0x42000),
        (0x2E, 0x12000),
        (0x36, 0x32000),
        (0x3E, 0x22000),
    ] {
        for prefixes in [[0xF3, prefix], [prefix, 0xF3]] {
            for ip in [0x0100, 0xFFFE, 0xFFFF] {
                // The second LODSB must run once even though REP leaves CX=0.
                let instruction = [prefixes[0], prefixes[1], 0xAC, 0xAC];
                let (mut cpu, mut memory) = setup(&instruction, ip);
                memory[source..source + 3].copy_from_slice(&[0x11, 0x22, 0x73]);
                memory[0x22003] = 0x42; // Next unprefixed LODSB must use DS again.
                let original_memory = memory.clone();
                let mut expected = registers(&cpu);
                expected[0] = 0xA573;
                expected[1] = 0;
                expected[6] = 0x2003;

                step(&mut cpu, &mut memory);

                assert_eq!(registers(&cpu), expected, "prefixes={prefixes:02X?}");
                assert_eq!(cpu.ip, ip.wrapping_add(3));
                assert_eq!(cpu.flags, 0x0BD7);
                expected[0] = 0xA542;
                expected[6] = 0x2004;

                step(&mut cpu, &mut memory);

                assert_eq!(registers(&cpu), expected);
                assert_eq!(cpu.ip, ip.wrapping_add(4));
                assert_eq!(cpu.flags, 0x0BD7);
                assert!(memory == original_memory);
            }
        }
    }
}
