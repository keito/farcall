use super::{Cpu, CpuStepResult};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup(instruction: &[u8], ip: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(0);
    [cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp] = [0xA55A, 3, 0x1234, 0x5678, 0x0800, 0x0900];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.si = 0x2000;
    cpu.di = 0x3000;
    cpu.ip = ip;
    cpu.flags = 0x0BD7;
    let mut memory = vec![0xCC; 0x50000];
    for (index, &byte) in instruction.iter().enumerate() {
        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
    }
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn assert_flags(actual: u16, initial: u16, arithmetic: u16) {
    // CMPS defines every arithmetic flag; DF, IF, TF and other bits survive.
    assert_eq!(actual, (initial & !0x08D5) | arithmetic);
}

#[test]
fn cmpsb_subtracts_ds_si_minus_es_di_without_storing_or_changing_cx() {
    // Source, destination, literal CF/PF/AF/ZF/SF/OF expectations.
    for (source, destination, arithmetic) in [
        (0x42, 0x42, 0x0044),
        (0x00, 0x01, 0x0095), // FF: unsigned borrow and nibble borrow.
        (0x10, 0x01, 0x0014), // 0F: nibble borrow, even parity.
        (0x80, 0x01, 0x0810), // 7F: signed overflow, odd parity.
        (0x7F, 0xFF, 0x0881), // 80: signed overflow and unsigned borrow.
        (0xFF, 0x00, 0x0084), // FF: negative, even parity, no borrow.
        (0x01, 0x00, 0x0000), // Clear previously set arithmetic flags.
    ] {
        for flags in [0, 0x0FD7] {
            for cx in [0, 0x1234] {
                let (mut cpu, mut memory) = setup(&[0xA6], 0x0100);
                cpu.cx = cx;
                cpu.flags = flags;
                memory[0x22000] = source;
                memory[0x43000] = destination;
                let original_memory = memory.clone();
                let mut expected = registers(&cpu);
                expected[6] = if flags & 0x0400 == 0 { 0x2001 } else { 0x1FFF };
                expected[7] = if flags & 0x0400 == 0 { 0x3001 } else { 0x2FFF };

                step(&mut cpu, &mut memory);

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "source={source:02X}, dest={destination:02X}"
                );
                assert_flags(cpu.flags, flags, arithmetic);
                assert_eq!(cpu.ip, 0x0101, "A6 has no encoded operands");
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn cmpsb_wraps_both_indexes_according_to_df() {
    for (flags, si, di, next_si, next_di) in [
        (0x0BD7, 0xFFFFu16, 0xFFFFu16, 0x0000, 0x0000),
        (0x0FD7, 0x0000, 0x0000, 0xFFFF, 0xFFFF),
    ] {
        let (mut cpu, mut memory) = setup(&[0xA6], 0x0100);
        cpu.flags = flags;
        cpu.si = si;
        cpu.di = di;
        memory[0x20000 + usize::from(si)] = 0x10;
        memory[0x40000 + usize::from(di)] = 0x01;
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[6] = next_si;
        expected[7] = next_di;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_flags(cpu.flags, flags, 0x0014);
        assert_eq!(cpu.ip, 0x0101);
        assert!(memory == original_memory);
    }
}

#[test]
fn cmpsb_segment_overrides_change_only_the_si_source() {
    for (prefix, source) in [
        (0x26, 0x42000),
        (0x2E, 0x12000),
        (0x36, 0x32000),
        (0x3E, 0x22000),
    ] {
        let (mut cpu, mut memory) = setup(&[prefix, 0xA6], 0x0100);
        memory[source] = 0x10;
        memory[0x43000] = 0x01; // Destination is always ES:DI.
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[6] = 0x2001;
        expected[7] = 0x3001;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_flags(cpu.flags, 0x0BD7, 0x0014);
        assert_eq!(cpu.ip, 0x0102);
        assert!(memory == original_memory);
    }
}

fn check_early_stop(prefix: u8, source: &[u8], destination: &[u8], arithmetic: u16) {
    // Initial ZF must not prevent the first comparison. Exercise both values
    // and both directions; the second comparison stops each sequence.
    for flags in [0x0B97, 0x0BD7, 0x0F97, 0x0FD7] {
        let (mut cpu, mut memory) = setup(&[prefix, 0xA6], 0x0100);
        cpu.flags = flags;
        let backwards = flags & 0x0400 != 0;
        for (index, (&src, &dst)) in source.iter().zip(destination).enumerate() {
            let si = if backwards {
                0x2000 - index
            } else {
                0x2000 + index
            };
            let di = if backwards {
                0x3000 - index
            } else {
                0x3000 + index
            };
            memory[0x20000 + si] = src;
            memory[0x40000 + di] = dst;
        }
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[1] = 1; // Initial CX=3, two comparisons completed.
        expected[6] = if backwards { 0x1FFE } else { 0x2002 };
        expected[7] = if backwards { 0x2FFE } else { 0x3002 };

        step(&mut cpu, &mut memory);

        assert_eq!(
            registers(&cpu),
            expected,
            "prefix={prefix:02X}, flags={flags:04X}"
        );
        assert_flags(cpu.flags, flags, arithmetic);
        assert_eq!(cpu.ip, 0x0102);
        assert!(memory == original_memory);
    }
}

#[test]
fn repe_cmpsb_stops_after_the_first_mismatch_and_keeps_its_flags() {
    check_early_stop(0xF3, &[0x11, 0x00, 0x33], &[0x11, 0x01, 0x33], 0x0095);
}

#[test]
fn repne_cmpsb_stops_after_the_first_match_and_keeps_its_flags() {
    check_early_stop(0xF2, &[0x10, 0x22, 0x00], &[0x01, 0x22, 0x01], 0x0044);
}

#[test]
fn repeated_cmpsb_is_limited_by_full_cx_instead_of_the_prefix_byte() {
    for prefix in [0xF2, 0xF3] {
        for count in [1u16, 3, 0x0101] {
            let (mut cpu, mut memory) = setup(&[prefix, 0xA6], 0x0100);
            cpu.cx = count;
            memory[0x22000..0x22000 + usize::from(count)].fill(0x10);
            memory[0x43000..0x43000 + usize::from(count)].fill(if prefix == 0xF3 {
                0x10
            } else {
                0x01
            });
            let original_memory = memory.clone();
            let mut expected = registers(&cpu);
            expected[1] = 0;
            expected[6] = 0x2000 + count;
            expected[7] = 0x3000 + count;

            step(&mut cpu, &mut memory);

            assert_eq!(
                registers(&cpu),
                expected,
                "prefix={prefix:02X}, count={count}"
            );
            assert_flags(
                cpu.flags,
                0x0BD7,
                if prefix == 0xF3 { 0x0044 } else { 0x0014 },
            );
            assert_eq!(cpu.ip, 0x0102);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn repeated_cmpsb_with_zero_cx_does_not_read_either_operand_or_change_flags() {
    for prefix in [0xF2, 0xF3] {
        for flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new(0);
            cpu.ds = 0x2000;
            cpu.es = 0x4000;
            cpu.si = 0xFFFF;
            cpu.di = 0xFFFF;
            cpu.flags = flags;
            let original = registers(&cpu);
            // No operand memory exists: CX=0 must skip both reads.
            let instruction = [prefix, 0xA6];
            let mut memory = instruction;

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), original);
            assert_eq!(cpu.flags, flags);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, instruction);
        }
    }
}

#[test]
fn repe_cmpsb_accepts_either_prefix_order_and_does_not_leak_prefixes() {
    for prefixes in [[0xF3, 0x2E], [0x2E, 0xF3]] {
        for ip in [0x0100, 0xFFFE, 0xFFFF] {
            let (mut cpu, mut memory) = setup(&[prefixes[0], prefixes[1], 0xA6, 0xA6], ip);
            memory[0x12000..0x12003].copy_from_slice(&[0x11, 0x22, 0x44]); // CS source
            memory[0x43000..0x43003].copy_from_slice(&[0x11, 0x20, 0x33]); // ES destination
            memory[0x22002] = 0x33; // Next, plain CMPSB uses DS again.
            let original_memory = memory.clone();
            let mut expected = registers(&cpu);
            expected[1] = 1;
            expected[6] = 0x2002;
            expected[7] = 0x3002;

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected);
            assert_flags(cpu.flags, 0x0BD7, 0x0000); // 22-20=02: nonzero, odd parity.
            assert_eq!(cpu.ip, ip.wrapping_add(3));
            expected[6] = 0x2003;
            expected[7] = 0x3003;

            step(&mut cpu, &mut memory);

            assert_eq!(registers(&cpu), expected, "unprefixed CMPSB preserves CX");
            assert_flags(cpu.flags, 0x0BD7, 0x0044);
            assert_eq!(cpu.ip, ip.wrapping_add(4));
            assert!(memory == original_memory);
        }
    }
}
