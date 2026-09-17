use super::{Cpu, CpuStepResult};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

#[test]
fn add_83_sign_extends_the_byte_and_updates_the_full_word_register_and_flags() {
    // Operand, encoded immediate, result, literal CF/PF/AF/ZF/SF/OF bits.
    let cases = [
        (0x0000, 0x00, 0x0000, 0x0044),
        (0x0000, 0x7F, 0x007F, 0x0000),
        (0x0000, 0x80, 0xFF80, 0x0080),
        (0x0100, 0xFF, 0x00FF, 0x0005),
        (0x00FF, 0x01, 0x0100, 0x0014),
        (0x7FFF, 0x01, 0x8000, 0x0894),
        (0x8000, 0xFF, 0x7FFF, 0x0805),
        (0xFFFF, 0x01, 0x0000, 0x0055),
        (0x0080, 0x80, 0x0000, 0x0045),
    ];
    for reg in 0..8 {
        for (operand, immediate, result, arithmetic_flags) in cases {
            for flags in [0, 0x0FD7] {
                let mut cpu = Cpu::new();
                let mut values = [
                    0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A, 0x89AB,
                ];
                values[reg] = operand;
                [
                    cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
                ] = values;
                [cpu.es, cpu.ss, cpu.ds] = [0x1000, 0x2000, 0x3000];
                cpu.flags = flags;
                let mut expected = registers(&cpu);
                expected[reg] = result;
                let instruction = [0x83, 0xC0 | reg as u8, immediate];
                let mut memory = instruction;

                assert!(matches!(
                    cpu.step(&mut memory).unwrap(),
                    CpuStepResult::Continue
                ));

                assert_eq!(
                    registers(&cpu),
                    expected,
                    "register={reg}, operand={operand:04X}, immediate={immediate:02X}"
                );
                assert_eq!(cpu.flags, (flags & !0x08D5) | arithmetic_flags);
                assert_eq!(cpu.ip, 3, "consume only one immediate byte");
                assert_eq!(memory, instruction);
            }
        }
    }
}

#[test]
fn add_83_updates_both_memory_bytes_and_fetches_the_immediate_after_displacement() {
    for (instruction, address, operand, result, arithmetic_flags) in [
        // ADD word [DS:2000h], 1: carry must reach the high byte.
        (
            &[0x83, 0x06, 0x00, 0x20, 0x01][..],
            0x12000,
            0x00FFu16,
            0x0100u16,
            0x0014,
        ),
        // ADD word [SS:BP-2], -1: BP=2002h and both bytes are signed independently.
        (
            &[0x83, 0x46, 0xFE, 0xFF][..],
            0x22000,
            0x0100,
            0x00FF,
            0x0005,
        ),
        // The same BP address with an ES override.
        (
            &[0x26, 0x83, 0x46, 0xFE, 0xFF][..],
            0x32000,
            0x0100,
            0x00FF,
            0x0005,
        ),
    ] {
        for flags in [0, 0x0FD7] {
            let mut cpu = Cpu::new();
            cpu.ax = 0xA55A;
            cpu.bp = 0x2002;
            [cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000];
            cpu.flags = flags;
            let expected_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x40000];
            memory[..instruction.len()].copy_from_slice(instruction);
            memory[address..address + 2].copy_from_slice(&operand.to_le_bytes());
            let mut expected_memory = memory.clone();
            expected_memory[address..address + 2].copy_from_slice(&result.to_le_bytes());

            assert!(matches!(
                cpu.step(&mut memory).unwrap(),
                CpuStepResult::Continue
            ));

            assert_eq!(registers(&cpu), expected_registers);
            assert_eq!(cpu.flags, (flags & !0x08D5) | arithmetic_flags);
            assert_eq!(cpu.ip, instruction.len() as u16);
            assert!(
                memory == expected_memory,
                "only the destination word should change"
            );
        }
    }
}
