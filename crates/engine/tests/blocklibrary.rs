//! Block libraries (the Blocks palette): BLOCKLIBRARY lists a folder of drawings or the blocks
//! of one drawing, BLOCKLIBIMPORT defines a library block in the current drawing, and
//! BLOCKLIBRARIES keeps the list of libraries.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Handle, Layer, Line};
use cadcraft_engine::{Input, Session};
use serde_json::json;

/// How many times a library file was parsed.
static READS: AtomicUsize = AtomicUsize::new(0);

/// Every library file reads as a drawing with blocks "Valve" (a line on layer Pipes, base point
/// (1, 1)) and "Pump" (empty), whatever its bytes.
fn read(_bytes: &[u8], name: &str) -> Result<Drawing, String> {
    READS.fetch_add(1, Ordering::SeqCst);
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Pipes".into(), ..Layer::default() });
    d.layers.push(Layer { name: "Spare".into(), ..Layer::default() });
    let mut valve = Block::new("Valve");
    valve.description = "Gate valve".into();
    valve.annotative = true;
    valve.units = 6;
    valve.explodable = false;
    valve.base = Vec3::new(1.0, 1.0, 0.0);
    let mut line = Entity::new(Handle(0x70), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(4.0, 0.0, 0.0) }));
    line.common.layer = "Pipes".into();
    valve.entities.push(line);
    d.blocks.insert("Valve".into(), Arc::new(valve));
    d.blocks.insert("Pump".into(), Arc::new(Block::new("Pump")));
    // "Outer" holds a reference to "Valve" and nothing else.
    let mut s = Session::new();
    *s.doc_mut().unwrap() = d;
    s.execute("insert", &json!({ "name": "Valve", "at": [2, 2] })).unwrap();
    let handles: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({ "name": "Outer", "base": [0, 0], "handles": handles, "keep": "delete" })).unwrap();
    // A dimension style nothing uses, with an arrowhead block of its own.
    if name.contains("arrow") {
        let d = s.doc_mut().unwrap();
        d.blocks.insert("TipArrow".into(), Arc::new(Block::new("TipArrow")));
        d.dim_styles.push(cadcraft_engine::doc::DimStyle { name: "Spare".into(), arrow_block: "TipArrow".into(), ..Default::default() });
    }
    // A crafted file: "Valve" holds "Outer", which holds "Valve".
    if name.contains("cycle") {
        let mut back = (**s.doc().unwrap().block("Outer").unwrap().entities.iter().next().unwrap()).clone();
        if let EntityKind::Insert(i) = &mut back.kind {
            i.block = "Outer".into();
        }
        let d = s.doc_mut().unwrap();
        let mut valve = (**d.block("Valve").unwrap()).clone();
        valve.entities.push(back);
        d.blocks.insert("Valve".into(), Arc::new(valve));
    }
    Ok(s.doc().unwrap().clone())
}

/// A new folder with two drawings and a file that is not one.
fn folder() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-blocklibrary-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["b-pumps.dwg", "A-valves.dxf", "notes.txt"] {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
    dir
}

/// Listing a drawing file and importing its blocks.
fn lists_and_imports(s: &mut Session, file: &str) {
    // A drawing file lists its blocks.
    let r = s.execute("blocklibrary", &json!({ "path": file })).unwrap();
    assert_eq!(r["kind"], "file");
    assert_eq!(
        r["blocks"],
        json!([{ "name": "Outer", "description": "" }, { "name": "Pump", "description": "" }, { "name": "Valve", "description": "Gate valve" }])
    );

    // Importing defines the block (and its layer) here, under another name when asked.
    let r = s.execute("blocklibimport", &json!({ "path": file, "block": "Valve", "name": "Gate" })).unwrap();
    assert_eq!(r["name"], "Gate");
    let d = s.doc().unwrap();
    let gate = d.block("Gate").unwrap();
    assert_eq!(gate.entities.iter().count(), 1);
    assert_eq!(gate.base, Vec3::new(1.0, 1.0, 0.0));
    assert_eq!((gate.description.as_str(), gate.annotative, gate.units, gate.explodable), ("Gate valve", true, 6, false));
    assert!(d.layers.iter().any(|l| l.name == "Pipes"));
    // Only what the block needs comes along: not the file's other layers or blocks.
    assert!(!d.layers.iter().any(|l| l.name == "Spare"), "an unused layer of the library stays out");
    assert!(d.block("Pump").is_none(), "an unused block of the library stays out");
    s.execute("insert", &json!({ "name": "Gate", "at": [10, 10] })).unwrap();
    // A block made of another block brings that one too (and its layer), not the unused ones.
    s.execute("blocklibimport", &json!({ "path": file, "block": "Outer", "name": "Assembly" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("Assembly").is_some());
    assert!(d.block("Valve").is_some(), "the nested block stays");
    assert!(d.block("Pump").is_none());
    assert!(d.layers.iter().any(|l| l.name == "Pipes"));
    // A whole drawing is named after its file and brings its objects (here none: model space is empty).
    let r = s.execute("blocklibimport", &json!({ "path": file })).unwrap();
    assert_eq!(r["name"], "A-valves");
    assert!(!s.doc().unwrap().layers.iter().any(|l| l.name == "Spare"));
    assert!(s.execute("blocklibimport", &json!({ "path": file, "block": "Nope" })).is_err());
    assert!(s.execute("blocklibimport", &json!({ "path": file, "block": "Valve", "name": "bad/name" })).is_err());
}

/// A dimension style nothing uses is left out, and its arrowhead block with it.
fn leaves_out_an_unused_dimension_style(s: &mut Session, dir: &std::path::Path) {
    let file = dir.join("arrow-set.dxf").to_string_lossy().to_string();
    std::fs::write(&file, b"x").unwrap();
    s.execute("blocklibimport", &json!({ "path": file, "block": "Valve", "name": "Tap" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("Tap").is_some());
    assert!(!d.dim_styles.iter().any(|t| t.name == "Spare"), "an unused dimension style stays out");
    assert!(d.block("TipArrow").is_none(), "so does its arrowhead block");
}

/// Adding and removing libraries.
fn keeps_the_list(s: &mut Session, dir: &std::path::Path, dir_s: &str, file: &str) {
    // The list: add (once), refuse what is not a library, remove.
    let l = s.execute("blocklibraries", &json!({ "op": "add", "path": dir_s })).unwrap();
    assert_eq!(l["libraries"], json!([dir_s]));
    // The same path again is kept once; letter case only matters on case-sensitive file systems.
    let again = s.execute("blocklibraries", &json!({ "op": "add", "path": dir_s })).unwrap();
    assert_eq!(again["libraries"], json!([dir_s]));
    if cfg!(any(windows, target_os = "macos")) {
        let upper = s.execute("blocklibraries", &json!({ "op": "add", "path": dir_s.to_uppercase() })).unwrap();
        assert_eq!(upper["libraries"], json!([dir_s]), "the same folder in other letters is the same library");
        assert_eq!(upper["path"], json!(dir_s), "the entry keeps its first spelling");
    }
    s.execute("blocklibraries", &json!({ "op": "add", "path": file })).unwrap();
    assert_eq!(s.execute("blocklibraries", &json!({})).unwrap()["libraries"], json!([dir_s, file]));
    assert!(s.execute("blocklibraries", &json!({ "op": "add", "path": dir.join("notes.txt").to_string_lossy() })).is_err());
    let l = s.execute("blocklibraries", &json!({ "op": "remove", "path": file })).unwrap();
    assert_eq!(l["libraries"], json!([dir_s]));
    s.execute("blocklibraries", &json!({ "op": "remove", "path": dir_s })).unwrap();
}

/// Names, paths and files that try to pass for something else.
fn refuses_hostile_input(s: &mut Session, dir: &std::path::Path, dir_s: &str) {
    // A block whose name looks like `name=path` is that block: INSERT reads no file for it.
    let reads = READS.load(Ordering::SeqCst);
    let mut odd = Session::new();
    odd.doc_mut().unwrap().blocks.insert("a=b".into(), Arc::new(Block::new("a=b")));
    odd.start("insert").unwrap();
    odd.input(Input::Text("a=b".into())).unwrap();
    assert!(odd.prompt_text().starts_with("INSERT Specify insertion point"), "{}", odd.prompt_text());
    assert_eq!(READS.load(Ordering::SeqCst), reads, "no file was read");
    odd.input(Input::Cancel).unwrap();

    // What is not a readable drawing fails with an error.
    assert!(s.execute("blocklibimport", &json!({ "path": dir_s })).is_err(), "a folder is not a drawing");
    assert!(s.execute("blocklibrary", &json!({ "path": dir.join("missing.dxf").to_string_lossy() })).is_err());

    // A library whose blocks hold each other imports without hanging, and the session goes on.
    let cycle = dir.join("cycle.dxf").to_string_lossy().to_string();
    std::fs::write(&cycle, b"x").unwrap();
    let _ = s.execute("blocklibimport", &json!({ "path": cycle, "block": "Outer", "name": "Loop" }));
    s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
}

/// A file with a huge block table.
fn bounds_a_huge_table() {
    // A file with a huge block table lists a bounded part of it, and says so.
    let mut many = Session::new();
    let d = many.doc_mut().unwrap();
    for i in 0..10_001 {
        let name = format!("B{i}");
        d.blocks.insert(name.clone(), Arc::new(Block::new(&name)));
    }
    let r = many.execute("blockspalette", &json!({})).unwrap();
    let names: Vec<&str> = r["blocks"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 10_000);
    assert_eq!(r["truncated"], true);
    // The first ones by name, sorted: B9999 comes last of all, so it is the one left out.
    assert_eq!(names.first(), Some(&"B0"));
    assert!(!names.contains(&"B9999"));
    assert!(names.windows(2).all(|w| w[0].to_ascii_lowercase() <= w[1].to_ascii_lowercase()));
}

/// The list file in the settings folder.
fn checks_the_list_file(s: &mut Session, dir: &std::path::Path, dir_s: &str) {
    // The list file is checked when it is read: junk, bad entries, too many, too big.
    let list = dir.join("block-libraries.json");
    let libraries = |s: &mut Session| s.execute("blocklibraries", &json!({})).unwrap()["libraries"].as_array().unwrap().clone();
    std::fs::write(&list, "not json").unwrap();
    assert!(libraries(s).is_empty());
    // A file that is there but unreadable is never written over: the list stays as the user left it.
    assert!(s.execute("blocklibraries", &json!({ "op": "add", "path": dir_s })).is_err());
    assert!(s.execute("blocklibraries", &json!({ "op": "remove", "path": dir_s })).is_err());
    assert_eq!(std::fs::read(&list).unwrap(), b"not json");
    let mut hostile: Vec<String> = (0..100).map(|i| format!("C:/lib{i}")).collect();
    hostile.insert(0, String::new());
    hostile.insert(1, "bad\nname".into());
    hostile.insert(2, "x".repeat(10_000));
    std::fs::write(&list, serde_json::to_vec(&hostile).unwrap()).unwrap();
    let kept = libraries(s);
    assert_eq!(kept.len(), 64);
    assert_eq!(kept[0], "C:/lib0");
    std::fs::write(&list, vec![b' '; 300 << 10]).unwrap();
    assert!(libraries(s).is_empty());
    // What the list file holds after an addition is the list, whole.
    std::fs::remove_file(&list).unwrap();
    s.execute("blocklibraries", &json!({ "op": "add", "path": dir_s })).unwrap();
    assert_eq!(serde_json::from_slice::<Vec<String>>(&std::fs::read(&list).unwrap()).unwrap(), vec![dir_s.to_string()]);
    // Paths with control characters or of absurd length are refused.
    assert!(s.execute("blocklibraries", &json!({ "op": "add", "path": format!("{dir_s}\nx") })).is_err());
    assert!(s.execute("blocklibraries", &json!({ "op": "add", "path": "y".repeat(5000) })).is_err());
}

/// One test: the file hooks and the settings folder are global to the process.
#[test]
fn libraries_list_import_and_are_kept() {
    set_io(IoHooks { read, write: |_, _| Err("no writer in this test".into()), plot: None });
    // No settings folder: the list lives in memory, and the real one is never written.
    cadcraft_engine::settings_dir::set(None);
    let dir = folder();
    let dir_s = dir.to_string_lossy().to_string();
    let mut s = Session::new();

    // A folder lists its drawings by name, case-insensitively sorted; other files are not drawings.
    let r = s.execute("blocklibrary", &json!({ "path": dir_s })).unwrap();
    assert_eq!(r["kind"], "folder");
    let names: Vec<&str> = r["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["A-valves", "b-pumps"]);

    let reads_before = READS.load(Ordering::SeqCst);
    let file = dir.join("A-valves.dxf").to_string_lossy().to_string();
    lists_and_imports(&mut s, &file);
    // The file was parsed once for all of that, and again only when it changes.
    assert_eq!(READS.load(Ordering::SeqCst) - reads_before, 1, "the file is parsed once, not for every block placed");
    std::fs::write(&file, b"changed contents, another size").unwrap();
    s.execute("blocklibrary", &json!({ "path": file })).unwrap();
    assert_eq!(READS.load(Ordering::SeqCst) - reads_before, 2, "a changed file is read again");
    // Refresh in the palette reads it again even when its size and time look the same.
    cadcraft_engine::cmd::clear_library_cache();
    let listed = s.execute("blocklibrary", &json!({ "path": file })).unwrap();
    assert_eq!(READS.load(Ordering::SeqCst) - reads_before, 3, "a cleared file is read again");
    // The palette's worker thread gets the same answer without a session.
    assert_eq!(cadcraft_engine::cmd::list_library(&file).unwrap(), listed);

    leaves_out_an_unused_dimension_style(&mut s, &dir);
    keeps_the_list(&mut s, &dir, &dir_s, &file);
    refuses_hostile_input(&mut s, &dir, &dir_s);
    bounds_a_huge_table();
    cadcraft_engine::settings_dir::set(Some(dir.clone()));
    checks_the_list_file(&mut s, &dir, &dir_s);
    cadcraft_engine::settings_dir::set(None);

    std::fs::remove_dir_all(&dir).unwrap();
}
