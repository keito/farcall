use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

// Intel PUSH: 68 iw pushes a word; 6A ib sign-extends the byte to a word.
// https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf
// On 8088 these bytes instead alias JS (68) and JP (6A).
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

// (initial SP, final SP, physical address of the pushed word at SS=3000h).
const STACKS: [(u16, u16, usize); 4] = [
    (0x1000, 0x0FFE, 0x30FFE),
    (0x0002, 0x0000, 0x30000),
    (0x0000, 0xFFFE, 0x3FFFE),
    (0xFFFE, 0xFFFC, 0x3FFFC),
];

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn seed_registers(cpu: &mut Cpu) {
    [cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di] =
        [0xA55A, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
    cpu.ip = 0x0100;
    cpu.sp = 0x1000;
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn i286_68_pushes_full_little_endian_immediate_and_preserves_flags() {
    for value in [
        0x0000u16, 0x007F, 0x0080, 0x00FF, 0x0100, 0x1234, 0x7FFF, 0x8000, 0xFF80, 0xFFFF,
    ] {
        let bytes = value.to_le_bytes();
        check_prefixed_pushes(&[0x68, bytes[0], bytes[1]], bytes);
    }
}

#[test]
fn i286_6a_sign_extends_every_immediate_byte() {
    for immediate in 0..=255u8 {
        // Check encoded sign bits directly, independently of Rust's signed casts.
        let high = if immediate < 0x80 { 0x00 } else { 0xFF };
        check_push(&[0x6A, immediate], [immediate, high], STACKS[0]);
    }
}

#[test]
fn i286_6a_sign_extension_boundaries_work_with_prefixes_and_stack_wrapping() {
    for (immediate, bytes) in [
        (0x00, [0x00, 0x00]),
        (0x01, [0x01, 0x00]),
        (0x7F, [0x7F, 0x00]),
        (0x80, [0x80, 0xFF]),
        (0x81, [0x81, 0xFF]),
        (0xFF, [0xFF, 0xFF]),
    ] {
        check_prefixed_pushes(&[0x6A, immediate], bytes);
    }
}

fn check_prefixed_pushes(instruction: &[u8], bytes: [u8; 2]) {
    // Segment overrides must not redirect an implicit stack write away from SS.
    for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
        let mut prefixed: Vec<u8> = prefix.into_iter().collect();
        prefixed.extend_from_slice(instruction);
        for stack in STACKS {
            check_push(&prefixed, bytes, stack);
        }
    }
}

fn check_push(instruction: &[u8], bytes: [u8; 2], (sp, final_sp, address): (u16, u16, usize)) {
    for flags in [0x0002, 0x0FD7] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        seed_registers(&mut cpu);
        cpu.sp = sp;
        cpu.flags = flags;
        let mut expected_registers = registers(&cpu);
        expected_registers[4] = final_sp;
        let mut memory = vec![0xCC; 0x50000];
        memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
        let mut expected_memory = memory.clone();
        expected_memory[address..address + 2].copy_from_slice(&bytes);

        step(&mut cpu, &mut memory);

        assert_eq!(
            registers(&cpu),
            expected_registers,
            "instruction={instruction:02X?}"
        );
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert_eq!(cpu.flags, flags, "PUSH preserves every flag");
        assert!(
            memory == expected_memory,
            "write exactly the two stack bytes: {instruction:02X?}"
        );
    }
}

#[test]
fn i286_immediate_pushes_round_trip_through_pop() {
    for sp in [0x1000, 0x0000] {
        for flags in [0x0002, 0x0FD7] {
            let mut cpu = Cpu::new(CpuModel::I80286, 0);
            seed_registers(&mut cpu);
            cpu.sp = sp;
            cpu.flags = flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[0] = 0x1234; // AX after POP AX.
            expected_registers[3] = 0xFFFF; // BX after POP BX.
            let mut memory = vec![0xCC; 0x50000];
            memory[0x10100..0x10107].copy_from_slice(&[
                0x68, 0x34, 0x12, // PUSH word 1234h.
                0x58, // POP AX.
                0x6A, 0xFF, // PUSH byte -1.
                0x5B, // POP BX.
            ]);
            let mut expected_memory = memory.clone();
            let address = if sp == 0 { 0x3FFFE } else { 0x30FFE };
            expected_memory[address..address + 2].copy_from_slice(&[0xFF, 0xFF]);

            for _ in 0..4 {
                step(&mut cpu, &mut memory);
            }

            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.ip, 0x0107);
            assert_eq!(cpu.flags, flags);
            assert!(
                memory == expected_memory,
                "POP must leave the last pushed word intact"
            );
        }
    }
}

#[test]
fn i286_immediate_pushes_do_not_fetch_past_the_immediate() {
    for (ip, instruction, bytes) in [
        (0x3FFD, &[0x68, 0x34, 0x12][..], [0x34, 0x12]),
        (0x3FFE, &[0x6A, 0x80][..], [0x80, 0xFF]),
    ] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        seed_registers(&mut cpu);
        cpu.cs = 0;
        cpu.ss = 0;
        cpu.ip = ip;
        let flags = cpu.flags;
        let mut expected_registers = registers(&cpu);
        expected_registers[4] = 0x0FFE;
        let mut memory = vec![0xCC; 0x4000];
        memory[usize::from(ip)..].copy_from_slice(instruction);
        let mut expected_memory = memory.clone();
        expected_memory[0x0FFE..0x1000].copy_from_slice(&bytes);

        step(&mut cpu, &mut memory);

        assert_eq!(cpu.ip, 0x4000);
        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.flags, flags);
        assert!(memory == expected_memory);
    }
}

#[test]
fn i286_immediate_pushes_report_short_ram_without_partial_word_writes() {
    for instruction in [&[0x68, 0x34, 0x12][..], &[0x6A, 0x80]] {
        // One destination byte available, neither byte available, wrapped SP outside RAM.
        for sp in [0x0201, 0x0202, 0x0000] {
            let mut cpu = Cpu::new(CpuModel::I80286, 0);
            seed_registers(&mut cpu);
            cpu.cs = 0;
            cpu.ss = 0;
            cpu.sp = sp;
            let flags = cpu.flags;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x0200];
            memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
            let original_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory),
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ));

            // A failed write may have decremented SP; no guest fault rollback is asserted.
            let mut expected_registers = original_registers;
            expected_registers[4] = cpu.sp;
            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.flags, flags);
            assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
            assert_eq!(memory, original_memory);
        }
    }
}

#[test]
fn i8088_68_alias_is_js() {
    check_jump(false, 0x68);
}

#[test]
fn i8088_6a_alias_is_jp() {
    check_jump(false, 0x6A);
}

#[test]
fn i8088_78_and_7a_remain_conditional_jumps() {
    check_jump(false, 0x78);
    check_jump(false, 0x7A);
}

#[test]
fn i286_78_and_7a_remain_conditional_jumps() {
    check_jump(true, 0x78);
    check_jump(true, 0x7A);
}

fn check_jump(is_286: bool, opcode: u8) {
    let cases = [
        (0x0100u16, 0x00, 0x0102, 0x0102),
        (0x0100, 0x05, 0x0107, 0x0102),
        (0x0100, 0x7F, 0x0181, 0x0102),
        (0x0100, 0x80, 0x0082, 0x0102),
        (0x0100, 0xFE, 0x0100, 0x0102),
        (0x0100, 0xFF, 0x0101, 0x0102),
        (0xFFFD, 0x01, 0x0000, 0xFFFF),
        (0xFFFE, 0xFE, 0xFFFE, 0x0000),
        (0xFFFF, 0x02, 0x0003, 0x0001),
        (0x0000, 0x80, 0xFF82, 0x0002),
    ];
    let cases = if is_286 { &cases[..6] } else { &cases[..] };
    for &(ip, displacement, taken_ip, untaken_ip) in cases {
        // SF and PF vary independently, with other flags both clear and set.
        for (low_flags, js_taken, jp_taken) in [
            (0x0002, false, false),
            (0x0082, true, false),
            (0x0006, false, true),
            (0x0086, true, true),
            (0x0F53, false, false),
            (0x0FD7, true, true),
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
            let mut memory = vec![0xCC; 0x50000];
            memory[0x10000 + usize::from(ip)] = opcode;
            memory[0x10000 + usize::from(ip.wrapping_add(1))] = displacement;
            let original_memory = memory.clone();
            let taken = if opcode == 0x68 || opcode == 0x78 {
                js_taken
            } else {
                jp_taken
            };

            step(&mut cpu, &mut memory);

            assert_eq!(
                cpu.ip,
                if taken { taken_ip } else { untaken_ip },
                "opcode={opcode:02X}, flags={flags:04X}, displacement={displacement:02X}"
            );
            assert_eq!(
                registers(&cpu),
                original_registers,
                "jump must not push an immediate"
            );
            assert_eq!(cpu.flags, flags);
            assert!(
                memory == original_memory,
                "jump leaves stack and memory intact"
            );
        }
    }
}
