use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup(instruction: &[u8], ip: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA55A, 0x1234, 0x5678, 0x0100, 0x0900, 0x0800, 0x0200, 0x0400,
    ];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = ip;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x60000];
    for (index, byte) in instruction.iter().copied().enumerate() {
        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
    }
    memory[0x11234] = 0x90; // Near target 1000:1234: NOP.
    memory[0x579B4] = 0x90; // Far target 5678:1234: NOP.
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn check_memory_jump(mut cpu: Cpu, mut memory: Vec<u8>, far: bool) {
    let mut expected = registers(&cpu);
    if far {
        expected[9] = 0x5678;
    }
    let flags = cpu.flags;
    let original_memory = memory.clone();

    step(&mut cpu, &mut memory);

    assert_eq!(
        cpu.ip, 0x1234,
        "target is an absolute offset, not a displacement"
    );
    assert_eq!(registers(&cpu), expected, "only the far jump may change CS");
    assert_eq!(cpu.flags, flags);
    assert!(
        memory == original_memory,
        "JMP must not push a return address or modify its operand"
    );

    step(&mut cpu, &mut memory); // Fetch NOP from the new CS:IP.

    assert_eq!(cpu.ip, 0x1235);
    assert_eq!(registers(&cpu), expected);
    assert_eq!(cpu.flags, flags);
    assert!(memory == original_memory);
}

#[test]
fn near_indirect_jump_accepts_every_word_register_and_preserves_all_other_state() {
    for reg in 0..8 {
        for target in [0x0000, 0x1234, 0xFFFF] {
            for flags in [0, 0x0FD7] {
                let (mut cpu, mut memory) = setup(&[0xFF, 0xE0 | reg as u8], 0x0100);
                let mut values = [
                    0xA55A, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A, 0x89AB,
                ];
                values[reg] = target;
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = values;
                cpu.flags = flags;
                let original = registers(&cpu);
                let original_memory = memory.clone();

                step(&mut cpu, &mut memory);

                assert_eq!(cpu.ip, target, "register={reg}");
                assert_eq!(registers(&cpu), original);
                assert_eq!(cpu.flags, flags);
                assert!(memory == original_memory);
            }
        }
    }
}

fn check_addressing_forms(far: bool) {
    let extension = if far { 5 } else { 4 };
    for (addressing, displacement, offset, default_base) in [
        (0x06, &[0x01, 0x02][..], 0x0201, 0x20000), // Direct, unaligned DS address.
        (0x00, &[][..], 0x0300, 0x20000),           // [BX+SI], DS.
        (0x46, &[0xFE][..], 0x07FE, 0x30000),       // [BP-2], SS.
        (0x83, &[0xF0, 0xFF][..], 0x0BF0, 0x30000), // [BP+DI-16], SS.
    ] {
        for (prefix, source_base) in [
            (None, default_base),
            (Some(0x26), 0x40000),
            (Some(0x2E), 0x10000),
            (Some(0x36), 0x30000),
            (Some(0x3E), 0x20000),
        ] {
            for flags in [0, 0x0FD7] {
                let mut instruction = Vec::new();
                if let Some(prefix) = prefix {
                    instruction.push(prefix);
                }
                instruction.extend_from_slice(&[0xFF, addressing | (extension << 3)]);
                instruction.extend_from_slice(displacement);
                let (mut cpu, mut memory) = setup(&instruction, 0x0100);
                cpu.flags = flags;
                let address = source_base + offset;
                // /4 must ignore the second word; /5 must load it into CS.
                memory[address..address + 4].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);

                check_memory_jump(cpu, memory, far);
            }
        }
    }
}

#[test]
fn near_indirect_jump_reads_one_word_using_normal_segment_and_displacement_rules() {
    check_addressing_forms(false);
}

#[test]
fn far_indirect_jump_reads_offset_then_segment_using_normal_addressing_rules() {
    check_addressing_forms(true);
}

#[test]
fn near_indirect_jump_needs_only_two_source_bytes_and_does_not_read_the_target_yet() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.sp = 0xFFFF;
    cpu.flags = 0x0FD7;
    let original = registers(&cpu);
    let mut memory = vec![0xCC; 0x0102];
    memory[..4].copy_from_slice(&[0xFF, 0x26, 0x00, 0x01]);
    memory[0x0100..].copy_from_slice(&[0xEF, 0xBE]);
    let original_memory = memory.clone();

    step(&mut cpu, &mut memory);

    assert_eq!(cpu.ip, 0xBEEF);
    assert_eq!(registers(&cpu), original);
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == original_memory);
}

#[test]
fn indirect_jumps_decode_prefix_modrm_and_displacement_across_ip_wrap() {
    for far in [false, true] {
        for ip in [0xFFFB, 0xFFFC, 0xFFFD, 0xFFFE, 0xFFFF] {
            let mod_rm = if far { 0x2E } else { 0x26 };
            let (cpu, mut memory) = setup(&[0x2E, 0xFF, mod_rm, 0x01, 0x02], ip);
            memory[0x10201..0x10205].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);

            check_memory_jump(cpu, memory, far);
        }
    }
}

#[test]
fn indirect_jumps_read_operands_across_the_twenty_bit_address_boundary() {
    for far in [false, true] {
        let mod_rm = if far { 0x2E } else { 0x26 };
        let (mut cpu, mut memory) = setup(&[0xFF, mod_rm, 0x0F, 0x00], 0x0100);
        cpu.ds = 0xFFFF;
        memory.resize(0x100000, 0xCC);
        for (address, byte) in [0xFFFFF, 0x00000, 0x00001, 0x00002]
            .into_iter()
            .zip([0x34, 0x12, 0x78, 0x56])
        {
            memory[address] = byte;
        }

        check_memory_jump(cpu, memory, far);
    }
}

#[test]
fn far_indirect_jump_rejects_register_operands_without_changing_state() {
    for reg in 0..8 {
        let (mut cpu, mut memory) = setup(&[0xFF, 0xE8 | reg], 0x0100);
        let original = registers(&cpu);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedRmField(_))
        ));

        assert_eq!(registers(&cpu), original);
        assert_eq!(cpu.ip, 0x0102);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
    }
}

#[test]
fn indirect_jumps_reject_truncated_operands_without_loading_a_partial_target() {
    for (mod_rm, required) in [(0x26, 2), (0x2E, 4)] {
        for available in 0..required {
            let mut cpu = Cpu::new(CpuModel::I8088, 0);
            cpu.sp = 0x0080;
            cpu.flags = 0x0FD7;
            let original = registers(&cpu);
            let mut memory = vec![0xCC; 0x0100 + available];
            memory[..4].copy_from_slice(&[0xFF, mod_rm, 0x00, 0x01]);
            memory[0x0100..].copy_from_slice(&[0x34, 0x12, 0x78, 0x56][..available]);
            let original_memory = memory.clone();

            assert!(matches!(
                cpu.step(&mut memory),
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ));

            assert_eq!(registers(&cpu), original);
            assert_eq!(cpu.ip, 4);
            assert_eq!(cpu.flags, 0x0FD7);
            assert!(memory == original_memory);
        }
    }
}
