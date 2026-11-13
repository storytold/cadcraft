//! The Options dialog's Dimensions tab and its JSON keys: `dimStyle` sets the open drawing's
//! current dimension style and is kept for new drawings, `dimDefaults` only for new drawings; a
//! refused value changes nothing; the profile keeps them across a restart.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::{Value, json};

/// An app with the startup drawing and no file dialogs.
fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// Every text shape painted in a frame, one per line, to check what a tab shows.
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
fn render(app: &mut CadApp, ctx: &egui::Context) -> String {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut text = String::new();
    for _ in 0..2 {
        app.logic(ctx);
        let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
        text = painted_text(&output.shapes);
        output.textures_delta.clear();
    }
    text
}

/// The current dimension style's variable `var` in the open drawing.
fn current(a: &mut CadApp, var: &str) -> Value {
    let name = cadcraft_ui_egui::options::current_dim_style_name(a).unwrap_or_default();
    let r = a.session.execute("dimstyle.list", &json!({ "name": name })).unwrap();
    r["variables"][var].clone()
}

/// `dimStyle` changes the open drawing's current style, and the JSON form reports it back.
#[test]
fn dim_style_sets_the_current_style_and_is_reported() {
    let mut a = app();
    let before = current(&mut a, "DIMDEC");
    let r = a.run("options", json!({ "dimStyle": { "DIMDEC": 2, "dimdsep": ",", "DIMZIN": 12 } })).unwrap();
    assert_eq!(current(&mut a, "DIMDEC"), json!(2));
    assert_eq!(current(&mut a, "DIMDSEP"), json!(","));
    assert_eq!(current(&mut a, "DIMZIN"), json!(12));
    assert_ne!(before, json!(2), "the test changes something");
    // Reported as the current style's values, and kept as the defaults for new drawings.
    assert_eq!(r["dimStyle"]["DIMDEC"], json!(2));
    assert_eq!(r["dimStyle"]["DIMDSEP"], json!(","));
    assert_eq!(r["dimDefaults"], json!({ "DIMDEC": 2, "DIMDSEP": ",", "DIMZIN": 12 }));
}

/// Defaults reach drawings made later, never drawings already open.
#[test]
fn new_drawings_take_the_defaults_and_other_drawings_keep_their_style() {
    let mut a = app();
    a.run("options", json!({ "dimStyle": { "DIMDEC": 1, "DIMDSEP": "," } })).unwrap();
    let first = a.session.active;
    a.run("new", json!({})).unwrap();
    assert_ne!(a.session.active, first);
    assert_eq!(current(&mut a, "DIMDEC"), json!(1));
    assert_eq!(current(&mut a, "DIMDSEP"), json!(","));
    // Changing the current drawing's style later leaves the first one as it was.
    a.run("options", json!({ "dimStyle": { "DIMDEC": 5 } })).unwrap();
    a.run("document.switch", json!({ "index": first })).unwrap();
    assert_eq!(current(&mut a, "DIMDEC"), json!(1));
    // `dimDefaults` is for new drawings only.
    a.run("options", json!({ "dimDefaults": { "DIMDEC": 3 } })).unwrap();
    assert_eq!(current(&mut a, "DIMDEC"), json!(1), "the open drawing keeps its style");
    a.run("new", json!({})).unwrap();
    assert_eq!(current(&mut a, "DIMDEC"), json!(3));
    // Forgotten, new drawings are as built in again.
    a.run("options", json!({ "dimDefaults": {} })).unwrap();
    a.run("new", json!({})).unwrap();
    assert_ne!(current(&mut a, "DIMDEC"), json!(3));
}

/// `dimDefaults` leaves even a fresh, untouched drawing as it is: only the profile read at
/// start-up gives it the defaults.
#[test]
fn dim_defaults_leave_the_untouched_first_drawing_alone() {
    let mut a = app();
    let before = current(&mut a, "DIMDEC");
    a.run("options", json!({ "dimDefaults": { "DIMDEC": 7 } })).unwrap();
    assert_eq!(current(&mut a, "DIMDEC"), before);
    let mut fresh = app();
    fresh.session.restore_dim_defaults(serde_json::from_value(json!({ "DIMDEC": 7 })).unwrap());
    assert_eq!(current(&mut fresh, "DIMDEC"), json!(7), "start-up: the profile's defaults reach the first drawing");
}

/// A value the dimension style refuses, or a name it doesn't have, is an error and leaves both the
/// style and the defaults as they were.
#[test]
fn refused_values_and_unknown_names_change_nothing() {
    let mut a = app();
    let before = a.run("options", json!({})).unwrap();
    for bad in [
        json!({ "dimStyle": { "DIMDEC": -1 } }),
        json!({ "dimStyle": { "DIMFOO": 1 } }),
        json!({ "dimStyle": { "DIMTXT": 0 } }),
        json!({ "dimStyle": 5 }),
        json!({ "dimDefaults": { "DIMBLK": "x" } }),
        json!({ "dimStyle": { "DIMDEC": 2 }, "PICKBOX": "nonsense" }),
    ] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
    }
    let after = a.run("options", json!({})).unwrap();
    assert_eq!(before["dimStyle"], after["dimStyle"]);
    assert_eq!(after["dimDefaults"], json!({}));
}

/// The defaults survive a restart through the profile, the startup drawing included, and Reset to
/// Defaults drops them.
#[test]
fn the_profile_keeps_the_defaults_and_reset_forgets_them() {
    let mut a = app();
    a.run("options", json!({ "dimStyle": { "DIMDEC": 2, "DIMDSEP": "," } })).unwrap();
    let saved = a.profile_json();
    let mut b = app();
    b.load_profile(&saved);
    assert_eq!(b.session.dim_defaults.get("DIMDEC"), Some(&json!(2)));
    // The startup drawing is made before the profile is read: it takes the defaults too.
    assert_eq!(current(&mut b, "DIMDEC"), json!(2));
    b.run("new", json!({})).unwrap();
    assert_eq!(current(&mut b, "DIMDSEP"), json!(","));
    // A hand-edited profile with a bad entry still restores the rest.
    let mut c = app();
    c.load_profile(r#"{"version":1,"dimDefaults":{"DIMDEC":"junk"},"systemCursor":true}"#);
    assert!(c.session.dim_defaults.is_empty());
    b.run("options", json!({ "reset": true })).unwrap();
    assert!(b.session.dim_defaults.is_empty());
}

/// Both new tabs draw their rows without a drawing error.
#[test]
fn the_dimensions_and_all_settings_tabs_render() {
    let mut a = app();
    a.ui.dialog = Some("options:dimensions".into());
    let ctx = egui::Context::default();
    let text = render(&mut a, &ctx);
    for want in
        ["Dimensions", "Current dimension style", "Linear units", "Decimal separator", "Precision (decimal places)", "Suppress trailing zeros"]
    {
        assert!(text.contains(want), "{want}: {text}");
    }
    assert_eq!(a.ui.dialog.as_deref(), Some("options:dimensions"));
    a.ui.dialog = Some("options:advanced".into());
    let text = render(&mut a, &ctx);
    for want in ["All settings", "APERTURE", "AUTOSNAP", "Filter", "Edit as JSON"] {
        assert!(text.contains(want), "{want}: {text}");
    }
}

/// The interface state is listed with the settings, and `set_ui_state` refuses a value of the wrong
/// kind without changing anything.
#[test]
fn the_all_settings_tab_lists_the_interface_state_and_set_ui_state_checks_it() {
    let mut a = app();
    // Filtered to the toolbar, the tab shows the interface state's row.
    a.ui.dialog = Some("options:advanced".into());
    let ctx = egui::Context::default();
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("cc_options_advanced_filter"), "toolbar".to_string()));
    let text = render(&mut a, &ctx);
    for want in ["Interface state", "showToolbar"] {
        assert!(text.contains(want), "{want}: {text}");
    }
    // The same call changes it, and a bad value or an invalid layout changes nothing.
    assert!(a.ui.show_toolbar);
    let patch = json!({ "showToolbar": false });
    a.set_ui_state(patch.as_object().unwrap()).unwrap();
    assert!(!a.ui.show_toolbar);
    let bad = json!({ "showToolbar": "maybe" });
    assert!(a.set_ui_state(bad.as_object().unwrap()).is_err());
    assert!(!a.ui.show_toolbar, "a refused value leaves the state as it was");
}

/// Where the text `label` is painted (its galley's top left), if anywhere.
fn text_pos(shapes: &[egui::epaint::ClippedShape], want: &dyn Fn(&str) -> bool) -> Vec<egui::Pos2> {
    /// Append where `shape`, or a shape it groups, paints text that `want` accepts.
    fn collect(shape: &egui::Shape, want: &dyn Fn(&str) -> bool, out: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Text(s) if want(s.galley.text()) => out.push(s.pos),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, want, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| collect(&s.shape, want, &mut out));
    out
}

/// Dragging a number over several frames sends nothing until release, then one `dimstyle` run: one
/// undo step.
#[test]
fn dragging_a_dimension_value_is_one_undo_step() {
    let mut a = app();
    a.ui.dialog = Some("options:dimensions".into());
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |a: &mut CadApp, events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        a.logic(&ctx);
        let mut output = ctx.run_ui(input, |ui| a.ui(ui));
        output.textures_delta.clear();
        output.shapes
    };
    frame(&mut a, Vec::new());
    let shapes = frame(&mut a, Vec::new());
    // The precision field: the number painted on the label's row, right of it.
    let label = text_pos(&shapes, &|t| t == "Precision (decimal places)");
    let label = *label.first().expect("the precision row");
    let before = current(&mut a, "DIMDEC");
    let digits = before.to_string();
    let field = text_pos(&shapes, &|t| t == digits).into_iter().find(|p| (p.y - label.y).abs() < 6.0 && p.x > label.x).expect("the precision field");
    let start = field + egui::vec2(3.0, 6.0);
    let undo_before = a.session.state().unwrap().undo.len();
    let press = |pressed, pos| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(&mut a, vec![egui::Event::PointerMoved(start)]);
    frame(&mut a, vec![press(true, start)]);
    // A drag over several frames, each of which changes the shown value.
    for step in 1..=6 {
        frame(&mut a, vec![egui::Event::PointerMoved(start - egui::vec2(step as f32 * 2.0, 0.0))]);
    }
    assert_eq!(a.session.state().unwrap().undo.len(), undo_before, "nothing is sent while the field is dragged");
    let end = start - egui::vec2(12.0, 0.0);
    frame(&mut a, vec![press(false, end)]);
    frame(&mut a, Vec::new());
    let after = current(&mut a, "DIMDEC");
    assert_ne!(after, before, "the drag changed the precision");
    assert_eq!(a.session.state().unwrap().undo.len(), undo_before + 1, "one drag, one undo step");
}
