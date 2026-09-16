use super::{Cpu, InterruptResult, Machine, MachineStepResult};
use crate::machine::MachineError;
use crate::memory::MemoryError;

fn registers(cpu: &Cpu) -> [u16; 13] {
    [
        cpu.ax, cpu.bx, cpu.cx, cpu.dx, cpu.si, cpu.di, cpu.bp, cpu.sp, cpu.cs, cpu.ds, cpu.es,
        cpu.ss, cpu.ip,
    ]
}

fn disk_image(size: usize) -> Vec<u8> {
    // Distinct sectors and changing bytes expose wrong offsets and short reads.
    (0..size)
        .map(|i| ((i % 512) * 37 + (i / 512) * 91 + 17) as u8)
        .collect()
}

fn machine_with_image(image: Option<Vec<u8>>) -> Machine {
    let mut cpu = Cpu::new();
    cpu.ax = 0x0201; // Read one sector.
    cpu.cx = 0x0002; // Cylinder 0, sector 2.
    cpu.dx = 0x0000; // Head 0, drive 0.
    cpu.es = 0x1000;
    cpu.bx = 0x7E00;
    cpu.cs = 0x2000;
    cpu.ip = 0x0100;
    cpu.ds = 0x4000; // Distinct from ES: using DS:BX would be wrong.
    cpu.ss = 0x3000;
    cpu.sp = 0x0800;
    cpu.si = 0x1234;
    cpu.di = 0x5678;
    cpu.bp = 0x9ABC;
    cpu.flags = 0xFFD7;
    Machine {
        cpu,
        memory: vec![0xCC; 0x40000],
        handles: vec![],
        floppy_image: image,
    }
}

fn assert_guest_error(machine: &mut Machine, status: u8, context: &str) {
    let mut expected_registers = registers(&machine.cpu);
    expected_registers[0] = (expected_registers[0] & 0x00FF) | (u16::from(status) << 8);
    let expected_flags = machine.cpu.flags | 1;
    let original_memory = machine.memory.clone();
    let original_image = machine.floppy_image.clone();

    let result = machine.handle_interrupt(0x13).expect(context);

    assert!(
        matches!(result, InterruptResult::ResumeExecution),
        "{context}"
    );
    assert_eq!(
        registers(&machine.cpu),
        expected_registers,
        "{context}: only AH changes"
    );
    assert_eq!(
        machine.cpu.flags, expected_flags,
        "{context}: only CF changes"
    );
    assert!(
        machine.memory == original_memory,
        "{context}: rejected reads must not write RAM"
    );
    assert_eq!(
        machine.floppy_image, original_image,
        "{context}: disk contents are unchanged"
    );
}

#[test]
fn int13_reads_exactly_one_sector_and_sets_success_status() {
    // Exact-size images catch an accidental 513-byte source slice. Extra data
    // catches copying beyond a sector when that slice happens to be in bounds.
    for (sector, image_len) in [(1u16, 512), (1, 1024), (2, 1024), (2, 1025)] {
        for initial_flags in [0xF002, 0xFFD7] {
            let image = disk_image(image_len);
            let offset = usize::from(sector - 1) * 512;
            let expected_sector = image[offset..offset + 512].to_vec();
            let mut machine = machine_with_image(Some(image.clone()));
            machine.cpu.cx = sector;
            machine.cpu.flags = initial_flags;
            let mut expected_registers = registers(&machine.cpu);
            expected_registers[0] = 0x0001; // AH=success, AL still one sector.
            let mut expected_memory = machine.memory.clone();
            expected_memory[0x17E00..0x18000].copy_from_slice(&expected_sector);

            assert!(matches!(
                machine.handle_interrupt(0x13).unwrap(),
                InterruptResult::ResumeExecution
            ));

            assert_eq!(registers(&machine.cpu), expected_registers);
            assert_eq!(machine.cpu.flags, initial_flags & !1);
            assert!(
                machine.memory == expected_memory,
                "sector={sector}, image_len={image_len}: exactly 512 destination bytes change"
            );
            assert_eq!(machine.floppy_image.as_ref().unwrap(), &image);
        }
    }
}

#[test]
fn int13_uses_es_bx_including_nonzero_and_aliasing_segments() {
    for (es, bx, physical, memory_len) in [
        (0x0000, 0x7E00, 0x07E00, 0x40000),
        (0x1000, 0x2345, 0x12345, 0x40000),
        (0x1234, 0x0005, 0x12345, 0x40000), // Same physical destination.
        (0x0000, 0x0200, 0x00200, 0x00400), // Exactly enough destination RAM.
    ] {
        let image = disk_image(1024);
        let mut machine = machine_with_image(Some(image.clone()));
        machine.cpu.es = es;
        machine.cpu.bx = bx;
        machine.memory.resize(memory_len, 0xCC);
        let mut expected_registers = registers(&machine.cpu);
        expected_registers[0] = 1;
        let mut expected_memory = machine.memory.clone();
        expected_memory[physical..physical + 512].copy_from_slice(&image[512..1024]);

        assert!(matches!(
            machine.handle_interrupt(0x13).unwrap(),
            InterruptResult::ResumeExecution
        ));

        assert_eq!(registers(&machine.cpu), expected_registers);
        assert_eq!(machine.cpu.flags, 0xFFD6);
        assert!(
            machine.memory == expected_memory,
            "destination={es:04X}:{bx:04X}"
        );
        assert_eq!(machine.floppy_image.as_ref().unwrap(), &image);
    }
}

#[test]
fn int13_rejects_parameters_outside_the_current_supported_subset() {
    // This intentionally documents today's limited service. Expand these
    // expectations when more operations, counts, or disk geometry are supported.
    for (name, ax, cx, dx) in [
        ("reset not yet supported", 0x0001, 0x0002, 0x0000),
        ("writes not supported", 0x0301, 0x0002, 0x0000),
        ("unknown operation", 0xFF01, 0x0002, 0x0000),
        ("zero sectors", 0x0200, 0x0002, 0x0000),
        ("multiple sectors", 0x0202, 0x0002, 0x0000),
        ("large count", 0x02FF, 0x0002, 0x0000),
        ("second floppy", 0x0201, 0x0002, 0x0001),
        ("hard disk", 0x0201, 0x0002, 0x0080),
        ("nonzero cylinder", 0x0201, 0x0102, 0x0000),
        ("nonzero head", 0x0201, 0x0002, 0x0100),
        ("sector numbers start at one", 0x0201, 0x0000, 0x0000),
        ("third sector not yet supported", 0x0201, 0x0003, 0x0000),
        ("high cylinder bits not supported", 0x0201, 0x0041, 0x0000),
    ] {
        for flags in [0xF002, 0xFFD7] {
            // Even sector 3 exists in this image; rejection is about the
            // explicitly supported parameters, not merely file length.
            let mut machine = machine_with_image(Some(disk_image(1536)));
            machine.cpu.ax = ax;
            machine.cpu.cx = cx;
            machine.cpu.dx = dx;
            machine.cpu.flags = flags;
            assert_guest_error(&mut machine, 0x01, name);
        }
    }
}

#[test]
fn int13_without_an_inserted_floppy_returns_not_ready() {
    for sector in [1, 2] {
        for flags in [0xF002, 0xFFD7] {
            let mut machine = machine_with_image(None);
            machine.cpu.cx = sector;
            machine.cpu.flags = flags;
            assert_guest_error(&mut machine, 0x80, "no floppy image");
        }
    }
}

#[test]
fn int13_missing_or_partial_sector_returns_sector_not_found_without_copying() {
    for (sector, image_len) in [(1, 0), (1, 511), (2, 512), (2, 513), (2, 1023)] {
        for flags in [0xF002, 0xFFD7] {
            let mut machine = machine_with_image(Some(disk_image(image_len)));
            machine.cpu.cx = sector;
            machine.cpu.flags = flags;
            assert_guest_error(
                &mut machine,
                0x04,
                &format!("sector={sector}, image_len={image_len}"),
            );
        }
    }
}

#[test]
fn int13_validates_the_entire_destination_before_writing_to_undersized_memory() {
    // An undersized host memory allocation is an emulator MemoryError. This
    // is separate from BIOS disk statuses and does not emulate DMA boundaries.
    for memory_len in [0x200, 0x201, 0x3FF] {
        let image = disk_image(1024);
        let mut machine = machine_with_image(Some(image.clone()));
        machine.cpu.es = 0;
        machine.cpu.bx = 0x0200;
        machine.memory.resize(memory_len, 0xCC);
        let original_registers = registers(&machine.cpu);
        let original_memory = machine.memory.clone();

        assert!(matches!(
            machine.handle_interrupt(0x13),
            Err(MachineError::MemoryError(MemoryError::OutOfBoundsAccess(_)))
        ));

        assert_eq!(registers(&machine.cpu), original_registers);
        assert_eq!(machine.cpu.flags, 0xFFD7);
        assert!(machine.memory == original_memory);
        assert_eq!(machine.floppy_image.as_ref().unwrap(), &image);
    }
}

#[test]
fn int13_success_clears_previous_error_status_and_carry() {
    let mut machine = machine_with_image(Some(disk_image(512)));
    machine.cpu.flags = 0xF002;
    assert_guest_error(&mut machine, 0x04, "sector 2 is missing");

    let image = disk_image(1024);
    machine.floppy_image = Some(image.clone());
    machine.cpu.ax = 0x0201; // Retry the request, preserving the previous CF.
    assert!(matches!(
        machine.handle_interrupt(0x13).unwrap(),
        InterruptResult::ResumeExecution
    ));
    assert_eq!(machine.cpu.ax, 1);
    assert_eq!(machine.cpu.flags, 0xF002);
    assert_eq!(&machine.memory[0x17E00..0x18000], &image[512..1024]);

    // A later failure must set carry again after success cleared it.
    machine.cpu.ax = 0x0201;
    machine.floppy_image = None;
    assert_guest_error(&mut machine, 0x80, "disk removed after successful read");
}

#[test]
fn boot_code_reads_sector_two_through_int13_then_executes_it() {
    let mut image = vec![0; 1024];
    // MOV AX,0; MOV ES,AX; MOV BX,7E00h; MOV AX,0201h;
    // MOV CX,2; MOV DX,0; INT 13h; JMP near 7E00h.
    let stage1 = [
        0xB8, 0x00, 0x00, 0x8E, 0xC0, 0xBB, 0x00, 0x7E, 0xB8, 0x01, 0x02, 0xB9, 0x02, 0x00, 0xBA,
        0x00, 0x00, 0xCD, 0x13, 0xE9, 0xEA, 0x01,
    ];
    image[..stage1.len()].copy_from_slice(&stage1);
    image[510..512].copy_from_slice(&[0x55, 0xAA]);
    image[512..517].copy_from_slice(&[0xB8, 0xEF, 0xBE, 0xEB, 0xFE]); // MOV AX,BEEFh; spin.
    let expected_sector = image[512..].to_vec();
    let mut machine = machine_with_image(None);
    machine.cpu.flags = 0xF003; // Ensure the successful read clears stale CF.
    machine.load_boot_sector(image.clone()).unwrap();
    assert!(
        machine.memory[0x7E00..0x8000]
            .iter()
            .all(|&byte| byte == 0xCC),
        "the boot loader must not preload sector 2"
    );

    // Fixed instruction counts keep the test bounded even if guest jumps break.
    for _ in 0..7 {
        assert!(matches!(
            machine.step().unwrap(),
            MachineStepResult::Continue
        ));
    }
    assert_eq!((machine.cpu.cs, machine.cpu.ip), (0, 0x7C13));
    assert_eq!(machine.cpu.ax, 1);
    assert_eq!(machine.cpu.flags, 0xF002);
    assert_eq!(&machine.memory[0x7E00..0x8000], &expected_sector);
    assert_eq!(&machine.memory[0x7C00..0x7E00], &image[..512]);
    assert_eq!(
        machine.cpu.sp, 0x0800,
        "hosted BIOS call does not enter a guest handler"
    );

    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    ));
    assert_eq!((machine.cpu.cs, machine.cpu.ip), (0, 0x7E00));
    assert!(matches!(
        machine.step().unwrap(),
        MachineStepResult::Continue
    ));
    assert_eq!(machine.cpu.ax, 0xBEEF);
    assert_eq!(machine.cpu.ip, 0x7E03);
    assert_eq!(machine.floppy_image.as_ref().unwrap(), &image);
}
