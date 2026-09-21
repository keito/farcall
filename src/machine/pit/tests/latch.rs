use super::{Pit, advance, counter, program, write_count};

// Intel 8253 count-latch / read operations:
// https://www.cpcwiki.eu/imgs/e/e3/8253.pdf
// Finish each complete read or write before starting the other. These tests
// do not assume the 8254's ability to interleave partial reads and writes.
// Direct, unlatched reads and pin-level bus timing are outside this suite.

fn latch(pit: &mut Pit, channel: u8) {
    pit.write(0x43, channel << 6);
}

fn read_word(pit: &mut Pit, channel: u8) -> u16 {
    let port = 0x40 + u16::from(channel);
    let low = pit.read(port);
    let high = pit.read(port);
    u16::from_le_bytes([low, high])
}

#[test]
fn full_bytes_are_read_low_then_high_on_each_channel() {
    for channel in 0..3 {
        for count in [0x1234, 0xABCD, 0xFFFE] {
            let mut pit = Pit::new();
            program(&mut pit, channel, count);
            latch(&mut pit, channel);
            let port = 0x40 + u16::from(channel);
            assert_eq!(pit.read(port), count as u8);
            assert_eq!(counter(&pit, channel).latched_count, Some(count));
            assert_eq!(pit.read(port), (count >> 8) as u8);
            assert!(counter(&pit, channel).latched_count.is_none());
        }
    }
}

#[test]
fn snapshot_survives_byte_rollover_between_reads() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 0x1200);
    latch(&mut pit, 0);
    assert_eq!(pit.read(0x40), 0x00);
    pit.advance(1);
    assert_eq!(pit.counter0.current, 0x11FE);
    assert_eq!(
        pit.read(0x40),
        0x12,
        "high byte belongs to the original snapshot"
    );
    latch(&mut pit, 0);
    assert_eq!(read_word(&mut pit, 0), 0x11FE);
}

#[test]
fn snapshot_survives_reload_and_output_transitions() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 6);
    pit.advance(1);
    latch(&mut pit, 0);
    pit.advance(2);
    assert_eq!(pit.counter0.current, 6);
    assert!(!pit.output0());
    pit.advance(3);
    assert!(pit.output0());
    assert_eq!(read_word(&mut pit, 0), 4);
}

#[test]
fn latch_command_ignores_mode_bits_and_does_not_reprogram_counter() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 6);
    pit.advance(4);
    assert_eq!(pit.counter0.current, 4);
    assert!(!pit.output0());
    // Bits 3..0 are don't-care for a latch command, even if they look like
    // a different mode and BCD selection in an ordinary control word.
    pit.write(0x43, 0x0F);
    assert_eq!(pit.counter0.mode, 3);
    assert!(!pit.counter0.bcd);
    assert_eq!(pit.counter0.reload, 6);
    assert_eq!(pit.counter0.current, 4);
    assert!(!pit.output0());
    pit.advance(2);
    assert!(pit.output0());
    assert_eq!(read_word(&mut pit, 0), 4);
}

#[test]
fn second_latch_before_any_read_preserves_first_snapshot() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 0x1234);
    latch(&mut pit, 0);
    pit.advance(3);
    latch(&mut pit, 0);
    assert_eq!(read_word(&mut pit, 0), 0x1234);
    latch(&mut pit, 0);
    assert_eq!(read_word(&mut pit, 0), 0x122E);
}

#[test]
fn second_latch_after_low_byte_does_not_restart_the_read_sequence() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 0x1200);
    latch(&mut pit, 0);
    assert_eq!(pit.read(0x40), 0);
    pit.advance(1);
    latch(&mut pit, 0);
    assert_eq!(pit.read(0x40), 0x12, "finish the original high byte");
    assert!(pit.counter0.latched_count.is_none());
    latch(&mut pit, 0);
    assert_eq!(read_word(&mut pit, 0), 0x11FE);
}

#[test]
fn each_channel_keeps_its_own_snapshot_and_read_position() {
    let mut pit = Pit::new();
    for (channel, count) in [(0, 0x1234), (1, 0x5678), (2, 0x9ABC)] {
        program(&mut pit, channel, count);
        latch(&mut pit, channel);
    }
    assert_eq!(pit.read(0x40), 0x34);
    assert_eq!(pit.read(0x42), 0xBC);
    pit.advance(7);
    assert_eq!(read_word(&mut pit, 1), 0x5678);
    assert_eq!(pit.read(0x40), 0x12);
    assert_eq!(pit.read(0x42), 0x9A);
    for channel in 0..3 {
        assert!(counter(&pit, channel).latched_count.is_none());
    }
}

#[test]
fn low_only_latch_is_consumed_by_one_read() {
    for channel in 0..3 {
        let mut pit = Pit::new();
        let port = 0x40 + u16::from(channel);
        pit.write(0x43, (channel << 6) | 0x16);
        pit.write(port, 0xAB);
        latch(&mut pit, channel);
        assert_eq!(pit.read(port), 0xAB);
        assert!(
            counter(&pit, channel).latched_count.is_none(),
            "one byte completes this access mode"
        );
        pit.advance(1);
        latch(&mut pit, channel);
        assert_eq!(pit.read(port), 0xAA);
    }
}

#[test]
fn high_only_latch_is_consumed_by_one_read() {
    for channel in 0..3 {
        let mut pit = Pit::new();
        let port = 0x40 + u16::from(channel);
        pit.write(0x43, (channel << 6) | 0x26);
        pit.write(port, 0x12);
        latch(&mut pit, channel);
        assert_eq!(pit.read(port), 0x12);
        assert!(
            counter(&pit, channel).latched_count.is_none(),
            "one byte completes this access mode"
        );
        pit.advance(1);
        latch(&mut pit, channel);
        assert_eq!(pit.read(port), 0x11);
    }
}

#[test]
fn completed_read_leaves_next_two_byte_write_aligned() {
    for channel in 0..3 {
        let mut pit = Pit::new();
        program(&mut pit, channel, 0x1234);
        latch(&mut pit, channel);
        assert_eq!(read_word(&mut pit, channel), 0x1234);
        write_count(&mut pit, channel, 0x5678);
        assert_eq!(counter(&pit, channel).reload, 0x5678);
        // The new reload takes effect at the old half-period boundary.
        advance(&mut pit, 0x1234 / 2);
        latch(&mut pit, channel);
        assert_eq!(read_word(&mut pit, channel), 0x5678);
    }
}

#[test]
fn encoded_zero_reads_as_zero_then_reflects_the_running_count() {
    for mode in [2, 3] {
        let mut pit = Pit::new();
        pit.write(0x43, 0x30 | (mode << 1));
        write_count(&mut pit, 0, 0);
        assert_eq!(pit.counter0.current, 65_536);
        latch(&mut pit, 0);
        assert_eq!(read_word(&mut pit, 0), 0);
        pit.advance(1);
        latch(&mut pit, 0);
        let expected = if mode == 2 { 0xFFFF } else { 0xFFFE };
        assert_eq!(read_word(&mut pit, 0), expected);
    }
}

#[test]
fn single_byte_count_writes_zero_fill_the_other_byte() {
    for (control, byte, expected) in [(0x16, 0xAB, 0x00AB), (0x26, 0x12, 0x1200)] {
        let mut pit = Pit::new();
        program(&mut pit, 0, 0xCDEF);
        pit.write(0x43, control);
        pit.write(0x40, byte);
        assert_eq!(pit.counter0.reload, expected);
        assert_eq!(pit.counter0.current, expected);
    }
    for control in [0x16, 0x26] {
        let mut pit = Pit::new();
        pit.write(0x43, control);
        pit.write(0x40, 0);
        assert_eq!(pit.counter0.current, 65_536);
        latch(&mut pit, 0);
        assert_eq!(pit.read(0x40), 0);
    }
}
