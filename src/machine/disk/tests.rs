use super::DiskGeometry;

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
