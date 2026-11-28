use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// OUTSW: output a little-endian word from DS:SI (segment overrides allowed),
// advance SI by +/-2, and preserve FLAGS. REP counts words, one event per step.
// On 286 in real mode, SI=FFFFh raises #GP(13) before any transfer.
// https://bitsavers.trailing-edge.com/components/intel/80286/210498-005_80286_and_80287_Programmers_Reference_Manual_1987.pdf
// On 8088, 6F instead aliases 7F (JG: ZF clear and SF=OF).
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn setup(model: CpuModel, instruction: &[u8]) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(model, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA55A, 0x2345, 0x1234, 0x3456, 0x0800, 0x4567, 0x0200, 0x6789,
    ];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = 0x0100;
    let mut memory = vec![0xCC; 0x50000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    (cpu, memory)
}

fn check_write(cpu: &mut Cpu, memory: &mut [u8], word: u16, si: u16, cx: u16, ip: u16) {
    let port = cpu.dx;
    let flags = cpu.flags;
    let mut expected = registers(cpu);
    expected[1] = cx;
    expected[6] = si;
    let original_memory = memory.to_vec();

    let result = cpu
        .step(memory)
        .expect("OUTSW should read an accessible word");

    assert!(
        matches!(result, CpuStepResult::PortWrite16 { port: actual_port, value }
        if actual_port == port && value == word),
        "expected one word event: port={port:04X}, value={word:04X}"
    );
    assert_eq!(
        registers(cpu),
        expected,
        "preserve AX, DX, DI, other registers and segments"
    );
    assert_eq!(cpu.ip, ip);
    assert_eq!(cpu.flags, flags);
    assert!(
        memory == original_memory,
        "OUTSW does not modify its source"
    );
}

#[test]
fn i286_outsw_outputs_little_endian_words_and_moves_si_by_two() {
    for (si, forward, backward) in [
        (0x0200, 0x0202, 0x01FE),
        (0x0000, 0x0002, 0xFFFE),
        (0x0001, 0x0003, 0xFFFF), // Unaligned words are valid.
        (0xFFFD, 0xFFFF, 0xFFFB),
        (0xFFFE, 0x0000, 0xFFFC), // Last complete word; SI increment wraps.
    ] {
        for (bytes, word) in [
            ([0, 0], 0x0000u16),
            ([0x80, 0], 0x0080),
            ([0x34, 0x12], 0x1234),
            ([0, 0x80], 0x8000),
            ([0xFF, 0xFF], 0xFFFF),
        ] {
            for port in [0x0000, 0x0300, 0x1234, 0x8000, 0xFFFF] {
                for (flags, final_si) in [(0x0002, forward), (0x0BD7, forward), (0x0FD7, backward)]
                {
                    let (mut cpu, mut memory) = setup(CpuModel::I80286, &[0x6F]);
                    cpu.si = si;
                    cpu.dx = port;
                    cpu.flags = flags;
                    cpu.cx = 0; // No REP means one transfer even with CX=0.
                    let address = 0x20000 + usize::from(si);
                    memory[address..address + 2].copy_from_slice(&bytes);

                    check_write(&mut cpu, &mut memory, word, final_si, 0, 0x0101);
                }
            }
        }
    }
}

#[test]
fn i286_outsw_honors_all_segment_overrides_and_preserves_nonrep_cx() {
    for (prefixes, word) in [
        (&[][..], 0x1234),
        (&[0x3E][..], 0x1234),
        (&[0x26][..], 0xABCD),
        (&[0x2E][..], 0x5AA5),
        (&[0x36][..], 0x80FF),
        (&[0x26, 0x3E][..], 0x1234),
        (&[0x3E, 0x26][..], 0xABCD),
    ] {
        let instruction: Vec<u8> = prefixes.iter().copied().chain([0x6F]).collect();
        for cx in [0, 1, 0xFFFF] {
            let (mut cpu, mut memory) = setup(CpuModel::I80286, &instruction);
            cpu.cx = cx;
            cpu.flags = 0x0FD7;
            for (address, bytes) in [
                (0x20200, [0x34, 0x12]),
                (0x30200, [0xCD, 0xAB]),
                (0x10200, [0xA5, 0x5A]),
                (0x40200, [0xFF, 0x80]),
            ] {
                memory[address..address + 2].copy_from_slice(&bytes);
            }

            check_write(
                &mut cpu,
                &mut memory,
                word,
                0x01FE,
                cx,
                0x0100 + instruction.len() as u16,
            );
        }
    }
}

#[test]
fn i286_rep_outsw_counts_words_and_restarts_until_the_last_transfer() {
    for (backward, offsets) in [
        (false, [0x0200u16, 0x0202, 0x0204, 0x0206]),
        (true, [0x0200, 0x01FE, 0x01FC, 0x01FA]),
        (false, [0xFFFE, 0x0000, 0x0002, 0x0004]),
        (true, [0x0000, 0xFFFE, 0xFFFC, 0xFFFA]),
    ] {
        for count in [1u16, 3] {
            for (instruction, source_base, words) in [
                (&[0xF3, 0x6F][..], 0x20000, [0x1234u16, 0x8001, 0xFF00]),
                (&[0x26, 0xF3, 0x6F][..], 0x30000, [0xABCD, 0x0080, 0x5AA5]),
                (&[0xF3, 0x26, 0x6F][..], 0x30000, [0xABCD, 0x0080, 0x5AA5]),
            ] {
                // ZF must not terminate REP OUTSW, whether clear or set.
                for other_flags in [0x0002, 0x0BD7] {
                    let (mut cpu, mut memory) = setup(CpuModel::I80286, instruction);
                    cpu.cx = count;
                    cpu.si = offsets[0];
                    cpu.flags = other_flags | if backward { 0x0400 } else { 0 };
                    let next_ip = 0x0100 + instruction.len() as u16;
                    memory[0x10000 + usize::from(next_ip)] = 0x90;
                    for index in 0..3 {
                        let address = source_base + usize::from(offsets[index]);
                        memory[address..address + 2].copy_from_slice(&words[index].to_le_bytes());
                    }
                    for index in 0..usize::from(count) {
                        let remaining = count - index as u16 - 1;
                        check_write(
                            &mut cpu,
                            &mut memory,
                            words[index],
                            offsets[index + 1],
                            remaining,
                            if remaining == 0 { next_ip } else { 0x0100 },
                        );
                    }
                    let saved = registers(&cpu);
                    let flags = cpu.flags;
                    assert!(matches!(
                        cpu.step(&mut memory).unwrap(),
                        CpuStepResult::Continue
                    ));
                    assert_eq!(
                        cpu.ip,
                        next_ip + 1,
                        "execute following NOP without an extra zero-count step"
                    );
                    assert_eq!(registers(&cpu), saved);
                    assert_eq!(cpu.flags, flags);
                }
            }
        }
    }
}

#[test]
fn i286_outsw_rejects_ffff_source_before_reading_unless_rep_count_is_zero() {
    for (instruction, repeated) in [
        (&[0x6F][..], false),
        (&[0x36, 0x6F][..], false),
        (&[0x26, 0xF3, 0x6F][..], true),
        (&[0xF3, 0x26, 0x6F][..], true),
    ] {
        for count in [0, 1, 3] {
            let (mut cpu, mut memory) = setup(CpuModel::I80286, instruction);
            cpu.si = 0xFFFF;
            cpu.cx = count;
            cpu.flags = 0x0FD7;
            let next_ip = 0x0100 + instruction.len() as u16;
            // No source RAM exists: count zero must skip validation and reads;
            // otherwise the segment violation must beat a host RAM bounds error.
            memory.truncate(0x10000 + usize::from(next_ip));
            let original_memory = memory.clone();
            let saved = registers(&cpu);

            let result = cpu.step(&mut memory).unwrap();

            if repeated && count == 0 {
                assert!(matches!(result, CpuStepResult::Continue));
                assert_eq!(cpu.ip, next_ip);
            } else {
                assert!(matches!(result, CpuStepResult::Interrupt(13)));
                assert_eq!(cpu.ip, 0x0100, "fault restarts before every prefix");
            }
            assert_eq!(
                registers(&cpu),
                saved,
                "fault or empty REP changes neither SI nor CX"
            );
            assert_eq!(cpu.flags, 0x0FD7);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn i286_rep_outsw_fault_after_one_word_preserves_completed_progress() {
    let (mut cpu, mut memory) = setup(CpuModel::I80286, &[0x3E, 0xF3, 0x6F]);
    cpu.si = 0xFFFD;
    cpu.cx = 2;
    cpu.flags = 0x0BD7;
    memory[0x2FFFD..0x2FFFF].copy_from_slice(&[0x34, 0x12]);
    check_write(&mut cpu, &mut memory, 0x1234, 0xFFFF, 1, 0x0100);
    let saved = registers(&cpu);
    let original_memory = memory.clone();

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Interrupt(13)
    ));

    assert_eq!(registers(&cpu), saved);
    assert_eq!((cpu.ip, cpu.si, cpu.cx), (0x0100, 0xFFFF, 1));
    assert_eq!(cpu.flags, 0x0BD7);
    assert!(memory == original_memory);
}

#[test]
fn i286_outsw_truncated_source_returns_memory_error_without_consuming_a_word() {
    for length in [0x0200, 0x0201] {
        let (mut cpu, mut memory) = setup(CpuModel::I80286, &[0xF3, 0x6F]);
        cpu.cs = 0;
        cpu.ds = 0;
        cpu.cx = 3;
        cpu.flags = 0x0FD7;
        memory[0x0100..0x0102].copy_from_slice(&[0xF3, 0x6F]);
        memory.truncate(length);
        let saved = registers(&cpu);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), saved);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
        // Host allocation errors do not specify architectural IP rollback.
    }
}

#[test]
fn i286_outsw_consumes_one_opcode_byte_and_reads_exactly_two_source_bytes() {
    let (mut cpu, mut memory) = setup(CpuModel::I80286, &[0x6F]);
    cpu.cs = 0;
    cpu.ds = 0;
    cpu.ip = 0x3FFF;
    memory.truncate(0x4000);
    memory[0x3FFF] = 0x6F; // No ModR/M or immediate byte follows.
    memory[0x0200..0x0202].copy_from_slice(&[0x34, 0x12]);
    check_write(&mut cpu, &mut memory, 0x1234, 0x0202, 0x2345, 0x4000);

    // Both bytes exist at the end of host RAM, but no third byte does.
    cpu.ip = 0x0100;
    cpu.si = 0x3FFE;
    memory[0x0100] = 0x6F;
    memory[0x3FFE..0x4000].copy_from_slice(&[0xCD, 0xAB]);
    check_write(&mut cpu, &mut memory, 0xABCD, 0x4000, 0x2345, 0x0101);
}

#[test]
fn i8088_6f_alias_and_canonical_7f_on_both_models_are_jg() {
    for (is_286, opcode) in [(false, 0x6F), (false, 0x7F), (true, 0x7F)] {
        // Complete ZF/SF/OF truth table; other flags vary independently.
        for (condition, taken) in [
            (0x0000, true),
            (0x0040, false),
            (0x0080, false),
            (0x00C0, false),
            (0x0800, false),
            (0x0840, false),
            (0x0880, true),
            (0x08C0, false),
        ] {
            for other_flags in [0x0000, 0x0715] {
                for (ip, displacement, target, fallthrough) in [
                    (0x0100u16, 0x00, 0x0102, 0x0102),
                    (0x0100, 0x7F, 0x0181, 0x0102),
                    (0x0100, 0x80, 0x0082, 0x0102),
                    (0x7FFD, 0x01, 0x8000, 0x7FFF),
                    (0x0000, 0xFD, 0xFFFF, 0x0002),
                    (0xFFFC, 0x02, 0x0000, 0xFFFE),
                    (0xFFFF, 0xFE, 0xFFFF, 0x0001),
                ] {
                    if is_286 && matches!(ip, 0x0000 | 0xFFFC | 0xFFFF) {
                        continue;
                    }
                    let (mut cpu, mut memory) = setup(
                        if is_286 {
                            CpuModel::I80286
                        } else {
                            CpuModel::I8088
                        },
                        &[],
                    );
                    cpu.ip = ip;
                    cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | condition | other_flags;
                    memory[0x10000 + usize::from(ip)] = opcode;
                    memory[0x10000 + usize::from(ip.wrapping_add(1))] = displacement;
                    let saved = registers(&cpu);
                    let flags = cpu.flags;
                    let original_memory = memory.clone();

                    assert!(matches!(
                        cpu.step(&mut memory).unwrap(),
                        CpuStepResult::Continue
                    ));

                    assert_eq!(cpu.ip, if taken { target } else { fallthrough });
                    assert_eq!(registers(&cpu), saved);
                    assert_eq!(cpu.flags, flags);
                    assert!(memory == original_memory);
                }
            }
        }
    }
}

#[test]
fn i8088_6f_alias_reads_one_displacement_without_accessing_ds_si() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ip = 0x3FFE;
    cpu.ds = 0xFFFF;
    cpu.si = 0x8000; // Source would be outside host RAM.
    let flags = cpu.flags; // ZF=SF=OF=0, so take JG.
    let saved = registers(&cpu);
    let mut memory = vec![0xCC; 0x4000];
    memory[0x3FFE..].copy_from_slice(&[0x6F, 0x80]);
    let original_memory = memory.clone();

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));

    assert_eq!(cpu.ip, 0x3F80);
    assert_eq!(registers(&cpu), saved);
    assert_eq!(cpu.flags, flags);
    assert!(memory == original_memory);
}
