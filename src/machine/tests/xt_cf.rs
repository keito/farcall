use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use super::Machine;
use crate::machine::{
    BootMode, DiskGeometry, MachineConfig,
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
        self.load_machine(Some(self.directory.join("hdd.img")), None)
            .unwrap()
    }

    fn load_machine(
        &self,
        image: Option<PathBuf>,
        geometry: Option<DiskGeometry>,
    ) -> anyhow::Result<Machine> {
        Machine::new(
            MachineConfig::default(),
            BootMode::Floppy {
                drive_a: DiskImageConfig {
                    path: self.directory.join("floppy.img"),
                    disk_type: DiskType::Floppy720,
                    write_protected: true,
                },
                drive_b: None,
                hdd: image,
                hdd_geometry: geometry,
                rom: Some(self.directory.join("bios.rom")),
                rom_fixed_disk: Some(self.directory.join("option.rom")),
            },
            0,
        )
    }

    fn resize(&self, size: u64) {
        fs::OpenOptions::new()
            .write(true)
            .open(self.directory.join("hdd.img"))
            .unwrap()
            .set_len(size)
            .unwrap();
    }

    fn sector(&self, offset: u64) -> [u8; 512] {
        let mut file = File::open(self.directory.join("hdd.img")).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut bytes = [0; 512];
        file.read_exact(&mut bytes).unwrap();
        bytes
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

fn read_sector(machine: &mut Machine) -> [u8; 512] {
    std::array::from_fn(|_| input(machine, DATA))
}

fn assert_identify_geometry(machine: &mut Machine, expected: [u16; 3]) {
    output(machine, COMMAND_STATUS, 0xEC);
    assert_eq!(input(machine, COMMAND_STATUS), 0x48);
    let identify = read_sector(machine);
    for (offset, value) in [2, 6, 12].into_iter().zip(expected) {
        assert_eq!(&identify[offset..offset + 2], &value.to_le_bytes());
    }
    assert_eq!(input(machine, COMMAND_STATUS), 0x40);
    assert_eq!(input(machine, ERROR), 0);
}

#[test]
fn autodetected_geometry_controls_identify_and_the_last_sector() {
    for cylinders in [1u16, 2, 65] {
        let disk = TestDisk::new();
        let size = u64::from(cylinders) * 16 * 63 * 512;
        disk.resize(size);
        let mut machine = disk.machine();
        assert_identify_geometry(&mut machine, [cylinders, 16, 63]);

        command(&mut machine, 0xC4, 1, 0, 0, 1);
        assert_eq!(read_sector(&mut machine), [0xA5; 512]);
        let bytes = pattern(71);
        command(&mut machine, 0xC5, 1, cylinders - 1, 15, 63);
        send_sector(&mut machine, &bytes);
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        assert_eq!(input(&mut machine, ERROR), 0);
        drop(machine);
        assert_eq!(disk.sector(size - 512), bytes);
        assert_eq!(disk.sector(0), [0xA5; 512]);

        let mut reopened = disk.machine();
        command(&mut reopened, 0xC4, 1, cylinders - 1, 15, 63);
        assert_eq!(read_sector(&mut reopened), bytes);
        assert_eq!(input(&mut reopened, COMMAND_STATUS), 0x40);
        assert_eq!(
            fs::metadata(disk.directory.join("hdd.img")).unwrap().len(),
            size
        );
    }
}

#[test]
fn explicit_geometry_controls_identify_and_persistent_sector_addresses() {
    let disk = TestDisk::new();
    disk.resize(100 * 4 * 25 * 512);
    let geometry = DiskGeometry {
        cylinders: 100,
        heads: 4,
        sectors_per_track: 25,
    };
    let mut machine = disk
        .load_machine(Some(disk.directory.join("hdd.img")), Some(geometry.clone()))
        .unwrap();
    assert_identify_geometry(&mut machine, [100, 4, 25]);
    let mut expected = disk.bytes();

    // These offsets are fixed independently of the controller's geometry helper.
    for (cylinder, head, sector, lba, seed) in
        [(0, 1, 1, 25, 11), (1, 2, 3, 152, 42), (99, 3, 25, 9999, 79)]
    {
        let bytes = pattern(seed);
        command(&mut machine, 0xC5, 1, cylinder, head, sector);
        send_sector(&mut machine, &bytes);
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        assert_eq!(input(&mut machine, ERROR), 0);
        expected[lba * 512..(lba + 1) * 512].copy_from_slice(&bytes);
    }
    drop(machine);
    assert!(
        disk.bytes() == expected,
        "explicit CHS wrote to an incorrect offset"
    );

    let mut reopened = disk
        .load_machine(Some(disk.directory.join("hdd.img")), Some(geometry))
        .unwrap();
    for (cylinder, head, sector, seed) in [(0, 1, 1, 11), (1, 2, 3, 42), (99, 3, 25, 79)] {
        command(&mut reopened, 0xC4, 1, cylinder, head, sector);
        assert_eq!(read_sector(&mut reopened), pattern(seed));
        assert_eq!(input(&mut reopened, COMMAND_STATUS), 0x40);
    }
}

#[test]
fn explicit_geometry_controls_multisector_track_and_cylinder_crossings() {
    let disk = TestDisk::new();
    disk.resize(100 * 4 * 25 * 512);
    let mut machine = disk
        .load_machine(
            Some(disk.directory.join("hdd.img")),
            Some(DiskGeometry {
                cylinders: 100,
                heads: 4,
                sectors_per_track: 25,
            }),
        )
        .unwrap();
    let mut expected = disk.bytes();
    for (head, lba) in [(0, 24), (3, 99)] {
        let sectors = [pattern(31 + head), pattern(61 + head)];
        command(&mut machine, 0xC5, 2, 0, head, 25);
        for (index, bytes) in sectors.iter().enumerate() {
            assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
            send_sector(&mut machine, bytes);
            let offset = (lba + index) * 512;
            expected[offset..offset + 512].copy_from_slice(bytes);
        }
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        assert!(
            disk.bytes() == expected,
            "incorrect crossing from head {head}"
        );
        command(&mut machine, 0xC4, 2, 0, head, 25);
        for bytes in sectors {
            assert_eq!(input(&mut machine, COMMAND_STATUS), 0x48);
            assert_eq!(read_sector(&mut machine), bytes);
        }
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        assert_eq!(input(&mut machine, ERROR), 0);
    }
}

#[test]
fn explicit_geometry_rejects_out_of_range_chs_for_reads_and_writes() {
    let disk = TestDisk::new();
    disk.resize(100 * 4 * 25 * 512);
    let mut machine = disk
        .load_machine(
            Some(disk.directory.join("hdd.img")),
            Some(DiskGeometry {
                cylinders: 100,
                heads: 4,
                sectors_per_track: 25,
            }),
        )
        .unwrap();
    let expected = disk.bytes();
    for opcode in [0xC4, 0xC5] {
        for (cylinder, head, sector) in [(100, 0, 1), (0, 4, 1), (0, 0, 0), (0, 0, 26)] {
            command(&mut machine, opcode, 1, cylinder, head, sector);
            assert_eq!(input(&mut machine, COMMAND_STATUS), 0x41);
            assert_eq!(input(&mut machine, ERROR), 0x10);
            send_sector(&mut machine, &pattern(37));
            assert_eq!(read_sector(&mut machine), [0; 512]);
        }
    }
    assert!(disk.bytes() == expected, "invalid CHS changed the image");
    command(&mut machine, 0xC4, 1, 0, 0, 1);
    assert_eq!(read_sector(&mut machine), [0xA5; 512]);
    assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
    assert_eq!(input(&mut machine, ERROR), 0);
}

#[test]
fn autodetection_rejects_empty_partial_and_oversized_images() {
    let disk = TestDisk::new();
    let cylinder_bytes = 16 * 63 * 512;
    for size in [
        0,
        1,
        511,
        512,
        cylinder_bytes - 1,
        cylinder_bytes + 1,
        cylinder_bytes + 512,
        16384 * cylinder_bytes,
    ] {
        disk.resize(size);
        assert!(
            disk.load_machine(Some(disk.directory.join("hdd.img")), None)
                .is_err(),
            "accepted invalid image size {size}"
        );
        assert_eq!(
            fs::metadata(disk.directory.join("hdd.img")).unwrap().len(),
            size
        );
    }
}

#[test]
fn explicit_geometry_requires_an_exact_image_size() {
    let disk = TestDisk::new();
    for size in [0, IMAGE_SIZE - 512, IMAGE_SIZE + 512] {
        disk.resize(size);
        let geometry = DiskGeometry {
            cylinders: 32,
            heads: 16,
            sectors_per_track: 63,
        };
        assert!(
            disk.load_machine(Some(disk.directory.join("hdd.img")), Some(geometry))
                .is_err(),
            "accepted mismatched image size {size}"
        );
        assert_eq!(
            fs::metadata(disk.directory.join("hdd.img")).unwrap().len(),
            size
        );
    }
}

#[test]
fn empty_controller_ignores_geometry_and_disk_commands() {
    let disk = TestDisk::new();
    let expected = disk.bytes();
    for geometry in [
        None,
        Some(DiskGeometry {
            cylinders: 32,
            heads: 16,
            sectors_per_track: 63,
        }),
    ] {
        let mut machine = disk.load_machine(None, geometry).unwrap();
        assert!(machine.xt_cf.is_some());
        for drive in [0xA0, 0xB0] {
            output(&mut machine, 0x30C, drive);
            assert_eq!(input(&mut machine, COMMAND_STATUS), 0);
            for opcode in [0xEC, 0xC4, 0xC5] {
                output(&mut machine, COMMAND_STATUS, opcode);
                send_sector(&mut machine, &pattern(91));
                assert_eq!(input(&mut machine, COMMAND_STATUS), 0);
                assert_eq!(read_sector(&mut machine), [0; 512]);
            }
        }
    }
    assert!(disk.bytes() == expected);
}

#[test]
fn large_chs_offsets_read_and_write_above_four_gib_without_wrapping() {
    for cylinders in [9000u16, 16383] {
        let disk = TestDisk::new();
        // set_len creates a sparse image; only a few sectors are read or written.
        let size = u64::from(cylinders) * 16 * 63 * 512;
        disk.resize(size);
        let last_offset = size - 512;
        assert!(last_offset > u64::from(u32::MAX));
        let bytes = pattern(113);
        let mut machine = disk.machine();
        assert_identify_geometry(&mut machine, [cylinders, 16, 63]);
        command(&mut machine, 0xC5, 1, cylinders - 1, 15, 63);
        send_sector(&mut machine, &bytes);
        assert_eq!(input(&mut machine, COMMAND_STATUS), 0x40);
        assert_eq!(input(&mut machine, ERROR), 0);
        drop(machine);

        assert_eq!(disk.sector(last_offset), bytes);
        assert_eq!(disk.sector(0), [0xA5; 512]);
        assert_eq!(disk.sector(last_offset - 512), [0; 512]);
        assert_eq!(disk.sector(last_offset & u64::from(u32::MAX)), [0; 512]);
        assert_eq!(
            fs::metadata(disk.directory.join("hdd.img")).unwrap().len(),
            size
        );

        let mut reopened = disk.machine();
        command(&mut reopened, 0xC4, 2, cylinders - 1, 15, 63);
        assert_eq!(read_sector(&mut reopened), bytes);
        assert_eq!(input(&mut reopened, COMMAND_STATUS), 0x41);
        assert_eq!(input(&mut reopened, ERROR), 0x10);
        assert_eq!(read_sector(&mut reopened), [0; 512]);
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
