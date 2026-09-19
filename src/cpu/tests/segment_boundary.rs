use super::{Cpu, CpuStepResult};

// 8088 word accesses wrap the OFFSET, independently of the 20-bit bus wrap.
// Hardware traces: https://martypc.blogspot.com/2025/06/exploring-16-bit-bus-access-on-pcat.html
// Keep expected physical addresses literal: do not use the memory helpers under
// test to construct inputs or expectations. CC at the unwrapped address is a
// deliberately different high byte. Each test executes a bounded single step.
fn setup(instruction: &[u8]) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(0);
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x6000];
    cpu.ip = 0x0100;
    cpu.sp = 0x0800;
    cpu.si = 0xFFFF;
    cpu.di = 0xFFFF;
    cpu.bp = 0xFFFF;
    cpu.ax = 0x1234;
    cpu.flags = 0xF603;
    let mut memory = vec![0xCC; 0x100000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn assert_memory(actual: &[u8], expected: &[u8]) {
    if let Some(address) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!(
            "memory at {address:05X}: got {:02X}, expected {:02X}",
            actual[address], expected[address]
        );
    }
}

macro_rules! word_load {
    ($name:ident, $bytes:expr, $low:expr, $high:expr, $register:ident) => {
        #[test]
        fn $name() {
            let (mut cpu, mut memory) = setup(&$bytes);
            memory[$low] = 0x78;
            memory[$high] = 0x56;
            let expected = memory.clone();
            step(&mut cpu, &mut memory);
            assert_eq!(cpu.$register, 0x5678);
            assert_eq!(cpu.flags, 0xF603);
            assert_eq!(cpu.ip, 0x0100 + $bytes.len() as u16);
            assert_memory(&memory, &expected);
        }
    };
}

word_load!(mov_8b_ds, [0x8B, 0x04], 0x2FFFF, 0x20000, ax);
word_load!(mov_8b_bp_uses_ss, [0x8B, 0x46, 0x00], 0x6FFFF, 0x60000, ax);
word_load!(mov_8b_es_override, [0x26, 0x8B, 0x04], 0x4FFFF, 0x40000, ax);
word_load!(mov_a1, [0xA1, 0xFF, 0xFF], 0x2FFFF, 0x20000, ax);
word_load!(
    mov_a1_es_override,
    [0x26, 0xA1, 0xFF, 0xFF],
    0x4FFFF,
    0x40000,
    ax
);
word_load!(
    mov_8e_load_ds_uses_old_ds_for_both_bytes,
    [0x8E, 0x1C],
    0x2FFFF,
    0x20000,
    ds
);

macro_rules! word_store {
    ($name:ident, $bytes:expr, $low:expr, $high:expr, $value:expr) => {
        #[test]
        fn $name() {
            let (mut cpu, mut memory) = setup(&$bytes);
            let mut expected = memory.clone();
            let [low, high] = ($value as u16).to_le_bytes();
            expected[$low] = low;
            expected[$high] = high;
            step(&mut cpu, &mut memory);
            assert_eq!(cpu.ax, 0x1234);
            assert_eq!(cpu.flags, 0xF603);
            assert_eq!(cpu.ip, 0x0100 + $bytes.len() as u16);
            assert_memory(&memory, &expected);
        }
    };
}

word_store!(mov_89_ds, [0x89, 0x04], 0x2FFFF, 0x20000, 0x1234);
word_store!(
    mov_89_bp_uses_ss,
    [0x89, 0x46, 0x00],
    0x6FFFF,
    0x60000,
    0x1234
);
word_store!(
    mov_89_es_override,
    [0x26, 0x89, 0x04],
    0x4FFFF,
    0x40000,
    0x1234
);
word_store!(mov_a3, [0xA3, 0xFF, 0xFF], 0x2FFFF, 0x20000, 0x1234);
word_store!(
    mov_a3_es_override,
    [0x26, 0xA3, 0xFF, 0xFF],
    0x4FFFF,
    0x40000,
    0x1234
);
word_store!(mov_c7, [0xC7, 0x04, 0x78, 0x56], 0x2FFFF, 0x20000, 0x5678);
word_store!(mov_8c_store_ss, [0x8C, 0x14], 0x2FFFF, 0x20000, 0x6000);

#[test]
fn xor_81_6_reproduces_single_step_tests_case_3931() {
    // 81.6.json.gz, hash 7b308c3f0652c9ab6fe43173df1b023c680790c7.
    // XOR word [ES:SI-3D60h],7009h: 84C5 XOR 7009 = F4CC.
    let (mut cpu, mut memory) = setup(&[0x26, 0x81, 0xB4, 0xA0, 0xC2, 0x09, 0x70]);
    cpu.es = 0x8BB0;
    cpu.si = 0x3D5F;
    memory[0x9BAFF] = 0xC5;
    memory[0x8BB00] = 0x84;
    memory[0x9BB00] = 0;
    let mut expected = memory.clone();
    expected[0x9BAFF] = 0xCC;
    expected[0x8BB00] = 0xF4;
    step(&mut cpu, &mut memory);
    assert_memory(&memory, &expected);
    assert_eq!(cpu.flags & !0x0010, 0xF686, "AF is undefined for XOR");
    assert_eq!((cpu.es, cpu.si, cpu.ip), (0x8BB0, 0x3D5F, 0x0107));
}

macro_rules! group1 {
    ($name:ident, $extension:expr, $result:expr, $flags:expr, $ignored:expr) => {
        #[test]
        fn $name() {
            for opcode in [0x81, 0x83] {
                let mut bytes = vec![opcode, ($extension << 3) | 4, 1];
                if opcode == 0x81 {
                    bytes.push(0);
                }
                let (mut cpu, mut memory) = setup(&bytes);
                memory[0x2FFFF] = 0x34;
                memory[0x20000] = 0x12;
                let mut expected = memory.clone();
                if $extension != 7 {
                    let [low, high] = ($result as u16).to_le_bytes();
                    expected[0x2FFFF] = low;
                    expected[0x20000] = high;
                }
                step(&mut cpu, &mut memory);
                assert_memory(&memory, &expected);
                assert_eq!(
                    cpu.flags & !$ignored,
                    0xF602 | $flags,
                    "opcode={opcode:02X}"
                );
                assert_eq!(cpu.ip, 0x0100 + bytes.len() as u16);
            }
        }
    };
}

// Operand 1234h, immediate 1, initial carry set. Literal result/flag oracles.
group1!(add_81_83, 0, 0x1235, 0x0004, 0);
group1!(or_81_83, 1, 0x1235, 0x0004, 0x0010);
group1!(adc_81_83, 2, 0x1236, 0x0004, 0);
group1!(sbb_81_83, 3, 0x1232, 0x0000, 0);
group1!(and_81_83, 4, 0x0000, 0x0044, 0x0010);
group1!(sub_81_83, 5, 0x1233, 0x0004, 0);
group1!(xor_81_83, 6, 0x1235, 0x0004, 0x0010);
group1!(cmp_81_83, 7, 0x1233, 0x0004, 0);

fn check_string(opcode: u8) {
    for prefix in [None, Some(0xF2), Some(0xF3)] {
        for backwards in [false, true] {
            // Exercise a wrapped source, destination, and both independently.
            for (si, source, di, dest) in [
                (0xFFFF, [0x2FFFF, 0x20000], 0x0200, [0x40200, 0x40201]),
                (0x0200, [0x20200, 0x20201], 0xFFFF, [0x4FFFF, 0x40000]),
                (0xFFFF, [0x2FFFF, 0x20000], 0xFFFF, [0x4FFFF, 0x40000]),
            ] {
                let mut bytes = Vec::new();
                bytes.extend(prefix);
                bytes.push(opcode);
                let (mut cpu, mut memory) = setup(&bytes);
                cpu.si = si;
                cpu.di = di;
                cpu.cx = 1; // REP executes exactly one iteration, in either direction.
                cpu.flags = if backwards { 0xF603 } else { 0xF203 };
                let flags = cpu.flags;
                memory[source[0]] = 0x34;
                memory[source[1]] = 0x12;
                memory[dest[0]] = 0x34;
                memory[dest[1]] = 0x12;
                // Different wrong high bytes catch even CMPSW with both operands wrapped.
                memory[source[0] + 1] = 0x77;
                memory[dest[0] + 1] = 0x88;
                // Restore high bytes of non-boundary control operands.
                memory[source[1]] = 0x12;
                memory[dest[1]] = 0x12;
                if matches!(opcode, 0xA5 | 0xAB) {
                    memory[dest[0]] = 0xEE;
                    memory[dest[1]] = 0xEE;
                }
                let mut expected = memory.clone();
                if matches!(opcode, 0xA5 | 0xAB) {
                    expected[dest[0]] = 0x34;
                    expected[dest[1]] = 0x12;
                }
                step(&mut cpu, &mut memory);
                assert_memory(&memory, &expected);
                assert_eq!(cpu.ax, 0x1234);
                let advance = |index: u16| {
                    if backwards {
                        index.wrapping_sub(2)
                    } else {
                        index.wrapping_add(2)
                    }
                };
                assert_eq!(
                    cpu.si,
                    if matches!(opcode, 0xA5 | 0xA7 | 0xAD) {
                        advance(si)
                    } else {
                        si
                    }
                );
                assert_eq!(
                    cpu.di,
                    if matches!(opcode, 0xA5 | 0xA7 | 0xAB | 0xAF) {
                        advance(di)
                    } else {
                        di
                    }
                );
                assert_eq!(cpu.cx, if prefix.is_some() { 0 } else { 1 });
                assert_eq!(cpu.ip, 0x0100 + bytes.len() as u16);
                assert_eq!(
                    cpu.flags,
                    if matches!(opcode, 0xA7 | 0xAF) {
                        (flags & !0x08D5) | 0x0044
                    } else {
                        flags
                    }
                );
            }
        }
    }
}

#[test]
fn movsw_a5() {
    check_string(0xA5);
}
#[test]
fn cmpsw_a7() {
    check_string(0xA7);
}
#[test]
fn stosw_ab() {
    check_string(0xAB);
}
#[test]
fn lodsw_ad() {
    check_string(0xAD);
}
#[test]
fn scasw_af() {
    check_string(0xAF);
}

#[test]
fn push_register_segment_flags_and_memory_wrap_the_stack_word() {
    for (bytes, value) in [
        (&[0x50][..], 0x1234u16),    // PUSH AX
        (&[0x1E][..], 0x2000),       // PUSH DS
        (&[0x9C][..], 0xF603),       // PUSHF
        (&[0xFF, 0x34][..], 0x5678), // PUSH word [SI], source also wraps
    ] {
        let (mut cpu, mut memory) = setup(bytes);
        cpu.sp = 1;
        memory[0x2FFFF] = 0x78;
        memory[0x20000] = 0x56;
        let mut expected = memory.clone();
        let [low, high] = value.to_le_bytes();
        expected[0x6FFFF] = low;
        expected[0x60000] = high;
        step(&mut cpu, &mut memory);
        assert_memory(&memory, &expected);
        assert_eq!(cpu.sp, 0xFFFF);
        assert_eq!(cpu.flags, 0xF603);
        assert_eq!(cpu.ip, 0x0100 + bytes.len() as u16);
    }
}

#[test]
fn pop_register_segment_flags_and_memory_wrap_the_stack_word() {
    for bytes in [&[0x58][..], &[0x1F], &[0x9D], &[0x8F, 0x04]] {
        let (mut cpu, mut memory) = setup(bytes);
        cpu.sp = 0xFFFF;
        memory[0x6FFFF] = 0xD7;
        memory[0x60000] = 0xFB;
        let mut expected = memory.clone();
        if bytes[0] == 0x8F {
            expected[0x2FFFF] = 0xD7;
            expected[0x20000] = 0xFB;
        }
        step(&mut cpu, &mut memory);
        assert_eq!(cpu.sp, 1);
        assert_eq!(cpu.ax, if bytes[0] == 0x58 { 0xFBD7 } else { 0x1234 });
        assert_eq!(cpu.ds, if bytes[0] == 0x1F { 0xFBD7 } else { 0x2000 });
        assert_eq!(cpu.flags, if bytes[0] == 0x9D { 0xFBD7 } else { 0xF603 });
        assert_memory(&memory, &expected);
    }
}

#[test]
fn near_call_e8_and_ff_2_wrap_the_return_address_write() {
    for bytes in [&[0xE8, 0x31, 0x11][..], &[0xFF, 0x14]] {
        let (mut cpu, mut memory) = setup(bytes);
        cpu.sp = 1;
        memory[0x2FFFF] = 0x34;
        memory[0x20000] = 0x12;
        let mut expected = memory.clone();
        expected[0x6FFFF] = bytes.len() as u8;
        expected[0x60000] = 0x01;
        step(&mut cpu, &mut memory);
        assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x1000, 0x1234, 0xFFFF));
        assert_eq!(cpu.flags, 0xF603);
        assert_memory(&memory, &expected);
    }
}

#[test]
fn near_jump_ff_4_wraps_the_target_read() {
    let (mut cpu, mut memory) = setup(&[0xFF, 0x24]);
    memory[0x2FFFF] = 0x34;
    memory[0x20000] = 0x12;
    let expected = memory.clone();
    step(&mut cpu, &mut memory);
    assert_eq!((cpu.cs, cpu.ip, cpu.sp), (0x1000, 0x1234, 0x0800));
    assert_eq!(cpu.flags, 0xF603);
    assert_memory(&memory, &expected);
}

#[test]
fn ret_c3_retf_ca_cb_and_iret_cf_wrap_each_stack_word() {
    for bytes in [&[0xC3][..], &[0xCB], &[0xCA, 0x08, 0], &[0xCF]] {
        // Put each popped word in turn at FFFF, not just the first word.
        let words = match bytes[0] {
            0xC3 => 1,
            0xCF => 3,
            _ => 2,
        };
        for boundary_word in 0..words {
            let (mut cpu, mut memory) = setup(bytes);
            cpu.sp = 0xFFFF - 2 * boundary_word as u16;
            let start_sp = cpu.sp;
            let frame = [0x34, 0x12, 0x78, 0x56, 0xD7, 0xFB];
            // Literal physical layouts for frames starting FFFF, FFFD, FFFB.
            let addresses = match boundary_word {
                0 => [0x6FFFF, 0x60000, 0x60001, 0x60002, 0x60003, 0x60004],
                1 => [0x6FFFD, 0x6FFFE, 0x6FFFF, 0x60000, 0x60001, 0x60002],
                _ => [0x6FFFB, 0x6FFFC, 0x6FFFD, 0x6FFFE, 0x6FFFF, 0x60000],
            };
            for i in 0..words * 2 {
                memory[addresses[i]] = frame[i];
            }
            let expected = memory.clone();
            step(&mut cpu, &mut memory);
            assert_eq!(cpu.ip, 0x1234);
            assert_eq!(cpu.cs, if words > 1 { 0x5678 } else { 0x1000 });
            assert_eq!(cpu.flags, if words == 3 { 0xFBD7 } else { 0xF603 });
            assert_eq!(
                cpu.sp,
                start_sp
                    .wrapping_add(2 * words as u16)
                    .wrapping_add(if bytes[0] == 0xCA { 8 } else { 0 })
            );
            assert_memory(&memory, &expected);
        }
    }
}

fn check_far_pointer(opcode: u8, modrm: u8) {
    // FFFD splits the segment word, FFFE wraps between words, FFFF splits offset.
    for (offset, addresses) in [
        (0xFFFDu16, [0x2FFFD, 0x2FFFE, 0x2FFFF, 0x20000]),
        (0xFFFE, [0x2FFFE, 0x2FFFF, 0x20000, 0x20001]),
        (0xFFFF, [0x2FFFF, 0x20000, 0x20001, 0x20002]),
    ] {
        let (mut cpu, mut memory) = setup(&[opcode, modrm]);
        cpu.si = offset;
        for (address, byte) in addresses.into_iter().zip([0x34, 0x12, 0x78, 0x56]) {
            memory[address] = byte;
        }
        let mut expected = memory.clone();
        if opcode == 0xFF && modrm == 0x1C {
            expected[0x607FC..0x60800].copy_from_slice(&[0x02, 0x01, 0x00, 0x10]);
        }
        step(&mut cpu, &mut memory);
        if opcode == 0xFF {
            assert_eq!((cpu.cs, cpu.ip), (0x5678, 0x1234));
            assert_eq!(cpu.sp, if modrm == 0x1C { 0x07FC } else { 0x0800 });
        } else {
            assert_eq!(cpu.ax, 0x1234);
            assert_eq!(cpu.ds, if opcode == 0xC5 { 0x5678 } else { 0x2000 });
            assert_eq!(cpu.es, if opcode == 0xC4 { 0x5678 } else { 0x4000 });
            assert_eq!(cpu.ip, 0x0102);
        }
        assert_eq!(cpu.flags, 0xF603);
        assert_memory(&memory, &expected);
    }
}

#[test]
fn les_c4() {
    check_far_pointer(0xC4, 0x04);
}
#[test]
fn lds_c5() {
    check_far_pointer(0xC5, 0x04);
}
#[test]
fn far_call_ff_3() {
    check_far_pointer(0xFF, 0x1C);
}
#[test]
fn far_jump_ff_5() {
    check_far_pointer(0xFF, 0x2C);
}

#[test]
fn int_cd_entry_wraps_each_saved_stack_word() {
    for (sp, addresses) in [
        (1, [0x6FFFB, 0x6FFFC, 0x6FFFD, 0x6FFFE, 0x6FFFF, 0x60000]),
        (3, [0x6FFFD, 0x6FFFE, 0x6FFFF, 0x60000, 0x60001, 0x60002]),
        (5, [0x6FFFF, 0x60000, 0x60001, 0x60002, 0x60003, 0x60004]),
    ] {
        let (mut cpu, mut memory) = setup(&[0xCD, 0x60]);
        cpu.sp = sp;
        memory[0x180..0x184].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
        let mut expected = memory.clone();
        for (address, byte) in addresses
            .into_iter()
            .zip([0x02, 0x01, 0x00, 0x10, 0x03, 0xF6])
        {
            expected[address] = byte;
        }
        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Interrupt(0x60)
        ));
        cpu.enter_interrupt(&mut memory, 0x60).unwrap();
        assert_eq!(
            (cpu.cs, cpu.ip, cpu.sp, cpu.flags),
            (0x5678, 0x1234, sp.wrapping_sub(6), 0xF403)
        );
        assert_memory(&memory, &expected);
    }
}

#[test]
fn a1_and_a3_distinguish_offset_wrap_from_physical_bus_wrap() {
    // Includes a non-boundary control and cases where translation itself wraps.
    for (segment, offset, low, high) in [
        (0x2000, 0xFFFEu16, 0x2FFFE, 0x2FFFF),
        (0x2000, 0xFFFF, 0x2FFFF, 0x20000),
        (0xFFFF, 0x000F, 0xFFFFF, 0x00000),
        (0xFFFF, 0xFFFF, 0x0FFEF, 0xFFFF0),
    ] {
        for opcode in [0xA1, 0xA3] {
            let [lo, hi] = offset.to_le_bytes();
            let (mut cpu, mut memory) = setup(&[opcode, lo, hi]);
            cpu.ds = segment;
            memory[low] = 0x78;
            memory[high] = 0x56;
            let mut expected = memory.clone();
            if opcode == 0xA3 {
                expected[low] = 0x34;
                expected[high] = 0x12;
            }
            step(&mut cpu, &mut memory);
            assert_eq!(cpu.ax, if opcode == 0xA1 { 0x5678 } else { 0x1234 });
            assert_eq!(cpu.flags, 0xF603);
            assert_memory(&memory, &expected);
        }
    }
}
