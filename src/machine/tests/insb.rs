use super::{
    BiosKeyboard, CgaVideo, Cpu, CpuModel, Dma, Fdc, KeyboardInputMode, LoadMode, Machine,
    MachineStepResult, PcKeyboard, Pic, Pit, Ppi,
};

// Test complete guest instructions through Machine: INSB's input value arrives
// after CPU decoding, so these tests do not prescribe a port-event API.
// INSB uses ES:DI even with segment prefixes, preserves FLAGS/AX, and advances
// DI by +/-1. REP counts bytes and can be interrupted between transfers.
// Intel 286 manual, B-47 and REP:
// https://bitsavers.trailing-edge.com/components/intel/80286/210498-005_80286_and_80287_Programmers_Reference_Manual_1987.pdf
// 8088 6C aliases 7C (JL):
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
        0x5A3C, 0x2345, 0x0021, 0x3456, 0x0800, 0x4567, 0x6789, 0x0200,
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

fn check_input(machine: &mut Machine, value: u8, final_di: u16, final_cx: u16, final_ip: u16) {
    let mut expected_registers = registers(&machine.cpu);
    expected_registers[1] = final_cx;
    expected_registers[7] = final_di;
    let flags = machine.cpu.flags;
    let mut expected_memory = machine.memory.clone();
    let address = usize::from(machine.cpu.es) * 16 + usize::from(machine.cpu.di);
    expected_memory[address] = value;

    step(machine);

    assert_eq!(
        registers(&machine.cpu),
        expected_registers,
        "INSB preserves AX, DX, SI, segments and other registers"
    );
    assert_eq!(machine.cpu.ip, final_ip);
    assert_eq!(machine.cpu.flags, flags, "INSB preserves all FLAGS");
    assert!(
        machine.memory == expected_memory,
        "write exactly one byte at old ES:DI"
    );
}

fn queue_input(machine: &mut Machine) {
    machine.cpu.dx = FIFO;
    // Fixture supplies a FIFO of distinct bytes through the real FDC read path.
    // DOR remains zero, disabling the FDC IRQ so tests control interrupt timing.
    machine.fdc.complete_transfer(INPUT_BYTES);
}

#[test]
fn i286_insb_without_rep_updates_di_for_both_directions_including_wrap() {
    for (di, forward, backward) in [
        (0x0200, 0x0201, 0x01FF),
        (0x0000, 0x0001, 0xFFFF),
        (0xFFFE, 0xFFFF, 0xFFFD),
        (0xFFFF, 0x0000, 0xFFFE),
    ] {
        for (flags, final_di) in [(0x0002, forward), (0x0BD7, forward), (0x0FD7, backward)] {
            for cx in [0, 1, 0xFFFF] {
                for value in [0x00, 0x7F, 0x80, 0xFF] {
                    let mut machine = machine_for(&[0x6C]);
                    machine.cpu.di = di;
                    machine.cpu.cx = cx;
                    machine.cpu.flags = flags;
                    machine.write_port8(0x21, value).unwrap(); // PIC mask readback.
                    check_input(&mut machine, value, final_di, cx, 0x0101);
                }
            }
        }
    }
}

#[test]
fn i286_insb_stores_every_input_byte_without_loading_al() {
    for value in 0..=255u8 {
        let mut machine = machine_for(&[0x6C]);
        machine.cpu.ax = 0x5A00 | u16::from(!value);
        machine.write_port8(0x21, value).unwrap();
        check_input(&mut machine, value, 0x0201, 0x2345, 0x0101);
    }
}

#[test]
fn i286_insb_ignores_all_segment_overrides_and_always_writes_es_di() {
    for prefixes in [
        &[][..],
        &[0x26],
        &[0x2E],
        &[0x36],
        &[0x3E],
        &[0x26, 0x2E, 0x36, 0x3E],
    ] {
        let instruction: Vec<u8> = prefixes.iter().copied().chain([0x6C]).collect();
        for (flags, di) in [(0x0BD7, 0x0201), (0x0FD7, 0x01FF)] {
            let mut machine = machine_for(&instruction);
            machine.cpu.flags = flags;
            machine.write_port8(0x21, 0xA5).unwrap();
            // Distinct non-ES bytes make wrong segment destinations observable.
            machine.memory[0x10200] = 0x11;
            machine.memory[0x20200] = 0x22;
            machine.memory[0x40200] = 0x44;
            check_input(
                &mut machine,
                0xA5,
                di,
                0x2345,
                0x0100 + instruction.len() as u16,
            );
        }
    }
}

#[test]
fn i286_insb_uses_the_full_dx_port_address() {
    for (port, expected) in [
        (0x0021, 0xA5),
        (0x0121, 0xFF),
        (0x8000, 0xFF),
        (0xFFFF, 0xFF),
    ] {
        let mut machine = machine_for(&[0x6C]);
        machine.write_port8(0x21, 0xA5).unwrap();
        machine.cpu.dx = port;
        check_input(&mut machine, expected, 0x0201, 0x2345, 0x0101);
    }
}

#[test]
fn i286_insb_consumes_only_its_opcode_and_writes_only_one_destination_byte() {
    for prefixes in [&[][..], &[0x3E][..]] {
        let instruction: Vec<u8> = prefixes.iter().copied().chain([0x6C]).collect();
        let mut machine = machine_for(&[]);
        machine.cpu.ip = 0x4000 - instruction.len() as u16;
        machine.cpu.es = 0;
        machine.memory.truncate(0x14000);
        machine.memory[0x10000 + usize::from(machine.cpu.ip)..].copy_from_slice(&instruction);
        machine.write_port8(0x21, 0x80).unwrap();
        // No ModR/M or immediate byte exists after the opcode.
        check_input(&mut machine, 0x80, 0x0201, 0x2345, 0x4000);
    }

    let mut machine = machine_for(&[0x6C]);
    machine.cpu.es = 0;
    machine.cpu.di = 0x13FF;
    machine.memory.truncate(0x1400); // Move code to CS=0; last byte is the destination.
    machine.cpu.cs = 0;
    machine.memory[0x0100] = 0x6C;
    machine.write_port8(0x21, 0xA5).unwrap();
    check_input(&mut machine, 0xA5, 0x1400, 0x2345, 0x0101);
}

#[test]
fn i286_rep_insb_consumes_one_fifo_byte_per_step_and_finishes_on_the_last_byte() {
    for (backward, offsets) in [
        (false, [0x0200u16, 0x0201, 0x0202, 0x0203]),
        (true, [0x0200, 0x01FF, 0x01FE, 0x01FD]),
        (false, [0xFFFF, 0x0000, 0x0001, 0x0002]),
        (true, [0x0000, 0xFFFF, 0xFFFE, 0xFFFD]),
    ] {
        for count in [1u16, 3] {
            for instruction in [&[0xF3, 0x6C][..], &[0x3E, 0xF3, 0x6C], &[0xF3, 0x3E, 0x6C]] {
                for other_flags in [0x0002, 0x0BD7] {
                    // Both ZF values; no ZF termination.
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
                            INPUT_BYTES[index],
                            offsets[index + 1],
                            remaining,
                            if remaining == 0 { next_ip } else { 0x0100 },
                        );
                    }
                    let saved = registers(&machine.cpu);
                    let flags = machine.cpu.flags;
                    let original_memory = machine.memory.clone();
                    step(&mut machine); // Following NOP, not an extra empty REP step.
                    assert_eq!(machine.cpu.ip, next_ip + 1);
                    assert_eq!(registers(&machine.cpu), saved);
                    assert_eq!(machine.cpu.flags, flags);
                    assert!(machine.memory == original_memory);
                    assert_eq!(
                        machine.fdc.read(FIFO),
                        INPUT_BYTES[usize::from(count)],
                        "consume exactly CX device bytes, without speculative extra reads"
                    );
                }
            }
        }
    }
}

#[test]
fn i286_rep_insb_zero_count_neither_reads_the_port_nor_touches_destination() {
    for instruction in [&[0xF3, 0x6C][..], &[0x26, 0xF3, 0x6C], &[0xF3, 0x3E, 0x6C]] {
        for flags in [0x0002, 0x0BD7, 0x0FD7] {
            let mut machine = machine_for(instruction);
            machine.cpu.cx = 0;
            machine.cpu.di = 0xFFFF;
            machine.cpu.flags = flags;
            queue_input(&mut machine);
            let next_ip = 0x0100 + instruction.len() as u16;
            machine.memory[0x10000 + usize::from(next_ip)] = 0x90;
            // ES:FFFFh has no host RAM; an empty repetition must not access it.
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
            assert_eq!(machine.cpu.flags, flags);
            assert!(machine.memory == original_memory);
            assert_eq!(
                machine.fdc.read(FIFO),
                INPUT_BYTES[0],
                "zero count must not consume input"
            );
        }
    }
}

#[test]
fn i286_rep_insb_resumes_remaining_transfers_after_a_hardware_interrupt_and_iret() {
    let mut machine = machine_for(&[0xF3, 0x3E, 0x6C, 0x90]);
    machine.cpu.cx = 3;
    machine.cpu.flags = 0x0AD7; // IF set, DF clear; no debug trap requested.
    queue_input(&mut machine);
    // Default PIC IRQ3 vector is 0Bh. The handler consists of IRET.
    machine.memory[0x002C..0x0030].copy_from_slice(&[0x00, 0x04, 0x00, 0x50]);
    machine.memory[0x50400] = 0xCF;
    check_input(&mut machine, 0xA5, 0x0201, 2, 0x0100);
    let saved = registers(&machine.cpu);
    let mut expected_memory = machine.memory.clone();
    expected_memory[0x407FA..0x40800].copy_from_slice(&[
        0x00, 0x01, 0x00, 0x10, 0xD7, 0x0A, // Original IP (first prefix), CS, FLAGS.
    ]);

    machine.pic.set_irq_line(3, true);
    step(&mut machine); // Enter IRQ before CPU execution, then execute its IRET.

    assert_eq!(registers(&machine.cpu), saved);
    assert_eq!(machine.cpu.ip, 0x0100);
    assert_eq!(machine.cpu.flags, 0x0AD7);
    assert!(
        machine.memory == expected_memory,
        "interrupt saves restart IP before every prefix"
    );
    check_input(&mut machine, 0xB6, 0x0202, 1, 0x0100);
    check_input(&mut machine, 0xC7, 0x0203, 0, 0x0103);
    step(&mut machine);
    assert_eq!(machine.cpu.ip, 0x0104);
    assert_eq!(
        machine.fdc.read(FIFO),
        0xD8,
        "interrupt must not duplicate or lose an input byte"
    );
}

#[test]
fn i286_in_al_and_insb_route_input_to_their_own_destinations() {
    let mut machine = machine_for(&[0xEC, 0x6C, 0xE4, 0x21]);
    machine.write_port8(0x21, 0xA5).unwrap();
    let mut expected = registers(&machine.cpu);
    let original_memory = machine.memory.clone();
    step(&mut machine); // IN AL,DX.
    expected[0] = 0x5AA5;
    assert_eq!(registers(&machine.cpu), expected);
    assert_eq!(machine.cpu.ip, 0x0101);
    assert!(machine.memory == original_memory);
    machine.write_port8(0x21, 0xB6).unwrap();
    check_input(&mut machine, 0xB6, 0x0201, 0x2345, 0x0102);
    expected[7] = 0x0201;
    machine.write_port8(0x21, 0xC7).unwrap();
    let original_memory = machine.memory.clone();
    step(&mut machine); // IN AL,21h.
    expected[0] = 0x5AC7;
    assert_eq!(registers(&machine.cpu), expected);
    assert_eq!(machine.cpu.ip, 0x0104);
    assert_eq!(machine.cpu.flags, 0x0002);
    assert!(machine.memory == original_memory);
}

#[test]
fn insb_opcode_retains_8088_jl_alias_and_canonical_jl_on_both_models() {
    for (is_286, opcode) in [(false, 0x6C), (false, 0x7C), (true, 0x7C)] {
        for (condition, taken) in [
            (0x0000, false),
            (0x0080, true),
            (0x0800, true),
            (0x0880, false),
        ] {
            for unrelated in [0x0000, 0x0755] {
                // ZF and other flags do not affect JL.
                for (ip, displacement, target, next_ip) in [
                    (0x0100u16, 0x7F, 0x0181, 0x0102),
                    (0x0100, 0x80, 0x0082, 0x0102),
                    (0x7FFD, 0x01, 0x8000, 0x7FFF),
                    (0xFFFF, 0xFE, 0xFFFF, 0x0001),
                ] {
                    if is_286 && ip == 0xFFFF {
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
                    cpu.si = 0x1234;
                    cpu.di = 0x5678;
                    cpu.dx = FIFO;
                    cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | condition | unrelated;
                    let saved = registers(&cpu);
                    let flags = cpu.flags;
                    let mut memory = vec![0xCC; 0x10000];
                    memory[usize::from(ip)] = opcode;
                    memory[usize::from(ip.wrapping_add(1))] = displacement;
                    let original_memory = memory.clone();
                    assert!(matches!(
                        cpu.step(&mut memory).unwrap(),
                        crate::cpu::CpuStepResult::Continue
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
