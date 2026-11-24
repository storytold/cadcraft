//! A name given to an INSERT's ATTRIB stays reserved when a command allocates handles on a copy
//! of the drawing (#1660).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, IoHooks, set_entity_codec, set_entity_sub_record, set_io, set_object_records};
use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Line};
use cadcraft_engine::lisp::value::to_text;
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

fn session() -> Session {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    set_entity_sub_record(cadcraft_io::entity_sub_record);
    set_object_records(cadcraft_io::object_records);
    set_io(IoHooks {
        read: |bytes, name| cadcraft_io::read(bytes, name).map_err(|e| e.to_string()),
        write: |drawing, name| cadcraft_io::write(drawing, name).map_err(|e| e.to_string()),
        plot: None,
    });
    Session::new()
}

fn eval(s: &mut Session, code: &str) -> String {
    match s.lisp_eval(code, false, true, Vec::new()) {
        Ok(Some(Ok(v))) => to_text(&v, true),
        Ok(Some(Err(e))) => panic!("{code}: {e}"),
        Ok(None) => panic!("{code}: did not finish"),
        Err(e) => panic!("{code}: {e}"),
    }
}

/// Block K with one attribute, one reference, and `i` naming that reference.
fn attributed() -> Session {
    let mut s = session();
    s.cmdline(
        "(progn (entmake '((0 . \"BLOCK\") (2 . \"K\") (70 . 2) (10 0.0 0.0 0.0))) \
         (entmake '((0 . \"CIRCLE\") (10 0.0 0.0 0.0) (40 . 1.0))) \
         (entmake '((0 . \"ATTDEF\") (10 0.0 5.0 0.0) (40 . 2.5) (1 . \"D1\") (3 . \"P\") (2 . \"T1\") (70 . 0))) \
         (entmake '((0 . \"ENDBLK\"))))",
    )
    .unwrap();
    let ins = s.execute("insert", &json!({ "name": "K", "at": [10, 10], "attribs": { "T1": "A1" } })).unwrap();
    let href = ins["handle"].as_str().unwrap();
    eval(&mut s, &format!("(setq i (handent \"{href}\"))"));
    s
}

/// The command line names the reference's first ATTRIB, with `$HANDSEED` on that handle.
fn name_at_top(s: &mut Session) -> u64 {
    s.doc_mut().unwrap().handseed = u64::MAX - 1;
    s.cmdline("(setq a (entnext i))").unwrap();
    let text = eval(s, "a");
    assert!(text.to_ascii_lowercase().contains("fffffffffffffffe"), "{text}");
    assert_eq!(eval(s, "(cdr (assoc 0 (entget a)))"), "\"ATTRIB\"");
    u64::MAX - 1
}

fn holds(s: &Session, h: u64) -> bool {
    let d = s.doc().unwrap();
    let want = Handle(h);
    d.model.contains(want) || d.layouts.iter().any(|l| l.entities.contains(want)) || d.blocks.values().any(|b| b.entities.contains(want))
}

fn assert_kept(s: &mut Session, issued: u64) {
    assert!(!holds(s, issued), "a copy gave out the named handle {issued:X}");
    assert_eq!(eval(s, "(cdr (assoc 0 (entget a)))"), "\"ATTRIB\"", "the old name still means the attribute");
}

fn one_line(dir: &TempDir) -> String {
    let mut d = Drawing::new_metric();
    d.add(
        &cadcraft_engine::doc::Space::Model,
        Common::default(),
        EntityKind::Line(Line { a: cadcraft_engine::doc::geom::Vec3::ZERO, b: cadcraft_engine::doc::geom::Vec3::new(1.0, 0.0, 0.0) }),
    )
    .unwrap();
    dir.file("one-line.dxf", cadcraft_io::write_dxf(&d))
}

#[test]
fn insert_from_file_keeps_an_issued_attrib_name() {
    let mut s = attributed();
    let issued = name_at_top(&mut s);
    let dir = TempDir::new("attrib-insert");
    let path = one_line(&dir);
    s.execute("insert", &json!({ "file": path, "name": "SRC", "at": [4.0, 4.0] })).unwrap();
    assert_kept(&mut s, issued);
}

#[test]
fn xattach_keeps_an_issued_attrib_name() {
    let mut s = attributed();
    let issued = name_at_top(&mut s);
    let dir = TempDir::new("attrib-xref");
    let path = one_line(&dir);
    s.execute("xattach", &json!({ "path": path, "name": "XR", "at": [8.0, 8.0] })).unwrap();
    assert_kept(&mut s, issued);
}

#[test]
fn audit_fix_keeps_an_issued_attrib_name() {
    let mut s = attributed();
    {
        let d = s.doc_mut().unwrap();
        let line = Entity::new(
            Handle(0xABC),
            EntityKind::Line(Line { a: cadcraft_engine::doc::geom::Vec3::ZERO, b: cadcraft_engine::doc::geom::Vec3::new(1.0, 0.0, 0.0) }),
        );
        d.model.push(line.clone());
        let mut b = Block::new("DUP");
        b.entities.push(line);
        d.blocks.insert("DUP".into(), Arc::new(b));
    }
    let issued = name_at_top(&mut s);
    s.execute("audit", &json!({ "fix": true })).unwrap();
    assert_kept(&mut s, issued);
}

#[test]
fn refclose_keeps_an_issued_attrib_name() {
    let mut s = attributed();
    s.cmdline(
        "(progn (entmake '((0 . \"BLOCK\") (2 . \"R\") (70 . 0) (10 0.0 0.0 0.0))) \
         (entmake '((0 . \"LINE\") (10 0.0 0.0 0.0) (11 2.0 0.0 0.0))) \
         (entmake '((0 . \"ENDBLK\"))))",
    )
    .unwrap();
    let r = s.execute("insert", &json!({ "name": "R", "at": [0, 0] })).unwrap();
    s.execute("refedit", &json!({ "handle": r["handle"] })).unwrap();
    s.execute("line", &json!({ "points": [[0, 1], [3, 1]] })).unwrap();
    let issued = name_at_top(&mut s);
    s.execute("refclose", &json!({ "save": true })).unwrap();
    assert_kept(&mut s, issued);
}
