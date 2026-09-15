use super::{Cpu, CpuError, CpuStepResult};
use crate::memory::{MemoryError, PhysAddr};

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.es, cpu.cs, cpu.ss,
        cpu.ds,
    ]
}

fn cpu_for_memory_access() -> Cpu {
    let mut cpu = Cpu::new();
    cpu.ax = 0xA55A;
    cpu.cx = 0x1234;
    cpu.dx = 0x5678;
    cpu.bx = 0x0300;
    cpu.bp = 0x0800;
    cpu.si = 0x0800;
    cpu.di = 0x0500;
    cpu.sp = 0x9000;
    cpu.es = 0xBEEF;
    cpu.ds = 0x1000;
    cpu.ss = 0x2000;
    cpu.ip = 0x0100;
    cpu.flags = 0x0FD7;
    cpu
}

fn assert_access(mut cpu: Cpu, opcode: u8, operand: &[u8], low: usize, high: usize) {
    let mut memory = vec![0xCC; 0x100000];
    memory[0x0100] = opcode;
    memory[0x0101..0x0101 + operand.len()].copy_from_slice(operand);
    memory[low] = 0x73;
    memory[high] = 0xE2;
    let mut expected_memory = memory.clone();
    let mut expected_registers = registers(&cpu);
    match opcode {
        0x8A => expected_registers[0] = 0xA573, // MOV AL, memory: preserve AH.
        0x88 => expected_memory[low] = 0x5A,    // MOV memory, AL: one byte only.
        0x8E => expected_registers[8] = 0xE273, // MOV ES, memory: little endian.
        0x8C => {
            expected_memory[low] = 0xEF; // MOV memory, ES: little endian.
            expected_memory[high] = 0xBE;
        }
        _ => unreachable!(),
    }

    let result = cpu
        .step(&mut memory)
        .unwrap_or_else(|error| panic!("opcode={opcode:02X}, operand={operand:02X?}: {error}"));

    assert!(matches!(result, CpuStepResult::Continue));
    assert_eq!(
        registers(&cpu),
        expected_registers,
        "only the destination changes"
    );
    assert_eq!(cpu.ip, 0x0101 + operand.len() as u16);
    assert_eq!(cpu.flags, 0x0FD7);
    // Avoid dumping two entire 1 MiB arrays on failure.
    assert!(
        memory == expected_memory,
        "incorrect write location, byte order, or width"
    );
}

fn assert_segment_selection(opcode: u8) {
    // SI and BP contain the same offset; DS and SS differ. Literal physical
    // addresses ensure these assertions do not reuse PhysAddr's calculation.
    for (operand, physical) in [
        (&[0x04][..], 0x10800),             // [SI] -> DS.
        (&[0x01][..], 0x10800),             // [BX+DI] -> DS.
        (&[0x46, 0x00][..], 0x20800),       // [BP+0] -> SS.
        (&[0x02][..], 0x21000),             // [BP+SI] -> SS.
        (&[0x06, 0x00, 0x08][..], 0x10800), // Direct [0800h] -> DS, not SS.
    ] {
        assert_access(
            cpu_for_memory_access(),
            opcode,
            operand,
            physical,
            physical + 1,
        );
    }
}

fn assert_bus_boundary(opcode: u8) {
    for (offset, low, high) in [
        (0x000E, 0xFFFFE, 0xFFFFF),
        (0x000F, 0xFFFFF, 0x00000), // Word's second byte crosses the bus boundary.
        (0x0010, 0x00000, 0x00001), // Segment + offset itself wraps to zero.
    ] {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0xFFFF;
        cpu.si = offset;
        assert_access(cpu, opcode, &[0x04], low, high);
    }
}

fn assert_bounds_error(opcode: u8) {
    let offsets: &[u16] = if matches!(opcode, 0x8C | 0x8E) {
        &[0x0100, 0x00FF] // Entire word missing, then only its high byte missing.
    } else {
        &[0x0100]
    };
    for &offset in offsets {
        let mut cpu = cpu_for_memory_access();
        cpu.ds = 0x0010;
        cpu.si = offset;
        let original_registers = registers(&cpu);
        // DS:0100 -> physical 0200, just outside the host buffer.
        // DS:00FF -> physical 01FF, only one byte remains.
        let mut memory = vec![0xCC; 0x0200];
        memory[0x0100..0x0102].copy_from_slice(&[opcode, 0x04]);
        let original_memory = memory.clone();

        let result = cpu.step(&mut memory);

        assert!(
            matches!(
                result,
                Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
            ),
            "opcode={opcode:02X}, offset={offset:04X}: bounds must use the physical address"
        );
        assert_eq!(registers(&cpu), original_registers);
        assert_eq!(cpu.flags, 0x0FD7);
        assert_eq!(
            memory, original_memory,
            "failed writes must not partially modify memory"
        );
    }
}

// Separate reads/writes and boundary cases so one unfinished operation does
// not hide results for the others. Each test executes a bounded CPU step.
macro_rules! access_tests {
    ($name:ident, $opcode:expr) => {
        mod $name {
            #[test]
            fn selects_ds_or_ss_and_preserves_unrelated_state() {
                super::assert_segment_selection($opcode);
            }

            #[test]
            fn handles_the_20_bit_bus_boundary() {
                super::assert_bus_boundary($opcode);
            }

            #[test]
            fn bounds_errors_preserve_registers_and_memory() {
                super::assert_bounds_error($opcode);
            }
        }
    };
}

access_tests!(byte_read, 0x8A);
access_tests!(byte_write, 0x88);
access_tests!(word_read, 0x8E);
access_tests!(word_write, 0x8C);

#[test]
fn physical_address_translation_widens_wraps_and_allows_aliases() {
    for (segment, offset, expected) in [
        (0x0000, 0x1234, 0x01234),
        (0x1234, 0x0056, 0x12396),
        (0x1230, 0x0096, 0x12396), // Different segment:offset, same physical byte.
        (0xFFFF, 0x000F, 0xFFFFF),
        (0xFFFF, 0x0010, 0x00000),
        (0xFFFF, 0xFFFF, 0x0FFEF),
    ] {
        assert_eq!(PhysAddr::new(segment, offset).as_usize(), expected);
    }
}
