//! Layout mode: a rectangle in the Model tab for the part of the model each layout shows. It works
//! like grips: a click selects, a click on a grip takes it, the next click puts it down. Real pointer
//! events drive these tests.
use cadcraft_doc::Space;
use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};

/// A drawing with a line, one layout "A" showing [0,0]-[40,30], the Model tab with layout mode on and
/// no object snaps (the tests that want them turn them on).
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
    h.state_mut().run("options", json!({"OSMODE": 0})).unwrap();
    h.state_mut().run("ui.toggle.layoutmode", json!({"on": true})).unwrap();
    h.run_steps(3);
    h
}

fn button(h: &mut Harness<'_, CadApp>, p: Pos2, pressed: bool) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

/// Press, move in steps, release.
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

fn click_world(h: &mut Harness<'_, CadApp>, p: (f64, f64)) {
    let p = px(h, p.0, p.1);
    click(h, p);
}

fn drag_world(h: &mut Harness<'_, CadApp>, a: (f64, f64), b: (f64, f64)) {
    let (a, b) = (px(h, a.0, a.1), px(h, b.0, b.1));
    drag(h, a, b);
}

/// One pixel in drawing units.
fn pixel(h: &Harness<'_, CadApp>) -> f64 {
    1.0 / h.state().canvas.xf.unwrap().scale
}

fn areas(h: &mut Harness<'_, CadApp>) -> Vec<Value> {
    h.state_mut().session.execute("layout.areas", &json!({})).unwrap().as_array().unwrap().clone()
}

fn win(a: &Value) -> [f64; 4] {
    let g = |i: usize, j: usize| a["window"][i][j].as_f64().unwrap();
    [g(0, 0), g(0, 1), g(1, 0), g(1, 1)]
}

fn center(w: &[f64; 4]) -> (f64, f64) {
    ((w[0] + w[2]) / 2.0, (w[1] + w[3]) / 2.0)
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

/// Select the rectangle of layout "A" (a click inside it, off its middle) and give back its corners.
fn select_a(h: &mut Harness<'_, CadApp>) -> [f64; 4] {
    let b = win(&areas(h)[0]);
    let p = pixel(h);
    click_world(h, (b[0] + 40.0 * p, b[1] + 40.0 * p));
    b
}

fn key(h: &mut Harness<'_, CadApp>, k: egui::Key) {
    h.key_press(k);
    h.run_steps(2);
}

fn right_click(h: &mut Harness<'_, CadApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn the_middle_grip_moves_the_area_with_two_clicks_in_one_undo_step() {
    let mut h = harness();
    let before = select_a(&mut h);
    let c = center(&before);
    let target = (c.0 + 10.0, c.1 + 5.0);
    click_world(&mut h, c);
    assert_eq!(win(&areas(&mut h)[0]), before, "taking a grip changes nothing yet");
    click_world(&mut h, target);
    let after = win(&areas(&mut h)[0]);
    let tol = 2.0 * pixel(&h);
    let new_c = center(&after);
    assert!((new_c.0 - target.0).abs() < tol && (new_c.1 - target.1).abs() < tol, "the middle is where the second click put it: {after:?}");
    assert!(((after[2] - after[0]) - (before[2] - before[0])).abs() < 1e-9, "a move keeps the size");
    h.state_mut().session.undo().unwrap();
    assert_eq!(win(&areas(&mut h)[0]), before, "one undo restores it");
}

#[test]
fn a_corner_grip_gives_the_area_any_proportion_and_the_viewport_follows() {
    let mut h = harness();
    let b = select_a(&mut h);
    let (w, ht) = (b[2] - b[0], b[3] - b[1]);
    // The top-right corner out to twice the width but only a quarter taller: another proportion.
    click_world(&mut h, (b[2], b[3]));
    click_world(&mut h, (b[0] + 2.0 * w, b[1] + 1.25 * ht));
    let a = win(&areas(&mut h)[0]);
    let tol = 2.0 * pixel(&h);
    assert!((a[0] - b[0]).abs() < tol && (a[1] - b[1]).abs() < tol, "the opposite corner stays: {b:?} -> {a:?}");
    assert!((a[2] - (b[0] + 2.0 * w)).abs() < tol && (a[3] - (b[1] + 1.25 * ht)).abs() < tol, "the corner goes where it was put: {a:?}");
    assert!(((a[2] - a[0]) / (a[3] - a[1]) - w / ht).abs() > 0.1, "a different proportion");
    let v = viewport_size(&h, "A");
    assert!((v.0 / v.1 - (a[2] - a[0]) / (a[3] - a[1])).abs() < 1e-6, "viewport {v:?} follows the area");
}

#[test]
fn escape_or_a_right_click_cancels_a_taken_grip() {
    for cancel in ["escape", "right click"] {
        let mut h = harness();
        let before = areas(&mut h);
        let b = select_a(&mut h);
        click_world(&mut h, center(&b));
        h.event(Event::PointerMoved(px(&h, b[2] + 20.0, b[3] + 20.0)));
        h.run_steps(2);
        if cancel == "escape" {
            key(&mut h, egui::Key::Escape);
        } else {
            let p = px(&h, b[2] + 20.0, b[3] + 20.0);
            right_click(&mut h, p);
        }
        // A click now takes nothing: the grip was put back. (The click lands on empty ground.)
        click_world(&mut h, (b[2] + 20.0, b[3] + 20.0));
        assert_eq!(areas(&mut h), before, "{cancel}: nothing was applied");
    }
}

#[test]
fn a_click_on_empty_ground_with_a_selection_only_deselects() {
    let mut h = harness();
    let n = layouts(&h);
    let b = select_a(&mut h);
    // Empty ground, two clicks: the first deselects, the second starts a window (still no layout).
    click_world(&mut h, (b[2] + 30.0, b[3] + 30.0));
    assert_eq!(layouts(&h), n, "deselecting adds nothing");
    click_world(&mut h, (b[2] + 30.0, b[3] + 30.0));
    assert_eq!(layouts(&h), n, "the window is only started");
    key(&mut h, egui::Key::Escape);
    assert_eq!(layouts(&h), n);
}

#[test]
fn two_clicks_on_empty_ground_add_a_layout_showing_that_area() {
    let mut h = harness();
    let n = layouts(&h);
    click_world(&mut h, (60.0, 30.0));
    click_world(&mut h, (90.0, 50.0));
    assert_eq!(layouts(&h), n + 1);
    let new = areas(&mut h).into_iter().find(|a| a["layout"] != "A").unwrap();
    let w = win(&new);
    let tol = 1e-6;
    assert!(
        (w[0] - 60.0).abs() < tol && (w[1] - 30.0).abs() < tol && (w[2] - 90.0).abs() < tol && (w[3] - 50.0).abs() < tol,
        "exactly the window: {w:?}"
    );
    let v = viewport_size(&h, "Layout3");
    assert!((v.0 / v.1 - 30.0 / 20.0).abs() < 1e-9, "the viewport has the window's proportion: {v:?}");
    h.state_mut().session.undo().unwrap();
    assert_eq!(layouts(&h), n, "one undo removes the layout");
}

#[test]
fn press_drag_and_release_on_empty_ground_adds_a_layout_too() {
    let mut h = harness();
    let n = layouts(&h);
    drag_world(&mut h, (60.0, 30.0), (90.0, 50.0));
    assert_eq!(layouts(&h), n + 1);
}

#[test]
fn a_tiny_drag_only_starts_the_window_and_escape_drops_it() {
    let mut h = harness();
    let n = layouts(&h);
    let p = px(&h, 80.0, 60.0);
    drag(&mut h, p, p + vec2(3.0, 2.0));
    assert_eq!(layouts(&h), n, "a click is not a window");
    key(&mut h, egui::Key::Escape);
    click_world(&mut h, (90.0, 70.0));
    assert_eq!(layouts(&h), n, "after Escape the next click starts a new window instead of closing the old one");
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
    click_world(&mut h, (20.0, 0.0));
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
    click_world(&mut h, (5.0, 5.0));
    click_world(&mut h, (30.0, 25.0));
    assert_eq!(layouts(&h), n, "a layout tab is not the place to add a layout");
    assert_eq!(areas(&mut h), before);
    h.state_mut().session.execute("layout.set", &json!({"name": "Model"})).unwrap();
    h.state_mut().session.start("circle").unwrap();
    h.run_steps(3);
    click_world(&mut h, (60.0, 30.0));
    click_world(&mut h, (90.0, 50.0));
    assert_eq!(layouts(&h), n, "while a command asks for points the clicks are its own");
}

#[test]
fn a_locked_viewport_has_no_grips_and_the_user_is_told() {
    let mut h = harness();
    let a = areas(&mut h);
    let handle = a[0]["handle"].as_str().unwrap().to_string();
    h.state_mut().session.execute("mview", &json!({"option": "lock", "handles": [handle], "locked": true, "layout": "A"})).unwrap();
    h.run_steps(2);
    let before = areas(&mut h);
    let b = win(&before[0]);
    let p = pixel(&h);
    click_world(&mut h, (b[0] + 40.0 * p, b[1] + 40.0 * p));
    assert!(h.state().session.log.iter().any(|l| l.contains("locked")), "{:?}", h.state().session.log);
    assert!(h.state().status.as_ref().is_some_and(|(m, _)| m.contains("locked")), "also in the status bar: {:?}", h.state().status);
    // Where the middle grip would be, a click takes nothing; the next click changes nothing.
    click_world(&mut h, center(&b));
    click_world(&mut h, (center(&b).0 + 10.0, center(&b).1 + 10.0));
    assert_eq!(areas(&mut h), before, "a locked viewport keeps its area");
}

#[test]
fn the_rectangles_follow_deleted_layouts() {
    let mut h = harness();
    h.state_mut().session.execute("layout.delete", &json!({"name": "A"})).unwrap();
    h.run_steps(2);
    // The rectangle of the deleted layout is gone: its place is empty ground, so two clicks add a layout.
    let n = layouts(&h);
    click_world(&mut h, (5.0, 5.0));
    click_world(&mut h, (30.0, 25.0));
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
        click(&mut h, p);
        click(&mut h, p + vec2(40.0, 30.0));
        click(&mut h, p);
        key(&mut h, egui::Key::Escape);
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

#[test]
fn the_middle_grip_puts_the_middle_exactly_on_an_object_point() {
    let mut h = harness();
    h.state_mut().run("options", json!({"OSMODE": 1})).unwrap();
    h.run_steps(2);
    let b = select_a(&mut h);
    let p = pixel(&h);
    click_world(&mut h, center(&b));
    // The line (0,0)-(100,0) ends at (100, 0): put the middle a few pixels from it.
    click_world(&mut h, (100.0 + 4.0 * p, 3.0 * p));
    let a = win(&areas(&mut h)[0]);
    let c = center(&a);
    assert!((c.0 - 100.0).abs() < 1e-6 && c.1.abs() < 1e-6, "the middle is on the endpoint: {a:?}");
    assert!(((a[2] - a[0]) - (b[2] - b[0])).abs() < 1e-9, "a move keeps the size");
}

#[test]
fn without_snaps_the_middle_goes_where_it_is_put() {
    let mut h = harness();
    let b = select_a(&mut h);
    let p = pixel(&h);
    click_world(&mut h, center(&b));
    click_world(&mut h, (100.0 + 10.0 * p, 10.0 * p));
    let c = center(&win(&areas(&mut h)[0]));
    assert!((c.0 - 100.0).abs() > 5.0 * p, "not snapped: {c:?}");
}

#[test]
fn a_corner_grip_lands_exactly_on_an_object_snap() {
    let mut h = harness();
    h.state_mut().run("options", json!({"OSMODE": 1})).unwrap();
    h.run_steps(2);
    let b = select_a(&mut h);
    let p = pixel(&h);
    click_world(&mut h, (b[2], b[3]));
    // The line (0,0)-(100,0) ends at (100, 0): put the top-right corner a few pixels from it.
    click_world(&mut h, (100.0 + 3.0 * p, 2.0 * p));
    let a = win(&areas(&mut h)[0]);
    assert!((a[2] - 100.0).abs() < 1e-6 && a[3].abs() < 1e-6, "both coordinates are on the endpoint: {a:?}");
}

#[test]
fn a_new_layout_window_snaps_both_corners() {
    let mut h = harness();
    h.state_mut().session.execute("line", &json!({"points": [[0, 60], [100, 60]]})).unwrap();
    h.state_mut().run("options", json!({"OSMODE": 1})).unwrap();
    h.run_steps(2);
    let p = pixel(&h);
    let n = layouts(&h);
    // Both corners are a few pixels from line ends: (0, 60) and (100, 0).
    click_world(&mut h, (-3.0 * p, 60.0 + 3.0 * p));
    click_world(&mut h, (100.0 + 3.0 * p, 2.0 * p));
    assert_eq!(layouts(&h), n + 1, "the layout was added");
    let new = areas(&mut h).into_iter().find(|a| a["layout"] != "A").unwrap();
    let w = win(&new);
    assert!(
        (w[0] - 0.0).abs() < 1e-6 && (w[3] - 60.0).abs() < 1e-6 && (w[2] - 100.0).abs() < 1e-6 && w[1].abs() < 1e-6,
        "exactly the snapped window (0,0)-(100,60): {w:?}"
    );
}

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

/// The Layout Mode button sits clear of the MODEL/PAPER button a layout tab shows.
#[test]
fn the_status_bar_button_does_not_overlap_the_model_paper_button() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    h.state_mut().session.execute("layout.set", &json!({"name": "A"})).unwrap();
    h.run_steps(4);
    let button = h.get_by_label("Layout Mode").rect();
    let paper = h.get_by_label("PAPER").rect();
    assert!(!button.intersects(paper), "the Layout Mode button {button:?} overlaps the PAPER button {paper:?}");
}

/// A taken grip does not survive the mode being turned off: nothing is applied when it comes back.
#[test]
fn turning_the_mode_off_drops_a_taken_grip() {
    let mut h = harness();
    let before = areas(&mut h);
    let b = select_a(&mut h);
    click_world(&mut h, center(&b));
    h.state_mut().run("ui.toggle.layoutmode", json!({"on": false})).unwrap();
    h.run_steps(3);
    h.state_mut().run("ui.toggle.layoutmode", json!({"on": true})).unwrap();
    h.run_steps(3);
    click_world(&mut h, (b[2] + 20.0, b[3] + 20.0));
    assert_eq!(areas(&mut h), before, "the grip was dropped, so that click put nothing down");
}

/// A double-click on a rectangle opens its layout even over a text object, which would otherwise be edited.
#[test]
fn a_double_click_over_text_opens_the_layout_instead_of_editing_the_text() {
    let mut h = harness();
    h.state_mut().run("text", json!({ "at": [10, 10], "text": "HELLO", "height": 2.5 })).unwrap();
    h.run_steps(2);
    let p = px(&h, 12.0, 11.0);
    h.event(Event::PointerMoved(p));
    for _ in 0..2 {
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
    assert_eq!(h.state().session.layout_space(), Space::Paper("A".into()), "the layout opened");
    assert!(h.state().session.running.is_none(), "no text edit was started");
}
