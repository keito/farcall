use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// Intel 286 BOUND: signed, inclusive word limits; #BR=5, register operand #UD=6,
// and real-mode four-byte operand crossing FFFFh #GP=13. Fault IP includes prefixes.
// https://bitsavers.trailing-edge.com/components/intel/80286/210498-005_80286_and_80287_Programmers_Reference_Manual_1987.pdf
// On 8088, 62 aliases 72 (JB); the next byte is a signed branch displacement.
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
    ] = [5, 6, 7, 0x0200, 0x0800, 0x0400, 0x0010, 0x0020];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
    cpu.ip = 0x0100;
    (cpu, vec![0xCC; 0x60000])
}

fn write_bounds(memory: &mut [u8], address: usize, lower: u16, upper: u16) {
    memory[address..address + 2].copy_from_slice(&lower.to_le_bytes());
    memory[address + 2..address + 4].copy_from_slice(&upper.to_le_bytes());
}

fn check_step(cpu: &mut Cpu, memory: &mut [u8], vector: Option<u8>, expected_ip: u16) {
    let start_ip = cpu.ip;
    let saved_registers = registers(cpu);
    let flags = cpu.flags;
    let original_memory = memory.to_vec();
    let result = cpu
        .step(memory)
        .expect("BOUND/JB must not fail on the host");
    let actual_vector = match result {
        CpuStepResult::Continue => None,
        CpuStepResult::Interrupt(vector) => Some(vector),
        _ => panic!("BOUND/JB must not request port I/O"),
    };
    assert_eq!(
        actual_vector, vector,
        "CS:IP={:04X}:{start_ip:04X}; None means Continue",
        cpu.cs
    );
    assert_eq!(cpu.ip, expected_ip);
    assert_eq!(
        registers(cpu),
        saved_registers,
        "reporting a fault does not push its frame yet"
    );
    assert_eq!(cpu.flags, flags, "BOUND and JB preserve every flag");
    assert!(
        memory == original_memory,
        "checking bounds or reporting a fault does not write memory"
    );
}

#[test]
fn i286_bound_signed_comparisons_include_both_endpoints_and_all_registers() {
    // Literal encodings and expected outcomes, including negative and reversed
    // ranges. No unsigned comparisons or host subtraction produce this oracle.
    for (lower, upper, index, vector) in [
        (0xFFF6, 0x000A, 0xFFF5, Some(5)), // -11 below [-10,10].
        (0xFFF6, 0x000A, 0xFFF6, None),
        (0xFFF6, 0x000A, 0xFFFE, None), // -2 is inside [-10,10].
        (0xFFF6, 0x000A, 0x0000, None),
        (0xFFF6, 0x000A, 0x000A, None),
        (0xFFF6, 0x000A, 0x000B, Some(5)),
        (0xFFEC, 0xFFF6, 0xFFEB, Some(5)), // -21 below [-20,-10].
        (0xFFEC, 0xFFF6, 0xFFEC, None),
        (0xFFEC, 0xFFF6, 0xFFF1, None),
        (0xFFEC, 0xFFF6, 0xFFF6, None),
        (0xFFEC, 0xFFF6, 0xFFF7, Some(5)),
        (0x8000, 0x7FFF, 0x8000, None), // Full signed range.
        (0x8000, 0x7FFF, 0xFFFF, None),
        (0x8000, 0x7FFF, 0x0000, None),
        (0x8000, 0x7FFF, 0x7FFF, None),
        (0x8000, 0xFFFF, 0x8000, None), // Negative half of the range.
        (0x8000, 0xFFFF, 0xFFFF, None),
        (0x8000, 0xFFFF, 0x0000, Some(5)),
        (0x0000, 0x7FFF, 0x8000, Some(5)),
        (0x0000, 0x7FFF, 0xFFFF, Some(5)),
        (0xFFFF, 0xFFFF, 0xFFFE, Some(5)), // Singleton [-1,-1].
        (0xFFFF, 0xFFFF, 0xFFFF, None),
        (0xFFFF, 0xFFFF, 0x0000, Some(5)),
        (0x000A, 0xFFF6, 0x0000, Some(5)), // Reversed bounds admit no value.
        (0x000A, 0xFFF6, 0xFFF6, Some(5)),
        (0x000A, 0xFFF6, 0x000A, Some(5)),
    ] {
        for reg in 0..8 {
            for flags in [0x0002, 0x0FD7] {
                let (mut cpu, mut memory) = setup(CpuModel::I80286);
                // Register field order AX,CX,DX,BX,SP,BP,SI,DI.
                let mut values = registers(&cpu);
                values[reg] = index;
                // Assign the selected register without depending on Reg16 decoding.
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = values[..8].try_into().unwrap();
                cpu.flags = flags;
                memory[0x10100..0x10104].copy_from_slice(&[0x62, (reg as u8) << 3 | 6, 0, 2]);
                write_bounds(&mut memory, 0x20200, lower, upper);
                check_step(
                    &mut cpu,
                    &mut memory,
                    vector,
                    if vector.is_some() { 0x0100 } else { 0x0104 },
                );
            }
        }
    }
}

#[test]
fn i286_bound_memory_modes_use_correct_segments_displacements_and_index_register() {
    // (ModR/M without reg bits, displacement bytes, literal effective offset,
    // default SS rather than DS). All index registers retain their seeded values.
    let forms: &[(u8, &[u8], u16, bool)] = &[
        (0x00, &[], 0x0210, false),
        (0x01, &[], 0x0220, false),
        (0x02, &[], 0x0410, true),
        (0x03, &[], 0x0420, true),
        (0x04, &[], 0x0010, false),
        (0x05, &[], 0x0020, false),
        (0x06, &[0x00, 0x03], 0x0300, false),
        (0x07, &[], 0x0200, false),
        (0x40, &[0xF0], 0x0200, false),
        (0x41, &[0xF0], 0x0210, false),
        (0x42, &[0xF0], 0x0400, true),
        (0x43, &[0xF0], 0x0410, true),
        (0x44, &[0xF0], 0x0000, false),
        (0x45, &[0xF0], 0x0010, false),
        (0x46, &[0xF0], 0x03F0, true),
        (0x47, &[0xF0], 0x01F0, false),
        (0x80, &[0x34, 0x12], 0x1444, false),
        (0x81, &[0x34, 0x12], 0x1454, false),
        (0x82, &[0x34, 0x12], 0x1644, true),
        (0x83, &[0x34, 0x12], 0x1654, true),
        (0x84, &[0x34, 0x12], 0x1244, false),
        (0x85, &[0x34, 0x12], 0x1254, false),
        (0x86, &[0x34, 0x12], 0x1634, true),
        (0x87, &[0x34, 0x12], 0x1434, false),
        (0x86, &[0x00, 0xFC], 0x0000, true), // BP+FC00h wraps effective offset.
    ];
    for &(form, displacement, offset, default_ss) in forms {
        for reg in 0..8 {
            for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
                for fail in [false, true] {
                    let (mut cpu, mut memory) = setup(CpuModel::I80286);
                    cpu.flags = 0x0FD7;
                    let index = registers(&cpu)[reg];
                    let segment = match prefix {
                        Some(0x26) => 0x4000,
                        Some(0x2E) => 0x1000,
                        Some(0x36) => 0x3000,
                        Some(0x3E) => 0x2000,
                        None if default_ss => 0x3000,
                        None => 0x2000,
                        _ => unreachable!(),
                    };
                    let instruction: Vec<u8> = prefix
                        .into_iter()
                        .chain([0x62, form | (reg as u8) << 3])
                        .chain(displacement.iter().copied())
                        .collect();
                    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
                    let limit = index + u16::from(fail);
                    write_bounds(
                        &mut memory,
                        segment * 16 + usize::from(offset),
                        limit,
                        limit,
                    );
                    let next_ip = if fail {
                        0x0100
                    } else {
                        0x0100 + instruction.len() as u16
                    };
                    check_step(
                        &mut cpu,
                        &mut memory,
                        if fail { Some(5) } else { None },
                        next_ip,
                    );
                }
            }
        }
    }
}

#[test]
fn i286_bound_four_byte_operand_is_valid_through_offset_fffc() {
    for offset in [0x0FFCu16, 0x0FFD, 0x1000, 0x7FFF, 0x8000, 0xFFFB, 0xFFFC] {
        for fail in [false, true] {
            let (mut cpu, mut memory) = setup(CpuModel::I80286);
            cpu.ax = if fail { 11 } else { 10 };
            cpu.flags = 0x0FD7;
            let [low, high] = offset.to_le_bytes();
            memory[0x10100..0x10104].copy_from_slice(&[0x62, 6, low, high]);
            write_bounds(&mut memory, 0x20000 + usize::from(offset), 0xFFF6, 10);
            check_step(
                &mut cpu,
                &mut memory,
                if fail { Some(5) } else { None },
                if fail { 0x0100 } else { 0x0104 },
            );
        }
    }
}

#[test]
fn i286_bound_register_second_operands_raise_ud_and_restart_before_prefixes() {
    for reg in 0..8 {
        for rm in 0..8 {
            for prefixes in [&[][..], &[0x26], &[0x26, 0x2E, 0x36, 0x3E]] {
                let (mut cpu, mut memory) = setup(CpuModel::I80286);
                cpu.flags = 0x0FD7;
                let instruction: Vec<u8> = prefixes
                    .iter()
                    .copied()
                    .chain([0x62, 0xC0 | reg << 3 | rm])
                    .collect();
                memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
                // The encoding ends at available RAM, so #UD must not read a
                // displacement, limits, or a purported following instruction.
                memory.truncate(0x10100 + instruction.len());
                check_step(&mut cpu, &mut memory, Some(6), 0x0100);
            }
        }
    }
}

#[test]
fn i286_bound_offsets_fffd_through_ffff_raise_gp_before_reading_limits() {
    for offset in [0xFFFDu16, 0xFFFE, 0xFFFF] {
        for prefixes in [&[][..], &[0x36], &[0x26, 0x2E, 0x3E]] {
            let (mut cpu, mut memory) = setup(CpuModel::I80286);
            cpu.flags = 0x0FD7;
            let [low, high] = offset.to_le_bytes();
            let instruction: Vec<u8> = prefixes
                .iter()
                .copied()
                .chain([0x62, 6, low, high])
                .collect();
            memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
            // None of the bounds are backed by host RAM; architectural #GP
            // must take priority over read_word's host bounds error.
            memory.truncate(0x10100 + instruction.len());
            check_step(&mut cpu, &mut memory, Some(13), 0x0100);
        }
    }
}

#[test]
fn i286_bound_consumes_exact_instruction_bytes_without_an_immediate() {
    for prefixes in [&[][..], &[0x26]] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        cpu.ax = 0xFFFE;
        cpu.ip = 0x4000 - (prefixes.len() as u16 + 4);
        let instruction: Vec<u8> = prefixes.iter().copied().chain([0x62, 6, 0, 1]).collect();
        let mut memory = vec![0xCC; 0x4000];
        memory[usize::from(cpu.ip)..].copy_from_slice(&instruction);
        write_bounds(&mut memory, 0x0100, 0xFFF6, 10);
        check_step(&mut cpu, &mut memory, None, 0x4000);
    }
}

#[test]
fn i286_bound_reports_truncated_bounds_as_host_memory_errors() {
    for length in [0x0100, 0x0101, 0x0102, 0x0103] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; length];
        memory[..4].copy_from_slice(&[0x62, 6, 0, 1]);
        let saved_registers = registers(&cpu);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), saved_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
        // Host allocation errors do not specify architectural IP rollback.
    }
}

#[test]
fn i286_bound_fault_entry_and_iret_save_the_instruction_start_for_br_ud_and_gp() {
    for (instruction, vector) in [
        (&[0x26, 0x3E, 0x62, 6, 0, 2][..], 5u8),
        (&[0x26, 0x3E, 0x62, 0xC0][..], 6),
        (&[0x26, 0x3E, 0x62, 6, 0xFD, 0xFF][..], 13),
    ] {
        let (mut cpu, mut memory) = setup(CpuModel::I80286);
        cpu.ax = 11;
        cpu.flags = 0x0FD7;
        memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
        write_bounds(&mut memory, 0x20200, 0xFFF6, 10);
        let ivt = usize::from(vector) * 4;
        memory[ivt..ivt + 4].copy_from_slice(&[0x00, 0x04, 0x00, 0x50]);
        memory[0x50400] = 0xCF; // IRET.
        let saved_registers = registers(&cpu);
        check_step(&mut cpu, &mut memory, Some(vector), 0x0100);
        let mut expected_memory = memory.clone();
        expected_memory[0x307FA..0x30800].copy_from_slice(&[
            0x00, 0x01, // IP points to first prefix.
            0x00, 0x10, // Original CS.
            0xD7, 0x0F, // Original FLAGS; no real-mode error-code word.
        ]);

        cpu.enter_interrupt(&mut memory, vector).unwrap();

        assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x5000, 0x0400, 0x07FA));
        assert_eq!(cpu.flags, 0x0CD7, "entry clears IF/TF after saving FLAGS");
        assert!(memory == expected_memory);
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
        assert_eq!(cpu.ip, 0x0100, "IRET retries BOUND including its prefixes");
        assert_eq!(registers(&cpu), saved_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == expected_memory);
    }
}

#[test]
fn i286_bound_guest_handler_can_fix_the_index_and_iret_to_retry() {
    let (mut cpu, mut memory) = setup(CpuModel::I80286);
    cpu.ax = 11;
    cpu.flags = 0x0FD7;
    memory[0x10100..0x10105].copy_from_slice(&[0x3E, 0x62, 6, 0, 2]);
    write_bounds(&mut memory, 0x20200, 0xFFF6, 10);
    memory[20..24].copy_from_slice(&[0, 4, 0, 0x50]);
    memory[0x50400..0x50404].copy_from_slice(&[0xB8, 0, 0, 0xCF]); // MOV AX,0; IRET.
    let mut expected_registers = registers(&cpu);
    expected_registers[0] = 0;
    check_step(&mut cpu, &mut memory, Some(5), 0x0100);
    cpu.enter_interrupt(&mut memory, 5).unwrap();
    // Execute exactly MOV, IRET, BOUND; a broken retry cannot loop indefinitely.
    for _ in 0..3 {
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
    }
    assert_eq!(registers(&cpu), expected_registers);
    assert_eq!(cpu.ip, 0x0105);
    assert_eq!(cpu.flags, 0x0FD7);
}

#[test]
fn i8088_62_alias_is_jb_using_only_cf_and_a_signed_displacement() {
    check_jb(false, 0x62);
}

#[test]
fn i8088_72_remains_jb() {
    check_jb(false, 0x72);
}

#[test]
fn i286_72_remains_jb() {
    check_jb(true, 0x72);
}

fn check_jb(is_286: bool, opcode: u8) {
    for (ip, displacement, taken_ip, next_ip) in [
        (0x0100u16, 0x00, 0x0102, 0x0102),
        (0x0100, 0x7F, 0x0181, 0x0102),
        (0x0100, 0x80, 0x0082, 0x0102),
        (0x0100, 0xFE, 0x0100, 0x0102), // Opcode-like FE is a displacement, not ModR/M.
        (0x7FFD, 0x01, 0x8000, 0x7FFF),
        (0x0000, 0xFD, 0xFFFF, 0x0002),
        (0xFFFC, 0x02, 0x0000, 0xFFFE),
        (0xFFFF, 0xFE, 0xFFFF, 0x0001),
    ] {
        if is_286 && matches!(ip, 0x0000 | 0xFFFC | 0xFFFF) {
            continue;
        }
        for other_flags in [0x0000, 0x0FD4] {
            for carry in [false, true] {
                let (mut cpu, mut memory) = setup(if is_286 {
                    CpuModel::I80286
                } else {
                    CpuModel::I8088
                });
                cpu.ip = ip;
                cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | other_flags | u16::from(carry);
                memory[0x10000 + usize::from(ip)] = opcode;
                memory[0x10000 + usize::from(ip.wrapping_add(1))] = displacement;
                check_step(
                    &mut cpu,
                    &mut memory,
                    None,
                    if carry { taken_ip } else { next_ip },
                );
            }
        }
    }
}

#[test]
fn i8088_62_alias_consumes_exactly_one_displacement_byte() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ip = 0x3FFE;
    cpu.flags = 0xF003;
    let mut memory = vec![0xCC; 0x4000];
    memory[0x3FFE..].copy_from_slice(&[0x62, 0x80]);
    // Taken branch to 3F80h. Treating 80 as ModR/M would need two missing bytes.
    check_step(&mut cpu, &mut memory, None, 0x3F80);
}
