use super::{Cpu, CpuError, CpuStepResult};

mod addressing;

#[test]
fn lea_direct_address_covers_all_destinations_without_reading_memory() {
    // Destination field order: AX, CX, DX, BX, SP, BP, SI, DI.
    let initial_registers = [
        0xA001, 0xA102, 0xA203, 0xA304, 0xA405, 0xA506, 0xA607, 0xA708,
    ];
    let segments = [0x1357, 0x0000, 0x9ABC, 0xDEF0]; // ES, CS, SS, DS.
    for destination in 0..8 {
        for offset in [0x0000u16, 0x00FF, 0x0137, 0xFF00, 0x8000, 0xFFFF] {
            for initial_flags in [0x0000, 0x0FD7] {
                let mut cpu = Cpu::new();
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = initial_registers;
                [cpu.es, cpu.cs, cpu.ss, cpu.ds] = segments;
                cpu.flags = initial_flags;
                let mod_rm = ((destination as u8) << 3) | 0x06;
                let [low, high] = offset.to_le_bytes();
                // Only instruction bytes exist. Most offsets are outside this
                // slice, which is fine: LEA must not read the addressed word.
                // Nonzero DS must not be added to the effective offset either.
                let mut memory = [0x8D, mod_rm, low, high];
                let original_memory = memory;
                let mut expected_registers = initial_registers;
                expected_registers[destination] = offset;

                let result = cpu
                    .step(&mut memory)
                    .expect("LEA r16, [disp16] should succeed");

                assert!(matches!(result, CpuStepResult::Continue));
                assert_eq!(
                    [
                        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di
                    ],
                    expected_registers,
                    "8D {mod_rm:02X}, offset={offset:#06X}: only the destination changes"
                );
                assert_eq!([cpu.es, cpu.cs, cpu.ss, cpu.ds], segments);
                assert_eq!(cpu.ip, 4, "consume opcode, ModR/M, and both offset bytes");
                assert_eq!(cpu.flags, initial_flags, "LEA preserves every flag");
                assert_eq!(memory, original_memory, "LEA must not write memory");
            }
        }
    }
}

#[test]
fn lea_direct_address_wraps_ip_during_instruction_fetch() {
    // Cover wrapping after the high byte, between offset bytes, before the
    // low byte, and before ModR/M. Data addressing is still the flat model.
    for (start, next) in [
        (0xFFFCu16, 0x0000),
        (0xFFFD, 0x0001),
        (0xFFFE, 0x0002),
        (0xFFFF, 0x0003),
    ] {
        let mut cpu = Cpu::new();
        cpu.ip = start;
        cpu.dx = 0xA55A;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; 0x10000];
        // Same encoding as the game's LEA DX, [GUESS_PROMPT].
        for (index, byte) in [0x8D, 0x16, 0x37, 0x01].into_iter().enumerate() {
            memory[start.wrapping_add(index as u16) as usize] = byte;
        }
        let original_memory = memory.clone();

        let result = cpu
            .step(&mut memory)
            .expect("LEA instruction fetch should wrap IP");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.dx, 0x0137, "decode the full little-endian offset");
        assert_eq!(cpu.ip, next, "starting IP={start:#06X}");
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn lea_rejects_register_sources_without_changing_registers_flags_or_memory() {
    let initial_registers = [
        0xA001, 0xA102, 0xA203, 0xA304, 0xA405, 0xA506, 0xA607, 0xA708,
    ];
    for destination in 0u8..8 {
        for source in 0u8..8 {
            let mut cpu = Cpu::new();
            [
                cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
            ] = initial_registers;
            cpu.flags = 0x0FD7;
            let mod_rm = 0xC0 | (destination << 3) | source;
            let mut memory = [0x8D, mod_rm];
            let original_memory = memory;

            let result = cpu.step(&mut memory);

            assert!(
                matches!(result, Err(CpuError::UnsupportedModField(0b11))),
                "8D {mod_rm:02X} must reject the register addressing mode"
            );
            assert_eq!(
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di
                ],
                initial_registers,
                "invalid LEA must not copy the source register into the destination"
            );
            assert_eq!([cpu.es, cpu.cs, cpu.ss, cpu.ds], [0; 4]);
            assert_eq!(cpu.flags, 0x0FD7);
            assert_eq!(memory, original_memory);
        }
    }
}

#[test]
fn mov_to_segment_register_covers_es_ss_ds_and_all_source_registers() {
    // Source field order: AX, CX, DX, BX, SP, BP, SI, DI.
    let initial_registers = [
        0xA001, 0xA102, 0xA203, 0xA304, 0xA405, 0xA506, 0xA607, 0xA708,
    ];
    let initial_segments = [0x1357, 0x0000, 0x9ABC, 0xDEF0]; // ES, CS, SS, DS.
    for destination in [0usize, 2, 3] {
        for (source, &value) in initial_registers.iter().enumerate() {
            for initial_flags in [0x0000, 0x0FD7] {
                let mut cpu = Cpu::new();
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = initial_registers;
                [cpu.es, cpu.cs, cpu.ss, cpu.ds] = initial_segments;
                cpu.ip = 2;
                cpu.flags = initial_flags;
                let mod_rm = 0xC0 | ((destination as u8) << 3) | source as u8;
                let mut memory = [0xCC, 0xCC, 0x8E, mod_rm, 0xCC];
                let original_memory = memory;
                let mut expected_segments = initial_segments;
                expected_segments[destination] = value;

                let result = cpu.step(&mut memory).expect("MOV Sreg, r16 should succeed");

                assert!(matches!(result, CpuStepResult::Continue));
                assert_eq!(
                    [cpu.es, cpu.cs, cpu.ss, cpu.ds],
                    expected_segments,
                    "8E {mod_rm:02X} must change only its destination segment register"
                );
                assert_eq!(
                    [
                        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di
                    ],
                    initial_registers,
                    "8E must preserve its source and every other general register"
                );
                assert_eq!(cpu.ip, 4, "register form consumes only opcode and ModR/M");
                assert_eq!(cpu.flags, initial_flags, "MOV preserves every flag");
                assert_eq!(
                    memory, original_memory,
                    "register form must not write memory"
                );
            }
        }
    }
}

#[test]
fn mov_ds_ax_copies_the_full_word_without_dereferencing_it() {
    for value in [0x0000, 0x00FF, 0xFF00, 0x8000, 0xFFFF] {
        let mut cpu = Cpu::new();
        cpu.ax = value;
        cpu.ds = 0xA55A;
        cpu.flags = 0x0FD7;
        // Exact MOV DS, AX encoding used in guess.com. AX contains the value
        // to copy, not a memory address; only these two instruction bytes exist.
        let mut memory = [0x8E, 0xD8];

        let result = cpu.step(&mut memory).expect("MOV DS, AX should succeed");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(
            cpu.ds, value,
            "replace both halves of DS with AX={value:#06X}"
        );
        assert_eq!(cpu.ax, value);
        assert_eq!(cpu.ip, 2);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, [0x8E, 0xD8]);
    }
}

#[test]
fn mov_to_segment_register_wraps_ip_at_instruction_boundaries() {
    for (start, next) in [(0xFFFEu16, 0x0000), (0xFFFF, 0x0001)] {
        let mut cpu = Cpu::new();
        cpu.ip = start;
        cpu.di = 0xBEEF;
        cpu.ds = 0x1234;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; 0x10000];
        memory[start as usize] = 0x8E;
        memory[start.wrapping_add(1) as usize] = 0xDF; // MOV DS, DI.
        let original_memory = memory.clone();

        let result = cpu
            .step(&mut memory)
            .expect("MOV instruction fetch should wrap IP");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.ds, 0xBEEF);
        assert_eq!(cpu.di, 0xBEEF);
        assert_eq!(cpu.ip, next);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn mov_from_segment_register_covers_all_register_encodings() {
    // Segment field order: ES, CS, SS, DS.
    // Destination order: AX, CX, DX, BX, SP, BP, SI, DI.
    let segment_values = [0x1357, 0x2468, 0x9ABC, 0xDEF0];
    let initial_registers = [
        0xA001, 0xA102, 0xA203, 0xA304, 0xA405, 0xA506, 0xA607, 0xA708,
    ];
    for (segment_field, &value) in segment_values.iter().enumerate() {
        for destination in 0..8 {
            for initial_flags in [0x0000, 0x0FD7] {
                let mut cpu = Cpu::new();
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = initial_registers;
                [cpu.es, cpu.cs, cpu.ss, cpu.ds] = segment_values;
                cpu.ip = 2;
                cpu.flags = initial_flags;
                let mod_rm = 0xC0 | ((segment_field as u8) << 3) | destination as u8;
                let mut memory = [0xCC, 0xCC, 0x8C, mod_rm, 0xCC];
                let original_memory = memory;
                let mut expected_registers = initial_registers;
                expected_registers[destination] = value;

                let result = cpu.step(&mut memory).expect("MOV r16, Sreg should succeed");

                assert!(matches!(result, CpuStepResult::Continue));
                assert_eq!(
                    [
                        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di
                    ],
                    expected_registers,
                    "8C {mod_rm:02X} must change only its destination register"
                );
                assert_eq!(
                    [cpu.es, cpu.cs, cpu.ss, cpu.ds],
                    segment_values,
                    "8C reads the segment register; it must not write it"
                );
                assert_eq!(cpu.ip, 4, "register form consumes only opcode and ModR/M");
                assert_eq!(cpu.flags, initial_flags, "MOV preserves every flag");
                assert_eq!(
                    memory, original_memory,
                    "register form must not write memory"
                );
            }
        }
    }
}

#[test]
fn mov_ax_cs_copies_the_full_word_without_dereferencing_it() {
    for value in [0x0000, 0x00FF, 0xFF00, 0x8000, 0xFFFF] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xA55A;
        cpu.cs = value;
        cpu.flags = 0x0FD7;
        // Exact first instruction in guess.com. Only instruction bytes exist;
        // copying CS must not try to read a word at the address stored in CS.
        // Instruction fetch still uses the emulator's current flat-memory model.
        let mut memory = [0x8C, 0xC8];

        let result = cpu.step(&mut memory).expect("MOV AX, CS should succeed");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(
            cpu.ax, value,
            "replace both halves of AX with CS={value:#06X}"
        );
        assert_eq!(cpu.cs, value);
        assert_eq!(cpu.ip, 2);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, [0x8C, 0xC8]);
    }
}

#[test]
fn mov_from_segment_register_wraps_ip_when_fetching_mod_rm() {
    let mut cpu = Cpu::new();
    cpu.ip = 0xFFFF;
    cpu.ds = 0xBEEF;
    cpu.di = 0x1234;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x10000];
    memory[0xFFFF] = 0x8C;
    memory[0] = 0xDF; // MOV DI, DS: mod=11, segment=011, destination=111.
    let original_memory = memory.clone();

    let result = cpu.step(&mut memory).expect("ModR/M fetch should wrap IP");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.di, 0xBEEF);
    assert_eq!(cpu.ds, 0xBEEF);
    assert_eq!(cpu.ip, 1);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(memory, original_memory);
}

#[test]
fn cmp_al_updates_arithmetic_flags_without_changing_registers_or_memory() {
    // CF=0001, PF=0004, AF=0010, ZF=0040, SF=0080, OF=0800.
    let arithmetic_flags = 0x08D5;
    for (al, immediate, expected_flags) in [
        (0x61u8, 0x61, 0x0044), // Equal: zero result has even parity.
        (0x60, 0x61, 0x0095),   // Unsigned borrow; temporary result FF.
        (0x7B, 0x7A, 0x0000),   // Above: temporary result 01.
        (0x10, 0x01, 0x0014),   // Nibble borrow and even parity, without CF.
        (0x80, 0x01, 0x0810),   // -128 - 1: signed overflow to +127.
        (0x7F, 0xFF, 0x0881),   // +127 - (-1): signed overflow to -128.
        (0xFF, 0x00, 0x0084),   // High unsigned value; no borrow or overflow.
        (0x00, 0xFF, 0x0011),   // Unsigned borrow without signed overflow.
    ] {
        // Exercise setting flags and clearing stale ones; preserve control flags.
        for initial_flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xAB00 | al as u16;
            cpu.ip = 0x0100;
            cpu.flags = initial_flags;
            let registers = (
                cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.sp, cpu.bp,
            );
            let mut memory = vec![0xCC; 0x0200];
            memory[0x0100..0x0102].copy_from_slice(&[0x3C, immediate]);
            let original_memory = memory.clone();

            let result = cpu.step(&mut memory).expect("CMP AL should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.ip, 0x0102);
            assert_eq!(
                cpu.flags,
                (initial_flags & !arithmetic_flags) | expected_flags,
                "AL={al:#04X}, immediate={immediate:#04X}, initial flags={initial_flags:#06X}"
            );
            assert_eq!(
                (
                    cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.sp, cpu.bp
                ),
                registers,
                "CMP must not store the subtraction result"
            );
            assert_eq!(memory, original_memory);
        }
    }
}

#[test]
fn je_depends_only_on_zero() {
    for (flags, target) in [
        (0x0000, 0x0102),
        (0x0040, 0x0107),
        (0x0F97, 0x0102), // Other flags set, ZF clear.
        (0x0FD7, 0x0107), // Other flags set, ZF set.
    ] {
        assert_conditional_jump(0x74, flags, 0x0100, 5, target);
    }
}

#[test]
fn jl_and_jg_cover_every_sign_overflow_and_zero_combination() {
    // Explicit truth table: ZF=0040, SF=0080, OF=0800.
    for (flags, jl_taken, jg_taken) in [
        (0x0000, false, true),
        (0x0040, false, false),
        (0x0080, true, false),
        (0x00C0, true, false),
        (0x0800, true, false),
        (0x0840, true, false),
        (0x0880, false, true),
        (0x08C0, false, false),
    ] {
        // CF, PF, AF and control flags must not influence either condition.
        for other_flags in [0, 0x0717] {
            for (opcode, taken) in [(0x7C, jl_taken), (0x7F, jg_taken)] {
                let target = if taken { 0x0107 } else { 0x0102 };
                assert_conditional_jump(opcode, flags | other_flags, 0x0100, 5, target);
            }
        }
    }
}

#[test]
fn je_jl_and_jg_use_signed_offsets_and_wrap_ip() {
    for (opcode, taken_flags, untaken_flags) in [
        (0x74, 0x0040, 0x0000),
        (0x7C, 0x0080, 0x0000),
        (0x7F, 0x0000, 0x0040),
    ] {
        // Start, encoded rel8, taken target, fall-through IP.
        for (start, displacement, target, next) in [
            (0x0100, 0x00, 0x0102, 0x0102),
            (0x0100, 0x7F, 0x0181, 0x0102), // +127.
            (0x0100, 0x80, 0x0082, 0x0102), // -128.
            (0x0100, 0xFE, 0x0100, 0x0102), // -2; execute once, not a loop.
            (0x0100, 0xFF, 0x0101, 0x0102), // -1.
            (0x7FFD, 0x01, 0x8000, 0x7FFF),
            (0x7FFE, 0xFF, 0x7FFF, 0x8000),
            (0xFFFD, 0x02, 0x0001, 0xFFFF), // Target wraps forward.
            (0x0000, 0xFD, 0xFFFF, 0x0002), // Target wraps backward.
            (0xFFFE, 0x00, 0x0000, 0x0000),
            (0xFFFF, 0xFE, 0xFFFF, 0x0001), // Displacement fetched at 0000.
        ] {
            assert_conditional_jump(opcode, taken_flags, start, displacement, target);
            assert_conditional_jump(opcode, untaken_flags, start, displacement, next);
        }
    }
}

#[test]
fn cmp_then_je_jl_or_jg_handles_guesses_and_signed_overflow() {
    for (al, immediate, expected_taken) in [
        (b'1', b'5', [false, true, false]),
        (b'5', b'5', [true, false, false]),
        (b'9', b'5', [false, false, true]),
        (0x80, 0x01, [false, true, false]), // -128 < +1, despite positive result.
        (0x7F, 0xFF, [false, false, true]), // +127 > -1, despite negative result.
        (0xFF, 0x00, [false, true, false]), // -1 < 0; differs from unsigned comparison.
    ] {
        for (opcode, taken) in [0x74, 0x7C, 0x7F].into_iter().zip(expected_taken) {
            let mut cpu = Cpu::new();
            cpu.ax = 0xAB00 | al as u16;
            cpu.flags = 0x0FD7;
            let mut memory = [0x3C, immediate, opcode, 0x05];
            let original_memory = memory;

            assert!(matches!(cpu.step(&mut memory), Ok(CpuStepResult::Continue)));
            let comparison_flags = cpu.flags;
            assert!(matches!(cpu.step(&mut memory), Ok(CpuStepResult::Continue)));

            assert_eq!(
                cpu.ip,
                if taken { 9 } else { 4 },
                "AL={al:#04X}, immediate={immediate:#04X}, opcode={opcode:#04X}"
            );
            assert_eq!(cpu.ax, 0xAB00 | al as u16);
            assert_eq!(cpu.flags, comparison_flags);
            assert_eq!(memory, original_memory);
        }
    }
}

#[test]
fn jb_depends_only_on_carry() {
    // Explicitly cover every CF/ZF combination, including both set.
    for (condition_flags, expected_ip) in [
        (0x0000, 0x0102),
        (0x0001, 0x0107),
        (0x0040, 0x0102),
        (0x0041, 0x0107),
    ] {
        for other_flags in [0, 0x0F94] {
            assert_conditional_jump(
                0x72,
                condition_flags | other_flags,
                0x0100,
                0x05,
                expected_ip,
            );
        }
    }
}

#[test]
fn ja_requires_both_carry_and_zero_clear() {
    for (condition_flags, expected_ip) in [
        (0x0000, 0x0107),
        (0x0001, 0x0102),
        (0x0040, 0x0102),
        (0x0041, 0x0102),
    ] {
        for other_flags in [0, 0x0F94] {
            assert_conditional_jump(
                0x77,
                condition_flags | other_flags,
                0x0100,
                0x05,
                expected_ip,
            );
        }
    }
}

#[test]
fn jb_and_ja_use_signed_displacements_from_the_next_ip() {
    for (opcode, taken_flags, untaken_flags) in [(0x72, 0x0001, 0x0000), (0x77, 0x0000, 0x0040)] {
        for (displacement, target) in [
            (0x00, 0x0102),
            (0x01, 0x0103),
            (0x7F, 0x0181), // Largest positive rel8: +127.
            (0x80, 0x0082), // Most negative rel8: -128.
            (0xFE, 0x0100), // -2 branches back to the opcode.
            (0xFF, 0x0101), // -1 branches to the displacement byte.
        ] {
            assert_conditional_jump(opcode, taken_flags, 0x0100, displacement, target);
            assert_conditional_jump(opcode, untaken_flags, 0x0100, displacement, 0x0102);
        }
    }
}

#[test]
fn jb_and_ja_wrap_targets_and_instruction_fetch_at_16_bits() {
    for (opcode, taken_flags, untaken_flags) in [(0x72, 0x0001, 0x0000), (0x77, 0x0000, 0x0040)] {
        // Starting IP, rel8, taken IP, untaken IP.
        for (start, displacement, target, next) in [
            (0x7FFD, 0x01, 0x8000, 0x7FFF),
            (0x7FFE, 0xFF, 0x7FFF, 0x8000),
            (0xFFFD, 0x02, 0x0001, 0xFFFF),
            (0x0000, 0xFD, 0xFFFF, 0x0002),
            (0xFFFE, 0x00, 0x0000, 0x0000),
            (0xFFFF, 0xFE, 0xFFFF, 0x0001), // Displacement fetched at 0000.
        ] {
            assert_conditional_jump(opcode, taken_flags, start, displacement, target);
            assert_conditional_jump(opcode, untaken_flags, start, displacement, next);
        }
    }
}

fn assert_conditional_jump(opcode: u8, flags: u16, start: u16, displacement: u8, expected_ip: u16) {
    let mut cpu = Cpu::new();
    [
        cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.sp, cpu.bp,
    ] = [
        0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A, 0x89AB,
    ];
    cpu.ip = start;
    cpu.flags = flags;
    let registers = (
        cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.sp, cpu.bp,
    );
    let mut memory = vec![0xCC; 0x10000];
    memory[start as usize] = opcode;
    memory[start.wrapping_add(1) as usize] = displacement;
    let original_memory = memory.clone();

    // Execute one instruction even when the target would loop or contain data.
    let result = cpu
        .step(&mut memory)
        .expect("conditional jump should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(
        cpu.ip, expected_ip,
        "opcode={opcode:#04X}, flags={flags:#06X}, start={start:#06X}, rel8={displacement:#04X}"
    );
    assert_eq!(cpu.flags, flags, "jumps must preserve every flag");
    assert_eq!(
        (
            cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.sp, cpu.bp
        ),
        registers
    );
    assert_eq!(memory, original_memory);
}

#[test]
fn cmp_then_jb_or_ja_handles_ascii_boundaries_and_unsigned_bytes() {
    for (al, immediate, opcode, expected_ip) in [
        (b'`', b'a', 0x72, 0x0107), // Below lowercase range.
        (b'a', b'a', 0x72, 0x0104), // Lower boundary is included.
        (b'z', b'z', 0x77, 0x0104), // Upper boundary is included.
        (b'{', b'z', 0x77, 0x0107), // Above lowercase range.
        (0xFF, b'a', 0x72, 0x0104), // FF is large unsigned, not negative.
        (0xFF, b'z', 0x77, 0x0107),
    ] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xAB00 | al as u16;
        cpu.ip = 0x0100;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0104].copy_from_slice(&[0x3C, immediate, opcode, 0x03]);

        let cmp = cpu.step(&mut memory).expect("CMP should succeed");
        assert!(matches!(cmp, CpuStepResult::Continue));
        let comparison_flags = cpu.flags;
        let jump = cpu
            .step(&mut memory)
            .expect("jump after CMP should succeed");

        assert!(matches!(jump, CpuStepResult::Continue));
        assert_eq!(cpu.ip, expected_ip);
        assert_eq!(cpu.ax, 0xAB00 | al as u16);
        assert_eq!(cpu.flags, comparison_flags);
    }
}

#[test]
fn call_uses_displacement_from_next_ip_and_saves_return_address() {
    for (start_ip, displacement, target) in [
        (0x0100u16, 0x0000u16, 0x0103),
        (0x0100, 0x0020, 0x0123),
        (0x0100, 0xFFFD, 0x0100), // -3: back to the CALL itself.
        (0x0100, 0xFF00, 0x0003), // -256: exercise both displacement bytes.
        (0x7FFD, 0x0001, 0x8001),
        (0xFFFB, 0x0005, 0x0003), // Target wraps past FFFF.
        (0x0000, 0xFFFC, 0xFFFF), // Target wraps below zero.
    ] {
        for flags in [0, 0x0FD5] {
            let mut cpu = Cpu::new();
            cpu.ip = start_ip;
            cpu.sp = 0x9000;
            cpu.flags = flags;
            let registers = (cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp);
            let mut memory = vec![0xCC; 0x10000];
            let address = start_ip as usize;
            memory[address..address + 3].copy_from_slice(&[
                0xE8,
                displacement as u8,
                (displacement >> 8) as u8,
            ]);
            let mut expected_memory = memory.clone();
            expected_memory[0x8FFE..0x9000].copy_from_slice(&(start_ip + 3).to_le_bytes());

            let result = cpu.step(&mut memory).expect("near CALL should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(
                cpu.ip, target,
                "CALL at {start_ip:#06X}, disp={displacement:#06X}"
            );
            assert_eq!(cpu.sp, 0x8FFE, "push exactly one word");
            assert_eq!(
                memory, expected_memory,
                "save the next IP in little-endian order"
            );
            assert_eq!(cpu.flags, flags);
            assert_eq!(
                (cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp),
                registers
            );
        }
    }
}

#[test]
fn call_at_end_of_address_space_saves_wrapped_return_address() {
    let mut cpu = Cpu::new();
    cpu.ip = 0xFFFD;
    cpu.sp = 0x9000;
    let mut memory = vec![0xCC; 0x10000];
    // The final displacement byte is at FFFF, so the next IP is 0000.
    memory[0xFFFD..0x10000].copy_from_slice(&[0xE8, 0x20, 0x00]);

    let result = cpu
        .step(&mut memory)
        .expect("instruction fetching must wrap IP");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x0020);
    assert_eq!(cpu.sp, 0x8FFE);
    assert_eq!(&memory[0x8FFE..0x9000], &[0x00, 0x00]);
}

#[test]
fn call_and_ret_wrap_the_stack_pointer() {
    let mut cpu = Cpu::new();
    cpu.ip = 0x0100;
    cpu.sp = 0;
    cpu.flags = 0x0FD5;
    let mut memory = vec![0xCC; 0x10000];
    memory[0x0100..0x0103].copy_from_slice(&[0xE8, 0x0D, 0x00]);
    memory[0x0110] = 0xC3;

    let call = cpu.step(&mut memory).expect("CALL should wrap SP downward");
    assert!(matches!(call, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x0110);
    assert_eq!(cpu.sp, 0xFFFE);
    assert_eq!(&memory[0xFFFE..0x10000], &[0x03, 0x01]);

    let ret = cpu.step(&mut memory).expect("RET should wrap SP upward");
    assert!(matches!(ret, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x0103);
    assert_eq!(cpu.sp, 0);
    assert_eq!(cpu.flags, 0x0FD5);
    assert_eq!(&memory[0xFFFE..0x10000], &[0x03, 0x01]);
}

#[test]
fn ret_loads_an_absolute_address_and_preserves_registers_flags_and_memory() {
    for target in [0x0000u16, 0x1234, 0xFFFF] {
        for flags in [0, 0x0FD5] {
            let mut cpu = Cpu::new();
            cpu.ip = 0x0100;
            cpu.sp = 0x01FE;
            cpu.flags = flags;
            let registers = (cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp);
            let mut memory = vec![0xCC; 0x10000];
            memory[0x0100] = 0xC3;
            memory[0x01FE..0x0200].copy_from_slice(&target.to_le_bytes());
            let original_memory = memory.clone();

            let result = cpu.step(&mut memory).expect("RET should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.ip, target, "do not add to or advance the popped IP");
            assert_eq!(cpu.sp, 0x0200);
            assert_eq!(cpu.flags, flags);
            assert_eq!(memory, original_memory);
            assert_eq!(
                (cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp),
                registers
            );
        }
    }
}

#[test]
fn nested_calls_return_in_reverse_order() {
    let mut cpu = Cpu::new();
    cpu.ip = 0x0100;
    cpu.sp = 0x0200;
    let mut memory = vec![0xCC; 0x0200];
    memory[0x0100..0x0103].copy_from_slice(&[0xE8, 0x0D, 0x00]); // CALL 0110
    memory[0x0110..0x0114].copy_from_slice(&[0xE8, 0x0D, 0x00, 0xC3]); // CALL 0120; RET
    memory[0x0120] = 0xC3;

    // A fixed step count keeps a broken branch from looping indefinitely.
    for (expected_ip, expected_sp) in [
        (0x0110, 0x01FE),
        (0x0120, 0x01FC),
        (0x0113, 0x01FE),
        (0x0103, 0x0200),
    ] {
        let result = cpu
            .step(&mut memory)
            .expect("nested CALL/RET should succeed");
        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.ip, expected_ip);
        assert_eq!(cpu.sp, expected_sp);
    }
    assert_eq!(&memory[0x01FC..0x0200], &[0x13, 0x01, 0x03, 0x01]);
}

#[test]
fn call_returns_a_stack_bounds_error_without_partially_writing_memory() {
    for initial_sp in [0x0201, 0x0202, 0x0000] {
        let mut cpu = Cpu::new();
        cpu.ip = 0x0100;
        cpu.sp = initial_sp;
        cpu.flags = 0x0FD5;
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0103].copy_from_slice(&[0xE8, 0x0D, 0x00]);
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        assert!(matches!(result, Err(CpuError::OutOfBoundsMemoryAccess(_))));
        assert_eq!(memory, original_memory);
        assert_eq!(cpu.flags, 0x0FD5);
        // Error recovery / rollback of IP and SP is not specified here.
    }
}

#[test]
fn ret_returns_a_stack_bounds_error_without_consuming_a_word() {
    for initial_sp in [0x01FF, 0x0200, 0xFFFE] {
        let mut cpu = Cpu::new();
        cpu.ip = 0x0100;
        cpu.sp = initial_sp;
        cpu.flags = 0x0FD5;
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100] = 0xC3;
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        assert!(matches!(result, Err(CpuError::OutOfBoundsMemoryAccess(_))));
        assert_eq!(cpu.sp, initial_sp);
        assert_eq!(cpu.flags, 0x0FD5);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn pop_ax_reads_little_endian_and_preserves_other_registers_flags_and_memory() {
    for initial_sp in [0x0180, 0x01FE] {
        for initial_flags in [0x0000, 0x0FD5] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xFFFF;
            cpu.sp = initial_sp;
            cpu.ip = 0x0100;
            cpu.flags = initial_flags;
            let other_registers = (cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp);
            let mut memory = vec![0xCC; 0x0200];
            memory[0x0100] = 0x58; // POP AX
            let address = initial_sp as usize;
            memory[address..address + 2].copy_from_slice(&[0x34, 0xAB]);
            let original_memory = memory.clone();

            let result = cpu.step(&mut memory).expect("POP AX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.ax, 0xAB34, "combine both bytes, replacing the old AX");
            assert_eq!(cpu.sp, initial_sp + 2, "read before advancing SP");
            assert_eq!(
                cpu.ip, 0x0101,
                "stack reads must not consume instruction bytes"
            );
            assert_eq!(cpu.flags, initial_flags);
            assert_eq!(
                (cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp),
                other_registers
            );
            assert_eq!(memory, original_memory, "POP does not erase the stack word");
        }
    }
}

#[test]
fn pop_ax_wraps_sp_from_fffe_to_zero() {
    let mut cpu = Cpu::new();
    cpu.ax = 0xFFFF;
    cpu.sp = 0xFFFE;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD5;
    let mut memory = vec![0xCC; 0x10000];
    memory[0x0100] = 0x58;
    memory[0xFFFE..0x10000].copy_from_slice(&[0x78, 0x56]);
    let original_memory = memory.clone();

    let result = cpu.step(&mut memory).expect("SP addition should wrap");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.ax, 0x5678);
    assert_eq!(cpu.sp, 0);
    assert_eq!(cpu.ip, 0x0101);
    assert_eq!(cpu.flags, 0x0FD5);
    assert_eq!(memory, original_memory);
}

#[test]
fn pop_ax_returns_a_bounds_error_without_changing_ax_or_sp() {
    // One byte available, no bytes available, and an address well beyond RAM.
    for initial_sp in [0x01FF, 0x0200, 0xFFFE] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x1234;
        cpu.sp = initial_sp;
        cpu.ip = 0x0100;
        cpu.flags = 0x0FD5;
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100] = 0x58;
        let original_memory = memory.clone();

        // A host panic is a test failure, not an acceptable bounds error.
        let result = cpu.step(&mut memory);

        assert!(
            matches!(result, Err(CpuError::OutOfBoundsMemoryAccess(_))),
            "expected a bounds error with SP={initial_sp:#06X}"
        );
        assert_eq!(cpu.ax, 0x1234, "do not load a partial word");
        assert_eq!(cpu.sp, initial_sp, "do not consume a word on failure");
        assert_eq!(cpu.flags, 0x0FD5);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn pop_ax_restores_pushed_values_in_reverse_order() {
    let mut cpu = Cpu::new();
    cpu.sp = 0x0200;
    cpu.ip = 0x0100;
    let mut memory = vec![0; 0x0200];
    memory[0x0100..0x0104].copy_from_slice(&[0x50, 0x50, 0x58, 0x58]);

    cpu.ax = 0x1234;
    cpu.step(&mut memory).expect("push first value");
    cpu.ax = 0xABCD;
    cpu.step(&mut memory).expect("push second value");
    cpu.ax = 0;

    let first = cpu.step(&mut memory).expect("pop most recent value");
    assert!(matches!(first, CpuStepResult::Continue));
    assert_eq!(cpu.ax, 0xABCD);
    assert_eq!(cpu.sp, 0x01FE);

    let second = cpu.step(&mut memory).expect("pop original value");
    assert!(matches!(second, CpuStepResult::Continue));
    assert_eq!(cpu.ax, 0x1234);
    assert_eq!(cpu.sp, 0x0200);
    assert_eq!(cpu.ip, 0x0104);
}

#[test]
fn push_ax_stores_a_little_endian_word_and_preserves_registers_and_flags() {
    // Exercise both an interior address and a word ending at the memory boundary.
    for initial_sp in [0x0180, 0x0200] {
        for initial_flags in [0x0000, 0x0FD5] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xAB34;
            cpu.sp = initial_sp;
            cpu.ip = 0x0100;
            cpu.flags = initial_flags;
            let mut memory = vec![0xCC; 0x0200];
            memory[0x0100] = 0x50; // PUSH AX
            let mut expected_memory = memory.clone();
            let address = (initial_sp - 2) as usize;
            expected_memory[address..address + 2].copy_from_slice(&[0x34, 0xAB]);
            let other_registers = (cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp);

            let result = cpu.step(&mut memory).expect("PUSH AX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.sp, initial_sp - 2);
            assert_eq!(cpu.ax, 0xAB34, "PUSH must preserve AX");
            assert_eq!(cpu.ip, 0x0101, "PUSH AX consumes only its opcode");
            assert_eq!(cpu.flags, initial_flags, "PUSH must preserve all flags");
            assert_eq!(
                (cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp),
                other_registers
            );
            assert_eq!(memory, expected_memory, "write exactly the two stack bytes");
        }
    }
}

#[test]
fn consecutive_pushes_grow_downward_without_overwriting_previous_words() {
    let mut cpu = Cpu::new();
    cpu.sp = 0x0200;
    cpu.ip = 0x0100;
    let mut memory = vec![0xCC; 0x0200];
    memory[0x0100..0x0102].copy_from_slice(&[0x50, 0x50]);

    cpu.ax = 0x1234;
    let first = cpu.step(&mut memory).expect("first PUSH should succeed");
    assert!(matches!(first, CpuStepResult::Continue));
    assert_eq!(cpu.sp, 0x01FE);
    assert_eq!(&memory[0x01FE..0x0200], &[0x34, 0x12]);

    cpu.ax = 0xABCD;
    let second = cpu.step(&mut memory).expect("second PUSH should succeed");
    assert!(matches!(second, CpuStepResult::Continue));
    assert_eq!(cpu.sp, 0x01FC);
    assert_eq!(cpu.ip, 0x0102);
    assert_eq!(&memory[0x01FC..0x0200], &[0xCD, 0xAB, 0x34, 0x12]);
    assert_eq!(memory[0x01FB], 0xCC);
}

#[test]
fn push_ax_wraps_sp_from_zero_to_fffe() {
    let mut cpu = Cpu::new();
    cpu.ax = 0x5678;
    cpu.sp = 0;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD5;
    let mut memory = vec![0xCC; 0x10000];
    memory[0x0100] = 0x50;

    let result = cpu.step(&mut memory).expect("SP subtraction should wrap");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.sp, 0xFFFE);
    assert_eq!(&memory[0xFFFE..0x10000], &[0x78, 0x56]);
    assert_eq!(memory[0xFFFD], 0xCC);
    assert_eq!(memory[0], 0xCC);
    assert_eq!(cpu.ax, 0x5678);
    assert_eq!(cpu.ip, 0x0101);
    assert_eq!(cpu.flags, 0x0FD5);
}

#[test]
fn push_ax_returns_a_bounds_error_without_partially_writing_memory() {
    // Only one byte available, neither byte available, and a wrapped SP outside RAM.
    for initial_sp in [0x0201, 0x0202, 0x0000] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x1234;
        cpu.sp = initial_sp;
        cpu.ip = 0x0100;
        cpu.flags = 0x0FD5;
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100] = 0x50;
        let original_memory = memory.clone();

        // A host panic fails the test; the emulator must return a typed error.
        let result = cpu.step(&mut memory);

        assert!(
            matches!(result, Err(CpuError::OutOfBoundsMemoryAccess(_))),
            "expected a bounds error with initial SP={initial_sp:#06X}"
        );
        assert_eq!(
            memory, original_memory,
            "a failed push must not write one byte"
        );
        assert_eq!(cpu.ax, 0x1234);
        assert_eq!(cpu.flags, 0x0FD5);
    }
}

#[test]
fn mov_from_si_disp8_accepts_zero_and_positive_offsets() {
    for (base, displacement, expected_address) in [
        (0x0180, 0x00, 0x0180),
        (0x0180, 0x05, 0x0185),
        (0x2000, 0x7F, 0x207F), // Largest positive disp8: +127.
    ] {
        assert_si_disp8_load(base, displacement, expected_address);
    }
}

#[test]
fn mov_from_si_disp8_sign_extends_negative_offsets() {
    for (base, displacement, expected_address) in [
        (0x0180, 0xFE, 0x017E), // -2, not +254.
        (0x2000, 0x80, 0x1F80), // Smallest disp8: -128, not +128.
    ] {
        assert_si_disp8_load(base, displacement, expected_address);
    }
}

#[test]
fn mov_from_si_disp8_wraps_the_effective_address() {
    for (base, displacement, expected_address) in [
        (0xFFFF, 0x01, 0x0000),
        (0x0000, 0xFF, 0xFFFF), // 0 - 1 wraps to 0xFFFF.
    ] {
        assert_si_disp8_load(base, displacement, expected_address);
    }
}

fn assert_si_disp8_load(base: u16, displacement: u8, expected_address: u16) {
    let mut cpu = Cpu::new();
    cpu.ax = 0xABCD;
    cpu.si = base;
    cpu.ip = 0x0100;
    cpu.flags = 0x08D5;
    let mut memory = vec![0; 0x10000];
    // MOV AL, [SI + disp8]: opcode, ModR/M, displacement.
    memory[0x0100..0x0103].copy_from_slice(&[0x8A, 0x44, displacement]);
    memory[expected_address as usize] = 0x5A;

    let result = cpu
        .step(&mut memory)
        .expect("displaced load should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(
        cpu.ax, 0xAB5A,
        "SI={base:#06X}, disp8={displacement:#04X}: expected a load from {expected_address:#06X}"
    );
    assert_eq!(cpu.si, base, "address calculation must not change SI");
    assert_eq!(cpu.ip, 0x0103, "consume exactly one displacement byte");
    assert_eq!(cpu.flags, 0x08D5, "MOV must preserve flags");
}

#[test]
fn mov_to_si_disp8_writes_at_a_negative_offset() {
    let mut cpu = Cpu::new();
    cpu.ax = 0xA534;
    cpu.si = 0x0180;
    cpu.ip = 0x0100;
    cpu.flags = 0x08D5;
    let mut memory = vec![0xCC; 0x10000];
    // MOV [SI - 2], AH.
    memory[0x0100..0x0103].copy_from_slice(&[0x88, 0x64, 0xFE]);

    let result = cpu
        .step(&mut memory)
        .expect("displaced store should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(&memory[0x017D..0x0180], &[0xCC, 0xA5, 0xCC]);
    assert_eq!(memory[0x027E], 0xCC, "must not interpret 0xFE as +254");
    assert_eq!(cpu.ax, 0xA534, "store must preserve its source register");
    assert_eq!(cpu.si, 0x0180);
    assert_eq!(cpu.ip, 0x0103);
    assert_eq!(cpu.flags, 0x08D5);
}

#[test]
fn dec_cx_updates_auxiliary_carry_on_low_nibble_borrow() {
    for (starting_cx, expected_cx, expected_af) in
        [(0x0010, 0x000F, 0x0010), (0x0011, 0x0010, 0x0000)]
    {
        // Start AF both clear and set to catch stale flag values.
        for initial_flags in [0x0000, 0x0010] {
            let mut cpu = Cpu::new();
            cpu.cx = starting_cx;
            cpu.flags = initial_flags;
            let mut memory = [0x49];

            let result = cpu.step(&mut memory).expect("DEC CX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.cx, expected_cx);
            assert_eq!(cpu.ip, 1);
            assert_eq!(
                cpu.flags & 0x0010,
                expected_af,
                "AF for CX={starting_cx:#06X}, initial flags={initial_flags:#06X}"
            );
        }
    }
}

#[test]
fn dec_cx_computes_parity_from_only_the_low_byte() {
    // The high byte has one set bit, so counting the entire word would
    // produce the opposite parity for both cases.
    for (starting_cx, expected_cx, expected_pf) in
        [(0x0101, 0x0100, 0x0004), (0x0102, 0x0101, 0x0000)]
    {
        for initial_flags in [0x0000, 0x0004] {
            let mut cpu = Cpu::new();
            cpu.cx = starting_cx;
            cpu.flags = initial_flags;
            let mut memory = [0x49];

            let result = cpu.step(&mut memory).expect("DEC CX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.cx, expected_cx);
            assert_eq!(cpu.ip, 1);
            assert_eq!(
                cpu.flags & 0x0004,
                expected_pf,
                "PF for CX={starting_cx:#06X}, initial flags={initial_flags:#06X}"
            );
        }
    }
}

#[test]
fn dec_cx_updates_sign_and_overflow() {
    let cases = [
        // Starting CX, result, SF mask, OF mask.
        (0x8000, 0x7FFF, 0x0000, 0x0800),
        (0x7FFF, 0x7FFE, 0x0000, 0x0000),
        (0x0000, 0xFFFF, 0x0080, 0x0000),
    ];

    for (starting_cx, expected_cx, expected_sf, expected_of) in cases {
        // Exercise both setting flags and clearing stale flags.
        for initial_flags in [0x0000, 0x08C1] {
            let mut cpu = Cpu::new();
            cpu.cx = starting_cx;
            cpu.flags = initial_flags;
            let mut memory = [0x49];

            let result = cpu.step(&mut memory).expect("DEC CX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.cx, expected_cx);
            assert_eq!(cpu.ip, 1);
            assert_eq!(
                cpu.flags & 0x0080,
                expected_sf,
                "SF for CX={starting_cx:#06X}"
            );
            assert_eq!(
                cpu.flags & 0x0800,
                expected_of,
                "OF for CX={starting_cx:#06X}"
            );
            assert_eq!(cpu.flags & 0x0040, 0, "all these results are nonzero");
            assert_eq!(cpu.flags & 1, initial_flags & 1, "DEC must preserve carry");
        }
    }
}

#[test]
fn dec_cx_clears_overflow_on_the_next_decrement() {
    let mut cpu = Cpu::new();
    cpu.cx = 0x8000;
    let mut memory = [0x49, 0x49];

    let first = cpu.step(&mut memory).expect("first DEC should succeed");

    assert!(matches!(first, CpuStepResult::Continue));
    assert_eq!(cpu.cx, 0x7FFF);
    assert_eq!(cpu.ip, 1);
    assert_eq!(cpu.flags & 0x0800, 0x0800, "first DEC must set OF");
    assert_eq!(cpu.flags & 0x00C0, 0, "SF and ZF must be clear");

    let second = cpu.step(&mut memory).expect("second DEC should succeed");

    assert!(matches!(second, CpuStepResult::Continue));
    assert_eq!(cpu.cx, 0x7FFE);
    assert_eq!(cpu.ip, 2);
    assert_eq!(cpu.flags & 0x0800, 0, "second DEC must clear OF");
    assert_eq!(cpu.flags & 0x00C0, 0, "SF and ZF must remain clear");
}

// These tests expect normal completion: a host arithmetic panic is a failure.
#[test]
fn dec_cx_wraps_zero_to_ffff() {
    let mut cpu = Cpu::new();
    cpu.cx = 0;
    cpu.flags = 0x0041; // Start with zero and carry set.
    let mut memory = [0x49];

    let result = cpu.step(&mut memory).expect("DEC CX should wrap at zero");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.cx, 0xFFFF);
    assert_eq!(cpu.ip, 1);
    assert_eq!(cpu.flags & 0x0040, 0, "the result is nonzero");
    assert_eq!(cpu.flags & 0x0001, 1, "DEC must preserve carry");
}

#[test]
fn jnz_forward_crosses_signed_16_bit_boundary() {
    let mut cpu = Cpu::new();
    cpu.ip = 0x7FFD;
    cpu.flags = 0; // JNZ is taken.
    let mut memory = vec![0; 0x10000];
    memory[0x7FFD..0x7FFF].copy_from_slice(&[0x75, 0x01]);

    // After fetching, IP is 0x7FFF. Adding one must produce 0x8000,
    // even though that calculation would overflow an i16.
    let result = cpu.step(&mut memory).expect("forward JNZ should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x8000);
    assert_eq!(cpu.flags, 0);
}

#[test]
fn jnz_backward_crosses_signed_16_bit_boundary() {
    let mut cpu = Cpu::new();
    cpu.ip = 0x7FFE;
    cpu.flags = 0;
    let mut memory = vec![0; 0x10000];
    memory[0x7FFE..0x8000].copy_from_slice(&[0x75, 0xFF]);

    // After fetching, IP is 0x8000. A displacement of -1 gives 0x7FFF.
    let result = cpu.step(&mut memory).expect("backward JNZ should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x7FFF);
    assert_eq!(cpu.flags, 0);
}

#[test]
fn jnz_target_wraps_at_16_bit_address_limits() {
    // Neither instruction fetch crosses 0xFFFF; these isolate target arithmetic.
    for (start_ip, displacement, expected_ip) in [(0xFFFC, 0x02, 0x0000), (0x0000, 0xFD, 0xFFFF)] {
        let mut cpu = Cpu::new();
        cpu.ip = start_ip;
        cpu.flags = 0;
        let mut memory = vec![0; 0x10000];
        let start = start_ip as usize;
        memory[start..start + 2].copy_from_slice(&[0x75, displacement]);

        let result = cpu.step(&mut memory).expect("JNZ target should wrap");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.ip, expected_ip, "starting IP = {start_ip:#06X}");
        assert_eq!(cpu.flags, 0);
    }
}

#[test]
fn mov_al_preserves_ah() {
    for (immediate, expected_ax) in [(0x00, 0xAB00), (0x56, 0xAB56), (0xFF, 0xABFF)] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xABCD;
        cpu.ip = 2;
        let mut memory = [0, 0, 0xB0, immediate];

        let result = cpu.step(&mut memory).expect("MOV AL should succeed");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.ax, expected_ax, "immediate = {immediate:#04X}");
        assert_eq!(cpu.ip, 4);
    }
}

#[test]
fn mov_ah_preserves_al() {
    for (immediate, expected_ax) in [(0x00, 0x00CD), (0x56, 0x56CD), (0xFF, 0xFFCD)] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xABCD;
        cpu.ip = 2;
        let mut memory = [0, 0, 0xB4, immediate];

        let result = cpu.step(&mut memory).expect("MOV AH should succeed");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.ax, expected_ax, "immediate = {immediate:#04X}");
        assert_eq!(cpu.ip, 4);
    }
}

#[test]
fn fetch_word_reads_little_endian_and_advances_ip() {
    let mut cpu = Cpu::new();
    cpu.ip = 2;
    let memory = [0xAA, 0xBB, 0x34, 0x12, 0xCC];

    let word = cpu.fetch_next_word_and_advance_ip(&memory);

    assert_eq!(word, 0x1234);
    assert_eq!(cpu.ip, 4);
}
