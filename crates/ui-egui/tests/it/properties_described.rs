//! The Properties palette's text, mtext and dimension rows come from `properties.describe`: a
//! dimension's text height is an override of that dimension, several objects of one type edit
//! together, styles are picked from the drawing's, and a fixed-height text style shows the
//! height it fixes instead of a field that would change nothing.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind, MText};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::json;

/// The palettes with `setup` run on a new drawing, then everything in it selected.
fn palettes(setup: impl FnOnce(&mut CadApp)) -> Harness<'static, CadApp> {
    harness(setup, false)
}

/// `palettes`, drawn with the GPU renderer and the dark theme when `render`, for pictures.
fn harness(setup: impl FnOnce(&mut CadApp), render: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    setup(&mut app);
    let all: Vec<String> = app.session.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    app.run("select", json!({ "handles": all })).unwrap();
    let builder = Harness::builder().with_size(vec2(400.0, 3000.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        move |ui, app: &mut CadApp| {
            if render {
                cadcraft_ui_egui::theme::apply(ui.ctx(), egui::Theme::Dark, false);
            }
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::palettes::right_palettes(app, ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(egui::accesskit::Role::TextInput).value(shown))
}

fn type_into(h: &mut Harness<'static, CadApp>, shown: &str, text: &str) {
    field(h, shown).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(h, shown).type_text(text);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn dims(app: &CadApp) -> Vec<Dimension> {
    app.session.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Dimension(d) = &e.kind { Some(d.clone()) } else { None }).collect()
}

fn mtext(app: &CadApp) -> MText {
    app.session.doc().unwrap().model.iter().find_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t.clone()) } else { None }).unwrap()
}

fn heights(app: &CadApp) -> Vec<f64> {
    app.session.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.height) } else { None }).collect()
}

#[test]
fn a_dimension_text_height_is_an_override_of_that_dimension() {
    let mut h = palettes(|app| {
        // A text height no other row has, so its field is easy to find.
        app.run("dimstyle", json!({ "name": "Standard", "DIMTXT": 0.123 })).unwrap();
        app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    });
    type_into(&mut h, "0.1230", "0.25");
    let d = &dims(h.state())[0];
    assert_eq!(d.overrides.get("textHeight"), Some(&json!(0.25)));
    field(&h, "0.2500");
    // The style keeps its own height.
    let st = h.state_mut().run("dimstyle.list", json!({ "name": "Standard" })).unwrap();
    assert_eq!(st["variables"]["DIMTXT"], 0.123);
}

#[test]
fn several_texts_edit_together() {
    let mut h = palettes(|app| {
        app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 1 })).unwrap();
        app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 2 })).unwrap();
    });
    type_into(&mut h, "*VARIES*", "3");
    assert_eq!(heights(h.state()), [3.0, 3.0]);
}

#[test]
fn an_mtext_style_is_picked_from_the_drawing_styles() {
    let mut h = palettes(|app| {
        app.run("style", json!({ "name": "Notes", "current": false })).unwrap();
        app.run("mtext", json!({ "at": [0, 0], "text": "One", "height": 2.5 })).unwrap();
    });
    h.get_by_label("Standard").click();
    h.run_steps(3);
    h.get_by_label("Notes").click();
    h.run_steps(3);
    assert_eq!(mtext(h.state()).style, "Notes");
    // Justify is a choice too.
    h.get_by_label("Top left").click();
    h.run_steps(3);
    h.get_by_label("Middle center").click();
    h.run_steps(3);
    assert_eq!(mtext(h.state()).attach, 5);
}

#[test]
fn a_fixed_height_text_style_shows_the_height_it_fixes() {
    let h = palettes(|app| {
        app.run("style", json!({ "name": "Fixed", "height": 0.5, "current": false })).unwrap();
        app.run("dimstyle", json!({ "name": "FixedDims", "textStyle": "Fixed", "DIMTXT": 0.123 })).unwrap();
        app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
    });
    // No field to type a height into: neither the style's fixed height nor DIMTXT.
    assert!(h.query(by().role(egui::accesskit::Role::TextInput).value("0.5000")).is_none());
    assert!(h.query(by().role(egui::accesskit::Role::TextInput).value("0.1230")).is_none());
}

#[test]
fn a_value_the_engine_refuses_is_reported() {
    let mut h = palettes(|app| {
        app.run("mtext", json!({ "at": [0, 0], "text": "One", "height": 2.5, "lineSpacing": 1.25 })).unwrap();
    });
    let before = h.state().session.log.len();
    // MTEXT line spacing factors run from 0.25 to 4.
    type_into(&mut h, "1.2500", "9");
    assert_eq!(mtext(h.state()).line_spacing, 1.25);
    let log: Vec<&String> = h.state().session.log[before..].iter().collect();
    assert!(log.iter().any(|l| l.contains("Line space factor")), "{log:?}");
}

#[test]
fn other_objects_keep_their_own_rows() {
    let h = palettes(|app| {
        app.run("line", json!({ "points": [[0, 0], [10, 5]] })).unwrap();
    });
    // End X of the line (its length is 11.1803).
    field(&h, "10.0000");
}

/// With `CADCRAFT_PROPERTIES_SHOTS=<directory>`, pictures of the palette for a dimension with a
/// text height override, an MTEXT and two TEXTs of different heights, to look at the rows.
#[test]
fn pictures_of_the_rows() {
    let Some(directory) = std::env::var_os("CADCRAFT_PROPERTIES_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    let shots: [(&str, fn(&mut CadApp)); 3] = [
        ("properties-dimension.png", |app| {
            app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
            let h = app.session.doc().unwrap().model.last().unwrap().handle.hex();
            app.run("properties.set", json!({ "handles": [h], "textHeight": 0.25 })).unwrap();
        }),
        ("properties-mtext.png", |app| {
            app.run("mtext", json!({ "at": [0, 0], "text": "One\\PTwo", "height": 2.5, "width": 40 })).unwrap();
        }),
        ("properties-texts.png", |app| {
            app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 1 })).unwrap();
            app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 2, "justify": "MC" })).unwrap();
        }),
    ];
    for (name, setup) in shots {
        let mut h = harness(setup, true);
        h.run_steps(2);
        h.render().unwrap().save(directory.join(name)).unwrap();
    }
}
