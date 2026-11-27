use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// Intel ENTER: save BP, copy the enclosing display for nonzero nesting, then
// set BP and reserve locals. FLAGS unchanged. Formal 286 algorithm: page B-40.
// https://bitsavers.trailing-edge.com/components/intel/80286/210498-005_80286_and_80287_Programmers_Reference_Manual_1987.pdf
// The nesting byte is reduced modulo 32 on 286, so 00/20/.../E0 all mean zero.
// https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2a-manual.pdf
// On 8088, C8 instead aliases CA: RETF imm16 (pop IP, pop CS, discard imm16 bytes).
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn setup(model: CpuModel) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(model, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0x1234, 0x2345, 0x3456, 0x4567, 0x1000, 0x5678, 0x6789, 0x789A,
    ];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
    cpu.ip = 0x0100;
    (cpu, vec![0xCC; 0x50000])
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn i286_enter_zero_builds_exact_frame_and_reserves_unsigned_local_bytes() {
    // (initial SP, allocation, frame BP, final SP). These frames and local
    // allocations fit inside the stack segment; segment-limit faults are separate.
    for (sp, allocation, frame_bp, final_sp) in [
        (0x1000, 0x0000u16, 0x0FFE, 0x0FFE),
        (0x1000, 0x0010, 0x0FFE, 0x0FEE),
        (0x1236, 0x1234, 0x1234, 0x0000),
        (0xFFFE, 0x7FFF, 0xFFFC, 0x7FFD),
        (0xFFFE, 0x8000, 0xFFFC, 0x7FFC),
        (0xFFFE, 0xFFFC, 0xFFFC, 0x0000),
        (0x0002, 0x0000, 0x0000, 0x0000),
        (0x1001, 0x0001, 0x0FFF, 0x0FFE),
    ] {
        for saved_bp in [0x0000u16, 0x5678, 0xFFFF] {
            for flags in [0x0002, 0x0FD7] {
                for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
                    let (mut cpu, mut memory) = setup(CpuModel::I80286);
                    cpu.sp = sp;
                    cpu.bp = saved_bp;
                    cpu.flags = flags;
                    let [low, high] = allocation.to_le_bytes();
                    let instruction: Vec<u8> =
                        prefix.into_iter().chain([0xC8, low, high, 0]).collect();
                    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
                    let mut expected_registers = registers(&cpu);
                    expected_registers[4] = final_sp;
                    expected_registers[5] = frame_bp;
                    let mut expected_memory = memory.clone();
                    let address = 0x30000 + usize::from(frame_bp);
                    expected_memory[address..address + 2].copy_from_slice(&saved_bp.to_le_bytes());

                    step(&mut cpu, &mut memory);

                    assert_eq!(
                        registers(&cpu),
                        expected_registers,
                        "SP={sp:04X}, allocation={allocation:04X}, old BP={saved_bp:04X}, prefix={prefix:?}"
                    );
                    assert_eq!(
                        cpu.ip,
                        0x0100 + instruction.len() as u16,
                        "consume both immediates"
                    );
                    assert_eq!(cpu.flags, flags, "ENTER preserves every flag");
                    assert!(
                        memory == expected_memory,
                        "write only saved BP to SS; leave local storage intact"
                    );
                }
            }
        }
    }
}

#[test]
fn i286_enter_masks_high_three_bits_of_zero_nesting_level() {
    for level in [0x00, 0x20, 0x40, 0x60, 0x80, 0xA0, 0xC0, 0xE0] {
        let (mut cpu, mut memory) = setup(CpuModel::I80286);
        memory[0x10100..0x10104].copy_from_slice(&[0xC8, 0x10, 0, level]);
        let flags = cpu.flags;
        let mut expected_registers = registers(&cpu);
        expected_registers[4] = 0x0FEE;
        expected_registers[5] = 0x0FFE;
        let mut expected_memory = memory.clone();
        expected_memory[0x30FFE..0x31000].copy_from_slice(&[0x78, 0x56]);

        step(&mut cpu, &mut memory);

        assert_eq!(
            registers(&cpu),
            expected_registers,
            "encoded nesting={level:02X}"
        );
        assert_eq!(cpu.ip, 0x0104);
        assert_eq!(cpu.flags, flags);
        assert!(
            memory == expected_memory,
            "masked level zero copies no display pointers"
        );
    }
}

#[test]
fn i286_enter_zero_saves_old_bp_without_reading_through_it() {
    let mut cpu = Cpu::new(CpuModel::I80286, 0);
    cpu.sp = 0x0102;
    cpu.bp = 0xF000; // Outside supplied RAM: level zero must not dereference it.
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x0102];
    memory[..4].copy_from_slice(&[0xC8, 0x10, 0, 0]);
    let mut expected_memory = memory.clone();
    expected_memory[0x0100..].copy_from_slice(&[0x00, 0xF0]);

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.bp, cpu.sp, cpu.ip), (0x0100, 0x00F0, 4));
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == expected_memory);
}

#[test]
fn i286_enter_zero_consumes_exactly_four_bytes() {
    let mut cpu = Cpu::new(CpuModel::I80286, 0);
    cpu.ip = 0x3FFC;
    cpu.sp = 0x0102;
    cpu.bp = 0x5678;
    let flags = cpu.flags;
    let mut memory = vec![0xCC; 0x4000];
    // Nesting byte is the last available byte: no ModR/M or fifth byte exists.
    memory[0x3FFC..].copy_from_slice(&[0xC8, 0x10, 0, 0]);
    let mut expected_memory = memory.clone();
    expected_memory[0x0100..0x0102].copy_from_slice(&[0x78, 0x56]);

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.ip, cpu.bp, cpu.sp), (0x4000, 0x0100, 0x00F0));
    assert_eq!(cpu.flags, flags);
    assert!(memory == expected_memory);
}

#[test]
fn i286_enter_zero_reports_a_truncated_stack_word_without_partial_writes() {
    for length in [0x0100, 0x0101] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        cpu.sp = 0x0102;
        cpu.bp = 0x5678;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; length];
        memory[..4].copy_from_slice(&[0xC8, 0, 0, 0]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(cpu.flags, 0x0FD7);
        assert!(
            memory == original_memory,
            "failed word write must not change its low byte"
        );
        // Host RAM bounds errors do not specify architectural register rollback.
    }
}

#[test]
fn i286_enter_zero_leave_ret_round_trip_restores_the_guest_frame() {
    for flags in [0x0002, 0x0FD7] {
        let (mut cpu, mut memory) = setup(CpuModel::I80286);
        cpu.flags = flags;
        memory[0x10100..0x10106].copy_from_slice(&[
            0xC8, 0x10, 0, 0,    // ENTER 16,0.
            0xC9, // LEAVE.
            0xC3, // RET.
        ]);
        memory[0x31000..0x31002].copy_from_slice(&[0x79, 0x24]);
        let original_registers = registers(&cpu);
        let mut expected_memory = memory.clone();
        expected_memory[0x30FFE..0x31000].copy_from_slice(&[0x78, 0x56]);

        step(&mut cpu, &mut memory);

        let mut expected = original_registers;
        expected[4] = 0x0FEE;
        expected[5] = 0x0FFE;
        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.ip, 0x0104);
        assert_eq!(cpu.flags, flags);
        assert!(memory == expected_memory);

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.ip, 0x0105);
        assert_eq!(cpu.flags, flags);

        step(&mut cpu, &mut memory);

        expected = original_registers;
        expected[4] = 0x1002;
        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.ip, 0x2479);
        assert_eq!(cpu.flags, flags);
        assert!(
            memory == expected_memory,
            "LEAVE and RET do not erase the saved frame"
        );
    }
}

#[test]
fn i8088_c8_alias_pops_ip_cs_then_discards_unsigned_immediate_bytes() {
    check_far_return(false, 0xC8);
}

#[test]
fn i8088_ca_remains_far_return_with_immediate_cleanup() {
    check_far_return(false, 0xCA);
}

#[test]
fn i286_ca_remains_far_return_with_immediate_cleanup() {
    check_far_return(true, 0xCA);
}

fn check_far_return(is_286: bool, opcode: u8) {
    for (sp, cleanup, final_sp) in [
        (0x1000, 0x0000u16, 0x1004),
        (0x1000, 0x0004, 0x1008),
        (0x1000, 0x1234, 0x2238),
        (0x1000, 0x8000, 0x9004),
        (0x1000, 0xFFFF, 0x1003),
        (0xFFFC, 0x0004, 0x0004),
        (0x0000, 0xFFFF, 0x0003),
        // Only 8088 permits stack words straddling the segment boundary.
        (0xFFFE, 0x0004, 0x0006),
        (0xFFFF, 0x0004, 0x0007),
    ] {
        if is_286 && sp > 0xFFFC {
            continue;
        }
        let ips: &[u16] = if is_286 {
            &[0x0100]
        } else {
            &[0x0100, 0xFFFD, 0xFFFE, 0xFFFF]
        };
        for &ip in ips {
            for pattern in [0x0000, 0x0FD5] {
                for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
                    let (mut cpu, mut memory) = setup(if is_286 {
                        CpuModel::I80286
                    } else {
                        CpuModel::I8088
                    });
                    cpu.sp = sp;
                    cpu.ip = ip;
                    cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | pattern;
                    let flags = cpu.flags;
                    let [low, high] = cleanup.to_le_bytes();
                    for (index, byte) in prefix.into_iter().chain([opcode, low, high]).enumerate() {
                        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
                    }
                    for (index, byte) in [0x79, 0x24, 0x67, 0x45].into_iter().enumerate() {
                        memory[0x30000 + usize::from(sp.wrapping_add(index as u16))] = byte;
                    }
                    let original_memory = memory.clone();
                    let mut expected = registers(&cpu);
                    expected[4] = final_sp;
                    expected[8] = 0x4567;

                    step(&mut cpu, &mut memory);

                    assert_eq!(
                        registers(&cpu),
                        expected,
                        "opcode={opcode:02X}, SP={sp:04X}, cleanup={cleanup:04X}, IP={ip:04X}, prefix={prefix:?}"
                    );
                    assert_eq!(
                        cpu.ip, 0x2479,
                        "first pop is IP, second is CS; BP stays unchanged"
                    );
                    assert_eq!(cpu.flags, flags, "RETF preserves every flag");
                    assert!(
                        memory == original_memory,
                        "stack cleanup changes SP without writing memory"
                    );
                }
            }
        }
    }
}

#[test]
fn far_return_immediate_consumes_three_bytes_including_8088_c8_alias() {
    for (is_286, opcode) in [(false, 0xC8), (false, 0xCA), (true, 0xCA)] {
        let mut cpu = Cpu::new(
            if is_286 {
                CpuModel::I80286
            } else {
                CpuModel::I8088
            },
            0,
        );
        cpu.ip = 0x3FFD;
        cpu.sp = 0x0100;
        cpu.bp = 0x0200;
        let flags = cpu.flags;
        let mut memory = vec![0xCC; 0x4000];
        // No fourth (nesting) byte exists: 8088 C8 must only fetch imm16.
        memory[0x3FFD..].copy_from_slice(&[opcode, 0x34, 0x12]);
        memory[0x0100..0x0104].copy_from_slice(&[0x42, 0x00, 0x78, 0x56]);
        let original_memory = memory.clone();

        step(&mut cpu, &mut memory);

        assert_eq!(
            (cpu.cs, cpu.ip, cpu.sp, cpu.bp),
            (0x5678, 0x0042, 0x1338, 0x0200)
        );
        assert_eq!(cpu.flags, flags);
        assert!(memory == original_memory);
    }
}

// Words at SS:1FFE, SS:1FFC, ... below old BP=2000h, in copy order.
// Distinct values expose reversed copies, repeated reads, or linked-list walks.
const DISPLAY: [u16; 30] = [
    0xA101, 0xB202, 0xC303, 0xD404, 0xE505, 0xF606, 0x1707, 0x2808, 0x3909, 0x4A0A, 0x5B0B, 0x6C0C,
    0x7D0D, 0x8E0E, 0x9F0F, 0xA010, 0xB111, 0xC212, 0xD313, 0xE414, 0xF515, 0x1616, 0x2717, 0x3818,
    0x4919, 0x5A1A, 0x6B1B, 0x7C1C, 0x8D1D, 0x9E1E,
];

fn check_nested_enter(
    encoded_level: u8,
    allocation: u16,
    sp: u16,
    prefix: Option<u8>,
    flags: u16,
) -> (Cpu, Vec<u8>) {
    // Use the architectural modulo definition, rather than the decoder's mask.
    let level = usize::from(encoded_level % 32);
    assert!((1..=31).contains(&level));
    let (mut cpu, mut memory) = setup(CpuModel::I80286);
    cpu.sp = sp;
    cpu.bp = 0x2000;
    cpu.flags = flags;
    let [low, high] = allocation.to_le_bytes();
    let program: Vec<u8> = prefix
        .into_iter()
        .chain([
            0xC8,
            low,
            high,
            encoded_level, // ENTER allocation,encoded_level.
            0xC9,          // LEAVE.
            0xC3,          // RET.
        ])
        .collect();
    memory[0x10100..0x10100 + program.len()].copy_from_slice(&program);
    for (index, pointer) in DISPLAY.iter().enumerate() {
        let address = 0x31FFE - index * 2;
        memory[address..address + 2].copy_from_slice(&pointer.to_le_bytes());
    }
    memory[0x30000 + usize::from(sp)..0x30002 + usize::from(sp)].copy_from_slice(&[0x79, 0x24]);

    let frame_bp = sp - 2;
    let frame_bottom = usize::from(sp) - 2 * (level + 1);
    let final_sp = u16::try_from(frame_bottom).unwrap() - allocation;
    let mut expected_registers = registers(&cpu);
    expected_registers[4] = final_sp;
    expected_registers[5] = frame_bp;
    let mut expected_memory = memory.clone();
    // Describe the completed frame in ascending memory order, independently of
    // the instruction's decrement/read/push loop. The unused display stays intact.
    let frame_words: Vec<u16> = std::iter::once(frame_bp)
        .chain(DISPLAY[..level - 1].iter().rev().copied())
        .chain([0x2000])
        .collect();
    for (index, word) in frame_words.iter().enumerate() {
        let address = 0x30000 + frame_bottom + index * 2;
        expected_memory[address..address + 2].copy_from_slice(&word.to_le_bytes());
    }

    step(&mut cpu, &mut memory);

    assert_eq!(
        registers(&cpu),
        expected_registers,
        "level={encoded_level:02X}, allocation={allocation:04X}, SP={sp:04X}, prefix={prefix:?}"
    );
    assert_eq!(cpu.ip, 0x0104 + u16::from(prefix.is_some()));
    assert_eq!(cpu.flags, flags, "nested ENTER preserves every flag");
    assert!(
        memory == expected_memory,
        "copy exactly level-1 SS words, append the new frame pointer, and leave locals untouched; level={encoded_level:02X}"
    );
    (cpu, memory)
}

#[test]
fn i286_enter_nonzero_levels_build_exact_frames_with_flags_and_segment_overrides() {
    for level in 1..=31 {
        for allocation in [0x0000, 0x0010] {
            for flags in [0x0002, 0x0FD7] {
                for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
                    check_nested_enter(level, allocation, 0x1000, prefix, flags);
                }
            }
        }
    }
}

#[test]
fn i286_enter_masks_every_encoding_of_nonzero_nesting_levels() {
    for encoded in 0..=255u8 {
        if encoded % 32 != 0 {
            check_nested_enter(encoded, 0x0010, 0x1000, None, 0x0FD7);
        }
    }
}

#[test]
fn i286_enter_nested_frames_can_end_exactly_at_stack_offset_zero() {
    // Pushes alone reach zero, or local allocation consumes the remaining space.
    for (level, sp, allocation) in [
        (1, 0x0004, 0x0000),
        (3, 0x0008, 0x0000),
        (31, 0x0040, 0x0000),
        (3, 0x0018, 0x0010),
        (31, 0xFFFE, 0xFFBE), // Large unsigned allocation, without segment crossing.
    ] {
        let (cpu, _) = check_nested_enter(level, allocation, sp, None, 0x0FD7);
        assert_eq!(cpu.sp, 0);
    }
}

#[test]
fn i286_enter_level_one_pushes_its_frame_pointer_without_reading_old_display() {
    let mut cpu = Cpu::new(CpuModel::I80286, 0);
    cpu.sp = 0x0102;
    cpu.bp = 0xF000; // Neither old BP nor old BP-2 is backed by RAM.
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x0102];
    memory[..4].copy_from_slice(&[0xC8, 0x10, 0, 1]);
    let mut expected_memory = memory.clone();
    expected_memory[0x00FE..0x0102].copy_from_slice(&[
        0x00, 0x01, // Extra frame pointer = 0100h.
        0x00, 0xF0, // Saved BP = F000h.
    ]);

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.ip, cpu.bp, cpu.sp), (4, 0x0100, 0x00EE));
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == expected_memory);
}

#[test]
fn i286_enter_level_three_copies_the_two_previous_display_words_in_order() {
    let (mut cpu, mut memory) = setup(CpuModel::I80286);
    cpu.bp = 0x2000;
    cpu.flags = 0x0FD7;
    memory[0x10100..0x10104].copy_from_slice(&[0xC8, 0x10, 0, 3]);
    memory[0x31FFC..0x32000].copy_from_slice(&[0xBB, 0xBB, 0xAA, 0xAA]);
    let mut expected_memory = memory.clone();
    expected_memory[0x30FF8..0x31000].copy_from_slice(&[
        0xFE, 0x0F, // New frame pointer.
        0xBB, 0xBB, // SS:[old BP-4].
        0xAA, 0xAA, // SS:[old BP-2].
        0x00, 0x20, // Saved BP.
    ]);

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.ip, cpu.bp, cpu.sp), (0x0104, 0x0FFE, 0x0FE8));
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == expected_memory);
}

#[test]
fn i286_enter_reads_overlapping_display_words_after_each_preceding_push() {
    for (old_bp, level, final_sp, expected_bytes) in [
        (
            0x1002,
            3,
            0x0FE8,
            &[0xFE, 0x0F, 0x02, 0x10, 0xAA, 0xAA, 0x02, 0x10][..],
        ),
        (
            0x1000,
            4,
            0x0FE6,
            &[0xFE, 0x0F, 0x00, 0x10, 0x00, 0x10, 0x00, 0x10, 0x00, 0x10][..],
        ),
    ] {
        let (mut cpu, mut memory) = setup(CpuModel::I80286);
        cpu.bp = old_bp;
        memory[0x10100..0x10104].copy_from_slice(&[0xC8, 0x10, 0, level]);
        memory[0x31000..0x31002].copy_from_slice(&[0xAA, 0xAA]);
        let flags = cpu.flags;
        let mut expected_memory = memory.clone();
        let address = 0x31000 - expected_bytes.len();
        expected_memory[address..0x31000].copy_from_slice(expected_bytes);

        step(&mut cpu, &mut memory);

        assert_eq!((cpu.ip, cpu.bp, cpu.sp), (0x0104, 0x0FFE, final_sp));
        assert_eq!(cpu.flags, flags);
        assert!(
            memory == expected_memory,
            "do not snapshot the display before pushing, or overwrite BP with frame_ptr before copying"
        );
    }
}

#[test]
fn i286_enter_display_offset_decrements_wrap_without_straddling_stack_words() {
    let (mut cpu, mut memory) = setup(CpuModel::I80286);
    cpu.bp = 0x0002;
    memory[0x10100..0x10104].copy_from_slice(&[0xC8, 0, 0, 3]);
    // Read BP-2 = 0000h, then BP-4 = FFFEh. Both word accesses fit the segment.
    memory[0x30000..0x30002].copy_from_slice(&[0xAA, 0xAA]);
    memory[0x3FFFE..0x40000].copy_from_slice(&[0xBB, 0xBB]);
    let flags = cpu.flags;
    let mut expected_memory = memory.clone();
    expected_memory[0x30FF8..0x31000]
        .copy_from_slice(&[0xFE, 0x0F, 0xBB, 0xBB, 0xAA, 0xAA, 0x02, 0x00]);

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.ip, cpu.bp, cpu.sp), (0x0104, 0x0FFE, 0x0FF8));
    assert_eq!(cpu.flags, flags);
    assert!(memory == expected_memory);
}

#[test]
fn i286_enter_nonzero_levels_unwind_with_leave_then_ret() {
    for level in 1..=31 {
        let (mut cpu, mut memory) = check_nested_enter(level, 0x0010, 0x1000, None, 0x0FD7);
        let mut expected_registers = registers(&cpu);
        expected_registers[4] = 0x1000;
        expected_registers[5] = 0x2000;
        let original_memory = memory.clone();

        step(&mut cpu, &mut memory);

        assert_eq!(
            registers(&cpu),
            expected_registers,
            "LEAVE discards the entire display, level={level}"
        );
        assert_eq!(cpu.ip, 0x0105);
        assert_eq!(cpu.flags, 0x0FD7);

        step(&mut cpu, &mut memory);

        expected_registers[4] = 0x1002;
        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, 0x2479);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
    }
}

#[test]
fn i286_enter_reports_truncated_display_source_words_as_memory_errors() {
    for length in [0x0200, 0x0201] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        cpu.sp = 0x0102;
        cpu.bp = 0x0202;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; length];
        memory[..4].copy_from_slice(&[0xC8, 0, 0, 2]);
        let mut expected_memory = memory.clone();
        expected_memory[0x0100..0x0102].copy_from_slice(&[0x02, 0x02]);

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(cpu.flags, 0x0FD7);
        assert!(
            memory == expected_memory,
            "only the initial saved BP is written before the missing display word"
        );
        // These are host RAM bounds errors, not architectural rollback tests.
    }
}
