use super::{Counter, Pit};

mod latch;
mod mode2;

// Intel 8253/8253-5, Mode 3: https://web.cecs.pdx.edu/~mpj/llp/references/8253.pdf
// Binary counting, with GATE assumed enabled. Traces start after the initial
// count is loaded, as modeled by the current write API; pin-level load timing
// and gate control are outside these tests. Count 1 is not a valid mode-3 count.
// The 8253's odd-count sequence differs from the 8254's internal sequence:
// reload the full odd count, then subtract 1 (high) or 3 (low) on the first tick.

fn counter(pit: &Pit, channel: u8) -> &Counter {
    match channel {
        0 => &pit.counter0,
        1 => &pit.counter1,
        2 => &pit.counter2,
        _ => unreachable!(),
    }
}

fn write_count(pit: &mut Pit, channel: u8, encoded: u16) {
    let port = 0x40 + u16::from(channel);
    pit.write(port, encoded as u8);
    pit.write(port, (encoded >> 8) as u8);
}

fn program(pit: &mut Pit, channel: u8, encoded: u16) {
    pit.write(0x43, (channel << 6) | 0x36);
    write_count(pit, channel, encoded);
}

fn advance(pit: &mut Pit, mut ticks: u32) {
    while ticks != 0 {
        let batch = ticks.min(u32::from(u8::MAX)) as u8;
        pit.advance(batch);
        ticks -= u32::from(batch);
    }
}

fn assert_state(pit: &Pit, channel: u8, count: u32, output: bool) {
    let c = counter(pit, channel);
    assert_eq!((c.current, c.output), (count, output), "channel {channel}");
}

fn assert_trace(encoded: u16, states: &[(u32, bool)]) {
    for channel in 0..3 {
        let mut pit = Pit::new();
        program(&mut pit, channel, encoded);
        for (tick, &(count, output)) in states.iter().enumerate() {
            if tick != 0 {
                pit.advance(1);
            }
            let c = counter(&pit, channel);
            assert_eq!(
                (c.current, c.output),
                (count, output),
                "reload={encoded}, channel={channel}, elapsed ticks={tick}"
            );
        }
    }
}

#[test]
fn even_six_count_trace_reloads_and_toggles_every_three_ticks() {
    assert_trace(
        6,
        &[
            (6, true),
            (4, true),
            (2, true),
            (6, false),
            (4, false),
            (2, false),
            (6, true),
            (4, true),
            (2, true),
            (6, false),
            (4, false),
            (2, false),
            (6, true),
        ],
    );
}

#[test]
fn odd_five_count_trace_uses_one_then_two_and_three_then_two() {
    assert_trace(
        5,
        &[
            (5, true),
            (4, true),
            (2, true),
            (5, false),
            (2, false),
            (5, true),
            (4, true),
            (2, true),
            (5, false),
            (2, false),
            (5, true),
        ],
    );
}

#[test]
fn minimum_even_count_two_toggles_every_tick() {
    assert_trace(
        2,
        &[(2, true), (2, false), (2, true), (2, false), (2, true)],
    );
}

#[test]
fn minimum_odd_count_three_has_two_high_ticks_and_one_low_tick() {
    assert_trace(
        3,
        &[
            (3, true),
            (2, true),
            (3, false),
            (3, true),
            (2, true),
            (3, false),
            (3, true),
        ],
    );
}

fn assert_period_lengths(reloads: &[u16]) {
    for &encoded in reloads {
        let reload = if encoded == 0 {
            65_536
        } else {
            u32::from(encoded)
        };
        let high_ticks = reload.div_ceil(2);
        let low_ticks = reload / 2;
        for channel in 0..3 {
            let mut pit = Pit::new();
            program(&mut pit, channel, encoded);
            for _ in 0..3 {
                assert_state(&pit, channel, reload, true);
                advance(&mut pit, high_ticks - 1);
                assert_state(&pit, channel, 2, true);
                pit.advance(1);
                assert_state(&pit, channel, reload, false);
                advance(&mut pit, low_ticks - 1);
                assert!(!counter(&pit, channel).output);
                pit.advance(1);
                assert_state(&pit, channel, reload, true);
            }
        }
    }
}

#[test]
fn even_counts_keep_equal_half_periods_across_repeated_cycles() {
    assert_period_lengths(&[2, 4, 6, 10, 254, 256, 65_534]);
}

#[test]
fn odd_counts_keep_the_extra_tick_in_the_high_half_across_repeated_cycles() {
    assert_period_lengths(&[3, 5, 7, 255, 257, 65_535]);
}

#[test]
fn encoded_zero_means_a_65536_tick_period() {
    assert_period_lengths(&[0]);
}

fn assert_batching(reloads: &[u16]) {
    for &reload in reloads {
        let mut batched = Pit::new();
        let mut individual = Pit::new();
        program(&mut batched, 0, reload);
        program(&mut individual, 0, reload);
        let mut elapsed = 0u32;
        for batch in [0, 1, 2, 7, 0, 255, 3, 254, 6, 1] {
            batched.advance(batch);
            for _ in 0..batch {
                individual.advance(1);
            }
            elapsed += u32::from(batch);
            let expected_high = elapsed % u32::from(reload) < u32::from(reload).div_ceil(2);
            assert_eq!(
                batched.counter0.output, expected_high,
                "reload={reload}, ticks={elapsed}"
            );
            assert_eq!(
                (batched.counter0.current, batched.counter0.output),
                (individual.counter0.current, individual.counter0.output),
                "batch={batch}, reload={reload}"
            );
        }
    }
}

#[test]
fn even_count_batches_cross_multiple_boundaries_without_losing_ticks() {
    assert_batching(&[2, 4, 6, 10, 256]);
}

#[test]
fn odd_count_batches_cross_multiple_boundaries_without_losing_ticks() {
    assert_batching(&[3, 5, 7, 255, 257]);
}

#[test]
fn unprogrammed_and_partially_programmed_counters_remain_idle() {
    let mut pit = Pit::new();
    pit.advance(255);
    for channel in 0..3 {
        assert_state(&pit, channel, 0, false);
        pit.write(0x43, (channel << 6) | 0x36);
        pit.advance(255);
        assert_state(&pit, channel, 0, true);
        pit.write(0x40 + u16::from(channel), 5);
        pit.advance(255);
        assert_state(&pit, channel, 0, true);
    }
}

#[test]
fn zero_ticks_do_not_consume_the_first_tick_of_either_half_period() {
    for reload in [5, 6] {
        let mut pit = Pit::new();
        program(&mut pit, 0, reload);
        for _ in 0..2 * reload {
            let before = (pit.counter0.current, pit.counter0.output);
            pit.advance(0);
            assert_eq!((pit.counter0.current, pit.counter0.output), before);
            pit.advance(1);
        }
    }
}

#[test]
fn even_reload_updates_wait_for_the_old_half_period() {
    for (old, new) in [(12, 4), (4, 12)] {
        let mut pit = Pit::new();
        program(&mut pit, 0, old);
        pit.advance(1);
        let remaining = pit.counter0.current;
        write_count(&mut pit, 0, new);
        assert_state(&pit, 0, remaining, true);
        advance(&mut pit, remaining / 2 - 1);
        assert_state(&pit, 0, 2, true);
        pit.advance(1);
        assert_state(&pit, 0, u32::from(new), false);
        advance(&mut pit, u32::from(new) / 2);
        assert_state(&pit, 0, u32::from(new), true);
    }
}

#[test]
fn reload_parity_changes_do_not_change_the_active_half_period() {
    // Update before the first tick to catch using the new reload's parity
    // when choosing the old half-period's initial decrement.
    for (old, new) in [(6, 5), (5, 6), (5, 7)] {
        let mut pit = Pit::new();
        program(&mut pit, 0, old);
        write_count(&mut pit, 0, new);
        assert_state(&pit, 0, u32::from(old), true);
        advance(&mut pit, u32::from(old).div_ceil(2) - 1);
        assert_state(&pit, 0, 2, true);
        pit.advance(1);
        assert_state(&pit, 0, u32::from(new), false);
        advance(&mut pit, u32::from(new) / 2);
        assert_state(&pit, 0, u32::from(new), true);
        advance(&mut pit, u32::from(new).div_ceil(2));
        assert_state(&pit, 0, u32::from(new), false);
    }
}

#[test]
fn incomplete_reload_does_not_replace_count_at_a_half_period_boundary() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 6);
    pit.advance(1);
    pit.write(0x40, 5); // Only the low byte of the replacement.
    pit.advance(2);
    assert_state(&pit, 0, 6, false);
    pit.write(0x40, 0); // Complete new reload = 5 during the old low half.
    assert_state(&pit, 0, 6, false);
    pit.advance(3);
    assert_state(&pit, 0, 5, true);
    pit.advance(1);
    assert_state(&pit, 0, 4, true);
}

#[test]
fn control_word_stops_old_sequence_and_complete_count_starts_high_again() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 6);
    pit.advance(3);
    assert!(!pit.counter0.output);
    pit.write(0x43, 0x36);
    assert_state(&pit, 0, 0, true);
    pit.advance(255);
    pit.write(0x40, 5);
    pit.advance(255);
    assert_state(&pit, 0, 0, true);
    pit.write(0x40, 0);
    assert_state(&pit, 0, 5, true);
    pit.advance(1);
    assert_state(&pit, 0, 4, true);
}

#[test]
fn all_three_channels_advance_independently_with_mixed_parities() {
    let mut pit = Pit::new();
    program(&mut pit, 0, 6);
    program(&mut pit, 1, 5);
    program(&mut pit, 2, 10);
    for elapsed in 1..=60 {
        pit.advance(1);
        for (channel, period, high_ticks) in [(0, 6, 3), (1, 5, 3), (2, 10, 5)] {
            assert_eq!(
                counter(&pit, channel).output,
                elapsed % period < high_ticks,
                "channel={channel}, elapsed={elapsed}"
            );
        }
    }
}
