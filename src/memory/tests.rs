use crate::memory::read_buffer;

use super::{MemoryError, PhysAddr, SegmentedAddr};

#[test]
fn physical_address_addition_crosses_64k_and_wraps_only_at_20_bits() {
    for (segment, offset, increment, expected) in [
        (0x1234, 0x0005, 0, 0x12345),
        (0x1234, 0x0005, 3, 0x12348),
        (0x0000, 0xFFFF, 1, 0x10000), // No 16-bit wrap.
        (0x2000, 0xFFFF, 1, 0x30000), // Do not wrap back to segment base.
        (0x2FFF, 0x000F, 1, 0x30000), // Same physical start, different spelling.
        (0xFFFF, 0x000E, 1, 0xFFFFF),
        (0xFFFF, 0x000F, 0, 0xFFFFF),
        (0xFFFF, 0x000F, 1, 0x00000), // Actual bus boundary.
        (0xFFFF, 0x000E, 4, 0x00002),
        (0xFFFF, 0x000F, 0xFFFF, 0x0FFFE), // Largest accepted increment.
        (0xFFFF, 0xFFFF, 1, 0x0FFF0),      // Constructor already wrapped the start.
    ] {
        let address = PhysAddr::new(segment, offset);
        let original = address.as_usize();
        assert_eq!(
            address.wrapping_add(increment).as_usize(),
            expected,
            "{segment:04X}:{offset:04X} + {increment:04X}"
        );
        assert_eq!(
            address.as_usize(),
            original,
            "addition leaves its input intact"
        );
    }

    let last = PhysAddr::new(0xFFFF, 0x000F);
    assert_eq!(last.wrapping_add(1).wrapping_add(1).as_usize(), 1);
}

#[test]
fn read_buffer_returns_exactly_the_requested_bytes() {
    let memory = [0xCC, 0x00, 0x80, 0xFF, b'$', b'A', 0xEE];
    for (offset, length, expected) in [
        (1, 1, &[0x00][..]),
        (1, 5, &[0x00, 0x80, 0xFF, b'$', b'A'][..]),
        (5, 2, &[b'A', 0xEE][..]), // Ends exactly at the slice boundary.
    ] {
        let bytes = read_buffer(&memory, PhysAddr::new(0, offset), length)
            .expect("valid buffer should be readable");
        assert_eq!(bytes, expected);
    }
}

#[test]
fn read_segmented_buffer_uses_the_translated_address() {
    let mut memory = vec![0xCC; 0x12350];
    memory[0x12345..0x12349].copy_from_slice(b"test");

    assert_eq!(
        read_buffer(&memory, PhysAddr::new(0x1234, 5), 4).unwrap(),
        b"test"
    );
}

#[test]
fn read_buffer_wraps_at_the_20_bit_boundary() {
    let mut memory = vec![0xCC; 0x100000];
    memory[0xFFFFE] = 0x11;
    memory[0xFFFFF] = 0x22;
    memory[0] = 0x33;
    memory[1] = 0x44;
    for (offset, length, expected) in [
        (0x000E, 4, &[0x11, 0x22, 0x33, 0x44][..]),
        (0x000F, 1, &[0x22][..]), // Last byte is valid without reading another.
        (0x000F, 2, &[0x22, 0x33][..]),
        (0x0010, 2, &[0x33, 0x44][..]), // Starting address itself wraps.
    ] {
        let bytes = read_buffer(&memory, PhysAddr::new(0xFFFF, offset), length)
            .expect("physical address wrapping should succeed");
        assert_eq!(bytes, expected);
    }
}

#[test]
fn read_buffer_with_zero_length_does_not_access_memory() {
    for memory in [&[][..], &[0xAA][..]] {
        for address in [SegmentedAddr::new(0, 0), SegmentedAddr::new(0xFFFF, 0x000F)] {
            assert!(
                read_buffer(memory, address.to_physical(), 0)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[test]
fn read_buffer_returns_a_bounds_error_instead_of_wrapping_at_the_slice_end() {
    for (memory, segment, offset, length) in [
        (&[][..], 0, 0, 1),
        (&[0xAA, 0xBB][..], 0, 2, 1), // First byte is already outside memory.
        (&[0xAA, 0xBB][..], 0, 1, 2), // Failure after reading one valid byte.
        (&[0xAA, 0xBB][..], 0, 0, 3),
        (&[0xAA, 0xBB][..], 0x1000, 0, 1), // Offset fits, physical address doesn't.
    ] {
        let result = read_buffer(memory, PhysAddr::new(segment, offset), length);
        assert!(
            matches!(result, Err(MemoryError::OutOfBoundsAccess(_))),
            "memory length={}, segment={segment:04X}, offset={offset:04X}, length={length}",
            memory.len()
        );
    }
}
