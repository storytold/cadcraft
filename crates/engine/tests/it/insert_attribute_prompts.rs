//! INSERT's attribute questions as AutoCAD 2024 asks them from AutoLISP's `command` (accoreconsole,
//! the attributes read back with `entget`).

use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Vec2;
use cadcraft_engine::{Input, Session};

/// A session with block K: a circle and attributes (tag, DXF flags, default value), each with the
/// prompt "P" + tag.
fn with_block(defs: &[(&str, i32, &str)]) -> Session {
    // `entmake` goes through the DXF codec, as in the app.
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    let mut code =
        String::from("(entmake '((0 . \"BLOCK\") (2 . \"K\") (70 . 2) (10 0.0 0.0 0.0))) (entmake '((0 . \"CIRCLE\") (10 0.0 0.0 0.0) (40 . 3.0)))");
    for (i, (tag, flags, default)) in defs.iter().enumerate() {
        let y = 5.0 + 3.0 * i as f64;
        code += &format!(
            " (entmake '((0 . \"ATTDEF\") (10 0.0 {y:.1} 0.0) (40 . 2.5) (1 . \"{default}\") (3 . \"P{tag}\") (2 . \"{tag}\") (70 . {flags})))"
        );
    }
    code += " (entmake '((0 . \"ENDBLK\")))";
    s.cmdline(&format!("(progn {code})")).unwrap();
    s.log.clear();
    s
}

/// The attribute values of the last block reference, by tag.
fn values(s: &Session) -> Vec<(String, String)> {
    let d = s.doc().unwrap();
    let Some(EntityKind::Insert(i)) = d.model.iter().last().map(|e| &e.kind) else { panic!("no block reference") };
    i.attribs.iter().map(|a| (a.tag.clone(), a.text.value.clone())).collect()
}

fn said(s: &Session, line: &str) -> usize {
    s.log.iter().filter(|l| l.as_str() == line).count()
}

fn pair(tag: &str, value: &str) -> (String, String) {
    (tag.to_string(), value.to_string())
}

#[test]
fn attreq_0_inserts_the_default_values_without_asking() {
    let mut s = with_block(&[("T1", 0, "V1"), ("T2", 0, "V2")]);
    s.cmdline("(progn (setvar \"ATTREQ\" 0) (command \"_.-INSERT\" \"K\" '(100.0 0.0) \"\" \"\" 0))").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), [pair("T1", "V1"), pair("T2", "V2")]);
    assert_eq!(said(&s, "Enter attribute values"), 0, "{:?}", s.log);
    s.cmdline("(setvar \"ATTREQ\" 1)").unwrap();
    s.cmdline("(princ (getvar \"ATTREQ\"))").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("1"), "{:?}", s.log);
}

#[test]
fn a_preset_attribute_is_not_asked() {
    let mut s = with_block(&[("T1", 8, "V1"), ("T2", 0, "V2")]);
    s.cmdline("(command \"_.-INSERT\" \"K\" '(200.0 0.0) \"\" \"\" 0 \"X\")").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), [pair("T1", "V1"), pair("T2", "X")]);
    assert_eq!(said(&s, "Enter attribute values"), 1, "{:?}", s.log);
}

#[test]
fn a_verify_attribute_is_asked_again_with_the_value_typed() {
    let mut s = with_block(&[("T1", 4, "V1")]);
    s.cmdline("(command \"_.-INSERT\" \"K\" '(300.0 0.0) \"\" \"\" 0 \"X\")").unwrap();
    assert_eq!(said(&s, "Verify attribute values"), 1, "{:?}", s.log);
    assert!(s.prompt_text().contains("PT1 <X>"), "{}", s.prompt_text());
    // A click is no answer.
    s.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    assert!(s.prompt_text().contains("PT1 <X>"), "{}", s.prompt_text());
    // Typed at the prompt, the whole line is the value.
    s.cmdline("Y").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(values(&s), [pair("T1", "Y")]);
    // Enter keeps the value typed.
    let mut s = with_block(&[("T1", 4, "V1")]);
    s.cmdline("(command \"_.-INSERT\" \"K\" '(300.0 0.0) \"\" \"\" 0 \"X\" \"\")").unwrap();
    assert_eq!(values(&s), [pair("T1", "X")]);
}
