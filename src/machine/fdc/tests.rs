use super::Fdc;

// NEC uPD765 command protocol:
// https://cpctech.cpcwiki.de/docs/upd765a/necfdc.htm
// These tests cover the current bring-up model: immediate reset/recalibration
// completion, one drive-A reset report, and successful recalibration without
// physical drive inputs. They do not establish full 765 timing, reset polling
// across four drives, absent-drive handling, or sector/DMA support.

const DOR: u16 = 0x3F2;
const STATUS: u16 = 0x3F4;
const DATA: u16 = 0x3F5;

fn expect_results(fdc: &mut Fdc, expected: &[u8]) {
    for &byte in expected {
        // Polling status must not consume data or change the result phase.
        assert_eq!(fdc.read(STATUS), 0xD0);
        assert_eq!(fdc.read(STATUS), 0xD0);
        assert_eq!(fdc.read(DATA), byte);
    }
    assert_eq!(fdc.read(STATUS), 0x80, "last byte ends the result phase");
}

fn ready_fdc() -> Fdc {
    let mut fdc = Fdc::new();
    fdc.write(DOR, 0x08);
    assert!(!fdc.irq_output());
    fdc.write(DOR, 0x0C);
    assert!(fdc.irq_output());
    fdc.write(DATA, 0x08);
    assert!(!fdc.irq_output());
    expect_results(&mut fdc, &[0xC0, 0x00]);
    fdc
}

fn specify(fdc: &mut Fdc, first: u8, second: u8) {
    fdc.write(DATA, 0x03);
    assert_eq!(fdc.read(STATUS), 0x90);
    fdc.write(DATA, first);
    assert_eq!(fdc.read(STATUS), 0x90);
    fdc.write(DATA, second);
    assert_eq!(fdc.read(STATUS), 0x80);
}

#[test]
fn reset_release_requests_irq_but_does_not_queue_results_until_sense() {
    let mut fdc = Fdc::new();
    assert!(!fdc.irq_output());
    fdc.write(DOR, 0x08);
    assert!(!fdc.irq_output());
    fdc.write(DOR, 0x0C);
    for _ in 0..3 {
        assert!(fdc.irq_output());
        assert_eq!(fdc.read(STATUS), 0x80);
    }
    fdc.write(DATA, 0x08);
    assert!(
        !fdc.irq_output(),
        "Sense acknowledges before results are read"
    );
    expect_results(&mut fdc, &[0xC0, 0x00]);
}

#[test]
fn dor_stores_motor_and_selection_changes_without_creating_reset_events() {
    let mut fdc = ready_fdc();
    for value in [0x0C, 0x1C, 0x2D, 0x4E, 0x8F, 0x0C] {
        fdc.write(DOR, value);
        assert_eq!(fdc.dor, value);
        assert!(!fdc.irq_output());
    }
    fdc.write(DOR, 0x08);
    fdc.write(DOR, 0x18);
    assert_eq!(fdc.dor, 0x18, "writes while held in reset are stored too");
    assert!(!fdc.irq_output());
}

#[test]
fn interrupt_gate_suppresses_output_without_losing_the_pending_event() {
    let mut fdc = Fdc::new();
    fdc.write(DOR, 0x04); // Release reset with the IRQ/DMA interface disabled.
    assert!(!fdc.irq_output());
    for _ in 0..3 {
        fdc.write(DOR, 0x0C);
        assert!(fdc.irq_output());
        fdc.write(DOR, 0x04);
        assert!(!fdc.irq_output());
    }
    // Sense uses the internal event even though its output pin is gated off.
    fdc.write(DATA, 0x08);
    expect_results(&mut fdc, &[0xC0, 0x00]);
    fdc.write(DOR, 0x0C);
    assert!(
        !fdc.irq_output(),
        "enabling the interface cannot restore an acknowledged event"
    );
}

#[test]
fn repeated_reset_cycles_clear_old_irq_and_generate_a_new_event() {
    let mut fdc = ready_fdc();
    for _ in 0..3 {
        fdc.write(DOR, 0x08);
        assert!(!fdc.irq_output());
        fdc.write(DOR, 0x0C);
        assert!(fdc.irq_output());
        // Enter reset again before acknowledging this event.
        fdc.write(DOR, 0x08);
        assert!(!fdc.irq_output());
        fdc.write(DOR, 0x0C);
        fdc.write(DATA, 0x08);
        expect_results(&mut fdc, &[0xC0, 0x00]);
        assert!(!fdc.irq_output());
    }
}

#[test]
fn sense_without_a_pending_event_returns_only_invalid_status() {
    let mut fdc = ready_fdc();
    for _ in 0..3 {
        fdc.write(DATA, 0x08);
        assert!(!fdc.irq_output());
        expect_results(&mut fdc, &[0x80]);
    }
}

#[test]
fn result_phase_ignores_command_writes_until_every_byte_is_read() {
    let mut fdc = Fdc::new();
    fdc.write(DOR, 0x0C);
    fdc.write(DATA, 0x08);
    for value in [0x03, 0x07, 0x08, 0xFF] {
        fdc.write(DATA, value);
        assert_eq!(fdc.read(STATUS), 0xD0);
    }
    assert_eq!(fdc.read(DATA), 0xC0);
    fdc.write(DATA, 0x03); // Still one result byte remaining.
    expect_results(&mut fdc, &[0x00]);
    specify(&mut fdc, 0xCF, 0x02);
    assert!(!fdc.irq_output());
}

#[test]
fn reset_discards_unread_results_even_after_the_first_byte_was_read() {
    for bytes_read in 0..=1 {
        let mut fdc = Fdc::new();
        fdc.write(DOR, 0x0C);
        fdc.write(DATA, 0x08);
        for _ in 0..bytes_read {
            fdc.read(DATA);
        }
        fdc.write(DOR, 0x08);
        assert!(!fdc.irq_output());
        fdc.write(DOR, 0x0C);
        assert_eq!(fdc.read(STATUS), 0x80, "old results cannot survive reset");
        fdc.write(DATA, 0x08);
        expect_results(&mut fdc, &[0xC0, 0x00]);
    }
}

#[test]
fn specify_decodes_bios_parameters_without_results_or_an_interrupt() {
    let mut fdc = ready_fdc();
    specify(&mut fdc, 0xCF, 0x02);
    assert_eq!(fdc.step_rate, 12);
    assert_eq!(fdc.head_unload_time, 15);
    assert_eq!(fdc.head_load_time, 1);
    assert!(!fdc.non_dma);
    assert!(!fdc.irq_output());
    fdc.write(DATA, 0x08);
    expect_results(&mut fdc, &[0x80]);
}

#[test]
fn specify_parameters_that_look_like_commands_are_still_parameters() {
    let mut fdc = ready_fdc();
    for (first, second, rate, unload, load, non_dma) in [
        (0x08, 0x03, 0, 8, 1, true),
        (0x07, 0x08, 0, 7, 4, false),
        (0xFF, 0xFF, 15, 15, 127, true),
        (0x00, 0x00, 0, 0, 0, false),
    ] {
        specify(&mut fdc, first, second);
        assert_eq!(fdc.step_rate, rate);
        assert_eq!(fdc.head_unload_time, unload);
        assert_eq!(fdc.head_load_time, load);
        assert_eq!(fdc.non_dma, non_dma);
        assert!(!fdc.irq_output());
    }
}

#[test]
fn reset_aborts_each_incomplete_command_parameter_sequence() {
    for partial in [&[0x03][..], &[0x03, 0xCF], &[0x07]] {
        let mut fdc = ready_fdc();
        for &byte in partial {
            fdc.write(DATA, byte);
        }
        assert_eq!(fdc.read(STATUS), 0x90);
        fdc.write(DOR, 0x08);
        fdc.write(DOR, 0x0C);
        assert_eq!(fdc.read(STATUS), 0x80);
        fdc.write(DATA, 0x08); // Must be a command, not the missing parameter.
        expect_results(&mut fdc, &[0xC0, 0x00]);
        specify(&mut fdc, 0xCF, 0x02);
    }
}

#[test]
fn reset_preserves_completed_specify_parameters() {
    let mut fdc = ready_fdc();
    specify(&mut fdc, 0xA5, 0x12);
    fdc.write(DOR, 0x08);
    fdc.write(DOR, 0x0C);
    assert_eq!(
        (fdc.step_rate, fdc.head_unload_time, fdc.head_load_time),
        (10, 5, 9)
    );
}

#[test]
fn recalibration_waits_for_drive_parameter_then_sense_returns_seek_completion() {
    let mut fdc = ready_fdc();
    fdc.write(DATA, 0x07);
    assert_eq!(fdc.read(STATUS), 0x90);
    assert!(
        !fdc.irq_output(),
        "command byte alone must not complete recalibration"
    );
    fdc.write(DATA, 0x00);
    assert!(fdc.irq_output());
    // Test command-ready bits, leaving room for future per-drive busy bits.
    assert_eq!(fdc.read(STATUS) & 0xF0, 0x80);
    fdc.write(DATA, 0x08);
    assert!(!fdc.irq_output());
    expect_results(&mut fdc, &[0x20, 0x00]);
    fdc.write(DATA, 0x08);
    expect_results(&mut fdc, &[0x80]);
}

#[test]
fn recalibration_sense_encodes_drive_and_ignores_unused_parameter_bits() {
    for (parameter, expected_status) in [
        (0x00, 0x20),
        (0x01, 0x21),
        (0x02, 0x22),
        (0x03, 0x23),
        (0xF8, 0x20),
        (0xF9, 0x21),
        (0xFA, 0x22),
        (0xFB, 0x23),
    ] {
        let mut fdc = ready_fdc();
        fdc.write(DATA, 0x07);
        fdc.write(DATA, parameter);
        assert!(fdc.irq_output());
        fdc.write(DATA, 0x08);
        expect_results(&mut fdc, &[expected_status, 0x00]);
        assert!(!fdc.irq_output());
    }
}

#[test]
fn recalibration_can_raise_a_fresh_irq_after_previous_results_are_consumed() {
    let mut fdc = ready_fdc();
    for _ in 0..3 {
        fdc.write(DATA, 0x07);
        assert!(!fdc.irq_output());
        fdc.write(DATA, 0x00);
        assert!(fdc.irq_output());
        fdc.write(DATA, 0x08);
        assert!(!fdc.irq_output());
        expect_results(&mut fdc, &[0x20, 0x00]);
    }
}

#[test]
fn reset_replaces_a_pending_recalibration_event() {
    let mut fdc = ready_fdc();
    fdc.write(DATA, 0x07);
    fdc.write(DATA, 0x00);
    assert!(fdc.irq_output());
    fdc.write(DOR, 0x08);
    assert!(!fdc.irq_output());
    fdc.write(DOR, 0x0C);
    fdc.write(DATA, 0x08);
    expect_results(&mut fdc, &[0xC0, 0x00]);
}

#[test]
fn invalid_command_returns_one_status_byte_without_irq_and_allows_recovery() {
    let mut fdc = ready_fdc();
    fdc.write(DATA, 0xFF);
    assert!(!fdc.irq_output());
    expect_results(&mut fdc, &[0x80]);
    specify(&mut fdc, 0xCF, 0x02);
    assert!(!fdc.irq_output());
}
