use super::{Cpu, CpuStepResult, InterruptResult, Machine};

#[test]
fn dec_cx_updates_auxiliary_carry_on_low_nibble_borrow() {
    for (starting_cx, expected_cx, expected_af) in
        [(0x0010, 0x000F, 0x0010), (0x0011, 0x0010, 0x0000)]
    {
        // Start AF both clear and set to catch stale flag values.
        for initial_flags in [0x0000, 0x0010] {
            let mut cpu = Cpu::new();
            cpu.cx = starting_cx;
            cpu.flags = initial_flags;
            let mut memory = [0x49];

            let result = cpu.step(&mut memory).expect("DEC CX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.cx, expected_cx);
            assert_eq!(cpu.ip, 1);
            assert_eq!(
                cpu.flags & 0x0010,
                expected_af,
                "AF for CX={starting_cx:#06X}, initial flags={initial_flags:#06X}"
            );
        }
    }
}

#[test]
fn dec_cx_computes_parity_from_only_the_low_byte() {
    // The high byte has one set bit, so counting the entire word would
    // produce the opposite parity for both cases.
    for (starting_cx, expected_cx, expected_pf) in
        [(0x0101, 0x0100, 0x0004), (0x0102, 0x0101, 0x0000)]
    {
        for initial_flags in [0x0000, 0x0004] {
            let mut cpu = Cpu::new();
            cpu.cx = starting_cx;
            cpu.flags = initial_flags;
            let mut memory = [0x49];

            let result = cpu.step(&mut memory).expect("DEC CX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.cx, expected_cx);
            assert_eq!(cpu.ip, 1);
            assert_eq!(
                cpu.flags & 0x0004,
                expected_pf,
                "PF for CX={starting_cx:#06X}, initial flags={initial_flags:#06X}"
            );
        }
    }
}

#[test]
fn dec_cx_updates_sign_and_overflow() {
    let cases = [
        // Starting CX, result, SF mask, OF mask.
        (0x8000, 0x7FFF, 0x0000, 0x0800),
        (0x7FFF, 0x7FFE, 0x0000, 0x0000),
        (0x0000, 0xFFFF, 0x0080, 0x0000),
    ];

    for (starting_cx, expected_cx, expected_sf, expected_of) in cases {
        // Exercise both setting flags and clearing stale flags.
        for initial_flags in [0x0000, 0x08C1] {
            let mut cpu = Cpu::new();
            cpu.cx = starting_cx;
            cpu.flags = initial_flags;
            let mut memory = [0x49];

            let result = cpu.step(&mut memory).expect("DEC CX should succeed");

            assert!(matches!(result, CpuStepResult::Continue));
            assert_eq!(cpu.cx, expected_cx);
            assert_eq!(cpu.ip, 1);
            assert_eq!(
                cpu.flags & 0x0080,
                expected_sf,
                "SF for CX={starting_cx:#06X}"
            );
            assert_eq!(
                cpu.flags & 0x0800,
                expected_of,
                "OF for CX={starting_cx:#06X}"
            );
            assert_eq!(cpu.flags & 0x0040, 0, "all these results are nonzero");
            assert_eq!(cpu.flags & 1, initial_flags & 1, "DEC must preserve carry");
        }
    }
}

#[test]
fn dec_cx_clears_overflow_on_the_next_decrement() {
    let mut cpu = Cpu::new();
    cpu.cx = 0x8000;
    let mut memory = [0x49, 0x49];

    let first = cpu.step(&mut memory).expect("first DEC should succeed");

    assert!(matches!(first, CpuStepResult::Continue));
    assert_eq!(cpu.cx, 0x7FFF);
    assert_eq!(cpu.ip, 1);
    assert_eq!(cpu.flags & 0x0800, 0x0800, "first DEC must set OF");
    assert_eq!(cpu.flags & 0x00C0, 0, "SF and ZF must be clear");

    let second = cpu.step(&mut memory).expect("second DEC should succeed");

    assert!(matches!(second, CpuStepResult::Continue));
    assert_eq!(cpu.cx, 0x7FFE);
    assert_eq!(cpu.ip, 2);
    assert_eq!(cpu.flags & 0x0800, 0, "second DEC must clear OF");
    assert_eq!(cpu.flags & 0x00C0, 0, "SF and ZF must remain clear");
}

// These tests expect normal completion: a host arithmetic panic is a failure.
#[test]
fn dec_cx_wraps_zero_to_ffff() {
    let mut cpu = Cpu::new();
    cpu.cx = 0;
    cpu.flags = 0x0041; // Start with zero and carry set.
    let mut memory = [0x49];

    let result = cpu.step(&mut memory).expect("DEC CX should wrap at zero");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.cx, 0xFFFF);
    assert_eq!(cpu.ip, 1);
    assert_eq!(cpu.flags & 0x0040, 0, "the result is nonzero");
    assert_eq!(cpu.flags & 0x0001, 1, "DEC must preserve carry");
}

#[test]
fn jnz_forward_crosses_signed_16_bit_boundary() {
    let mut cpu = Cpu::new();
    cpu.ip = 0x7FFD;
    cpu.flags = 0; // JNZ is taken.
    let mut memory = vec![0; 0x10000];
    memory[0x7FFD..0x7FFF].copy_from_slice(&[0x75, 0x01]);

    // After fetching, IP is 0x7FFF. Adding one must produce 0x8000,
    // even though that calculation would overflow an i16.
    let result = cpu.step(&mut memory).expect("forward JNZ should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x8000);
    assert_eq!(cpu.flags, 0);
}

#[test]
fn jnz_backward_crosses_signed_16_bit_boundary() {
    let mut cpu = Cpu::new();
    cpu.ip = 0x7FFE;
    cpu.flags = 0;
    let mut memory = vec![0; 0x10000];
    memory[0x7FFE..0x8000].copy_from_slice(&[0x75, 0xFF]);

    // After fetching, IP is 0x8000. A displacement of -1 gives 0x7FFF.
    let result = cpu.step(&mut memory).expect("backward JNZ should succeed");

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(cpu.ip, 0x7FFF);
    assert_eq!(cpu.flags, 0);
}

#[test]
fn jnz_target_wraps_at_16_bit_address_limits() {
    // Neither instruction fetch crosses 0xFFFF; these isolate target arithmetic.
    for (start_ip, displacement, expected_ip) in [(0xFFFC, 0x02, 0x0000), (0x0000, 0xFD, 0xFFFF)] {
        let mut cpu = Cpu::new();
        cpu.ip = start_ip;
        cpu.flags = 0;
        let mut memory = vec![0; 0x10000];
        let start = start_ip as usize;
        memory[start..start + 2].copy_from_slice(&[0x75, displacement]);

        let result = cpu.step(&mut memory).expect("JNZ target should wrap");

        assert!(matches!(result, CpuStepResult::Continue));
        assert_eq!(cpu.ip, expected_ip, "starting IP = {start_ip:#06X}");
        assert_eq!(cpu.flags, 0);
    }
}

#[test]
fn mov_al_preserves_ah() {
    for (immediate, expected_ax) in [(0x00, 0xAB00), (0x56, 0xAB56), (0xFF, 0xABFF)] {
        let mut cpu = Cpu::new();
        cpu.ax = 0xABCD;
        cpu.ip = 2;
        let mut memory = [0, 0, 0xB0, immediate];

        let result = cpu.step(&mut memory).expect("MOV AL should succeed");

        assert!(matches!(result, CpuStepResult::Continue));
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

        assert!(matches!(result, CpuStepResult::Continue));
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
