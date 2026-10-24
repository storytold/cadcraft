//! Quick Properties shows AutoCAD's default rows for text, mtext and dimensions, from the rows
//! `properties.describe` flags `quick`, edited as the Properties palette edits them: for one
//! object or several of one type (with *VARIES*). Other objects keep their own rows.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{Dimension, EntityKind};
use cadcraft_ui_egui::{CadApp, Services};
use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use serde_json::json;

/// Quick Properties for everything `setup` draws, selected; drawn with the GPU renderer and the
/// dark theme when `render`, for pictures.
fn quick(setup: impl FnOnce(&mut CadApp), render: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.start_tab = false;
    app.session.settings.qpmode = true;
    setup(&mut app);
    let all: Vec<String> = app.session.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    app.run("select", json!({ "handles": all })).unwrap();
    let builder = Harness::builder().with_size(vec2(700.0, 600.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        move |ui, app: &mut CadApp| {
            if render {
                cadcraft_ui_egui::theme::apply(ui.ctx(), egui::Theme::Dark, false);
            }
            app.logic(&ui.ctx().clone());
            cadcraft_ui_egui::quick::quick_properties(app, ui.ctx());
        },
        app,
    );
    h.run_steps(4);
    h
}

fn has_label(h: &Harness<'static, CadApp>, label: &str) -> bool {
    h.query_all(by().label(label)).next().is_some()
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(Role::TextInput).value(shown))
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

fn text_heights(app: &CadApp) -> Vec<f64> {
    app.session.doc().unwrap().model.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.height) } else { None }).collect()
}

#[test]
fn text_shows_autocads_quick_rows() {
    let mut h = quick(
        |app| {
            app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 0.25 })).unwrap();
        },
        false,
    );
    for label in ["Layer", "Contents", "Style", "Annotative", "Justification", "Height", "Rotation"] {
        assert!(has_label(&h, label), "{label}");
    }
    // The rest stays in the Properties palette.
    for label in ["Color", "Linetype", "Width factor", "Obliquing"] {
        assert!(!has_label(&h, label), "{label}");
    }
    type_into(&mut h, "0.2500", "0.5");
    assert_eq!(text_heights(h.state()), [0.5]);
    type_into(&mut h, "Note", "Changed");
    let d = h.state().session.doc().unwrap();
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "Changed")));
}

#[test]
fn the_layer_is_picked_from_the_drawings_layers() {
    let mut h = quick(
        |app| {
            app.run("layer.new", json!({ "name": "Notes", "current": false })).unwrap();
            app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 0.25 })).unwrap();
        },
        false,
    );
    h.get(by().label("0").role(Role::Button)).click();
    h.run_steps(3);
    h.get(by().label("Notes").role(Role::Button)).click();
    h.run_steps(3);
    let d = h.state().session.doc().unwrap();
    assert!(d.model.iter().all(|e| e.common.layer == "Notes"));
}

#[test]
fn several_texts_edit_together() {
    // The same contents, different heights: only the height varies.
    let mut h = quick(
        |app| {
            app.run("text", json!({ "at": [0, 0], "text": "a", "height": 1 })).unwrap();
            app.run("text", json!({ "at": [0, 5], "text": "a", "height": 2 })).unwrap();
        },
        false,
    );
    assert!(has_label(&h, "Text (2)") || has_label(&h, "Text"), "titled by type");
    type_into(&mut h, "*VARIES*", "3");
    assert_eq!(text_heights(h.state()), [3.0, 3.0]);
}

#[test]
fn mtext_shows_its_text_height() {
    let mut h = quick(
        |app| {
            app.run("mtext", json!({ "at": [0, 0], "text": "Para", "height": 2.5, "width": 20 })).unwrap();
        },
        false,
    );
    for label in ["Contents", "Style", "Justification", "Text height", "Rotation"] {
        assert!(has_label(&h, label), "{label}");
    }
    type_into(&mut h, "2.5000", "4");
    let d = h.state().session.doc().unwrap();
    assert!(d.model.iter().any(|e| matches!(&e.kind, EntityKind::MText(t) if t.height == 4.0)));
}

#[test]
fn a_dimension_shows_its_style_measurement_and_text_override() {
    let mut h = quick(
        |app| {
            app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
        },
        false,
    );
    for label in ["Layer", "Dim style", "Annotative", "Measurement", "Text override"] {
        assert!(has_label(&h, label), "{label}");
    }
    assert!(!has_label(&h, "Text height"), "the dimension's other rows stay in the palette");
    // The measurement is read-only; the text override is edited.
    assert!(h.query(by().role(Role::TextInput).value("10.0000")).is_none(), "measurement read-only");
    type_into(&mut h, "", "<> TYP.");
    let d = h.state().session.doc().unwrap();
    let dm: Dimension = d.model.iter().find_map(|e| if let EntityKind::Dimension(dm) = &e.kind { Some(dm.clone()) } else { None }).unwrap();
    assert_eq!(dm.text, "<> TYP.");
}

#[test]
fn other_objects_keep_their_rows() {
    let h = quick(
        |app| {
            app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap();
        },
        false,
    );
    for label in ["Color", "Layer", "Linetype", "Length", "Angle"] {
        assert!(has_label(&h, label), "{label}");
    }
    let _ = h.get(by().role(Role::TextInput).value("10.0000")).accesskit_node();
}

/// With `CADCRAFT_QUICK_SHOTS=<directory>`, pictures of Quick Properties for text and a
/// dimension.
#[test]
fn pictures() {
    let Some(dir) = std::env::var_os("CADCRAFT_QUICK_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let shots: [(&str, fn(&mut CadApp)); 2] = [
        ("quick-text.png", |app| {
            app.run("text", json!({ "at": [0, 0], "text": "Note", "height": 0.25 })).unwrap();
        }),
        ("quick-dimension.png", |app| {
            app.run("dimlinear", json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap();
        }),
    ];
    for (name, setup) in shots {
        let mut h = quick(setup, true);
        h.run_steps(2);
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}
