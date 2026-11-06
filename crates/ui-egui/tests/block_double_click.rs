//! Double-clicking a block reference edits that reference in place with REFEDIT, as AutoCAD's
//! default DBLCLKEDIT does (#1609): the rest of the drawing stays visible, faded, instead of the
//! Block Editor hiding it. The Block Editor stays on the context menu and menu bar.

use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};

/// The handle a command result reports, from either the `handle` or the `handles` array.
fn handle(v: &Value) -> String {
    v.get("handle").or_else(|| v.get("handles").and_then(|a| a.get(0))).and_then(Value::as_str).expect("a handle").to_string()
}

/// A drawing with block `B` (a 10×10 rectangle) and one reference to it; `scope` is the run
/// before the reference is inserted (to choose model or paper space).
fn harness(scope: impl FnOnce(&mut CadApp), scale: Value) -> (Harness<'static, CadApp>, String) {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    let r = app.run("rectang", json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let r = handle(&r);
    app.run("block", json!({ "name": "B", "base": [0, 0], "handles": [r], "keep": "delete" })).unwrap();
    scope(&mut app);
    let i = app.run("insert", json!({ "name": "B", "at": [100, 0], "scale": scale })).unwrap();
    let i = handle(&i);
    app.run("line", json!({ "points": [[0, 50], [10, 50]] })).unwrap();
    app.session.execute("view.set", &json!({ "center": [55, 10], "height": 120 })).unwrap();
    app.canvas.zoom_pending = false;
    // 60 frames a second: the default step (0.25 s) spreads two clicks past the double-click delay.
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    (h, i)
}

/// Send two primary-button clicks at world point `at` through the harness, one frame per press
/// and release, so egui sees them as a double-click rather than two separate clicks.
fn double_click(h: &mut Harness<'_, CadApp>, at: Vec2) {
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(at);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

/// Guards #1609: double-clicking a reference starts REFEDIT on that reference in both model and
/// paper space, hiding it and standing its geometry in the drawing, without opening the Block
/// Editor dialog (which would take the drawing away) or leaving an error behind.
#[test]
fn double_click_on_a_block_reference_edits_it_in_place() {
    for paper in [false, true] {
        let (mut h, ins) = harness(
            |app| {
                if paper {
                    app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
                }
            },
            json!(1.0),
        );
        // Pick the reference on its outline (the left edge of its 10×10 rectangle).
        double_click(&mut h, Vec2::new(100.0, 5.0));
        let d = h.state().session.doc().unwrap();
        let r = d.refedit.as_ref().unwrap_or_else(|| panic!("no REFEDIT after a double-click (scope {})", if paper { "paper" } else { "model" }));
        assert_eq!(r.insert.hex(), ins, "the clicked reference is being edited in place");
        // The reference is hidden and its geometry stands in the drawing as the working set.
        assert!(!d.entity(r.insert).unwrap().common.visible);
        assert!(!r.items.is_empty());
        assert!(h.state().session.running.is_none(), "the JSON form needs no prompts");
        // The edit is in place: no Block Editor dialog took the drawing away.
        assert_ne!(h.state().ui.dialog.as_deref(), Some("bedit"));
        assert!(h.state().session.log.iter().all(|l| !l.contains("refedit:")), "{:?}", h.state().session.log);
    }
}

/// Guards that the new double-click case is narrow: a double-click over empty space starts no
/// REFEDIT session and no command.
#[test]
fn double_click_on_empty_space_does_nothing() {
    let (mut h, _) = harness(|_| {}, json!(1.0));
    double_click(&mut h, Vec2::new(50.0, 40.0));
    assert!(h.state().session.doc().unwrap().refedit.is_none());
    assert!(h.state().session.running.is_none());
}

/// Guards the never-crash rule: a reference REFEDIT refuses (here unequal X and Y scale) leaves
/// no editing session, echoes the reason on the command line, and does not panic.
#[test]
fn a_reference_refedit_refuses_leaves_an_error_and_does_not_panic() {
    // Unequal X and Y scale: REFEDIT can't map the working set back into the block.
    let (mut h, _) = harness(|_| {}, json!([2.0, 1.0]));
    double_click(&mut h, Vec2::new(100.0, 5.0));
    assert!(h.state().session.doc().unwrap().refedit.is_none());
    assert!(h.state().session.log.iter().any(|l| l.contains("scaled equally in X and Y")), "the refusal is echoed: {:?}", h.state().session.log);
}
