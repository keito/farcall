use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// Intel IMUL: 69 /r iw and 6B /r ib store a truncated signed product in r16.
// 6B sign-extends imm8. Only CF/OF are defined among arithmetic flags.
// https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2a-manual.pdf
// On 8088, 69/6B alias JNS/JNP instead:
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

// (source word, encoded immediate, destination word, expected CF|OF).
type Case = (u16, u16, u16, u16);
const IMM8_CASES: &[Case] = &[
    (0x0000, 0x80, 0x0000, 0x0000),
    (0x0001, 0x7F, 0x007F, 0x0000),
    (0x0001, 0x80, 0xFF80, 0x0000),
    (0x0001, 0xFF, 0xFFFF, 0x0000),
    (0xFFFF, 0x80, 0x0080, 0x0000),
    (0xFFFF, 0xFF, 0x0001, 0x0000),
    (0xFFFF, 0x01, 0xFFFF, 0x0000), // Negative result fits despite high product bits.
    (0x0004, 0xFD, 0xFFF4, 0x0000),
    (0xFFFC, 0xFD, 0x000C, 0x0000),
    (0x7FFF, 0x01, 0x7FFF, 0x0000),
    (0x8000, 0x01, 0x8000, 0x0000),
    (0x8000, 0xFF, 0x8000, 0x0801), // -32768 * -1 = +32768, does not fit.
    (0x7FFF, 0x02, 0xFFFE, 0x0801),
    (0x8000, 0x02, 0x0000, 0x0801),
    (0x0100, 0x7F, 0x7F00, 0x0000),
    (0x0100, 0x80, 0x8000, 0x0000), // Exactly -32768 fits.
    (0x0101, 0x80, 0x7F80, 0x0801), // One step beyond the negative bound.
    (0xFF00, 0x80, 0x8000, 0x0801), // +32768 overflows.
    (0x8000, 0x00, 0x0000, 0x0000),
];
const IMM16_CASES: &[Case] = &[
    (0x0000, 0xFFFF, 0x0000, 0x0000),
    (0x0001, 0x0080, 0x0080, 0x0000), // 69 must retain the immediate's high byte.
    (0x0001, 0x8000, 0x8000, 0x0000),
    (0xFFFF, 0x0080, 0xFF80, 0x0000),
    (0xFFFF, 0xFFFF, 0x0001, 0x0000),
    (0xFFFF, 0x0001, 0xFFFF, 0x0000),
    (0x0004, 0xFFFD, 0xFFF4, 0x0000),
    (0xFFFC, 0xFFFD, 0x000C, 0x0000),
    (0x7FFF, 0x0001, 0x7FFF, 0x0000),
    (0x8000, 0x0001, 0x8000, 0x0000),
    (0x8000, 0xFFFF, 0x8000, 0x0801),
    (0xFFFF, 0x8000, 0x8000, 0x0801),
    (0x7FFF, 0x0002, 0xFFFE, 0x0801),
    (0x8000, 0x0002, 0x0000, 0x0801),
    (0x8000, 0x8000, 0x0000, 0x0801),
    (0x7FFF, 0x7FFF, 0x0001, 0x0801), // Small truncated result still overflows.
    (0x0100, 0x007F, 0x7F00, 0x0000),
    (0x0100, 0x0080, 0x8000, 0x0801),
    (0xFF00, 0x0080, 0x8000, 0x0000),
    (0x0100, 0x0100, 0x0000, 0x0801),
    (0x0002, 0x3FFF, 0x7FFE, 0x0000),
    (0x0002, 0x4000, 0x8000, 0x0801),
];

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss,
        cpu.es,
    ]
}

fn seed_registers(cpu: &mut Cpu) {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA55A, 0xC33C, 0xD22D, 0x0200, 0x0800, 0x0300, 0x0040, 0x0080,
    ];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x0100, 0x0200, 0x0300, 0x0400];
    cpu.ip = 0x0100;
}

fn append_immediate(instruction: &mut Vec<u8>, opcode: u8, immediate: u16) {
    if opcode == 0x6B {
        instruction.push(immediate as u8);
    } else {
        instruction.extend_from_slice(&immediate.to_le_bytes());
    }
}

fn assert_flags(actual: u16, initial: u16, cf_of: u16) {
    assert_eq!(actual & 0x0801, cf_of, "set or clear CF and OF together");
    assert_eq!(
        actual & !0x08D5,
        initial & !0x08D5,
        "preserve non-arithmetic flags"
    );
    // SF/ZF/AF/PF are undefined and deliberately excluded.
}

#[test]
fn i286_6b_signed_register_products_and_overflow() {
    check_register_cases(0x6B, IMM8_CASES);
}

#[test]
fn i286_69_signed_register_products_and_overflow() {
    check_register_cases(0x69, IMM16_CASES);
}

fn check_register_cases(opcode: u8, cases: &[Case]) {
    for &case in cases {
        // Include AX/DX, SP, and overlapping source/destination registers.
        for destination in 0..8 {
            for source in 0..8 {
                check_register_case(opcode, destination, source, case);
            }
        }
    }
}

fn check_register_case(
    opcode: u8,
    destination: u8,
    source: u8,
    (operand, immediate, product, cf_of): Case,
) {
    for flags in [0x0002, 0x0FD7] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        seed_registers(&mut cpu);
        cpu.flags = flags;
        let mut expected_registers = registers(&cpu);
        expected_registers[usize::from(source)] = operand;
        [
            cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
        ] = expected_registers[..8].try_into().unwrap();
        expected_registers[usize::from(destination)] = product;
        let mut instruction = vec![opcode, 0xC0 | (destination << 3) | source];
        append_immediate(&mut instruction, opcode, immediate);
        let mut memory = vec![0xCC; 0x6000];
        memory[0x1100..0x1100 + instruction.len()].copy_from_slice(&instruction);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(
            registers(&cpu),
            expected_registers,
            "opcode={opcode:02X}, dst={destination}, src={source}, operand={operand:04X}, imm={immediate:04X}"
        );
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert_flags(cpu.flags, flags, cf_of);
        assert!(
            memory == original_memory,
            "register IMUL does not write memory"
        );
    }
}

#[test]
fn i286_6b_sign_extends_every_immediate_byte() {
    for immediate in 0..=255u16 {
        let expected = immediate | if immediate < 0x80 { 0x0000 } else { 0xFF00 };
        // BX := DX * imm8, with DX=1; AX/DX are not implicit destinations.
        check_register_case(0x6B, 3, 2, (1, immediate, expected, 0));
    }
}

#[test]
fn i286_6b_memory_sources_use_original_addresses_and_segment_overrides() {
    check_memory_cases(0x6B, IMM8_CASES);
}

#[test]
fn i286_69_memory_sources_use_original_addresses_and_segment_overrides() {
    check_memory_cases(0x69, IMM16_CASES);
}

fn check_memory_cases(opcode: u8, cases: &[Case]) {
    // (ModR/M addressing bits + displacement, effective offset, default segment base).
    for (encoding, offset, default_base) in [
        (&[0x00][..], 0x0240, 0x2000),             // BX+SI.
        (&[0x01][..], 0x0280, 0x2000),             // BX+DI.
        (&[0x02][..], 0x0340, 0x3000),             // BP+SI uses SS.
        (&[0x03][..], 0x0380, 0x3000),             // BP+DI uses SS.
        (&[0x04][..], 0x0040, 0x2000),             // SI.
        (&[0x05][..], 0x0080, 0x2000),             // DI.
        (&[0x06, 0x35, 0x12][..], 0x1235, 0x2000), // Unaligned direct word.
        (&[0x07][..], 0x0200, 0x2000),             // BX.
        (&[0x46, 0xFE][..], 0x02FE, 0x3000),       // BP-2, signed disp8.
        (&[0x86, 0x01, 0x01][..], 0x0401, 0x3000), // BP+0101h, disp16.
        (&[0x84, 0xC0, 0xFF][..], 0x0000, 0x2000), // SI+FFC0h wraps offset.
    ] {
        for (prefix, base) in [
            (None, default_base),
            (Some(0x26), 0x4000),
            (Some(0x2E), 0x1000),
            (Some(0x36), 0x3000),
            (Some(0x3E), 0x2000),
        ] {
            for destination in 0..8u8 {
                for &(operand, immediate, product, cf_of) in cases {
                    for flags in [0x0002, 0x0FD7] {
                        let mut cpu = Cpu::new(CpuModel::I80286, 0);
                        seed_registers(&mut cpu);
                        cpu.flags = flags;
                        let mut expected_registers = registers(&cpu);
                        expected_registers[usize::from(destination)] = product;
                        let mut instruction: Vec<u8> = prefix.into_iter().collect();
                        instruction.extend_from_slice(&[opcode, encoding[0] | (destination << 3)]);
                        instruction.extend_from_slice(&encoding[1..]);
                        append_immediate(&mut instruction, opcode, immediate);
                        let mut memory = vec![0xCC; 0x6000];
                        memory[0x1100..0x1100 + instruction.len()].copy_from_slice(&instruction);
                        let address = base + offset;
                        memory[address..address + 2].copy_from_slice(&operand.to_le_bytes());
                        let original_memory = memory.clone();

                        assert!(matches!(
                            cpu.step(&mut memory).unwrap(),
                            CpuStepResult::Continue
                        ));

                        assert_eq!(
                            registers(&cpu),
                            expected_registers,
                            "instruction={instruction:02X?}, operand={operand:04X}, imm={immediate:04X}"
                        );
                        assert_eq!(
                            cpu.ip,
                            0x0100 + instruction.len() as u16,
                            "immediate follows the displacement"
                        );
                        assert_flags(cpu.flags, flags, cf_of);
                        assert!(
                            memory == original_memory,
                            "IMUL must not write its memory source"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn i286_immediate_imul_consumes_only_its_encoded_bytes() {
    for (ip, instruction) in [
        (0x3FFCu16, &[0x69, 0xC1, 0xFD, 0xFF][..]),
        (0x3FFD, &[0x6B, 0xC1, 0xFD][..]),
    ] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        seed_registers(&mut cpu);
        cpu.cs = 0;
        cpu.ip = ip;
        cpu.cx = 4;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0xFFF4; // AX := CX * -3.
        let mut memory = vec![0xCC; 0x4000];
        memory[usize::from(ip)..].copy_from_slice(instruction);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, 0x4000);
        assert_flags(cpu.flags, 0x0FD7, 0);
        assert!(memory == original_memory);
    }
}

#[test]
fn i286_immediate_imul_reports_bad_source_memory_without_changing_destination() {
    for opcode in [0x69, 0x6B] {
        for offset in [0x01FFu16, 0x0200] {
            let mut cpu = Cpu::new(CpuModel::I80286, 0);
            seed_registers(&mut cpu);
            cpu.cs = 0;
            cpu.ds = 0;
            let flags = cpu.flags;
            let original_registers = registers(&cpu);
            let [low, high] = offset.to_le_bytes();
            let mut instruction = vec![opcode, 0x1E, low, high]; // IMUL BX,[disp16],-3.
            append_immediate(
                &mut instruction,
                opcode,
                if opcode == 0x69 { 0xFFFD } else { 0xFD },
            );
            let mut memory = vec![0xCC; 0x0200];
            memory[0x0100..0x0100 + instruction.len()].copy_from_slice(&instruction);
            let original_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory),
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ));

            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, flags);
            assert_eq!(memory, original_memory);
            // No fault-IP rollback policy is asserted for a host RAM bounds error.
        }
    }
}

#[test]
fn i8088_69_alias_is_jns() {
    check_jump(false, 0x69);
}

#[test]
fn i8088_6b_alias_is_jnp() {
    check_jump(false, 0x6B);
}

#[test]
fn i8088_79_and_7b_remain_conditional_jumps() {
    check_jump(false, 0x79);
    check_jump(false, 0x7B);
}

#[test]
fn i286_79_and_7b_remain_conditional_jumps() {
    check_jump(true, 0x79);
    check_jump(true, 0x7B);
}

fn check_jump(is_286: bool, opcode: u8) {
    let cases = [
        (0x0100u16, 0x00, 0x0102, 0x0102),
        (0x0100, 0x05, 0x0107, 0x0102),
        (0x0100, 0x7F, 0x0181, 0x0102),
        (0x0100, 0x80, 0x0082, 0x0102),
        (0x0100, 0xC0, 0x00C2, 0x0102),
        (0x0100, 0xFE, 0x0100, 0x0102),
        (0xFFFD, 0x01, 0x0000, 0xFFFF),
        (0xFFFE, 0xFE, 0xFFFE, 0x0000),
        (0xFFFF, 0x02, 0x0003, 0x0001),
        (0x0000, 0x80, 0xFF82, 0x0002),
    ];
    let cases = if is_286 { &cases[..6] } else { &cases[..] };
    for &(ip, displacement, taken_ip, untaken_ip) in cases {
        for (low_flags, jns_taken, jnp_taken) in [
            (0x0002, true, true),
            (0x0082, false, true),
            (0x0006, true, false),
            (0x0086, false, false),
            (0x0F53, true, true),
            (0x0FD7, false, false),
        ] {
            let model = if is_286 {
                CpuModel::I80286
            } else {
                CpuModel::I8088
            };
            let mut cpu = Cpu::new(model, 0);
            seed_registers(&mut cpu);
            cpu.ip = ip;
            cpu.flags = (if is_286 { 0 } else { 0xF000 }) | low_flags;
            let flags = cpu.flags;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x30000];
            memory[0x1000 + usize::from(ip)] = opcode;
            memory[0x1000 + usize::from(ip.wrapping_add(1))] = displacement;
            let original_memory = memory.clone();
            let taken = if opcode == 0x69 || opcode == 0x79 {
                jns_taken
            } else {
                jnp_taken
            };

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(
                cpu.ip,
                if taken { taken_ip } else { untaken_ip },
                "opcode={opcode:02X}, flags={flags:04X}, displacement={displacement:02X}"
            );
            assert_eq!(
                registers(&cpu),
                original_registers,
                "jump must not multiply registers"
            );
            assert_eq!(cpu.flags, flags);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn i8088_imul_aliases_consume_only_a_jump_displacement() {
    for opcode in [0x69, 0x6B] {
        for flags in [0xF002, 0xFFD7] {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            seed_registers(&mut cpu);
            cpu.cs = 0;
            cpu.ip = 0x3FFE;
            cpu.flags = flags;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x4000];
            memory[0x3FFE..].copy_from_slice(&[opcode, 0x00]);
            let original_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(cpu.ip, 0x4000);
            assert_eq!(registers(&cpu), original_registers);
            assert_eq!(cpu.flags, flags);
            assert!(memory == original_memory);
        }
    }
}
