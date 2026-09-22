use super::{Cpu, InterruptResult, Machine, MachineStepResult};
use crate::machine::disk::{DiskGeometry, DiskType, FloppyDisk};
use crate::machine::{Dma, Fdc, LoadMode, MachineError, Pic, Pit, Ppi, TextVideo};
use crate::memory::MemoryError;
use std::time::Instant;

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

fn floppy_360kib(bytes: Vec<u8>) -> FloppyDisk {
    FloppyDisk {
        bytes,
        geometry: DiskGeometry {
            cylinders: 40,
            heads: 2,
            sectors_per_track: 9,
        },
    }
}

fn disk_snapshot(machine: &Machine) -> Option<(Vec<u8>, [usize; 3])> {
    machine.floppy_image.as_ref().map(|disk| {
        (
            disk.bytes.clone(),
            [
                disk.geometry.cylinders,
                disk.geometry.heads,
                disk.geometry.sectors_per_track,
            ],
        )
    })
}

fn machine_with_image(image: Option<Vec<u8>>) -> Machine {
    let mut cpu = Cpu::new(0);
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
        dma: Dma::new(),
        memory: vec![0xCC; 0x40000],
        handles: vec![],
        fdc: Fdc::new(),
        floppy_image: image.map(floppy_360kib),
        boot_time: Instant::now(),
        last_step_time: Instant::now(),
        tick_remainder: 0,
        verbosity: 0,
        load_mode: LoadMode::Boot,
        pending_input: None,
        keyboard_buffer: Default::default(),
        keyboard_irq_pending: false,
        pic: Pic::new(),
        pit: Pit::new(),
        ppi: Ppi::new(),
        console_read_buffer: Default::default(),
        text_video: TextVideo::new(),
    }
}

#[test]
fn int13_read_uses_physical_contiguity_when_the_buffer_offset_crosses_ffff() {
    // These spellings all name physical 20000h. A 512-byte transfer stays
    // inside one DMA page, even when the supplied BX crosses FFFFh.
    // IBM PC BIOS DMA_SETUP translates ES:BX before programming the DMA:
    // https://github.com/philspil66/IBM-PC-BIOS/blob/main/PCBIOS.ASM#L2631-L2673
    let image = disk_image(1024);
    for (es, bx) in [(0x2000, 0x0000), (0x1001, 0xFFF0), (0x1010, 0xFF00)] {
        let mut machine = machine_with_image(Some(image.clone()));
        machine.cpu.es = es;
        machine.cpu.bx = bx;
        let mut expected_registers = registers(&machine.cpu);
        expected_registers[0] = 0x0001;
        let mut expected_memory = machine.memory.clone();
        expected_memory[0x20000..0x20200].copy_from_slice(&image[512..1024]);

        assert!(matches!(
            machine.handle_interrupt(0x13).unwrap(),
            InterruptResult::ResumeExecution
        ));

        assert_eq!(registers(&machine.cpu), expected_registers);
        assert_eq!(machine.cpu.flags, 0xFFD6);
        if let Some(address) = machine
            .memory
            .iter()
            .zip(&expected_memory)
            .position(|(a, b)| a != b)
        {
            panic!(
                "ES:BX={es:04X}:{bx:04X}: memory at {address:05X} is {:02X}, expected {:02X}; disk data must stay physically contiguous",
                machine.memory[address], expected_memory[address]
            );
        }
        assert_eq!(machine.floppy_image.as_ref().unwrap().bytes, image);
    }
}

fn assert_guest_error(machine: &mut Machine, status: u8, context: &str) {
    let mut expected_registers = registers(&machine.cpu);
    expected_registers[0] = (expected_registers[0] & 0x00FF) | (u16::from(status) << 8);
    let expected_flags = machine.cpu.flags | 1;
    let original_memory = machine.memory.clone();
    let original_image = disk_snapshot(machine);

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
        "{context}: rejected operations must not write RAM"
    );
    assert_eq!(
        disk_snapshot(machine),
        original_image,
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
            assert_eq!(&machine.floppy_image.as_ref().unwrap().bytes, &image);
        }
    }
}

#[test]
fn int13_reads_multiple_sectors_within_a_track() {
    // Explicit offsets keep expectations independent of the CHS helper.
    for (cx, dx, count, offset) in [
        (0x0001, 0x0000, 2u16, 0),
        (0x0002, 0x0000, 2, 512),
        (0x0003, 0x0000, 3, 1024),
        (0x0008, 0x0000, 2, 3584),   // Ends exactly at sector 9.
        (0x0001, 0x0100, 9, 4608),   // Entire track on head 1.
        (0x2708, 0x0100, 2, 367616), // Ends at the end of the disk.
    ] {
        let length = usize::from(count) * 512;
        for extra_bytes in [0, 1] {
            for flags in [0xF002, 0xFFD7] {
                let image = disk_image(offset + length + extra_bytes);
                let mut machine = machine_with_image(Some(image.clone()));
                machine.cpu.ax = 0x0200 | count;
                machine.cpu.cx = cx;
                machine.cpu.dx = dx;
                machine.cpu.flags = flags;
                let mut expected_registers = registers(&machine.cpu);
                expected_registers[0] = count;
                let mut expected_memory = machine.memory.clone();
                expected_memory[0x17E00..0x17E00 + length]
                    .copy_from_slice(&image[offset..offset + length]);

                assert!(matches!(
                    machine.handle_interrupt(0x13).unwrap(),
                    InterruptResult::ResumeExecution
                ));

                assert_eq!(registers(&machine.cpu), expected_registers);
                assert_eq!(machine.cpu.flags, flags & !1);
                assert!(
                    machine.memory == expected_memory,
                    "CX={cx:04X}, DX={dx:04X}, count={count}, extra_bytes={extra_bytes}"
                );
                assert_eq!(&machine.floppy_image.as_ref().unwrap().bytes, &image);
            }
        }
    }
}

#[test]
fn int13_rejects_reads_crossing_a_track_without_copying() {
    // This is our current conservative service policy. Bytes on the next track
    // exist, so the rejection must not depend on reaching the image's end.
    for (sector, count) in [(1, 10), (5, 6), (8, 3), (9, 2)] {
        for head in [0, 1] {
            for flags in [0xF002, 0xFFD7] {
                let mut machine = machine_with_image(Some(disk_image(360 * 1024)));
                machine.cpu.ax = 0x0200 | count;
                machine.cpu.cx = sector;
                machine.cpu.dx = head << 8;
                machine.cpu.flags = flags;
                assert_guest_error(
                    &mut machine,
                    0x01,
                    &format!("head={head}, sector={sector}, count={count}"),
                );
            }
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
        assert_eq!(&machine.floppy_image.as_ref().unwrap().bytes, &image);
    }
}

#[test]
fn int13_rejects_parameters_outside_the_current_supported_subset() {
    // This intentionally documents today's limited service. Expand these
    // expectations when more operations, counts, or drives are supported.
    for (name, ax, cx, dx) in [
        ("writes not supported", 0x0301, 0x0002, 0x0000),
        ("unknown operation", 0xFF01, 0x0002, 0x0000),
        ("zero sectors", 0x0200, 0x0002, 0x0000),
        ("large count", 0x02FF, 0x0002, 0x0000),
        ("second floppy", 0x0201, 0x0002, 0x0001),
        ("hard disk", 0x0201, 0x0002, 0x0080),
    ] {
        for flags in [0xF002, 0xFFD7] {
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
fn int13_reset_succeeds_without_read_parameters_and_preserves_state() {
    for image in [None, Some(disk_image(1024))] {
        for flags in [0xF002, 0xF003, 0xFFD6, 0xFFD7] {
            // Reset ignores AL and CHS, including values invalid for a read.
            for (al, cx, dx) in [(0, 0, 0), (1, 2, 0), (0xFF, 0xFFFF, 0xFF00)] {
                let mut machine = machine_with_image(image.clone());
                machine.cpu.ax = al; // AH=00h: reset.
                machine.cpu.cx = cx;
                machine.cpu.dx = dx;
                machine.cpu.flags = flags;
                // Outside the allocated RAM: reset must not access ES:BX.
                machine.cpu.es = 0x8000;
                machine.cpu.bx = 0x1234;
                let expected_registers = registers(&machine.cpu);
                let original_memory = machine.memory.clone();
                let original_disk = disk_snapshot(&machine);

                assert!(matches!(
                    machine.handle_interrupt(0x13).unwrap(),
                    InterruptResult::ResumeExecution
                ));

                assert_eq!(registers(&machine.cpu), expected_registers);
                assert_eq!(machine.cpu.flags, flags & !1, "only CF is cleared");
                assert!(
                    machine.memory == original_memory,
                    "reset must not write RAM"
                );
                assert_eq!(
                    disk_snapshot(&machine),
                    original_disk,
                    "reset preserves the disk"
                );
            }
        }
    }
}

#[test]
fn int13_reset_rejects_unsupported_drives_without_changing_memory() {
    for image in [None, Some(disk_image(1024))] {
        for flags in [0xF002, 0xFFD7] {
            for drive in [1, 0x80, 0xFF] {
                let mut machine = machine_with_image(image.clone());
                machine.cpu.ax = 0x00A5; // Reset, with an unrelated AL to preserve.
                machine.cpu.dx = drive;
                machine.cpu.flags = flags;
                assert_guest_error(
                    &mut machine,
                    0x01,
                    &format!("reset unsupported drive {drive:02X}"),
                );
            }
        }
    }
}

#[test]
fn int13_single_sided_geometry_rejects_a_second_head_and_track_crossing() {
    for (ax, cx, dx, status) in [
        (0x0201, 0x0001, 0x0100, 0x04), // Head 1 does not exist.
        (0x0201, 0x2801, 0x0000, 0x04), // Cylinder 40 does not exist.
        (0x0201, 0x0000, 0x0000, 0x04), // Sectors start at one.
        (0x0201, 0x000A, 0x0000, 0x04), // Sector 10 does not exist.
        (0x0202, 0x0009, 0x0000, 0x01), // Read crosses the track boundary.
    ] {
        // Extra bytes ensure that geometry, not image length, rejects the read.
        let mut machine = machine_with_image(Some(disk_image(360 * 1024)));
        machine.floppy_image.as_mut().unwrap().geometry.heads = 1;
        machine.cpu.ax = ax;
        machine.cpu.cx = cx;
        machine.cpu.dx = dx;
        assert_guest_error(&mut machine, status, "single-sided geometry");
    }
}

#[test]
fn int13_reads_across_heads_and_cylinders_through_the_last_sector() {
    for (cx, dx, offset) in [
        (0x0003, 0x0000, 1024),
        (0x0009, 0x0000, 4096),
        (0x0001, 0x0100, 4608),
        (0x0009, 0x0100, 8704),
        (0x0101, 0x0000, 9216),
        (0x2709, 0x0100, 368128),
    ] {
        for flags in [0xF002, 0xFFD7] {
            let image = disk_image(360 * 1024);
            let mut machine = machine_with_image(Some(image.clone()));
            machine.cpu.cx = cx;
            machine.cpu.dx = dx;
            machine.cpu.flags = flags;
            let mut expected_registers = registers(&machine.cpu);
            expected_registers[0] = 1;
            let mut expected_memory = machine.memory.clone();
            expected_memory[0x17E00..0x18000].copy_from_slice(&image[offset..offset + 512]);

            assert!(matches!(
                machine.handle_interrupt(0x13).unwrap(),
                InterruptResult::ResumeExecution
            ));

            assert_eq!(registers(&machine.cpu), expected_registers);
            assert_eq!(machine.cpu.flags, flags & !1);
            assert!(
                machine.memory == expected_memory,
                "CX={cx:04X}, DX={dx:04X}"
            );
            assert_eq!(&machine.floppy_image.as_ref().unwrap().bytes, &image);
        }
    }
}

#[test]
fn int13_invalid_chs_returns_sector_not_found_without_copying() {
    for (name, cx, dx) in [
        ("sector numbers start at one", 0x0000, 0x0000),
        ("sector past end of track", 0x000A, 0x0000),
        ("head past end", 0x0001, 0x0200),
        ("cylinder past end", 0x2801, 0x0000),
        ("high cylinder bits exceed floppy geometry", 0x0041, 0x0000),
    ] {
        for flags in [0xF002, 0xFFD7] {
            // Extra image bytes ensure geometry validation, rather than file
            // length alone, rejects addresses outside a 360 KiB floppy.
            let mut machine = machine_with_image(Some(disk_image(400 * 1024)));
            machine.cpu.cx = cx;
            machine.cpu.dx = dx;
            machine.cpu.flags = flags;
            assert_guest_error(&mut machine, 0x04, name);
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
    for (sector, count, image_len) in [
        (1, 1, 0),
        (1, 1, 511),
        (2, 1, 512),
        (2, 1, 513),
        (2, 1, 1023),
        (2, 2, 1024), // First requested sector exists; second is absent.
        (2, 2, 1535), // Second sector is one byte short.
        (1, 9, 4607), // Whole-track read is one byte short.
    ] {
        for flags in [0xF002, 0xFFD7] {
            let mut machine = machine_with_image(Some(disk_image(image_len)));
            machine.cpu.ax = 0x0200 | count;
            machine.cpu.cx = sector;
            machine.cpu.flags = flags;
            assert_guest_error(
                &mut machine,
                0x04,
                &format!("sector={sector}, count={count}, image_len={image_len}"),
            );
        }
    }
}

#[test]
fn int13_validates_the_entire_destination_before_writing_to_undersized_memory() {
    // An undersized host memory allocation is an emulator MemoryError. This
    // is separate from BIOS disk statuses and does not emulate DMA boundaries.
    for (count, memory_len) in [(1, 0x200), (1, 0x201), (1, 0x3FF), (2, 0x400), (2, 0x5FF)] {
        let image = disk_image(1536);
        let mut machine = machine_with_image(Some(image.clone()));
        machine.cpu.ax = 0x0200 | count;
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
        assert_eq!(&machine.floppy_image.as_ref().unwrap().bytes, &image);
    }
}

#[test]
fn int13_success_clears_previous_error_status_and_carry() {
    let mut machine = machine_with_image(Some(disk_image(512)));
    machine.cpu.flags = 0xF002;
    assert_guest_error(&mut machine, 0x04, "sector 2 is missing");

    let image = disk_image(1024);
    machine.floppy_image = Some(floppy_360kib(image.clone()));
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
fn boot_code_reads_a_sector_through_int13_then_executes_it() {
    // Explicit offsets distinguish one-head and two-head layouts at cylinder 1.
    for (image_type, image_len, cx, dx, sector_offset) in [
        (None, 180 * 1024, 0x0002u16, 0x0000u16, 512usize),
        (None, 180 * 1024, 0x0101, 0x0000, 4608),
        (Some(DiskType::Floppy180), 180 * 1024, 0x0101, 0x0000, 4608),
        (Some(DiskType::Floppy360), 360 * 1024, 0x0001, 0x0100, 4608),
        (Some(DiskType::Floppy360), 360 * 1024, 0x0101, 0x0000, 9216),
    ] {
        let mut image = vec![0; image_len];
        // MOV AX,0; MOV ES,AX; MOV BX,7E00h; MOV AX,0201h;
        // MOV CX,cylinder/sector; MOV DX,head/drive; INT 13h; JMP near 7E00h.
        let stage1 = [
            0xB8,
            0x00,
            0x00,
            0x8E,
            0xC0,
            0xBB,
            0x00,
            0x7E,
            0xB8,
            0x01,
            0x02,
            0xB9,
            cx as u8,
            (cx >> 8) as u8,
            0xBA,
            dx as u8,
            (dx >> 8) as u8,
            0xCD,
            0x13,
            0xE9,
            0xEA,
            0x01,
        ];
        image[..stage1.len()].copy_from_slice(&stage1);
        image[510..512].copy_from_slice(&[0x55, 0xAA]);
        image[sector_offset..sector_offset + 5].copy_from_slice(&[0xB8, 0xEF, 0xBE, 0xEB, 0xFE]); // MOV AX,BEEFh; spin.
        let expected_sector = image[sector_offset..sector_offset + 512].to_vec();
        let mut machine = machine_with_image(None);
        machine.cpu.flags = 0xF003; // Ensure the successful read clears stale CF.
        machine.load_boot_sector(image.clone(), image_type).unwrap();
        assert!(
            machine.memory[0x7E00..0x8000]
                .iter()
                .all(|&byte| byte == 0xCC),
            "the boot loader must not preload the requested sector"
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
        assert_eq!(&machine.floppy_image.as_ref().unwrap().bytes, &image);
    }
}
