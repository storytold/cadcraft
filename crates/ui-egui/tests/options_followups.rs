//! OPTIONS follow-ups (#626): Open and Save shows automatic save (SAVETIME, SAVEFILEPATH) and the
//! default save version, User Preferences the time-sensitive right-click, all in the JSON form.
//! (Its own test binary: SAVETIME and SAVEFILEPATH are process-wide.)

use cadcraft_engine::Session;
use cadcraft_engine::autosave;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let mut text = String::new();
    shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    text
}

/// One frame at `time` with `events`, returning the painted text.
fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64) -> String {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        time: Some(time),
        events,
        ..Default::default()
    };
    app.logic(ctx);
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    painted_text(&output.shapes)
}

#[test]
fn autosave_save_version_and_right_click_in_options() {
    let mut a = app();
    let all = a.run("options", json!({})).unwrap();
    for key in ["SAVETIME", "SAVEFILEPATH", "saveVersion", "timeSensitiveRightClick"] {
        assert!(all.get(key).is_some(), "{key} is reported");
    }
    assert_eq!(all["timeSensitiveRightClick"], json!(true));

    let r = a
        .run("options", json!({"SAVETIME": 25, "savefilepath": " /tmp/cc-autosave ", "saveVersion": 2010, "timeSensitiveRightClick": false}))
        .unwrap();
    assert_eq!((r["SAVETIME"].clone(), r["SAVEFILEPATH"].clone()), (json!(25), json!("/tmp/cc-autosave")));
    assert_eq!((r["saveVersion"].clone(), r["timeSensitiveRightClick"].clone()), (json!("2010"), json!(false)));
    assert_eq!((autosave::savetime(), autosave::savefilepath()), (25, "/tmp/cc-autosave".to_string()));
    assert_eq!(a.run("ui.saveversion", json!({})).unwrap()["saveVersion"], "2010", "the same setting as File ▸ Default Save Format");

    // All or nothing.
    let before = a.run("options", json!({})).unwrap();
    for bad in [
        json!({"SAVETIME": 601, "PICKBOX": 9}),
        json!({"SAVETIME": -1}),
        json!({"SAVETIME": "often"}),
        json!({"SAVEFILEPATH": "a\nb", "SAVETIME": 5}),
        json!({"SAVEFILEPATH": "x".repeat(2000)}),
        json!({"saveVersion": "2005", "SAVETIME": 5}),
        json!({"timeSensitiveRightClick": "yes", "SAVETIME": 5}),
        json!({"SAVETIME": 5, "LUNITS": 4}),
    ] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
        assert_eq!(a.run("options", json!({})).unwrap(), before, "{bad} changed nothing");
    }

    // The right-click setting survives a restart; reset puts it and the save version back.
    let mut b = app();
    b.load_profile(&a.profile_json());
    assert!(!b.options.time_sensitive_right_click);
    let r = b.run("options", json!({"reset": true})).unwrap();
    assert_eq!((r["timeSensitiveRightClick"].clone(), r["saveVersion"].clone()), (json!(true), json!("2018")));

    // The tabs show the settings.
    let ctx = egui::Context::default();
    a.ui.dialog = Some("options:files".into());
    frame(&mut a, &ctx, Vec::new(), 0.0);
    let text = frame(&mut a, &ctx, Vec::new(), 0.1);
    for l in ["Default save version", "2010", "Automatic save every", "\n25\n", "Automatic save file location"] {
        assert!(text.contains(l), "{l} missing:\n{text}");
    }
    a.ui.dialog = Some("options:preferences".into());
    frame(&mut a, &ctx, Vec::new(), 0.2);
    assert!(frame(&mut a, &ctx, Vec::new(), 0.3).contains("Time-sensitive right-click"));

    // Turned off, a quick right-click during a command opens the menu instead of acting as Enter.
    a.ui.dialog = None;
    let ctx = egui::Context::default();
    frame(&mut a, &ctx, Vec::new(), 0.0);
    let at = egui::pos2(800.0, 500.0);
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE };
    frame(&mut a, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
    a.cmdline("LINE 0,0 10,0");
    frame(&mut a, &ctx, vec![button(true)], 1.0);
    frame(&mut a, &ctx, vec![button(false)], 1.05);
    let text = frame(&mut a, &ctx, Vec::new(), 1.1);
    assert!(text.contains("Cancel") && text.contains("Snap Overrides"), "the menu opens:\n{text}");
    assert!(a.session.prompt_text().contains("next point"), "no Enter: {}", a.session.prompt_text());
}

#[test]
fn interface_scale_in_options() {
    let mut a = app();
    assert_eq!(a.run("options", json!({})).unwrap()["interfaceScale"], json!(1.0));

    let r = a.run("options", json!({"interfaceScale": 1.5})).unwrap();
    assert_eq!(r["interfaceScale"], json!(1.5));
    let ctx = egui::Context::default();
    frame(&mut a, &ctx, Vec::new(), 0.0);
    assert_eq!(ctx.zoom_factor(), 1.5, "applied as egui's zoom factor");

    // Refused all or nothing.
    let before = a.run("options", json!({})).unwrap();
    for bad in [
        json!({"interfaceScale": 0.4, "PICKBOX": 9}),
        json!({"interfaceScale": 3.1}),
        json!({"interfaceScale": "big", "snapMarkerSize": 20}),
        json!({"interfaceScale": null}),
        json!({"interfaceScale": f64::NAN}),
    ] {
        let err = a.run("options", bad.clone()).expect_err(&bad.to_string());
        assert!(err.to_string().contains("interfaceScale must be from 0.5 to 3"), "{bad}: {err}");
        assert_eq!(a.run("options", json!({})).unwrap(), before, "{bad} changed nothing");
    }

    // It survives a restart; an out-of-range saved value is skipped.
    let mut b = app();
    b.load_profile(&a.profile_json());
    assert_eq!(b.options.interface_scale, 1.5);
    let mut c = app();
    c.load_profile(&a.profile_json().replace("\"interfaceScale\":1.5", "\"interfaceScale\":9.0"));
    assert_eq!(c.options.interface_scale, 1.0);

    // Reset brings back the system's own size.
    assert_eq!(a.run("options", json!({"reset": true})).unwrap()["interfaceScale"], json!(1.0));
    frame(&mut a, &ctx, Vec::new(), 0.1);
    assert_eq!(ctx.zoom_factor(), 1.0);

    // The Display tab shows it.
    a.run("options", json!({"interfaceScale": 1.25})).unwrap();
    a.ui.dialog = Some("options:display".into());
    frame(&mut a, &ctx, Vec::new(), 0.2);
    let text = frame(&mut a, &ctx, Vec::new(), 0.3);
    assert!(text.contains("Interface scale") && text.contains("125 %"), "{text}");
}
