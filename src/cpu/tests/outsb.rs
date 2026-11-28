use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// Single-transfer OUTSB: byte from DS:SI (segment override allowed) to port DX;
// SI += DF ? -1 : 1, with FLAGS and other registers preserved.
// REP tests also check one event per step, CX countdown, and instruction restart.
// https://bitsavers.trailing-edge.com/components/intel/80286/210498-005_80286_and_80287_Programmers_Reference_Manual_1987.pdf
// On 8088, 6E aliases 7E: JLE, taken when ZF=1 or SF != OF.
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn seed_cpu(model: CpuModel) -> Cpu {
    let mut cpu = Cpu::new(model, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0x1122, 0x2345, 0x1234, 0x3456, 0x0800, 0x4567, 0x0200, 0x6789,
    ];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = 0x0100;
    cpu
}

fn check_output(cpu: &mut Cpu, memory: &mut [u8], value: u8, final_si: u16, next_ip: u16) {
    let port = cpu.dx;
    let flags = cpu.flags;
    let mut expected = registers(cpu);
    expected[6] = final_si;
    let original_memory = memory.to_vec();

    let result = cpu
        .step(memory)
        .expect("OUTSB should read one accessible byte");

    assert!(
        matches!(result, CpuStepResult::PortWrite8 { port: actual_port, value: actual_value }
        if actual_port == port && actual_value == value),
        "expected one byte write: port={port:04X}, value={value:02X}"
    );
    assert_eq!(
        registers(cpu),
        expected,
        "only SI changes; AX, CX, DX, DI and segments are preserved"
    );
    assert_eq!(cpu.ip, next_ip, "consume opcode and segment prefixes only");
    assert_eq!(cpu.flags, flags, "OUTSB preserves every flag, including DF");
    assert!(
        memory == original_memory,
        "OUTSB reads guest memory without modifying it"
    );
}

#[test]
fn i286_outsb_reads_ds_si_uses_full_dx_and_moves_si_according_to_df() {
    // (SI, final SI with DF clear, final SI with DF set). Each source byte is
    // within the segment, even when the following SI adjustment wraps.
    for (si, forward, backward) in [
        (0x0200, 0x0201, 0x01FF),
        (0x0000, 0x0001, 0xFFFF),
        (0xFFFE, 0xFFFF, 0xFFFD),
        (0xFFFF, 0x0000, 0xFFFE),
    ] {
        for port in [0x0000, 0x0080, 0x0300, 0x03D8, 0x1234, 0x8000, 0xFFFF] {
            for value in [0x00, 0x7F, 0x80, 0xFF] {
                for other_flags in [0x0002, 0x0BD7] {
                    for backward_direction in [false, true] {
                        let mut cpu = seed_cpu(CpuModel::I80286);
                        cpu.si = si;
                        cpu.dx = port;
                        cpu.flags = other_flags | if backward_direction { 0x0400 } else { 0 };
                        let mut memory = vec![0xCC; 0x50000];
                        memory[0x10100] = 0x6E;
                        memory[0x20000 + usize::from(si)] = value;

                        check_output(
                            &mut cpu,
                            &mut memory,
                            value,
                            if backward_direction {
                                backward
                            } else {
                                forward
                            },
                            0x0101,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn i286_outsb_segment_overrides_select_source_and_last_prefix_wins() {
    for (prefixes, value) in [
        (&[][..], 0xA5),
        (&[0x3E][..], 0xA5), // DS.
        (&[0x26][..], 0xB6), // ES.
        (&[0x2E][..], 0xC7), // CS.
        (&[0x36][..], 0xD8), // SS.
        (&[0x26, 0x3E][..], 0xA5),
        (&[0x3E, 0x26][..], 0xB6),
        (&[0x36, 0x2E][..], 0xC7),
        (&[0x2E, 0x36][..], 0xD8),
    ] {
        for (flags, final_si) in [(0x0BD7, 0x0201), (0x0FD7, 0x01FF)] {
            let mut cpu = seed_cpu(CpuModel::I80286);
            cpu.flags = flags;
            let mut memory = vec![0xCC; 0x50000];
            let instruction: Vec<u8> = prefixes.iter().copied().chain([0x6E]).collect();
            memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
            for (address, byte) in [
                (0x20200, 0xA5),
                (0x30200, 0xB6),
                (0x10200, 0xC7),
                (0x40200, 0xD8),
            ] {
                memory[address] = byte;
            }

            check_output(
                &mut cpu,
                &mut memory,
                value,
                final_si,
                0x0100 + instruction.len() as u16,
            );
        }
    }
}

#[test]
fn i286_outsb_without_rep_ignores_cx_including_zero() {
    for cx in [0x0000, 0x0001, 0x0002, 0xFFFF] {
        let mut cpu = seed_cpu(CpuModel::I80286);
        cpu.cx = cx;
        let mut memory = vec![0xCC; 0x50000];
        memory[0x10100] = 0x6E;
        memory[0x20200] = 0xA5;

        check_output(&mut cpu, &mut memory, 0xA5, 0x0201, 0x0101);
    }
}

#[test]
fn i286_outsb_outputs_every_byte_value_without_loading_al() {
    for value in 0..=255u8 {
        let mut cpu = seed_cpu(CpuModel::I80286);
        // Make AL different from every tested source byte, including zero and FF.
        cpu.ax = 0x5A00 | u16::from(!value);
        let mut memory = vec![0xCC; 0x50000];
        memory[0x10100] = 0x6E;
        memory[0x20200] = value;

        check_output(&mut cpu, &mut memory, value, 0x0201, 0x0101);
    }
}

#[test]
fn i286_outsb_consumes_one_opcode_byte_without_modrm_or_immediate() {
    for prefixes in [&[][..], &[0x3E][..]] {
        let mut cpu = seed_cpu(CpuModel::I80286);
        cpu.cs = 0;
        cpu.ds = 0;
        cpu.ip = 0x4000 - (prefixes.len() as u16 + 1);
        let mut memory = vec![0xCC; 0x4000];
        let instruction: Vec<u8> = prefixes.iter().copied().chain([0x6E]).collect();
        memory[usize::from(cpu.ip)..].copy_from_slice(&instruction);
        memory[0x0200] = 0xA5;

        check_output(&mut cpu, &mut memory, 0xA5, 0x0201, 0x4000);
    }
}

#[test]
fn i286_outsb_reads_only_one_source_byte() {
    let mut cpu = seed_cpu(CpuModel::I80286);
    cpu.cs = 0;
    cpu.ds = 0;
    cpu.si = 0x3FFF;
    let mut memory = vec![0xCC; 0x4000];
    memory[0x0100] = 0x6E;
    memory[0x3FFF] = 0x80; // Last available byte; a word read would fail.

    check_output(&mut cpu, &mut memory, 0x80, 0x4000, 0x0101);
}

#[test]
fn i286_outsb_inaccessible_source_reports_memory_error_without_advancing_si() {
    for backward_direction in [false, true] {
        let mut cpu = seed_cpu(CpuModel::I80286);
        cpu.cs = 0;
        cpu.ds = 0;
        cpu.flags = 0x0BD7 | if backward_direction { 0x0400 } else { 0 };
        let mut memory = vec![0xCC; 0x0200]; // DS:0200 is outside host RAM.
        memory[0x0100] = 0x6E;
        let original_memory = memory.clone();
        let saved_registers = registers(&cpu);
        let flags = cpu.flags;

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), saved_registers);
        assert_eq!(cpu.flags, flags);
        assert!(memory == original_memory);
        // Host RAM errors do not specify architectural instruction-IP rollback.
    }
}

#[test]
fn i286_two_unprefixed_outsb_instructions_emit_two_successive_bytes() {
    for (flags, second_si, final_si) in [(0x0BD7, 0x0201, 0x0202), (0x0FD7, 0x01FF, 0x01FE)] {
        let mut cpu = seed_cpu(CpuModel::I80286);
        cpu.flags = flags;
        cpu.cx = 0;
        let mut memory = vec![0xCC; 0x50000];
        memory[0x10100..0x10103].copy_from_slice(&[0x6E, 0x6E, 0x90]);
        memory[0x20200] = 0xA1;
        memory[0x20000 + usize::from(second_si)] = 0xB2;

        check_output(&mut cpu, &mut memory, 0xA1, second_si, 0x0101);
        check_output(&mut cpu, &mut memory, 0xB2, final_si, 0x0102);
        let saved_registers = registers(&cpu);
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
        assert_eq!(
            cpu.ip, 0x0103,
            "following NOP does not request another transfer"
        );
        assert_eq!(registers(&cpu), saved_registers);
        assert_eq!(cpu.flags, flags);
    }
}

#[test]
fn i286_rep_outsb_emits_one_byte_per_step_and_finishes_after_the_last_transfer() {
    // Each row gives direction and literal SI values before/after three bytes.
    // Offset arithmetic can wrap while every byte read still fits its segment.
    for (backward, offsets) in [
        (false, [0x0200u16, 0x0201, 0x0202, 0x0203]),
        (true, [0x0200, 0x01FF, 0x01FE, 0x01FD]),
        (false, [0xFFFF, 0x0000, 0x0001, 0x0002]),
        (true, [0x0000, 0xFFFF, 0xFFFE, 0xFFFD]),
    ] {
        for count in [0u16, 1, 3] {
            // REP must preserve a source override whichever prefix comes first.
            for (instruction, bytes) in [
                (&[0xF3, 0x6E][..], [0xA5, 0xB6, 0xC7]),
                (&[0x26, 0xF3, 0x6E][..], [0x11, 0x22, 0x33]),
                (&[0xF3, 0x26, 0x6E][..], [0x11, 0x22, 0x33]),
            ] {
                // ZF starts clear and set; REP OUTSB must ignore it.
                for other_flags in [0x0002, 0x0BD7] {
                    let mut cpu = seed_cpu(CpuModel::I80286);
                    cpu.cx = count;
                    cpu.si = offsets[0];
                    cpu.flags = other_flags | if backward { 0x0400 } else { 0 };
                    let flags = cpu.flags;
                    let mut expected_registers = registers(&cpu);
                    let next_ip = 0x0100 + instruction.len() as u16;
                    let mut memory = vec![0xCC; 0x40000];
                    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
                    memory[0x10000 + usize::from(next_ip)] = 0x90; // Following NOP.
                    if count == 0 {
                        // Neither DS:SI nor ES:SI is backed by RAM: an empty
                        // repetition must skip both the source read and output.
                        memory.truncate(0x10000 + usize::from(next_ip) + 1);
                    } else {
                        for (index, &offset) in offsets[..3].iter().enumerate() {
                            memory[0x20000 + usize::from(offset)] = [0xA5, 0xB6, 0xC7][index];
                            memory[0x30000 + usize::from(offset)] = [0x11, 0x22, 0x33][index];
                        }
                    }
                    let original_memory = memory.clone();

                    if count == 0 {
                        assert!(matches!(
                            cpu.step(&mut memory).unwrap(),
                            CpuStepResult::Continue
                        ));
                        assert_eq!(cpu.ip, next_ip);
                        assert_eq!(registers(&cpu), expected_registers);
                        assert_eq!(cpu.flags, flags);
                        assert!(memory == original_memory);
                    } else {
                        // Bounded execution: exactly CX output events, never a
                        // loop that could hide an incorrect restart/countdown.
                        for index in 0..usize::from(count) {
                            let result = cpu.step(&mut memory).unwrap();
                            assert!(
                                matches!(result, CpuStepResult::PortWrite8 { port: 0x1234, value }
                                if value == bytes[index]),
                                "count={count}, transfer={index}, backward={backward}, instruction={instruction:02X?}"
                            );
                            let remaining = count - index as u16 - 1;
                            expected_registers[1] = remaining;
                            expected_registers[6] = offsets[index + 1];
                            assert_eq!(registers(&cpu), expected_registers);
                            assert_eq!(cpu.flags, flags);
                            assert_eq!(
                                cpu.ip,
                                if remaining == 0 { next_ip } else { 0x0100 },
                                "restart before all prefixes only while more transfers remain"
                            );
                            assert!(memory == original_memory);
                        }
                    }

                    // Last transfer must finish the instruction immediately:
                    // the next call executes NOP, not an extra zero-count step.
                    assert!(matches!(
                        cpu.step(&mut memory).unwrap(),
                        CpuStepResult::Continue
                    ));
                    assert_eq!(cpu.ip, next_ip + 1);
                    assert_eq!(registers(&cpu), expected_registers);
                    assert_eq!(cpu.flags, flags);
                    assert!(memory == original_memory);
                }
            }
        }
    }
}

#[test]
fn i8088_6e_alias_is_jle_using_zf_sf_and_of() {
    check_jle(false, 0x6E);
}

#[test]
fn i8088_7e_remains_jle() {
    check_jle(false, 0x7E);
}

#[test]
fn i286_7e_remains_jle() {
    check_jle(true, 0x7E);
}

fn check_jle(is_286: bool, opcode: u8) {
    // (ZF/SF/OF bits, taken): a literal truth table independent of the decoder.
    for (condition_flags, taken) in [
        (0x0000, false),
        (0x0040, true),
        (0x0080, true),
        (0x00C0, true),
        (0x0800, true),
        (0x0840, true),
        (0x0880, false),
        (0x08C0, true),
    ] {
        for other_flags in [0x0000, 0x0715] {
            for (ip, displacement, target, fallthrough) in [
                (0x0100u16, 0x00, 0x0102, 0x0102),
                (0x0100, 0x7F, 0x0181, 0x0102),
                (0x0100, 0x80, 0x0082, 0x0102),
                (0x0100, 0xFE, 0x0100, 0x0102),
                (0x0100, 0xC0, 0x00C2, 0x0102),
                (0x7FFD, 0x01, 0x8000, 0x7FFF),
                (0x0000, 0xFD, 0xFFFF, 0x0002),
                (0xFFFC, 0x02, 0x0000, 0xFFFE),
                (0xFFFF, 0xFE, 0xFFFF, 0x0001),
            ] {
                // 286 code/segment-limit fault behavior is a separate milestone.
                if is_286 && matches!(ip, 0x0000 | 0xFFFC | 0xFFFF) {
                    continue;
                }
                let mut cpu = seed_cpu(if is_286 {
                    CpuModel::I80286
                } else {
                    CpuModel::I8088
                });
                cpu.ip = ip;
                cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | condition_flags | other_flags;
                let mut memory = vec![0xCC; 0x50000];
                memory[0x10000 + usize::from(ip)] = opcode;
                memory[0x10000 + usize::from(ip.wrapping_add(1))] = displacement;
                let original_memory = memory.clone();
                let saved_registers = registers(&cpu);
                let flags = cpu.flags;

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    cpu.ip,
                    if taken { target } else { fallthrough },
                    "opcode={opcode:02X}, FLAGS={flags:04X}, IP={ip:04X}, disp={displacement:02X}"
                );
                assert_eq!(
                    registers(&cpu),
                    saved_registers,
                    "JLE must not update SI or request output"
                );
                assert_eq!(cpu.flags, flags);
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn i8088_6e_alias_reads_one_displacement_and_never_reads_ds_si() {
    let mut cpu = seed_cpu(CpuModel::I8088);
    cpu.cs = 0;
    cpu.ip = 0x3FFE;
    cpu.flags = 0xF042; // ZF set: take the branch.
    let mut memory = vec![0xCC; 0x4000];
    memory[0x3FFE..].copy_from_slice(&[0x6E, 0x80]);
    let original_memory = memory.clone();
    let saved_registers = registers(&cpu);

    // The displacement is the last byte; neither a source read at DS:SI nor
    // decoding 80h as ModR/M can succeed with this RAM slice.
    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));

    assert_eq!(cpu.ip, 0x3F80);
    assert_eq!(registers(&cpu), saved_registers);
    assert_eq!(cpu.flags, 0xF042);
    assert!(memory == original_memory);
}
