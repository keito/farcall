use super::{Cpu, InterruptResult, Machine, MachineStepResult};
use std::time::Instant;

mod disk;

#[test]
fn divide_errors_enter_vector_zero_and_iret_resumes_after_div_on_8086() {
    // The 8086 saves the address after DIV, unlike later processors' restartable
    // divide faults. Test register and memory forms, including IP wrapping.
    // 8086 Family Users Manual, Interrupt Procedures (2-26), Division (2-37).
    for (dx, ax, divisor) in [(0, 10, 0), (1, 0, 1)] {
        for flags in [0xFDD7u16, 0xFFD7] {
            // IF clear/set; TF set in both.
            for (instruction, ip, return_ip) in [
                (&[0xF7, 0xF3][..], 0x0100, 0x0102u16),          // DIV BX.
                (&[0xF7, 0x36, 0x34, 0x12][..], 0x0100, 0x0104), // DIV word [DS:1234].
                (&[0xF7, 0xF3][..], 0xFFFF, 0x0001),
                (&[0xF7, 0x36, 0x34, 0x12][..], 0xFFFE, 0x0002),
            ] {
                let mut machine = Machine {
                    cpu: Cpu::new(),
                    memory: vec![0xCC; 0x50000],
                    handles: vec![],
                    floppy_image: None,
                    boot_time: Instant::now(),
                };
                machine.cpu.cs = 0x1000;
                machine.cpu.ip = ip;
                machine.cpu.ss = 0x3000;
                machine.cpu.sp = 0x0800;
                machine.cpu.ds = 0x4000;
                machine.cpu.ax = ax;
                machine.cpu.dx = dx;
                machine.cpu.bx = divisor;
                machine.cpu.flags = flags;
                // Type 0 vector: offset 0200h, segment 2000h, at physical zero.
                machine.memory[..4].copy_from_slice(&[0x00, 0x02, 0x00, 0x20]);
                machine.memory[0x41234..0x41236].copy_from_slice(&divisor.to_le_bytes());
                let before_wrap = instruction.len().min(0x10000 - usize::from(ip));
                let start = 0x10000 + usize::from(ip);
                machine.memory[start..start + before_wrap]
                    .copy_from_slice(&instruction[..before_wrap]);
                machine.memory[0x10000..0x10000 + instruction.len() - before_wrap]
                    .copy_from_slice(&instruction[before_wrap..]);
                let continuation = 0x10000 + usize::from(return_ip);
                // Following DIV: MOV BX,BEEFh; INT 20h.
                machine.memory[continuation..continuation + 5]
                    .copy_from_slice(&[0xBB, 0xEF, 0xBE, 0xCD, 0x20]);
                // Handler changes SI and arithmetic flags, then returns.
                machine.memory[0x20200..0x20202].copy_from_slice(&[0x46, 0xCF]); // INC SI; IRET.
                let original_memory = machine.memory.clone();

                assert!(matches!(
                    machine.step().unwrap(),
                    MachineStepResult::Continue
                ));

                assert_eq!(
                    (machine.cpu.cs, machine.cpu.ip, machine.cpu.sp),
                    (0x2000, 0x0200, 0x07FA)
                );
                let saved_flags =
                    u16::from_le_bytes([machine.memory[0x307FE], machine.memory[0x307FF]]);
                // DIV's arithmetic flags and error AX/DX results are undefined.
                // The saved control flags, however, must be the caller's.
                assert_eq!(saved_flags & !0x08D5, flags & !0x08D5);
                assert_eq!(
                    machine.cpu.flags,
                    saved_flags & !0x0300,
                    "interrupt entry clears IF/TF"
                );
                let [ip_low, ip_high] = return_ip.to_le_bytes();
                assert_eq!(
                    &machine.memory[0x307FA..0x307FE],
                    &[ip_low, ip_high, 0x00, 0x10]
                );
                let mut expected_memory = original_memory;
                expected_memory[0x307FA..0x30800].copy_from_slice(&[
                    ip_low,
                    ip_high,
                    0x00,
                    0x10,
                    saved_flags as u8,
                    (saved_flags >> 8) as u8,
                ]);
                assert!(
                    machine.memory == expected_memory,
                    "push exactly one interrupt frame through SS"
                );
                assert_eq!((machine.cpu.bx, machine.cpu.si), (divisor, 0));
                assert_eq!((machine.cpu.ss, machine.cpu.ds), (0x3000, 0x4000));

                assert!(matches!(
                    machine.step().unwrap(),
                    MachineStepResult::Continue
                )); // INC SI.
                assert_eq!(machine.cpu.si, 1);
                assert!(matches!(
                    machine.step().unwrap(),
                    MachineStepResult::Continue
                )); // IRET.
                assert_eq!(
                    (machine.cpu.cs, machine.cpu.ip, machine.cpu.sp),
                    (0x1000, return_ip, 0x0800)
                );
                assert_eq!(machine.cpu.flags, saved_flags);
                assert!(
                    machine.memory == expected_memory,
                    "IRET does not erase the stack frame"
                );

                assert!(matches!(
                    machine.step().unwrap(),
                    MachineStepResult::Continue
                )); // Caller MOV.
                assert_eq!(machine.cpu.bx, 0xBEEF);
                assert_eq!(machine.cpu.si, 1, "the divide handler ran exactly once");
                assert!(matches!(
                    machine.step().unwrap(),
                    MachineStepResult::Exit(0)
                ));
            }
        }
    }
}

#[test]
fn int60_runs_a_guest_handler_and_iret_resumes_the_caller_in_its_original_segment() {
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0xCC; 0x40000],
        handles: vec![],
        floppy_image: None,
        boot_time: Instant::now(),
    };
    machine.cpu.cs = 0x1000;
    machine.cpu.ip = 0x0100;
    machine.cpu.ss = 0x3000;
    machine.cpu.sp = 0x0800;
    machine.cpu.ds = 0x4000; // The vector table must not depend on DS.
    machine.cpu.flags = 0xF6D7;
    machine.memory[0x180..0x184].copy_from_slice(&[0x00, 0x02, 0x00, 0x20]);
    // INT 60h; MOV BX,BEEFh; INT 20h (existing hosted DOS exit).
    machine.memory[0x10100..0x10107].copy_from_slice(&[0xCD, 0x60, 0xBB, 0xEF, 0xBE, 0xCD, 0x20]);
    // Handler at 2000:0200: MOV AX,1234h; CMP AL,34h; IRET.
    machine.memory[0x20200..0x20206].copy_from_slice(&[0xB8, 0x34, 0x12, 0x3C, 0x34, 0xCF]);

    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    ));
    assert_eq!(
        (machine.cpu.cs, machine.cpu.ip, machine.cpu.sp),
        (0x2000, 0x0200, 0x07FA)
    );
    assert_eq!(machine.cpu.flags, 0xF4D7);
    assert_eq!(
        &machine.memory[0x307FA..0x30800],
        &[0x02, 0x01, 0x00, 0x10, 0xD7, 0xF6]
    );

    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    )); // MOV
    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    )); // CMP
    assert_eq!(machine.cpu.ax, 0x1234);
    assert_eq!(
        machine.cpu.flags, 0xF446,
        "handler changes flags before IRET"
    );
    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    )); // IRET
    assert_eq!(
        (machine.cpu.cs, machine.cpu.ip, machine.cpu.sp),
        (0x1000, 0x0102, 0x0800)
    );
    assert_eq!(machine.cpu.flags, 0xF6D7);
    assert_eq!(
        machine.cpu.ax, 0x1234,
        "IRET does not restore general registers"
    );
    assert_eq!((machine.cpu.ss, machine.cpu.ds), (0x3000, 0x4000));

    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    )); // Caller MOV
    assert_eq!(machine.cpu.bx, 0xBEEF);
    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Exit(0)
    ));
}

#[test]
fn write_service_clears_only_carry() {
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; 16],
        handles: vec![],
        floppy_image: None,
        boot_time: Instant::now(),
    };
    machine.cpu.ax = 0x4000;
    machine.cpu.bx = 1;
    machine.cpu.cx = 0;
    machine.cpu.dx = 8;
    machine.cpu.flags = 0x0243; // Carry, zero, interrupt-enable, and reserved bit 1.

    let result = machine
        .write_service()
        .expect("stdout write should succeed");

    assert!(matches!(result, InterruptResult::ResumeExecution));
    assert_eq!(machine.cpu.ax, 0);
    assert_eq!(machine.cpu.flags, 0x0242, "only carry should be cleared");
}

#[test]
fn dos_termination_returns_al_as_exit_code() {
    for (ax, expected_code) in [(0x4C00, 0), (0x4C01, 1), (0x4CFF, 255)] {
        let mut machine = Machine {
            cpu: Cpu::new(),
            memory: vec![],
            handles: vec![],
            floppy_image: None,
            boot_time: Instant::now(),
        };
        machine.cpu.ax = ax;

        let result = machine
            .handle_interrupt(0x21)
            .expect("DOS termination should succeed");

        match result {
            InterruptResult::Exit(code) => assert_eq!(code, expected_code),
            InterruptResult::ResumeExecution => panic!("termination must not resume execution"),
        }
    }
}
