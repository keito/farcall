use super::{DiskGeometry, parse_disk_geometry};

#[test]
fn hard_disk_offsets_above_four_gib_preserve_the_full_address() {
    let geometry = DiskGeometry {
        cylinders: 16383,
        heads: 16,
        sectors_per_track: 63,
    };
    for (cylinder, head, sector, offset) in [
        (9000, 0, 1, 4_644_864_000u64),
        (16382, 15, 63, 8_455_200_256),
    ] {
        assert_eq!(geometry.byte_offset(cylinder, head, sector), Some(offset));
    }
    assert_eq!(geometry.byte_offset(16383, 0, 1), None);
}

#[test]
fn geometry_parser_accepts_boundary_counts_and_custom_geometries() {
    for (text, expected) in [
        ("1,1,1", (1, 1, 1)),
        ("16383,16,63", (16383, 16, 63)),
        ("100,4,25", (100, 4, 25)),
    ] {
        let geometry = parse_disk_geometry(text).unwrap();
        assert_eq!(
            (
                geometry.cylinders,
                geometry.heads,
                geometry.sectors_per_track
            ),
            expected,
            "geometry {text}"
        );
    }
}

#[test]
fn geometry_parser_rejects_malformed_and_out_of_range_values() {
    for text in [
        "",
        "32,16",
        "32,16,63,1",
        "32,,63",
        "abc,16,63",
        "-1,16,63",
        "0,16,63",
        "16384,16,63",
        "32,0,63",
        "32,17,63",
        "32,16,0",
        "32,16,64",
        "18446744073709551616,16,63",
    ] {
        assert!(
            parse_disk_geometry(text).is_err(),
            "accepted geometry {text:?}"
        );
    }
}

#[test]
fn single_sided_geometry_maps_cylinders_and_rejects_a_second_head() {
    let geometry = DiskGeometry {
        cylinders: 40,
        heads: 1,
        sectors_per_track: 9,
    };
    for (cylinder, sector, offset) in [(0, 1, 0), (0, 9, 4096), (1, 1, 4608), (39, 9, 183808)] {
        assert_eq!(geometry.byte_offset(cylinder, 0, sector), Some(offset));
    }
    for (cylinder, head, sector) in [(0, 1, 1), (40, 0, 1), (0, 0, 0), (0, 0, 10)] {
        assert_eq!(geometry.byte_offset(cylinder, head, sector), None);
    }
}

#[test]
fn disk_offset_maps_chs_and_rejects_geometry_boundaries() {
    let geometry = DiskGeometry {
        cylinders: 40,
        heads: 2,
        sectors_per_track: 9,
    };
    for (cylinder, head, sector, offset) in [
        (0, 0, 1, 0),
        (0, 0, 2, 512),
        (0, 0, 9, 4096),
        (0, 1, 1, 4608),
        (0, 1, 9, 8704),
        (1, 0, 1, 9216),
        (39, 1, 9, 368128),
    ] {
        assert_eq!(
            geometry.byte_offset(cylinder, head, sector),
            Some(offset),
            "CHS={cylinder}/{head}/{sector}"
        );
    }
    for (cylinder, head, sector) in [
        (40, 0, 1),
        (0, 2, 1),
        (0, 0, 0),
        (0, 0, 10),
        (usize::MAX, 0, 1),
        (0, usize::MAX, 1),
        (0, 0, usize::MAX),
    ] {
        assert_eq!(
            geometry.byte_offset(cylinder, head, sector),
            None,
            "CHS={cylinder}/{head}/{sector}"
        );
    }
}
