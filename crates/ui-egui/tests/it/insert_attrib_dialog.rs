//! Edit Attributes (ATTDIA 1): a typed INSERT asks for its attribute values in one dialog. OK
//! gives them to INSERT (a Verify one is not asked again), Enter in a field is OK, Cancel inserts
//! nothing, and with ATTDIA 0, a JSON call, a script or a LISP routine there is no dialog. The
//! dialog belongs to the command that asked for it: a confirmation meant for the dialog a command
//! replaced never finishes the replacement.

use cadcraft_engine::Input;
use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_ui_egui::control::ControlRequest;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{Queryable, by};
use serde_json::{Value, json};

/// Block T with the definitions A ("Room", default "101") and B ("Floor", default "1"), and
/// ATTDIA 1 (the default, as in AutoCAD).
fn app() -> CadApp {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.run("setvar", json!({ "name": "ATTDIA", "value": 1 })).unwrap();
    let a = app.run("attdef", json!({ "tag": "A", "prompt": "Room", "default": "101", "at": [0, 2] })).unwrap();
    let b = app.run("attdef", json!({ "tag": "B", "prompt": "Floor", "default": "1", "at": [0, 4] })).unwrap();
    app.run("block", json!({ "name": "T", "base": [0, 0], "handles": [a["handle"], b["handle"]], "keep": "delete" })).unwrap();
    app
}

/// INSERT of T typed, answered up to its attribute values.
fn insert(app: &mut CadApp) {
    app.start("insert");
    app.cmdline("T");
    for i in [Input::Point(Vec2::new(50.0, 0.0)), Input::Enter, Input::Enter, Input::Enter] {
        app.session.input(i).unwrap();
    }
}

/// One frame as eframe runs it: the raw hook puts in what `ui.click`/`ui.key` queued (one pointer
/// event a frame, as the app delivers them step by step), then the frame itself drains the
/// control requests in `logic` and draws the dialog.
fn frame(h: &mut Harness<'static, CadApp>) {
    let ctx = h.ctx.clone();
    let mut raw = egui::RawInput::default();
    h.state_mut().raw_input_hook(&ctx, &mut raw);
    h.input_mut().events.extend(raw.events);
    h.run_steps(1);
}

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(900.0, 700.0)).build_ui_state(|ui, app: &mut CadApp| dialogs::show(app, ui.ctx()), app);
    h.run_steps(3);
    h
}

fn values(app: &CadApp) -> Option<Vec<(String, String)>> {
    let d = app.session.doc().unwrap();
    d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Insert(i) => Some(i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()),
        _ => None,
    })
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(Role::TextInput).value(shown))
}

fn replace(h: &mut Harness<'static, CadApp>, shown: &str, with: &str) {
    field(h, shown).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(h, shown).type_text(with);
    h.run_steps(2);
}

/// An app with a control channel, its old INSERT's dialog drawn: the harness, the channel and
/// where that dialog's OK is.
fn control_harness() -> (Harness<'static, CadApp>, std::sync::mpsc::Sender<ControlRequest>, egui::Pos2) {
    let mut a = app();
    insert(&mut a);
    let (tx, rx) = std::sync::mpsc::channel();
    // The app's own frame: `logic` drains the control requests, then the UI draws the dialog.
    let mut h = Harness::builder().with_size(vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        a.with_control(rx),
    );
    frame(&mut h);
    frame(&mut h);
    let ok = h.get_by_label("OK").rect().center();
    (h, tx, ok)
}

/// One press or release of the primary button at `pos`, as the window delivers it.
fn pointer(h: &mut Harness<'static, CadApp>, pos: egui::Pos2, pressed: bool) {
    if pressed {
        h.input_mut().events.push(egui::Event::PointerMoved(pos));
    }
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    });
}

/// The control batch the app drains in one go: cancel the old INSERT, start another INSERT of the
/// same block up to its attribute prompts, then `confirmation` — the input meant for the old
/// dialog — queued for the frames after it.
fn replacement(tx: &std::sync::mpsc::Sender<ControlRequest>, confirmation: Option<(&str, Value)>) -> Vec<cadcraft_ui_egui::control::PendingReply> {
    let mut requests = vec![
        ("cmdline.key", json!({ "key": "escape" })),
        ("cmdline.input", json!({ "text": "insert" })),
        ("cmdline.input", json!({ "text": "T" })),
        ("cmdline.input", json!({ "text": "50,0" })),
        ("cmdline.input", json!({ "text": "" })),
        ("cmdline.input", json!({ "text": "" })),
        ("cmdline.input", json!({ "text": "" })),
    ];
    requests.extend(confirmation);
    let mut replies = Vec::new();
    for (method, params) in requests {
        let (req, reply) = ControlRequest::new(method, params);
        tx.send(req).expect("the app listens for control requests");
        replies.push(reply);
    }
    replies
}

/// The replacement is still waiting for its values, showing its own defaults.
fn assert_replacement_asks(h: &Harness<'static, CadApp>) {
    assert!(h.state().session.attribute_entry().is_some(), "the replacement's dialog is still up");
    assert_eq!(h.get_by_label("Room").value().as_deref(), Some("101"), "showing its own default");
    assert_eq!(h.get_by_label("Floor").value().as_deref(), Some("1"), "showing its own default");
}

/// A confirmation made once the replacement has been seen completes it with what it showed.
fn confirm_the_replacement(h: &mut Harness<'static, CadApp>) {
    h.get_by_label("OK").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none(), "{}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "101".into()), ("B".into(), "1".into())]));
}

#[test]
fn ok_gives_the_values_to_insert() {
    let mut a = app();
    insert(&mut a);
    let mut h = harness(a);
    h.get_by_label("Edit Attributes");
    // The fields are named by their prompts.
    assert_eq!(h.get_by_label("Room").value().as_deref(), Some("101"));
    replace(&mut h, "1", "3");
    replace(&mut h, "101", "205");
    h.get_by_label("OK").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none(), "{}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "205".into()), ("B".into(), "3".into())]));
    assert!(h.query_by_label("Edit Attributes").is_none(), "closed");
}

#[test]
fn enter_in_a_field_is_ok_and_verify_is_not_asked_again() {
    let mut a = app();
    a.run("battman", json!({ "name": "T", "action": "edit", "tag": "A", "default": "7" })).unwrap();
    // Make A a Verify attribute in the block definition.
    let d = a.session.doc_mut().unwrap();
    let mut b = (**d.block("T").unwrap()).clone();
    for e in b.entities.iter().map(|e| e.handle).collect::<Vec<_>>() {
        b.entities.modify(e, |e| {
            if let EntityKind::AttDef(ad) = &mut e.kind
                && ad.tag == "A"
            {
                ad.modes.verify = true;
            }
        });
    }
    d.blocks.insert("T".into(), std::sync::Arc::new(b));
    insert(&mut a);
    let mut h = harness(a);
    replace(&mut h, "7", "8");
    h.key_press(Key::Enter);
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none(), "{}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "8".into()), ("B".into(), "1".into())]));
}

#[test]
fn cancel_inserts_nothing() {
    let mut a = app();
    insert(&mut a);
    let mut h = harness(a);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none());
    assert_eq!(values(app), None);
}

#[test]
fn a_new_insert_does_not_inherit_a_cancelled_draft() {
    let mut a = app();
    insert(&mut a);
    let mut h = harness(a);
    replace(&mut h, "101", "205");
    replace(&mut h, "1", "3");
    let first = h.state().session.attribute_entry().expect("the old dialog's rows");
    // The command channel cancels, and a second INSERT of the same block reaches its attribute
    // prompts before another frame: the draft the old dialog left is still in egui's data.
    h.state_mut().session.cancel();
    insert(h.state_mut());
    let second = h.state().session.attribute_entry().expect("the replacement's rows");
    assert_ne!((second.drawing, second.command), (first.drawing, first.command), "another command asks");
    assert_eq!(second.drawing, h.state().session.state().unwrap().uid);
    h.run_steps(2);
    assert_eq!(h.get_by_label("Room").value().as_deref(), Some("101"), "the replacement's own default");
    assert_eq!(h.get_by_label("Floor").value().as_deref(), Some("1"), "the replacement's own default");
    // A confirmation made once the replacement has been drawn completes it with what it shows.
    h.get_by_label("OK").click();
    h.run_steps(3);
    let app = h.state();
    assert!(app.session.running.is_none(), "{}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "101".into()), ("B".into(), "1".into())]));
}

#[test]
fn an_ok_clicked_on_the_old_dialog_does_not_finish_the_replacement() {
    let (mut h, tx, ok) = control_harness();
    let replies = replacement(&tx, Some(("ui.click", json!({ "x": ok.x, "y": ok.y }))));
    // The click arrives in the frames after the one that drained it, so the replacement must not
    // take it: it has not been seen, and its defaults are not confirmed.
    for _ in 0..5 {
        frame(&mut h);
    }
    for r in replies {
        assert_eq!(r.try_reply().expect("answered")["ok"], true);
    }
    assert_replacement_asks(&h);
    confirm_the_replacement(&mut h);
}

#[test]
fn an_enter_keyed_in_the_old_dialog_does_not_finish_the_replacement() {
    let (mut h, tx, _) = control_harness();
    let replies = replacement(&tx, Some(("ui.key", json!({ "key": "enter" }))));
    // Enter reaches egui whole in the frame after the batch: the replacement takes none of it.
    for _ in 0..4 {
        frame(&mut h);
    }
    for r in replies {
        assert_eq!(r.try_reply().expect("answered")["ok"], true);
    }
    assert_replacement_asks(&h);
    confirm_the_replacement(&mut h);
}

#[test]
fn a_press_on_the_old_dialog_does_not_confirm_the_replacement() {
    let (mut h, tx, ok) = control_harness();
    // The button goes down on the old dialog's OK...
    pointer(&mut h, ok, true);
    h.run_steps(1);
    // ...the command is replaced while it is still down...
    let replies = replacement(&tx, None);
    frame(&mut h);
    frame(&mut h);
    // ...and it comes up where the replacement's OK is.
    pointer(&mut h, ok, false);
    h.run_steps(2);
    for r in replies {
        assert_eq!(r.try_reply().expect("answered")["ok"], true);
    }
    assert_replacement_asks(&h);
    confirm_the_replacement(&mut h);
}

#[test]
fn a_script_never_opens_the_dialog() {
    let mut a = app();
    // A script that ends before the first attribute answer leaves INSERT waiting.
    a.session.script("-INSERT T 50,0 1 1 0\n").unwrap();
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none(), "a script never opens the dialog");
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");
}

#[test]
fn a_lisp_insert_never_opens_the_dialog() {
    let mut a = app();
    // (command "-INSERT" …) through the programmatic route: a routine cannot click a dialog,
    // and it ends before the first attribute answer.
    a.run("lisp", json!({ "code": "(command \"-INSERT\" \"T\" \"0,0\" 1 1 0)" })).unwrap();
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none(), "a LISP-started INSERT never opens the dialog");
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");
}

/// A routine that types a command into a running LINE, as a person's tokens would arrive.
fn lisp_token_replaces_line(spelling: &str, extra: &str) -> CadApp {
    let mut a = app();
    let code = format!("(command \"LINE\" \"0,0\" \"{spelling}\" \"T\" \"50,0\" 1 1 0{extra})");
    a.run("lisp", json!({ "code": code })).unwrap();
    a
}

#[test]
fn a_lisp_insert_token_replacing_a_command_never_opens_the_dialog() {
    // The token reaches the typed-input path while LINE runs, not the idle start: the INSERT
    // that replaces LINE is still the routine's, so ATTDIA 1 opens no dialog.
    let h = harness(lisp_token_replaces_line("'-INSERT", ""));
    assert!(h.query_by_label("Edit Attributes").is_none(), "a LISP token never opens the dialog");
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");
}

#[test]
fn a_lisp_minsert_token_replacing_a_command_never_opens_the_dialog() {
    let h = harness(lisp_token_replaces_line("'MINSERT", " 1 1"));
    assert!(h.query_by_label("Edit Attributes").is_none(), "a LISP token never opens the dialog");
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");
}

#[test]
fn a_person_typing_a_transparent_insert_still_opens_the_dialog() {
    // What the routine feeds above, typed by a person at the command line.
    let mut a = app();
    a.cmdline("LINE 0,0 '-INSERT T 50,0 1 1 0");
    let h = harness(a);
    h.get_by_label("Edit Attributes");
    assert!(h.state().session.attribute_entry().is_some());
}

#[test]
fn a_person_replacing_paused_lisp_keeps_their_insert_dialog() {
    // A routine waits in `(command "LINE" "0,0" pause)`; the person's `'-INSERT` cancels it,
    // and the routine's `*error*` handler starts a LINE of its own. That handler command is the
    // program's; the dialog still belongs to the person's INSERT.
    let mut a = app();
    a.cmdline("(defun *error* (msg) (command \"LINE\" \"1,1\" \"2,2\" \"\"))");
    a.cmdline("(command \"LINE\" \"0,0\" pause)");
    a.cmdline("'-INSERT T 50,0 1 1 0");
    assert!(a.session.attribute_entry().is_some(), "the person's INSERT asks in the dialog: {}", a.session.prompt_text());
    let h = harness(a);
    h.get_by_label("Edit Attributes");
    assert_eq!(h.state().session.command_origin(), cadcraft_engine::Origin::Person);
}

#[test]
fn a_person_replacing_paused_lisp_keeps_their_minsert_dialog() {
    let mut a = app();
    a.cmdline("(defun *error* (msg) (command \"LINE\" \"1,1\" \"2,2\" \"\"))");
    a.cmdline("(command \"LINE\" \"0,0\" pause)");
    a.cmdline("'-MINSERT T 50,0 1 1 0 1 1");
    assert!(a.session.attribute_entry().is_some(), "the person's MINSERT asks in the dialog: {}", a.session.prompt_text());
    let h = harness(a);
    h.get_by_label("Edit Attributes");
    assert_eq!(h.state().session.command_origin(), cadcraft_engine::Origin::Person);
}

#[test]
fn a_typed_or_menu_insert_still_opens_the_dialog() {
    // Typed at the command line.
    let mut a = app();
    insert(&mut a);
    let h = harness(a);
    h.get_by_label("Edit Attributes");
    assert!(h.state().session.attribute_entry().is_some());

    // Started from Insert ▸ Block….
    let mut a = app();
    menus::activate(&mut a, "insert");
    a.cmdline("T");
    for i in [Input::Point(Vec2::new(50.0, 0.0)), Input::Enter, Input::Enter, Input::Enter] {
        a.session.input(i).unwrap();
    }
    let h = harness(a);
    h.get_by_label("Edit Attributes");
    assert!(h.state().session.attribute_entry().is_some());
}

#[test]
fn a_fresh_session_starts_with_attdia_1() {
    // AutoCAD's initial value is 1: a typed INSERT opens the dialog without SETVAR.
    let mut a = app();
    a.session.settings = Default::default();
    assert!(a.session.settings.attdia);
    insert(&mut a);
    let h = harness(a);
    h.get_by_label("Edit Attributes");
    assert!(h.state().session.attribute_entry().is_some());
}

#[test]
fn a_canvas_click_does_not_take_the_first_value_or_close_the_dialog() {
    let mut a = app();
    insert(&mut a);
    // A full frame, not only the dialog, so the click goes through the canvas.
    let mut h = Harness::builder().with_size(vec2(900.0, 700.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        a,
    );
    h.run_steps(3);
    h.get_by_label("Edit Attributes");
    replace(&mut h, "101", "202");
    // A click on empty canvas away from the dialog reaches INSERT as Input::Point.
    let rect = h.state().canvas.rect.expect("the canvas is drawn");
    let pos = rect.left_center() + vec2(10.0, 0.0);
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(3);
    h.get_by_label("Edit Attributes");
    assert!(h.state().session.attribute_entry().is_some(), "the point did not close the dialog");
    field(&h, "202");
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert_eq!(values(h.state()), Some(vec![("A".into(), "202".into()), ("B".into(), "1".into())]));
}

#[test]
fn no_dialog_with_attdia_0_or_from_json() {
    let mut a = app();
    a.run("setvar", json!({ "name": "ATTDIA", "value": 0 })).unwrap();
    insert(&mut a);
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none());
    assert!(h.state().session.prompt_text().contains("Room"), "asked at the command line");

    let mut a = app();
    a.run("insert", json!({ "name": "T", "at": [0, 0], "attribs": { "A": "x" } })).unwrap();
    let h = harness(a);
    assert!(h.query_by_label("Edit Attributes").is_none());
    assert_eq!(values(h.state()), Some(vec![("A".into(), "x".into()), ("B".into(), "1".into())]));
}

#[test]
fn a_control_click_on_ok_confirms_the_dialog_it_was_meant_for() {
    // The click the tests above keep from a replacement does confirm the dialog it is meant for:
    // their queued input reaches egui and the button.
    let (mut h, tx, ok) = control_harness();
    let (req, reply) = ControlRequest::new("ui.click", json!({ "x": ok.x, "y": ok.y }));
    tx.send(req).expect("the app listens for control requests");
    for _ in 0..5 {
        frame(&mut h);
    }
    assert_eq!(reply.try_reply().expect("answered")["ok"], true);
    let app = h.state();
    assert!(app.session.running.is_none(), "the click confirmed the dialog: {}", app.session.prompt_text());
    assert_eq!(values(app), Some(vec![("A".into(), "101".into()), ("B".into(), "1".into())]));
}
