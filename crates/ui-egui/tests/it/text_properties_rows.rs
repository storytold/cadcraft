//! The Properties palette shows a single-line text's Text style and Justify rows.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services};
use serde_json::json;

/// The text painted in one frame.
fn painted(app: &mut CadApp, ctx: &egui::Context) -> String {
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
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut text = String::new();
    for s in &output.shapes {
        collect(&s.shape, &mut text);
    }
    text
}

#[test]
fn a_texts_style_and_justification_rows_are_listed() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("text", json!({ "at": [0, 0], "text": "HELLO", "height": 2.5, "justify": "MC" })).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap()).unwrap();
    app.session.set_selection(vec![h]);
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    // (Drop-down contents are not painted shapes here, so only the row labels are checked.)
    for s in ["Contents", "Text style", "Justify", "Height", "Rotation", "Width factor"] {
        assert!(text.lines().any(|l| l == s), "{s} missing from:\n{text}");
    }
    // The rows change the text through properties.set.
    app.run("properties.set", json!({ "handles": [h.hex()], "justify": "BR", "style": "Standard" })).unwrap();
    let d = app.session.doc().unwrap();
    let Some(cadcraft_engine::doc::EntityKind::Text(t)) = d.entity(h).map(|e| &e.kind) else { panic!("text") };
    assert_eq!((t.halign, t.valign), (cadcraft_engine::doc::HAlign::Right, cadcraft_engine::doc::VAlign::Bottom));
}
