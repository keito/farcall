use super::{Cpu, CpuError, CpuStepResult, MemoryError};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn assert_mul_flags(actual: u16, initial: u16, expected_cf_of: u16) {
    // MUL defines CF/OF; SF/ZF/AF/PF are undefined, so do not constrain them.
    assert_eq!(actual & 0x0801, expected_cf_of);
    assert_eq!(
        actual & !0x08D5,
        initial & !0x08D5,
        "preserve control flags and unrelated bits"
    );
}

#[test]
fn mul_byte_computes_every_unsigned_product_and_sets_or_clears_cf_of() {
    for al in 0..=255u16 {
        for bl in 0..=255u16 {
            for flags in [0x0000, 0x0FD7] {
                let mut cpu = Cpu::new();
                cpu.ax = 0xAB00 | al; // Old AH must not contribute to the product.
                cpu.bx = 0xCD00 | bl;
                cpu.dx = 0xDEAD; // Byte MUL leaves DX unchanged.
                cpu.flags = flags;
                let product = al * bl;
                let mut expected_registers = registers(&cpu);
                expected_registers[0] = product;
                let mut memory = [0xF6, 0xE3]; // MUL BL; no immediate bytes exist.

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    registers(&cpu),
                    expected_registers,
                    "AL={al:02X}, BL={bl:02X}"
                );
                assert_mul_flags(cpu.flags, flags, if product > 255 { 0x0801 } else { 0 });
                assert_eq!(cpu.ip, 2);
                assert_eq!(memory, [0xF6, 0xE3]);
            }
        }
    }
}

#[test]
fn mul_byte_reads_every_register_before_overwriting_ax() {
    // AL=81, CL=03, DL=05, BL=07, AH=02, CH=04, DH=06, BH=08.
    let expected_products = [
        0x4101, 0x0183, 0x0285, 0x0387, 0x0102, 0x0204, 0x0306, 0x0408,
    ];
    for (source, product) in expected_products.into_iter().enumerate() {
        let mut cpu = Cpu::new();
        cpu.ax = 0x0281;
        cpu.cx = 0x0403;
        cpu.dx = 0x0605;
        cpu.bx = 0x0807;
        cpu.flags = 0x0700;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = product;
        let mut memory = [0xF6, 0xE0 | source as u8];
        let original_memory = memory;

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers, "source={source}");
        assert_mul_flags(cpu.flags, 0x0700, 0x0801);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn mul_byte_reads_segmented_memory_and_handles_bus_wrapping_without_writing() {
    for (instruction, ds, address) in [
        (&[0xF6, 0x24][..], 0x1000, 0x10800),       // MUL byte [SI].
        (&[0xF6, 0x66, 0xFE][..], 0x1000, 0x207FE), // MUL byte [BP-2], using SS.
        (&[0xF6, 0xA4, 0x34, 0x12][..], 0x1000, 0x11A34), // MUL byte [SI+1234].
        (&[0x26, 0xF6, 0x24][..], 0x1000, 0x30800), // MUL byte [ES:SI].
        (&[0x26, 0xF6, 0x66, 0xFE][..], 0x1000, 0x307FE), // ES overrides SS too.
        (&[0xF6, 0x26, 0x20, 0x00][..], 0xFFFF, 0x00010), // Wrap the 20-bit address.
        (&[0xF6, 0x26, 0x0F, 0x00][..], 0xFFFF, 0xFFFFF), // Last byte of memory.
    ] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xABFF;
        cpu.dx = 0xDEAD;
        cpu.si = 0x0800;
        cpu.bp = 0x0800;
        cpu.ds = ds;
        cpu.ss = 0x2000;
        cpu.es = 0x3000;
        cpu.ip = 0x0100;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0xFE01; // FF * FF = FE01.
        let mut memory = vec![0xCC; 0x100000];
        memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
        memory[address] = 0xFF;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_mul_flags(cpu.flags, 0x0FD7, 0x0801);
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert!(memory == original_memory, "instruction={instruction:02X?}");
    }
}

#[test]
fn mul_byte_fetches_mod_rm_and_displacement_across_ip_wrap_using_cs() {
    let instruction = [0xF6, 0x26, 0x34, 0x12];
    for (ip, next_ip) in [(0xFFFC, 0), (0xFFFD, 1), (0xFFFE, 2), (0xFFFF, 3)] {
        let mut cpu = Cpu::new();
        cpu.cs = 0x3000;
        cpu.ds = 0x1000;
        cpu.ip = ip;
        cpu.ax = 0xAB02;
        cpu.dx = 0xDEAD;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0x0068; // 02 * 34; old AH is replaced with zero.
        let mut memory = vec![0xCC; 0x50000];
        for (index, byte) in instruction.into_iter().enumerate() {
            memory[0x30000 + usize::from(ip.wrapping_add(index as u16))] = byte;
        }
        memory[0x11234] = 0x34;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, next_ip);
        assert_mul_flags(cpu.flags, 0x0FD7, 0);
        assert!(memory == original_memory);
    }
}

#[test]
fn mul_byte_memory_errors_preserve_registers_flags_and_memory() {
    let mut cpu = Cpu::new();
    cpu.ax = 0x1234;
    cpu.dx = 0x5678;
    cpu.flags = 0x0FD7;
    let original_registers = registers(&cpu);
    let mut memory = vec![0xCC; 0x0200];
    memory[..4].copy_from_slice(&[0xF6, 0x26, 0x00, 0x02]); // MUL byte [0200].
    let original_memory = memory.clone();

    assert!(matches!(
        cpu.step(&mut memory),
        Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
    ));

    assert_eq!(registers(&cpu), original_registers);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(memory, original_memory);
}

#[test]
fn mul_word_computes_full_unsigned_product_and_sets_or_clears_cf_of() {
    for (ax, bx, expected_dx, expected_ax, expected_flags) in [
        (0x0000, 0xFFFF, 0x0000, 0x0000, 0x0000),
        (0xFFFF, 0x0000, 0x0000, 0x0000, 0x0000),
        (0xFFFF, 0x0001, 0x0000, 0xFFFF, 0x0000), // Unsigned, despite the sign bit.
        (0x0100, 0x0100, 0x0001, 0x0000, 0x0801), // Low half zero, upper half nonzero.
        (0xFFFF, 0x0002, 0x0001, 0xFFFE, 0x0801),
        (0xFFFF, 0xFFFF, 0xFFFE, 0x0001, 0x0801), // Largest possible product.
    ] {
        for flags in [0x0000, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = ax;
            cpu.bx = bx;
            cpu.dx = 0xDEAD; // MUL ignores the old high half and replaces it.
            cpu.flags = flags;
            let mut expected_registers = registers(&cpu);
            expected_registers[0] = expected_ax;
            expected_registers[2] = expected_dx;
            let mut memory = [0xF7, 0xE3]; // MUL BX; no immediate bytes exist.

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(
                registers(&cpu),
                expected_registers,
                "AX={ax:04X}, BX={bx:04X}"
            );
            assert_mul_flags(cpu.flags, flags, expected_flags);
            assert_eq!(cpu.ip, 2);
            assert_eq!(memory, [0xF7, 0xE3]);
        }
    }
}

#[test]
fn mul_word_reads_every_register_before_overwriting_ax_and_dx() {
    let initial = [0x8001, 2, 3, 4, 5, 6, 7, 8];
    let expected_products = [
        (0x4001, 0x0001),
        (0x0001, 0x0002),
        (0x0001, 0x8003),
        (0x0002, 0x0004),
        (0x0002, 0x8005),
        (0x0003, 0x0006),
        (0x0003, 0x8007),
        (0x0004, 0x0008),
    ];
    for (source, (dx, ax)) in expected_products.into_iter().enumerate() {
        let mut cpu = Cpu::new();
        [
            cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
        ] = initial;
        cpu.flags = 0x0700;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = ax;
        expected_registers[2] = dx;
        let mut memory = [0xF7, 0xE0 | source as u8];
        let original_memory = memory;

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers, "source={source}");
        assert_mul_flags(cpu.flags, 0x0700, 0x0801);
        assert_eq!(cpu.ip, 2);
        assert_eq!(memory, original_memory);
    }
}

#[test]
fn mul_word_reads_segmented_memory_and_handles_bus_wrapping_without_writing() {
    for (instruction, ds, low, high) in [
        (&[0xF7, 0x24][..], 0x1000, 0x10800, 0x10801), // MUL [SI].
        (&[0xF7, 0x66, 0xFE][..], 0x1000, 0x207FE, 0x207FF), // MUL [BP-2].
        (&[0xF7, 0xA4, 0x34, 0x12][..], 0x1000, 0x11A34, 0x11A35), // MUL [SI+1234].
        (&[0xF7, 0x26, 0x20, 0x00][..], 0xFFFF, 0x00010, 0x00011),
        (&[0xF7, 0x26, 0x0F, 0x00][..], 0xFFFF, 0xFFFFF, 0x00000),
    ] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x0100;
        cpu.dx = 0xDEAD;
        cpu.si = 0x0800;
        cpu.bp = 0x0800;
        cpu.ds = ds;
        cpu.ss = 0x2000;
        cpu.ip = 0x0100;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0x3400;
        expected_registers[2] = 0x0012; // 0100 * 1234 = 00123400.
        let mut memory = vec![0xCC; 0x100000];
        memory[0x0100..0x0100 + instruction.len()].copy_from_slice(instruction);
        memory[low] = 0x34;
        memory[high] = 0x12;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_mul_flags(cpu.flags, 0x0FD7, 0x0801);
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert!(memory == original_memory, "instruction={instruction:02X?}");
    }
}

#[test]
fn mul_word_fetches_mod_rm_and_displacement_across_ip_wrap_using_cs() {
    let instruction = [0xF7, 0x26, 0x34, 0x12];
    for (ip, next_ip) in [(0xFFFC, 0), (0xFFFD, 1), (0xFFFE, 2), (0xFFFF, 3)] {
        let mut cpu = Cpu::new();
        cpu.cs = 0x3000;
        cpu.ds = 0x1000;
        cpu.ip = ip;
        cpu.ax = 2;
        cpu.flags = 0x0FD7;
        let mut expected_registers = registers(&cpu);
        expected_registers[0] = 0x2468;
        let mut memory = vec![0xCC; 0x50000];
        let before_wrap = 0x10000 - usize::from(ip);
        memory[0x30000 + usize::from(ip)..0x40000].copy_from_slice(&instruction[..before_wrap]);
        memory[0x30000..0x30000 + instruction.len() - before_wrap]
            .copy_from_slice(&instruction[before_wrap..]);
        memory[0x11234..0x11236].copy_from_slice(&[0x34, 0x12]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(registers(&cpu), expected_registers);
        assert_eq!(cpu.ip, next_ip);
        assert_mul_flags(cpu.flags, 0x0FD7, 0);
        assert!(memory == original_memory);
    }
}

#[test]
fn mul_word_memory_errors_preserve_registers_flags_and_memory() {
    for offset in [0x01FFu16, 0x0200] {
        let mut cpu = Cpu::new();
        cpu.ax = 0x1234;
        cpu.dx = 0x5678;
        cpu.flags = 0x0FD7;
        let original_registers = registers(&cpu);
        let [low, high] = offset.to_le_bytes();
        let mut memory = vec![0xCC; 0x0200];
        memory[..4].copy_from_slice(&[0xF7, 0x26, low, high]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(memory, original_memory);
    }
}
