use super::{Cpu, CpuError, CpuModel, CpuStepResult};
use crate::memory::MemoryError;

// LEAVE: SP <- BP, then BP <- Pop(); FLAGS are unchanged.
// https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-software-developer-vol-2a-manual.pdf
// On 8088, C9 instead aliases CB (RETF: pop IP, then CS).
// https://www.righto.com/2023/07/undocumented-8086-instructions.html

fn registers(cpu: &Cpu) -> [u16; 12] {
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di, cpu.cs, cpu.ds, cpu.es,
        cpu.ss,
    ]
}

fn setup(model: CpuModel) -> (Cpu, Vec<u8>) {
    let mut cpu = Cpu::new(model, 0);
    [
        cpu.ax, cpu.cx, cpu.dx, cpu.bx, cpu.sp, cpu.bp, cpu.si, cpu.di,
    ] = [
        0x1234, 0x2345, 0x3456, 0x4567, 0x0800, 0x0FF0, 0x6789, 0x789A,
    ];
    [cpu.cs, cpu.ds, cpu.es, cpu.ss] = [0x1000, 0x2000, 0x4000, 0x3000];
    cpu.ip = 0x0100;
    (cpu, vec![0xCC; 0x50000])
}

fn step(cpu: &mut Cpu, memory: &mut [u8]) {
    assert!(matches!(cpu.step(memory).unwrap(), CpuStepResult::Continue));
}

#[test]
fn i286_leave_restores_bp_from_ss_and_discards_locals_preserving_flags() {
    // Both bytes of each saved BP fit within the segment. The final pop may
    // wrap SP, but this does not assert 286 accesses across a segment limit.
    for (bp, final_sp) in [
        (0x0FF0, 0x0FF2),
        (0x0000, 0x0002),
        (0xFFFE, 0x0000),
        (0x0123, 0x0125),
    ] {
        for old_sp in [0x0800, 0xFFFF] {
            for saved_bp in [0x0000u16, 0x1234, 0xFFFF] {
                for flags in [0x0002, 0x0FD7] {
                    // Segment overrides must not redirect this implicit stack read.
                    for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
                        let (mut cpu, mut memory) = setup(CpuModel::I80286);
                        cpu.bp = bp;
                        cpu.sp = old_sp;
                        cpu.flags = flags;
                        let instruction: Vec<u8> = prefix.into_iter().chain([0xC9]).collect();
                        memory[0x10100..0x10100 + instruction.len()].copy_from_slice(&instruction);
                        let address = 0x30000 + usize::from(bp);
                        memory[address..address + 2].copy_from_slice(&saved_bp.to_le_bytes());
                        let original_memory = memory.clone();
                        let mut expected = registers(&cpu);
                        expected[4] = final_sp;
                        expected[5] = saved_bp;

                        step(&mut cpu, &mut memory);

                        assert_eq!(
                            registers(&cpu),
                            expected,
                            "BP={bp:04X}, old SP={old_sp:04X}, prefix={prefix:?}"
                        );
                        assert_eq!(cpu.ip, 0x0100 + instruction.len() as u16);
                        assert_eq!(cpu.flags, flags, "LEAVE preserves every flag");
                        assert!(
                            memory == original_memory,
                            "LEAVE leaves stack contents intact"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn i286_leave_does_not_read_the_old_sp() {
    let mut cpu = Cpu::new(CpuModel::I80286, 0);
    cpu.sp = 0xF000; // This address does not exist in the supplied RAM.
    cpu.bp = 0x0100;
    cpu.flags = 0x0FD7;
    let mut memory = vec![0xCC; 0x0102];
    memory[0] = 0xC9;
    memory[0x0100..].copy_from_slice(&[0x34, 0x12]);
    let original_memory = memory.clone();

    step(&mut cpu, &mut memory);

    assert_eq!((cpu.bp, cpu.sp, cpu.ip), (0x1234, 0x0102, 1));
    assert_eq!(cpu.flags, 0x0FD7);
    assert!(memory == original_memory);
}

#[test]
fn i286_leave_reports_a_truncated_saved_bp_as_a_memory_error() {
    for length in [0x0100, 0x0101] {
        let mut cpu = Cpu::new(CpuModel::I80286, 0);
        cpu.bp = 0x0100;
        cpu.sp = 0x0080;
        cpu.flags = 0x0FD7;
        let mut memory = vec![0xCC; length];
        memory[0] = 0xC9;
        let original_memory = memory.clone();

        assert!(matches!(
            cpu.step(&mut memory),
            Err(CpuError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(cpu.bp, 0x0100, "a failed read must not replace BP");
        assert_eq!(cpu.flags, 0x0FD7);
        assert!(memory == original_memory);
        // Host RAM bounds errors do not model architectural fault rollback.
    }
}

#[test]
fn i286_leave_then_ret_unwinds_a_guest_stack_frame() {
    let (mut cpu, mut memory) = setup(CpuModel::I80286);
    cpu.sp = 0x1000;
    cpu.bp = 0x5678;
    // Pre-existing near-return address. Build a frame without relying on ENTER.
    memory[0x31000..0x31002].copy_from_slice(&[0x79, 0x24]);
    memory[0x10100..0x10108].copy_from_slice(&[
        0x55, // PUSH BP.
        0x89, 0xE5, // MOV BP,SP.
        0x83, 0xEC, 0x10, // SUB SP,16: reserve local variables.
        0xC9, // LEAVE.
        0xC3, // RET.
    ]);
    for _ in 0..3 {
        step(&mut cpu, &mut memory);
    }
    assert_eq!((cpu.bp, cpu.sp, cpu.ip), (0x0FFE, 0x0FEE, 0x0106));
    let flags = cpu.flags;
    let original_memory = memory.clone();
    let mut expected = registers(&cpu);
    expected[4] = 0x1000;
    expected[5] = 0x5678;

    step(&mut cpu, &mut memory);

    assert_eq!(registers(&cpu), expected);
    assert_eq!(cpu.ip, 0x0107, "LEAVE itself does not return");
    assert_eq!(cpu.flags, flags);

    step(&mut cpu, &mut memory);

    expected[4] = 0x1002;
    assert_eq!(registers(&cpu), expected);
    assert_eq!(cpu.ip, 0x2479);
    assert_eq!(cpu.flags, flags);
    assert!(memory == original_memory);
}

#[test]
fn i8088_c9_alias_pops_ip_then_cs_without_changing_bp() {
    check_far_return(false, 0xC9);
}

#[test]
fn i8088_cb_remains_an_ordinary_far_return() {
    check_far_return(false, 0xCB);
}

#[test]
fn i286_cb_remains_an_ordinary_far_return() {
    check_far_return(true, 0xCB);
}

fn check_far_return(is_286: bool, opcode: u8) {
    let stacks: &[(u16, u16)] = if is_286 {
        &[(0x1000, 0x1004), (0x0000, 0x0004), (0xFFFC, 0x0000)]
    } else {
        &[
            (0x1000, 0x1004),
            (0x0000, 0x0004),
            (0xFFFC, 0x0000),
            (0xFFFE, 0x0002),
            (0xFFFF, 0x0003),
        ]
    };
    let ips: &[u16] = if is_286 {
        &[0x0100]
    } else {
        &[0x0100, 0xFFFE, 0xFFFF]
    };
    for &(sp, final_sp) in stacks {
        for &ip in ips {
            for pattern in [0x0000, 0x0FD5] {
                for prefix in [None, Some(0x26), Some(0x2E), Some(0x36), Some(0x3E)] {
                    let (mut cpu, mut memory) = setup(if is_286 {
                        CpuModel::I80286
                    } else {
                        CpuModel::I8088
                    });
                    cpu.ip = ip;
                    cpu.sp = sp;
                    cpu.flags = (if is_286 { 0x0002 } else { 0xF002 }) | pattern;
                    let flags = cpu.flags;
                    for (index, byte) in prefix.into_iter().chain([opcode]).enumerate() {
                        memory[0x10000 + usize::from(ip.wrapping_add(index as u16))] = byte;
                    }
                    // RETF reads SS:SP, not SS:BP; BP still points at a decoy.
                    for (index, byte) in [0x79, 0x24, 0x67, 0x45].into_iter().enumerate() {
                        memory[0x30000 + usize::from(sp.wrapping_add(index as u16))] = byte;
                    }
                    let original_memory = memory.clone();
                    let mut expected = registers(&cpu);
                    expected[4] = final_sp;
                    expected[8] = 0x4567;

                    step(&mut cpu, &mut memory);

                    assert_eq!(
                        registers(&cpu),
                        expected,
                        "opcode={opcode:02X}, SP={sp:04X}, IP={ip:04X}, prefix={prefix:?}"
                    );
                    assert_eq!(cpu.ip, 0x2479, "first word is IP, second is CS");
                    assert_eq!(cpu.flags, flags, "RETF preserves FLAGS");
                    assert!(memory == original_memory, "RETF leaves its frame intact");
                }
            }
        }
    }
}

#[test]
fn c9_consumes_only_one_byte_on_both_models() {
    for is_286 in [false, true] {
        let mut cpu = Cpu::new(
            if is_286 {
                CpuModel::I80286
            } else {
                CpuModel::I8088
            },
            0,
        );
        cpu.ip = 0x3FFF;
        cpu.sp = 0x0100;
        cpu.bp = 0x0200;
        let flags = cpu.flags;
        let mut memory = vec![0xCC; 0x4000];
        memory[0x3FFF] = 0xC9; // No ModR/M or immediate byte exists after this.
        memory[0x0100..0x0104].copy_from_slice(&[0x42, 0x00, 0x34, 0x12]);
        memory[0x0200..0x0202].copy_from_slice(&[0x78, 0x56]);
        let original_memory = memory.clone();

        step(&mut cpu, &mut memory);

        assert_eq!(
            (cpu.cs, cpu.ip, cpu.sp, cpu.bp),
            if is_286 {
                (0, 0x4000, 0x0202, 0x5678)
            } else {
                (0x1234, 0x0042, 0x0104, 0x0200)
            }
        );
        assert_eq!(cpu.flags, flags);
        assert!(memory == original_memory);
    }
}
