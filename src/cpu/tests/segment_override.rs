use super::{Cpu, CpuError, CpuModel, CpuStepResult, MemoryError};

fn setup(instruction: &[u8]) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.cs = 0x1000;
    cpu.ds = 0x2000;
    cpu.ss = 0x3000;
    cpu.es = 0x4000;
    cpu.ip = 0x0100;
    cpu.ax = 0xA55A;
    cpu.bx = 0x0200;
    cpu.bp = 0x0300;
    cpu.si = 0x0040;
    cpu.di = 0x0080;
    cpu.sp = 0x0800;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x50000];
    memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn segments(cpu: &Cpu) -> [u16; 4] {
    [cpu.cs, cpu.ds, cpu.ss, cpu.es]
}

#[test]
fn a0_reads_cs_preserves_ah_and_flags_and_expires_after_one_instruction() {
    let (mut cpu, mut memory) = setup(&[0x2E, 0xA0, 0x00, 0x02, 0xA0, 0x00, 0x02]);
    memory[0x10200] = 0x73;
    memory[0x20200] = 0x19;
    let original = memory.clone();

    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ax, 0xA573);
    assert_eq!(cpu.ip, 0x0104, "prefix and opcode execute in one step");
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ax, 0xA519, "the next instruction uses DS again");
    assert_eq!(cpu.ip, 0x0107);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(segments(&cpu), [0x1000, 0x2000, 0x3000, 0x4000]);
    assert!(memory == original);
}

#[test]
fn a2_and_a3_write_only_the_cs_destination_with_the_correct_width() {
    for opcode in [0xA2, 0xA3] {
        let (mut cpu, mut memory) = setup(&[0x2E, opcode, 0x00, 0x02, opcode, 0x00, 0x02]);
        let mut expected = memory.clone();
        expected[0x10200] = 0x5A;
        if opcode == 0xA3 {
            expected[0x10201] = 0xA5;
        }
        step(&mut cpu, &mut memory);
        assert!(memory == expected, "opcode={opcode:02X}");
        assert_eq!(cpu.ip, 0x0104);

        expected[0x20200] = 0x5A;
        if opcode == 0xA3 {
            expected[0x20201] = 0xA5;
        }
        step(&mut cpu, &mut memory);
        assert!(memory == expected, "the next store must use DS");
        assert_eq!(cpu.ip, 0x0107);
        assert_eq!(cpu.ax, 0xA55A);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(segments(&cpu), [0x1000, 0x2000, 0x3000, 0x4000]);
    }
}

#[test]
fn lodsb_overrides_the_source_and_preserves_direction_and_si_wrapping() {
    for (flags, si, next_si, final_si) in [
        (0x0BD7, 0x0200u16, 0x0201, 0x0202),
        (0x0FD7, 0x0200, 0x01FF, 0x01FE),
        (0x0BD7, 0xFFFF, 0x0000, 0x0001),
        (0x0FD7, 0x0000, 0xFFFF, 0xFFFE),
    ] {
        let (mut cpu, mut memory) = setup(&[0x2E, 0xAC, 0xAC]);
        cpu.flags = flags;
        cpu.si = si;
        memory[0x10000 + usize::from(si)] = 0x73;
        memory[0x20000 + usize::from(si)] = 0x19;
        memory[0x20000 + usize::from(next_si)] = 0x42;
        let original = memory.clone();

        step(&mut cpu, &mut memory);
        assert_eq!(cpu.ax, 0xA573);
        assert_eq!(cpu.si, next_si);
        assert_eq!(cpu.ip, 0x0102);
        step(&mut cpu, &mut memory);
        assert_eq!(cpu.ax, 0xA542);
        assert_eq!(cpu.si, final_si);
        assert_eq!(cpu.ip, 0x0103);
        assert_eq!(cpu.di, 0x0080);
        assert_eq!(cpu.flags, flags);
        assert_eq!(segments(&cpu), [0x1000, 0x2000, 0x3000, 0x4000]);
        assert!(memory == original);
    }
}

// ModR/M's reg field is zero in each case. Include every base form,
// the direct-address special case, both displacement widths, and offset wrap.
const ADDRESSES: &[(&[u8], usize)] = &[
    (&[0x00], 0x0240),             // BX+SI
    (&[0x01], 0x0280),             // BX+DI
    (&[0x02], 0x0340),             // BP+SI (normally SS)
    (&[0x03], 0x0380),             // BP+DI (normally SS)
    (&[0x04], 0x0040),             // SI
    (&[0x05], 0x0080),             // DI
    (&[0x06, 0x34, 0x12], 0x1234), // direct disp16
    (&[0x07], 0x0200),             // BX
    (&[0x46, 0xFE], 0x02FE),       // BP-2 (normally SS)
    (&[0x47, 0x7F], 0x027F),       // BX+127
    (&[0x86, 0x34, 0x12], 0x1534), // BP+1234h (normally SS)
    (&[0x84, 0xC0, 0xFF], 0x0000), // SI+FFC0h wraps
];

fn check_mod_rm_mov(opcode: u8) {
    for &(operand, offset) in ADDRESSES {
        let mut instruction = vec![0x2E, opcode];
        instruction.extend_from_slice(operand);
        if opcode == 0xC6 {
            instruction.push(0x73);
        } else if opcode == 0xC7 {
            instruction.extend_from_slice(&[0x73, 0xE2]);
        }
        let (mut cpu, mut memory) = setup(&instruction);
        memory[0x10000 + offset..0x10002 + offset].copy_from_slice(&[0x73, 0xE2]);
        let mut expected = memory.clone();
        let mut expected_ax = cpu.ax;
        let mut expected_es = cpu.es;
        match opcode {
            0x8A => expected_ax = 0xA573, // MOV AL, [CS:...]
            0x8E => expected_es = 0xE273, // MOV ES, [CS:...]
            0x88 => expected[0x10000 + offset] = 0x5A,
            0x89 => expected[0x10000 + offset..0x10002 + offset].copy_from_slice(&[0x5A, 0xA5]),
            0xC6 | 0xC7 => {
                // Seed a different old value so a missing write cannot pass.
                memory[0x10000 + offset..0x10002 + offset].fill(0xCC);
                if opcode == 0xC6 {
                    expected[0x10001 + offset] = 0xCC;
                }
            }
            _ => unreachable!(),
        }

        step(&mut cpu, &mut memory);
        assert_eq!(
            cpu.ax, expected_ax,
            "opcode={opcode:02X}, operand={operand:02X?}"
        );
        assert_eq!(segments(&cpu), [0x1000, 0x2000, 0x3000, expected_es]);
        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(
            memory == expected,
            "opcode={opcode:02X}, operand={operand:02X?}"
        );
    }
}

#[test]
fn mod_rm_byte_reads_override_ds_and_ss() {
    check_mod_rm_mov(0x8A);
}
#[test]
fn mod_rm_word_reads_override_ds_and_ss() {
    check_mod_rm_mov(0x8E);
}
#[test]
fn mod_rm_byte_writes_override_ds_and_ss() {
    check_mod_rm_mov(0x88);
}
#[test]
fn mod_rm_word_writes_override_ds_and_ss() {
    check_mod_rm_mov(0x89);
}
#[test]
fn c6_immediate_stores_override_ds_and_ss() {
    check_mod_rm_mov(0xC6);
}
#[test]
fn c7_immediate_stores_override_ds_and_ss() {
    check_mod_rm_mov(0xC7);
}

#[test]
fn group1_reads_and_writes_cs_and_sets_flags_from_that_operand() {
    // ADD byte [CS:BP-2], 1: FF+1 wraps to zero with CF/PF/AF/ZF set.
    let (mut cpu, mut memory) = setup(&[0x2E, 0x80, 0x46, 0xFE, 0x01]);
    memory[0x102FE] = 0xFF;
    memory[0x302FE] = 0x10;
    let mut expected = memory.clone();
    expected[0x102FE] = 0;
    step(&mut cpu, &mut memory);
    assert!(memory == expected);
    assert_eq!(cpu.flags, 0x0757);
    assert_eq!(cpu.ip, 0x0105);
    assert_eq!(cpu.ax, 0xA55A);
    assert_eq!(segments(&cpu), [0x1000, 0x2000, 0x3000, 0x4000]);
}

#[test]
fn prefix_on_register_only_instruction_does_not_leak_into_next_memory_access() {
    // MOV AL, AH; MOV AL, [0200h].
    let (mut cpu, mut memory) = setup(&[0x2E, 0x8A, 0xC4, 0xA0, 0x00, 0x02]);
    memory[0x10200] = 0x73;
    memory[0x20200] = 0x19;
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ax, 0xA5A5);
    assert_eq!(cpu.ip, 0x0103);
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ax, 0xA519);
    assert_eq!(cpu.ip, 0x0106);
    assert_eq!(cpu.flags, 0x0FD7);
}

#[test]
fn lea_still_returns_only_the_offset_without_reading_memory() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.flags = 0x0FD7;
    cpu.ds = 0x2000;
    let mut memory = [0x2E, 0x8D, 0x06, 0xFF, 0xFF]; // LEA AX, [FFFFh]
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ax, 0xFFFF, "no data exists at this offset");
    assert_eq!(cpu.ip, 5);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(cpu.ds, 0x2000);
    assert_eq!(cpu.cs, 0);
}

#[test]
fn push_and_pop_still_use_ss_and_wrap_sp() {
    let (mut cpu, mut memory) = setup(&[0x2E, 0x50, 0x2E, 0x5B]); // PUSH AX; POP BX
    cpu.sp = 0;
    let mut expected = memory.clone();
    expected[0x3FFFE..0x40000].copy_from_slice(&[0x5A, 0xA5]);
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.sp, 0xFFFE);
    assert_eq!(cpu.ip, 0x0102);
    assert!(memory == expected);
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.bx, 0xA55A);
    assert_eq!(cpu.sp, 0);
    assert_eq!(cpu.ip, 0x0104);
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == expected);
}

#[test]
fn call_and_ret_use_ss_and_return_past_the_entire_prefixed_call() {
    let (mut cpu, mut memory) = setup(&[0x2E, 0xE8, 0x02, 0x00, 0xCC, 0xCC, 0x2E, 0xC3]);
    let mut expected = memory.clone();
    expected[0x307FE..0x30800].copy_from_slice(&[0x04, 0x01]);
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ip, 0x0106);
    assert_eq!(cpu.sp, 0x07FE);
    assert!(memory == expected);
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ip, 0x0104);
    assert_eq!(cpu.sp, 0x0800);
    assert_eq!(cpu.flags, 0x0FD7);
    assert_eq!(segments(&cpu), [0x1000, 0x2000, 0x3000, 0x4000]);
    assert!(memory == expected);
}

#[test]
fn instruction_fetch_wraps_ip_between_prefix_opcode_and_offset_bytes() {
    for (ip, next) in [(0xFFFCu16, 0), (0xFFFD, 1), (0xFFFE, 2), (0xFFFF, 3)] {
        let (mut cpu, mut memory) = setup(&[]);
        cpu.ip = ip;
        for (index, byte) in [0x2E, 0xA0, 0x00, 0x02].into_iter().enumerate() {
            memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
        }
        memory[0x10200] = 0x73;
        step(&mut cpu, &mut memory);
        assert_eq!(cpu.ax, 0xA573);
        assert_eq!(cpu.ip, next);
        assert_eq!(cpu.cs, 0x1000);
        assert_eq!(cpu.flags, 0x0FD7);
    }
}

#[test]
fn cs_word_store_wraps_at_the_twenty_bit_bus_boundary() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.cs = 0xFFFF;
    cpu.ip = 0x0100;
    cpu.ds = 0x2000;
    cpu.ax = 0xA55A;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x100000];
    // FFFF:0100 wraps to physical 000F0; FFFF:000F is physical FFFFF.
    memory[0xF0..0xF4].copy_from_slice(&[0x2E, 0xA3, 0x0F, 0x00]);
    let mut expected = memory.clone();
    expected[0xFFFFF] = 0x5A;
    expected[0] = 0xA5;
    step(&mut cpu, &mut memory);
    assert!(memory == expected);
    assert_eq!(cpu.ip, 0x0104);
    assert_eq!(cpu.ax, 0xA55A);
    assert_eq!(cpu.flags, 0x0FD7);
}

#[test]
fn failed_cs_access_does_not_leave_an_override_on_the_next_instruction() {
    let (mut cpu, mut memory) = setup(&[0x2E, 0xA0, 0x00, 0x02, 0xA0, 0x00, 0x02]);
    cpu.ds = 0;
    memory.truncate(0x10200); // Instruction fetch succeeds; CS:0200 is missing.
    memory[0x0200] = 0x19;
    let original = memory.clone();
    assert!(matches!(
        cpu.step(&mut memory),
        Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
    ));
    assert_eq!(cpu.ax, 0xA55A);
    assert_eq!(cpu.ip, 0x0104);
    step(&mut cpu, &mut memory);
    assert_eq!(cpu.ax, 0xA519);
    assert_eq!(cpu.ip, 0x0107);
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == original);
}
