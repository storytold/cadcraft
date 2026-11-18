//! Selection cycling on the canvas (#1143, #1145): with SELECTIONCYCLING 2 a click over
//! overlapping objects lists them and a click on a row selects that object; Shift+Space steps the
//! highlight through them and a click selects the highlighted one.

use cadcraft_doc::Handle;
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// Two lines overlapping from x = 0 to 10 (the second on top), zoomed to fit; the lines'
/// handles.
fn harness(selectioncycling: i64) -> (Harness<'static, CadApp>, Vec<Handle>) {
    let mut session = Session::new();
    session.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap();
    session.execute("line", &json!({ "points": [[0, 0], [20, 0]] })).unwrap();
    session.execute("setvar", &json!({ "name": "SELECTIONCYCLING", "value": selectioncycling })).unwrap();
    session.set_selection(Vec::new());
    let hs: Vec<Handle> = session.doc().unwrap().model.iter().map(|e| e.handle).collect();
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = true;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    (h, hs)
}

fn click(h: &mut Harness<'static, CadApp>, at: Pos2) {
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn overlap(h: &Harness<'static, CadApp>) -> Pos2 {
    h.state().canvas.xf.unwrap().to_screen(Vec2::new(5.0, 0.0))
}

#[test]
fn a_click_over_overlapping_objects_lists_them_and_a_row_picks_one() {
    let (mut h, hs) = harness(2);
    let at = overlap(&h);
    click(&mut h, at);
    let rows = h.state().canvas.cycle_rows();
    assert_eq!(rows.iter().map(|(_, e)| *e).collect::<Vec<_>>(), vec![hs[1], hs[0]], "a row per line, the one on top first");
    assert!(h.state().session.selection().is_empty(), "the click opens the list instead of picking");
    // Hovering the second row highlights its line; clicking it selects only that line.
    let second = rows[1].0.center();
    h.event(egui::Event::PointerMoved(second));
    h.run_steps(2);
    assert_eq!(h.state().canvas.hover, Some(hs[0]));
    click(&mut h, second);
    assert_eq!(h.state().session.selection(), vec![hs[0]]);
    assert!(h.state().canvas.cycle_rows().is_empty(), "picking a row closes the list");
}

#[test]
fn with_selectioncycling_0_a_click_picks_the_top_object() {
    let (mut h, hs) = harness(0);
    let at = overlap(&h);
    click(&mut h, at);
    assert!(h.state().canvas.cycle_rows().is_empty());
    assert_eq!(h.state().session.selection(), vec![hs[1]]);
}

#[test]
fn shift_space_steps_the_highlight_and_a_click_picks_it() {
    // Selection cycling off (AutoCAD's initial -2): Shift+Space works as its temporary override.
    let (mut h, hs) = harness(-2);
    let at = overlap(&h);
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(2);
    assert_eq!(h.state().canvas.hover, Some(hs[1]));
    h.event(egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT });
    h.event(egui::Event::Text(" ".into()));
    h.run_steps(2);
    assert_eq!(h.state().canvas.hover, Some(hs[0]), "Shift+Space highlights the line underneath");
    click(&mut h, at);
    assert_eq!(h.state().session.selection(), vec![hs[0]]);
}

fn type_line(h: &mut Harness<'static, CadApp>, text: &str) {
    h.event(egui::Event::Text(text.into()));
    h.event(egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// The start point LINE's next prompt measures from: where its first click landed.
fn line_started_at(h: &Harness<'static, CadApp>) -> Option<Vec2> {
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    h.state().session.current_prompt().and_then(|p| p.base)
}

#[test]
fn after_shift_space_a_command_still_gets_its_first_click() {
    let (mut h, _) = harness(-2);
    let at = overlap(&h);
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(2);
    h.event(egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT });
    h.event(egui::Event::Text(" ".into()));
    h.run_steps(2);
    type_line(&mut h, "L");
    click(&mut h, at);
    let start = line_started_at(&h).expect("the click placed LINE's first point");
    assert!(start.dist(Vec2::new(5.0, 0.0)) < 0.5, "{start:?}");
}

#[test]
fn a_command_closes_the_list_and_gets_its_first_click() {
    let (mut h, _) = harness(2);
    let at = overlap(&h);
    click(&mut h, at);
    assert_eq!(h.state().canvas.cycle_rows().len(), 2);
    type_line(&mut h, "L");
    assert!(h.state().canvas.cycle_rows().is_empty(), "starting a command closes the list");
    let elsewhere = h.state().canvas.xf.unwrap().to_screen(Vec2::new(5.0, 8.0));
    click(&mut h, elsewhere);
    let start = line_started_at(&h).expect("the click placed LINE's first point");
    assert!(start.dist(Vec2::new(5.0, 8.0)) < 0.5, "{start:?}");
}

#[test]
fn after_an_erase_a_row_never_selects_an_object_that_is_gone() {
    let (mut h, hs) = harness(2);
    let at = overlap(&h);
    click(&mut h, at);
    let rows = h.state().canvas.cycle_rows();
    assert_eq!(rows[0].1, hs[1]);
    // The top line goes away (as by undo or ERASE elsewhere); then the row that listed it is clicked.
    h.state_mut().run("erase", json!({ "handles": [hs[1].hex()] })).unwrap();
    h.run_steps(1);
    click(&mut h, rows[0].0.center());
    let sel = h.state().session.selection();
    let d = h.state().session.doc().unwrap();
    assert!(sel.iter().all(|s| d.entity(*s).is_some()), "{sel:?}");
    assert!(!sel.contains(&hs[1]));
}
