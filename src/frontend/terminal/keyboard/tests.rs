use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use super::{
    crossterm_to_bios_keystroke, crossterm_to_pc_scan_code, normalize_basic_crossterm_key_event,
};

#[test]
fn plain_minus_stays_unshifted_and_reaches_both_keyboard_paths() {
    let event =
        normalize_basic_crossterm_key_event(KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE));

    assert_eq!(event.code, KeyCode::Char('-'));
    assert_eq!(event.modifiers, KeyModifiers::NONE);
    let keystroke = crossterm_to_bios_keystroke(&event).expect("minus should be mapped");
    assert_eq!((keystroke.scan_code, keystroke.ascii), (0x0C, Some(b'-')));
    assert_eq!(crossterm_to_pc_scan_code(&event), Some(0x0C));
}

#[test]
fn underscore_becomes_shift_minus_and_reaches_both_keyboard_paths() {
    let event =
        normalize_basic_crossterm_key_event(KeyEvent::new(KeyCode::Char('_'), KeyModifiers::NONE));

    assert_eq!(event.code, KeyCode::Char('-'));
    assert_eq!(event.modifiers, KeyModifiers::SHIFT);
    let keystroke = crossterm_to_bios_keystroke(&event).expect("underscore should be mapped");
    assert_eq!((keystroke.scan_code, keystroke.ascii), (0x0C, Some(b'_')));
    assert_eq!(crossterm_to_pc_scan_code(&event), Some(0x0C));
}

#[test]
fn uppercase_letters_become_shift_lowercase_and_keep_their_bios_character() {
    for character in b'A'..=b'Z' {
        // Crossterm may already supply SHIFT for an uppercase character.
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            let event = normalize_basic_crossterm_key_event(KeyEvent::new(
                KeyCode::Char(char::from(character)),
                modifiers,
            ));

            assert_eq!(
                event.code,
                KeyCode::Char(char::from(character.to_ascii_lowercase()))
            );
            assert_eq!(event.modifiers, KeyModifiers::SHIFT);
            let keystroke =
                crossterm_to_bios_keystroke(&event).expect("uppercase letters should be mapped");
            assert_eq!(keystroke.ascii, Some(character));
        }
    }
}

#[test]
fn adding_shift_preserves_existing_ctrl_and_alt_modifiers() {
    for character in ['A', '_'] {
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT,
        ] {
            let event = normalize_basic_crossterm_key_event(KeyEvent::new(
                KeyCode::Char(character),
                modifiers,
            ));

            assert_eq!(event.modifiers, modifiers | KeyModifiers::SHIFT);
        }
    }
}

#[test]
fn unshifted_characters_and_non_character_keys_are_unchanged() {
    for code in [
        KeyCode::Char('a'),
        KeyCode::Char('1'),
        KeyCode::Char('-'),
        KeyCode::Enter,
    ] {
        let modifiers = KeyModifiers::CONTROL | KeyModifiers::ALT;
        let event = normalize_basic_crossterm_key_event(KeyEvent::new(code, modifiers));

        assert_eq!(event.code, code);
        assert_eq!(event.modifiers, modifiers);
    }
}

#[test]
fn normalization_preserves_event_kind_and_state() {
    for character in ['A', '_', 'a'] {
        for kind in [
            KeyEventKind::Press,
            KeyEventKind::Repeat,
            KeyEventKind::Release,
        ] {
            let state = KeyEventState::CAPS_LOCK | KeyEventState::NUM_LOCK;
            let event = normalize_basic_crossterm_key_event(KeyEvent::new_with_kind_and_state(
                KeyCode::Char(character),
                KeyModifiers::NONE,
                kind,
                state,
            ));

            assert_eq!(event.kind, kind);
            assert_eq!(event.state, state);
        }
    }
}
