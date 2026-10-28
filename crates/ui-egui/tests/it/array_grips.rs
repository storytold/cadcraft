//! A selected associative array shows its count and spacing arrows; clicking the column count
//! arrow and clicking further along adds columns, as one undo step.
use cadcraft_engine::Session;
use cadcraft_engine::doc::{ArrayGripKind, ArrayKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> (Harness<'static, CadApp>, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    let c = app.session.execute("circle", &json!({"center": [0, 0], "radius": 0.5})).unwrap()["handle"].clone();
    let r =
        app.session.execute("arrayrect", &json!({"handles": [c], "cols": 3, "rows": 2, "colSpacing": 3, "rowSpacing": 3, "base": [0, 0]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.execute("view.set", &json!({"center": [6, 2], "height": 24})).unwrap();
    app.session.set_selection(vec![h]);
    let mut harness = Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    (harness, h)
}

fn click(h: &mut Harness<'_, CadApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn cols(h: &Harness<'_, CadApp>, a: Handle) -> u32 {
    let d = h.state().session.doc().unwrap();
    match d.array_params(d.entity(a).unwrap()).unwrap().kind {
        ArrayKind::Rect(r) => r.cols,
        ArrayKind::Polar(_) => 0,
    }
}

fn shot(h: &mut Harness<'_, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os("CADCRAFT_ARRAY_GRIP_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

#[test]
fn dragging_the_column_count_arrow_adds_columns() {
    let (mut h, a) = harness();
    shot(&mut h, "array-grips.png");
    let xf = h.state().canvas.xf.unwrap();
    let gs = h.state().session.settings.gripsize as f32;
    let grip = h.state().session.doc().unwrap().array_grips(a).into_iter().find(|g| g.kind == ArrayGripKind::ColumnCount).unwrap();
    // The count arrow is drawn just beyond the last column, pointing along the row.
    let on_arrow = xf.to_screen(grip.at) + vec2(gs * 2.6, 0.0);
    click(&mut h, on_arrow);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("grip"));
    assert_eq!(h.state().canvas.array_hot.map(|(_, k)| k), Some(ArrayGripKind::ColumnCount));
    // Moving along previews the bigger array; clicking at x = 15 makes six columns.
    h.event(Event::PointerMoved(xf.to_screen(Vec2::new(13.0, 0.0))));
    h.run_steps(2);
    shot(&mut h, "array-grip-drag.png");
    click(&mut h, xf.to_screen(Vec2::new(15.2, 0.3)));
    assert!(h.state().session.running.is_none());
    assert_eq!(cols(&h, a), 6);
    h.state_mut().session.execute("undo", &json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(cols(&h, a), 3);
}

#[test]
fn an_unselected_array_shows_no_array_grips() {
    let (mut h, a) = harness();
    h.state_mut().session.set_selection(Vec::new());
    h.run_steps(2);
    let xf = h.state().canvas.xf.unwrap();
    let gs = h.state().session.settings.gripsize as f32;
    let grip = h.state().session.doc().unwrap().array_grips(a).into_iter().find(|g| g.kind == ArrayGripKind::ColumnCount).unwrap();
    click(&mut h, xf.to_screen(grip.at) + vec2(gs * 2.6, 0.0));
    assert!(h.state().canvas.array_hot.is_none());
    assert_eq!(cols(&h, a), 3);
}
