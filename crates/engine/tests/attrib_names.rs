//! AutoLISP's name of an INSERT's ATTRIB keeps naming that attribute when a command, another
//! session, another document or another thread gives out handles in the drawing or in a copy of
//! it, and installs the result (#1660).

use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, IoHooks, set_entity_codec, set_entity_sub_record, set_io, set_object_records};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Handle, Line, Space};
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

/// The command line names the reference's first ATTRIB `a`, with `$HANDSEED` one below the top:
/// the next handle the drawing, or a copy of it, gives out. The command line does not run the
/// after-command pass.
fn name_at_top(s: &mut Session) {
    s.doc_mut().unwrap().handseed = u64::MAX - 1;
    s.cmdline("(setq a (entnext i))").unwrap();
    assert_eq!(eval(s, "(cdr (assoc 0 (entget a)))"), "\"ATTRIB\"");
}

/// Every object's handle: model space, layouts and block definitions.
fn every_handle(d: &Drawing) -> Vec<Handle> {
    let mut v = d.model.handles();
    for l in &d.layouts {
        v.extend(l.entities.handles());
    }
    for b in d.blocks.values() {
        v.extend(b.entities.handles());
    }
    v
}

/// `a` still names the attribute, and no object of the drawing is `a`: whatever handles were
/// given out, each object keeps its own name.
fn assert_kept(s: &mut Session) {
    assert_eq!(
        eval(s, "(list (cdr (assoc 0 (entget a))) (cdr (assoc 1 (entget a))))"),
        "(\"ATTRIB\" \"A1\")",
        "the old name still means the attribute"
    );
    let handles = every_handle(s.doc().unwrap());
    for h in handles {
        let hex = h.hex();
        assert_eq!(eval(s, &format!("(eq a (handent \"{hex}\"))")), "nil", "object {hex} is not the attribute");
    }
}

/// A line added to `d` with the drawing's next handle.
fn add_line(d: &mut Drawing) -> Handle {
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) })).unwrap()
}

/// After a copy of the drawing took `line`, the name still means the attribute and the line is
/// the line.
fn assert_line_and_name(s: &mut Session, line: Handle) {
    assert_kept(s);
    let hex = line.hex();
    assert_eq!(eval(s, &format!("(cdr (assoc 0 (entget (handent \"{hex}\"))))")), "\"LINE\"");
}

fn one_line(dir: &TempDir) -> String {
    let mut d = Drawing::new_metric();
    add_line(&mut d);
    dir.file("one-line.dxf", cadcraft_io::write_dxf(&d))
}

#[test]
fn another_sessions_read_leaves_a_record_name_alone() {
    // Session A names an attribute and keeps a copy of its drawing; session B, which named
    // nothing, is read; then A's copy gives out a handle and A installs it.
    let mut a = attributed();
    let b = attributed();
    name_at_top(&mut a);
    let mut copy = a.doc().unwrap().clone();
    let _ = b.doc().unwrap();
    let line = add_line(&mut copy);
    *a.doc_mut().unwrap() = copy;
    assert_line_and_name(&mut a, line);
}

#[test]
fn a_copy_edited_on_another_thread_leaves_a_record_name_alone() {
    let mut a = attributed();
    name_at_top(&mut a);
    let copy = a.doc().unwrap().clone();
    let (copy, line) = std::thread::spawn(move || {
        let mut copy = copy;
        let line = add_line(&mut copy);
        (copy, line)
    })
    .join()
    .unwrap();
    *a.doc_mut().unwrap() = copy;
    assert_line_and_name(&mut a, line);
}

#[test]
fn a_copy_made_while_another_document_is_current_leaves_a_record_name_alone() {
    let mut s = attributed();
    name_at_top(&mut s);
    let first = s.active;
    let mut copy = s.doc().unwrap().clone();
    s.open_drawing(Drawing::new_metric(), "other", None);
    let _ = s.doc().unwrap();
    let line = add_line(&mut copy);
    s.active = first;
    *s.doc_mut().unwrap() = copy;
    assert_line_and_name(&mut s, line);
}

#[test]
fn insert_from_file_keeps_an_attrib_name() {
    let mut s = attributed();
    name_at_top(&mut s);
    let dir = TempDir::new("attrib-insert");
    let path = one_line(&dir);
    s.execute("insert", &json!({ "file": path, "name": "SRC", "at": [4.0, 4.0] })).unwrap();
    assert_kept(&mut s);
}

#[test]
fn xattach_keeps_an_attrib_name() {
    let mut s = attributed();
    name_at_top(&mut s);
    let dir = TempDir::new("attrib-xref");
    let path = one_line(&dir);
    s.execute("xattach", &json!({ "path": path, "name": "XR", "at": [8.0, 8.0] })).unwrap();
    assert_kept(&mut s);
}

#[test]
fn audit_fix_keeps_an_attrib_name() {
    let mut s = attributed();
    {
        let d = s.doc_mut().unwrap();
        let line = Entity::new(Handle(0xABC), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 0.0) }));
        d.model.push(line.clone());
        let mut b = Block::new("DUP");
        b.entities.push(line);
        d.blocks.insert("DUP".into(), Arc::new(b));
    }
    name_at_top(&mut s);
    s.execute("audit", &json!({ "fix": true })).unwrap();
    assert_kept(&mut s);
}

#[test]
fn refclose_keeps_an_attrib_name() {
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
    name_at_top(&mut s);
    s.execute("refclose", &json!({ "save": true })).unwrap();
    assert_kept(&mut s);
}
