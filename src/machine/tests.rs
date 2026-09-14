use super::{Cpu, InterruptResult, Machine, StepResult};

#[test]
fn mov_al_preserves_ah() {
    for (immediate, expected_ax) in [(0x00, 0xAB00), (0x56, 0xAB56), (0xFF, 0xABFF)] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xABCD;
        cpu.ip = 2;
        let mut memory = [0, 0, 0xB0, immediate];

        let result = cpu.step(&mut memory).expect("MOV AL should succeed");

        assert!(matches!(result, StepResult::Continue));
        assert_eq!(cpu.ax, expected_ax, "immediate = {immediate:#04X}");
        assert_eq!(cpu.ip, 4);
    }
}

#[test]
fn mov_ah_preserves_al() {
    for (immediate, expected_ax) in [(0x00, 0x00CD), (0x56, 0x56CD), (0xFF, 0xFFCD)] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xABCD;
        cpu.ip = 2;
        let mut memory = [0, 0, 0xB4, immediate];

        let result = cpu.step(&mut memory).expect("MOV AH should succeed");

        assert!(matches!(result, StepResult::Continue));
        assert_eq!(cpu.ax, expected_ax, "immediate = {immediate:#04X}");
        assert_eq!(cpu.ip, 4);
    }
}

#[test]
fn fetch_word_reads_little_endian_and_advances_ip() {
    let mut cpu = Cpu::new();
    cpu.ip = 2;
    let memory = [0xAA, 0xBB, 0x34, 0x12, 0xCC];

    let word = cpu.fetch_next_word_and_advance_ip(&memory);

    assert_eq!(word, 0x1234);
    assert_eq!(cpu.ip, 4);
}

#[test]
fn write_service_clears_only_carry() {
    let mut machine = Machine {
        cpu: Cpu::new(),
        memory: vec![0; 16],
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
