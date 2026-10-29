//! External references (#525): XATTACH and -XREF on real DXF files in a temporary folder —
//! loading with `xref|name` symbols, saving only the reference, reopening, Reload/Unload/Detach/
//! Path, Bind and XBIND, overlays, missing files, circular and deeply nested references.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Block, Circle, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line, Linetype, MAX_BLOCK_DEPTH, XrefStatus};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A fresh folder for one test.
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-xref-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn save(d: &Drawing, path: &Path) {
    std::fs::write(path, cadcraft_io::write(d, path).unwrap()).unwrap();
}

fn line(d: &mut Drawing, layer: &str, a: (f64, f64), b: (f64, f64)) {
    let h = d.new_handle();
    let mut e = Entity::new(h, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }));
    e.common.layer = layer.into();
    d.model.push(e);
}

fn insert(block: &str, x: f64) -> EntityKind {
    EntityKind::Insert(Insert {
        block: block.into(),
        insert: Vec3::new(x, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    })
}

/// A site plan: a red line on layer Walls and a reference to its block Tree (a circle).
fn site() -> Drawing {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), color: Color::Index(1), ..Layer::default() });
    line(&mut d, "Walls", (0.0, 0.0), (10.0, 0.0));
    let mut tree = Block::new("Tree");
    tree.entities.push(Entity::new(Handle(0x50), EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    d.blocks.insert("Tree".into(), Arc::new(tree));
    let h = d.new_handle();
    d.model.push(Entity::new(h, insert("Tree", 5.0)));
    d
}

/// A drawing with an xref named after `path`'s stem (as attach or overlay) placed at 0,0.
fn host_with(xrefs: &[(&str, bool)]) -> Drawing {
    let mut d = Drawing::new_metric();
    for (path, overlay) in xrefs {
        let name = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
        let mut b = Block::new(&name);
        b.xref_path = Some(path.to_string());
        b.xref_overlay = *overlay;
        d.blocks.insert(name.clone(), Arc::new(b));
        let h = d.new_handle();
        d.model.push(Entity::new(h, insert(&name, 0.0)));
    }
    d
}

fn open(s: &mut Session, path: &Path) {
    s.execute("open", &json!({ "path": path.to_string_lossy() })).unwrap();
}

fn status(s: &Session, name: &str) -> Option<XrefStatus> {
    s.doc().unwrap().block(name).map(|b| b.xref_status)
}

fn objects(s: &Session, name: &str) -> usize {
    s.doc().unwrap().block(name).map_or(0, |b| b.entities.len())
}

fn has_layer(s: &Session, name: &str) -> bool {
    s.doc().unwrap().layer(name).is_some()
}

#[test]
fn attach_loads_the_file_and_saving_keeps_only_the_reference() {
    hooks();
    let dir = folder("attach");
    save(&site(), &dir.join("site.dxf"));
    let host = dir.join("host.dxf");
    save(&Drawing::new_metric(), &host);
    let mut s = Session::new();
    open(&mut s, &host);

    // A path relative to the host drawing's folder, placed at 100,0.
    let r = s.execute("xattach", &json!({ "path": "site.dxf", "at": [100, 0] })).unwrap();
    assert_eq!(r["name"], "site");
    assert_eq!(status(&s, "site"), Some(XrefStatus::Loaded));
    assert_eq!(objects(&s, "site"), 2, "the site's model space");
    let d = s.doc().unwrap();
    assert_eq!(d.layer("site|Walls").map(|l| l.color), Some(Color::Index(1)), "layers come prefixed");
    assert!(d.block("site|Tree").is_some(), "its blocks too");
    let ext = d.extents(&cadcraft_engine::doc::Space::Model);
    assert!(ext.max.x >= 110.0 - 1e-9, "the xref is drawn where it was placed: {ext:?}");

    // Saved: the xref block with its path and no objects, and none of its symbols but its layers
    // (their settings in the host are the host's, #886).
    s.execute("qsave", &json!({})).unwrap();
    let saved = cadcraft_io::read(&std::fs::read(&host).unwrap(), "host.dxf").unwrap();
    let b = saved.block("site").unwrap();
    assert_eq!((b.xref_path.as_deref(), b.entities.len()), (Some("site.dxf"), 0));
    assert_eq!(saved.layer("site|Walls").map(|l| l.color), Some(Color::Index(1)));
    assert!(saved.linetypes.iter().all(|t| !t.name.contains('|')), "only the ones its layers use");
    assert!(saved.blocks.keys().all(|n| !n.contains('|')));

    // Reopened: loaded again from the file.
    let mut t = Session::new();
    open(&mut t, &host);
    assert_eq!(status(&t, "site"), Some(XrefStatus::Loaded));
    assert_eq!(objects(&t, "site"), 2);
    assert!(has_layer(&t, "site|Walls"));
}

#[test]
fn xref_layer_overrides_survive_save_and_reopen_in_dxf_and_dwg() {
    // #886: a colour and a freeze set in the host on the xref's layers are the host's (AutoCAD,
    // VISRETAIN = 1), so they come back after saving and reopening instead of the xref's own.
    hooks();
    let dir = folder("overrides");
    let mut x = site();
    x.linetypes.push(Linetype::simple("Dashed", "Dashed", &[0.5, -0.25]));
    x.layers.push(Layer { name: "Doors".into(), color: Color::Index(3), linetype: "Dashed".into(), ..Layer::default() });
    save(&x, &dir.join("site.dxf"));
    for ext in ["dxf", "dwg"] {
        let host = dir.join(format!("host.{ext}"));
        save(&host_with(&[("site.dxf", false)]), &host);
        let mut s = Session::new();
        open(&mut s, &host);
        let d = s.doc_mut().unwrap();
        d.layer_mut("site|Walls").unwrap().color = Color::Index(252);
        d.layer_mut("site|Doors").unwrap().frozen = true;
        s.execute("qsave", &json!({})).unwrap();

        let mut t = Session::new();
        open(&mut t, &host);
        assert_eq!(status(&t, "site"), Some(XrefStatus::Loaded), "{ext}");
        let d = t.doc().unwrap();
        assert_eq!(d.layer("site|Walls").map(|l| l.color), Some(Color::Index(252)), "{ext}: the host's colour, not the xref's red");
        assert_eq!(d.layer("site|Doors").map(|l| (l.frozen, l.color)), Some((true, Color::Index(3))), "{ext}: frozen in the host");
        assert_eq!(d.layer("site|Doors").map(|l| l.linetype.as_str()), Some("site|Dashed"), "{ext}: its linetype is still the xref's");
        assert!(d.linetype("site|Dashed").is_some(), "{ext}");
    }
}

#[test]
fn reload_unload_path_and_detach() {
    hooks();
    let dir = folder("manage");
    save(&site(), &dir.join("site.dxf"));
    let host = dir.join("host.dxf");
    save(&host_with(&[("site.dxf", false)]), &host);
    let mut s = Session::new();
    open(&mut s, &host);
    assert_eq!(objects(&s, "site"), 2);

    // A frozen xref layer stays frozen when the xref reloads.
    s.doc_mut().unwrap().layer_mut("site|Walls").unwrap().frozen = true;
    let mut changed = site();
    line(&mut changed, "Walls", (0.0, 5.0), (10.0, 5.0));
    save(&changed, &dir.join("site.dxf"));
    s.execute("-xref", &json!({ "option": "reload", "names": "SITE" })).unwrap();
    assert_eq!(objects(&s, "site"), 3, "reload reads the file again");
    assert!(s.doc().unwrap().layer("site|Walls").unwrap().frozen);

    s.execute("-xref", &json!({ "option": "unload", "names": "site" })).unwrap();
    assert_eq!((status(&s, "site"), objects(&s, "site")), (Some(XrefStatus::Unloaded), 0));
    assert!(s.doc().unwrap().block("site|Tree").is_none());
    s.execute("-xref", &json!({ "option": "reload" })).unwrap();
    assert_eq!(status(&s, "site"), Some(XrefStatus::Loaded));

    // Path: point it at another file.
    std::fs::create_dir_all(dir.join("old")).unwrap();
    save(&site(), &dir.join("old/site2.dxf"));
    s.execute("-xref", &json!({ "option": "path", "names": "site", "path": "old/site2.dxf" })).unwrap();
    assert_eq!(s.doc().unwrap().block("site").unwrap().xref_path.as_deref(), Some("old/site2.dxf"));
    assert_eq!(objects(&s, "site"), 2);

    // ?: the list the palette shows.
    let r = s.execute("-xref", &json!({ "option": "?" })).unwrap();
    assert_eq!(r["xrefs"][0]["status"], "Loaded");
    assert_eq!(r["xrefs"][0]["references"], 1);

    // Detach: the definition, its references and its symbols go.
    s.execute("-xref", &json!({ "option": "detach", "names": "site" })).unwrap();
    let d = s.doc().unwrap();
    assert!(d.block("site").is_none());
    assert!(d.model.iter().all(|e| !matches!(&e.kind, EntityKind::Insert(_))));
    assert!(d.layers.iter().all(|l| !l.name.contains('|')));
    assert!(s.execute("-xref", &json!({ "option": "reload", "names": "site" })).is_err(), "nothing left to reload");
}

#[test]
fn missing_files_leave_the_host_intact() {
    hooks();
    let dir = folder("missing");
    let host = dir.join("host.dxf");
    save(&host_with(&[("gone.dxf", false)]), &host);
    let mut s = Session::new();
    open(&mut s, &host);
    assert_eq!((status(&s, "gone"), objects(&s, "gone")), (Some(XrefStatus::NotFound), 0));
    assert!(s.log.iter().any(|m| m.contains("gone") && m.contains("not loaded")), "{:?}", s.log);
    // The reference survives a save.
    s.execute("qsave", &json!({})).unwrap();
    let saved = cadcraft_io::read(&std::fs::read(&host).unwrap(), "host.dxf").unwrap();
    assert_eq!(saved.block("gone").and_then(|b| b.xref_path.clone()).as_deref(), Some("gone.dxf"));

    // Attaching a file that isn't there changes nothing.
    let before = s.doc().unwrap().blocks.len();
    assert!(s.execute("xattach", &json!({ "path": "nowhere/x.dxf" })).is_err());
    assert_eq!(s.doc().unwrap().blocks.len(), before);

    // Found later by its file name next to the host, wherever it was saved from.
    save(&site(), &dir.join("gone.dxf"));
    s.execute("-xref", &json!({ "option": "path", "names": "gone", "path": "C:\\old\\job\\gone.dxf" })).unwrap();
    assert_eq!(status(&s, "gone"), Some(XrefStatus::Loaded));
}

#[test]
fn overlays_do_not_load_in_the_drawings_that_reference_their_host() {
    hooks();
    let dir = folder("overlay");
    save(&site(), &dir.join("site.dxf"));
    let plan = dir.join("plan.dxf");
    save(&host_with(&[("site.dxf", true)]), &plan);
    let mut s = Session::new();
    open(&mut s, &plan);
    assert_eq!(status(&s, "site"), Some(XrefStatus::Loaded), "an overlay loads in its own host");
    s.execute("qsave", &json!({})).unwrap();
    let saved = cadcraft_io::read(&std::fs::read(&plan).unwrap(), "plan.dxf").unwrap();
    assert!(saved.block("site").unwrap().xref_overlay, "the overlay flag is saved");

    // Attached plan: its overlaid site isn't loaded; an attached one would be.
    let top = dir.join("top.dxf");
    save(&host_with(&[("plan.dxf", false)]), &top);
    let mut t = Session::new();
    open(&mut t, &top);
    assert_eq!(status(&t, "plan"), Some(XrefStatus::Loaded));
    assert_eq!(objects(&t, "plan|site"), 0);
    s.execute("-xref", &json!({ "option": "type", "names": "site", "overlay": false })).unwrap();
    s.execute("qsave", &json!({})).unwrap();
    t.execute("-xref", &json!({ "option": "reload", "names": "plan" })).unwrap();
    assert_eq!(objects(&t, "plan|site"), 2, "nested attached xrefs load");
    assert!(t.doc().unwrap().layer("plan|site|Walls").is_some());
}

#[test]
fn circular_and_deeply_nested_references_are_bounded() {
    hooks();
    let dir = folder("cycle");
    // a → b → a.
    let mut a = host_with(&[("b.dxf", false)]);
    line(&mut a, "0", (0.0, 0.0), (1.0, 1.0));
    save(&a, &dir.join("a.dxf"));
    let mut b = host_with(&[("a.dxf", false)]);
    line(&mut b, "0", (0.0, 0.0), (2.0, 2.0));
    save(&b, &dir.join("b.dxf"));
    let mut s = Session::new();
    open(&mut s, &dir.join("a.dxf"));
    assert_eq!(status(&s, "b"), Some(XrefStatus::Loaded));
    assert_eq!(objects(&s, "b|a"), 0, "the circular reference back to a isn't loaded");
    // A drawing can't reference itself.
    assert!(s.execute("xattach", &json!({ "path": "a.dxf", "name": "self" })).is_err());

    // A chain deeper than the nesting limit stops at the limit.
    let n = MAX_BLOCK_DEPTH + 4;
    for i in 0..n {
        let mut d = host_with(&[(&format!("f{}.dxf", i + 1), false)]);
        line(&mut d, "0", (0.0, 0.0), (i as f64, 1.0));
        save(&d, &dir.join(format!("f{i}.dxf")));
    }
    let mut t = Session::new();
    open(&mut t, &dir.join("f0.dxf"));
    assert_eq!(status(&t, "f1"), Some(XrefStatus::Loaded));
    let depth = t.doc().unwrap().blocks.keys().map(|k| k.matches('|').count()).max().unwrap_or(0);
    assert!(depth <= MAX_BLOCK_DEPTH, "nesting is bounded: {depth}");
}

#[test]
fn bind_makes_the_xref_part_of_the_drawing() {
    hooks();
    let dir = folder("bind");
    save(&site(), &dir.join("site.dxf"));
    let host = dir.join("host.dxf");
    save(&host_with(&[("site.dxf", false)]), &host);
    let mut s = Session::new();
    open(&mut s, &host);

    // XBIND copies one layer; the xref stays attached.
    let r = s.execute("xbind", &json!({ "type": "layer", "names": "site|walls" })).unwrap();
    assert_eq!(r["bound"][0], "site$0$Walls");
    assert!(s.doc().unwrap().block("site").unwrap().xref_path.is_some());

    // Bind: a normal block, its symbols renamed site$n$name.
    s.execute("-xref", &json!({ "option": "bind", "names": "site" })).unwrap();
    let d = s.doc().unwrap();
    let b = d.block("site").unwrap();
    assert!(b.xref_path.is_none());
    assert_eq!(b.entities.len(), 2);
    assert!(d.layer("site$1$Walls").is_some(), "the next free number after the XBIND copy");
    assert!(d.block("site$0$Tree").is_some());
    assert!(d.layers.iter().all(|l| !l.name.contains('|')));
    assert!(b.entities.iter().any(|e| matches!(&e.kind, EntityKind::Insert(i) if i.block == "site$0$Tree")));

    // It is saved as a normal block now.
    s.execute("qsave", &json!({})).unwrap();
    let saved = cadcraft_io::read(&std::fs::read(&host).unwrap(), "host.dxf").unwrap();
    assert_eq!(saved.block("site").map(|b| b.entities.len()), Some(2));
    assert!(saved.layer("site$1$Walls").is_some());

    // Insert type: the symbols keep their own names (an existing one is used instead).
    let mut t = Session::new();
    let host2 = dir.join("host2.dxf");
    let mut h2 = host_with(&[("site.dxf", false)]);
    h2.layers.push(Layer { name: "Walls".into(), color: Color::Index(5), ..Layer::default() });
    save(&h2, &host2);
    open(&mut t, &host2);
    t.execute("-xref", &json!({ "option": "bind", "insert": true })).unwrap();
    let d = t.doc().unwrap();
    assert_eq!(d.layer("Walls").map(|l| l.color), Some(Color::Index(5)));
    assert!(d.block("Tree").is_some());
    assert!(d.layers.iter().all(|l| !l.name.contains('|')));

    // Unloaded xrefs can't be bound.
    let mut u = Session::new();
    open(&mut u, &host2);
    u.execute("-xref", &json!({ "option": "unload" })).unwrap();
    assert!(u.execute("-xref", &json!({ "option": "bind" })).is_err());
}

#[test]
fn command_line_xref_and_xattach() {
    hooks();
    let dir = folder("cmdline");
    save(&site(), &dir.join("site.dxf"));
    let host = dir.join("host.dxf");
    save(&Drawing::new_metric(), &host);
    let mut s = Session::new();
    open(&mut s, &host);

    s.script("-XREF\nA\nsite.dxf\n50,0\n2\n\n0\n").unwrap();
    assert!(s.running.is_none(), "finished: {}", s.prompt_text());
    let ins: Vec<(String, f64, f64)> = s
        .doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some((i.block.clone(), i.insert.x, i.scale.y)),
            _ => None,
        })
        .collect();
    assert_eq!(ins, vec![("site".to_string(), 50.0, 2.0)]);

    s.script("-XREF\n?\n\n").unwrap();
    assert!(s.log.iter().any(|m| m.starts_with("site") && m.contains("Loaded")), "{:?}", s.log);
    s.script("-XREF\nU\nsite\n").unwrap();
    assert_eq!(status(&s, "site"), Some(XrefStatus::Unloaded));
    s.script("-XREF\nR\n*\n").unwrap();
    assert_eq!(status(&s, "site"), Some(XrefStatus::Loaded));
    s.script("XBIND\nLA\nsite|Walls\n").unwrap();
    assert!(has_layer(&s, "site$0$Walls"));
    s.script("XATTACH\nsite.dxf\n0,0\n1\n\n0\n").unwrap();
    assert!(s.running.is_none(), "finished: {}", s.prompt_text());
    assert_eq!(s.doc().unwrap().blocks.values().filter(|b| b.xref_path.is_some()).count(), 1, "the same xref is used again");
    s.script("-XREF\nD\nsite\n").unwrap();
    assert!(s.doc().unwrap().block("site").is_none());
    assert!(s.undo().unwrap().is_some());
    assert!(s.doc().unwrap().block("site").is_some(), "detach undoes");
}

#[test]
fn hostile_xref_input_never_panics() {
    hooks();
    let dir = folder("hostile");
    std::fs::write(dir.join("junk.dxf"), b"0\nSECTION\n2\nENTITIES\n0\nLINE\n10\nnan\n").unwrap();
    std::fs::write(dir.join("empty.dxf"), b"").unwrap();
    let host = dir.join("host.dxf");
    let mut d = host_with(&[("junk.dxf", false), ("empty.dxf", false), ("../../../../../../dev/null", false)]);
    let mut blank = Block::new("blank");
    blank.xref_path = Some(String::new());
    d.blocks.insert("blank".into(), Arc::new(blank));
    save(&d, &host);
    let mut s = Session::new();
    open(&mut s, &host);
    let dirpath = dir.to_string_lossy().into_owned();
    for p in [
        json!({}),
        json!({ "path": "" }),
        json!({ "path": dirpath }),
        json!({ "path": "junk.dxf", "scale": 0 }),
        json!({ "path": "junk.dxf", "scale": [1, "x"] }),
        json!({ "path": "junk.dxf", "rotation": 1e308, "name": "a|b" }),
        json!({ "path": "\u{0}" }),
        json!({ "path": 5 }),
    ] {
        let _ = s.execute("xattach", &p);
    }
    for p in [
        json!({ "option": "nonsense" }),
        json!({ "option": "path" }),
        json!({ "option": "bind", "names": [1, 2] }),
        json!({ "option": "detach", "names": "*" }),
        json!({ "option": "reload", "names": "*" }),
    ] {
        let _ = s.execute("-xref", &p);
    }
    for p in [json!({}), json!({ "type": "layer" }), json!({ "type": "block", "names": "|" }), json!({ "type": "x", "names": "*" })] {
        let _ = s.execute("xbind", &p);
    }
    let _ = s.execute("xopen", &json!({ "name": "nothing" }));
    let _ = s.execute("xopen", &json!({}));
}

/// A drawing with xrefs `(name, saved path)`, each placed at 0,0.
fn host_with_paths(xrefs: &[(&str, &str)]) -> Drawing {
    let mut d = Drawing::new_metric();
    for (name, path) in xrefs {
        let mut b = Block::new(name);
        b.xref_path = Some(path.to_string());
        d.blocks.insert(name.to_string(), Arc::new(b));
        let h = d.new_handle();
        d.model.push(Entity::new(h, insert(name, 0.0)));
    }
    d
}

#[test]
fn saved_xref_paths_are_followed_beside_the_host_or_on_this_computer_only() {
    hooks();
    let base = folder("saved-paths");
    let (dir, elsewhere) = (base.join("plans"), base.join("elsewhere"));
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    save(&site(), &elsewhere.join("far.dxf"));
    // A part beside the host that references drawings the same ways.
    let mut nested = vec![("unc", r"\\192.0.2.1\share\far.dxf".to_string())];
    // Read as a local path by Linux, and as a network path by the resolver.
    let far_slashes = format!("/{}", elsewhere.join("far.dxf").display());
    if cfg!(unix) {
        nested.push(("slashes", far_slashes.clone()));
    }
    let mut part = host_with_paths(&nested.iter().map(|(n, p)| (*n, p.as_str())).collect::<Vec<_>>());
    line(&mut part, "0", (0.0, 0.0), (1.0, 1.0));
    save(&part, &dir.join("parts").join("part.dxf"));

    let mut xrefs = vec![
        ("unc", r"\\192.0.2.1\share\far.dxf".to_string()),
        ("url", "file://192.0.2.1/s/far.dxf".to_string()),
        ("part", "parts/part.dxf".to_string()),
    ];
    if cfg!(unix) {
        xrefs.push(("slashes", far_slashes.clone()));
    }
    let host = dir.join("host.dxf");
    save(&host_with_paths(&xrefs.iter().map(|(n, p)| (*n, p.as_str())).collect::<Vec<_>>()), &host);
    let mut s = Session::new();
    open(&mut s, &host);

    assert_eq!(status(&s, "part"), Some(XrefStatus::Loaded), "a relative path beside the host: {:?}", s.log);
    assert_eq!(objects(&s, "part"), 1 + nested.len(), "the part's line and references, with nothing loaded into them");
    for name in ["unc", "url", "slashes"].into_iter().take(xrefs.len() - 1) {
        assert_eq!((status(&s, name), objects(&s, name)), (Some(XrefStatus::Unresolved), 0), "{name}: {:?}", s.log);
        assert!(s.log.iter().any(|m| m.contains(&format!("\"{name}\" not loaded")) && m.contains("another computer")), "{name}: {:?}", s.log);
    }
    assert!(s.log.iter().any(|m| m.contains("nested xref \"unc\" not loaded") && m.contains("another computer")), "{:?}", s.log);
    if cfg!(unix) {
        assert!(s.log.iter().any(|m| m.contains("nested xref \"slashes\" not loaded")), "{:?}", s.log);
    }
    let r = s.execute("-xref", &json!({ "option": "?" })).unwrap();
    let found: Vec<(&str, bool)> = r["xrefs"].as_array().unwrap().iter().map(|x| (x["name"].as_str().unwrap(), x["foundAt"].is_string())).collect();
    assert!(found.iter().all(|(n, f)| *f == (*n == "part")), "{found:?}");
    assert!(s.execute("xopen", &json!({ "name": "unc" })).unwrap_err().to_string().contains("another computer"));
    // Reloading follows the saved path the same way.
    s.execute("-xref", &json!({ "option": "reload", "names": "*" })).unwrap();
    assert_eq!(status(&s, "unc"), Some(XrefStatus::Unresolved));

    // A path the user gives is used as given.
    let far = elsewhere.join("far.dxf");
    s.execute("-xref", &json!({ "option": "path", "names": "unc", "path": far.to_str().unwrap() })).unwrap();
    assert_eq!(status(&s, "unc"), Some(XrefStatus::Loaded));
}
