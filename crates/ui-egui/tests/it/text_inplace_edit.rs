//! Double-clicking a single-line text edits it in place: a field over the text takes the new
//! value; Enter commits it, Escape (or Enter on a blank field) keeps the text; the command line's
//! prompt still works.

use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};
use cadcraft_ui_egui::{CadApp, Services, text_editor};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("text", json!({ "at": [0, 0], "text": "HELLO", "height": 2.5 })).unwrap();
    app.run("attdef", json!({ "tag": "NO", "default": "1", "at": [0, 10], "height": 2.5 })).unwrap();
    app.run("mtext", json!({ "at": [0, 20], "text": "first\\Psecond", "height": 2.5, "width": 20 })).unwrap();
    app.session.execute("view.set", &json!({ "center": [15, 10], "height": 40 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

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

fn key(h: &mut Harness<'_, CadApp>, key: Key) {
    for pressed in [true, false] {
        h.event(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn value(h: &Harness<'_, CadApp>, i: usize) -> String {
    let d = h.state().session.doc().unwrap();
    match &d.model.iter().nth(i).unwrap().kind {
        EntityKind::Text(t) => t.value.clone(),
        EntityKind::AttDef(a) => a.text.value.clone(),
        EntityKind::MText(t) => t.contents.clone(),
        k => panic!("not text: {}", k.type_name()),
    }
}

#[test]
fn double_clicked_text_is_edited_in_a_field_over_it() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(4.0, 1.2));
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("textedit"));
    assert_eq!(text_editor::editing(&h.ctx).as_deref(), Some("HELLO"), "the field shows the text");
    h.event(Event::Text(" WORLD".into()));
    h.run_steps(2);
    assert_eq!(text_editor::editing(&h.ctx).as_deref(), Some("HELLO WORLD"), "typing goes to the field");
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, 0), "HELLO WORLD");
    assert!(text_editor::editing(&h.ctx).is_none(), "the field is gone");
    // TEXTEDIT goes on asking for objects, as AutoCAD's does.
    assert_eq!(h.state().session.current_prompt().unwrap().message, "Select an annotation object");
}

#[test]
fn escape_keeps_the_text_and_an_attribute_definition_edits_its_default() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(4.0, 1.2));
    h.event(Event::Text("X".into()));
    h.run_steps(2);
    key(&mut h, Key::Escape);
    assert_eq!(value(&h, 0), "HELLO");
    assert!(text_editor::editing(&h.ctx).is_none());

    let mut h = harness();
    double_click(&mut h, Vec2::new(1.0, 11.2));
    assert_eq!(text_editor::editing(&h.ctx).as_deref(), Some("1"));
    h.event(Event::Text("2".into()));
    h.run_steps(2);
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, 1), "12");
}

#[test]
fn the_command_line_prompt_still_takes_the_text_and_mtext_has_no_field() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(4.0, 1.2));
    assert!(text_editor::editing(&h.ctx).is_some());
    h.state_mut().session.input(Input::Text("typed".into())).unwrap();
    h.run_steps(2);
    assert_eq!(value(&h, 0), "typed");
    assert!(text_editor::editing(&h.ctx).is_none());

    let mut h = harness();
    double_click(&mut h, Vec2::new(3.0, 18.8));
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("textedit"));
    assert!(text_editor::editing(&h.ctx).is_none(), "multiline text has its own editor");
}

#[test]
fn enter_on_a_blank_field_keeps_the_text_as_the_command_line_does() {
    let mut h = harness();
    double_click(&mut h, Vec2::new(4.0, 1.2));
    assert_eq!(text_editor::editing(&h.ctx).as_deref(), Some("HELLO"));
    for _ in 0.."HELLO".len() {
        key(&mut h, Key::Backspace);
    }
    h.event(Event::Text("  ".into()));
    h.run_steps(2);
    assert_eq!(text_editor::editing(&h.ctx).as_deref(), Some("  "), "the field holds only blanks");
    key(&mut h, Key::Enter);
    assert_eq!(value(&h, 0), "HELLO", "an empty text would draw nothing and be hard to pick again");
    assert!(text_editor::editing(&h.ctx).is_none(), "the field is gone");
    assert_eq!(h.state().session.current_prompt().unwrap().message, "Select an annotation object");
}
