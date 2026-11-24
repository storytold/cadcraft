//! An INSERT's ATTRIBs and SEQEND reached from AutoLISP through the CLI (`entnext`, `entget`,
//! `entmod`, `entdel`): the walk, a value edit (one Undo step), names that stop holding (erase,
//! ATTSYNC, Undo, a reinsert that reuses the parent handle), edits that are refused, and the
//! name bounds.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

/// A scratch directory, removed on drop also when an assertion fails.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("cc-cli-attribs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create scratch dir");
        Dir(p)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One `cadcraft-cli run`: a fresh profile, an optional drawing to open, and the steps to run.
struct Run {
    args: Vec<String>,
}

impl Run {
    fn new(dir: &Dir, file: Option<&Path>) -> Run {
        let mut args: Vec<String> = vec!["run".into(), "--config-dir".into(), dir.path("profile").to_string_lossy().into_owned()];
        match file {
            Some(f) => args.push(f.to_string_lossy().into_owned()),
            None => args.push("--metric".into()),
        }
        Run { args }
    }

    fn cmd(&mut self, id: &str, params: Value) -> &mut Run {
        self.args.push("--cmd".into());
        self.args.push(format!("{id} {params}"));
        self
    }

    fn lisp(&mut self, code: &str) -> &mut Run {
        self.cmd("lisp", json!({ "code": code }))
    }

    fn save(&mut self, path: &Path) -> &mut Run {
        self.args.push("--save".into());
        self.args.push(path.to_string_lossy().into_owned());
        self
    }

    fn output(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(&self.args).output().expect("run cadcraft-cli")
    }

    /// Run it: the value each `lisp` step returned, in order (commands' own results are left out).
    fn out(&self) -> Vec<Value> {
        let out = self.output();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{}: {}", self.args.join(" "), err);
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap_or_else(|e| panic!("not a result: {l}: {e}")))
            .filter(|v| v.get("value").is_some())
            .collect()
    }
}

/// The value a `lisp` step returned, as AutoLISP's `prin1` shows it.
fn value(results: &[Value], i: usize) -> String {
    match results.get(i).and_then(|r| r.get("value")) {
        Some(Value::String(s)) => s.clone(),
        other => panic!("step {i}: no value: {other:?} in {results:?}"),
    }
}

/// `value` without the quotes `prin1` puts round a string.
fn text(results: &[Value], i: usize) -> String {
    value(results, i).trim_matches('"').to_string()
}

/// TAGBLK.dxf: block TAGBLK with a circle and four attribute definitions (TAG1 D1, TAG2 D2
/// verify, TAG3 P3 preset, TAG4 C4 constant). Opening the file again gives the drawing fresh
/// names.
const FIXTURE: &str = r#"(progn
  (entmake '((0 . "BLOCK") (2 . "TAGBLK") (70 . 2) (10 0.0 0.0 0.0)))
  (entmake '((0 . "CIRCLE") (10 -5.0 0.0 0.0) (40 . 2.0)))
  (entmake '((0 . "ATTDEF") (10 0.0 0.0 0.0) (40 . 2.5) (1 . "D1") (3 . "P1") (2 . "TAG1") (70 . 0)))
  (entmake '((0 . "ATTDEF") (10 0.0 5.0 0.0) (40 . 2.5) (1 . "D2") (3 . "P2") (2 . "TAG2") (70 . 4)))
  (entmake '((0 . "ATTDEF") (10 0.0 10.0 0.0) (40 . 2.5) (1 . "P3") (3 . "P3") (2 . "TAG3") (70 . 8)))
  (entmake '((0 . "ATTDEF") (10 0.0 15.0 0.0) (40 . 2.5) (1 . "C4") (3 . "P4") (2 . "TAG4") (70 . 2)))
  (entmake '((0 . "ENDBLK"))))"#;

/// The first reference: INSERT TAGBLK at (10,10) with TAG1 "A1", TAG2 verified as "B1" and
/// TAG3 preset ("P3"); TAG4 is constant, so it is not asked.
const INSERT: &str = r#"(command "_.-INSERT" "TAGBLK" '(10 10) 1 1 0 "A1" "B1" "")"#;

fn fixture(dir: &Dir) -> PathBuf {
    let file = dir.path("TAGBLK.dxf");
    let mut run = Run::new(dir, None);
    let out = run.lisp(FIXTURE).save(&file).out();
    assert_eq!(text(&out, 0), "TAGBLK");
    file
}

/// The walk from the entity named `i`: its records' tags and values, then the type of the record
/// that ends the run.
const WALK: &str = r#"(progn
  (setq e (entnext i) n 0 ts nil ed nil)
  (while (and e (= "ATTRIB" (cdr (assoc 0 (setq ed (entget e))))))
    (setq ts (append ts (list (cdr (assoc 2 ed)) (cdr (assoc 1 ed)))) n (1+ n) e (entnext e)))
  (list n ts (cdr (assoc 0 ed))))"#;

#[test]
fn the_walk_reaches_each_attrib_and_ends_at_the_seqend() {
    let dir = Dir::new("walk");
    let file = fixture(&dir);
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(
            r#"(progn
  {INSERT}
  (setq i (entlast))
  (setvar "ATTREQ" 0)
  (command "_.-INSERT" "TAGBLK" '(60 10) 1 1 0)
  (setvar "ATTREQ" 1)
  (setq second (entlast))
  "inserted")"#
        ))
        // Three attributes, then the SEQEND owned by the reference, then the next main entity.
        .lisp(&format!(
            r#"(progn {WALK}
  (list n ts (cdr (assoc 0 ed)) (equal (cdr (assoc -2 ed)) i) (equal (entnext e) second) (cdr (assoc 0 (entget (entnext e))))))"#
        ))
        // The second reference (no attribute answers, ATTREQ 0) took the definitions' values; its
        // own run ends at its SEQEND, with nothing drawn after it.
        .lisp(&format!(
            r#"(progn (setq i second) {WALK}
  (list n ts (cdr (assoc 0 ed)) (equal (cdr (assoc -2 ed)) i) (entnext e)))"#
        ))
        .out();
    assert_eq!(text(&out, 0), "inserted");
    assert_eq!(value(&out, 1), "(3 (\"TAG1\" \"A1\" \"TAG2\" \"B1\" \"TAG3\" \"P3\") \"SEQEND\" T T \"INSERT\")");
    // Its record ends with its own SEQEND, and nothing is drawn after the reference.
    assert_eq!(value(&out, 2), "(3 (\"TAG1\" \"D1\" \"TAG2\" \"D2\" \"TAG3\" \"P3\") \"SEQEND\" T nil)");
}

#[test]
fn a_value_edit_changes_group_one_only_and_is_one_undo_step() {
    let dir = Dir::new("value");
    let file = fixture(&dir);
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(r#"(progn {INSERT} (setq i (entlast) a (entnext i)) (list (cdr (assoc 2 (entget a))) (cdr (assoc 1 (entget a)))))"#))
        .lisp(
            r#"(progn
  (setq d (entget a) r (entmod (subst (cons 1 "A2") (assoc 1 d) d)))
  (list (cdr (assoc 0 r)) (cdr (assoc 1 r)) (cdr (assoc 1 (entget a))) (cdr (assoc 2 (entget a)))
        (cdr (assoc 40 (entget a))) (cdr (assoc 8 (entget a))) (equal (cdr (assoc -2 (entget a))) i)))"#,
        )
        .cmd("undo", json!({ "count": 1 }))
        // The edit is one Undo step: the value is back, and the name given before it holds no
        // longer (the drawing it was given in was restored from another state).
        .lisp(
            r#"(list (entget a) (progn (setq e (entnext (entlast)))
                     (list (cdr (assoc 2 (entget e))) (cdr (assoc 1 (entget e))))))"#,
        )
        .out();
    assert_eq!(value(&out, 0), "(\"TAG1\" \"A1\")");
    assert_eq!(value(&out, 1), "(\"ATTRIB\" \"A2\" \"A2\" \"TAG1\" 2.5 \"0\" T)");
    assert_eq!(value(&out, 2), "(nil (\"TAG1\" \"A1\"))");
    // Redo brings the edit back; the name given before the Undo still names nothing.
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(r#"(progn {INSERT} (setq i (entlast) a (entnext i) d (entget a)) (entmod (subst (cons 1 "A2") (assoc 1 d) d)))"#))
        .cmd("undo", json!({ "count": 1 }))
        .cmd("redo", json!({ "count": 1 }))
        .lisp("(list (entget a) (cdr (assoc 1 (entget (entnext (entlast))))))")
        .out();
    assert_eq!(value(&out, 1), "(nil \"A2\")");
}

#[test]
fn a_name_stops_holding_after_erase_attsync_and_undo() {
    let dir = Dir::new("stale");
    let file = fixture(&dir);
    let mut run = Run::new(&dir, Some(&file));
    let out = run.lisp(&format!(r#"(progn {INSERT} (setq i (entlast)) (cdr (assoc 5 (entget i))))"#)).out();
    let handle = text(&out, 0);

    // Erased: the reference's records are gone with it, and the name names nothing.
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(r#"(progn {INSERT} (setq i (entlast) a (entnext i)) (cdr (assoc 2 (entget a))))"#))
        .cmd("erase", json!({ "handles": [handle] }))
        .lisp("(list (entget a) (entnext a))")
        // Undone: the reference is back, but a name given before the Undo does not come back with
        // it: a walk gives its records new ones.
        .cmd("undo", json!({ "count": 1 }))
        .lisp("(list (entget a) (cdr (assoc 1 (entget (entnext (entlast))))))")
        // ATTSYNC rebuilds the reference's attributes: the name given before names nothing, and
        // the rebuilt attribute keeps the value.
        .lisp("(progn (setq b (entnext (entlast))) (cdr (assoc 2 (entget b))))")
        .cmd("attsync", json!({ "name": "TAGBLK" }))
        .lisp("(list (entget b) (cdr (assoc 1 (entget (entnext (entlast))))))")
        .out();
    assert_eq!(value(&out, 0), "\"TAG1\"");
    assert_eq!(value(&out, 1), "(nil nil)");
    assert_eq!(value(&out, 2), "(nil \"A1\")");
    assert_eq!(value(&out, 3), "\"TAG1\"");
    assert_eq!(value(&out, 4), "(nil \"A1\")");

    // A reinsert after an Undo takes the parent handle back: the old name must not name its
    // attribute.
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(r#"(progn {INSERT} (setq i (entlast) a (entnext i)) (list (cdr (assoc 5 (entget i))) (cdr (assoc 2 (entget a)))))"#))
        .cmd("undo", json!({ "count": 1 }))
        .lisp(&format!(r#"(progn {INSERT} (list (cdr (assoc 5 (entget (entlast)))) (entget a)))"#))
        .lisp("(list (cdr (assoc 2 (entget (entnext (entlast))))) (cdr (assoc 1 (entget (entnext (entlast))))))")
        .out();
    assert_eq!(text(&out, 0), format!("(\"{handle}\" \"TAG1\")"));
    // The same handle, and the old name names nothing in the new reference.
    assert_eq!(value(&out, 1), format!("(\"{handle}\" nil)"));
    assert_eq!(value(&out, 2), "(\"TAG1\" \"A1\")");
}

#[test]
fn an_edit_of_anything_but_the_value_is_refused() {
    let dir = Dir::new("refused");
    let file = fixture(&dir);
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(
            r#"(progn {INSERT} (setq i (entlast) a (entnext i) s (entnext (entnext (entnext a))) p0 (cdr (assoc 10 (entget a)))) "named")"#
        ))
        .lisp(
            r#"(progn
  (setq d (entget a))
  (list
    (entmod (subst (cons 2 "TAGX") (assoc 2 d) d))
    (entmod (subst (cons 40 9.0) (assoc 40 d) d))
    (entmod (subst (cons 0 "SEQEND") (assoc 0 d) d))
    (entmod (entget s))
    (entmod (subst (cons 410 "Layout2") (assoc 410 d) d))
    (entmod (subst (cons 1 "A2") (assoc 1 d) (subst (cons 410 "Layout2") (assoc 410 d) d)))
    (entdel a) (entdel s)))"#,
        )
        // Nothing changed: the same tag, value, height and place, and the name still holds.
        .lisp(
            r#"(progn (setq e (entnext (entlast)) d (entget e))
  (list (cdr (assoc 2 d)) (cdr (assoc 1 d)) (cdr (assoc 40 d)) (equal p0 (cdr (assoc 10 d))) (equal (entget a) d)))"#,
        )
        .out();
    assert_eq!(text(&out, 0), "named");
    assert_eq!(value(&out, 1), "(nil nil nil nil nil nil nil nil)");
    assert_eq!(value(&out, 2), "(\"TAG1\" \"A1\" 2.5 T T)");
}

/// A minimal drawing: `before` (records written as they are), then an INSERT with `attribs`
/// ATTRIBs (each tag T, value V) and its SEQEND, last in the file. A header `$HANDSEED` is
/// written when `handseed` is given (a hostile file can name any next handle).
fn insert_dxf_seed(attribs: usize, before: &str, handseed: Option<&str>) -> String {
    let mut s = String::with_capacity(before.len().saturating_add(attribs.saturating_mul(96)));
    if let Some(seed) = handseed {
        s.push_str("0\nSECTION\n2\nHEADER\n9\n$HANDSEED\n5\n");
        s.push_str(seed);
        s.push('\n');
        s.push_str("0\nENDSEC\n");
    }
    s.push_str("0\nSECTION\n2\nENTITIES\n");
    s.push_str(before);
    s.push_str("0\nINSERT\n5\n20\n8\n0\n66\n1\n2\nTAGBLK\n10\n0\n20\n0\n30\n0\n");
    for _ in 0..attribs {
        s.push_str("0\nATTRIB\n5\n21\n8\n0\n10\n0\n20\n0\n30\n0\n40\n1\n1\nV\n2\nT\n70\n0\n");
    }
    s.push_str("0\nSEQEND\n5\n22\n0\nENDSEC\n0\nEOF\n");
    s
}

fn insert_dxf(attribs: usize, before: &str) -> String {
    insert_dxf_seed(attribs, before, None)
}

/// The number of records a walk from `i` reaches, counting each `entnext`.
const COUNT: &str = r#"(progn (setq e (entnext i) n 0) (while e (setq n (1+ n) e (entnext e))) n)"#;

#[test]
fn a_walk_is_stable_and_a_hostile_handle_takes_no_attribute_name() {
    let dir = Dir::new("stable");
    // A file's handle at the first one a name is taken from (one below the top of the range):
    // the ATTRIB must skip it for another name, and the line must keep its own.
    let before = "0\nLINE\n5\nFFFFFFFFFFFFFFFE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n5\n21\n5\n31\n0\n";
    let file = dir.path("hostile.dxf");
    std::fs::write(&file, insert_dxf(2, before)).expect("write the DXF");

    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(
            r#"(progn
  (setq i (handent "20") line (handent "FFFFFFFFFFFFFFFE") a (entnext i) b (entnext i))
  (list (cdr (assoc 5 (entget line))) (equal (handent "FFFFFFFFFFFFFFFE") line)
        (not (equal a line)) (cdr (assoc 0 (entget a))) (equal a b) (equal (entnext a) (entnext b))))"#,
        )
        .lisp(&format!(r#"(progn (setq i (handent "20")) {COUNT})"#))
        .out();
    assert_eq!(value(&out, 0), "(\"FFFFFFFFFFFFFFFE\" T T \"ATTRIB\" T T)");
    // Two ATTRIBs and the SEQEND, then the line (it sorts last by its handle), then the end.
    assert_eq!(value(&out, 1), "4");
}

#[test]
fn a_restored_reference_does_not_revive_an_old_name() {
    let dir = Dir::new("restored");
    let file = fixture(&dir);
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(
            r#"(progn {INSERT} (setq i (entlast) a (entnext i))
  (entdel i)
  (setq gone (entget a))
  (entdel i)
  (list gone (entget a) (cdr (assoc 1 (entget (entnext i))))))"#
        ))
        .out();
    assert_eq!(value(&out, 0), "(nil nil \"A1\")", "the reference comes back, the old name does not");

    // ERASE, then OOPS: the same. The old name never comes back with the reference.
    let mut run = Run::new(&dir, Some(&file));
    let handle = text(&run.lisp(&format!(r#"(progn {INSERT} (cdr (assoc 5 (entget (entlast)))))"#)).out(), 0);
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(&format!(r#"(progn {INSERT} (setq a (entnext (entlast))) "named")"#))
        .cmd("erase", json!({ "handles": [handle] }))
        .cmd("oops", json!({}))
        .lisp("(list (entget a) (cdr (assoc 1 (entget (entnext (entlast))))))")
        .out();
    assert_eq!(value(&out, 1), "(nil \"A1\")", "OOPS restores the reference, not the old name");
}

#[test]
fn a_new_line_never_takes_an_issued_name() {
    let dir = Dir::new("handseed");
    let file = dir.path("hostile.dxf");
    // A file's $HANDSEED in the reserved band is the file's own handle and is not followed (#913),
    // so the line made after the walk gets the drawing's seed: never the name given below the top
    // of the range. The engine's `a_new_entity_never_takes_an_issued_name` sets the seed at the top
    // and checks the same rule where the seed can reach the names.
    std::fs::write(&file, insert_dxf_seed(1, "", Some("FFFFFFFFFFFFFFFF"))).expect("write the DXF");
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(r#"(progn (setq i (handent "20") a (entnext i)) (list (cdr (assoc 0 (entget a))) (cdr (assoc 5 (entget (entlast))))))"#)
        .lisp(r#"(progn (entmake '((0 . "LINE") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0))) (cdr (assoc 5 (entget (entlast)))))"#)
        .lisp(r#"(list (cdr (assoc 0 (entget a))) (entdel a))"#)
        .out();
    assert_eq!(value(&out, 0), "(\"ATTRIB\" \"20\")");
    assert_eq!(value(&out, 1), "\"100\"");
    assert_eq!(value(&out, 2), "(\"ATTRIB\" nil)");
}

#[test]
fn a_real_entity_in_the_name_range_is_not_filtered() {
    let dir = Dir::new("walkrange");
    let file = dir.path("hostile.dxf");
    let before = "0\nLINE\n5\nFFFFFFFFFFFFFFFE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n5\n21\n5\n31\n0\n\
                  0\nLINE\n5\nFFFFFFFFFFFFFFFF\n8\n0\n10\n0\n20\n0\n30\n0\n11\n6\n21\n6\n31\n0\n";
    std::fs::write(&file, insert_dxf_seed(1, before, Some("FFFFFFFFFFFFFFFF"))).expect("write the DXF");
    let mut run = Run::new(&dir, Some(&file));
    let out = run
        .lisp(r#"(progn (setq i (handent "20")) (entnext i))"#)
        .lisp(r#"(equal (entnext (handent "FFFFFFFFFFFFFFFE")) (handent "FFFFFFFFFFFFFFFF"))"#)
        .lisp(r#"(entnext (handent "FFFFFFFFFFFFFFFF"))"#)
        .out();
    assert_eq!(value(&out, 1), "T");
    assert_eq!(value(&out, 2), "nil");
}

#[test]
fn the_session_names_at_most_65536_records() {
    let dir = Dir::new("names");
    let file = dir.path("many.dxf");
    // One more attribute than the limit of names: the walk stops at the limit, and nothing in the
    // drawing changed.
    std::fs::write(&file, insert_dxf(65_537, "")).expect("write the DXF");
    let mut run = Run::new(&dir, Some(&file));
    let out = run.lisp(&format!("(progn (setq i (entlast)) {COUNT})")).lisp(&format!("(progn (setq i (entlast)) {COUNT})")).out();
    assert_eq!(value(&out, 0), "65536");
    // The names given by the first walk hold: the second walk is the same.
    assert_eq!(value(&out, 1), "65536");
}
