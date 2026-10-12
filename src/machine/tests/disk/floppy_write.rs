//! FDC/DMA writes through Machine::handle_floppy_operation and the port interface.
//!
//! Covers the current 512-byte-sector DMA subset. Unsupported transfers are
//! rejected with MachineError; these tests do not establish full FDC timing,
//! formatting, partial-sector transfers, or BIOS INT 13h write support.
//! NEC uPD765A/B: Write Data and Table 6 (result C/H/R/N after terminal count).
//! https://cpctech.cpcwiki.de/docs/upd765a/necfdc.htm

use super::{Machine, disk_image, disk_snapshot, floppy_360kib, machine_with_image};
use crate::machine::disk::DiskGeometry;
use crate::machine::fdc::FdcDataRequest;

const SECTOR_BYTES: usize = 512;
const SOURCE: usize = 0x12000;

fn machine_with_two_disks() -> Machine {
    let image = disk_image(360 * 1024);
    let other = image.iter().map(|byte| byte ^ 0xFF).collect();
    let mut machine = machine_with_image(Some(image));
    machine.floppy_images[1] = Some(floppy_360kib(other));
    machine
}

fn setup_dma(machine: &mut Machine, address: usize, byte_count: usize) {
    let count = u16::try_from(byte_count - 1).unwrap();
    machine.dma.write(0x0C, 0); // Reset the shared low/high-byte flip-flop.
    machine.dma.write(0x81, (address >> 16) as u8);
    for byte in (address as u16).to_le_bytes() {
        machine.dma.write(0x04, byte);
    }
    for byte in count.to_le_bytes() {
        machine.dma.write(0x05, byte);
    }
    machine.dma.write(0x0B, 0x4A); // Channel 2: memory to device, increment address.
    machine.dma.write(0x0A, 0x02); // Unmask channel 2.
}

fn data_request(
    machine: &mut Machine,
    command: u8,
    drive: u8,
    [cylinder, head, sector]: [u8; 3],
    eot: u8,
) -> FdcDataRequest {
    // Use the command parser instead of constructing its private parser state.
    machine.fdc.write(0x3F2, 0x08); // Enable IRQ without generating a reset event.
    for byte in [
        command,
        (head << 2) | drive,
        cylinder,
        head,
        sector,
        2, // 512-byte sectors.
        eot,
        0x2A,
        0xFF,
    ] {
        machine.fdc.write(0x3F5, byte);
    }
    machine.fdc.take_data_request().unwrap()
}

fn completion_results(machine: &mut Machine) -> [u8; 7] {
    assert!(machine.fdc.irq_output(), "completion must raise IRQ");
    let results = std::array::from_fn(|_| {
        assert_eq!(machine.fdc.read(0x3F4), 0xD0);
        machine.fdc.read(0x3F5)
    });
    assert_eq!(machine.fdc.read(0x3F4), 0x80, "exactly seven result bytes");
    assert!(
        !machine.fdc.irq_output(),
        "reading results acknowledges IRQ"
    );
    results
}

fn expect_dma_finished(machine: &Machine, source: usize, byte_count: usize) {
    assert_eq!(
        machine.dma.physical_address() as usize,
        (source & 0xF0000) | ((source + byte_count) & 0xFFFF),
        "DMA advances the offset without incrementing its page"
    );
    assert_eq!(
        machine.dma.count_bytes(),
        0x10000,
        "count underflows to FFFF"
    );
    assert!(
        machine.dma.is_masked(),
        "completed transfer masks channel 2"
    );
}

fn expect_write(
    drive: u8,
    multi_track: bool,
    start: [u8; 3],
    eot: u8,
    destination_sectors: &[usize],
    result_chrn: [u8; 4],
    source: usize,
) -> Machine {
    let mut machine = machine_with_two_disks();
    let bytes: Vec<u8> = disk_image(destination_sectors.len() * SECTOR_BYTES)
        .into_iter()
        .map(|byte| byte ^ 0x5A)
        .collect();
    machine.memory[source..source + bytes.len()].copy_from_slice(&bytes);
    let original_memory = machine.memory.clone();
    let mut expected_disks = disk_snapshot(&machine);
    for (index, &sector) in destination_sectors.iter().enumerate() {
        expected_disks[drive as usize].as_mut().unwrap().0
            [sector * SECTOR_BYTES..(sector + 1) * SECTOR_BYTES]
            .copy_from_slice(&bytes[index * SECTOR_BYTES..(index + 1) * SECTOR_BYTES]);
    }
    setup_dma(&mut machine, source, bytes.len());
    let command = if multi_track { 0xC5 } else { 0x45 }; // MFM Write Data.
    let request = data_request(&mut machine, command, drive, start, eot);

    machine
        .handle_floppy_operation(request)
        .expect("valid floppy write must succeed");

    assert!(
        machine.memory == original_memory,
        "write must preserve guest RAM"
    );
    assert!(
        disk_snapshot(&machine) == expected_disks,
        "copy distinct source chunks into exactly the selected sectors and drive"
    );
    expect_dma_finished(&machine, source, bytes.len());
    let results = completion_results(&mut machine);
    assert_eq!(
        results[0] & !0x04,
        drive,
        "normal ST0 status and selected unit"
    );
    assert_eq!(&results[1..3], &[0, 0], "ST1/ST2 report success");
    assert_eq!(&results[3..], &result_chrn);
    machine
}

fn expect_rejected_write(machine: &mut Machine, request: FdcDataRequest) {
    let disks = disk_snapshot(machine);
    let memory = machine.memory.clone();
    let address = machine.dma.physical_address();
    let count = machine.dma.count_bytes();
    let masked = machine.dma.is_masked();

    assert!(machine.handle_floppy_operation(request).is_err());

    assert!(
        disk_snapshot(machine) == disks,
        "rejected write preserves both disks"
    );
    assert!(
        machine.memory == memory,
        "rejected write preserves guest RAM"
    );
    assert_eq!(machine.dma.physical_address(), address);
    assert_eq!(machine.dma.count_bytes(), count);
    assert_eq!(machine.dma.is_masked(), masked);
}

#[test]
fn single_sector_write_changes_only_the_selected_drive_and_sector() {
    for drive in [0, 1] {
        expect_write(drive, false, [5, 1, 4], 9, &[102], [5, 1, 5, 2], SOURCE);
    }
}

#[test]
fn multi_sector_writes_use_successive_source_chunks_across_tracks() {
    expect_write(1, false, [5, 0, 4], 9, &[93, 94, 95], [5, 0, 7, 2], SOURCE);
    expect_write(1, true, [5, 0, 8], 9, &[97, 98, 99], [5, 1, 2, 2], SOURCE);
    // EOT=5 switches heads early; physical sectors 6..9 must stay unchanged.
    expect_write(1, true, [5, 0, 4], 5, &[93, 94, 99], [5, 1, 2, 2], SOURCE);
    let sectors: Vec<usize> = (90..108).collect();
    expect_write(1, true, [5, 0, 1], 9, &sectors, [6, 0, 1, 2], SOURCE);
}

#[test]
fn result_coordinates_follow_terminal_count_and_eot_on_either_head() {
    for (mt, start, eot, destination, result) in [
        (false, [5, 0, 4], 9, 93, [5, 0, 5, 2]),
        (false, [5, 1, 4], 9, 102, [5, 1, 5, 2]),
        (false, [5, 0, 9], 9, 98, [6, 0, 1, 2]),
        (false, [5, 1, 9], 9, 107, [6, 1, 1, 2]),
        (true, [5, 0, 9], 9, 98, [5, 1, 1, 2]),
        (true, [5, 1, 9], 9, 107, [6, 0, 1, 2]),
        (false, [5, 0, 5], 5, 94, [6, 0, 1, 2]),
        (true, [5, 0, 5], 5, 94, [5, 1, 1, 2]),
    ] {
        expect_write(1, mt, start, eot, &[destination], result, SOURCE);
    }
}

#[test]
fn starting_above_eot_writes_consecutive_sectors_until_dma_finishes() {
    for multi_track in [false, true] {
        expect_write(
            1,
            multi_track,
            [5, 0, 6],
            5,
            &[95, 96],
            [5, 0, 8, 2],
            SOURCE,
        );
    }
}

#[test]
fn write_can_end_exactly_at_a_dma_page_boundary() {
    expect_write(1, false, [5, 0, 4], 9, &[93], [5, 0, 5, 2], 0x1FE00);
}

#[test]
fn written_sectors_can_be_read_back_through_the_same_handler() {
    let mut machine = expect_write(1, true, [5, 0, 4], 5, &[93, 94, 99], [5, 1, 2, 2], SOURCE);
    let byte_count = 3 * SECTOR_BYTES;
    let bytes = machine.memory[SOURCE..SOURCE + byte_count].to_vec();
    let disks = disk_snapshot(&machine);
    let destination = 0x14000;
    let mut expected_memory = machine.memory.clone();
    expected_memory[destination..destination + byte_count].copy_from_slice(&bytes);
    setup_dma(&mut machine, destination, byte_count);
    machine.dma.write(0x0B, 0x46); // Read direction.
    let request = data_request(&mut machine, 0xC6, 1, [5, 0, 4], 5);

    machine.handle_floppy_operation(request).unwrap();

    assert!(
        machine.memory == expected_memory,
        "read back the exact written bytes"
    );
    assert!(
        disk_snapshot(&machine) == disks,
        "reading preserves both images"
    );
    expect_dma_finished(&machine, destination, byte_count);
    assert_eq!(&completion_results(&mut machine)[3..], &[5, 1, 2, 2]);
}

#[test]
fn truncated_first_or_later_sector_is_rejected_before_any_write() {
    for (image_length, sector_count) in [(511, 1), (512, 2), (1023, 2)] {
        let mut machine = machine_with_two_disks();
        machine.floppy_images[1]
            .as_mut()
            .unwrap()
            .bytes
            .truncate(image_length);
        setup_dma(&mut machine, SOURCE, sector_count * SECTOR_BYTES);
        let request = data_request(&mut machine, 0x45, 1, [0, 0, 1], 9);
        expect_rejected_write(&mut machine, request);
    }
}

#[test]
fn invalid_geometry_or_track_progression_is_rejected_before_any_write() {
    for (command, start, sector_count) in [
        (0x45, [0, 0, 0], 1),
        (0x45, [0, 0, 10], 1),
        (0x45, [40, 0, 1], 1),
        (0x45, [0, 0, 9], 2), // Requires going past EOT without MT.
        (0xC5, [0, 1, 9], 2), // Cannot continue beyond the last head.
    ] {
        let mut machine = machine_with_two_disks();
        setup_dma(&mut machine, SOURCE, sector_count * SECTOR_BYTES);
        let request = data_request(&mut machine, command, 1, start, 9);
        expect_rejected_write(&mut machine, request);
    }
    // A valid first sector followed by an absent second head must not be written.
    let mut machine = machine_with_two_disks();
    machine.floppy_images[1].as_mut().unwrap().geometry.heads = 1;
    setup_dma(&mut machine, SOURCE, 2 * SECTOR_BYTES);
    let request = data_request(&mut machine, 0xC5, 1, [0, 0, 9], 9);
    expect_rejected_write(&mut machine, request);
}

#[test]
fn unsupported_dma_or_unreadable_source_is_rejected_without_changes() {
    for (address, count, mode, masked) in [
        (SOURCE, 512, 0x46, false), // Read mode cannot execute a write.
        (SOURCE, 512, 0x4A, true),
        (SOURCE, 1, 0x4A, false),
        (SOURCE, 513, 0x4A, false),
        (0x1FF00, 512, 0x4A, false),  // Crosses DMA's 64 KiB page.
        (0x3FE00, 1024, 0x4A, false), // Source exceeds the fixture's RAM.
    ] {
        let mut machine = machine_with_two_disks();
        setup_dma(&mut machine, address, count);
        machine.dma.write(0x0B, mode);
        if masked {
            machine.dma.write(0x0A, 0x06);
        }
        let request = data_request(&mut machine, 0x45, 1, [0, 0, 1], 9);
        expect_rejected_write(&mut machine, request);
    }
}

#[test]
fn missing_selected_disk_completes_with_error_and_preserves_other_drive() {
    for drive in [0, 1] {
        let mut machine = machine_with_two_disks();
        machine.floppy_images[drive as usize] = None;
        let disks = disk_snapshot(&machine);
        let memory = machine.memory.clone();
        setup_dma(&mut machine, SOURCE, SECTOR_BYTES);
        let request = data_request(&mut machine, 0x45, drive, [5, 1, 4], 9);

        machine.handle_floppy_operation(request).unwrap();

        // Current absent-media model: abnormal termination + missing address mark.
        assert_eq!(
            completion_results(&mut machine),
            [0x44 | drive, 1, 0, 5, 1, 4, 2]
        );
        assert!(disk_snapshot(&machine) == disks);
        assert!(machine.memory == memory);
        assert_eq!(machine.dma.physical_address() as usize, SOURCE);
        assert_eq!(machine.dma.count_bytes(), SECTOR_BYTES);
        assert!(
            !machine.dma.is_masked(),
            "error must not consume the DMA transfer"
        );
    }
}

#[test]
fn writes_use_selected_disk_geometry_for_all_supported_image_sizes() {
    for (cylinders, heads, image_length, start, destination, result) in [
        (40, 1, 180 * 1024, [39, 0, 9], 359, [40, 0, 1, 2]),
        (40, 2, 360 * 1024, [39, 1, 9], 719, [40, 0, 1, 2]),
        (80, 2, 720 * 1024, [79, 1, 9], 1439, [80, 0, 1, 2]),
    ] {
        let mut machine = machine_with_two_disks();
        let mut image = disk_image(image_length);
        machine.floppy_images[1] = Some(floppy_360kib(image.clone()));
        machine.floppy_images[1].as_mut().unwrap().geometry = DiskGeometry {
            cylinders,
            heads,
            sectors_per_track: 9,
        };
        let other = machine.floppy_images[0].as_ref().unwrap().bytes.clone();
        let bytes = vec![0xA5; SECTOR_BYTES];
        machine.memory[SOURCE..SOURCE + SECTOR_BYTES].copy_from_slice(&bytes);
        let memory = machine.memory.clone();
        image[destination * SECTOR_BYTES..(destination + 1) * SECTOR_BYTES].copy_from_slice(&bytes);
        setup_dma(&mut machine, SOURCE, SECTOR_BYTES);
        let command = if heads == 1 { 0x45 } else { 0xC5 };
        let request = data_request(&mut machine, command, 1, start, 9);

        machine.handle_floppy_operation(request).unwrap();

        assert!(machine.floppy_images[1].as_ref().unwrap().bytes == image);
        assert!(machine.floppy_images[0].as_ref().unwrap().bytes == other);
        assert!(machine.memory == memory);
        expect_dma_finished(&machine, SOURCE, SECTOR_BYTES);
        assert_eq!(&completion_results(&mut machine)[3..], &result);
    }
}

#[test]
fn write_request_is_available_only_after_all_parameters_and_only_once() {
    for command in [0x05, 0x45, 0xC5] {
        let mut machine = machine_with_two_disks();
        machine.fdc.write(0x3F2, 0x08);
        machine.fdc.write(0x3F5, command);
        let parameters = [1, 0, 0, 1, 2, 9, 0x2A, 0xFF];
        for byte in parameters {
            assert!(machine.fdc.take_data_request().is_none());
            assert_eq!(machine.fdc.read(0x3F4), 0x90);
            assert!(!machine.fdc.irq_output());
            machine.fdc.write(0x3F5, byte);
        }
        assert_eq!(machine.fdc.read(0x3F4), 0x10);
        let request = machine.fdc.take_data_request().unwrap();
        assert_eq!(request.command, command, "preserve the command flags");
        assert_eq!(request.parameters, parameters);
        assert!(machine.fdc.take_data_request().is_none());
        assert_eq!(
            machine.fdc.read(0x3F4),
            0x10,
            "remain busy until completion"
        );
        assert!(!machine.fdc.irq_output());
    }
}

#[test]
fn machine_port_dispatch_executes_write_after_the_last_parameter() {
    let mut machine = machine_with_two_disks();
    let bytes = disk_image(2 * SECTOR_BYTES);
    machine.memory[SOURCE..SOURCE + bytes.len()].copy_from_slice(&bytes);
    let memory = machine.memory.clone();
    let original = disk_snapshot(&machine);
    let mut expected = original.clone();
    expected[1].as_mut().unwrap().0[..bytes.len()].copy_from_slice(&bytes);
    setup_dma(&mut machine, SOURCE, bytes.len());
    machine.write_port8(0x3F2, 0x08).unwrap();
    for byte in [0x45, 1, 0, 0, 1, 2, 9, 0x2A] {
        machine.write_port8(0x3F5, byte).unwrap();
        assert!(
            disk_snapshot(&machine) == original,
            "incomplete command must not write"
        );
        assert!(!machine.fdc.irq_output());
    }

    machine.write_port8(0x3F5, 0xFF).unwrap();

    assert!(disk_snapshot(&machine) == expected);
    assert!(machine.memory == memory);
    expect_dma_finished(&machine, SOURCE, bytes.len());
    assert_eq!(completion_results(&mut machine), [1, 0, 0, 0, 0, 3, 2]);
}
