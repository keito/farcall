use super::{Cpu, CpuError, CpuStepResult, MemoryError};

// INT saves FLAGS, CS, IP; IRET restores IP, CS, FLAGS. These tests cover
// software interrupt mechanics, not delivery of hardware IRQs or debug traps.
// https://pdos.csail.mit.edu/6.828/2004/readings/i386/INT.htm
// https://pdos.csail.mit.edu/6.828/2004/readings/i386/IRET.htm

fn other_registers(cpu: &Cpu) -> [u16; 10] {
    [
        cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.bp, cpu.si, cpu.di, cpu.ds, cpu.es, cpu.ss,
    ]
}

fn cpu_with_distinct_registers() -> Cpu {
    let mut cpu = Cpu::new();
    [cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.bp, cpu.si, cpu.di] =
        [0x1111, 0x2222, 0x3333, 0x4444, 0x5555, 0x6666, 0x7777];
    cpu.cs = 0x1000;
    cpu.ds = 0x4000;
    cpu.es = 0x5000;
    cpu.ss = 0x3000;
    cpu.sp = 0x0800;
    cpu.ip = 0x0100;
    cpu
}

#[test]
fn int_decoder_reports_unsigned_vector_and_advances_return_ip_including_wrap() {
    for interrupt in [0x00, 0x60, 0x80, 0xFF] {
        for (ip, next_ip, operand_address) in [
            (0x0100, 0x0102, 0x10101),
            (0xFFFE, 0x0000, 0x1FFFF),
            (0xFFFF, 0x0001, 0x10000),
        ] {
            let mut cpu = cpu_with_distinct_registers();
            cpu.ip = ip;
            cpu.flags = 0xFFD7;
            let original_registers = other_registers(&cpu);
            let mut memory = vec![0xCC; 0x20000];
            memory[0x10000 + usize::from(ip)] = 0xCD;
            memory[operand_address] = interrupt;
            let original_memory = memory.clone();

            // This project's CPU reports the event; Machine chooses whether
            // to enter a guest handler or invoke a hosted DOS/BIOS service.
            assert!(matches!(cpu.step(&mut memory).unwrap(),
                CpuStepResult::Interrupt(n) if n == interrupt));
            assert_eq!(cpu.ip, next_ip);
            assert_eq!(cpu.cs, 0x1000);
            assert_eq!(cpu.sp, 0x0800, "decoding must not push a second frame");
            assert_eq!(cpu.flags, 0xFFD7);
            assert_eq!(other_registers(&cpu), original_registers);
            assert!(memory == original_memory);
        }
    }
}

#[test]
fn interrupt_entry_reads_both_vector_words_and_saves_original_flags_cs_ip() {
    for interrupt in [0x00u8, 0x60, 0x80, 0xFF] {
        // Independently exercise IF/TF set and clear, plus other flags.
        // Reserved bits use values appropriate for the 8088 target.
        for flags in [0xF002u16, 0xF203, 0xF102, 0xFFD7] {
            let mut cpu = cpu_with_distinct_registers();
            cpu.ip = 0xABCD; // Already points after the INT instruction.
            cpu.flags = flags;
            let original_registers = other_registers(&cpu);
            let mut memory = vec![0xCC; 0x100000];
            let vector = usize::from(interrupt) * 4;
            // IP=1234h, CS=5678h, stored at physical segment zero.
            memory[vector..vector + 4].copy_from_slice(&[0x34, 0x12, 0x78, 0x56]);
            let mut expected_memory = memory.clone();
            let [flags_low, flags_high] = flags.to_le_bytes();
            expected_memory[0x307FA..0x30800]
                .copy_from_slice(&[0xCD, 0xAB, 0x00, 0x10, flags_low, flags_high]);

            cpu.enter_interrupt(&mut memory, interrupt).unwrap();

            assert_eq!((cpu.cs, cpu.ip), (0x5678, 0x1234));
            assert_eq!(cpu.sp, 0x07FA);
            assert_eq!(cpu.flags, flags & !0x0300, "only IF and TF clear");
            assert_eq!(other_registers(&cpu), original_registers);
            assert!(
                memory == expected_memory,
                "vector={interrupt:02X}, flags={flags:04X}"
            );
        }
    }
}

#[test]
fn iret_restores_a_prebuilt_frame_in_ip_cs_flags_order_without_writing_memory() {
    for restored_flags in [0xF002u16, 0xF203, 0xF102, 0xFFD7] {
        let mut cpu = cpu_with_distinct_registers();
        cpu.flags = restored_flags ^ 0x0FD5; // Opposite values for writable flags.
        let original_registers = other_registers(&cpu);
        let mut memory = vec![0xCC; 0x40000];
        memory[0x10100] = 0xCF;
        let [flags_low, flags_high] = restored_flags.to_le_bytes();
        memory[0x30800..0x30806].copy_from_slice(&[0xEF, 0xBE, 0x57, 0x13, flags_low, flags_high]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));

        assert_eq!(
            (cpu.cs, cpu.ip),
            (0x1357, 0xBEEF),
            "do not advance restored IP"
        );
        assert_eq!(cpu.sp, 0x0806);
        assert_eq!(cpu.flags, restored_flags);
        assert_eq!(other_registers(&cpu), original_registers);
        assert!(memory == original_memory, "popping doesn't erase the frame");
    }
}

#[test]
fn interrupt_and_iret_wrap_stack_offsets_and_the_twenty_bit_bus() {
    for (ss, sp, frame_sp, word_addresses) in [
        // Frame crosses the 16-bit SP boundary between IP and CS.
        (0x3000, 0x0004, 0xFFFE, [0x3FFFE, 0x30000, 0x30002]),
        // Saved FLAGS straddles physical FFFFFh and 00000h.
        (0xFFFF, 0x0011, 0x000B, [0xFFFFB, 0xFFFFD, 0xFFFFF]),
    ] {
        let mut cpu = cpu_with_distinct_registers();
        cpu.ss = ss;
        cpu.sp = sp;
        cpu.ip = 0x3456;
        cpu.flags = 0xFFD7;
        let original_registers = other_registers(&cpu);
        let mut memory = vec![0xCC; 0x100000];
        memory[0x0180..0x0184].copy_from_slice(&[0x00, 0x01, 0x00, 0x20]);
        memory[0x20100] = 0xCF;
        let mut expected_memory = memory.clone();
        for (address, bytes) in word_addresses.into_iter().zip([
            [0x56, 0x34], // IP
            [0x00, 0x10], // CS
            [0xD7, 0xFF], // FLAGS
        ]) {
            expected_memory[address] = bytes[0];
            expected_memory[(address + 1) & 0xFFFFF] = bytes[1];
        }

        cpu.enter_interrupt(&mut memory, 0x60).unwrap();
        assert_eq!(
            (cpu.cs, cpu.ip, cpu.sp, cpu.flags),
            (0x2000, 0x0100, frame_sp, 0xFCD7)
        );
        assert!(
            memory == expected_memory,
            "SS={ss:04X}, original SP={sp:04X}"
        );

        assert!(matches!(
            cpu.step(&mut memory).unwrap(),
            CpuStepResult::Continue
        ));
        assert_eq!(
            (cpu.cs, cpu.ip, cpu.sp, cpu.flags),
            (0x1000, 0x3456, sp, 0xFFD7)
        );
        assert_eq!(other_registers(&cpu), original_registers);
        assert!(memory == expected_memory);
    }
}

#[test]
fn nested_interrupt_frames_return_to_their_respective_callers() {
    let mut cpu = cpu_with_distinct_registers();
    cpu.ip = 0x1234;
    cpu.flags = 0xF6D7;
    let original_registers = other_registers(&cpu);
    let mut memory = vec![0xCC; 0x40000];
    memory[0x0180..0x0188].copy_from_slice(&[
        0x00, 0x02, 0x00, 0x20, // INT 60h -> 2000:0200
        0x00, 0x04, 0x00, 0x20, // INT 61h -> 2000:0400
    ]);
    memory[0x20200..0x20203].copy_from_slice(&[0xCD, 0x61, 0xCF]);
    memory[0x20400] = 0xCF;

    cpu.enter_interrupt(&mut memory, 0x60).unwrap();
    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Interrupt(0x61)
    ));
    assert_eq!(cpu.ip, 0x0202);
    cpu.enter_interrupt(&mut memory, 0x61).unwrap();
    assert_eq!(cpu.sp, 0x07F4);
    assert_eq!(
        &memory[0x307F4..0x30800],
        &[
            0x02, 0x02, 0x00, 0x20, 0xD7, 0xF4, // Inner frame.
            0x34, 0x12, 0x00, 0x10, 0xD7, 0xF6, // Outer frame.
        ]
    );

    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));
    assert_eq!(
        (cpu.cs, cpu.ip, cpu.sp, cpu.flags),
        (0x2000, 0x0202, 0x07FA, 0xF4D7)
    );
    assert!(matches!(
        cpu.step(&mut memory).unwrap(),
        CpuStepResult::Continue
    ));
    assert_eq!(
        (cpu.cs, cpu.ip, cpu.sp, cpu.flags),
        (0x1000, 0x1234, 0x0800, 0xF6D7)
    );
    assert_eq!(other_registers(&cpu), original_registers);
}

#[test]
fn interrupt_entry_reports_missing_stack_or_vector_bytes_as_memory_errors() {
    for (memory_len, sp) in [
        (0x200, 0x0202), // First pushed word is outside the memory slice.
        (0x180, 0x0100), // Missing vector IP word.
        (0x181, 0x0100), // Only half of vector IP exists.
        (0x182, 0x0100), // Missing vector CS word.
        (0x183, 0x0100), // Only half of vector CS exists.
    ] {
        let mut cpu = Cpu::new();
        cpu.sp = sp;
        cpu.flags = 0xF002;
        let mut memory = vec![0xCC; memory_len];
        assert!(matches!(
            cpu.enter_interrupt(&mut memory, 0x60),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));
        // No atomic rollback contract is imposed on this multi-access operation.
    }
}

#[test]
fn iret_reports_a_truncated_frame_as_a_memory_error_without_writing_memory() {
    for frame_bytes in 0..6 {
        let mut cpu = Cpu::new();
        cpu.sp = 0x0100;
        let mut memory = vec![0xCC; 0x0100 + frame_bytes];
        memory[0] = 0xCF;
        let frame = [0x34, 0x12, 0x00, 0x20, 0x02, 0xF0];
        memory[0x0100..].copy_from_slice(&frame[..frame_bytes]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));
        assert_eq!(memory, original_memory);
    }
}
