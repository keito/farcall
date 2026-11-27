use super::{Cpu, CpuModel, CpuStepResult};

// Intel SDM: the 8088 uses all eight count bits; the 286 masks to five.
// https://cdrdv2-public.intel.com/789581/325383-sdm-vol-2abcd.pdf
// Literal results for operands 81h / 8001h. Each pair is incoming CF=0 / CF=1.
// Each packed value is FLAGS:RESULT. Only architecturally defined flags are checked.
const COUNTS: [u8; 13] = [0, 1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 255];
// Expected fixture indexes for 286 counts: 32 -> 0, 33 -> 1, 255 -> 31.
const I286_INDEXES: [usize; 13] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 9];

struct Operation {
    name: &'static str,
    extension: u8,
    byte: [[u32; 2]; 13],
    word: [[u32; 2]; 13],
}

const OPERATIONS: [Operation; 7] = [
    Operation {
        name: "ROL",
        extension: 0,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x08010003, 0x08010003], // Count 1.
            [0x00000006, 0x00000006], // Count 2.
            [0x000000C0, 0x000000C0], // Count 7.
            [0x00010081, 0x00010081], // Count 8.
            [0x00010003, 0x00010003], // Count 9.
            [0x000000C0, 0x000000C0], // Count 15.
            [0x00010081, 0x00010081], // Count 16.
            [0x00010003, 0x00010003], // Count 17.
            [0x000000C0, 0x000000C0], // Count 31.
            [0x00010081, 0x00010081], // Count 32.
            [0x00010003, 0x00010003], // Count 33.
            [0x000000C0, 0x000000C0], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x08010003, 0x08010003], // Count 1.
            [0x00000006, 0x00000006], // Count 2.
            [0x000000C0, 0x000000C0], // Count 7.
            [0x00000180, 0x00000180], // Count 8.
            [0x00000300, 0x00000300], // Count 9.
            [0x0000C000, 0x0000C000], // Count 15.
            [0x00018001, 0x00018001], // Count 16.
            [0x00010003, 0x00010003], // Count 17.
            [0x0000C000, 0x0000C000], // Count 31.
            [0x00018001, 0x00018001], // Count 32.
            [0x00010003, 0x00010003], // Count 33.
            [0x0000C000, 0x0000C000], // Count 255.
        ],
    },
    Operation {
        name: "ROR",
        extension: 1,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x000100C0, 0x000100C0], // Count 1.
            [0x00000060, 0x00000060], // Count 2.
            [0x00000003, 0x00000003], // Count 7.
            [0x00010081, 0x00010081], // Count 8.
            [0x000100C0, 0x000100C0], // Count 9.
            [0x00000003, 0x00000003], // Count 15.
            [0x00010081, 0x00010081], // Count 16.
            [0x000100C0, 0x000100C0], // Count 17.
            [0x00000003, 0x00000003], // Count 31.
            [0x00010081, 0x00010081], // Count 32.
            [0x000100C0, 0x000100C0], // Count 33.
            [0x00000003, 0x00000003], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x0001C000, 0x0001C000], // Count 1.
            [0x00006000, 0x00006000], // Count 2.
            [0x00000300, 0x00000300], // Count 7.
            [0x00000180, 0x00000180], // Count 8.
            [0x000000C0, 0x000000C0], // Count 9.
            [0x00000003, 0x00000003], // Count 15.
            [0x00018001, 0x00018001], // Count 16.
            [0x0001C000, 0x0001C000], // Count 17.
            [0x00000003, 0x00000003], // Count 31.
            [0x00018001, 0x00018001], // Count 32.
            [0x0001C000, 0x0001C000], // Count 33.
            [0x00000003, 0x00000003], // Count 255.
        ],
    },
    Operation {
        name: "RCL",
        extension: 2,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x08010002, 0x08010003], // Count 1.
            [0x00000005, 0x00000007], // Count 2.
            [0x000000A0, 0x000000E0], // Count 7.
            [0x00010040, 0x000100C0], // Count 8.
            [0x00000081, 0x00010081], // Count 9.
            [0x00000050, 0x00000070], // Count 15.
            [0x000000A0, 0x000000E0], // Count 16.
            [0x00010040, 0x000100C0], // Count 17.
            [0x00000014, 0x0000001C], // Count 31.
            [0x00000028, 0x00000038], // Count 32.
            [0x00000050, 0x00000070], // Count 33.
            [0x0000000A, 0x0000000E], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x08010002, 0x08010003], // Count 1.
            [0x00000005, 0x00000007], // Count 2.
            [0x000000A0, 0x000000E0], // Count 7.
            [0x00000140, 0x000001C0], // Count 8.
            [0x00000280, 0x00000380], // Count 9.
            [0x0000A000, 0x0000E000], // Count 15.
            [0x00014000, 0x0001C000], // Count 16.
            [0x00008001, 0x00018001], // Count 17.
            [0x00005000, 0x00007000], // Count 31.
            [0x0000A000, 0x0000E000], // Count 32.
            [0x00014000, 0x0001C000], // Count 33.
            [0x00008001, 0x00018001], // Count 255.
        ],
    },
    Operation {
        name: "RCR",
        extension: 3,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x08010040, 0x000100C0], // Count 1.
            [0x000000A0, 0x000000E0], // Count 2.
            [0x00000005, 0x00000007], // Count 7.
            [0x00010002, 0x00010003], // Count 8.
            [0x00000081, 0x00010081], // Count 9.
            [0x0000000A, 0x0000000E], // Count 15.
            [0x00000005, 0x00000007], // Count 16.
            [0x00010002, 0x00010003], // Count 17.
            [0x00000028, 0x00000038], // Count 31.
            [0x00000014, 0x0000001C], // Count 32.
            [0x0000000A, 0x0000000E], // Count 33.
            [0x00000050, 0x00000070], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x08014000, 0x0001C000], // Count 1.
            [0x0000A000, 0x0000E000], // Count 2.
            [0x00000500, 0x00000700], // Count 7.
            [0x00000280, 0x00000380], // Count 8.
            [0x00000140, 0x000001C0], // Count 9.
            [0x00000005, 0x00000007], // Count 15.
            [0x00010002, 0x00010003], // Count 16.
            [0x00008001, 0x00018001], // Count 17.
            [0x0000000A, 0x0000000E], // Count 31.
            [0x00000005, 0x00000007], // Count 32.
            [0x00010002, 0x00010003], // Count 33.
            [0x00008001, 0x00018001], // Count 255.
        ],
    },
    Operation {
        name: "SHL",
        extension: 4,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x08010002, 0x08010002], // Count 1.
            [0x00000004, 0x00000004], // Count 2.
            [0x00800080, 0x00800080], // Count 7.
            [0x00450000, 0x00450000], // Count 8.
            [0x00440000, 0x00440000], // Count 9.
            [0x00440000, 0x00440000], // Count 15.
            [0x00440000, 0x00440000], // Count 16.
            [0x00440000, 0x00440000], // Count 17.
            [0x00440000, 0x00440000], // Count 31.
            [0x00440000, 0x00440000], // Count 32.
            [0x00440000, 0x00440000], // Count 33.
            [0x00440000, 0x00440000], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x08010002, 0x08010002], // Count 1.
            [0x00000004, 0x00000004], // Count 2.
            [0x00000080, 0x00000080], // Count 7.
            [0x00040100, 0x00040100], // Count 8.
            [0x00040200, 0x00040200], // Count 9.
            [0x00848000, 0x00848000], // Count 15.
            [0x00450000, 0x00450000], // Count 16.
            [0x00440000, 0x00440000], // Count 17.
            [0x00440000, 0x00440000], // Count 31.
            [0x00440000, 0x00440000], // Count 32.
            [0x00440000, 0x00440000], // Count 33.
            [0x00440000, 0x00440000], // Count 255.
        ],
    },
    Operation {
        name: "SHR",
        extension: 5,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x08010040, 0x08010040], // Count 1.
            [0x00000020, 0x00000020], // Count 2.
            [0x00000001, 0x00000001], // Count 7.
            [0x00450000, 0x00450000], // Count 8.
            [0x00440000, 0x00440000], // Count 9.
            [0x00440000, 0x00440000], // Count 15.
            [0x00440000, 0x00440000], // Count 16.
            [0x00440000, 0x00440000], // Count 17.
            [0x00440000, 0x00440000], // Count 31.
            [0x00440000, 0x00440000], // Count 32.
            [0x00440000, 0x00440000], // Count 33.
            [0x00440000, 0x00440000], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x08054000, 0x08054000], // Count 1.
            [0x00042000, 0x00042000], // Count 2.
            [0x00040100, 0x00040100], // Count 7.
            [0x00000080, 0x00000080], // Count 8.
            [0x00000040, 0x00000040], // Count 9.
            [0x00000001, 0x00000001], // Count 15.
            [0x00450000, 0x00450000], // Count 16.
            [0x00440000, 0x00440000], // Count 17.
            [0x00440000, 0x00440000], // Count 31.
            [0x00440000, 0x00440000], // Count 32.
            [0x00440000, 0x00440000], // Count 33.
            [0x00440000, 0x00440000], // Count 255.
        ],
    },
    Operation {
        name: "SAR",
        extension: 7,
        byte: [
            [0x00000081, 0x00000081], // Count 0.
            [0x008500C0, 0x008500C0], // Count 1.
            [0x008000E0, 0x008000E0], // Count 2.
            [0x008400FF, 0x008400FF], // Count 7.
            [0x008500FF, 0x008500FF], // Count 8.
            [0x008500FF, 0x008500FF], // Count 9.
            [0x008500FF, 0x008500FF], // Count 15.
            [0x008500FF, 0x008500FF], // Count 16.
            [0x008500FF, 0x008500FF], // Count 17.
            [0x008500FF, 0x008500FF], // Count 31.
            [0x008500FF, 0x008500FF], // Count 32.
            [0x008500FF, 0x008500FF], // Count 33.
            [0x008500FF, 0x008500FF], // Count 255.
        ],
        word: [
            [0x00008001, 0x00008001], // Count 0.
            [0x0085C000, 0x0085C000], // Count 1.
            [0x0084E000, 0x0084E000], // Count 2.
            [0x0084FF00, 0x0084FF00], // Count 7.
            [0x0080FF80, 0x0080FF80], // Count 8.
            [0x0084FFC0, 0x0084FFC0], // Count 9.
            [0x0084FFFF, 0x0084FFFF], // Count 15.
            [0x0085FFFF, 0x0085FFFF], // Count 16.
            [0x0085FFFF, 0x0085FFFF], // Count 17.
            [0x0085FFFF, 0x0085FFFF], // Count 31.
            [0x0085FFFF, 0x0085FFFF], // Count 32.
            [0x0085FFFF, 0x0085FFFF], // Count 33.
            [0x0085FFFF, 0x0085FFFF], // Count 255.
        ],
    },
];

#[derive(Clone, Copy, Debug)]
enum Width {
    Byte,
    Word,
}

#[derive(Clone, Copy, Debug)]
enum CountSource {
    Cl,
    One,
}

#[derive(Clone, Copy, Debug)]
enum Target {
    Register(u8),
    DsDirect,
    DsIndexed,
    SsDisp8,
    SsDisp16,
    EsOverride,
}

// CL/CX are covered separately because their operand also contains the count.
const TARGETS: [Target; 12] = [
    Target::Register(0),
    Target::Register(2),
    Target::Register(3),
    Target::Register(4),
    Target::Register(5),
    Target::Register(6),
    Target::Register(7),
    Target::DsDirect,
    Target::DsIndexed,
    Target::SsDisp8,
    Target::SsDisp16,
    Target::EsOverride,
];

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.ss,
        cpu.es,
    ]
}

fn set_register(words: &mut [u16; 12], width: Width, rm: u8, value: u16) {
    match width {
        Width::Word => words[usize::from(rm)] = value,
        Width::Byte => {
            let index = usize::from(rm & 3);
            let shift = if rm < 4 { 0 } else { 8 };
            words[index] = (words[index] & !(0xFF << shift)) | (value << shift);
        }
    }
}

fn check_flags(
    actual: u16,
    initial: u16,
    packed: u32,
    operation: &Operation,
    width: Width,
    count: u8,
    context: &str,
) {
    if count == 0 {
        assert_eq!(
            actual, initial,
            "zero effective count preserves FLAGS: {context}"
        );
        return;
    }
    let bits = match width {
        Width::Byte => 8,
        Width::Word => 16,
    };
    let rotate = operation.extension < 4;
    let preserved = if rotate { !0x0801u16 } else { !0x08D5u16 };
    let mut defined = if rotate { 0x0001 } else { 0x00C4 }; // CF or SF/ZF/PF.
    if !rotate && (count < bits || operation.extension == 7) {
        defined |= 0x0001;
    }
    if count == 1 {
        defined |= 0x0800;
    }
    // AF is undefined for nonzero shifts, OF for counts > 1, and CF for
    // SHL/SHR counts >= operand width. Rotates preserve SF/ZF/AF/PF.
    let checked = preserved | defined;
    let expected = (initial & preserved) | ((packed >> 16) as u16 & defined);
    assert_eq!(actual & checked, expected, "defined FLAGS: {context}");
}

#[allow(clippy::too_many_arguments)]
fn check_case(
    is_286: bool,
    operation: &Operation,
    width: Width,
    source: CountSource,
    cl: u8,
    effective_count: u8,
    target: Target,
    input: u16,
    packed: u32,
    flag_pattern: u16,
) {
    let model = if is_286 {
        CpuModel::I80286
    } else {
        CpuModel::I8088
    };
    let mut cpu = Cpu::new(model, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA55A,
        0xB500 | u16::from(cl),
        0xC55A,
        0x0200,
        0x0800,
        0x0300,
        0x0040,
        0x0080,
    ];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x0100, 0x0200, 0x0300, 0x0400];
    cpu.ip = 0x0100;
    cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | flag_pattern;
    let initial_flags = cpu.flags;
    let mut memory = vec![0xCC; 0x6000];
    let mut instruction = Vec::new();
    if matches!(target, Target::EsOverride) {
        instruction.push(0x26);
    }
    let opcode = 0xD0
        | if matches!(width, Width::Word) { 1 } else { 0 }
        | if matches!(source, CountSource::Cl) {
            2
        } else {
            0
        };
    instruction.push(opcode);
    let mut displacement = Vec::new();
    let (rm, address) = match target {
        Target::Register(rm) => (0xC0 | rm, None),
        Target::DsDirect => {
            displacement.extend_from_slice(&[0x35, 0x12]);
            (0x06, Some(0x3235))
        }
        Target::DsIndexed => (0x00, Some(0x2240)), // DS:[BX+SI].
        Target::SsDisp8 => {
            displacement.push(0xFE);
            (0x46, Some(0x32FE))
        } // SS:[BP-2].
        Target::SsDisp16 => {
            displacement.extend_from_slice(&[0x00, 0x01]);
            (0x86, Some(0x3400))
        }
        Target::EsOverride => {
            displacement.extend_from_slice(&[0x35, 0x12]);
            (0x06, Some(0x5235))
        }
    };
    instruction.push(rm | (operation.extension << 3));
    instruction.extend_from_slice(&displacement);
    memory[0x1100..0x1100 + instruction.len()].copy_from_slice(&instruction);
    let mut expected_registers = registers(&cpu);
    if let Target::Register(rm) = target {
        set_register(&mut expected_registers, width, rm, input);
        [
            cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
        ] = expected_registers[..8].try_into().unwrap();
    } else {
        let address = address.unwrap();
        let length = if matches!(width, Width::Byte) { 1 } else { 2 };
        memory[address..address + length].copy_from_slice(&input.to_le_bytes()[..length]);
    }
    assert_eq!(cpu.cx as u8, cl, "instruction must use the original CL");
    let expected_value = packed as u16;
    let mut expected_memory = memory.clone();
    if let Target::Register(rm) = target {
        set_register(&mut expected_registers, width, rm, expected_value);
    } else {
        let address = address.unwrap();
        let length = if matches!(width, Width::Byte) { 1 } else { 2 };
        expected_memory[address..address + length]
            .copy_from_slice(&expected_value.to_le_bytes()[..length]);
    }
    let context = format!(
        "{} {width:?} {target:?}, 286={is_286}, {source:?}, CL={cl}, flags={initial_flags:04X}",
        operation.name
    );

    let result = cpu.step(&mut memory).expect(&context);

    assert!(matches!(result, CpuStepResult::Continue), "{context}");
    assert_eq!(registers(&cpu), expected_registers, "{context}");
    assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16, "{context}");
    check_flags(
        cpu.flags,
        initial_flags,
        packed,
        operation,
        width,
        effective_count,
        &context,
    );
    assert!(memory == expected_memory, "exact memory writes: {context}");
}

fn check_cl_counts(is_286: bool, operation_index: usize) {
    let operation = &OPERATIONS[operation_index];
    for width in [Width::Byte, Width::Word] {
        let (input, expected) = match width {
            Width::Byte => (0x0081, &operation.byte),
            Width::Word => (0x8001, &operation.word),
        };
        for (index, cl) in COUNTS.into_iter().enumerate() {
            let fixture = if is_286 { I286_INDEXES[index] } else { index };
            for (carry, flag_pattern) in [0x0000, 0x0FD5].into_iter().enumerate() {
                for target in TARGETS {
                    check_case(
                        is_286,
                        operation,
                        width,
                        CountSource::Cl,
                        cl,
                        COUNTS[fixture],
                        target,
                        input,
                        expected[fixture][carry],
                        flag_pattern,
                    );
                }
            }
        }
    }
}

macro_rules! count_tests {
    ($test_8088:ident, $test_286:ident, $index:expr) => {
        #[test]
        fn $test_8088() {
            check_cl_counts(false, $index);
        }
        #[test]
        fn $test_286() {
            check_cl_counts(true, $index);
        }
    };
}

count_tests!(i8088_rol_uses_full_cl, i286_rol_masks_cl, 0);
count_tests!(i8088_ror_uses_full_cl, i286_ror_masks_cl, 1);
count_tests!(i8088_rcl_uses_full_cl, i286_rcl_masks_cl, 2);
count_tests!(i8088_rcr_uses_full_cl, i286_rcr_masks_cl, 3);
count_tests!(i8088_shl_uses_full_cl, i286_shl_masks_cl, 4);
count_tests!(i8088_shr_uses_full_cl, i286_shr_masks_cl, 5);
count_tests!(i8088_sar_uses_full_cl, i286_sar_masks_cl, 6);

#[test]
fn i8088_implicit_one_opcodes_ignore_cl() {
    check_implicit_one(false);
}

#[test]
fn i286_implicit_one_opcodes_ignore_cl() {
    check_implicit_one(true);
}

fn check_implicit_one(is_286: bool) {
    for operation in &OPERATIONS {
        for width in [Width::Byte, Width::Word] {
            let (input, expected) = match width {
                Width::Byte => (0x0081, &operation.byte),
                Width::Word => (0x8001, &operation.word),
            };
            for cl in [0, 32, 255] {
                for (carry, flag_pattern) in [0x0000, 0x0FD5].into_iter().enumerate() {
                    for target in TARGETS {
                        check_case(
                            is_286,
                            operation,
                            width,
                            CountSource::One,
                            cl,
                            1,
                            target,
                            input,
                            expected[1][carry],
                            flag_pattern,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn i8088_shifting_cl_or_cx_uses_the_original_count() {
    check_count_destination(false);
}

#[test]
fn i286_shifting_cl_or_cx_uses_the_original_count() {
    check_count_destination(true);
}

fn check_count_destination(is_286: bool) {
    // (CL, byte 8088, byte 286, word 8088, word 286, 286 effective count).
    for (cl, byte_8088, byte_286, word_8088, word_286, masked) in [
        (0, 0x00000000, 0x00000000, 0x0000B500, 0x0000B500, 0),
        (1, 0x00000002, 0x00000002, 0x08016A02, 0x08016A02, 1),
        (31, 0x00440000, 0x00440000, 0x00440000, 0x00440000, 31),
        (32, 0x00440000, 0x00000020, 0x00440000, 0x0000B520, 0),
        (33, 0x00440000, 0x00040042, 0x00440000, 0x08056A42, 1),
        (255, 0x00440000, 0x00440000, 0x00440000, 0x00440000, 31),
    ] {
        for width in [Width::Byte, Width::Word] {
            let (input, expected) = match width {
                Width::Byte => (u16::from(cl), if is_286 { byte_286 } else { byte_8088 }),
                Width::Word => (
                    0xB500 | u16::from(cl),
                    if is_286 { word_286 } else { word_8088 },
                ),
            };
            for flag_pattern in [0x0000, 0x0FD5] {
                check_case(
                    is_286,
                    &OPERATIONS[4],
                    width,
                    CountSource::Cl,
                    cl,
                    if is_286 { masked } else { cl },
                    Target::Register(1),
                    input,
                    expected,
                    flag_pattern,
                );
            }
        }
    }
}
