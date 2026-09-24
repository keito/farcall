//! Direct tests of Machine::handle_floppy_read, bypassing the hosted INT 13h BIOS.
//!
//! NEC uPD765A/B: Read Data and Table 6 (result C/H/R/N after terminal count).
//! https://cpctech.cpcwiki.de/docs/upd765a/necfdc.htm

use super::{Machine, disk_image, floppy_360kib, machine_with_image};
use crate::machine::fdc::FdcReadData;

const SECTOR_BYTES: usize = 512;
const DESTINATION: usize = 0x12000;

fn setup_dma(machine: &mut Machine, destination: usize, sectors: usize) {
    let count = u16::try_from(sectors * SECTOR_BYTES - 1).unwrap();
    machine.dma.write(0x0C, 0); // Reset the shared low/high-byte flip-flop.
    machine.dma.write(0x81, (destination >> 16) as u8);
    for byte in (destination as u16).to_le_bytes() {
        machine.dma.write(0x04, byte);
    }
    for byte in count.to_le_bytes() {
        machine.dma.write(0x05, byte);
    }
    machine.dma.write(0x0B, 0x46); // Channel 2: device to memory, increment address.
    machine.dma.write(0x0A, 0x02); // Unmask channel 2.
}

fn read_request(
    machine: &mut Machine,
    multi_track: bool,
    drive: u8,
    [cylinder, head, sector]: [u8; 3],
    eot: u8,
) -> FdcReadData {
    // Build the request through the FDC's command interface, including its
    // execution state, rather than exposing private request-parser fields.
    machine.fdc.write(0x3F2, 0x08); // Enable the interrupt gate without a reset event.
    let command = if multi_track { 0xC6 } else { 0x46 }; // MFM Read Data.
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
    machine.fdc.take_read_request().unwrap()
}

fn expect_completion(machine: &mut Machine, drive: u8, chrn: [u8; 4]) {
    assert!(machine.fdc.irq_output(), "read completion must raise IRQ");
    let results: [u8; 7] = std::array::from_fn(|_| {
        assert_eq!(machine.fdc.read(0x3F4), 0xD0);
        machine.fdc.read(0x3F5)
    });
    // Check normal status and unit selection independently of ST0's HD bit.
    assert_eq!(results[0] & !0x04, drive, "ST0 status and drive");
    assert_eq!(&results[1..3], &[0, 0], "ST1/ST2 must report no error");
    assert_eq!(&results[3..], &chrn, "result C/H/R/N");
    assert_eq!(machine.fdc.read(0x3F4), 0x80, "exactly seven result bytes");
    assert!(
        !machine.fdc.irq_output(),
        "reading results acknowledges IRQ"
    );
}

fn expect_dma_finished(machine: &Machine, destination: usize, sectors: usize) {
    let next_offset = (destination + sectors * SECTOR_BYTES) & 0xFFFF;
    assert_eq!(
        machine.dma.physical_address() as usize,
        (destination & 0xF0000) | next_offset,
        "DMA advances its address without incrementing the page"
    );
    // The count register underflows to FFFF after its final byte.
    assert_eq!(machine.dma.count_bytes(), 0x10000);
}

fn expect_read(
    multi_track: bool,
    start: [u8; 3],
    eot: u8,
    source_sectors: &[usize],
    result_chrn: [u8; 4],
    destination: usize,
) {
    let image = disk_image(360 * 1024);
    let mut machine = machine_with_image(Some(image.clone()));
    let mut expected_memory = machine.memory.clone();
    for (index, &source) in source_sectors.iter().enumerate() {
        let dest = destination + index * SECTOR_BYTES;
        let source = source * SECTOR_BYTES;
        expected_memory[dest..dest + SECTOR_BYTES]
            .copy_from_slice(&image[source..source + SECTOR_BYTES]);
    }
    setup_dma(&mut machine, destination, source_sectors.len());
    let request = read_request(&mut machine, multi_track, 0, start, eot);

    machine
        .handle_floppy_read(request)
        .expect("valid floppy read must succeed");

    assert!(
        machine.memory == expected_memory,
        "copy exactly the requested sectors"
    );
    assert!(machine.floppy_images[0].as_ref().unwrap().bytes == image);
    expect_dma_finished(&machine, destination, source_sectors.len());
    expect_completion(&mut machine, 0, result_chrn);
}

#[test]
fn result_coordinates_follow_terminal_count_and_eot_on_either_head() {
    // Cylinder 5 occupies image sectors 90..108 (zero-based).
    // Explicit expected coordinates cover both heads, MT off/on, and TC
    // before/at EOT. They are examples of the datasheet's result table.
    for (mt, start, eot, sources, result) in [
        (false, [5, 0, 4], 9, [93, 94], [5, 0, 6, 2]),
        (false, [5, 1, 4], 9, [102, 103], [5, 1, 6, 2]),
        (true, [5, 0, 4], 9, [93, 94], [5, 0, 6, 2]),
        (true, [5, 1, 4], 9, [102, 103], [5, 1, 6, 2]),
        (false, [5, 0, 8], 9, [97, 98], [6, 0, 1, 2]),
        (false, [5, 1, 8], 9, [106, 107], [6, 1, 1, 2]),
        (true, [5, 0, 8], 9, [97, 98], [5, 1, 1, 2]),
        (true, [5, 1, 8], 9, [106, 107], [6, 0, 1, 2]),
        // EOT is a command parameter, not necessarily the physical track end.
        (false, [5, 0, 4], 5, [93, 94], [6, 0, 1, 2]),
        (true, [5, 0, 4], 5, [93, 94], [5, 1, 1, 2]),
    ] {
        expect_read(mt, start, eot, &sources, result, DESTINATION);
    }
}

#[test]
fn starting_above_eot_reads_consecutive_sectors_until_dma_finishes() {
    // EOT=5 is never encountered when reading sectors 6 and 7. DMA ends
    // the transfer; neither repeating sector 6 nor switching heads is correct.
    for multi_track in [false, true] {
        expect_read(
            multi_track,
            [5, 0, 6],
            5,
            &[95, 96],
            [5, 0, 8, 2],
            DESTINATION,
        );
    }
}

#[test]
fn selected_drive_supplies_the_data_and_result_unit() {
    let image_a = disk_image(360 * 1024);
    let image_b: Vec<u8> = image_a.iter().map(|byte| byte ^ 0xFF).collect();
    for drive in [0, 1] {
        let mut machine = machine_with_image(Some(image_a.clone()));
        machine.floppy_images[1] = Some(floppy_360kib(image_b.clone()));
        let source = if drive == 0 { &image_a } else { &image_b };
        let mut expected_memory = machine.memory.clone();
        expected_memory[DESTINATION..DESTINATION + 512].copy_from_slice(&source[512..1024]);
        setup_dma(&mut machine, DESTINATION, 1);
        let request = read_request(&mut machine, false, drive, [0, 0, 2], 9);

        machine.handle_floppy_read(request).unwrap();

        assert!(machine.memory == expected_memory);
        assert!(machine.floppy_images[0].as_ref().unwrap().bytes == image_a);
        assert!(machine.floppy_images[1].as_ref().unwrap().bytes == image_b);
        expect_dma_finished(&machine, DESTINATION, 1);
        expect_completion(&mut machine, drive, [0, 0, 3, 2]);
    }
}

#[test]
fn transfer_can_end_exactly_at_a_dma_page_boundary() {
    expect_read(false, [5, 0, 4], 9, &[93], [5, 0, 5, 2], 0x1FE00);
}

#[test]
fn multi_track_read_continues_onto_head_one_and_stops_at_dma_count() {
    // Head 0 sectors 8,9, then head 1 sector 1; TC stops before its EOT.
    expect_read(true, [5, 0, 8], 9, &[97, 98, 99], [5, 1, 2, 2], DESTINATION);
}

#[test]
fn multi_track_read_of_a_full_cylinder_reports_the_next_cylinder() {
    // Both surfaces of cylinder 5; reporting C=6 does not read cylinder 6.
    let sources: Vec<usize> = (90..108).collect();
    expect_read(true, [5, 0, 1], 9, &sources, [6, 0, 1, 2], DESTINATION);
}

#[test]
fn multi_track_read_uses_eot_to_switch_heads_before_the_physical_track_end() {
    // Head 0 sectors 4,5, then head 1 sector 1. Physical sectors 6..9 on
    // head 0 must be skipped: a contiguous image slice would be incorrect.
    expect_read(true, [5, 0, 4], 5, &[93, 94, 99], [5, 1, 2, 2], DESTINATION);
}
