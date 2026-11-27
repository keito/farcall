use super::{Cpu, CpuModel, CpuStepResult};

fn address_test_cpu() -> Cpu {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ax = 0xA65B;
    cpu.cx = 0xC27D;
    cpu.dx = 0xD38E;
    cpu.bx = 0x1000;
    cpu.bp = 0x2000;
    cpu.si = 0x0030;
    cpu.di = 0x0040;
    cpu.sp = 0x9000;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD7;
    cpu
}

// AX, CX, DX, BX, SP, BP, SI, DI, ES, CS, SS, DS.
fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn assert_memory_form(mod_rm: u8, displacement: &[u8], address: u16, word: bool) {
    // Exercise reads and writes through the public instruction behavior, so
    // these tests survive moving the address calculation into a shared helper.
    // All segments are zero: segment selection is a later milestone.
    let opcodes: &[u8] = if word {
        &[0x8E, 0x8C, 0x8D]
    } else {
        &[0x8A, 0x88]
    };
    for &opcode in opcodes {
        let mut cpu = address_test_cpu();
        cpu.es = if opcode == 0x8C { 0xBEEF } else { 0 };
        let mut expected_registers = registers(&cpu);
        let mut memory = vec![0xCC; 0x10000];
        memory[0x0100] = opcode;
        memory[0x0101] = mod_rm; // reg=000: AL, AX, or ES depending on opcode.
        memory[0x0102..0x0102 + displacement.len()].copy_from_slice(displacement);
        memory[address as usize..address as usize + 2].copy_from_slice(&[0x73, 0xE2]);
        let mut expected_memory = memory.clone();
        match opcode {
            0x8A => expected_registers[0] = 0xA673, // MOV AL, [address].
            0x88 => expected_memory[address as usize] = 0x5B, // MOV [address], AL.
            0x8E => expected_registers[8] = 0xE273, // MOV ES, [address].
            0x8C => expected_memory[address as usize..address as usize + 2]
                .copy_from_slice(&[0xEF, 0xBE]), // MOV [address], ES.
            0x8D => expected_registers[0] = address, // LEA AX, [address].
            _ => unreachable!(),
        }

        let result = cpu
            .step(&mut memory)
            .unwrap_or_else(|error| panic!("opcode={opcode:02X}, ModR/M={mod_rm:02X}: {error}"));

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(registers(&cpu), expected_registers, "opcode={opcode:02X}");
        assert_eq!(cpu.ip, 0x0102 + displacement.len() as u16);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(
            memory, expected_memory,
            "only the destination may be written"
        );
    }
}

// Separate tests keep one unsupported mode from hiding the results for others.
// Expected addresses are literal examples, not computed using the decoder.
macro_rules! memory_case {
    ($name:ident, $mod_rm:expr, $displacement:expr, $address:expr) => {
        mod $name {
            #[test]
            fn byte() {
                super::assert_memory_form($mod_rm, $displacement, $address, false);
            }

            #[test]
            fn word_and_lea() {
                super::assert_memory_form($mod_rm, $displacement, $address, true);
            }
        }
    };
}

// mod=00: no displacement, except r/m=110 means direct [disp16], not [BP].
memory_case!(no_disp_bx_si, 0x00, &[], 0x1030);
memory_case!(no_disp_bx_di, 0x01, &[], 0x1040);
memory_case!(no_disp_bp_si, 0x02, &[], 0x2030);
memory_case!(no_disp_bp_di, 0x03, &[], 0x2040);
memory_case!(no_disp_si, 0x04, &[], 0x0030);
memory_case!(no_disp_di, 0x05, &[], 0x0040);
memory_case!(direct_disp16, 0x06, &[0x56, 0x34], 0x3456);
memory_case!(no_disp_bx, 0x07, &[], 0x1000);

// mod=01: FE is -2, including for the real [BP + disp8] encoding.
memory_case!(disp8_bx_si, 0x40, &[0xFE], 0x102E);
memory_case!(disp8_bx_di, 0x41, &[0xFE], 0x103E);
memory_case!(disp8_bp_si, 0x42, &[0xFE], 0x202E);
memory_case!(disp8_bp_di, 0x43, &[0xFE], 0x203E);
memory_case!(disp8_si, 0x44, &[0xFE], 0x002E);
memory_case!(disp8_di, 0x45, &[0xFE], 0x003E);
memory_case!(disp8_bp, 0x46, &[0xFE], 0x1FFE);
memory_case!(disp8_bx, 0x47, &[0xFE], 0x0FFE);

// mod=10: 34 12 is the little-endian displacement 1234h.
memory_case!(disp16_bx_si, 0x80, &[0x34, 0x12], 0x2264);
memory_case!(disp16_bx_di, 0x81, &[0x34, 0x12], 0x2274);
memory_case!(disp16_bp_si, 0x82, &[0x34, 0x12], 0x3264);
memory_case!(disp16_bp_di, 0x83, &[0x34, 0x12], 0x3274);
memory_case!(disp16_si, 0x84, &[0x34, 0x12], 0x1264);
memory_case!(disp16_di, 0x85, &[0x34, 0x12], 0x1274);
memory_case!(disp16_bp, 0x86, &[0x34, 0x12], 0x3234);
memory_case!(disp16_bx, 0x87, &[0x34, 0x12], 0x2234);

#[test]
fn register_mode_uses_byte_or_word_register_encoding() {
    // mod=11: r/m=100, for example, means AH for a byte but SP for a word.
    let byte_values = [0x5B, 0x7D, 0x8E, 0x00, 0xA6, 0xC2, 0xD3, 0x10];
    let word_values = [
        0xA65B, 0xC27D, 0xD38E, 0x1000, 0x9000, 0x2000, 0x0030, 0x0040,
    ];
    for rm in 0..8 {
        for opcode in [0x8A, 0x8E] {
            let mut cpu = address_test_cpu();
            cpu.ip = 0;
            let mut expected = registers(&cpu);
            if opcode == 0x8A {
                expected[0] = 0xA600 | byte_values[rm];
            } else {
                expected[8] = word_values[rm];
            }
            // No displacement or guest data should be read in register mode.
            let mut memory = [opcode, 0xC0 | rm as u8];
            let original_memory = memory;

            let result = cpu.step(&mut memory).expect("register MOV should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(registers(&cpu), expected, "opcode={opcode:02X}, r/m={rm}");
            assert_eq!(cpu.ip, 2);
            assert_eq!(cpu.flags, 0x0FD7);
            assert_eq!(memory, original_memory);
        }
    }
}
