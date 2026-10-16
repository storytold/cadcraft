//! Resting the cursor on a multifunctional grip opens its menu, as in AutoCAD (gaps U5).

use cadcraft_engine::Session;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A selected three-vertex polyline: its first vertex is a grip with Add and Remove Vertex.
fn harness() -> Harness<'static, CadApp> {
    let mut session = Session::empty();
    session.open_drawing(cadcraft_engine::doc::Drawing::new_imperial(), "Grip menu", None);
    session.execute("view.set", &json!({"center":[50,50],"height":100})).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.run("pline", json!({ "vertices": [[20, 50], [50, 50], [80, 70]] })).unwrap();
    let all = app.session.doc().unwrap().model.handles();
    app.session.set_selection(all);
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(0.05).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

fn screen(h: &Harness<'_, CadApp>, p: Vec2) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(p)
}

fn grip(h: &Harness<'_, CadApp>) -> Pos2 {
    screen(h, Vec2::new(20.0, 50.0))
}

/// Keep the pointer where it is for `secs` (frames of 0.05 s).
fn wait(h: &mut Harness<'_, CadApp>, secs: f32) {
    h.run_steps((secs / 0.05).round() as usize);
}

/// "Add Vertex" is only ever drawn by the grip menu.
fn menu_open(h: &Harness<'_, CadApp>) -> bool {
    h.query_by_label("Add Vertex").is_some()
}

#[test]
fn resting_on_a_grip_opens_its_menu() {
    let mut h = harness();
    let at = grip(&h);
    h.event(Event::PointerMoved(at));
    wait(&mut h, 1.0);
    assert!(menu_open(&h), "the menu opens after the cursor rests on the grip");
    assert!(h.state().session.running.is_none(), "hovering doesn't start a grip edit");
    if let Some(directory) = std::env::var_os("CADCRAFT_GRIP_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&directory).unwrap();
        h.render().unwrap().save(directory.join("grip-hover-menu.png")).unwrap();
    }
}

#[test]
fn passing_over_a_grip_does_not_open_its_menu() {
    let mut h = harness();
    let at = grip(&h);
    h.event(Event::PointerMoved(at));
    wait(&mut h, 0.25);
    assert!(!menu_open(&h), "a quarter of a second is shorter than the delay");
}

#[test]
fn no_menu_while_the_button_is_held_on_the_grip() {
    let mut h = harness();
    let at = grip(&h);
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    // Dragging the grip, still inside its box.
    h.event(Event::PointerMoved(at + vec2(2.0, 1.0)));
    wait(&mut h, 1.0);
    assert!(!menu_open(&h), "a grip being dragged shows no menu");
}

#[test]
fn moving_away_closes_the_menu() {
    let mut h = harness();
    let at = grip(&h);
    h.event(Event::PointerMoved(at));
    wait(&mut h, 1.0);
    assert!(menu_open(&h));
    let away = screen(&h, Vec2::new(50.0, 20.0));
    h.event(Event::PointerMoved(away));
    h.run_steps(2);
    assert!(!menu_open(&h), "the menu closes once the cursor leaves the grip");
}

#[test]
fn choosing_from_the_hover_menu_runs_the_option() {
    let mut h = harness();
    let at = grip(&h);
    h.event(Event::PointerMoved(at));
    wait(&mut h, 1.0);
    h.get_by_label("Add Vertex").click();
    h.run_steps(3);
    let prompt = h.state().session.prompt_text();
    assert!(prompt.contains("ADD VERTEX"), "{prompt}");
    assert!(!menu_open(&h), "the menu closes after a choice");
}

#[test]
fn right_click_on_a_grip_still_opens_its_menu() {
    let mut h = harness();
    let at = grip(&h);
    h.event(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(1);
    assert!(menu_open(&h), "right-click opens the menu without waiting");
}
