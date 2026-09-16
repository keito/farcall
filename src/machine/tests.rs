use super::{Cpu, InterruptResult, Machine, MachineStepResult};

#[test]
fn int60_runs_a_guest_handler_and_iret_resumes_the_caller_in_its_original_segment() {
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0xCC; 0x40000],
        handles: vec![],
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
