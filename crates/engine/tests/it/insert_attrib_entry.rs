//! INSERT and the attribute modes (gap F17): Preset and constant attributes take their values
//! without asking, ATTREQ 0 gives every attribute its default, ATTDIA 0 asks at the command line
//! (a Verify attribute's value again once every value is in) and ATTDIA 1 (the default) asks for
//! the values all at once in the Edit Attributes dialog (`Session::attribute_entry`), typed
//! answers still working. The JSON form never asks. ATTREQ and ATTDIA are user settings, as in
//! AutoCAD (DWG keeps them in its header only before R2000).

use std::sync::Arc;

use cadcraft_engine::doc::{Attrib, AttribModes, Block, Entity, EntityKind, Handle, Text};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{AttribAsk, Input, Origin, Session};
use serde_json::json;

fn def(tag: &str, prompt: &str, default: &str, y: f64, constant: bool, modes: AttribModes) -> Attrib {
    Attrib {
        tag: tag.into(),
        text: Text {
            insert: Vec3::new(0.0, y, 0.0),
            align_pt: None,
            height: 1.0,
            value: default.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        },
        invisible: false,
        constant,
        prompt: prompt.into(),
        props: Default::default(),
        multiline: None,
        modes,
    }
}

/// Block T with the definitions A (Verify), B (Preset), C (plain) and D (constant); ATTREQ and
/// ATTDIA as given (their defaults, ATTREQ 1 and ATTDIA 1, otherwise).
fn session(attreq: Option<bool>, attdia: Option<bool>) -> Session {
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let mut b = Block::new("T");
    let none = AttribModes::default();
    for a in [
        def("A", "First", "1", 2.0, false, AttribModes { verify: true, ..none }),
        def("B", "Second", "two", 4.0, false, AttribModes { preset: true, ..none }),
        def("C", "Third", "3", 6.0, false, none),
        def("D", "", "fixed", 8.0, true, none),
    ] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::AttDef(a)));
    }
    d.blocks.insert("T".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "attribs", None);
    if let Some(v) = attreq {
        s.settings.attreq = v;
    }
    if let Some(v) = attdia {
        s.settings.attdia = v;
    }
    s
}

/// Start INSERT of T at 100,0 and answer the scale and rotation prompts.
fn insert_up_to_the_attributes(s: &mut Session) {
    s.start("insert").unwrap();
    s.cmdline("T").unwrap();
    s.input(Input::Point(Vec2::new(100.0, 0.0))).unwrap();
    s.input(Input::Enter).unwrap(); // X scale 1
    s.input(Input::Enter).unwrap(); // Y scale = X
    s.input(Input::Enter).unwrap(); // rotation 0
}

/// The attribute values of the last block reference, in order.
fn values(s: &Session) -> Vec<(String, String)> {
    let d = s.doc().unwrap();
    let Some(EntityKind::Insert(i)) = d.model.iter().filter(|e| matches!(e.kind, EntityKind::Insert(_))).last().map(|e| e.kind.clone()) else {
        panic!("no block reference")
    };
    i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(t, v)| (t.to_string(), v.to_string())).collect()
}

#[test]
fn a_fresh_session_reads_attdia_1() {
    // AutoCAD 2026's ATTDIA page: Type Integer, Saved in Registry, Initial value 1.
    let mut s = session(None, None);
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTDIA" })).unwrap()["value"], 1);
    assert!(s.settings.attdia);
}

#[test]
fn typed_insert_asks_verify_again_and_skips_preset_and_constant() {
    // ATTDIA 0 set explicitly, so the command line asks.
    let mut s = session(Some(true), Some(false));
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTDIA" })).unwrap()["value"], 0);
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTREQ" })).unwrap()["value"], 1);
    insert_up_to_the_attributes(&mut s);
    assert!(s.attribute_entry().is_none(), "ATTDIA 0: no dialog");
    assert_eq!(s.prompt_text(), "INSERT First <1>:", "A first");
    s.input(Input::Text("10".into())).unwrap();
    assert_eq!(s.prompt_text(), "INSERT Third <3>:", "B is Preset: not asked");
    s.input(Input::Enter).unwrap(); // C takes its default
    // Then the Verify attributes, with the values typed.
    assert_eq!(s.prompt_text(), "INSERT First <10>:");
    s.input(Input::Text("11".into())).unwrap(); // the value typed at Verify replaces it
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "11"), ("B", "two"), ("C", "3")]));

    // Enter at Verify keeps the value given; Enter at A takes its default, verified.
    insert_up_to_the_attributes(&mut s);
    s.input(Input::Enter).unwrap();
    s.input(Input::Text("c".into())).unwrap();
    assert_eq!(s.prompt_text(), "INSERT First <1>:");
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "1"), ("B", "two"), ("C", "c")]));

    // A script answers in turn too.
    s.script("-INSERT T 0,50 1 1 0\na\nc\nz\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "z"), ("B", "two"), ("C", "c")]));
}

#[test]
fn a_point_does_not_answer_an_attribute_prompt() {
    // A canvas click reaches INSERT as Input::Point while a text-only attribute prompt waits.
    // It is no answer: the prompt stays, and only Enter keeps the default.
    let mut s = session(Some(true), Some(false));
    insert_up_to_the_attributes(&mut s);
    assert_eq!(s.prompt_text(), "INSERT First <1>:");
    s.input(Input::Point(Vec2::new(20.0, 20.0))).unwrap();
    assert_eq!(s.prompt_text(), "INSERT First <1>:", "the click did not take A's default");
    assert!(s.attribute_entry().is_none(), "ATTDIA 0: no dialog");
    s.input(Input::Text("10".into())).unwrap();
    assert_eq!(s.prompt_text(), "INSERT Third <3>:", "A was answered by the typed text");
    s.input(Input::Enter).unwrap(); // C takes its default
    assert_eq!(s.prompt_text(), "INSERT First <10>:", "then A's Verify");
    s.input(Input::Enter).unwrap(); // Verify keeps the value typed
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "10"), ("B", "two"), ("C", "3")]));
}

#[test]
fn attreq_0_gives_every_attribute_its_default() {
    let mut s = session(Some(false), None);
    insert_up_to_the_attributes(&mut s);
    assert!(s.running.is_none(), "no attribute prompt: {}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "1"), ("B", "two"), ("C", "3")]));
    // SETVAR changes it.
    s.execute("setvar", &json!({ "name": "ATTREQ", "value": 1 })).unwrap();
    s.execute("setvar", &json!({ "name": "ATTDIA", "value": 0 })).unwrap();
    insert_up_to_the_attributes(&mut s);
    assert_eq!(s.prompt_text(), "INSERT First <1>:");
}

#[test]
fn attdia_1_asks_for_every_value_at_once_and_typed_answers_still_work() {
    let mut s = session(None, None);
    s.execute("setvar", &json!({ "name": "ATTDIA", "value": 1 })).unwrap();
    assert_eq!(s.execute("getvar", &json!({ "name": "ATTDIA" })).unwrap()["value"], 1);
    s.start("insert").unwrap();
    s.cmdline("T").unwrap();
    assert!(s.attribute_entry().is_none(), "not before the attribute prompts");
    s.input(Input::Point(Vec2::new(100.0, 0.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    let e = s.attribute_entry().expect("the dialog's rows");
    assert_eq!(e.block, "T");
    let ask =
        |tag: &str, prompt: &str, default: &str, verify: bool| AttribAsk { tag: tag.into(), prompt: prompt.into(), default: default.into(), verify };
    assert_eq!(e.rows, vec![ask("A", "First", "1", true), ask("C", "Third", "3", false)]);
    // The dialog's OK: every value, then the Verify attribute's again.
    let answers = e.answers(&["x".into(), String::new()]);
    assert_eq!(answers, ["x", "", "x"]);
    for a in answers {
        s.input(Input::Text(a)).unwrap();
    }
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "x"), ("B", "two"), ("C", "")]));

    // Typed instead: the dialog goes away after the first answer.
    insert_up_to_the_attributes(&mut s);
    assert!(s.attribute_entry().is_some());
    s.input(Input::Text("typed".into())).unwrap();
    assert!(s.attribute_entry().is_none());
    s.input(Input::Enter).unwrap(); // C takes its default
    s.input(Input::Enter).unwrap(); // Verify keeps the value typed
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "typed"), ("B", "two"), ("C", "3")]));

    // Cancel from the dialog inserts nothing.
    let before = s.doc().unwrap().model.len();
    insert_up_to_the_attributes(&mut s);
    s.cancel();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.len(), before);
}

#[test]
fn json_insert_never_asks_whatever_attreq_and_attdia_say() {
    for (req, dia) in [(false, false), (true, true)] {
        let mut s = session(Some(req), Some(dia));
        let r = s.execute("insert", &json!({ "name": "T", "at": [0, 0], "attribs": { "a": "given", "B": "preset given" } })).unwrap();
        assert!(r["handle"].is_string());
        assert!(s.running.is_none() && s.attribute_entry().is_none());
        assert_eq!(values(&s), pairs(&[("A", "given"), ("B", "preset given"), ("C", "3")]));
    }
}

#[test]
fn hostile_values_never_panic() {
    let mut s = session(None, Some(true));
    insert_up_to_the_attributes(&mut s);
    let e = s.attribute_entry().expect("asks");
    // Too few answers take the defaults, too many are ignored.
    assert_eq!(e.answers(&[]), ["1", "3", "1"]);
    assert_eq!(e.answers(&["p".into(), "q".into(), "r".into()]), ["p", "q", "p"]);
    s.cancel();
    for bad in [json!(2), json!(-1), json!("x"), json!(null), json!(1.5)] {
        assert!(s.execute("setvar", &json!({ "name": "ATTDIA", "value": bad })).is_err(), "{bad}");
        assert!(s.execute("setvar", &json!({ "name": "ATTREQ", "value": bad })).is_err(), "{bad}");
    }
    assert!(s.settings.attdia && s.settings.attreq, "a refused value changes nothing");
    // An INSERT of a block with only Preset and constant attributes asks nothing.
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let mut b = Block::new("P");
    let h: Handle = d.new_handle();
    b.entities.push(Entity::new(h, EntityKind::AttDef(def("P", "p", "v", 0.0, false, AttribModes { preset: true, ..AttribModes::default() }))));
    d.blocks.insert("P".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "preset", None);
    s.script("-INSERT P 0,0 1 1 0\n").unwrap();
    assert!(s.running.is_none() && s.attribute_entry().is_none());
    assert_eq!(values(&s), pairs(&[("P", "v")]));
}

#[test]
fn a_script_never_asks_in_the_dialog() {
    // A script cannot click a dialog: ATTDIA 1 keeps its prompts at the command line, even when
    // the script ends with INSERT still waiting for the first attribute.
    let mut s = session(None, Some(true));
    s.script("-INSERT T 0,50 1 1 0\n").unwrap();
    assert_eq!(s.prompt_text(), "INSERT First <1>:", "INSERT still waits after the script");
    assert!(s.attribute_entry().is_none(), "a scripted INSERT never opens the dialog");
    // The command the script started keeps answering at the command line.
    s.input(Input::Text("a".into())).unwrap();
    s.input(Input::Text("c".into())).unwrap();
    s.input(Input::Text("a".into())).unwrap(); // A's Verify
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "a"), ("B", "two"), ("C", "c")]));
    // A typed INSERT with ATTDIA 1 still asks in the dialog.
    insert_up_to_the_attributes(&mut s);
    assert!(s.attribute_entry().is_some(), "a typed INSERT asks in the dialog");
}

#[test]
fn a_lisp_insert_never_asks_in_the_dialog() {
    // (command "-INSERT" …) runs INSERT outside Session::script: the routine cannot click a
    // dialog, and it finishes before the first attribute answer.
    let mut s = session(None, Some(true));
    let r = s.execute("lisp", &json!({ "code": "(command \"-INSERT\" \"T\" \"0,50\" 1 1 0)" })).unwrap();
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(s.prompt_text(), "INSERT First <1>:", "INSERT waits at the command line");
    assert!(s.attribute_entry().is_none(), "a LISP-started INSERT never opens the dialog");
    // The command keeps answering at the command line after the routine ended.
    s.input(Input::Text("a".into())).unwrap();
    s.input(Input::Text("c".into())).unwrap();
    s.input(Input::Text("a".into())).unwrap(); // A's Verify
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), pairs(&[("A", "a"), ("B", "two"), ("C", "c")]));
    // A typed INSERT with ATTDIA 1 asks in the dialog again.
    insert_up_to_the_attributes(&mut s);
    assert!(s.attribute_entry().is_some(), "a typed INSERT asks in the dialog");
    s.cancel();

    // A LISP expression typed at the command line is the same program: no dialog.
    s.cmdline("(command \"-INSERT\" \"T\" \"0,60\" 1 1 0)").unwrap();
    assert_eq!(s.prompt_text(), "INSERT First <1>:");
    assert!(s.attribute_entry().is_none(), "a typed (command …) never opens the dialog");
    s.cancel();
}

/// A routine that types a command into a running LINE: `(command "LINE" "0,0" spelling …)`.
/// `extra` answers MINSERT's grid, and the arguments after it answer the attribute prompts.
fn lisp_token_replaces_line(spelling: &str, extra: &str) -> Session {
    let mut s = session(None, Some(true));
    let code = format!("(command \"LINE\" \"0,0\" \"{spelling}\" \"T\" \"0,50\" 1 1 0{extra})");
    let r = s.execute("lisp", &json!({ "code": code })).unwrap();
    assert!(r.get("error").is_none(), "{spelling}: {r}");
    s
}

/// Answer the attribute prompts the routine's INSERT left waiting, as the person at the
/// command line would.
fn answer_the_routines_insert(s: &mut Session) {
    s.input(Input::Text("a".into())).unwrap();
    s.input(Input::Text("c".into())).unwrap();
    s.input(Input::Text("a".into())).unwrap(); // A's Verify
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(s), pairs(&[("A", "a"), ("B", "two"), ("C", "c")]));
}

#[test]
fn a_lisp_insert_token_replacing_a_command_never_asks_in_the_dialog() {
    // The token reaches the typed-input path while LINE runs, not the idle start: the INSERT
    // that replaces LINE is still the routine's, so ATTDIA 1 opens no dialog nobody can click.
    let mut s = lisp_token_replaces_line("'-INSERT", "");
    assert_eq!(s.prompt_text(), "INSERT First <1>:", "asked at the command line");
    assert!(s.attribute_entry().is_none(), "a LISP token never opens the dialog");
    // The command keeps answering at the command line after the routine ended.
    answer_the_routines_insert(&mut s);
}

#[test]
fn a_lisp_minsert_token_replacing_a_command_never_asks_in_the_dialog() {
    let mut s = lisp_token_replaces_line("'MINSERT", " 1 1");
    assert_eq!(s.prompt_text(), "MINSERT First <1>:", "asked at the command line");
    assert!(s.attribute_entry().is_none(), "a LISP token never opens the dialog");
    answer_the_routines_insert(&mut s);
}

#[test]
fn a_person_typing_a_transparent_insert_still_asks_in_the_dialog() {
    // The tokens the routine feeds above, typed by a person: the INSERT that replaces LINE is
    // theirs, so ATTDIA 1 asks in the dialog.
    let mut s = session(None, Some(true));
    s.cmdline("LINE 0,0 '-INSERT T 0,50 1 1 0").unwrap();
    assert!(s.running.is_some(), "{}", s.prompt_text());
    assert!(s.attribute_entry().is_some(), "a typed transparent INSERT asks in the dialog");
    s.cancel();
}

/// A person typing `'NAME T …` while a routine waits in `(command "LINE" "0,0" pause)`
/// cancels it; the routine's `*error*` handler then starts a LINE of its own. The handler's
/// command is the program's, and it must not take the origin or number of the person's start.
/// `extra` answers MINSERT's rows and columns.
fn person_replaces_paused_lisp(spelling: &str, extra: &str) -> Session {
    let mut s = session(None, Some(true));
    s.cmdline("(defun *error* (msg) (command \"LINE\" \"1,1\" \"2,2\" \"\"))").unwrap();
    s.cmdline("(command \"LINE\" \"0,0\" pause)").unwrap();
    assert!(s.running.is_some(), "the routine's LINE waits for the person: {}", s.prompt_text());
    // The person replaces the waiting routine with their own command.
    s.cmdline(&format!("'{spelling} T 0,50 1 1 0{extra}")).unwrap();
    assert!(s.running.is_some(), "{}", s.prompt_text());
    // The handler's LINE is the only one drawn: the paused LINE was cancelled at its first
    // point, and the handler's own command ran as the program's (nobody typed 1,1 or 2,2).
    let lines: Vec<(Vec2, Vec2)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
            _ => None,
        })
        .collect();
    assert_eq!(lines, vec![(Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0))], "the error handler's LINE is drawn, and the paused one added nothing");
    s
}

#[test]
fn a_person_replacing_paused_lisp_keeps_their_insert_dialog() {
    let s = person_replaces_paused_lisp("-INSERT", "");
    assert_eq!(s.command_origin(), Origin::Person, "the person's INSERT is not the handler's command");
    let e = s.attribute_entry().expect("ATTDIA 1: the person's INSERT asks in the dialog");
    // The handler's LINE finished as a command of its own; the INSERT's answers belong to a
    // later number, so the dialog a cancelled command left behind is not taken.
    assert_eq!(s.last_finished.map(|(command, _)| command), Some(e.command.wrapping_sub(1)), "the handler's LINE keeps its own number");
}

#[test]
fn a_person_replacing_paused_lisp_keeps_their_minsert_dialog() {
    let s = person_replaces_paused_lisp("MINSERT", " 1 1");
    assert_eq!(s.command_origin(), Origin::Person, "the person's MINSERT is not the handler's command");
    let e = s.attribute_entry().expect("ATTDIA 1: the person's MINSERT asks in the dialog");
    assert_eq!(s.last_finished.map(|(command, _)| command), Some(e.command.wrapping_sub(1)), "the handler's LINE keeps its own number");
}

#[test]
fn an_entry_belongs_to_the_drawing_and_command_that_asked() {
    let mut s = session(None, Some(true));
    insert_up_to_the_attributes(&mut s);
    let first = s.attribute_entry().expect("asks");
    assert_eq!(first.drawing, s.state().unwrap().uid);
    assert_eq!(first.command, s.commands_started);
    // Asked again in a later frame: the same owner.
    assert_eq!(s.attribute_entry().map(|e| (e.drawing, e.command)), Some((first.drawing, first.command)));
    // Cancelled and started again: a new number, so the old dialog's draft is not taken.
    s.cancel();
    insert_up_to_the_attributes(&mut s);
    let second = s.attribute_entry().expect("asks again");
    assert_eq!(second.block, "T");
    assert_ne!((second.drawing, second.command), (first.drawing, first.command));
    // Another drawing's INSERT is another owner too.
    let mut other = session(None, Some(true));
    insert_up_to_the_attributes(&mut other);
    let third = other.attribute_entry().expect("asks there");
    assert_eq!(third.drawing, other.state().unwrap().uid);
    assert_ne!(third.drawing, first.drawing);
}

#[test]
fn two_definitions_with_one_tag_are_both_asked_once() {
    // INSERT counted its answers by tag: the second X was asked for ever.
    let mut d = cadcraft_engine::doc::Drawing::new_imperial();
    let mut b = Block::new("DUP");
    for (prompt, y) in [("first", 0.0), ("second", 2.0)] {
        let h = d.new_handle();
        b.entities.push(Entity::new(h, EntityKind::AttDef(def("X", prompt, "d", y, false, AttribModes::default()))));
    }
    d.blocks.insert("DUP".into(), Arc::new(b));
    let mut s = Session::empty();
    s.open_drawing(d, "dup", None);
    s.settings.attdia = false;
    s.script("-INSERT DUP 0,0 1 1 0\na\nb\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s).len(), 2);
}
