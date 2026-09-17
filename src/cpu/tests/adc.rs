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

// LHS, RHS, incoming CF, result, literal CF/PF/AF/ZF/SF/OF expectations.
type Case = (u16, u16, bool, u16, u16);

const BYTE_CASES: &[Case] = &[
    (1, 2, false, 3, 0x0004),
    (1, 2, true, 4, 0x0000),
    (0, 0, false, 0, 0x0044),
    (0, 0, true, 1, 0x0000),
    (0x0F, 0, true, 0x10, 0x0010),
    (0xFF, 0, true, 0, 0x0055),
    (0, 0xFF, true, 0, 0x0055), // RHS + carry must not truncate.
    (0x7F, 0, true, 0x80, 0x0890),
    (0x80, 0x80, false, 0, 0x0845),
    (0x80, 0x80, true, 1, 0x0801),
    (0x80, 0x7F, true, 0, 0x0055),
    (0xFF, 0xFF, true, 0xFF, 0x0095),
];

const WORD_CASES: &[Case] = &[
    (1, 2, false, 3, 0x0004),
    (1, 2, true, 4, 0x0000),
    (0, 0, false, 0, 0x0044),
    (0, 0, true, 1, 0x0000),
    (0x00FF, 0, true, 0x0100, 0x0014), // Nonzero word with zero low byte.
    (0x0100, 0, true, 0x0101, 0x0000), // PF uses only the low byte.
    (0xFFFF, 0, true, 0, 0x0055),
    (0, 0xFFFF, true, 0, 0x0055),
    (0x7FFF, 0, true, 0x8000, 0x0894),
    (0x8000, 0x8000, false, 0, 0x0845),
    (0x8000, 0x8000, true, 1, 0x0801),
    (0x8000, 0x7FFF, true, 0, 0x0055),
    (0xFFFF, 0xFFFF, true, 0xFFFF, 0x0095),
];

fn check_register_pairs(opcode: u8, cases: &[Case]) {
    let word = opcode & 1 != 0;
    for destination in 0..8 {
        for source in 0..8 {
            for &(lhs, rhs, carry, result, arithmetic) in cases {
                // Equal-register cases use equal inputs; retain their literal expectations.
                if destination == source && lhs != rhs {
                    continue;
                }
                for base_flags in [0xF002, 0xFFD6] {
                    let mut cpu = Cpu::new(0);
                    if word {
                        let mut words = [0xA55A; 8];
                        words[destination] = lhs;
                        words[source] = rhs;
                        [
                            cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                        ] = words;
                    } else {
                        // Initialize independently of CPU register getters/setters.
                        let mut bytes = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
                        bytes[destination] = lhs as u8;
                        bytes[source] = rhs as u8;
                        cpu.ax = u16::from_le_bytes([bytes[0], bytes[4]]);
                        cpu.cx = u16::from_le_bytes([bytes[1], bytes[5]]);
                        cpu.dx = u16::from_le_bytes([bytes[2], bytes[6]]);
                        cpu.bx = u16::from_le_bytes([bytes[3], bytes[7]]);
                    }
                    cpu.flags = base_flags | u16::from(carry);
                    let initial_flags = cpu.flags;
                    let mut expected = registers(&cpu);
                    if word {
                        expected[destination] = result;
                    } else {
                        let shift = if destination < 4 { 0 } else { 8 };
                        expected[destination % 4] =
                            (expected[destination % 4] & !(0xFF << shift)) | (result << shift);
                    }
                    let (reg, rm) = if opcode < 0x12 {
                        (source, destination)
                    } else {
                        (destination, source)
                    };
                    let instruction = [opcode, 0xC0 | ((reg as u8) << 3) | rm as u8];
                    let mut memory = instruction;

                    step(&mut cpu, &mut memory);

                    assert_eq!(
                        registers(&cpu),
                        expected,
                        "opcode={opcode:02X}, dest={destination}, source={source}, lhs={lhs:04X}, rhs={rhs:04X}, CF={carry}"
                    );
                    assert_eq!(cpu.flags, (initial_flags & !0x08D5) | arithmetic);
                    assert_eq!(cpu.ip, 2);
                    assert_eq!(memory, instruction);
                }
            }
        }
    }
}

#[test]
fn adc_10_writes_byte_rm_and_sets_flags_for_all_register_pairs() {
    check_register_pairs(0x10, BYTE_CASES);
}

#[test]
fn adc_11_writes_word_rm_and_sets_flags_for_all_register_pairs() {
    check_register_pairs(0x11, WORD_CASES);
}

#[test]
fn adc_12_writes_byte_reg_and_sets_flags_for_all_register_pairs() {
    check_register_pairs(0x12, BYTE_CASES);
}

#[test]
fn adc_13_writes_word_reg_and_sets_flags_for_all_register_pairs() {
    check_register_pairs(0x13, WORD_CASES);
}

// AX/AH = 2, memory = 1, CF = 1: only the selected destination becomes 4.
fn check_memory(opcode: u8, mut cpu: Cpu, instruction: &[u8], address: usize) {
    let word = opcode & 1 != 0;
    cpu.ax = if word { 2 } else { 0x025A };
    cpu.flags = 0xFFD7;
    let mut memory = vec![0xCC; 0x100000];
    for (i, &byte) in instruction.iter().enumerate() {
        let offset = cpu.ip.wrapping_add(i as u16);
        memory[((usize::from(cpu.cs) << 4) + usize::from(offset)) & 0xFFFFF] = byte;
    }
    memory[address] = 1;
    if word {
        memory[(address + 1) & 0xFFFFF] = 0;
    }
    let mut expected_memory = memory.clone();
    let mut expected_registers = registers(&cpu);
    if opcode < 0x12 {
        expected_memory[address] = 4;
    } else {
        expected_registers[0] = if word { 4 } else { 0x045A };
    }
    let next_ip = cpu.ip.wrapping_add(instruction.len() as u16);

    step(&mut cpu, &mut memory);

    assert_eq!(registers(&cpu), expected_registers);
    assert_eq!(cpu.flags, 0xFFD7 & !0x08D5);
    assert_eq!(cpu.ip, next_ip);
    assert!(memory == expected_memory, "instruction={instruction:02X?}");
}

#[test]
fn adc_memory_direction_addressing_and_segment_overrides() {
    for opcode in 0x10..=0x13 {
        for (addressing, displacement, offset, default_base) in [
            (0x00, &[][..], 0x0120, 0x20000),           // BX+SI.
            (0x01, &[][..], 0x0140, 0x20000),           // BX+DI.
            (0x02, &[][..], 0x0320, 0x30000),           // BP+SI.
            (0x03, &[][..], 0x0340, 0x30000),           // BP+DI.
            (0x04, &[][..], 0x0020, 0x20000),           // SI.
            (0x05, &[][..], 0x0040, 0x20000),           // DI.
            (0x06, &[0x01, 0x02][..], 0x0201, 0x20000), // Direct, unaligned.
            (0x07, &[][..], 0x0100, 0x20000),           // BX.
            (0x46, &[0xFE][..], 0x02FE, 0x30000),       // BP-2.
            (0x83, &[0xF0, 0xFF][..], 0x0330, 0x30000), // BP+DI-16.
            (0x84, &[0xF0, 0xFF][..], 0x0010, 0x20000), // SI+FFF0 wraps.
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
                let mut instruction = Vec::new();
                if let Some(prefix) = prefix {
                    instruction.push(prefix);
                }
                instruction.extend_from_slice(&[
                    opcode,
                    addressing | if opcode & 1 == 0 { 0x20 } else { 0 },
                ]);
                instruction.extend_from_slice(displacement);
                check_memory(opcode, cpu, &instruction, base + offset);
            }
        }
    }
}

#[test]
fn adc_wraps_instruction_fetch_and_operand_physical_addresses() {
    for opcode in 0x10..=0x13 {
        for ip in [0xFFFBu16, 0xFFFC, 0xFFFD, 0xFFFE, 0xFFFF] {
            for (segment, offset, address) in [
                (0x2000, 0xFFFFu16, 0x2FFFF),
                (0xFFFF, 0x000F, 0xFFFFF), // Word's second byte wraps the bus.
                (0xFFFF, 0x0020, 0x00010),
            ] {
                let mut cpu = Cpu::new(0);
                cpu.cs = 0x1000;
                cpu.es = segment;
                cpu.ip = ip;
                let [low, high] = offset.to_le_bytes();
                let modrm = if opcode & 1 == 0 { 0x26 } else { 0x06 };
                check_memory(opcode, cpu, &[0x26, opcode, modrm, low, high], address);
            }
        }
    }
}

#[test]
fn adc_memory_address_uses_register_value_before_destination_changes() {
    for opcode in 0x10..=0x13 {
        let mut cpu = Cpu::new(0);
        cpu.bx = 0x0101;
        cpu.flags = 0xF003;
        let mut memory = vec![0xCC; 0x0300];
        // ADC [BX],BL/BX or ADC BL/BX,[BX].
        memory[..2].copy_from_slice(&[opcode, 0x1F]);
        memory[0x101..0x103].copy_from_slice(&[2, 0]);
        let mut expected_memory = memory.clone();
        let mut expected_registers = registers(&cpu);
        if opcode < 0x12 {
            expected_memory[0x101] = 4;
            if opcode == 0x11 {
                expected_memory[0x102] = 1;
            }
        } else {
            expected_registers[3] = 0x0104;
        }

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected_registers);
        assert!(memory == expected_memory);
        assert_eq!(cpu.flags, 0xF002);
        assert_eq!(cpu.ip, 2);
    }
}

#[test]
fn adc_reads_exact_operand_width_and_preserves_state_on_missing_memory() {
    for opcode in 0x10..=0x13 {
        for offset in [0x01FEu16, 0x01FF, 0x0200] {
            let mut cpu = Cpu::new(0);
            cpu.ax = 2;
            cpu.flags = 0xFFD7;
            let [low, high] = offset.to_le_bytes();
            let mut memory = vec![0; 0x0200];
            memory[..4].copy_from_slice(&[opcode, 0x06, low, high]);
            let mut expected_memory = memory.clone();
            let mut expected_registers = registers(&cpu);
            let succeeds = offset < 0x0200 && (opcode & 1 == 0 || offset < 0x01FF);

            let outcome = cpu.step(&mut memory);

            if succeeds {
                assert!(matches!(outcome.unwrap(), CpuStepResult::Continue));
                if opcode < 0x12 {
                    expected_memory[offset as usize] = 3;
                } else {
                    expected_registers[0] = 3;
                }
                assert_eq!(cpu.flags, (0xFFD7 & !0x08D5) | 0x0004);
            } else {
                assert!(matches!(
                    outcome,
                    Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
                ));
                assert_eq!(cpu.flags, 0xFFD7);
            }
            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(memory, expected_memory);
            assert_eq!(cpu.ip, 4);
        }
    }
}
