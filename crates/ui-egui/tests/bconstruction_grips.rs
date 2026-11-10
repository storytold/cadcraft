//! A hidden construction object restored to the selection must not paint grips.

use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Session, View};
use cadcraft_ui_egui::{CadApp, Services, theme::Tokens};
use serde_json::json;

fn editor_with_hidden_guide() -> (CadApp, Handle, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    let regular = app.run("line", json!({ "points": [[0, 0], [100, 0]] })).unwrap()["handles"][0].clone();
    let guide = app.run("line", json!({ "points": [[0, 30], [100, 30]] })).unwrap()["handles"][0].clone();
    app.run("block", json!({ "name": "Door", "base": [0, 0], "handles": [regular, guide] })).unwrap();
    app.run("bedit", json!({ "name": "Door" })).unwrap();
    let handles = app.session.doc().unwrap().model.handles();
    assert_eq!(handles.len(), 2);
    let (regular, guide) = (handles[0], handles[1]);
    app.run("bconstruction", json!({ "mode": "convert", "handles": [guide.hex()] })).unwrap();
    app.run("bconstruction", json!({ "mode": "hide" })).unwrap();
    app.session.set_selection(Vec::new());
    (app, regular, guide)
}

fn count_grips(shape: &egui::Shape, canvas: egui::Rect, color: egui::Color32) -> usize {
    match shape {
        egui::Shape::Rect(rect) => usize::from(rect.fill == color && canvas.contains(rect.rect.center())),
        egui::Shape::Vec(shapes) => shapes.iter().map(|shape| count_grips(shape, canvas, color)).sum(),
        _ => 0,
    }
}

fn painted_grips(app: &mut CadApp, ctx: &egui::Context) -> usize {
    app.logic(ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        events: vec![egui::Event::PointerGone],
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let canvas = app.canvas.rect.unwrap();
    let color = Tokens::get().grip;
    output.shapes.iter().map(|shape| count_grips(&shape.shape, canvas, color)).sum()
}

#[test]
fn a_hidden_construction_selection_paints_no_grips_while_an_ordinary_selection_does() {
    let (mut app, regular, guide) = editor_with_hidden_guide();
    let ctx = egui::Context::default();
    // Let the dock layout establish the actual aspect before choosing a generous fixed view.
    painted_grips(&mut app, &ctx);
    painted_grips(&mut app, &ctx);
    assert!(app.session.state_mut().unwrap().set_view(View { center: Vec2::new(50.0, 15.0), height: 120.0, twist: 0.0 }));
    app.session.set_selection(vec![regular]);
    painted_grips(&mut app, &ctx);
    let ordinary_grips = painted_grips(&mut app, &ctx);
    let state = app.session.state().unwrap();
    let xf = app.canvas.xf.unwrap();
    assert!(state.doc.entity(regular).unwrap().kind.grips().iter().all(|point| xf.rect.contains(xf.to_screen(*point))));
    assert!(ordinary_grips >= 2, "the counter must find ordinary selected-line grips, got {ordinary_grips}");

    // Undo/Redo or another selection source can restore a handle that is still hidden.
    app.session.set_selection(vec![guide]);
    painted_grips(&mut app, &ctx);
    let hidden_grips = painted_grips(&mut app, &ctx);
    assert!(app.session.selection().contains(&guide), "the forced hidden selection must reach the paint loop");
    let state = app.session.state().unwrap();
    assert!(!state.doc.is_visible(state.doc.entity(guide).unwrap()));
    let xf = app.canvas.xf.unwrap();
    assert!(state.doc.entity(guide).unwrap().kind.grips().iter().all(|point| xf.rect.contains(xf.to_screen(*point))));
    assert_eq!(hidden_grips, 0, "hidden construction leaked blue grip rectangles into the canvas");
}
