use super::{MemoryError, PhysAddr, read_buffer};

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
fn read_buffer_uses_the_translated_address_and_crosses_64k_without_wrapping() {
    let mut memory = vec![0xCC; 0x12350];
    memory[0x12345..0x12349].copy_from_slice(b"test");
    memory[0xFFFE..0x10002].copy_from_slice(&[1, 2, 3, 4]);
    memory[0..2].copy_from_slice(&[0xEE, 0xFF]);

    assert_eq!(
        read_buffer(&memory, PhysAddr::new(0x1234, 5), 4).unwrap(),
        b"test"
    );
    // Physical addresses continue through 10000h; only the 1 MiB boundary wraps.
    assert_eq!(
        read_buffer(&memory, PhysAddr::new(0, 0xFFFE), 4).unwrap(),
        [1, 2, 3, 4]
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
        for address in [PhysAddr::new(0, 0), PhysAddr::new(0xFFFF, 0x000F)] {
            assert!(read_buffer(memory, address, 0).unwrap().is_empty());
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
