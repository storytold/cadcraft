//! #2773: a drawing whose block references expand past what a display list draws says so once in
//! the command history, instead of leaving the blocks past the limit silently missing.
use cadcraft_engine::Session;
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Common, Entity, EntityKind, Handle, Insert, Line, Space};
use cadcraft_ui_egui::{CadApp, Services};
use egui::vec2;
use egui_kittest::Harness;
use serde_json::json;

const NOTE: &str = "Some block contents are not drawn";

fn harness(copies: u32) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    // A MINSERT of `copies` × 100 copies of a block holding one line and 200 lines on a frozen
    // layer: each copy costs 202 units of the block budget and draws one line.
    let d = app.session.doc_mut().unwrap();
    d.ensure_layer("HIDDEN");
    d.layer_mut("HIDDEN").unwrap().frozen = true;
    let line = |y: f64| EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(1.0, y, 0.0) });
    let mut blk = Block::new("HEAVY");
    blk.entities.push(Entity::new(Handle(1), line(0.0)));
    for i in 0..200u64 {
        let mut e = Entity::new(Handle(i + 2), line(0.5));
        e.common.layer = "HIDDEN".into();
        blk.entities.push(e);
    }
    d.blocks.insert("HEAVY".into(), std::sync::Arc::new(blk));
    let ins = Insert {
        block: "HEAVY".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 100,
        rows: copies,
        col_spacing: 2.0,
        row_spacing: 2.0,
        clip: None,
    };
    d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
    app.session.execute("view.set", &json!({"center": [100, 100], "height": 250})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn notes(h: &Harness<'_, CadApp>) -> usize {
    h.state().session.log.iter().filter(|l| l.starts_with(NOTE)).count()
}

#[test]
fn a_drawing_past_the_block_budget_says_so_once() {
    // 100 × 100 copies: 2,020,000 units, past the 2,000,000 a list draws.
    let mut h = harness(100);
    assert!(h.state().canvas.list.as_ref().is_some_and(|l| l.truncated));
    assert_eq!(notes(&h), 1, "{:?}", h.state().session.log);
    // Zooming rebuilds the list, still cut: nothing more is said.
    for height in [60.0, 30.0, 500.0] {
        h.state_mut().session.execute("view.set", &json!({"center": [100, 100], "height": height})).unwrap();
        h.run_steps(4);
    }
    assert_eq!(notes(&h), 1, "said once per drawing, not at every rebuild");
}

#[test]
fn a_drawing_within_the_budget_says_nothing() {
    // 100 × 50 copies: 1,010,000 units.
    let h = harness(50);
    assert!(h.state().canvas.list.as_ref().is_some_and(|l| !l.truncated));
    assert_eq!(notes(&h), 0);
}
