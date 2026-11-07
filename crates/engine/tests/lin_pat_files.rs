//! Linetypes and hatch patterns from the user's own `.lin` / `.pat` files (#552): LINETYPE's JSON
//! form, -LINETYPE at the command line, HATCH with a pattern file, HATCH.PATTERNS. The files are
//! tiny synthetic ones written here, with our own names.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Hatch};
use serde_json::json;

#[path = "support/temp_dir.rs"]
mod temp_dir;

use temp_dir::TempDir;

const LIN: &str = "*MYDASH,My dash __ __\nA,.5,-.25\n*MYTEXT,My text --MY--\nA,.5,-.2,[\"MY\",Standard,S=.1,X=-.1,Y=-.05],-.2\n";

fn linetype_pattern(s: &Session, name: &str) -> Option<Vec<f64>> {
    s.doc().unwrap().linetype(name).map(|l| l.pattern.iter().map(|e| e.length).collect())
}

#[test]
fn linetype_json_loads_named_linetypes_from_file_text() {
    let mut s = Session::new();
    let r = s.execute("linetype", &json!({ "load": "MYDASH", "text": LIN, "current": "MYDASH" })).unwrap();
    assert_eq!(r["added"], json!(["MYDASH"]));
    assert_eq!(linetype_pattern(&s, "MYDASH"), Some(vec![0.5, -0.25]));
    assert!(linetype_pattern(&s, "MYTEXT").is_none(), "only the named linetype is loaded");
    assert_eq!(s.doc().unwrap().header.str("CELTYPE", ""), "MYDASH");

    // All of them by default; one already loaded is kept unless asked to reload.
    let r = s.execute("linetype", &json!({ "text": LIN.replace(".5,-.25", "1,-1") })).unwrap();
    assert_eq!((r["added"].clone(), r["alreadyLoaded"].clone()), (json!(["MYTEXT"]), json!(["MYDASH"])));
    assert_eq!(linetype_pattern(&s, "MYDASH"), Some(vec![0.5, -0.25]));
    let text = &s.doc().unwrap().linetype("MYTEXT").unwrap().pattern[1];
    assert_eq!(text.text.as_deref(), Some("MY"));
    let r = s.execute("linetype", &json!({ "load": "my*", "text": LIN.replace(".5,-.25", "1,-1"), "reload": true })).unwrap();
    assert_eq!(r["reloaded"], json!(["MYDASH", "MYTEXT"]));
    assert_eq!(linetype_pattern(&s, "MYDASH"), Some(vec![1.0, -1.0]));
    // Undo takes a file load back like any change.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(linetype_pattern(&s, "MYDASH"), Some(vec![0.5, -0.25]));
}

#[test]
fn linetype_json_reads_a_lin_file_by_path() {
    let dir = TempDir::new("lin-pat");
    let path = dir.file("mine.lin", LIN);
    let mut s = Session::new();
    s.execute("linetype", &json!({ "load": "MYTEXT", "file": path })).unwrap();
    assert!(linetype_pattern(&s, "MYTEXT").is_some());
    let err = s.execute("linetype", &json!({ "load": "NOPE", "file": path })).unwrap_err().to_string();
    assert!(err.contains("NOPE"), "{err}");
    let err = s.execute("linetype", &json!({ "file": "/no/such/dir/x.lin" })).unwrap_err().to_string();
    assert!(err.contains("x.lin"), "{err}");
}

#[test]
fn dash_linetype_loads_from_a_file_and_the_library_at_the_command_line() {
    let dir = TempDir::new("lin-pat");
    let path = dir.file("cmdline.lin", LIN);
    let mut s = Session::new();
    s.cmdline("-linetype").unwrap();
    for line in ["l", "MYDASH,MYTEXT", path.as_str(), "s", "MYTEXT"] {
        s.cmdline(line).unwrap();
    }
    assert!(s.running.is_some(), "back at the option prompt");
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert!(linetype_pattern(&s, "MYDASH").is_some() && linetype_pattern(&s, "MYTEXT").is_some());
    assert_eq!(s.doc().unwrap().header.str("CELTYPE", ""), "MYTEXT");
    assert!(s.log.iter().any(|l| l.contains("Linetype MYDASH loaded")), "{:?}", s.log);

    // Already loaded: asked whether to reload, in the file's order.
    let changed = dir.file("changed.lin", LIN.replace(".5,-.25", "2,-2"));
    s.cmdline("-linetype").unwrap();
    for line in ["l", "*", changed.as_str()] {
        s.cmdline(line).unwrap();
    }
    assert!(s.current_prompt().unwrap().message.contains("MYDASH is already loaded"), "{:?}", s.current_prompt());
    s.cmdline("y").unwrap();
    assert!(s.current_prompt().unwrap().message.contains("MYTEXT is already loaded"));
    s.cmdline("n").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(linetype_pattern(&s, "MYDASH"), Some(vec![2.0, -2.0]));

    // Enter at the file prompt: the built-in library.
    s.cmdline("-linetype").unwrap();
    for line in ["l", "DASHED", "", ""] {
        s.cmdline(line).unwrap();
    }
    assert!(s.running.is_none());
    assert!(linetype_pattern(&s, "DASHED").is_some());
}

const PAT: &str = "*MYGRID,My grid\n0,0,0,0,.5\n90,0,0,0,.5\n*MYSTEPS,My steps\n0,0,0,.25,.25,.25,-.25\n";

fn hatches(s: &Session) -> Vec<Hatch> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Hatch(h) => Some(h.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn hatch_uses_a_pattern_from_a_pat_file_and_keeps_its_definition() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    s.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "MYGRID", "patternText": PAT })).unwrap();
    let h = hatches(&s).pop().unwrap();
    assert_eq!(h.pattern, "MYGRID");
    assert_eq!(h.pattern_lines.len(), 2);
    assert_eq!(h.pattern_lines[1].angle, 90.0);

    let list = s.execute("hatch.patterns", &json!({})).unwrap();
    let user: Vec<&str> = list["user"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(user.contains(&"MYGRID") && user.contains(&"MYSTEPS"), "{list}");
    assert!(list["standard"].as_array().unwrap().iter().any(|n| n == "ANSI31"));

    // Another pattern from the list replaces the definition the hatch carries; a library one clears it.
    let hx = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).unwrap().handle.hex();
    s.execute("hatchedit", &json!({ "handles": [hx], "pattern": "MYSTEPS" })).unwrap();
    assert_eq!(hatches(&s).pop().unwrap().pattern_lines[0].dashes, [0.25, -0.25]);
    s.execute("hatchedit", &json!({ "handles": [hx], "pattern": "ANSI31" })).unwrap();
    assert!(hatches(&s).pop().unwrap().pattern_lines.is_empty());
}

#[test]
fn hatch_pattern_prompt_takes_a_pat_file_name() {
    let dir = TempDir::new("lin-pat");
    let path = dir.file("MYSTAIRS.pat", "*MYOTHER\n0,0,0,0,1\n*MYSTAIRS,Stairs\n0,0,0,.5,.5,.5,-.5\n90,.5,0,.5,.5,.5,-.5\n");
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    s.cmdline("hatch").unwrap();
    s.cmdline("p").unwrap();
    s.cmdline(&path).unwrap();
    // Its scale and angle follow; Enter keeps them.
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    // The pattern named like the file is the one used.
    s.cmdline("5,5").unwrap();
    s.cmdline("").unwrap();
    let h = hatches(&s).pop().unwrap();
    assert_eq!((h.pattern.as_str(), h.pattern_lines.len()), ("MYSTAIRS", 2));
    assert_eq!(s.doc().unwrap().header.str("HPNAME", ""), "MYSTAIRS");

    // A loaded pattern is now offered by name, too.
    s.cmdline("hatch").unwrap();
    s.cmdline("p").unwrap();
    s.cmdline("myother").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("").unwrap();
    s.cmdline("5,5").unwrap();
    s.cmdline("").unwrap();
    assert_eq!(hatches(&s).iter().filter(|h| h.pattern == "MYOTHER").count(), 1);

    // A file with no pattern in it, or none at all, asks for the pattern again.
    let empty = dir.file("EMPTY.pat", ";; no patterns\n");
    for file in [empty, dir.path_str("MISSING.pat")] {
        s.cmdline("hatch").unwrap();
        s.cmdline("p").unwrap();
        s.cmdline(&file).unwrap();
        assert!(s.prompt_text().contains("Enter a pattern name"), "{file}: {}", s.prompt_text());
        s.cancel();
    }
}

#[test]
fn a_pat_file_cannot_replace_a_library_pattern() {
    let mut s = Session::new();
    let r = s.execute("hatch.patterns", &json!({ "text": "*ANSI31\n0,0,0,0,9\n*MYKEEP\n0,0,0,0,1\n" })).unwrap();
    assert_eq!(r["added"], json!(["MYKEEP"]));
    assert!(r["warnings"].to_string().contains("ANSI31"));
}

#[test]
fn hostile_support_files_never_panic() {
    let mut s = Session::new();
    s.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
    let huge = "x".repeat(9 << 20);
    let garbage = "*\u{0}[\"]]\n,,,,\n*X\nA,[[[\n\u{feff}*Y\n1e999,0,0,0,1\n";
    for text in [huge.as_str(), garbage, "", "*A\n"] {
        assert!(s.execute("linetype", &json!({ "text": text })).is_err());
        assert!(s.execute("hatch", &json!({ "points": [[5, 5]], "patternText": text })).is_err());
        let _ = s.execute("hatch.patterns", &json!({ "text": text }));
    }
    for p in [json!({ "file": 7 }), json!({ "file": [] }), json!({ "text": null, "file": null })] {
        let _ = s.execute("linetype", &p);
        let _ = s.execute("hatch.patterns", &p);
    }
    assert!(hatches(&s).is_empty());
}
