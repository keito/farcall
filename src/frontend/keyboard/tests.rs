//! Representative US 83/84-key BIOS results; see docs/keyboard-bios-83-84.md.

use super::{ModifierState, translate_winit_key};
use winit::keyboard::KeyCode;

const PLAIN: ModifierState = ModifierState {
    shift: false,
    ctrl: false,
    alt: false,
};
const SHIFT: ModifierState = ModifierState {
    shift: true,
    ..PLAIN
};
const CTRL: ModifierState = ModifierState {
    ctrl: true,
    ..PLAIN
};
const ALT: ModifierState = ModifierState { alt: true, ..PLAIN };

fn translated(
    key: KeyCode,
    text: Option<&str>,
    modifiers: ModifierState,
) -> Option<(u8, Option<u8>)> {
    translate_winit_key(key, text.map(Into::into), &modifiers).map(|key| (key.scan_code, key.ascii))
}

#[test]
fn ordinary_key_preserves_host_character_and_physical_scan_code() {
    for character in ["a", "A"] {
        assert_eq!(
            translated(KeyCode::KeyA, Some(character), PLAIN),
            Some((0x1E, Some(character.as_bytes()[0]))),
        );
    }
}

#[test]
fn shift_slash_produces_question_mark_without_changing_scan_code() {
    assert_eq!(
        translated(KeyCode::Slash, Some("/"), PLAIN),
        Some((0x35, Some(b'/'))),
    );
    assert_eq!(
        translated(KeyCode::Slash, Some("?"), SHIFT),
        Some((0x35, Some(b'?'))),
    );
}

#[test]
fn ctrl_c_produces_etx_even_without_host_text() {
    for text in [None, Some("c")] {
        assert_eq!(
            translated(KeyCode::KeyC, text, CTRL),
            Some((0x2E, Some(0x03))),
        );
    }
}

#[test]
fn f1_uses_modifier_scan_codes_with_alt_then_ctrl_then_shift_priority() {
    for (modifiers, scan_code) in [
        (PLAIN, 0x3B),
        (SHIFT, 0x54),
        (CTRL, 0x5E),
        (ALT, 0x68),
        (
            ModifierState {
                shift: true,
                ..CTRL
            },
            0x5E,
        ),
        (
            ModifierState {
                shift: true,
                ctrl: true,
                ..ALT
            },
            0x68,
        ),
    ] {
        assert_eq!(
            translated(KeyCode::F1, None, modifiers),
            Some((scan_code, Some(0))),
        );
    }
}

#[test]
fn ignored_combinations_are_distinct_from_queued_zero_characters() {
    assert_eq!(
        translated(KeyCode::Digit2, None, CTRL),
        Some((0x03, Some(0))),
    );
    assert_eq!(translated(KeyCode::Digit3, Some("3"), CTRL), None);
    // Alt+Escape is ignored even though Ctrl+Escape alone has a valid result.
    assert_eq!(
        translated(
            KeyCode::Escape,
            Some("\x1b"),
            ModifierState { ctrl: true, ..ALT }
        ),
        None,
    );
}

#[test]
fn unusable_host_text_does_not_queue_a_key() {
    // Do not manufacture an extended BIOS key from missing/invalid text.
    let results =
        [None, Some(""), Some("é"), Some("ab")].map(|text| translated(KeyCode::KeyA, text, PLAIN));
    assert_eq!(results, [None; 4]);
}
