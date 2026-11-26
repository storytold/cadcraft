//! Double-clicking TEXT, an attribute definition or a dimension edits its text in place on the
//! canvas, as in AutoCAD (#842): Enter or a click outside keeps it, Esc cancels.

use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{EditorKind, Session};
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};

struct Drawing {
    text: Handle,
    attdef: Handle,
    dim: Handle,
}

fn handle(v: &Value) -> Handle {
    Handle::parse_hex(v["handle"].as_str().unwrap()).unwrap()
}

fn harness() -> (Harness<'static, CadApp>, Drawing) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let text = handle(&app.run("text", json!({ "at": [0, 0], "text": "HELLO", "height": 2.5 })).unwrap());
    let attdef = handle(&app.run("attdef", json!({ "tag": "TAG", "default": "D1", "at": [0, 10], "height": 2.5 })).unwrap());
    let dim = handle(&app.run("dimlinear", json!({ "p1": [0, -10], "p2": [20, -10], "at": [10, -14] })).unwrap());
    app.session.execute("view.set", &json!({ "center": [10, 0], "height": 40 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = Harness::builder()
        .with_context_setup(cadcraft_ui_egui::framework::install_headless)
        .with_size(vec2(1600.0, 1000.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
    h.run_steps(5);
    (h, Drawing { text, attdef, dim })
}

fn double_click(h: &mut Harness<'_, CadApp>, at: Vec2) {
    // Far enough from any earlier click not to count as its second or third.
    h.run_steps(40);
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(at);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(3);
}

fn value(h: &Harness<'_, CadApp>, e: Handle) -> String {
    match &h.state().session.doc().unwrap().entity(e).unwrap().kind {
        EntityKind::Text(t) => t.value.clone(),
        EntityKind::AttDef(a) => a.text.value.clone(),
        EntityKind::Dimension(d) => d.text.clone(),
        k => panic!("not text: {}", k.type_name()),
    }
}

/// The one-line editor open on the double-clicked object: its kind and starting text.
fn editor(h: &Harness<'_, CadApp>) -> Option<(EditorKind, String)> {
    h.state().session.text_editor().map(|e| (e.kind, e.mtext.contents))
}

/// Type into the in-place box: the command line gets none of it.
fn type_text(h: &mut Harness<'_, CadApp>, s: &str) {
    h.event(Event::Text(s.into()));
    h.run_steps(2);
    assert!(h.state().cmd.buffer.is_empty(), "typed into the command line: {:?}", h.state().cmd.buffer);
}

fn press(h: &mut Harness<'_, CadApp>, key: Key) {
    h.key_press(key);
    h.run_steps(3);
}

const TEXT_AT: Vec2 = Vec2::new(4.0, 1.2);
const ATTDEF_AT: Vec2 = Vec2::new(2.5, 11.2);
const DIM_AT: Vec2 = Vec2::new(4.0, -14.0);

#[test]
fn double_clicked_text_is_edited_in_place_and_undone_in_one_step() {
    let (mut h, d) = harness();
    double_click(&mut h, TEXT_AT);
    assert_eq!(editor(&h), Some((EditorKind::Line, "HELLO".into())));
    // The whole text opens selected: typing replaces it.
    type_text(&mut h, "WORLD");
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.text), "WORLD");
    assert!(editor(&h).is_none(), "the editor closed");
    h.state_mut().session.cancel();
    h.state_mut().session.undo().unwrap();
    assert_eq!(value(&h, d.text), "HELLO", "one undo takes the edit back");
}

#[test]
fn escape_and_an_emptied_box_leave_the_text_as_it_was() {
    let (mut h, d) = harness();
    double_click(&mut h, TEXT_AT);
    type_text(&mut h, "WORLD");
    press(&mut h, Key::Escape);
    assert_eq!(value(&h, d.text), "HELLO");
    assert!(h.state().session.running.is_none(), "Esc ends the edit");

    double_click(&mut h, TEXT_AT);
    assert!(editor(&h).is_some());
    press(&mut h, Key::Backspace);
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.text), "HELLO", "an emptied box keeps the old text");
}

#[test]
fn a_click_outside_the_box_keeps_what_was_typed() {
    let (mut h, d) = harness();
    double_click(&mut h, TEXT_AT);
    type_text(&mut h, "OUTSIDE");
    let pos = h.state().canvas.xf.unwrap().to_screen(Vec2::new(25.0, 15.0));
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
    assert_eq!(value(&h, d.text), "OUTSIDE");
    assert!(editor(&h).is_none());
}

#[test]
fn double_clicked_attribute_definition_is_edited_in_place() {
    let (mut h, d) = harness();
    double_click(&mut h, ATTDEF_AT);
    // TEXTEDIT edits a definition's default value (as on the command line before #842).
    assert_eq!(editor(&h), Some((EditorKind::Line, "D1".into())));
    type_text(&mut h, "D2");
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.attdef), "D2");
    h.state_mut().session.cancel();

    double_click(&mut h, ATTDEF_AT);
    type_text(&mut h, "OTHER");
    press(&mut h, Key::Escape);
    assert_eq!(value(&h, d.attdef), "D2", "Esc keeps it");
}

#[test]
fn double_clicked_dimension_edits_its_text_override() {
    let (mut h, d) = harness();
    double_click(&mut h, DIM_AT);
    let Some((EditorKind::Dimension { measured }, contents)) = editor(&h) else { panic!("no dimension editor: {:?}", editor(&h)) };
    assert_eq!(contents, "<>", "no override: the measured value");
    assert!(measured.starts_with("20"), "{measured}");
    type_text(&mut h, "<> TYP");
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.dim), "<> TYP");
    h.state_mut().session.cancel();

    double_click(&mut h, DIM_AT);
    assert_eq!(editor(&h).map(|e| e.1), Some("<> TYP".into()), "the editor shows the override");
    type_text(&mut h, "<>");
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.dim), "", "<> alone is the measured value again");
}

#[test]
fn textedit_through_engine_execute_opens_no_editor() {
    let (mut h, d) = harness();
    h.state_mut().run("textedit", json!({ "handle": d.text.hex(), "text": "SCRIPTED" })).unwrap();
    h.run_steps(3);
    assert_eq!(value(&h, d.text), "SCRIPTED");
    assert!(h.state().session.running.is_none());
    assert!(editor(&h).is_none());
}

/// The box covers the text as drawn, from its drawn left edge, for left-justified and
/// middle-centre text alike (not starting at a centred text's insertion point).
#[test]
fn the_box_covers_the_drawn_text_whatever_its_justification() {
    let (mut h, d) = harness();
    let mc = handle(&h.state_mut().run("text", json!({ "at": [16, 16], "text": "Middle centre", "height": 2.5, "justify": "MC" })).unwrap());
    h.run_steps(3);
    for (e, at) in [(d.text, TEXT_AT), (mc, Vec2::new(16.0, 16.0))] {
        double_click(&mut h, at);
        assert!(editor(&h).is_some(), "{e:?}: no editor");
        let drawn = {
            let s = &h.state().session;
            let doc = s.doc().unwrap();
            let EntityKind::Text(t) = &doc.entity(e).unwrap().kind else { panic!("not text") };
            let b = cadcraft_engine::render::place_text_entity(doc, t, &t.value).1;
            let xf = h.state().canvas.xf.unwrap();
            egui::Rect::from_points(&[
                xf.to_screen(b.min),
                xf.to_screen(b.max),
                xf.to_screen(Vec2::new(b.min.x, b.max.y)),
                xf.to_screen(Vec2::new(b.max.x, b.min.y)),
            ])
        };
        let area = h.ctx.memory(|m| m.area_rect(egui::Id::unique("line_editor_area"))).expect("the box is shown");
        assert!(area.expand(0.5).contains_rect(drawn), "{e:?}: box {area:?} does not cover the drawn text {drawn:?}");
        assert!((area.left() - drawn.left()).abs() <= 4.0, "{e:?}: box starts at {} but the text at {}", area.left(), drawn.left());
        press(&mut h, Key::Escape);
    }
}

/// Each box starts with no undo history: Ctrl+Z in a newly opened box can't bring back the text
/// typed into the last one (review of #842).
#[test]
fn undo_in_a_new_box_does_not_bring_back_another_objects_text() {
    let (mut h, d) = harness();
    double_click(&mut h, TEXT_AT);
    type_text(&mut h, "WORLD");
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.text), "WORLD");
    h.state_mut().session.cancel();

    double_click(&mut h, ATTDEF_AT);
    assert_eq!(editor(&h).map(|e| e.1), Some("D1".into()));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    press(&mut h, Key::Enter);
    assert_eq!(value(&h, d.attdef), "D1", "Ctrl+Z took another object's text");
    assert_eq!(value(&h, d.text), "WORLD");
}
