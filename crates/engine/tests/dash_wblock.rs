//! -WBLOCK at the command line, as AutoCAD 2024 runs it (accoreconsole: these scripts, the
//! drawing read back afterwards). It writes objects from a base point and erases them, whatever
//! its mOde option says; a block by name, `=` (the block named as the file) or `*` (the whole
//! drawing); asks before replacing a file; and writes nothing when nothing is selected.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Drawing, EntityKind, Handle};
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

/// One line per object written: a line's start (where the base point puts it), else its type.
fn write(d: &Drawing, _path: &str) -> Result<Vec<u8>, String> {
    let mut out = String::new();
    for e in d.model.iter() {
        match &e.kind {
            EntityKind::Line(l) => out.push_str(&format!("LINE {},{}\n", l.a.x, l.a.y)),
            k => out.push_str(&format!("{}\n", k.dxf_name())),
        }
    }
    Ok(out.into_bytes())
}

/// A session writing through [`write`], with the drawing the measured scripts start from: line A
/// (0,0)-(10,0), circle B at (5,5) radius 3, line C (0,20)-(10,20).
fn drawing() -> (Session, Handle, Handle, Handle) {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write, plot: None });
    let mut s = Session::new();
    let handle = |v: serde_json::Value| Handle::parse_hex(v["handles"][0].as_str().or(v["handle"].as_str()).unwrap()).unwrap();
    let a = handle(s.execute("line", &json!({ "points": [[0, 0], [10, 0]] })).unwrap());
    let b = handle(s.execute("circle", &json!({ "center": [5, 5], "radius": 3 })).unwrap());
    let c = handle(s.execute("line", &json!({ "points": [[0, 20], [10, 20]] })).unwrap());
    (s, a, b, c)
}

/// A file in a test's own folder, with forward slashes for a LISP string.
fn file(dir: &TempDir, name: &str) -> String {
    dir.path_str(name).replace('\\', "/")
}

fn lisp(s: &mut Session, expr: &str) {
    s.cmdline(expr).unwrap();
}

fn ent(h: Handle) -> String {
    format!("(handent \"{}\")", h.hex())
}

fn alive(s: &Session, h: Handle) -> bool {
    s.doc().unwrap().entity(h).is_some()
}

fn read(path: &str) -> Vec<String> {
    std::fs::read_to_string(path).unwrap().lines().map(str::to_string).collect()
}

#[test]
fn objects_are_written_from_the_base_point_and_erased() {
    // WBLOCK-000: A and B from (5,5); AutoCAD erases both and keeps C.
    let (mut s, a, b, c) = drawing();
    let dir = TempDir::new("dash-wblock");
    let path = file(&dir, "WB00.dwg");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"\" '(5.0 5.0) {} {} \"\")", ent(a), ent(b)));
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(read(&path), ["LINE -5,-5", "CIRCLE"]);
    assert!(!alive(&s, a) && !alive(&s, b) && alive(&s, c));
    assert!(s.log.iter().any(|l| l == "Current settings: Object conversion=Delete"), "{:?}", s.log);
}

#[test]
fn the_prompts_are_autocads() {
    let (mut s, a, _, _) = drawing();
    let dir = TempDir::new("dash-wblock");
    s.cmdline("-wblock").unwrap();
    assert!(s.prompt_text().ends_with("Enter name of output file:"), "{}", s.prompt_text());
    s.cmdline(&file(&dir, "P.dwg")).unwrap();
    assert!(
        s.prompt_text().ends_with("Enter name of existing block or [= (block=output file)/* (whole drawing)] <define new drawing>:"),
        "{}",
        s.prompt_text()
    );
    s.cmdline("").unwrap();
    assert!(s.prompt_text().ends_with("Specify insertion base point or [mOde]:"), "{}", s.prompt_text());
    // WBLOCK-008: anything but a point or mOde there.
    s.cmdline("").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Point or option keyword required."));
    s.cmdline("_O").unwrap();
    assert!(s.prompt_text().ends_with("[Convert to block/Retain/Delete] <Delete>:"), "{}", s.prompt_text());
    s.cmdline("_R").unwrap();
    assert!(s.prompt_text().ends_with("Specify insertion base point or [mOde]:"), "{}", s.prompt_text());
    s.cmdline("0,0").unwrap();
    assert!(s.prompt_text().ends_with("Select objects:"), "{}", s.prompt_text());
    lisp(&mut s, &ent(a));
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
}

#[test]
fn mode_is_asked_but_the_objects_are_erased_whatever_it_says() {
    // WBM-01, WBM-02: Retain and Convert to block are taken, yet A is erased (no block reference
    // takes its place), and the next -WBLOCK still says "Object conversion=Delete".
    for mode in ["_R", "_C"] {
        let (mut s, a, b, c) = drawing();
        let dir = TempDir::new("dash-wblock");
        let first = file(&dir, "M01a.dwg");
        lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{first}\" \"\" \"_O\" \"{mode}\" '(0.0 0.0) {} \"\")", ent(a)));
        assert!(s.running.is_none(), "{mode}: {}", s.prompt_text());
        assert_eq!(read(&first), ["LINE 0,0"]);
        assert!(!alive(&s, a) && alive(&s, b) && alive(&s, c), "{mode}");
        assert_eq!(s.doc().unwrap().model.len(), 2, "{mode}: nothing takes A's place");
        let before = s.log.len();
        lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{}\" \"\" '(0.0 0.0) {} \"\")", file(&dir, "M01b.dwg"), ent(b)));
        assert!(s.log[before..].iter().any(|l| l == "Current settings: Object conversion=Delete"), "{mode}: {:?}", s.log);
        assert!(!alive(&s, b) && alive(&s, c), "{mode}");
    }
}

#[test]
fn nothing_selected_writes_nothing() {
    // WBM-05: the command ends; no file, nothing erased.
    let (mut s, a, b, c) = drawing();
    let dir = TempDir::new("dash-wblock");
    let path = file(&dir, "M05.dwg");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"\" '(0.0 0.0) \"\")"));
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert!(!std::path::Path::new(&path).exists());
    assert!(alive(&s, a) && alive(&s, b) && alive(&s, c));
}

#[test]
fn an_existing_file_is_replaced_only_on_yes() {
    // WBM-06: No (the default) ends the command; WBM-07: Yes goes on to the block prompt.
    let (mut s, a, b, _) = drawing();
    let dir = TempDir::new("dash-wblock");
    let path = file(&dir, "M06.dwg");
    std::fs::write(&path, "old").unwrap();
    s.cmdline("-wblock").unwrap();
    s.cmdline(&path).unwrap();
    assert!(s.prompt_text().ends_with(&format!("{path} already exists, do you want to replace it? [Yes/No] <N>:")), "{}", s.prompt_text());
    // Only Yes replaces it: other text is asked again.
    s.cmdline("yodel").unwrap();
    assert_eq!(s.log.last().map(String::as_str), Some("Invalid option keyword."));
    assert!(s.prompt_text().ends_with("[Yes/No] <N>:"), "{}", s.prompt_text());
    s.cmdline("_N").unwrap();
    assert!(s.running.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    // Enter is No too.
    s.cmdline("-wblock").unwrap();
    s.cmdline(&path).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"_Y\" \"\" '(0.0 0.0) {} \"\")", ent(b)));
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(read(&path), ["CIRCLE"]);
    assert!(alive(&s, a) && !alive(&s, b));
}

#[test]
fn a_block_is_written_by_name_by_equals_or_the_whole_drawing_by_star() {
    let (mut s, a, b, c) = drawing();
    let dir = TempDir::new("dash-wblock");
    s.execute("block", &json!({ "name": "WB01K", "base": [5, 5], "handles": [a.hex(), b.hex()], "keep": "delete" })).unwrap();
    // WBLOCK-001: a block by name, from its base point; nothing is erased.
    let path = file(&dir, "WB01.dwg");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"WB01K\")"));
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(read(&path), ["LINE -5,-5", "CIRCLE"]);
    assert!(alive(&s, c));
    // WBLOCK-002: "=" is the block named as the file.
    let path = file(&dir, "WB01K.dwg");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"=\")"));
    assert_eq!(read(&path), ["LINE -5,-5", "CIRCLE"]);
    // WBLOCK-003: "*" is the whole drawing.
    let path = file(&dir, "WB03.dwg");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"*\")"));
    assert_eq!(read(&path), ["LINE 0,20"]);
    assert!(alive(&s, c));
    // WBLOCK-005: a point given there is a name too.
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{}\" '(0.0 0.0))", file(&dir, "WB05.dwg")));
    assert!(s.log.iter().any(|l| l == "Block \"0,0\" not found."), "{:?}", s.log);
    s.cancel();
    // WBLOCK-008, WBLOCK-015: a name that isn't a block, typed or by "=", is refused and asked again.
    for (answer, name) in [("WB08ZZ", "WB08ZZ"), ("=", "WB15")] {
        lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{}\" \"{answer}\")", file(&dir, "WB15.dwg")));
        assert!(s.log.iter().any(|l| *l == format!("Block \"{name}\" not found.")), "{answer}: {:?}", s.log);
        assert!(s.prompt_text().contains("Enter name of existing block"), "{}", s.prompt_text());
        s.cancel();
    }
    assert!(!std::path::Path::new(&file(&dir, "WB15.dwg")).exists());
}

#[test]
fn a_name_without_extension_is_a_dwg_and_a_dxf_asks_for_its_precision() {
    // WBLOCK-004: no extension.
    let (mut s, a, b, _) = drawing();
    let dir = TempDir::new("dash-wblock");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{}\" \"\" '(0.0 0.0) {} \"\")", file(&dir, "WB04"), ent(a)));
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(read(&file(&dir, "WB04.dwg")), ["LINE 0,0"]);
    // WBLOCK-010: a DXF file asks for its decimal places first; Enter takes 16.
    let path = file(&dir, "WB10.dxf");
    s.cmdline("-wblock").unwrap();
    s.cmdline(&path).unwrap();
    assert!(s.log.iter().any(|l| l.starts_with("Current DXF settings: Precision= 16 Format= ASCII")), "{:?}", s.log);
    assert!(s.prompt_text().ends_with("Enter decimal places of accuracy (0 to 16) or [Binary/Preview] <16>:"), "{}", s.prompt_text());
    s.cancel();
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{path}\" \"\" \"\" '(0.0 0.0) {} \"\")", ent(b)));
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(read(&path), ["CIRCLE"]);
    assert!(!alive(&s, b));
}

#[test]
fn undo_brings_the_written_objects_back() {
    // WBM-09: U after -WBLOCK.
    let (mut s, a, b, c) = drawing();
    let dir = TempDir::new("dash-wblock");
    lisp(&mut s, &format!("(command \"_.-WBLOCK\" \"{}\" \"\" '(5.0 5.0) {} {} \"\")", file(&dir, "M09.dwg"), ent(a), ent(b)));
    assert!(!alive(&s, a) && !alive(&s, b));
    s.cmdline("u").unwrap();
    assert!(alive(&s, a) && alive(&s, b) && alive(&s, c));
}

#[test]
fn the_json_form_of_dash_wblock_erases_what_it_writes() {
    let (mut s, a, b, _) = drawing();
    let dir = TempDir::new("dash-wblock");
    s.execute("wblock", &json!({ "path": file(&dir, "keep.dwg"), "handles": [a.hex()] })).unwrap();
    assert!(alive(&s, a), "WBLOCK's JSON form keeps them");
    s.execute("-wblock", &json!({ "path": file(&dir, "erase.dwg"), "handles": [b.hex()], "base": [5, 5] })).unwrap();
    assert_eq!(read(&file(&dir, "erase.dwg")), ["CIRCLE"]);
    assert!(!alive(&s, b));
    // Nothing selected writes no file, as at the command line.
    let none = file(&dir, "none.dwg");
    assert!(s.execute("-wblock", &json!({ "path": none, "handles": [] })).is_err());
    assert!(s.execute("-wblock", &json!({ "path": none, "handles": [b.hex()] })).is_err(), "erased already");
    assert!(s.execute("-wblock", &json!({ "path": none })).is_err());
    assert!(!std::path::Path::new(&none).exists());
}
