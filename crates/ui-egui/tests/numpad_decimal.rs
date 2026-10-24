//! The keypad's decimal key types a point, also where the system sends a comma for it.

use cadcraft_ui_egui::numpad_decimal_point;
use egui::{Event, Key, Modifiers};

fn key(key: Key, physical_key: Option<Key>) -> Event {
    Event::Key { key, physical_key, pressed: true, repeat: false, modifiers: Modifiers::default() }
}

fn text(e: &[Event]) -> String {
    e.iter().filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None }).collect()
}

#[test]
fn the_keypad_comma_becomes_a_point_the_main_comma_stays() {
    let mut e = vec![
        key(Key::Num2, Some(Key::Num2)),
        Event::Text("2".into()),
        // Keypad decimal key, German layout: no physical key, a comma typed.
        key(Key::Comma, None),
        Event::Text(",".into()),
        key(Key::Num5, Some(Key::Num5)),
        Event::Text("5".into()),
        // Main keyboard comma: the coordinate separator.
        key(Key::Comma, Some(Key::Comma)),
        Event::Text(",".into()),
        Event::Text("3".into()),
    ];
    numpad_decimal_point(&mut e);
    assert_eq!(text(&e), "2.5,3");
}

#[test]
fn only_the_comma_right_after_the_keypad_key_changes() {
    let mut e = vec![key(Key::Comma, None), key(Key::A, Some(Key::A)), Event::Text(",".into())];
    numpad_decimal_point(&mut e);
    assert_eq!(text(&e), ",");
    // A point layout sends a point already: nothing to change.
    let mut e = vec![key(Key::Period, None), Event::Text(".".into())];
    numpad_decimal_point(&mut e);
    assert_eq!(text(&e), ".");
}
