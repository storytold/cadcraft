//! Layout mode: a rectangle in the Model tab for the part of the model each layout shows; real pointer
//! gestures move and resize it, add a layout, or open one.
use cadcraft_doc::Space;
use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};

/// A drawing with a line, one layout "A" showing [0,0]-[40,30], the Model tab with layout mode on.
fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.canvas.zoom_pending = false;
    app.session.execute("line", &json!({"points": [[0, 0], [100, 0]]})).unwrap();
    app.session.execute("layout.new", &json!({"name": "A", "window": [[0, 0], [40, 30]]})).unwrap();
    app.session.execute("layout.set", &json!({"name": "Model"})).unwrap();
    app.session.execute("view.set", &json!({"center": [50, 20], "height": 120})).unwrap();
    // 60 frames a second: the default step (0.25 s) spreads two clicks past the double-click delay.
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().run("ui.toggle.layoutmode", json!({"on": true})).unwrap();
    h.run_steps(3);
    h
}

fn button(h: &mut Harness<'_, CadApp>, p: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn drag(h: &mut Harness<'_, CadApp>, a: Pos2, b: Pos2) {
    button(h, a, true);
    for i in 1..=4 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 4.0)));
        h.run_steps(1);
    }
    button(h, b, false);
    h.run_steps(3);
}

fn click(h: &mut Harness<'_, CadApp>, p: Pos2) {
    button(h, p, true);
    button(h, p, false);
    h.run_steps(2);
}

fn px(h: &Harness<'_, CadApp>, x: f64, y: f64) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(Vec2::new(x, y))
}

/// One pixel in drawing units.
fn pixel(h: &Harness<'_, CadApp>) -> f64 {
    1.0 / h.state().canvas.xf.unwrap().scale
}

fn drag_world(h: &mut Harness<'_, CadApp>, a: (f64, f64), b: (f64, f64)) {
    let (a, b) = (px(h, a.0, a.1), px(h, b.0, b.1));
    drag(h, a, b);
}

fn click_world(h: &mut Harness<'_, CadApp>, p: (f64, f64)) {
    let p = px(h, p.0, p.1);
    click(h, p);
}

fn areas(h: &mut Harness<'_, CadApp>) -> Vec<Value> {
    h.state_mut().session.execute("layout.areas", &json!({})).unwrap().as_array().unwrap().clone()
}

fn win(a: &Value) -> [f64; 4] {
    let g = |i: usize, j: usize| a["window"][i][j].as_f64().unwrap();
    [g(0, 0), g(0, 1), g(1, 0), g(1, 1)]
}

/// The paper size of layout `name`'s viewport.
fn viewport_size(h: &Harness<'_, CadApp>, name: &str) -> (f64, f64) {
    let d = h.state().session.doc().unwrap();
    let l = d.layout(name).unwrap();
    l.entities
        .iter()
        .find_map(|e| if let cadcraft_doc::EntityKind::Viewport(v) = &e.kind { (v.id != 1).then_some((v.width, v.height)) } else { None })
        .unwrap()
}

fn layouts(h: &Harness<'_, CadApp>) -> usize {
    h.state().session.doc().unwrap().layouts.len()
}

#[test]
fn dragging_the_rectangle_moves_the_area_in_one_undo_step() {
    let mut h = harness();
    let before = win(&areas(&mut h)[0]);
    let c = ((before[0] + before[2]) / 2.0, (before[1] + before[3]) / 2.0);
    let (from, to) = (px(&h, c.0, c.1), px(&h, c.0 + 10.0, c.1 + 5.0));
    drag(&mut h, from, to);
    let after = win(&areas(&mut h)[0]);
    let tol = 2.0 * pixel(&h);
    assert!((after[0] - before[0] - 10.0).abs() < tol && (after[1] - before[1] - 5.0).abs() < tol, "{before:?} -> {after:?}");
    assert!(((after[2] - after[0]) - (before[2] - before[0])).abs() < 1e-9, "a move keeps the size");
    h.state_mut().session.undo().unwrap();
    assert_eq!(win(&areas(&mut h)[0]), before, "one undo restores it");
    h.run_steps(2);
}

#[test]
fn dragging_a_corner_gives_the_area_any_proportion_and_the_viewport_follows() {
    let mut h = harness();
    let b = win(&areas(&mut h)[0]);
    // A click selects the rectangle, which shows its corners.
    click_world(&mut h, ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0));
    // The top-right corner out to twice the width but only half as much taller: another proportion.
    let (w, ht) = (b[2] - b[0], b[3] - b[1]);
    drag_world(&mut h, (b[2], b[3]), (b[0] + 2.0 * w, b[1] + 1.25 * ht));
    let a = win(&areas(&mut h)[0]);
    let tol = 2.0 * pixel(&h);
    assert!((a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol, "the opposite corner stays: {b:?} -> {a:?}");
    assert!((a[2] - (b[0] + 2.0 * w)).abs() < tol && (a[3] - (b[1] + 1.25 * ht)).abs() < tol, "the dragged corner goes where it was dropped: {a:?}");
    assert!(((a[2] - a[0]) / (a[3] - a[1]) - w / ht).abs() > 0.1, "a different proportion");
    // The viewport on the sheet took the area's proportion.
    let v = viewport_size(&h, "A");
    assert!((v.0 / v.1 - (a[2] - a[0]) / (a[3] - a[1])).abs() < 1e-6, "viewport {v:?} follows the area");
}

#[test]
fn a_dragged_corner_lands_exactly_on_an_object_snap() {
    let mut h = harness();
    h.state_mut().run("options", json!({"OSMODE": 1})).unwrap();
    h.run_steps(2);
    let b = win(&areas(&mut h)[0]);
    click_world(&mut h, ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0));
    let p = pixel(&h);
    // The line (0,0)-(100,0) ends at (100, 0): drop the top-right corner a few pixels from it.
    drag_world(&mut h, (b[2], b[3]), (100.0 + 3.0 * p, 2.0 * p));
    let a = win(&areas(&mut h)[0]);
    assert!((a[2] - 100.0).abs() < 1e-6 && a[3].abs() < 1e-6, "both coordinates are on the endpoint: {a:?}");
}

#[test]
fn dragging_on_empty_ground_adds_a_layout_showing_that_area() {
    let mut h = harness();
    assert_eq!(layouts(&h), 3, "Model-less count: two stock layouts and A");
    let n = layouts(&h);
    drag_world(&mut h, (60.0, 30.0), (90.0, 50.0));
    assert_eq!(layouts(&h), n + 1);
    let new = areas(&mut h).into_iter().find(|a| a["layout"] != "A").unwrap();
    let w = win(&new);
    assert!(w[0] <= 60.0 + 1e-6 && w[2] >= 90.0 - 1e-6 && w[1] <= 30.0 + 1e-6 && w[3] >= 50.0 - 1e-6, "the whole dragged area is in view: {w:?}");
    h.state_mut().session.undo().unwrap();
    assert_eq!(layouts(&h), n, "one undo removes the layout");
}

#[test]
fn a_click_or_a_tiny_drag_on_empty_ground_adds_nothing() {
    let mut h = harness();
    let n = layouts(&h);
    click_world(&mut h, (80.0, 60.0));
    let p = px(&h, 80.0, 60.0);
    drag(&mut h, p, p + vec2(3.0, 2.0));
    assert_eq!(layouts(&h), n);
}

#[test]
fn escape_during_a_drag_cancels_it() {
    let mut h = harness();
    let before = areas(&mut h);
    let n = layouts(&h);
    let b = win(&before[0]);
    let from = px(&h, (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    button(&mut h, from, true);
    h.event(Event::PointerMoved(from + vec2(60.0, 40.0)));
    h.run_steps(1);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    button(&mut h, from + vec2(60.0, 40.0), false);
    assert_eq!(areas(&mut h), before, "nothing was applied");
    assert_eq!(layouts(&h), n);
}

#[test]
fn double_clicking_a_rectangle_opens_its_layout() {
    let mut h = harness();
    let b = win(&areas(&mut h)[0]);
    let p = px(&h, (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    h.event(Event::PointerMoved(p));
    for _ in 0..2 {
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
    assert_eq!(h.state().session.layout_space(), Space::Paper("A".into()));
}

#[test]
fn in_layout_mode_the_drawings_objects_are_not_picked() {
    let mut h = harness();
    // The line is under the rectangle; clicking it selects the rectangle, not the line.
    let p = px(&h, 20.0, 0.0);
    click(&mut h, p);
    assert!(h.state().session.selection().is_empty(), "no object was selected");
    h.state_mut().run("ui.toggle.layoutmode", json!({"on": false})).unwrap();
    h.run_steps(2);
    click_world(&mut h, (60.0, 0.0));
    assert!(!h.state().session.selection().is_empty(), "with the mode off the line is picked again");
}

#[test]
fn the_mode_does_nothing_in_a_layout_tab_or_while_a_command_runs() {
    let mut h = harness();
    let n = layouts(&h);
    h.state_mut().session.execute("layout.set", &json!({"name": "A"})).unwrap();
    h.run_steps(3);
    let before = areas(&mut h);
    drag_world(&mut h, (5.0, 5.0), (30.0, 25.0));
    assert_eq!(layouts(&h), n, "a layout tab is not the place to add a layout");
    assert_eq!(areas(&mut h), before);
    h.state_mut().session.execute("layout.set", &json!({"name": "Model"})).unwrap();
    h.state_mut().session.start("circle").unwrap();
    h.run_steps(3);
    drag_world(&mut h, (60.0, 30.0), (90.0, 50.0));
    assert_eq!(layouts(&h), n, "while a command asks for points the clicks are its own");
}

#[test]
fn a_locked_viewport_is_not_moved_and_the_user_is_told() {
    let mut h = harness();
    let a = areas(&mut h);
    let handle = a[0]["handle"].as_str().unwrap().to_string();
    h.state_mut().session.execute("mview", &json!({"option": "lock", "handles": [handle], "locked": true, "layout": "A"})).unwrap();
    h.run_steps(2);
    let before = areas(&mut h);
    let b = win(&before[0]);
    let from = px(&h, (b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    drag(&mut h, from, from + vec2(80.0, 50.0));
    assert_eq!(areas(&mut h), before, "a locked viewport keeps its area");
    assert!(h.state().session.log.iter().any(|l| l.contains("locked")), "{:?}", h.state().session.log);
    assert!(h.state().status.as_ref().is_some_and(|(m, _)| m.contains("locked")), "also in the status bar: {:?}", h.state().status);
}

#[test]
fn the_rectangles_follow_undo_and_deleted_layouts() {
    let mut h = harness();
    h.state_mut().session.execute("layout.delete", &json!({"name": "A"})).unwrap();
    h.run_steps(2);
    // The rectangle of the deleted layout is gone: its place is empty ground, so a drag adds a layout.
    let n = layouts(&h);
    drag_world(&mut h, (5.0, 5.0), (30.0, 25.0));
    assert_eq!(layouts(&h), n + 1);
}

#[test]
fn hostile_view_states_do_not_panic() {
    let mut h = harness();
    // A view far out, and one far in: rectangles smaller than a pixel or off-screen are still handled.
    for height in [1e9, 1e-6, 5e5] {
        h.state_mut().session.execute("view.set", &json!({"center": [50, 20], "height": height})).unwrap();
        h.run_steps(3);
        let p = h.state().canvas.xf.unwrap().rect.center();
        drag(&mut h, p, p + vec2(40.0, 30.0));
        click(&mut h, p);
    }
}

#[test]
fn the_command_toggles_and_takes_on() {
    let mut h = harness();
    assert!(h.state().canvas.layout.on);
    h.state_mut().run("ui.toggle.layoutmode", json!({})).unwrap();
    assert!(!h.state().canvas.layout.on);
    h.state_mut().run("ui.toggle.layoutmode", json!({})).unwrap();
    assert!(h.state().canvas.layout.on);
    h.state_mut().run("ui.toggle.layoutmode", json!({"on": true})).unwrap();
    assert!(h.state().canvas.layout.on, "on stays on");
}

/// With the endpoint snap on, a moved rectangle puts its nearest corner exactly on the endpoint.
#[test]
fn a_moved_rectangle_snaps_a_corner_to_an_object_point() {
    let mut h = harness();
    h.state_mut().run("options", json!({"OSMODE": 1})).unwrap();
    h.run_steps(2);
    let b = win(&areas(&mut h)[0]);
    // Grab just inside the bottom-left corner, then drop it a few pixels from the line's end (100, 0).
    let p = pixel(&h);
    let from = (b[0] + 6.0 * p, b[1] + 6.0 * p);
    let near = (100.0 + 4.0 * p, 3.0 * p);
    drag_world(&mut h, from, (near.0 + 6.0 * p, near.1 + 6.0 * p));
    let a = win(&areas(&mut h)[0]);
    assert!((a[0] - 100.0).abs() < 1e-6 && a[1].abs() < 1e-6, "the corner is on the endpoint: {a:?}");
    assert!(((a[2] - a[0]) - (b[2] - b[0])).abs() < 1e-9, "a move keeps the size");
}

/// With snaps off the same drop lands where the pointer put it.
#[test]
fn without_snaps_the_rectangle_goes_where_it_is_dropped() {
    let mut h = harness();
    h.state_mut().run("options", json!({"OSMODE": 0})).unwrap();
    h.run_steps(2);
    let b = win(&areas(&mut h)[0]);
    let p = pixel(&h);
    let from = (b[0] + 6.0 * p, b[1] + 6.0 * p);
    drag_world(&mut h, from, (100.0 + 10.0 * p, 10.0 * p));
    let a = win(&areas(&mut h)[0]);
    assert!((a[0] - 100.0).abs() > 0.5 * p, "not snapped: {a:?}");
}

/// A new layout's window corners snap to object points too.
#[test]
fn a_new_layout_window_snaps_both_corners() {
    let mut h = harness();
    h.state_mut().session.execute("line", &json!({"points": [[0, 60], [100, 60]]})).unwrap();
    h.state_mut().run("options", json!({"OSMODE": 1})).unwrap();
    h.run_steps(2);
    let p = pixel(&h);
    let n = layouts(&h);
    // Both corners are a few pixels from line ends: (0, 60) and (100, 0).
    drag_world(&mut h, (-3.0 * p, 60.0 + 3.0 * p), (100.0 + 3.0 * p, 2.0 * p));
    assert_eq!(layouts(&h), n + 1, "the layout was added");
    let new = areas(&mut h).into_iter().find(|a| a["layout"] != "A").unwrap();
    let w = win(&new);
    let c = ((w[0] + w[2]) / 2.0, (w[1] + w[3]) / 2.0);
    assert!((c.0 - 50.0).abs() < 1e-6 && (c.1 - 30.0).abs() < 1e-6, "centred on the snapped window (0,60)-(100,0): {c:?}");
}

/// The status bar button turns the mode on and off.
#[test]
fn the_status_bar_button_toggles_the_mode() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    assert!(h.state().canvas.layout.on);
    h.get_by_label("Layout Mode").click();
    h.run_steps(3);
    assert!(!h.state().canvas.layout.on, "the button switched it off");
    h.get_by_label("Layout Mode").click();
    h.run_steps(3);
    assert!(h.state().canvas.layout.on, "and on again");
}
