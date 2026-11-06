//! Non-graphic objects from AutoLISP (#1661, #1659): `handent` and `entget` on table records,
//! layouts and dictionaries, `tblobjname`, `namedobjdict`, `dictsearch` and `dictnext`. Their
//! handles are the ones a save writes, the same as in the saved file while the drawing is unchanged.

#![allow(clippy::unwrap_used)]

use super::value::{Value, to_text};
use crate::Session;
use crate::cmd::file::{EntityCodec, set_entity_codec};

fn session() -> Session {
    set_entity_codec(EntityCodec {
        groups: cadcraft_io::entity_groups,
        entity: cadcraft_io::entity_from_groups,
        objects: cadcraft_io::object_records,
    });
    Session::new()
}

fn eval(s: &mut Session, code: &str) -> Result<String, String> {
    match s.lisp_eval(code, false, true, Vec::<Value>::new()) {
        Ok(Some(Ok(v))) => Ok(to_text(&v, true)),
        Ok(Some(Err(e))) => Err(e),
        Ok(None) => Err("did not finish".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn ok(s: &mut Session, code: &str) -> String {
    match eval(s, code) {
        Ok(v) => v,
        Err(e) => panic!("{code}: {e}"),
    }
}

/// The handle of the first `kind` record in a DXF whose group `code` is `name`.
fn file_handle(dxf: &str, kind: &str, code: &str, name: &str) -> String {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let (mut cur, mut handle) = (String::new(), String::new());
    for pair in lines.chunks(2) {
        let [c, v] = pair else { continue };
        match *c {
            "0" => (cur, handle) = (v.to_string(), String::new()),
            "5" | "105" if cur == kind => handle = v.to_string(),
            c2 if cur == kind && c2 == code && *v == name => return handle,
            _ => {}
        }
    }
    panic!("no {kind} {name} in the file")
}

/// A saved and reopened drawing (the issue's case), with the handles its file gives layer "0"
/// and layout "Layout1".
fn reopened() -> (Session, String, String) {
    let mut s = session();
    s.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let dxf = String::from_utf8(cadcraft_io::write(s.doc().unwrap(), "h.dxf").unwrap()).unwrap();
    let (layer, layout) = (file_handle(&dxf, "LAYER", "2", "0"), file_handle(&dxf, "LAYOUT", "1", "Layout1"));
    let mut s = session();
    s.open_drawing(cadcraft_io::read(dxf.as_bytes(), "h.dxf").unwrap(), "h", None);
    (s, layer, layout)
}

#[test]
fn handent_reaches_table_records_and_layouts() {
    let (mut s, layer, layout) = reopened();
    let lower = |h: &str| h.to_ascii_lowercase();
    assert_eq!(ok(&mut s, &format!("(handent \"{layer}\")")), format!("<Entity name: {}>", lower(&layer)));
    assert_eq!(ok(&mut s, &format!("(handent \"{layout}\")")), format!("<Entity name: {}>", lower(&layout)));
    // What entget lists is the record the file holds under that handle.
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget (handent \"{layer}\"))))")), "\"LAYER\"");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 2 (entget (handent \"{layer}\"))))")), "\"0\"");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 5 (entget (handent \"{layer}\"))))")), format!("\"{layer}\""));
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget (handent \"{layout}\"))))")), "\"LAYOUT\"");
    // Graphic entities as before; an unknown handle is nil.
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (handent \"100\"))))"), "\"LINE\"");
    assert_eq!(ok(&mut s, "(handent \"FFFFFF\")"), "nil");
}

/// The ENDBLK record that closes a block definition has a handle in the file too.
#[test]
fn handent_reaches_the_endblk_of_a_block() {
    let mut s = session();
    let dxf = String::from_utf8(cadcraft_io::write(s.doc().unwrap(), "h.dxf").unwrap()).unwrap();
    let end = file_handle(&dxf, "ENDBLK", "100", "AcDbBlockEnd");
    let e = format!("(handent \"{end}\")");
    assert_eq!(ok(&mut s, &e), format!("<Entity name: {}>", end.to_ascii_lowercase()));
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 0 (entget {e})))")), "\"ENDBLK\"");
    assert_eq!(ok(&mut s, &format!("(cdr (assoc 5 (entget {e})))")), format!("\"{end}\""));
    // It isn't a table record.
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (tblobjname \"BLOCK\" \"*Model_Space\"))))"), "\"BLOCK\"");
}

/// Every record of every table: its name, its list's handle and handent agree.
#[test]
fn tblobjname_entget_and_handent_agree_for_every_table() {
    let mut s = session();
    s.execute("layer.new", &serde_json::json!({"name": "WALLS"})).unwrap();
    let code = r#"(not (member nil
      (mapcar '(lambda (tbl / r ok e)
                 (setq ok T r (tblnext tbl T))
                 (while r
                   (setq e (tblobjname tbl (cdr (assoc 2 r))))
                   (setq ok (and ok e
                                 (equal (cdr (assoc -1 (entget e))) e)
                                 (equal (handent (cdr (assoc (if (= tbl "DIMSTYLE") 105 5) (entget e)))) e)
                                 (= (strcase (cdr (assoc 2 (entget e)))) (strcase (cdr (assoc 2 r))))))
                   (setq r (tblnext tbl)))
                 ok)
              '("LAYER" "LTYPE" "STYLE" "DIMSTYLE" "BLOCK" "APPID"))))"#;
    assert_eq!(ok(&mut s, code), "T");
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget (tblobjname \"layer\" \"walls\"))))"), "\"WALLS\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 0 (entget (tblobjname \"BLOCK\" \"*Model_Space\"))))"), "\"BLOCK\"");
    assert_eq!(ok(&mut s, "(tblobjname \"LAYER\" \"NOPE\")"), "nil");
    assert_eq!(ok(&mut s, "(tblobjname \"NOTATABLE\" \"0\")"), "nil");
}

#[test]
fn dictionaries_lead_to_the_layouts() {
    let mut s = session();
    let walk = r#"(progn
      (setq nod (namedobjdict) ld (dictsearch nod "acad_layout") names nil)
      (defun lastval (code lst / r) (foreach p lst (if (= (car p) code) (setq r (cdr p)))) r)
      (setq e (dictnext (cdr (assoc -1 ld)) T))
      (while e (setq names (cons (lastval 1 e) names) e (dictnext (cdr (assoc -1 ld)))))
      (list (cdr (assoc 0 (entget nod))) (cdr (assoc 0 ld)) (reverse names)))"#;
    assert_eq!(ok(&mut s, walk), "(\"DICTIONARY\" \"DICTIONARY\" (\"Model\" \"Layout1\" \"Layout2\"))");
    // dictnext after the last entry is nil; rewind starts again.
    assert_eq!(ok(&mut s, "(dictnext (cdr (assoc -1 ld)))"), "nil");
    assert_eq!(ok(&mut s, "(not (null (dictnext (cdr (assoc -1 ld)) T)))"), "T");
    // setnext: dictnext continues after the entry dictsearch found.
    let d = "(cdr (assoc -1 (dictsearch (namedobjdict) \"ACAD_LAYOUT\")))";
    let after = format!("(progn (setq d {d}) (dictsearch d \"Layout1\" T) (lastval 1 (dictnext d)))");
    assert_eq!(ok(&mut s, &after), "\"Layout2\"");
    assert_eq!(ok(&mut s, "(dictsearch (namedobjdict) \"NO_SUCH_KEY\")"), "nil");
    // A name that isn't a dictionary has no entries.
    assert_eq!(ok(&mut s, "(dictnext (tblobjname \"LAYER\" \"0\"))"), "nil");
}

#[test]
fn the_records_follow_changes_to_the_drawing() {
    let mut s = session();
    assert_eq!(ok(&mut s, "(tblobjname \"LAYER\" \"LATER\")"), "nil");
    s.execute("layer.new", &serde_json::json!({"name": "LATER"})).unwrap();
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (entget (tblobjname \"LAYER\" \"LATER\"))))"), "\"LATER\"");
    // And within one run, after a command added a layer.
    assert_eq!(
        ok(
            &mut s,
            "(progn (tblobjname \"LAYER\" \"0\") (command \"_.-LAYER\" \"_N\" \"INRUN\" \"\") (cdr (assoc 2 (entget (tblobjname \"LAYER\" \"INRUN\")))))"
        ),
        "\"INRUN\""
    );
}

#[test]
fn bad_arguments_are_errors_not_crashes() {
    let mut s = session();
    for code in ["(tblobjname \"LAYER\")", "(tblobjname 1 2)", "(namedobjdict 1)", "(dictsearch 1 \"A\")", "(dictnext \"x\")", "(dictsearch)"] {
        assert!(eval(&mut s, code).is_err(), "{code}");
    }
    // Enames of non-graphic objects are refused by the entity editors, not crashed on.
    assert_eq!(ok(&mut s, "(entdel (tblobjname \"LAYER\" \"0\"))"), "nil");
    assert_eq!(ok(&mut s, "(entmod (entget (tblobjname \"LAYER\" \"0\")))"), "nil");
}

/// A dictionary with a default entry (ACDBDICTIONARYWDFLT, the plot style names) is a dictionary
/// too: dictsearch and dictnext read its entries.
#[test]
fn a_dictionary_with_a_default_has_entries() {
    let mut s = session();
    let walk = r#"(progn
      (setq d (dictsearch (namedobjdict) "ACAD_PLOTSTYLENAME") e (cdr (assoc -1 d)) names nil)
      (setq x (dictnext e T))
      (while x (setq names (cons (cdr (assoc 0 x)) names) x (dictnext e)))
      (list (cdr (assoc 0 d)) (> (length names) 0) (not (null (dictsearch e "Normal")))))"#;
    assert_eq!(ok(&mut s, walk), "(\"ACDBDICTIONARYWDFLT\" T T)");
}

/// Each drawing has its own LISP environment, so `dictnext` and `tblnext` keep a place per
/// drawing: a second drawing starts at its own first entry.
#[test]
fn each_drawing_keeps_its_own_places() {
    let mut s = session();
    let layouts = "(setq d (cdr (assoc -1 (dictsearch (namedobjdict) \"ACAD_LAYOUT\"))))";
    let name = "(defun lastval (code lst / r) (foreach p lst (if (= (car p) code) (setq r (cdr p)))) r)";
    ok(&mut s, name);
    ok(&mut s, layouts);
    assert_eq!(ok(&mut s, "(lastval 1 (dictnext d T))"), "\"Model\"");
    assert_eq!(ok(&mut s, "(lastval 1 (dictnext d))"), "\"Layout1\"");
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (tblnext \"LAYER\" T)))"), "\"0\"");
    s.open_drawing(cadcraft_doc::Drawing::new_imperial(), "second", None);
    ok(&mut s, name);
    ok(&mut s, layouts);
    assert_eq!(ok(&mut s, "(lastval 1 (dictnext d))"), "\"Model\"", "not the first drawing's place");
    assert_eq!(ok(&mut s, "(cdr (assoc 2 (tblnext \"LAYER\")))"), "\"0\"");
}

/// A routine that walks a table with `tblnext` and changes the drawing on each step (which
/// rebuilds the records `tblobjname` reads) visits every record once and ends.
#[test]
fn a_tblnext_loop_that_edits_the_drawing_ends() {
    let mut s = session();
    for n in ["A", "B", "C"] {
        s.execute("layer.new", &serde_json::json!({"name": n})).unwrap();
    }
    // The layers, walked without changing anything.
    let plain = ok(
        &mut s,
        r#"(progn (setq all nil r (tblnext "LAYER" T)) (while r (setq all (cons (cdr (assoc 2 r)) all) r (tblnext "LAYER"))) (reverse all))"#,
    );
    assert!(plain.contains("\"A\"") && plain.contains("\"C\""), "{plain}");
    let code = r#"(progn
      (setq names nil steps 0 r (tblnext "LAYER" T))
      (while (and r (< steps 50))
        (setq names (cons (cdr (assoc 2 r)) names) steps (1+ steps))
        (tblobjname "LAYER" (cdr (assoc 2 r)))
        (entmake '((0 . "LINE") (10 0.0 0.0 0.0) (11 1.0 0.0 0.0)))
        (setq r (tblnext "LAYER")))
      (reverse names))"#;
    assert_eq!(ok(&mut s, code), plain, "each layer once, and the loop ends");
}
