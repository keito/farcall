use super::{Cpu, CpuModel, CpuStepResult};

// Intel PUSHA/POPA: save AX,CX,DX,BX,original SP,BP,SI,DI; restore in
// reverse order, skipping the saved-SP slot. Neither instruction changes FLAGS.
// https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2b-manual.pdf
// 8088 60/61 are JO/JNO aliases, established by microcode analysis:
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn seed_registers(cpu: &mut Cpu) {
    [cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.bp, cpu.si, cpu.di] =
        [0x1234, 0x2345, 0x3456, 0x4567, 0x5678, 0x6789, 0x789A];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
    cpu.ip = 0x0100;
    cpu.sp = 0x1000;
}

fn setup_286(instruction: &[u8], sp: u16, flags: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(CpuModel::I80286, 0);
    seed_registers(&mut cpu);
    cpu.sp = sp;
    cpu.flags = flags;
    let mut memory = vec![0xCC; 0x50000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn i286_pusha_writes_exact_frame_with_original_sp_and_preserves_flags() {
    // These stacks stay within the segment, including a final SP of zero.
    for (sp, final_sp, address) in [
        (0x1000u16, 0x0FF0, 0x30FF0),
        (0x0010, 0x0000, 0x30000),
        (0xFFFE, 0xFFEE, 0x3FFEE),
    ] {
        for instruction in [&[0x60][..], &[0x26, 0x60], &[0x2E, 0x60], &[0x3E, 0x60]] {
            for flags in [0x0002, 0x0FD7] {
                let (mut cpu, mut memory) = setup_286(instruction, sp, flags);
                let mut expected_registers = registers(&cpu);
                expected_registers[4] = final_sp;
                let [sp_low, sp_high] = sp.to_le_bytes();
                let mut expected_memory = memory.clone();
                // Ascending addresses from final SS:SP: DI,SI,BP,SP,BX,DX,CX,AX.
                expected_memory[address..address + 16].copy_from_slice(&[
                    0x9A, 0x78, 0x89, 0x67, 0x78, 0x56, sp_low, sp_high, 0x67, 0x45, 0x56, 0x34,
                    0x45, 0x23, 0x34, 0x12,
                ]);

                step(&mut cpu, &mut memory);

                assert_eq!(registers(&cpu), expected_registers);
                assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
                assert_eq!(cpu.flags, flags, "PUSHA preserves every flag");
                assert!(
                    memory == expected_memory,
                    "PUSHA writes only its eight stack words"
                );
            }
        }
    }
}

#[test]
fn i286_popa_restores_prebuilt_frame_and_ignores_saved_sp() {
    for (sp, final_sp, address) in [
        (0x1000, 0x1010, 0x31000),
        (0x0000, 0x0010, 0x30000),
        (0xFFF0, 0x0000, 0x3FFF0),
    ] {
        for instruction in [&[0x61][..], &[0x26, 0x61], &[0x2E, 0x61], &[0x3E, 0x61]] {
            for saved_sp in [0x0000u16, 0x0001, 0xFFFF, 0xDEAD] {
                for flags in [0x0002, 0x0FD7] {
                    let (mut cpu, mut memory) = setup_286(instruction, sp, flags);
                    let [sp_low, sp_high] = saved_sp.to_le_bytes();
                    memory[address..address + 16].copy_from_slice(&[
                        0xB2, 0xA1, // DI=A1B2h.
                        0xD4, 0xC3, // SI=C3D4h.
                        0xF6, 0xE5, // BP=E5F6h.
                        sp_low, sp_high, // Deliberately unrelated to the real SP.
                        0x34, 0x12, // BX=1234h.
                        0x78, 0x56, // DX=5678h.
                        0xBC, 0x9A, // CX=9ABCh.
                        0xF0, 0xDE, // AX=DEF0h.
                    ]);
                    let original_memory = memory.clone();

                    step(&mut cpu, &mut memory);

                    assert_eq!(
                        registers(&cpu),
                        [
                            0xDEF0, 0x9ABC, 0x5678, 0x1234, final_sp, 0xE5F6, 0xC3D4, 0xA1B2,
                            0x1000, 0x2000, 0x4000, 0x3000,
                        ],
                        "saved SP slot={saved_sp:04X}"
                    );
                    assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
                    assert_eq!(cpu.flags, flags, "POPA preserves every flag");
                    assert!(memory == original_memory, "POPA leaves its frame intact");
                }
            }
        }
    }
}

#[test]
fn i286_pusha_popa_round_trip_restores_guest_registers_with_poisoned_sp_slot() {
    let program = [
        0x60, // PUSHA.
        0xB8, 0x00, 0xA0, // MOV AX,A000h.
        0xB9, 0x01, 0xA1, // MOV CX,A101h.
        0xBA, 0x02, 0xA2, // MOV DX,A202h.
        0xBB, 0x03, 0xA3, // MOV BX,A303h.
        0xBD, 0x05, 0xA5, // MOV BP,A505h.
        0xBE, 0x06, 0xA6, // MOV SI,A606h.
        0xBF, 0x07, 0xA7, // MOV DI,A707h.
        0x61, // POPA.
    ];
    for flags in [0x0002, 0x0FD7] {
        let (mut cpu, mut memory) = setup_286(&program, 0x1000, flags);
        let original_registers = registers(&cpu);

        step(&mut cpu, &mut memory);
        assert_eq!(cpu.sp, 0x0FF0);
        // POPA must ignore this slot even when it no longer contains saved SP.
        memory[0x30FF6..0x30FF8].copy_from_slice(&[0xAD, 0xDE]);
        let expected_memory = memory.clone();
        // Seven MOV instructions followed by POPA, bounded independently of IP.
        for _ in 0..8 {
            step(&mut cpu, &mut memory);
        }

        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.ip, 0x0117);
        assert_eq!(cpu.flags, flags);
        assert!(memory == expected_memory);
    }
}

#[test]
fn i286_pusha_is_one_byte_and_does_not_fetch_a_displacement() {
    check_one_byte(0x60, 0x0FF0);
}

#[test]
fn i286_popa_is_one_byte_and_does_not_fetch_a_displacement() {
    check_one_byte(0x61, 0x1010);
}

fn check_one_byte(opcode: u8, final_sp: u16) {
    let mut cpu = Cpu::new(CpuModel::I80286, 0);
    seed_registers(&mut cpu);
    cpu.cs = 0;
    cpu.ss = 0;
    cpu.ip = 0x3FFF;
    let flags = cpu.flags;
    let mut memory = vec![0xCC; 0x4000];
    // Reading a jump displacement after this byte would be out of bounds.
    memory[0x3FFF] = opcode;

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0, 0x4000, final_sp));
    assert_eq!(cpu.flags, flags);
}

#[test]
fn i8088_60_alias_is_jo() {
    check_jump(false, 0x60);
}

#[test]
fn i8088_61_alias_is_jno() {
    check_jump(false, 0x61);
}

#[test]
fn i8088_70_and_71_remain_conditional_jumps() {
    check_jump(false, 0x70);
    check_jump(false, 0x71);
}

#[test]
fn i286_70_and_71_remain_conditional_jumps() {
    check_jump(true, 0x70);
    check_jump(true, 0x71);
}

fn check_jump(is_286: bool, opcode: u8) {
    // (IP, displacement byte, taken IP, untaken IP). Extra 8088 cases wrap IP.
    let cases = [
        (0x0100u16, 0x00, 0x0102, 0x0102),
        (0x0100, 0x05, 0x0107, 0x0102),
        (0x0100, 0x7F, 0x0181, 0x0102),
        (0x0100, 0xFE, 0x0100, 0x0102),
        (0x0100, 0x80, 0x0082, 0x0102),
        (0xFFFD, 0x01, 0x0000, 0xFFFF),
        (0xFFFE, 0xFE, 0xFFFE, 0x0000),
        (0xFFFF, 0x02, 0x0003, 0x0001),
        (0x0000, 0x80, 0xFF82, 0x0002),
    ];
    let cases = if is_286 { &cases[..5] } else { &cases[..] };
    for &(ip, displacement, taken_ip, untaken_ip) in cases {
        // Vary all other flags independently of OF to detect a wrong condition.
        for (low_flags, jo_taken, jno_taken) in [
            (0x0002, false, true),
            (0x07D7, false, true),
            (0x0802, true, false),
            (0x0FD7, true, false),
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
            let original_flags = cpu.flags;
            let original_registers = registers(&cpu);
            let mut memory = vec![0xCC; 0x50000];
            memory[0x10000 + usize::from(ip)] = opcode;
            memory[0x10000 + usize::from(ip.wrapping_add(1))] = displacement;
            let original_memory = memory.clone();
            let taken = if opcode == 0x60 || opcode == 0x70 {
                jo_taken
            } else {
                jno_taken
            };

            step(&mut cpu, &mut memory);

            assert_eq!(
                cpu.ip,
                if taken { taken_ip } else { untaken_ip },
                "opcode={opcode:02X}, OF={}, displacement={displacement:02X}",
                (low_flags & 0x0800) != 0
            );
            assert_eq!(
                registers(&cpu),
                original_registers,
                "jump must not push/pop registers"
            );
            assert_eq!(
                cpu.flags, original_flags,
                "conditional jumps preserve FLAGS"
            );
            assert!(
                memory == original_memory,
                "conditional jumps leave the stack intact"
            );
        }
    }
}
