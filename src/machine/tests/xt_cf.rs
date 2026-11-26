use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use super::Machine;
use crate::machine::{
    BootMode,
    disk::{DiskImageConfig, DiskType},
};

const IMAGE_SIZE: u64 = 32 * 16 * 63 * 512;
const DATA: u16 = 0x300;
const ERROR: u16 = 0x302;
const COMMAND_STATUS: u16 = 0x30E;

// Disposable images go through Machine::new so the tests also check that the
// normal HDD-loading path opens the file with write access. No BIOS is executed.
struct TestDisk {
    directory: PathBuf,
}

impl TestDisk {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "farcall-xt-cf-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        let disk = Self { directory };
        let mut file = File::create(disk.directory.join("hdd.img")).unwrap();
        file.set_len(IMAGE_SIZE).unwrap();
        file.write_all(&[0xA5; 512]).unwrap();
        fs::write(disk.directory.join("floppy.img"), [0u8; 512]).unwrap();
        fs::write(disk.directory.join("bios.rom"), [0u8; 8192]).unwrap();
        let mut option_rom = [0u8; 512];
        option_rom[..4].copy_from_slice(&[0x55, 0xAA, 1, 0xCB]); // RETF entry.
        option_rom[511] = 0u8.wrapping_sub(option_rom.iter().fold(0u8, |a, b| a.wrapping_add(*b)));
        fs::write(disk.directory.join("option.rom"), option_rom).unwrap();
        disk
    }

    fn machine(&self) -> Machine {
        Machine::new(
            BootMode::Floppy {
                drive_a: DiskImageConfig {
                    path: self.directory.join("floppy.img"),
                    disk_type: DiskType::Floppy720,
                    write_protected: true,
                },
                drive_b: None,
                hdd: Some(self.directory.join("hdd.img")),
                rom: Some(self.directory.join("bios.rom")),
                rom_fixed_disk: Some(self.directory.join("option.rom")),
            },
            0,
        )
        .unwrap()
    }

    fn bytes(&self) -> Vec<u8> {
        // Reopen each time: this checks the backing image rather than the
        // controller's in-memory transfer buffer.
        fs::read(self.directory.join("hdd.img")).unwrap()
    }
}

impl Drop for TestDisk {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn input(machine: &mut Machine, port: u16) -> u8 {
    machine.read_port8(port).unwrap();
    machine.cpu.ax as u8
}

fn output(machine: &mut Machine, port: u16, value: u8) {
    machine.write_port8(port, value).unwrap();
}

fn command(machine: &mut Machine, opcode: u8, count: u8, cylinder: u16, head: u8, sector: u8) {
    for (port, value) in [
        (0x30C, 0xA0 | head),
        (0x304, count),
        (0x306, sector),
        (0x308, cylinder as u8),
        (0x30A, (cylinder >> 8) as u8),
        (COMMAND_STATUS, opcode),
    ] {
        output(machine, port, value);
    }
}

fn pattern(seed: u8) -> [u8; 512] {
    std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(seed))
}

fn send_sector(machine: &mut Machine, bytes: &[u8; 512]) {
    for &byte in bytes {
        output(machine, DATA, byte);
    }
}

#[test]
fn write_one_sector_persists_and_leaves_other_sectors_unchanged() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    let bytes = pattern(17);
    // CHS 1/2/3 is LBA 1136: exercise all three address registers.
    let offset = 1136 * 512;
    command(&mut machine, 0xC5, 1, 1, 2, 3);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
    send_sector(&mut machine, &bytes);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert_eq!(input(&mut machine, ERROR), 0);
    drop(machine);

    expected[offset..offset + 512].copy_from_slice(&bytes);
    assert!(
        disk.bytes() == expected,
        "backing image differs from expected bytes"
    );

    let mut reopened = disk.machine();
    command(&mut reopened, 0xC4, 1, 1, 2, 3);
    let readback: Vec<_> = (0..512).map(|_| input(&mut reopened, DATA)).collect();
    assert_eq!(readback, bytes);
    assert_eq!(input(&mut reopened, COMMAND_STATUS), 0x40);
}

#[test]
fn partial_write_does_not_commit_and_a_new_command_cancels_it() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let expected = disk.bytes();
    command(&mut machine, 0xC5, 1, 0, 0, 1);
    for &byte in &pattern(42)[..511] {
        output(&mut machine, DATA, byte);
    }
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
    assert!(
        disk.bytes() == expected,
        "backing image differs from expected bytes"
    );
    output(&mut machine, COMMAND_STATUS, 0xEF); // Cancel with SET FEATURES.
    output(&mut machine, DATA, 0xFF);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert!(
        disk.bytes() == expected,
        "backing image differs from expected bytes"
    );
}

#[test]
fn consecutive_writes_seek_to_each_commands_address() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    for (cylinder, head, sector, lba, seed) in [(2, 3, 4, 2208, 7), (0, 0, 1, 0, 99)] {
        let bytes = pattern(seed);
        command(&mut machine, 0xC5, 1, cylinder, head, sector);
        send_sector(&mut machine, &bytes);
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        expected[lba * 512..(lba + 1) * 512].copy_from_slice(&bytes);
    }
    assert!(
        disk.bytes() == expected,
        "backing image differs from expected bytes"
    );
}

#[test]
fn invalid_write_address_reports_idnf_without_changing_image() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let expected = disk.bytes();
    for (cylinder, sector) in [(0, 0), (0, 64), (32, 1)] {
        command(&mut machine, 0xC5, 1, cylinder, 0, sector);
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x41);
        assert_eq!(input(&mut machine, ERROR), 0x10);
        send_sector(&mut machine, &pattern(9));
    }
    assert!(
        disk.bytes() == expected,
        "backing image differs from expected bytes"
    );
}

#[test]
fn data_writes_during_identify_do_not_write_to_disk() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let expected = disk.bytes();
    output(&mut machine, COMMAND_STATUS, 0xEC);
    send_sector(&mut machine, &pattern(29));
    assert!(
        disk.bytes() == expected,
        "IDENTIFY authorizes data reads only"
    );
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
    let identify: Vec<_> = (0..512).map(|_| input(&mut machine, DATA)).collect();
    assert_eq!(&identify[2..4], &32u16.to_le_bytes());
    assert_eq!(&identify[6..8], &16u16.to_le_bytes());
    assert_eq!(&identify[12..14], &63u16.to_le_bytes());
}

#[test]
fn data_writes_during_read_do_not_write_to_the_following_sector() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let expected = disk.bytes();
    command(&mut machine, 0xC4, 1, 0, 0, 1);
    send_sector(&mut machine, &pattern(81));
    assert!(disk.bytes() == expected, "READ must not permit disk writes");
    let readback: Vec<_> = (0..512).map(|_| input(&mut machine, DATA)).collect();
    assert_eq!(readback, expected[..512]);
}

#[test]
fn data_reads_during_write_do_not_consume_the_write_buffer() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    let bytes = pattern(63);
    command(&mut machine, 0xC5, 1, 0, 0, 1);
    let _ = input(&mut machine, DATA); // Wrong direction: must not advance cursor.
    send_sector(&mut machine, &bytes);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    expected[..512].copy_from_slice(&bytes);
    assert!(
        disk.bytes() == expected,
        "backing image differs from expected bytes"
    );
}

#[test]
fn two_sector_write_commits_only_complete_sectors_and_persists() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    let sectors = [pattern(11), pattern(92)];
    let start = 1136 * 512; // CHS 1/2/3.
    command(&mut machine, 0xC5, 2, 1, 2, 3);

    for (index, bytes) in sectors.iter().enumerate() {
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
        for &byte in &bytes[..511] {
            output(&mut machine, DATA, byte);
        }
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
        assert!(
            disk.bytes() == expected,
            "an incomplete sector was committed"
        );
        output(&mut machine, DATA, bytes[511]);
        let offset = start + index * 512;
        expected[offset..offset + 512].copy_from_slice(bytes);
        assert!(
            disk.bytes() == expected,
            "sector {index} was not written at the expected offset"
        );
        assert_eq!(input(&mut machine, ERROR), 0);
        assert_eq!(
            input(&mut machine, COMMAND_STATUS),
            if index == 0 { 0x48 } else { 0x40 }
        );
    }
    // Once complete, extra data must not overwrite the next sector.
    send_sector(&mut machine, &pattern(201));
    drop(machine);
    assert!(disk.bytes() == expected);

    let mut reopened = disk.machine();
    command(&mut reopened, 0xC4, 2, 1, 2, 3);
    for expected_sector in sectors {
        assert_eq!(input(&mut reopened, COMMAND_STATUS), 0x48);
        let actual: Vec<_> = (0..512).map(|_| input(&mut reopened, DATA)).collect();
        assert_eq!(actual, expected_sector);
    }
    assert_eq!(input(&mut reopened, COMMAND_STATUS), 0x40);
}

#[test]
fn multi_sector_writes_cross_track_and_cylinder_boundaries() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    for (head, lba) in [(0, 62), (15, 1007)] {
        // Sector 63 is followed by sector 1 on the next head or cylinder.
        command(&mut machine, 0xC5, 2, 0, head, 63);
        for index in 0..2 {
            let bytes = pattern(head.wrapping_add(index as u8).wrapping_add(30));
            assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
            send_sector(&mut machine, &bytes);
            let offset = (lba + index) * 512;
            expected[offset..offset + 512].copy_from_slice(&bytes);
        }
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        assert_eq!(input(&mut machine, ERROR), 0);
        assert!(
            disk.bytes() == expected,
            "incorrect write across head {head} boundary"
        );
    }
}

#[test]
fn large_write_counts_include_zero_meaning_256_sectors() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    for (count, total) in [(255, 255usize), (0, 256)] {
        command(&mut machine, 0xC5, count, 0, 0, 9);
        for index in 0..total {
            assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48, "sector {index}");
            let bytes = pattern((index as u8).wrapping_add(count));
            send_sector(&mut machine, &bytes);
            let offset = (8 + index) * 512;
            expected[offset..offset + 512].copy_from_slice(&bytes);
        }
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40, "count={count}");
        assert_eq!(input(&mut machine, ERROR), 0);
        assert!(
            disk.bytes() == expected,
            "incorrect image after count={count}"
        );
    }
}

#[test]
fn cancelling_partial_multi_sector_write_preserves_only_completed_sectors() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    let first = pattern(19);
    command(&mut machine, 0xC5, 2, 0, 0, 1);
    send_sector(&mut machine, &first);
    expected[..512].copy_from_slice(&first);
    for &byte in &pattern(83)[..511] {
        output(&mut machine, DATA, byte);
    }
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
    assert!(disk.bytes() == expected);
    output(&mut machine, COMMAND_STATUS, 0xEF);
    output(&mut machine, DATA, 0xFF);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert!(disk.bytes() == expected);

    // A new command must start at byte zero with a fresh sector count.
    let replacement = pattern(141);
    command(&mut machine, 0xC5, 1, 0, 0, 3);
    send_sector(&mut machine, &replacement);
    expected[1024..1536].copy_from_slice(&replacement);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert!(disk.bytes() == expected);
}

#[test]
fn write_to_final_sector_succeeds_without_growing_image() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    let bytes = pattern(37);
    command(&mut machine, 0xC5, 1, 31, 15, 63);
    send_sector(&mut machine, &bytes);
    expected[IMAGE_SIZE as usize - 512..].copy_from_slice(&bytes);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert_eq!(input(&mut machine, ERROR), 0);
    assert!(disk.bytes() == expected);
}

#[test]
fn write_past_image_end_reports_idnf_without_growing_image_and_recovers() {
    let disk = TestDisk::new();
    let mut machine = disk.machine();
    let mut expected = disk.bytes();
    let first = pattern(47);
    command(&mut machine, 0xC5, 2, 31, 15, 63);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
    send_sector(&mut machine, &first);
    expected[IMAGE_SIZE as usize - 512..].copy_from_slice(&first);
    send_sector(&mut machine, &pattern(89));
    // File::write_all can extend a regular file. The controller must enforce
    // the disk's capacity and report IDNF instead of enlarging the image.
    assert_eq!(
        (
            input(&mut machine, COMMAND_STATUS),
            input(&mut machine, ERROR),
            fs::metadata(disk.directory.join("hdd.img")).unwrap().len(),
        ),
        (0x41, 0x10, IMAGE_SIZE),
        "a write beyond capacity must report IDNF and preserve the image size"
    );
    send_sector(&mut machine, &pattern(211));
    assert!(disk.bytes() == expected);

    let recovery = pattern(131);
    command(&mut machine, 0xC5, 1, 0, 0, 1);
    send_sector(&mut machine, &recovery);
    expected[..512].copy_from_slice(&recovery);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert_eq!(input(&mut machine, ERROR), 0);
    assert!(disk.bytes() == expected);
}
