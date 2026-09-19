use super::{Cpu, CpuError, CpuStepResult, MemoryError};

const POINTER: [u8; 4] = [0x34, 0x12, 0x78, 0x56]; // 5678:1234, offset first.

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn setup(instruction: &[u8], ip: u16) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0xA55A, 0x1234, 0x5678, 0x0100, 0x0900, 0x0800, 0x0200, 0x0400,
    ];
    [cpu.cs, cpu.ds, cpu.ss, cpu.es] = [0x1000, 0x2000, 0x3000, 0x4000];
    cpu.ip = ip;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x60000];
    for (index, byte) in instruction.iter().copied().chain([0x90]).enumerate() {
        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
    }
    memory[0x579B4] = 0xCB; // RETF at 5678:1234.
    (cpu, memory)
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

fn check_round_trip(mut cpu: Cpu, mut memory: Vec<u8>, instruction_len: u16) {
    let original = registers(&cpu);
    let flags = cpu.flags;
    let return_ip = cpu.ip.wrapping_add(instruction_len);
    let next_sp = cpu.sp.wrapping_sub(4);
    let mut expected_memory = memory.clone();
    // Return IP must be on top of the stack, with caller CS in the next word.
    for (offset, word) in [(next_sp, return_ip), (next_sp.wrapping_add(2), cpu.cs)] {
        let address = (usize::from(cpu.ss) * 16 + usize::from(offset)) & 0xFFFFF;
        let [low, high] = word.to_le_bytes();
        expected_memory[address] = low;
        let high_address =
            (usize::from(cpu.ss) * 16 + usize::from(offset.wrapping_add(1))) & 0xFFFFF;
        expected_memory[high_address] = high;
    }
    let mut expected = original;
    expected[4] = next_sp;
    expected[9] = 0x5678;

    step(&mut cpu, &mut memory);

    assert_eq!(registers(&cpu), expected, "CALL changes only CS, IP and SP");
    assert_eq!(
        cpu.ip, 0x1234,
        "the first pointer word is the target offset"
    );
    assert_eq!(cpu.flags, flags);
    assert!(
        memory == expected_memory,
        "only the four-byte return address is written"
    );

    step(&mut cpu, &mut memory); // RETF, fetched from the new CS:IP.

    assert_eq!(registers(&cpu), original);
    assert_eq!(cpu.ip, return_ip);
    assert_eq!(cpu.flags, flags);
    assert!(
        memory == expected_memory,
        "RETF reads the stack without erasing it"
    );

    step(&mut cpu, &mut memory); // NOP immediately after the original CALL.

    assert_eq!(registers(&cpu), original);
    assert_eq!(cpu.ip, return_ip.wrapping_add(1));
    assert_eq!(cpu.flags, flags);
    assert!(memory == expected_memory);
}

#[test]
fn indirect_far_call_reads_offset_then_segment_and_pushes_cs_then_return_ip() {
    for (operand, offset, default_base) in [
        (&[0x1E, 0x01, 0x02][..], 0x0201, 0x20000), // [DS:0201h], unaligned.
        (&[0x18][..], 0x0300, 0x20000),             // [BX+SI], DS.
        (&[0x5E, 0xFE][..], 0x07FE, 0x30000),       // [BP-2], SS.
        (&[0x9B, 0xF0, 0xFF][..], 0x0BF0, 0x30000), // [BP+DI-16], SS.
    ] {
        for (prefix, source_base) in [
            (None, default_base),
            (Some(0x26), 0x40000),
            (Some(0x2E), 0x10000),
            (Some(0x36), 0x30000),
            (Some(0x3E), 0x20000),
        ] {
            for flags in [0, 0x0FD7] {
                let mut instruction = Vec::new();
                if let Some(prefix) = prefix {
                    instruction.push(prefix);
                }
                instruction.push(0xFF);
                instruction.extend_from_slice(operand);
                let (mut cpu, mut memory) = setup(&instruction, 0x0100);
                cpu.flags = flags;
                let address = source_base + offset;
                memory[address..address + 4].copy_from_slice(&POINTER);

                // Source prefixes never redirect stack writes away from SS.
                check_round_trip(cpu, memory, instruction.len() as u16);
            }
        }
    }
}

#[test]
fn indirect_far_call_and_retf_wrap_sp_including_unaligned_stack_words() {
    for sp in [0x0000, 0x0001, 0x0002, 0x0003, 0x1000, 0xFFFF] {
        let (mut cpu, mut memory) = setup(&[0x26, 0xFF, 0x1E, 0x01, 0x02], 0x0100);
        cpu.sp = sp;
        memory[0x40201..0x40205].copy_from_slice(&POINTER);

        check_round_trip(cpu, memory, 5);
    }
}

#[test]
fn indirect_far_call_saves_the_wrapped_ip_after_all_instruction_bytes() {
    for ip in [0xFFFB, 0xFFFC, 0xFFFD, 0xFFFE, 0xFFFF] {
        let (cpu, mut memory) = setup(&[0x2E, 0xFF, 0x1E, 0x01, 0x02], ip);
        memory[0x10201..0x10205].copy_from_slice(&POINTER);

        check_round_trip(cpu, memory, 5);
    }
}

#[test]
fn indirect_far_call_reads_a_pointer_across_the_twenty_bit_address_boundary() {
    for (offset, addresses) in [
        (0x000Du16, [0xFFFFD, 0xFFFFE, 0xFFFFF, 0x00000]),
        (0x000E, [0xFFFFE, 0xFFFFF, 0x00000, 0x00001]),
        (0x000F, [0xFFFFF, 0x00000, 0x00001, 0x00002]),
    ] {
        let [low, high] = offset.to_le_bytes();
        let (mut cpu, mut memory) = setup(&[0xFF, 0x1E, low, high], 0x0100);
        cpu.ds = 0xFFFF;
        memory.resize(0x100000, 0xCC);
        for (address, byte) in addresses.into_iter().zip(POINTER) {
            memory[address] = byte;
        }

        check_round_trip(cpu, memory, 4);
    }
}

#[test]
fn indirect_far_call_rejects_register_operands_without_pushing_a_return_address() {
    for reg in 0..8 {
        let (mut cpu, mut memory) = setup(&[0xFF, 0xD8 | reg], 0x0100);
        let original = registers(&cpu);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::UnsupportedRmField(_))
        ));

        assert_eq!(registers(&cpu), original);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 0x0102);
        assert!(memory == original_memory);
    }
}

#[test]
fn indirect_far_call_requires_the_complete_pointer_before_changing_the_stack() {
    for available in 0..4 {
        let mut cpu = Cpu::new(0);
        cpu.ip = 0x0100;
        cpu.sp = 0x0080;
        cpu.flags = 0x0FD7;
        let original = registers(&cpu);
        let mut memory = vec![0xCC; 0x0200 + available];
        memory[0x0100..0x0104].copy_from_slice(&[0xFF, 0x1E, 0x00, 0x02]);
        memory[0x0200..].copy_from_slice(&POINTER[..available]);
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&cpu), original);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(cpu.ip, 0x0104);
        assert!(
            memory == original_memory,
            "incomplete pointer must not cause stack writes"
        );
    }
}
