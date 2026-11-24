//! Options ▸ Files: the default template (QNEWTEMPLATE) and the font substitutes (FONTALT,
//! FONTFALLBACK) through the JSON form, the tab, the profile and Reset to Defaults.
//! (Its own test binary: these settings are process-wide, so everything runs in one test.)

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

/// An app with the startup drawing and no file dialogs.
fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// Every text shape painted in a frame, one per line, to check what the tab shows.
fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    /// Append the text of `shape` and of the shapes it groups.
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

/// Run two frames of the app and return the text they painted.
fn render(app: &mut CadApp) -> String {
    let ctx = egui::Context::default();
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut text = String::new();
    for _ in 0..2 {
        app.logic(&ctx);
        let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
        text = painted_text(&output.shapes);
        output.textures_delta.clear();
    }
    text
}

/// QNEWTEMPLATE, FONTALT and FONTFALLBACK are reported, set (GETVAR sees them), refused when
/// malformed, shown in the tab, kept by the profile and cleared by Reset.
#[test]
fn the_files_settings_are_reported_set_kept_and_reset() {
    let mut a = app();
    let r = a.run("options", json!({})).unwrap();
    for key in ["QNEWTEMPLATE", "FONTALT", "FONTFALLBACK"] {
        assert_eq!(r[key], json!(""), "{key} is reported, empty by default");
    }

    let r = a
        .run("options", json!({ "qnewtemplate": " /tmp/cc-office.dwt ", "FONTALT": "DejaVu Sans", "FontFallback": "Noto Sans; Noto Sans CJK" }))
        .unwrap();
    assert_eq!(r["QNEWTEMPLATE"], json!("/tmp/cc-office.dwt"));
    assert_eq!(r["FONTALT"], json!("DejaVu Sans"));
    assert_eq!(r["FONTFALLBACK"], json!("Noto Sans, Noto Sans CJK"));
    // The engine's variables are the same settings.
    let var = |a: &mut CadApp, name: &str| a.session.execute("getvar", &json!({ "name": name })).unwrap()["value"].clone();
    assert_eq!(var(&mut a, "QNEWTEMPLATE"), json!("/tmp/cc-office.dwt"));
    assert_eq!(var(&mut a, "FONTALT"), json!("DejaVu Sans"));

    // Refused values change nothing.
    for bad in
        [json!({ "FONTALT": 3 }), json!({ "QNEWTEMPLATE": "a\nb" }), json!({ "QNEWTEMPLATE": "x".repeat(2000) }), json!({ "FONTFALLBACK": null })]
    {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(var(&mut a, "QNEWTEMPLATE"), json!("/tmp/cc-office.dwt"));

    // The Files tab shows them.
    a.ui.dialog = Some("options:locations".into());
    let text = render(&mut a);
    for want in ["Files", "Default template for new drawings", "/tmp/cc-office.dwt", "Alternate font", "DejaVu Sans", "Fallback fonts"] {
        assert!(text.contains(want), "{want}: {text}");
    }

    // The profile keeps them; Reset to Defaults clears them.
    let saved = a.profile_json();
    a.run("options", json!({ "reset": true })).unwrap();
    assert_eq!(var(&mut a, "QNEWTEMPLATE"), json!(""));
    assert_eq!(var(&mut a, "FONTFALLBACK"), json!(""));
    let mut b = app();
    b.load_profile(&saved);
    let r = b.run("options", json!({})).unwrap();
    assert_eq!((r["QNEWTEMPLATE"].clone(), r["FONTALT"].clone()), (json!("/tmp/cc-office.dwt"), json!("DejaVu Sans")));
    // A hand-edited profile with a bad entry keeps the setting as it was.
    b.load_profile(r#"{"version":1,"QNEWTEMPLATE":["x"],"FONTALT":"a\u0007b"}"#);
    assert_eq!(var(&mut b, "QNEWTEMPLATE"), json!("/tmp/cc-office.dwt"));
    assert_eq!(var(&mut b, "FONTALT"), json!("DejaVu Sans"));
    b.run("options", json!({ "reset": true })).unwrap();
}
