use super::{Pit, advance, assert_state, write_count};

// Intel 8253 Mode 2: https://www.cpcwiki.eu/imgs/e/e3/8253.pdf
// Binary counting, GATE assumed enabled, and the initial count loaded by the
// existing write API. Reload N gives N-1 high clocks and one low clock. Count 1
// is excluded; these tests do not model gate inputs or bus/clock-edge timing.

fn program_mode2(pit: &mut Pit, channel: u8, encoded: u16) {
    pit.write(0x43, (channel << 6) | 0x34);
    write_count(pit, channel, encoded);
}

#[test]
fn five_count_trace_has_a_single_low_tick_each_period() {
    let expected = [
        (5, true),
        (4, true),
        (3, true),
        (2, true),
        (1, false),
        (5, true),
        (4, true),
        (3, true),
        (2, true),
        (1, false),
        (5, true),
    ];
    for channel in 0..3 {
        let mut pit = Pit::new();
        program_mode2(&mut pit, channel, 5);
        for (tick, (count, output)) in expected.into_iter().enumerate() {
            if tick != 0 {
                pit.advance(1);
            }
            assert_state(&pit, channel, count, output);
        }
    }
}

#[test]
fn even_odd_minimum_and_zero_counts_have_the_programmed_period() {
    for encoded in [2, 3, 4, 5, 255, 256, 65_535, 0] {
        let reload = if encoded == 0 {
            65_536
        } else {
            u32::from(encoded)
        };
        for channel in 0..3 {
            let mut pit = Pit::new();
            program_mode2(&mut pit, channel, encoded);
            for _ in 0..3 {
                assert_state(&pit, channel, reload, true);
                advance(&mut pit, reload - 2);
                assert_state(&pit, channel, 2, true);
                pit.advance(1);
                assert_state(&pit, channel, 1, false);
                pit.advance(1);
                assert_state(&pit, channel, reload, true);
            }
        }
    }
}

#[test]
fn batches_cross_period_boundaries_without_losing_ticks() {
    for reload in [2, 3, 5, 256] {
        let mut pit = Pit::new();
        program_mode2(&mut pit, 0, reload);
        let mut elapsed = 0;
        for batch in [0, 1, 2, 7, 255, 0, 254, 6, 1] {
            pit.advance(batch);
            elapsed += u32::from(batch);
            let expected_count = u32::from(reload) - elapsed % u32::from(reload);
            assert_state(&pit, 0, expected_count, expected_count != 1);
        }
    }
}

#[test]
fn waiting_for_count_bytes_does_not_underflow_or_start_counting() {
    let mut pit = Pit::new();
    pit.advance(255);
    for channel in 0..3 {
        pit.write(0x43, (channel << 6) | 0x34);
        pit.advance(255);
        assert_state(&pit, channel, 0, true);
        pit.write(0x40 + u16::from(channel), 5);
        pit.advance(255);
        assert_state(&pit, channel, 0, true);
    }
}

#[test]
fn replacement_reload_waits_until_current_period_finishes() {
    for (old, new) in [(9, 4), (4, 9)] {
        let mut pit = Pit::new();
        program_mode2(&mut pit, 0, old);
        pit.advance(1);
        write_count(&mut pit, 0, new);
        assert_state(&pit, 0, u32::from(old) - 1, true);
        advance(&mut pit, u32::from(old) - 2);
        assert_state(&pit, 0, 1, false);
        pit.advance(1);
        assert_state(&pit, 0, u32::from(new), true);
        advance(&mut pit, u32::from(new) - 1);
        assert_state(&pit, 0, 1, false);
    }
}

#[test]
fn incomplete_reload_does_not_change_the_next_period() {
    let mut pit = Pit::new();
    program_mode2(&mut pit, 0, 5);
    pit.write(0x40, 9);
    pit.advance(5);
    assert_state(&pit, 0, 5, true);
    pit.write(0x40, 0);
    assert_state(&pit, 0, 5, true);
    pit.advance(5);
    assert_state(&pit, 0, 9, true);
}

#[test]
fn new_control_word_stops_old_count_and_returns_output_high() {
    let mut pit = Pit::new();
    program_mode2(&mut pit, 0, 5);
    pit.advance(4);
    assert_state(&pit, 0, 1, false);
    pit.write(0x43, 0x34);
    assert_state(&pit, 0, 0, true);
    pit.advance(255);
    assert_state(&pit, 0, 0, true);
    write_count(&mut pit, 0, 3);
    assert_state(&pit, 0, 3, true);
    pit.advance(2);
    assert_state(&pit, 0, 1, false);
}

#[test]
fn all_three_channels_count_with_independent_periods() {
    let mut pit = Pit::new();
    for (channel, reload) in [(0, 3), (1, 4), (2, 5)] {
        program_mode2(&mut pit, channel, reload);
    }
    for elapsed in 0..60 {
        for (channel, reload) in [(0, 3), (1, 4), (2, 5)] {
            let remaining = reload - elapsed % reload;
            assert_state(&pit, channel, remaining, remaining != 1);
        }
        pit.advance(1);
    }
}
