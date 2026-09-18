use super::{Cpu, CpuError, CpuStepResult, MemoryError};

#[derive(Clone, Copy, Debug)]
enum Width {
    Byte,
    Word,
}

impl Width {
    fn mask(self) -> u16 {
        match self {
            Self::Byte => 0xFF,
            Self::Word => 0xFFFF,
        }
    }

    fn bytes(self) -> usize {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Form {
    RmReg,
    RegRm,
    Immediate(u8),
    Unary(u8),
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    And,
    Or,
    Xor,
    Test,
    Not,
}

impl Operation {
    fn result(self, lhs: u16, rhs: u16) -> u16 {
        match self {
            Self::And | Self::Test => lhs & rhs,
            Self::Or => lhs | rhs,
            Self::Xor => lhs ^ rhs,
            Self::Not => !lhs,
        }
    }
}

// This is an independent list of guest encodings, not the emulator's dispatch
// table. In particular, width is explicit rather than inferred from opcode bits.
// 30 and the logical 83 extensions are not implemented yet.
type Encoding = (u8, Width, Form, Operation);
const ENCODINGS: &[Encoding] = &[
    (0x08, Width::Byte, Form::RmReg, Operation::Or),
    (0x09, Width::Word, Form::RmReg, Operation::Or),
    (0x0A, Width::Byte, Form::RegRm, Operation::Or),
    (0x0B, Width::Word, Form::RegRm, Operation::Or),
    (0x20, Width::Byte, Form::RmReg, Operation::And),
    (0x21, Width::Word, Form::RmReg, Operation::And),
    (0x22, Width::Byte, Form::RegRm, Operation::And),
    (0x23, Width::Word, Form::RegRm, Operation::And),
    (0x31, Width::Word, Form::RmReg, Operation::Xor),
    (0x32, Width::Byte, Form::RegRm, Operation::Xor),
    (0x33, Width::Word, Form::RegRm, Operation::Xor),
    (0x84, Width::Byte, Form::RmReg, Operation::Test),
    (0x85, Width::Word, Form::RmReg, Operation::Test),
    (0x80, Width::Byte, Form::Immediate(1), Operation::Or),
    (0x81, Width::Word, Form::Immediate(1), Operation::Or),
    (0x80, Width::Byte, Form::Immediate(4), Operation::And),
    (0x81, Width::Word, Form::Immediate(4), Operation::And),
    (0x80, Width::Byte, Form::Immediate(6), Operation::Xor),
    (0x81, Width::Word, Form::Immediate(6), Operation::Xor),
    (0xF6, Width::Byte, Form::Immediate(0), Operation::Test),
    (0xF7, Width::Word, Form::Immediate(0), Operation::Test),
    (0xF6, Width::Byte, Form::Unary(2), Operation::Not),
    (0xF7, Width::Word, Form::Unary(2), Operation::Not),
];

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup() -> Cpu {
    let mut cpu = Cpu::new(0);
    // All halves differ; byte register codes 4..7 must select AH..BH, not SP..DI.
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA5F3, 0x5A0F, 0xC369, 0x0200, 0xE517, 0x0800, 0x0300, 0x0040,
    ];
    [cpu.es, cpu.cs, cpu.ss, cpu.ds] = [0x3000, 0x4000, 0x2000, 0x1000];
    cpu.ip = 0x0100;
    cpu
}

// The test oracle uses snapshots and numeric register codes, never production
// register getters, operand decoders, memory helpers, or flag helpers.
fn reg_value(regs: &[u16; 12], width: Width, code: u8) -> u16 {
    match width {
        Width::Word => regs[code as usize],
        Width::Byte => (regs[(code & 3) as usize] >> (8 * (code / 4))) & 0xFF,
    }
}

fn set_reg_value(regs: &mut [u16; 12], width: Width, code: u8, value: u16) {
    match width {
        Width::Word => regs[code as usize] = value,
        Width::Byte => {
            let shift = 8 * (code / 4);
            let word = &mut regs[(code & 3) as usize];
            *word = (*word & !(0xFF << shift)) | ((value & 0xFF) << shift);
        }
    }
}

fn instruction(enc: Encoding, reg: u8, addressing: &[u8], immediate: u16) -> Vec<u8> {
    let (opcode, width, form, _) = enc;
    let field = match form {
        Form::Immediate(extension) | Form::Unary(extension) => extension,
        _ => reg,
    };
    let mut code = vec![opcode, addressing[0] | (field << 3)];
    code.extend_from_slice(&addressing[1..]);
    if matches!(form, Form::Immediate(_)) {
        code.extend_from_slice(&immediate.to_le_bytes()[..width.bytes()]);
    }
    code
}

fn load_code(cpu: &Cpu, memory: &mut [u8], code: &[u8]) {
    for (i, &byte) in code.iter().enumerate() {
        let offset = cpu.ip.wrapping_add(i as u16);
        memory[((usize::from(cpu.cs) * 16) + usize::from(offset)) & 0xFFFFF] = byte;
    }
}

fn result_flags(width: Width, value: u16) -> u16 {
    let sign_bit = match width {
        Width::Byte => 0x80,
        Width::Word => 0x8000,
    };
    (if value == 0 { 0x40 } else { 0 })
        | (if value & sign_bit != 0 { 0x80 } else { 0 })
        | (if (value & 0xFF).count_ones() % 2 == 0 {
            4
        } else {
            0
        })
}

fn assert_flags(enc: Encoding, actual: u16, initial: u16, expected: u16) {
    if matches!(enc.3, Operation::Not) {
        assert_eq!(actual, initial, "{enc:?}: NOT preserves every flag");
    } else {
        // AF is undefined; CF/OF clear, SZP take the expected values, and all
        // other bits (including DF/IF/TF) survive.
        assert_eq!(actual & !0x10, (initial & !0x08D5) | expected, "{enc:?}");
    }
}

fn check_register_case(
    enc: Encoding,
    reg: u8,
    rm: u8,
    ax: u16,
    cx: u16,
    immediate: u16,
    flags: u16,
    literal: Option<(u16, u16)>,
) {
    let (_, width, form, op) = enc;
    let mut cpu = setup();
    cpu.ax = ax;
    cpu.cx = cx;
    cpu.flags = flags;
    let mut expected = registers(&cpu);
    let lhs = reg_value(&expected, width, rm);
    let rhs = match form {
        Form::Immediate(_) => immediate & width.mask(),
        Form::Unary(_) => 0,
        _ => reg_value(&expected, width, reg),
    };
    let (value, expected_flags) = literal.unwrap_or_else(|| {
        let value = op.result(lhs, rhs) & width.mask();
        (value, result_flags(width, value))
    });
    if !matches!(op, Operation::Test) {
        let destination = if matches!(form, Form::RegRm) { reg } else { rm };
        set_reg_value(&mut expected, width, destination, value);
    }
    // Trailing sentinels allow an incorrect immediate width to execute, so IP
    // and state assertions diagnose it instead of relying on a host panic.
    let code = instruction(enc, reg, &[0xC0 | rm], immediate);
    let mut memory = vec![0xCC; 0x40200];
    load_code(&cpu, &mut memory, &code);
    let original = memory.clone();
    assert!(
        matches!(cpu.step(&mut memory).unwrap(), CpuStepResult::Continue),
        "{enc:?}"
    );
    assert_eq!(registers(&cpu), expected, "{enc:?}, reg={reg}, rm={rm}");
    assert_eq!(cpu.ip, 0x0100 + code.len() as u16, "{enc:?}");
    assert!(memory == original, "{enc:?}: register form wrote memory");
    assert_flags(enc, cpu.flags, flags, expected_flags);
}

#[test]
fn every_register_encoding_preserves_sources_and_other_halves_including_aliases() {
    for &enc in ENCODINGS {
        let reg_count = if matches!(enc.2, Form::RmReg | Form::RegRm) {
            8
        } else {
            1
        };
        for reg in 0..reg_count {
            for rm in 0..8 {
                for flags in [0, 0xFFFF] {
                    check_register_case(enc, reg, rm, 0xA5F3, 0x5A0F, 0x9234, flags, None);
                }
            }
        }
    }
}

// lhs, rhs, full-word result, byte SZP, word SZP. Literal flag expectations
// distinguish bit 7 from bit 15 and low-byte parity from whole-word parity.
type FlagCase = (u16, u16, u16, u16, u16);
const AND_CASES: &[FlagCase] = &[
    (0x0000, 0xFFFF, 0x0000, 0x44, 0x44),
    (0x0080, 0xFFFF, 0x0080, 0x80, 0x00),
    (0x8000, 0xFFFF, 0x8000, 0x44, 0x84),
    (0x0100, 0xFFFF, 0x0100, 0x44, 0x04),
    (0x0101, 0xFFFF, 0x0101, 0x00, 0x00),
    (0xA55A, 0x0FF0, 0x0550, 0x04, 0x04),
    (0xF0F0, 0x0F0F, 0x0000, 0x44, 0x44),
    (0xFFFF, 0xFFFF, 0xFFFF, 0x84, 0x84),
];
const OR_CASES: &[FlagCase] = &[
    (0x0000, 0x0000, 0x0000, 0x44, 0x44),
    (0x0080, 0x0000, 0x0080, 0x80, 0x00),
    (0x8000, 0x0000, 0x8000, 0x44, 0x84),
    (0x0100, 0x0000, 0x0100, 0x44, 0x04),
    (0x0100, 0x0001, 0x0101, 0x00, 0x00),
    (0x1234, 0xA55A, 0xB77E, 0x04, 0x84),
    (0x0055, 0x0055, 0x0055, 0x04, 0x04),
    (0x8000, 0x0001, 0x8001, 0x00, 0x80),
];
const XOR_CASES: &[FlagCase] = &[
    (0x1234, 0x1234, 0x0000, 0x44, 0x44),
    (0x0080, 0x0000, 0x0080, 0x80, 0x00),
    (0x8000, 0x0000, 0x8000, 0x44, 0x84),
    (0x1234, 0x0234, 0x1000, 0x44, 0x04),
    (0x1234, 0x1335, 0x0101, 0x00, 0x00),
    (0xA55A, 0x255A, 0x8000, 0x44, 0x84),
    (0xAAAA, 0x5555, 0xFFFF, 0x84, 0x84),
    (0x1234, 0x00FF, 0x12CB, 0x80, 0x00),
];

#[test]
fn every_logic_encoding_uses_its_own_width_for_zero_sign_and_parity() {
    for &enc in ENCODINGS {
        let cases = match enc.3 {
            Operation::And | Operation::Test => AND_CASES,
            Operation::Or => OR_CASES,
            Operation::Xor => XOR_CASES,
            Operation::Not => continue, // Its flag preservation is checked above.
        };
        for &(lhs, rhs, result, byte_flags, word_flags) in cases {
            for initial in [0, 0xFFFF] {
                let flags = match enc.1 {
                    Width::Byte => byte_flags,
                    Width::Word => word_flags,
                };
                // For byte instructions retain deliberately different upper halves.
                let (ax, cx) = match enc.1 {
                    Width::Byte => (0xA500 | (lhs & 0xFF), 0x5A00 | (rhs & 0xFF)),
                    Width::Word => (lhs, rhs),
                };
                check_register_case(
                    enc,
                    1,
                    0,
                    ax,
                    cx,
                    rhs,
                    initial,
                    Some((result & enc.1.mask(), flags)),
                );
            }
        }
    }
}

fn check_memory_case(
    enc: Encoding,
    mut cpu: Cpu,
    addressing: &[u8],
    prefix: Option<u8>,
    low: usize,
    high: usize,
    memory_len: usize,
) {
    let (_, width, form, op) = enc;
    cpu.cx = 0xA55A; // Never aliases any address register in the cases below.
    cpu.flags = 0xFFFF;
    let code = instruction(enc, 1, addressing, 0xA55A);
    let mut prefixed = prefix.into_iter().collect::<Vec<_>>();
    prefixed.extend_from_slice(&code);
    let mut memory = vec![0xCC; memory_len];
    memory[low] = 0x34;
    if high < memory.len() {
        memory[high] = 0x92;
    }
    load_code(&cpu, &mut memory, &prefixed);
    let mut expected_memory = memory.clone();
    let mut expected_regs = registers(&cpu);
    let result = op.result(0x9234 & width.mask(), 0xA55A & width.mask()) & width.mask();
    if !matches!(op, Operation::Test) {
        if matches!(form, Form::RegRm) {
            set_reg_value(&mut expected_regs, width, 1, result);
        } else {
            expected_memory[low] = result as u8;
            if matches!(width, Width::Word) {
                expected_memory[high] = (result >> 8) as u8;
            }
        }
    }
    let next_ip = cpu.ip.wrapping_add(prefixed.len() as u16);
    assert!(
        matches!(cpu.step(&mut memory).unwrap(), CpuStepResult::Continue),
        "{enc:?}"
    );
    assert_eq!(
        registers(&cpu),
        expected_regs,
        "{enc:?}, addressing={addressing:02X?}, prefix={prefix:?}"
    );
    assert_eq!(
        cpu.ip, next_ip,
        "{enc:?}: consume displacement before immediate"
    );
    assert!(
        memory == expected_memory,
        "{enc:?}, addressing={addressing:02X?}, prefix={prefix:?}: incorrect memory write/width"
    );
    assert_flags(enc, cpu.flags, 0xFFFF, result_flags(width, result));
}

#[test]
fn every_memory_form_uses_the_right_direction_width_displacement_and_segment() {
    // Mod/RM base (reg field zero), displacement, expected offset, default segment.
    // BX=0200, BP=0800, SI=0300, DI=0040. Include an odd address and offset wrap.
    let addresses: &[(&[u8], usize, usize)] = &[
        (&[0x00], 0x0500, 0x1000),
        (&[0x01], 0x0240, 0x1000),
        (&[0x02], 0x0B00, 0x2000),
        (&[0x03], 0x0840, 0x2000),
        (&[0x04], 0x0300, 0x1000),
        (&[0x05], 0x0040, 0x1000),
        (&[0x06, 0x35, 0x12], 0x1235, 0x1000),
        (&[0x07], 0x0200, 0x1000),
        (&[0x46, 0xFE], 0x07FE, 0x2000),
        (&[0x47, 0x80], 0x0180, 0x1000),
        (&[0x84, 0xFF, 0xFF], 0x02FF, 0x1000),
        (&[0x86, 0x00, 0xF8], 0x0000, 0x2000),
    ];
    for &enc in ENCODINGS {
        for &(addressing, offset, default_segment) in addresses {
            for (prefix, segment) in [
                (None, default_segment),
                (Some(0x26), 0x3000),
                (Some(0x2E), 0x4000),
                (Some(0x36), 0x2000),
                (Some(0x3E), 0x1000),
            ] {
                let low = segment * 16 + offset;
                check_memory_case(enc, setup(), addressing, prefix, low, low + 1, 0x50000);
            }
        }
    }
}

#[test]
fn every_encoding_fetches_modrm_displacement_and_immediate_across_ip_wrap() {
    for &enc in ENCODINGS {
        // Vary the split point, including within the immediate word.
        let length = instruction(enc, 1, &[0x06, 0x35, 0x12], 0xA55A).len() + 1;
        for consumed in 1..=length {
            let mut cpu = setup();
            cpu.ip = 0u16.wrapping_sub(consumed as u16);
            check_memory_case(
                enc,
                cpu,
                &[0x06, 0x35, 0x12],
                Some(0x36),
                0x21235,
                0x21236,
                0x50000,
            );
        }
    }
}

#[test]
fn every_encoding_wraps_physical_addresses_at_the_twenty_bit_boundary() {
    for &enc in ENCODINGS {
        for (offset, low, high) in [(0x000Fu16, 0xFFFFF, 0), (0x0010, 0, 1)] {
            let mut cpu = setup();
            cpu.ds = 0xFFFF;
            let [lo, hi] = offset.to_le_bytes();
            check_memory_case(enc, cpu, &[0x06, lo, hi], None, low, high, 0x100000);
        }
    }
}

#[test]
fn byte_forms_access_exactly_one_byte_at_the_end_of_memory() {
    for &enc in ENCODINGS.iter().filter(|enc| matches!(enc.1, Width::Byte)) {
        let mut cpu = setup();
        cpu.cs = 0;
        cpu.ds = 0;
        check_memory_case(enc, cpu, &[0x06, 0xFF, 0x01], None, 0x1FF, 0x200, 0x200);
    }
}

#[test]
fn missing_operands_do_not_partially_write_registers_memory_or_flags() {
    for &enc in ENCODINGS {
        // No bytes available, and for word operations only one available.
        for available in 0..enc.1.bytes() {
            let mut cpu = setup();
            cpu.cs = 0;
            cpu.ds = 0;
            cpu.flags = 0xFFFF;
            let initial_regs = registers(&cpu);
            let offset = 0x0200u16 - available as u16;
            let [lo, hi] = offset.to_le_bytes();
            let code = instruction(enc, 1, &[0x06, lo, hi], 0xA55A);
            let mut memory = vec![0xCC; 0x200];
            load_code(&cpu, &mut memory, &code);
            let original = memory.clone();
            assert!(
                matches!(
                    cpu.step(&mut memory),
                    Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
                ),
                "{enc:?}"
            );
            assert_eq!(registers(&cpu), initial_regs, "{enc:?}");
            assert_eq!(cpu.flags, 0xFFFF, "{enc:?}");
            assert!(memory == original, "{enc:?}: partial memory write");
        }
    }
}
