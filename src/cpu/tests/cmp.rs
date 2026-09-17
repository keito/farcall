use super::{Cpu, CpuError, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss,
        cpu.es,
    ]
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn check_register_pairs(opcode: u8, cases: &[(u16, u16, u16)]) {
    for destination in 0..8 {
        for source in 0..8 {
            for &(lhs, rhs, arithmetic) in cases {
                for initial_flags in [0xF002, 0xFFD7] {
                    let mut cpu = Cpu::new(0);
                    if opcode == 0x38 {
                        // ModR/M byte-register order: AL, CL, DL, BL, AH, CH, DH, BH.
                        // Initialize directly, independently of the decoder/getters.
                        let mut bytes = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
                        bytes[destination] = lhs as u8;
                        bytes[source] = rhs as u8;
                        cpu.ax = u16::from_le_bytes([bytes[0], bytes[4]]);
                        cpu.cx = u16::from_le_bytes([bytes[1], bytes[5]]);
                        cpu.dx = u16::from_le_bytes([bytes[2], bytes[6]]);
                        cpu.bx = u16::from_le_bytes([bytes[3], bytes[7]]);
                    } else {
                        let mut words = [
                            0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A, 0x89AB,
                        ];
                        words[destination] = lhs;
                        words[source] = rhs;
                        [
                            cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                        ] = words;
                    }
                    cpu.flags = initial_flags;
                    let original_registers = registers(&cpu);
                    let mut memory = [opcode, 0xC0 | ((source as u8) << 3) | destination as u8];
                    let original_memory = memory;

                    step(&mut cpu, &mut memory);

                    // Aliased operands always compare equal, even when lhs != rhs.
                    let expected_arithmetic = if source == destination {
                        0x0044
                    } else {
                        arithmetic
                    };
                    assert_eq!(
                        cpu.flags,
                        (initial_flags & !0x08D5) | expected_arithmetic,
                        "opcode={opcode:02X}, source={source}, destination={destination}, lhs={lhs:04X}, rhs={rhs:04X}"
                    );
                    assert_eq!(
                        registers(&cpu),
                        original_registers,
                        "CMP must not store its result"
                    );
                    assert_eq!(cpu.ip, 2);
                    assert_eq!(memory, original_memory);
                }
            }
        }
    }
}

#[test]
fn cmp_38_covers_all_byte_register_pairs_and_subtraction_flags() {
    check_register_pairs(
        0x38,
        &[
            (0x42, 0x42, 0x0044),
            (0x00, 0x01, 0x0095), // CF, PF, AF, SF: unsigned and nibble borrows.
            (0x10, 0x01, 0x0014),
            (0x80, 0x01, 0x0810), // Signed overflow without unsigned borrow.
            (0x7F, 0xFF, 0x0881), // Signed overflow and unsigned borrow.
            (0xFF, 0x00, 0x0084),
            (0x01, 0x00, 0x0000), // Clear old arithmetic flags; ignore incoming CF.
        ],
    );
}

#[test]
fn cmp_39_covers_all_word_register_pairs_and_subtraction_flags() {
    check_register_pairs(
        0x39,
        &[
            (0x1234, 0x1234, 0x0044),
            (0x0000, 0x0001, 0x0095),
            (0x0010, 0x0001, 0x0014),
            (0x8000, 0x0001, 0x0814),
            (0x7FFF, 0xFFFF, 0x0885),
            (0xFFFF, 0x0000, 0x0084),
            (0x0001, 0x0000, 0x0000),
            (0x0100, 0x0000, 0x0004), // Equal low bytes do not mean equal words; PF uses only the low byte.
            (0x0000, 0xFFFF, 0x0011),
        ],
    );
}

#[test]
fn cmp_38_and_39_read_memory_minus_register_with_addressing_and_segment_overrides() {
    for opcode in [0x38, 0x39] {
        for (addressing, displacement, offset, default_base) in [
            (0x00, &[][..], 0x0120, 0x20000),           // BX+SI.
            (0x01, &[][..], 0x0140, 0x20000),           // BX+DI.
            (0x02, &[][..], 0x0320, 0x30000),           // BP+SI.
            (0x03, &[][..], 0x0340, 0x30000),           // BP+DI.
            (0x04, &[][..], 0x0020, 0x20000),           // SI.
            (0x05, &[][..], 0x0040, 0x20000),           // DI.
            (0x06, &[0x01, 0x02][..], 0x0201, 0x20000), // Direct, unaligned.
            (0x07, &[][..], 0x0100, 0x20000),           // BX.
            (0x46, &[0xFE][..], 0x02FE, 0x30000),       // BP-2: signed disp8.
            (0x83, &[0xF0, 0xFF][..], 0x0330, 0x30000), // BP+DI-16: disp16.
        ] {
            for (prefix, base) in [
                (None, default_base),
                (Some(0x26), 0x40000),
                (Some(0x2E), 0x10000),
                (Some(0x36), 0x30000),
                (Some(0x3E), 0x20000),
            ] {
                let mut cpu = Cpu::new(0);
                [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
                [cpu.bx, cpu.si, cpu.bp, cpu.di] = [0x0100, 0x0020, 0x0300, 0x0040];
                cpu.ip = 0x0800;
                cpu.flags = 0xFFD7;
                // Byte form takes AH=1, word form takes AX=1.
                cpu.ax = if opcode == 0x38 { 0x015A } else { 1 };
                let reg_field = if opcode == 0x38 { 0x20 } else { 0 };
                let mut instruction = Vec::new();
                if let Some(prefix) = prefix {
                    instruction.push(prefix);
                }
                instruction.extend_from_slice(&[opcode, addressing | reg_field]);
                instruction.extend_from_slice(displacement);
                let mut memory = vec![0xCC; 0x50000];
                memory[0x10800..0x10800 + instruction.len()].copy_from_slice(&instruction);
                if opcode == 0x38 {
                    memory[base + offset] = 0x80;
                } else {
                    memory[base + offset..base + offset + 2].copy_from_slice(&[0x00, 0x80]);
                }
                let original_memory = memory.clone();
                let original_registers = registers(&cpu);

                step(&mut cpu, &mut memory);

                assert_eq!(
                    cpu.flags,
                    (0xFFD7 & !0x08D5) | if opcode == 0x38 { 0x0810 } else { 0x0814 }
                );
                assert_eq!(registers(&cpu), original_registers);
                assert_eq!(cpu.ip, 0x0800 + instruction.len() as u16);
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn cmp_38_and_39_fetch_prefix_modrm_and_displacement_across_ip_wrap() {
    for opcode in [0x38, 0x39] {
        for start_ip in [0xFFFBu16, 0xFFFC, 0xFFFD, 0xFFFE, 0xFFFF] {
            let mut cpu = Cpu::new(0);
            cpu.cs = 0x1000;
            cpu.es = 0x2000;
            cpu.ip = start_ip;
            cpu.ax = 1;
            let mut memory = vec![0xCC; 0x30000];
            for (index, byte) in [0x26, opcode, 0x06, 0x34, 0x12].into_iter().enumerate() {
                memory[0x10000 + usize::from(start_ip.wrapping_add(index as u16))] = byte;
            }
            memory[0x21234..0x21236].copy_from_slice(&[0x10, 0x00]);
            let original_memory = memory.clone();
            let original_registers = registers(&cpu);

            step(&mut cpu, &mut memory);

            assert_eq!(cpu.flags, 0xF016); // 10h-1: AF and PF.
            assert_eq!(cpu.ip, start_ip.wrapping_add(5));
            assert_eq!(registers(&cpu), original_registers);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn cmp_38_and_39_read_operands_at_segment_and_physical_address_boundaries() {
    for opcode in [0x38, 0x39] {
        for (segment, offset, address) in [
            (0x2000, 0xFFFFu16, 0x2FFFF),
            (0xFFFF, 0x000F, 0xFFFFF),
            (0xFFFF, 0x0020, 0x00010),
        ] {
            let mut cpu = Cpu::new(0);
            cpu.cs = 0x1000;
            cpu.ds = segment;
            cpu.ax = 1;
            let mut memory = vec![0xCC; 0x100000];
            let [low, high] = offset.to_le_bytes();
            memory[0x10000..0x10004].copy_from_slice(&[opcode, 0x06, low, high]);
            memory[address] = if opcode == 0x38 { 0x80 } else { 0x00 };
            if opcode == 0x39 {
                memory[(address + 1) & 0xFFFFF] = 0x80;
            }
            let original_memory = memory.clone();
            let original_registers = registers(&cpu);

            step(&mut cpu, &mut memory);

            assert_eq!(cpu.flags, if opcode == 0x38 { 0xF812 } else { 0xF816 });
            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.ip, 4);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn cmp_38_and_39_read_exact_operand_width_and_leave_state_intact_on_missing_bytes() {
    for (opcode, offset, succeeds) in [
        (0x38, 0x01FFu16, true), // Last available byte is enough for CMP r/m8.
        (0x38, 0x0200, false),
        (0x39, 0x01FE, true),  // Last complete word.
        (0x39, 0x01FF, false), // Low byte exists, high byte does not.
        (0x39, 0x0200, false),
    ] {
        let mut cpu = Cpu::new(0);
        cpu.ax = if opcode == 0x38 { 0xA5CC } else { 0xCCCC };
        cpu.flags = 0xFFD7;
        let mut memory = vec![0xCC; 0x0200];
        let [low, high] = offset.to_le_bytes();
        memory[..4].copy_from_slice(&[opcode, 0x06, low, high]);
        let original_memory = memory.clone();
        let original_registers = registers(&cpu);

        let result = cpu.step(&mut memory);

        if succeeds {
            assert!(matches!(result.unwrap(), CpuStepResult::Continue));
            assert_eq!(cpu.flags, (0xFFD7 & !0x08D5) | 0x0044);
        } else {
            assert!(matches!(
                result,
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ));
            assert_eq!(cpu.flags, 0xFFD7, "failed reads must preserve flags");
        }
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.ip, 4);
        assert!(memory == original_memory);
    }
}
