use super::{Cpu, InterruptResult, Machine};

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
