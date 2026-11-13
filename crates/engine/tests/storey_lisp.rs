//! AutoLISP can't reach objects of storeys other than the current one: `entdel` and `entmod`
//! return nil for them and `ssget` doesn't find them; the current storey's objects are a routine's
//! to change as always.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::Handle;
use serde_json::{Value, json};

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap()
}

fn dump(s: &Session) -> String {
    format!("{:?}", s.doc().unwrap().model.iter().map(|e| (e.handle, e.common.clone(), e.kind.clone())).collect::<Vec<_>>())
}

#[test]
fn lisp_cannot_reach_other_storeys() {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    let eg = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.new", &json!({ "name": "OG1" })).unwrap();
    let og = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    let before = dump(&s);
    let code = format!(
        "(list (entdel (handent \"{0}\")) (entmod (subst (cons 10 (list 5.0 5.0 0.0)) (assoc 10 (entget (handent \"{0}\"))) (entget (handent \"{0}\")))) (sslength (ssget \"X\")))",
        eg.hex()
    );
    let r = s.execute("lisp", &json!({ "code": code })).unwrap();
    assert_eq!(r["value"], Value::from("(nil nil 1)"), "entdel and entmod refuse, ssget X sees one object: {r}");
    assert_eq!(dump(&s), before);
    let code = format!("(entdel (handent \"{}\"))", og.hex());
    s.execute("lisp", &json!({ "code": code })).unwrap();
    assert!(s.doc().unwrap().entity(og).is_none(), "the current storey's objects are the routine's");
}

#[test]
fn entmake_lands_on_the_current_storey_and_entdel_restores_only_there() {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let mut s = Session::new();
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.new", &json!({ "name": "OG1" })).unwrap();
    let og = s.doc().unwrap().storeys.current;
    // (CADCraft's entmake takes no xdata yet; whatever it makes lands on the current storey.)
    let r = s.execute("lisp", &json!({ "code": "(entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0)))" })).unwrap();
    assert!(r["error"].is_null(), "{r}");
    assert_eq!(s.doc().unwrap().model.last().unwrap().common.storey(), Some(og));
    // Deleted on OG1, then (in the same routine) EG becomes current: no bringing it back there.
    let h = line(&mut s, [0.0, 5.0], [1.0, 5.0]);
    let code = format!("(setq e (handent \"{}\")) (entdel e) (command \"STOREY\" \"EG\" \"\") (list (entdel e))", h.hex());
    let r = s.execute("lisp", &json!({ "code": code })).unwrap();
    assert_eq!(r["value"], Value::from("(nil)"), "{r}");
    assert!(s.doc().unwrap().entity(h).is_none());
}
