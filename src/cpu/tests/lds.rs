use super::{Cpu, CpuError, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup() -> Cpu {
    let mut cpu = Cpu::new(0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA55A, 0x1234, 0x5678, 0x0100, 0x0900, 0x0800, 0x0200, 0x0400,
    ];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD7;
    cpu
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn lds_loads_little_endian_offset_and_segment_into_every_word_register() {
    for reg in 0..8 {
        for (pointer, offset, segment) in [
            ([0x34, 0x12, 0x78, 0x56], 0x1234, 0x5678),
            ([0x00, 0x00, 0x00, 0x00], 0x0000, 0x0000),
            ([0xFF, 0xFF, 0xFF, 0xFF], 0xFFFF, 0xFFFF),
        ] {
            for flags in [0, 0x0FD7] {
                let mut cpu = setup();
                cpu.flags = flags;
                let mut expected = registers(&cpu);
                expected[reg] = offset;
                expected[11] = segment;
                let mut memory = vec![0xCC; 0x50000];
                // Unaligned pointer at the old DS:0201h.
                memory[0x10100..0x10104].copy_from_slice(&[
                    0xC5,
                    0x06 | ((reg as u8) << 3),
                    0x01,
                    0x02,
                ]);
                memory[0x20201..0x20205].copy_from_slice(&pointer);
                let original_memory = memory.clone();

                step(&mut cpu, &mut memory);

                assert_eq!(registers(&cpu), expected, "destination={reg}");
                assert_eq!(cpu.flags, flags);
                assert_eq!(cpu.ip, 0x0104);
                assert!(memory == original_memory);
            }
        }
    }
}

#[test]
fn lds_resolves_the_whole_pointer_before_changing_ds_or_an_address_register() {
    for (reg, addressing, displacement, offset, default_base) in [
        (3, 0x00, &[][..], 0x0300, 0x20000),           // LDS BX,[BX+SI].
        (6, 0x04, &[][..], 0x0200, 0x20000),           // LDS SI,[SI].
        (5, 0x46, &[0xFE][..], 0x07FE, 0x30000),       // LDS BP,[BP-2].
        (7, 0x83, &[0xF0, 0xFF][..], 0x0BF0, 0x30000), // LDS DI,[BP+DI-16].
    ] {
        for (prefix, source_base) in [
            (None, default_base),
            (Some(0x26), 0x40000),
            (Some(0x2E), 0x10000),
            (Some(0x36), 0x30000),
            (Some(0x3E), 0x20000),
        ] {
            let mut cpu = setup();
            let mut instruction = Vec::new();
            if let Some(prefix) = prefix {
                instruction.push(prefix);
            }
            instruction.extend_from_slice(&[0xC5, addressing | ((reg as u8) << 3)]);
            instruction.extend_from_slice(displacement);
            let mut memory = vec![0xCC; 0x50000];
            memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
            let address = source_base + offset;
            memory[address..address + 4].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
            let original_memory = memory.clone();
            let mut expected = registers(&cpu);
            expected[reg] = 0x1234;
            expected[11] = 0x5678; // Even an ES/SS/CS override still loads DS.

            step(&mut cpu, &mut memory);

            assert_eq!(
                registers(&cpu),
                expected,
                "destination={reg}, prefix={prefix:02X?}"
            );
            assert_eq!(cpu.flags, 0x0FD7);
            assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn lds_reads_a_pointer_across_the_twenty_bit_address_boundary() {
    for (offset, addresses) in [
        (0x000Cu16, [0xFFFFC, 0xFFFFD, 0xFFFFE, 0xFFFFF]),
        (0x000D, [0xFFFFD, 0xFFFFE, 0xFFFFF, 0x00000]),
        (0x000E, [0xFFFFE, 0xFFFFF, 0x00000, 0x00001]),
        (0x000F, [0xFFFFF, 0x00000, 0x00001, 0x00002]),
    ] {
        let mut cpu = setup();
        cpu.ds = 0xFFFF;
        let mut memory = vec![0xCC; 0x100000];
        let [low, high] = offset.to_le_bytes();
        memory[0x10100..0x10104].copy_from_slice(&[0xC5, 0x36, low, high]); // LDS SI,[offset].
        for (address, byte) in addresses.into_iter().zip([0x34, 0x12, 0x78, 0x56]) {
            memory[address] = byte;
        }
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[6] = 0x1234;
        expected[11] = 0x5678;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 0x0104);
        assert!(memory == original_memory);
    }
}

#[test]
fn lds_decodes_modrm_and_displacement_across_ip_wrap() {
    for ip in [0xFFFCu16, 0xFFFD, 0xFFFE, 0xFFFF] {
        let mut cpu = setup();
        cpu.ip = ip;
        let mut memory = vec![0xCC; 0x50000];
        for (index, byte) in [0xC5, 0x1E, 0x01, 0x02].into_iter().enumerate() {
            memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
        }
        memory[0x20201..0x20205].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
        let original_memory = memory.clone();
        let mut expected = registers(&cpu);
        expected[3] = 0x1234;
        expected[11] = 0x5678;

        step(&mut cpu, &mut memory);

        assert_eq!(registers(&cpu), expected);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, ip.wrapping_add(4));
        assert!(memory == original_memory);
    }
}

#[test]
fn lds_rejects_register_sources_without_changing_registers_or_flags() {
    for destination in 0..8 {
        for source in 0..8 {
            let mut cpu = setup();
            cpu.cs = 0;
            cpu.ip = 0;
            let original = registers(&cpu);
            let instruction = [0xC5, 0xC0 | (destination << 3) | source];
            let mut memory = instruction;

            assert!(matches!(
                cpu.step(&mut memory),
                Err(CpuError::UnsupportedRmField(_))
            ));

            assert_eq!(registers(&cpu), original);
            assert_eq!(cpu.flags, 0x0FD7);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, instruction);
        }
    }
}

#[test]
fn lds_does_not_partially_load_registers_when_the_pointer_is_truncated() {
    for available in 0..4 {
        let mut cpu = setup();
        cpu.cs = 0;
        cpu.ds = 0;
        cpu.ip = 0;
        let original = registers(&cpu);
        let mut memory = vec![0xCC; 0x0200 + available];
        memory[..4].copy_from_slice(&[0xC5, 0x1E, 0x00, 0x02]); // LDS BX,[0200h].
        memory[0x0200..].copy_from_slice(&[0x34, 0x12, 0x78, 0x56][..available]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), original, "available bytes={available}");
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 4);
        assert!(memory == original_memory);
    }
}
