use super::{
    BiosKeyboard, CgaVideo, Cpu, CpuModel, Dma, Fdc, KeyboardInputMode, LoadMode, Machine,
    MachineStepResult, PcKeyboard, Pic, Pit, Ppi,
};
use crate::cpu::CpuStepResult;

// INSW writes a little-endian word to ES:DI, ignores segment overrides, moves DI
// by +/-2 and preserves FLAGS/AX. REP counts words; real-mode DI=FFFFh raises
// #GP(13) with restart IP, except when REP has CX=0 and does no input at all.
// https://bitsavers.trailing-edge.com/components/intel/80286/210498-005_80286_and_80287_Programmers_Reference_Manual_1987.pdf
// 8088 6D aliases 7D: JGE (SF == OF), independent of ZF.
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

const FIFO: u16 = 0x3F5;
const INPUT_BYTES: [u8; 7] = [0xA5, 0xB6, 0xC7, 0xD8, 0xE9, 0xFA, 0x0B];

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn machine_for(instruction: &[u8]) -> Machine {
    let mut machine = Machine {
        bios: None,
        bios_keyboard: BiosKeyboard::new(),
        cga_video: CgaVideo::new(),
        cpu: Cpu::new(CpuModel::I80286, 0),
        dma: Dma::new(),
        dos: None,
        fdc: Fdc::new(),
        floppy_images: [None, None],
        keyboard_input_mode: KeyboardInputMode::BiosKeystrokes,
        load_mode: LoadMode::Rom,
        memory: vec![0xCC; 0x60000],
        pc_keyboard: PcKeyboard::new(),
        pic: Pic::new(),
        pit: Pit::new(),
        pit_cycle_remainder: 0,
        ppi: Ppi::new(),
        synthetic_int09_pending: false,
        xt_cf: None,
        total_elapsed_cycles: 0,
        verbosity: 0,
    };
    [
        machine.cpu.ax,
        machine.cpu.cx,
        machine.cpu.dx,
        machine.cpu.bx,
        machine.cpu.sp,
        machine.cpu.bp,
        machine.cpu.si,
        machine.cpu.di,
    ] = [
        0x5A3C, 0x2345, 0x0020, 0x3456, 0x0800, 0x4567, 0x6789, 0x0200,
    ];
    [
        machine.cpu.cs,
        machine.cpu.ds,
        machine.cpu.es,
        machine.cpu.ss,
    ] = [0x1000, 0x2000, 0x3000, 0x4000];
    machine.cpu.ip = 0x0100;
    machine.memory[0x10100..0x10100 + instruction.len()].copy_from_slice(instruction);
    machine
}

fn step(machine: &mut Machine) {
    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    ));
}

fn check_input(machine: &mut Machine, bytes: [u8; 2], di: u16, cx: u16, ip: u16) {
    let mut expected_registers = registers(&machine.cpu);
    expected_registers[1] = cx;
    expected_registers[7] = di;
    let flags = machine.cpu.flags;
    let mut expected_memory = machine.memory.clone();
    let address = usize::from(machine.cpu.es) * 16 + usize::from(machine.cpu.di);
    expected_memory[address..address + 2].copy_from_slice(&bytes);

    step(machine);

    assert_eq!(
        registers(&machine.cpu),
        expected_registers,
        "INSW preserves AX, DX, SI and segments"
    );
    assert_eq!(machine.cpu.ip, ip);
    assert_eq!(machine.cpu.flags, flags);
    assert!(
        machine.memory == expected_memory,
        "store exactly two bytes at old ES:DI in little-endian order"
    );
}

fn pic_word(machine: &mut Machine, low: u8, high: u8) {
    // The PIC is an 8-bit device: a word input at 20h reads IRR at 20h and
    // IMR at 21h. This provides independent known low/high bytes without a
    // new device mock. Keep IF clear when requesting arbitrary IRQ bits.
    machine.cpu.dx = 0x20;
    for line in 0..8 {
        if low & (1 << line) != 0 {
            machine.pic.set_irq_line(line, true);
        }
    }
    machine.pic.write(0x21, high);
}

fn queue_input(machine: &mut Machine) {
    machine.cpu.dx = FIFO;
    // On the byte-wide FDC, each word reads FIFO low byte at 3F5h, then FFh
    // from unmapped 3F6h. One FIFO byte is consumed per word, not two.
    // DOR stays zero so the FDC itself cannot interrupt these fixtures.
    machine.fdc.complete_transfer(INPUT_BYTES);
}

#[test]
fn i286_insw_without_rep_stores_words_and_moves_di_by_two_in_both_directions() {
    for (di, forward, backward) in [
        (0x0200, 0x0202, 0x01FE),
        (0x0000, 0x0002, 0xFFFE),
        (0x0001, 0x0003, 0xFFFF), // Unaligned words are valid.
        (0xFFFD, 0xFFFF, 0xFFFB),
        (0xFFFE, 0x0000, 0xFFFC),
    ] {
        for (flags, final_di) in [(0x0002, forward), (0x09D7, forward), (0x0DD7, backward)] {
            for cx in [0, 1, 0xFFFF] {
                for bytes in [
                    [0x00, 0x00],
                    [0x80, 0],
                    [0, 0x80],
                    [0x34, 0xA5],
                    [0xFF, 0xFF],
                ] {
                    let mut machine = machine_for(&[0x6D]);
                    machine.cpu.di = di;
                    machine.cpu.cx = cx;
                    machine.cpu.flags = flags;
                    pic_word(&mut machine, bytes[0], bytes[1]);
                    check_input(&mut machine, bytes, final_di, cx, 0x0101);
                }
            }
        }
    }
}

#[test]
fn i286_insw_ignores_segment_overrides_and_writes_only_es_di() {
    for prefixes in [
        &[][..],
        &[0x26],
        &[0x2E],
        &[0x36],
        &[0x3E],
        &[0x26, 0x2E, 0x36, 0x3E],
    ] {
        let instruction: Vec<u8> = prefixes.iter().copied().chain([0x6D]).collect();
        for (flags, di) in [(0x09D7, 0x0202), (0x0DD7, 0x01FE)] {
            let mut machine = machine_for(&instruction);
            machine.cpu.flags = flags;
            pic_word(&mut machine, 0x34, 0xA5);
            machine.memory[0x10200..0x10202].fill(0x11);
            machine.memory[0x20200..0x20202].fill(0x22);
            machine.memory[0x40200..0x40202].fill(0x44);
            check_input(
                &mut machine,
                [0x34, 0xA5],
                di,
                0x2345,
                0x0100 + instruction.len() as u16,
            );
        }
    }
}

#[test]
fn i286_insw_uses_full_dx_without_truncating_the_port() {
    for (port, bytes) in [
        (0x0020, [0x34, 0xA5]),
        (0x0120, [0xFF, 0xFF]),
        (0x8000, [0xFF, 0xFF]),
        (0xFFFF, [0xFF, 0xFF]),
    ] {
        let mut machine = machine_for(&[0x6D]);
        pic_word(&mut machine, 0x34, 0xA5);
        machine.cpu.dx = port;
        check_input(&mut machine, bytes, 0x0202, 0x2345, 0x0101);
    }
}

#[test]
fn i286_rep_insw_counts_words_and_reads_one_word_per_step() {
    for (backward, offsets) in [
        (false, [0x0200u16, 0x0202, 0x0204, 0x0206]),
        (true, [0x0200, 0x01FE, 0x01FC, 0x01FA]),
        (false, [0xFFFE, 0x0000, 0x0002, 0x0004]),
        (true, [0x0000, 0xFFFE, 0xFFFC, 0xFFFA]),
    ] {
        for count in [1u16, 3] {
            for instruction in [&[0xF3, 0x6D][..], &[0x3E, 0xF3, 0x6D], &[0xF3, 0x3E, 0x6D]] {
                for other_flags in [0x0002, 0x0BD7] {
                    // REP ignores ZF.
                    let mut machine = machine_for(instruction);
                    machine.cpu.cx = count;
                    machine.cpu.di = offsets[0];
                    machine.cpu.flags = other_flags | if backward { 0x0400 } else { 0 };
                    queue_input(&mut machine);
                    let next_ip = 0x0100 + instruction.len() as u16;
                    machine.memory[0x10000 + usize::from(next_ip)] = 0x90;
                    for index in 0..usize::from(count) {
                        let remaining = count - index as u16 - 1;
                        check_input(
                            &mut machine,
                            [INPUT_BYTES[index], 0xFF],
                            offsets[index + 1],
                            remaining,
                            if remaining == 0 { next_ip } else { 0x0100 },
                        );
                    }
                    let saved = registers(&machine.cpu);
                    let flags = machine.cpu.flags;
                    let original_memory = machine.memory.clone();
                    step(&mut machine);
                    assert_eq!(
                        machine.cpu.ip,
                        next_ip + 1,
                        "following NOP executes without an extra empty REP step"
                    );
                    assert_eq!(registers(&machine.cpu), saved);
                    assert_eq!(machine.cpu.flags, flags);
                    assert!(machine.memory == original_memory);
                    assert_eq!(
                        machine.fdc.read(FIFO),
                        INPUT_BYTES[usize::from(count)],
                        "exactly one FIFO read per word"
                    );
                }
            }
        }
    }
}

#[test]
fn i286_rep_insw_zero_count_skips_input_and_the_ffff_destination_check() {
    for instruction in [&[0xF3, 0x6D][..], &[0x26, 0xF3, 0x6D], &[0xF3, 0x3E, 0x6D]] {
        for flags in [0x0002, 0x0BD7, 0x0FD7] {
            let mut machine = machine_for(instruction);
            machine.cpu.cx = 0;
            machine.cpu.di = 0xFFFF;
            machine.cpu.flags = flags;
            queue_input(&mut machine);
            let next_ip = 0x0100 + instruction.len() as u16;
            machine.memory[0x10000 + usize::from(next_ip)] = 0x90;
            machine.memory.truncate(0x10000 + usize::from(next_ip) + 1);
            let saved = registers(&machine.cpu);
            let original_memory = machine.memory.clone();
            step(&mut machine);
            assert_eq!(machine.cpu.ip, next_ip);
            assert_eq!(registers(&machine.cpu), saved);
            assert_eq!(machine.cpu.flags, flags);
            assert!(machine.memory == original_memory);
            step(&mut machine);
            assert_eq!(machine.cpu.ip, next_ip + 1);
            assert_eq!(registers(&machine.cpu), saved);
            assert!(machine.memory == original_memory);
            assert_eq!(
                machine.fdc.read(FIFO),
                INPUT_BYTES[0],
                "empty REP must not read either input byte"
            );
        }
    }
}

#[test]
fn i286_insw_ffff_destination_raises_gp_before_input_or_register_updates() {
    for (instruction, repeated) in [
        (&[0x6D][..], false),
        (&[0x36, 0x6D][..], false),
        (&[0x3E, 0xF3, 0x6D][..], true),
        (&[0xF3, 0x26, 0x6D][..], true),
    ] {
        for count in [0, 1, 3] {
            if repeated && count == 0 {
                continue;
            } // Tested separately above.
            let mut machine = machine_for(instruction);
            machine.cpu.di = 0xFFFF;
            machine.cpu.cx = count;
            machine.cpu.flags = 0x0FD7;
            queue_input(&mut machine);
            let saved = registers(&machine.cpu);
            let original_memory = machine.memory.clone();

            assert!(
                matches!(
                    machine.cpu.step(&mut machine.memory).unwrap(),
                    CpuStepResult::Interrupt(13)
                ),
                "nonempty INSW at ES:FFFF must report #GP"
            );

            assert_eq!(
                machine.cpu.ip, 0x0100,
                "fault restart includes every prefix"
            );
            assert_eq!(registers(&machine.cpu), saved);
            assert_eq!(machine.cpu.flags, 0x0FD7);
            assert!(
                machine.memory == original_memory,
                "do not wrap the second store to ES:0000"
            );
            assert_eq!(machine.fdc.read(FIFO), INPUT_BYTES[0]);
        }
    }
}

#[test]
fn i286_rep_insw_fault_after_one_word_preserves_progress_and_remaining_input() {
    let mut machine = machine_for(&[0x3E, 0xF3, 0x6D]);
    machine.cpu.di = 0xFFFD;
    machine.cpu.cx = 2;
    machine.cpu.flags = 0x0BD7;
    queue_input(&mut machine);
    check_input(&mut machine, [0xA5, 0xFF], 0xFFFF, 1, 0x0100);
    let saved = registers(&machine.cpu);
    let original_memory = machine.memory.clone();

    assert!(matches!(
        machine.cpu.step(&mut machine.memory).unwrap(),
        CpuStepResult::Interrupt(13)
    ));

    assert_eq!(
        (machine.cpu.ip, machine.cpu.di, machine.cpu.cx),
        (0x0100, 0xFFFF, 1)
    );
    assert_eq!(registers(&machine.cpu), saved);
    assert_eq!(machine.cpu.flags, 0x0BD7);
    assert!(machine.memory == original_memory);
    assert_eq!(
        machine.fdc.read(FIFO),
        0xB6,
        "the faulting iteration must not consume another word"
    );
}

#[test]
fn i286_insw_decodes_one_opcode_byte_and_writes_exactly_two_bytes() {
    let mut machine = machine_for(&[]);
    machine.cpu.ip = 0x3FFF;
    machine.cpu.es = 0;
    machine.memory.truncate(0x14000);
    machine.memory[0x13FFF] = 0x6D; // No ModR/M or immediate follows.
    pic_word(&mut machine, 0x34, 0xA5);
    check_input(&mut machine, [0x34, 0xA5], 0x0202, 0x2345, 0x4000);

    let mut machine = machine_for(&[0x6D]);
    machine.cpu.cs = 0;
    machine.cpu.es = 0;
    machine.cpu.di = 0x13FE;
    machine.memory.truncate(0x1400);
    machine.memory[0x0100] = 0x6D;
    pic_word(&mut machine, 0x34, 0xA5);
    check_input(&mut machine, [0x34, 0xA5], 0x1400, 0x2345, 0x0101);
}

#[test]
fn i286_rep_insw_resumes_remaining_words_after_irq_and_iret() {
    let mut machine = machine_for(&[0xF3, 0x3E, 0x6D, 0x90]);
    machine.cpu.cx = 3;
    machine.cpu.flags = 0x0AD7; // IF=1, DF=0.
    queue_input(&mut machine);
    machine.memory[0x002C..0x0030].copy_from_slice(&[0x00, 0x04, 0x00, 0x50]);
    machine.memory[0x50400] = 0xCF; // IRQ3 handler: IRET.
    check_input(&mut machine, [0xA5, 0xFF], 0x0202, 2, 0x0100);
    let saved = registers(&machine.cpu);
    let mut expected_memory = machine.memory.clone();
    expected_memory[0x407FA..0x40800].copy_from_slice(&[0x00, 0x01, 0x00, 0x10, 0xD7, 0x0A]);
    machine.pic.set_irq_line(3, true);
    step(&mut machine); // IRQ entry, then its IRET before the next iteration.
    assert_eq!(registers(&machine.cpu), saved);
    assert_eq!(machine.cpu.ip, 0x0100);
    assert_eq!(machine.cpu.flags, 0x0AD7);
    assert!(machine.memory == expected_memory);
    check_input(&mut machine, [0xB6, 0xFF], 0x0204, 1, 0x0100);
    check_input(&mut machine, [0xC7, 0xFF], 0x0206, 0, 0x0103);
    step(&mut machine);
    assert_eq!(machine.cpu.ip, 0x0104);
    assert_eq!(machine.fdc.read(FIFO), 0xD8);
}

#[test]
fn i286_in_ax_and_insw_route_word_input_to_different_destinations() {
    let mut machine = machine_for(&[0xED, 0x6D, 0xE5, 0x20]);
    pic_word(&mut machine, 0x34, 0xA5);
    let mut expected = registers(&machine.cpu);
    let original_memory = machine.memory.clone();
    step(&mut machine); // IN AX,DX.
    expected[0] = 0xA534;
    assert_eq!(registers(&machine.cpu), expected);
    assert_eq!(machine.cpu.ip, 0x0101);
    assert!(machine.memory == original_memory);
    machine.pic.write(0x21, 0xB6);
    check_input(&mut machine, [0x34, 0xB6], 0x0202, 0x2345, 0x0102);
    expected[7] = 0x0202;
    machine.pic.write(0x21, 0xC7);
    let original_memory = machine.memory.clone();
    step(&mut machine); // IN AX,20h.
    expected[0] = 0xC734;
    assert_eq!(registers(&machine.cpu), expected);
    assert_eq!(machine.cpu.ip, 0x0104);
    assert_eq!(machine.cpu.flags, 0x0002);
    assert!(machine.memory == original_memory);
}

#[test]
fn insw_opcode_retains_8088_jge_alias_and_canonical_jge_on_both_models() {
    for (is_286, opcode) in [(false, 0x6D), (false, 0x7D), (true, 0x7D)] {
        for (condition, taken) in [
            (0x0000, true),
            (0x0080, false),
            (0x0800, false),
            (0x0880, true),
        ] {
            for unrelated in [0x0000, 0x0755] {
                // ZF does not affect JGE.
                for (ip, displacement, target, next_ip) in [
                    (0x0100u16, 0x7F, 0x0181, 0x0102),
                    (0x0100, 0x80, 0x0082, 0x0102),
                    (0x7FFD, 0x01, 0x8000, 0x7FFF),
                    (0x0000, 0xFD, 0xFFFF, 0x0002),
                    (0xFFFC, 0x02, 0x0000, 0xFFFE),
                    (0xFFFF, 0xFE, 0xFFFF, 0x0001),
                ] {
                    if is_286 && matches!(ip, 0x0000 | 0xFFFC | 0xFFFF) {
                        continue;
                    }
                    let mut cpu = Cpu::new(
                        if is_286 {
                            CpuModel::I80286
                        } else {
                            CpuModel::I8088
                        },
                        0,
                    );
                    cpu.ip = ip;
                    cpu.dx = FIFO;
                    cpu.di = 0xFFFF;
                    cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | condition | unrelated;
                    let saved = registers(&cpu);
                    let flags = cpu.flags;
                    let mut memory = vec![0xCC; 0x10000];
                    memory[usize::from(ip)] = opcode;
                    memory[usize::from(ip.wrapping_add(1))] = displacement;
                    let original_memory = memory.clone();
                    assert!(matches!(
                        cpu.step(&mut memory).unwrap(),
                        CpuStepResult::Continue
                    ));
                    assert_eq!(cpu.ip, if taken { target } else { next_ip });
                    assert_eq!(registers(&cpu), saved);
                    assert_eq!(cpu.flags, flags);
                    assert!(memory == original_memory);
                }
            }
        }
    }
}

#[test]
fn i8088_6d_alias_consumes_one_displacement_without_accessing_destination() {
    let mut cpu = Cpu::new(CpuModel::I8088, 0);
    cpu.ip = 0x3FFE;
    cpu.es = 0x3000;
    cpu.di = 0xFFFF; // Invalid word destination is irrelevant to an 8088 jump.
    let saved = registers(&cpu);
    let flags = cpu.flags;
    let mut memory = vec![0xCC; 0x4000];
    memory[0x3FFE..].copy_from_slice(&[0x6D, 0x80]);
    let original_memory = memory.clone();
    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));
    assert_eq!(cpu.ip, 0x3F80);
    assert_eq!(registers(&cpu), saved);
    assert_eq!(cpu.flags, flags);
    assert!(memory == original_memory);
}
