use cadcraft_engine::Session;
use serde_json::{Value, json};

use super::bindings::{Binding, CALIBRATE, MAX_BUTTONS, default_buttons, fired};
use super::{Settings, Status};
use crate::{CadApp, Services};

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn set(app: &mut CadApp, settings: Value) -> Result<Value, String> {
    app.run("ui.spacemouse", json!({"settings": settings}))
}

#[test]
fn defaults_reproduce_the_old_hard_coded_buttons() {
    let settings = Settings::default();
    assert_eq!(settings.buttons, default_buttons());
    assert!(!settings.invert_pan_x && !settings.invert_pan_y);
    // Button 1 zoomed to the extents, button 2 recalibrated, and nothing else did anything.
    let mut app = app();
    let pressed = |mask| fired(&app.spacemouse.settings.buttons, mask, true).iter().map(|b| (b.command.clone(), b.run_params())).collect::<Vec<_>>();
    assert_eq!(pressed(1), [("zoom".to_string(), json!({"mode": "extents"}))]);
    assert_eq!(pressed(2), [(CALIBRATE.to_string(), json!({}))]);
    assert!(pressed(!3).is_empty());
    // The zoom binding runs, and so does the same call the old code made.
    let zoom = app.spacemouse.settings.buttons[&1].clone();
    assert!(app.run(&zoom.command, zoom.run_params()).is_ok());
}

#[test]
fn settings_round_trip_through_json() {
    let mut settings = Settings { enabled: false, pan_speed: 1.25, invert_pan_y: true, ..Settings::default() };
    settings.buttons.insert(3, Binding::new("undo"));
    settings.buttons.insert(32, Binding::with_params("zoom", json!({"mode": "in"})));
    settings.buttons.remove(&1);
    let wire = serde_json::to_value(&settings).expect("serialises");
    assert_eq!(wire["buttons"]["3"], json!({"command": "undo"}), "empty params are not written");
    assert_eq!(wire["buttons"]["32"]["params"], json!({"mode": "in"}));
    assert!(wire["buttons"].get("1").is_none(), "an unbound button is absent");
    assert_eq!(wire["invertPanY"], json!(true), "camelCase like the other fields");
    let mut state = super::State::default();
    state.load_settings(&wire).expect("what was written loads");
    assert_eq!(state.settings, settings);
}

#[test]
fn an_old_prefs_blob_still_loads_with_the_default_buttons() {
    // What earlier versions saved: no buttons, no inversions.
    let old = r#"{"theme":"dark","spacemouse":{"enabled":false,"panSpeed":1.5,"zoomSpeed":2.0,"reverseZoom":true}}"#;
    let mut app = app();
    app.load_prefs(old);
    let settings = &app.spacemouse.settings;
    assert!(!settings.enabled && settings.reverse_zoom);
    assert_eq!((settings.pan_speed, settings.zoom_speed), (1.5, 2.0));
    assert_eq!(settings.buttons, default_buttons());
    assert!(!settings.invert_pan_x && !settings.invert_pan_y);
    // A blob that predates every field still means "the defaults".
    let mut bare = super::State::default();
    bare.load_settings(&json!({})).expect("empty settings");
    assert_eq!(bare.settings, Settings::default());
}

#[test]
fn prefs_keep_default_settings_out_and_round_trip_changes() {
    let mut app = app();
    let prefs: Value = serde_json::from_str(&app.prefs_json()).expect("json");
    assert!(prefs.get("spacemouse").is_none(), "default settings write nothing new");

    set(&mut app, json!({"buttons": {"1": {"command": "zoom.in"}}})).expect("valid");
    let saved = app.prefs_json();
    let mut restored = self::app();
    restored.load_prefs(&saved);
    assert_eq!(restored.spacemouse.settings, app.spacemouse.settings);
    assert_eq!(restored.spacemouse.settings.buttons.get(&2), None, "the table replaces the defaults");

    // Resetting to the defaults drops the entry again.
    set(&mut app, json!({})).expect("defaults");
    let prefs: Value = serde_json::from_str(&app.prefs_json()).expect("json");
    assert!(prefs.get("spacemouse").is_none());
}

#[test]
fn saved_bindings_that_no_longer_validate_are_dropped_alone() {
    let saved = json!({"spacemouse": {
        "panSpeed": 2.0,
        "buttons": {
            "1": {"command": "zoom.out"},
            "2": {"command": "a.command.removed.in.a.later.version"},
            "3": {"command": "ui.spacemouse.navigate"},
            "4": {"command": "zoom", "params": [1, 2]},
            "5": null,
            "6": "nonsense",
            "99": {"command": "undo"},
            "x": {"command": "undo"},
            "7": {"command": "undo"}
        }
    }});
    let mut app = app();
    app.load_prefs(&saved.to_string());
    assert_eq!(app.spacemouse.settings.pan_speed, 2.0, "the sensitivity survives");
    let kept: Vec<_> = app.spacemouse.settings.buttons.iter().map(|(i, b)| (*i, b.command.as_str())).collect();
    assert_eq!(kept, [(1, "zoom.out"), (7, "undo")]);
    // Buttons of the wrong shape fall back to the defaults.
    let mut app = self::app();
    app.load_prefs(&json!({"spacemouse": {"buttons": [1, 2, 3]}}).to_string());
    assert_eq!(app.spacemouse.settings.buttons, default_buttons());
    // Invalid sensitivities still reject the lot, as before.
    let mut app = self::app();
    app.load_prefs(&json!({"spacemouse": {"panSpeed": 99.0, "buttons": {"1": {"command": "undo"}}}}).to_string());
    assert_eq!(app.spacemouse.settings, Settings::default());
}

#[test]
fn the_summary_reports_the_bindings_and_updates_apply() {
    let mut app = app();
    let summary = app.run("ui.spacemouse", json!({"settings": {}})).expect("summary");
    assert_eq!(summary["settings"]["buttons"]["1"], json!({"command": "zoom", "params": {"mode": "extents"}}));
    assert_eq!(summary["settings"]["buttons"]["2"], json!({"command": "ui.spacemouse.calibrate"}));
    assert_eq!(summary["settings"]["invertPanX"], json!(false));
    assert_eq!(summary["highestButton"], json!(0));

    let summary = set(&mut app, json!({"buttons": {"3": {"command": "Undo"}, "4": null}, "invertPanX": true})).expect("update");
    assert_eq!(summary["settings"]["buttons"], json!({"3": {"command": "undo"}}), "ids are normalised, null is unbound");
    assert!(app.spacemouse.settings.invert_pan_x);
    // The summary can be read back without changing anything.
    let again = app.run("ui.spacemouse", json!({})).expect("read");
    assert_eq!(again["settings"], summary["settings"]);
}

#[test]
fn a_refused_update_changes_nothing() {
    let mut app = app();
    let before = app.spacemouse.settings.clone();
    let big = "x".repeat(10_000);
    let mut deep = json!(1);
    for _ in 0..20 {
        deep = json!({"a": deep});
    }
    let bad = [
        (json!({"buttons": {"1": {"command": "no.such.command"}}}), "not a command"),
        (json!({"buttons": {"1": {"command": "ui.spacemouse.navigate"}}}), "not a command"),
        (json!({"buttons": {"1": {"command": "ui.spacemouse", "params": {"settings": {}}}}}), "not a command"),
        (json!({"buttons": {"1": {"command": "zoom", "params": [1]}}}), "JSON object"),
        (json!({"buttons": {"1": {"command": "zoom", "params": "text"}}}), "JSON object"),
        (json!({"buttons": {"1": {"command": "zoom", "params": {"text": big}}}}), "too large"),
        (json!({"buttons": {"1": {"command": "zoom", "params": deep}}}), "too large or nested"),
        (json!({"buttons": {"0": {"command": "undo"}}}), "out of range"),
        (json!({"buttons": {"33": {"command": "undo"}}}), "out of range"),
        (json!({"enabled": true, "buttons": {"1": {"command": "undo"}, "2": {"command": ""}}}), "characters"),
        (json!({"panSpeed": 50.0}), "sensitivity"),
    ];
    for (settings, expected) in bad {
        let error = set(&mut app, settings.clone()).expect_err(&settings.to_string());
        assert!(error.contains(expected), "{error} should mention {expected:?}");
        assert_eq!(app.spacemouse.settings, before, "{settings}");
    }
}

#[test]
fn hostile_json_through_the_control_method_is_an_error_not_a_panic() {
    let mut app = app();
    let before = app.spacemouse.settings.clone();
    let hostile = [
        json!([]),
        json!("buttons"),
        json!(7),
        json!(true),
        json!({"buttons": []}),
        json!({"buttons": "1"}),
        json!({"buttons": 5}),
        json!({"buttons": {"1": "undo"}}),
        json!({"buttons": {"1": 7}}),
        json!({"buttons": {"1": []}}),
        json!({"buttons": {"1": {}}}),
        json!({"buttons": {"1": {"command": 7}}}),
        json!({"buttons": {"1": {"command": null}}}),
        json!({"buttons": {"1": {"command": ["undo"]}}}),
        json!({"buttons": {"1": {"command": "undo", "unknown": 1}}}),
        json!({"buttons": {"": {"command": "undo"}}}),
        json!({"buttons": {"-1": {"command": "undo"}}}),
        json!({"buttons": {"256": {"command": "undo"}}}),
        json!({"buttons": {"1.5": {"command": "undo"}}}),
        json!({"buttons": {"99999999999999999999": {"command": "undo"}}}),
        json!({"buttons": {"one": {"command": "undo"}}}),
        json!({"buttons": {"1": {"command": "\u{0}\u{202e}\u{1F4A5}"}}}),
        json!({"buttons": {"1": {"command": "z".repeat(1_000_000)}}}),
        json!({"invertPanX": "yes"}),
        json!({"invertPanY": 1}),
        json!({"panSpeed": null, "buttons": {"1": {"command": "undo"}}}),
        json!({"zoomSpeed": "fast"}),
    ];
    for settings in hostile {
        let shown: String = settings.to_string().chars().take(80).collect();
        assert!(set(&mut app, settings).is_err(), "{shown}");
        assert_eq!(app.spacemouse.settings, before, "{shown}");
    }
    // The settings value itself may also be absent or null; that just reports.
    assert!(app.run("ui.spacemouse", json!({"settings": null})).is_err());
    assert!(app.run("ui.spacemouse", json!({"other": 1})).is_ok());
    // Every button at once is fine and stays bounded by the 32 buttons.
    let all: serde_json::Map<String, Value> = (1..=MAX_BUTTONS).map(|n| (n.to_string(), json!({"command": "zoom.extents"}))).collect();
    set(&mut app, json!({"buttons": all})).expect("32 buttons");
    assert_eq!(app.spacemouse.settings.buttons.len(), usize::from(MAX_BUTTONS));
}

#[test]
fn calibrate_stays_reachable_when_every_binding_is_cleared() {
    let mut app = app();
    set(&mut app, json!({"buttons": {}})).expect("clear all");
    assert!(app.spacemouse.settings.buttons.is_empty());
    assert!(fired(&app.spacemouse.settings.buttons, u32::MAX, true).is_empty(), "nothing is bound");
    // The dialog's Calibrate button and the command are independent of the table.
    assert!(app.run(CALIBRATE, Value::Null).is_err(), "no device yet");
    app.spacemouse.status = Status::Connected;
    assert!(app.run(CALIBRATE, Value::Null).is_ok());
    assert!(app.spacemouse.calibrate_requested);
}

#[test]
fn opening_the_dialog_and_the_navigate_command_still_work() {
    let mut app = app();
    app.run("ui.spacemouse", Value::Null).expect("opens");
    assert_eq!(app.ui.dialog.as_deref(), Some("spacemouse"));
    assert!(app.run("ui.spacemouse.navigate", json!({"pan": [0.1, 0.0], "zoom": 1.1})).is_ok());
    assert!(app.run("ui.spacemouse.navigate", json!({"pan": [9.0, 0.0], "zoom": 1.1})).is_err());
}
