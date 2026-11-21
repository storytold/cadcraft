//! REFEDIT on xrefs and nested references (gap F2): the source drawing written back on REFCLOSE
//! Save (with a backup; refused for read-only, locked or changed files) and nesting levels.
//! Files go in a folder under Cargo's temporary directory for integration tests.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cadcraft_engine::cmd::file::{IoHooks, Version, set_io, set_write_as};
use cadcraft_engine::doc::geom::{Vec2, Vec3};
use cadcraft_engine::doc::{Block, Circle, Drawing, Entity, EntityKind, Handle, Insert, Layer, Line, XrefStatus};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_write_as(|d, name, o| {
        cadcraft_io::write_with(d, name, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
}

/// A fresh folder for one test, removed when the test ends however it ends (a skip, a failed
/// assertion).
struct Folder(PathBuf);

impl std::ops::Deref for Folder {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder(tag: &str) -> Folder {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("refedit-xref").join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Folder(dir)
}

fn save(d: &Drawing, path: &Path) {
    std::fs::write(path, cadcraft_io::write(d, path).unwrap()).unwrap();
}

fn read(path: &Path) -> Drawing {
    cadcraft_io::read(&std::fs::read(path).unwrap(), path).unwrap()
}

fn line(d: &mut Drawing, layer: &str, a: (f64, f64), b: (f64, f64)) -> Handle {
    let h = d.new_handle();
    let mut e = Entity::new(h, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) }));
    e.common.layer = layer.into();
    d.model.push(e);
    h
}

fn insert(block: &str, at: Vec3) -> Insert {
    Insert {
        block: block.into(),
        insert: at,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
        clip: None,
    }
}

/// A site plan: a line on layer Walls and a reference to its block Tree (a circle).
fn site() -> (Drawing, Handle) {
    let mut d = Drawing::new_metric();
    d.layers.push(Layer { name: "Walls".into(), ..Layer::default() });
    let wall = line(&mut d, "Walls", (0.0, 0.0), (10.0, 0.0));
    let mut tree = Block::new("Tree");
    let c = d.new_handle();
    tree.entities.push(Entity::new(c, EntityKind::Circle(Circle { center: Vec3::ZERO, radius: 1.0 })));
    d.blocks.insert("Tree".into(), Arc::new(tree));
    let h = d.new_handle();
    d.model.push(Entity::new(h, EntityKind::Insert(insert("Tree", Vec3::new(5.0, 0.0, 0.0)))));
    (d, wall)
}

/// A host drawing (saved as host.dxf) with site.dxf attached at (100, 0).
fn attached(tag: &str) -> (Session, Folder, Handle, String) {
    hooks();
    let dir = folder(tag);
    let (d, wall) = site();
    save(&d, &dir.join("site.dxf"));
    let host = dir.join("host.dxf");
    save(&Drawing::new_metric(), &host);
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_string_lossy() })).unwrap();
    let r = s.execute("xattach", &json!({ "path": "site.dxf", "at": [100, 0] })).unwrap();
    let insert = r["handle"].as_str().unwrap().to_string();
    (s, dir, wall, insert)
}

fn hx(v: Value) -> String {
    v.get("handle").unwrap_or(&v["handles"][0]).as_str().unwrap().to_string()
}

fn working(r: &Value) -> Vec<String> {
    r["working"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

fn find_line(s: &Session, hs: &[String]) -> String {
    let d = s.doc().unwrap();
    hs.iter().find(|h| matches!(d.entity(Handle::parse_hex(h).unwrap()).unwrap().kind, EntityKind::Line(_))).unwrap().clone()
}

#[test]
fn editing_an_xref_writes_its_drawing_back_with_a_backup() {
    let (mut s, dir, wall, insert) = attached("save");
    let file = dir.join("site.dxf");
    // A layer of the host only, current for what is drawn during the edit.
    s.execute("layer.new", &json!({ "name": "Added", "current": true })).unwrap();
    let r = s.execute("refedit", &json!({ "handle": insert })).unwrap();
    assert_eq!(r["xref"]["name"], "site");
    let work = working(&r);
    assert_eq!(work.len(), 2);
    // Everything but the working set is locked while editing, and drawn faded.
    let l = find_line(&s, &work);
    s.execute("move", &json!({ "handles": [l], "delta": [0, 5] })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    let out = s.execute("refclose", &json!({ "option": "save" })).unwrap();
    assert_eq!(out["objects"], 3);
    assert!(out.get("notSaved").is_none() && !s.log.iter().any(|m| m.contains("Not saved")), "the file keeps everything: no warning");
    assert!(out["file"].as_str().unwrap().ends_with("site.dxf"));
    let bak = dir.join("site.dxf.bak");
    assert!(out["backup"].as_str().unwrap().ends_with("site.dxf.bak"), "{out}");
    assert_eq!(read(&bak).model.len(), 2, "the previous file is kept");
    // The file holds the edit, in its own coordinates, with the line's own handle.
    let back = read(&file);
    assert_eq!(back.model.len(), 3);
    let moved = back.entity(wall).expect("the edited line keeps its handle");
    let EntityKind::Line(ln) = &moved.kind else { panic!() };
    assert!((ln.a.y - 5.0).abs() < 1e-9 && (ln.a.x).abs() < 1e-9, "{ln:?}");
    assert_eq!(moved.common.layer, "Walls", "the xref's layer loses its prefix");
    let new_line = back.model.iter().find(|e| matches!(&e.kind, EntityKind::Line(l) if (l.a.y - 20.0).abs() < 1e-9)).unwrap();
    assert_eq!(new_line.common.layer, "Added");
    assert!(back.layer("Added").is_some(), "a host layer the new line uses comes along");
    // The host shows the xref as written (loaded again), and nothing of the edit is left.
    let d = s.doc().unwrap();
    assert!(d.refedit.is_none());
    assert_eq!(d.block("site").unwrap().entities.len(), 3);
    assert_eq!(d.block("site").unwrap().xref_status, XrefStatus::Loaded);
}

#[test]
fn a_block_of_the_xref_is_written_into_that_block() {
    let (mut s, dir, _, insert) = attached("nested");
    let levels = s.execute("refedit.levels", &json!({ "handle": insert })).unwrap();
    let names: Vec<&str> = levels["levels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["site", "site|Tree"]);
    let r = s.execute("refedit", &json!({ "handle": insert, "nested": "site|Tree" })).unwrap();
    assert_eq!(r["block"], "site|Tree");
    assert_eq!(r["path"].as_array().unwrap().len(), 1);
    assert!(r["faded"].as_u64().unwrap() >= 1, "the site's other objects stand faded around it");
    let c = working(&r)[0].clone();
    // The circle shows at the tree's place in the drawing: (100 + 5, 0).
    let EntityKind::Circle(circle) = &s.doc().unwrap().entity(Handle::parse_hex(&c).unwrap()).unwrap().kind else { panic!() };
    assert!((circle.center.x - 105.0).abs() < 1e-9);
    s.execute("scale", &json!({ "handles": [c], "base": [105, 0], "factor": 2 })).unwrap();
    s.execute("refclose", &json!({})).unwrap();
    let back = read(&dir.join("site.dxf"));
    let EntityKind::Circle(c) = &back.block("Tree").unwrap().entities.iter().next().unwrap().kind else { panic!() };
    assert!((c.radius - 2.0).abs() < 1e-9);
}

#[test]
fn locked_read_only_or_changed_files_are_not_overwritten() {
    // A lock file of another program: refused at the start.
    let (mut s, dir, _, insert) = attached("locked");
    let file = dir.join("site.dxf");
    std::fs::write(dir.join("site.dwl"), b"lock").unwrap();
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("open in another program"), "{e}");
    std::fs::remove_file(dir.join("site.dwl")).unwrap();

    // Changed on disk while editing: the save is refused, the edit stays, Discard ends it.
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    let mut other = read(&file);
    line(&mut other, "0", (0.0, 50.0), (1.0, 50.0));
    std::thread::sleep(std::time::Duration::from_millis(20));
    save(&other, &file);
    let e = s.execute("refclose", &json!({ "option": "save" })).unwrap_err().to_string();
    assert!(e.contains("changed since"), "{e}");
    assert!(s.doc().unwrap().refedit.is_some(), "the edit stays open");
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
    assert_eq!(read(&file).model.len(), 3, "the other program's change is kept");

    // Changed since it was loaded: reload first.
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("since") && e.contains("reload"), "{e}");
    s.execute("-xref", &json!({ "option": "reload", "names": "site" })).unwrap();

    // Read-only: refused.
    let mut perm = std::fs::metadata(&file).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&file, perm.clone()).unwrap();
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("read-only"), "{e}");
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&file, perm).unwrap();

    // Open in another drawing tab of this session: refused.
    s.execute("qsave", &json!({})).unwrap();
    let host = s.state().unwrap().path.clone().unwrap();
    s.execute("open", &json!({ "path": file.to_string_lossy() })).unwrap();
    s.execute("open", &json!({ "path": host })).unwrap();
    let ih = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Insert(_))).unwrap().handle;
    let e = s.execute("refedit", &json!({ "handle": ih.hex() })).unwrap_err().to_string();
    assert!(e.contains("another drawing tab"), "{e}");
}

#[test]
fn nested_block_references_are_picked_through_their_parents() {
    let mut s = Session::new();
    let c = hx(s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Inner", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();
    let i = hx(s.execute("insert", &json!({ "name": "Inner", "at": [10, 0] })).unwrap());
    let l = hx(s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap());
    s.execute("block", &json!({ "name": "Outer", "base": [0, 0], "handles": [i, l], "keep": "delete" })).unwrap();
    let o = hx(s.execute("insert", &json!({ "name": "Outer", "at": [100, 100] })).unwrap());

    // At the command line: pick, Next to the nested level, OK.
    s.start("refedit").unwrap();
    s.input(Input::Pick(vec![Handle::parse_hex(&o).unwrap()])).unwrap();
    assert!(s.prompt_text().contains("Select nesting level"), "{}", s.prompt_text());
    assert!(s.log.iter().any(|m| m.contains("Reference name: Outer")));
    s.input(Input::Enter).unwrap(); // Next
    assert!(s.log.iter().any(|m| m.contains("Reference name: Inner")));
    s.cmdline("o").unwrap();
    let d = s.doc().unwrap();
    let r = d.refedit.clone().unwrap();
    assert_eq!(r.block, "Inner");
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.ghosts.len(), 1, "Outer's line stands faded");
    let EntityKind::Circle(circle) = &d.entity(r.items[0].copy).unwrap().kind else { panic!() };
    assert!(circle.center.xy().near(Vec2::new(110.0, 100.0), 1e-9));
    // Only the working set can change: Outer's faded line is locked.
    s.execute("move", &json!({ "handles": [r.items[0].copy.hex()], "delta": [0, 3] })).unwrap();
    s.execute("refclose", &json!({ "option": "save" })).unwrap();
    let d = s.doc().unwrap();
    let EntityKind::Circle(c) = &d.block("Inner").unwrap().entities.iter().next().unwrap().kind else { panic!() };
    assert!(c.center.xy().near(Vec2::new(0.0, 3.0), 1e-9), "{c:?}");
    assert_eq!(d.block("Outer").unwrap().entities.len(), 2, "the parent is untouched");
    // Bad paths are refused.
    assert!(s.execute("refedit", &json!({ "handle": o, "path": ["FFFFF"] })).is_err());
    assert!(s.execute("refedit", &json!({ "handle": o, "path": "x" })).is_err());
    assert!(s.execute("refedit", &json!({ "handle": o, "nested": "Nope" })).is_err());
    let _ = Vec2::ZERO;
}

/// A symbolic link at `link` to `target`; `None` where the system doesn't let a test make one
/// (Windows without the privilege), and the test is skipped there. CI runs it on Linux.
fn symlink(target: &Path, link: &Path) -> Option<()> {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    #[cfg(not(any(unix, windows)))]
    let made: std::io::Result<()> = Err(std::io::Error::other("no symbolic links"));
    match made {
        Ok(()) => Some(()),
        Err(e) => {
            eprintln!("skipped: symbolic links can't be made here ({e})");
            None
        }
    }
}

fn is_plain_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file())
}

/// Review of #2355: a link planted at the temporary or backup path beside the xref must not take
/// the write (CWE-59). Before, the new bytes went into `victim.txt` through `site.dxf.cadcraft-tmp`
/// and `site.dxf` itself became that link.
#[test]
fn links_planted_beside_the_xref_are_not_written_through() {
    let (mut s, dir, _, insert) = attached("planted");
    let file = dir.join("site.dxf");
    let (victim, other) = (dir.join("victim.txt"), dir.join("other.txt"));
    std::fs::write(&victim, b"victim").unwrap();
    std::fs::write(&other, b"other").unwrap();
    if symlink(&victim, &dir.join("site.dxf.cadcraft-tmp")).is_none() {
        return;
    }
    symlink(&other, &dir.join("site.dxf.bak")).unwrap();
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    s.execute("refclose", &json!({ "option": "save" })).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"victim", "nothing is written through the planted link");
    assert_eq!(std::fs::read(&other).unwrap(), b"other", "nor through a link at the backup");
    assert!(is_plain_file(&file), "the xref's file is still a file, not a link");
    assert_eq!(read(&file).model.len(), 3, "and holds the edit");
    assert!(is_plain_file(&dir.join("site.dxf.bak")), "the link at the backup is replaced by the backup");
    assert_eq!(read(&dir.join("site.dxf.bak")).model.len(), 2);
}

/// Review of #2355: the xref's own path turned into a link to another file during the edit (here
/// a byte-identical copy with the same time, so the change check can't tell) is not written.
#[test]
fn an_xref_path_turned_into_a_link_during_the_edit_is_not_written() {
    let (mut s, dir, _, insert) = attached("swapped");
    let file = dir.join("site.dxf");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let target = elsewhere.join("victim.dxf");
    if symlink(&file, &dir.join("probe")).is_none() {
        return;
    }
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::fs::copy(&file, &target).unwrap();
    std::fs::OpenOptions::new().write(true).open(&target).unwrap().set_modified(modified).unwrap();
    std::fs::rename(&file, dir.join("moved.dxf")).unwrap();
    symlink(&target, &file).unwrap();
    let before = std::fs::read(&target).unwrap();
    let e = s.execute("refclose", &json!({ "option": "save" })).unwrap_err().to_string();
    assert!(e.contains("leads to another file"), "{e}");
    assert_eq!(std::fs::read(&target).unwrap(), before, "the link's target is untouched");
    assert!(!elsewhere.join("victim.dxf.bak").exists() && !dir.join("site.dxf.bak").exists(), "no backup either");
    assert!(s.doc().unwrap().refedit.is_some(), "the edit stays open");
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
}

/// Review of #2355: `site.dxf` and `site.dwg` in one folder both used to back up to `site.bak`,
/// so editing one and then the other lost the first one's old copy. They keep their own now, named
/// as SAVE names them (`site.dxf.bak`, `site.bak`).
#[test]
fn xrefs_of_one_name_in_one_folder_keep_their_own_backups() {
    // These files are our own: DWG is read and written in this process.
    cadcraft_io::dwg_worker::allow_in_process();
    hooks();
    let dir = folder("two-backups");
    let (d, _) = site();
    let (dxf, dwg) = (dir.join("site.dxf"), dir.join("site.dwg"));
    save(&d, &dxf);
    save(&d, &dwg);
    let (old_dxf, old_dwg) = (std::fs::read(&dxf).unwrap(), std::fs::read(&dwg).unwrap());
    let host = dir.join("host.dxf");
    save(&Drawing::new_metric(), &host);
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_string_lossy() })).unwrap();
    for (path, name, x) in [("site.dxf", "a", 0), ("site.dwg", "b", 100)] {
        let r = s.execute("xattach", &json!({ "path": path, "name": name, "at": [x, 0] })).unwrap();
        s.execute("refedit", &json!({ "handle": r["handle"] })).unwrap();
        s.execute("line", &json!({ "points": [[x, 20], [x + 10, 20]] })).unwrap();
        s.execute("refclose", &json!({ "option": "save" })).unwrap();
    }
    assert_eq!(std::fs::read(dir.join("site.dxf.bak")).unwrap(), old_dxf, "the DXF's previous file");
    assert_eq!(std::fs::read(dir.join("site.bak")).unwrap(), old_dwg, "the DWG's previous file");
    assert_eq!((read(&dxf).model.len(), read(&dwg).model.len()), (3, 3), "both hold their edit");
}

/// A host drawing (host.dxf in `dir`) with `file` of `dir` attached as xref `name` at (100, 0).
fn host_with(dir: &Path, file: &str, name: &str) -> (Session, String) {
    let host = dir.join("host.dxf");
    save(&Drawing::new_metric(), &host);
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_string_lossy() })).unwrap();
    let r = s.execute("xattach", &json!({ "path": file, "name": name, "at": [100, 0] })).unwrap();
    (s, r["handle"].as_str().unwrap().to_string())
}

/// The `$ACADVER` an ASCII DXF declares.
fn dxf_version(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.windows(3).find(|w| w[0] == "$ACADVER" && w[1] == "1").map(|w| w[2].to_string())
}

/// Review of #2355: the xref was saved in the writer's default version (an R2000 DWG came back
/// as 2004, AC1018). It is written back in the version, and form, it was read in.
#[test]
fn an_xref_is_written_back_in_the_version_it_was_read_in() {
    // These files are our own: DWG is read and written in this process.
    cadcraft_io::dwg_worker::allow_in_process();
    hooks();
    let dir = folder("versions");
    let (d, _) = site();
    let write = |name: &str, version: Version, binary: bool| {
        let p = dir.join(name);
        let opts = cadcraft_io::SaveOptions { version: Some(version), binary };
        std::fs::write(&p, cadcraft_io::write_with(&d, &p, &opts).unwrap()).unwrap();
        p
    };
    let dwg = write("old.dwg", Version::R2000, false);
    let dxf = write("old.dxf", Version::R2004, false);
    let bin = write("bin.dxf", Version::R2010, true);
    assert_eq!((&std::fs::read(&dwg).unwrap()[..6], dxf_version(&std::fs::read(&dxf).unwrap()).as_deref()), (&b"AC1015"[..], Some("AC1018")));
    for (file, name) in [("old.dwg", "a"), ("old.dxf", "b"), ("bin.dxf", "c")] {
        let (mut s, insert) = host_with(&dir, file, name);
        let r = s.execute("refedit", &json!({ "handle": insert })).unwrap();
        assert_eq!(r["xref"]["binary"], file == "bin.dxf", "{r}");
        s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
        s.execute("refclose", &json!({ "option": "save" })).unwrap();
    }
    let (dwg_bytes, dxf_bytes, bin_bytes) = (std::fs::read(&dwg).unwrap(), std::fs::read(&dxf).unwrap(), std::fs::read(&bin).unwrap());
    assert_eq!(&dwg_bytes[..6], b"AC1015", "the R2000 DWG stays R2000");
    assert_eq!(dxf_version(&dxf_bytes).as_deref(), Some("AC1018"), "the 2004 DXF stays 2004");
    assert!(bin_bytes.starts_with(b"AutoCAD Binary DXF"), "the binary DXF stays binary");
    assert_eq!(read(&bin).header.get("ACADVER").and_then(cadcraft_engine::doc::HVal::as_str), Some("AC1024"), "and 2010");
    assert_eq!((read(&dwg).model.len(), read(&dxf).model.len(), read(&bin).model.len()), (3, 3, 3), "each holds its edit");
}

/// Review of #2355: an R12 DXF xref came back as an R2000 one. CADCraft doesn't write R12, so the
/// edit is refused before it begins (no work lost), and the file is left as it is.
#[test]
fn an_xref_in_a_version_cadcraft_cannot_write_is_not_edited_in_place() {
    hooks();
    let dir = folder("r12");
    let r12 = "0
SECTION
2
HEADER
9
$ACADVER
1
AC1009
0
ENDSEC
0
SECTION
2
ENTITIES
0
LINE
8
0
10
0
20
0
11
1
21
1
0
ENDSEC
0
EOF
";
    std::fs::write(dir.join("r12.dxf"), r12).unwrap();
    let (mut s, insert) = host_with(&dir, "r12.dxf", "old");
    let e = s.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("AC1009") && e.contains("XOPEN"), "{e}");
    assert!(s.doc().unwrap().refedit.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("r12.dxf")).unwrap(), r12, "the file is untouched");
}

/// Review of #2355: SAVE says what a file can't keep; the write-back said nothing. A 2013 DXF xref
/// holding a solid whose ACIS geometry is SAT text (no data store record) loses it when written:
/// the save says so, names it in `notSaved`, and points at the backup that still has it.
#[test]
fn what_the_xref_file_cannot_keep_is_reported() {
    hooks();
    let dir = folder("lost");
    let dxf = "0
SECTION
2
HEADER
9
$ACADVER
1
AC1027
0
ENDSEC
0
SECTION
2
ENTITIES
               0
LINE
5
20
8
0
10
0
20
0
11
1
21
1
               0
3DSOLID
5
21
100
AcDbEntity
8
0
100
AcDbModelerGeometry
70
1
1
400 0 1 0
               0
ENDSEC
0
EOF
";
    std::fs::write(dir.join("solid.dxf"), dxf).unwrap();
    let (mut s, insert) = host_with(&dir, "solid.dxf", "solid");
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    let out = s.execute("refclose", &json!({ "option": "save" })).unwrap();
    assert_eq!(out["notSaved"], json!({ "3DSOLID": 1 }), "{out}");
    let warning = s.log.iter().find(|m| m.contains("Not saved in the DXF file")).expect("a warning on the command line");
    assert!(warning.contains("1 3DSOLID") && warning.contains("solid.dxf.bak"), "{warning}");
    assert!(std::fs::read_to_string(dir.join("solid.dxf.bak")).unwrap().contains("3DSOLID"), "the backup still has it");
}

/// Review of #2355: a reference nested with different X and Y scales was offered as a level, and
/// OK then refused it. It isn't offered (nor anything below it); a uniformly scaled one is.
#[test]
fn nested_references_refedit_cannot_open_are_not_offered() {
    let mut s = Session::new();
    let c = hx(s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Leaf", "base": [0, 0], "handles": [c], "keep": "delete" })).unwrap();
    let uniform = hx(s.execute("insert", &json!({ "name": "Leaf", "at": [10, 0], "scale": 2 })).unwrap());
    let stretched = hx(s.execute("insert", &json!({ "name": "Leaf", "at": [20, 0], "scale": [2, 1] })).unwrap());
    s.execute("block", &json!({ "name": "Holder", "base": [0, 0], "handles": [uniform, stretched], "keep": "delete" })).unwrap();
    let o = hx(s.execute("insert", &json!({ "name": "Holder", "at": [0, 0] })).unwrap());
    let levels = s.execute("refedit.levels", &json!({ "handle": o })).unwrap();
    let names: Vec<&str> = levels["levels"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Holder", "Leaf"], "{levels}");
    // The one offered opens.
    let path = levels["levels"][1]["path"].clone();
    s.execute("refedit", &json!({ "handle": o, "path": path })).unwrap();
}

/// Review of #2355: the faded copies carried XFADECTL in their own transparency, so lowering
/// XFADECTL during the edit left them faded. They keep the object's own transparency now; the
/// fade is drawn from the current XFADECTL.
#[test]
fn faded_copies_keep_their_own_transparency() {
    let mut s = Session::new();
    let a = hx(s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap());
    let b = hx(s.execute("circle", &json!({ "center": [0, 0], "radius": 1 })).unwrap());
    s.execute("block", &json!({ "name": "Pair", "base": [0, 0], "handles": [a, b], "keep": "delete" })).unwrap();
    let i = hx(s.execute("insert", &json!({ "name": "Pair", "at": [0, 0] })).unwrap());
    let d = s.doc().unwrap();
    let blk = d.block("Pair").unwrap();
    let line = blk.entities.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
    let own = blk.entities.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().common.transparency;
    s.execute("refedit", &json!({ "handle": i, "objects": [line.hex()] })).unwrap();
    let d = s.doc().unwrap();
    let r = d.refedit.clone().unwrap();
    let ghost = d.entity(r.ghosts[0].0).unwrap();
    assert_eq!(ghost.common.transparency, own, "the circle's copy is faded by the renderer, not by its own transparency");
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();
}

/// Review of #2355: the xref's file opened in another tab after the edit began was overwritten
/// by REFCLOSE Save; the save is refused while it is open there.
#[test]
fn an_xref_opened_in_another_tab_during_the_edit_is_not_written() {
    let (mut s, dir, _, insert) = attached("tab-later");
    let file = dir.join("site.dxf");
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    let host = s.active;
    s.execute("open", &json!({ "path": file.to_string_lossy() })).unwrap();
    s.execute("document.switch", &json!({ "index": host })).unwrap();
    let before = std::fs::read(&file).unwrap();
    let e = s.execute("refclose", &json!({ "option": "save" })).unwrap_err().to_string();
    assert!(e.contains("another drawing tab"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "the file is untouched");
    assert!(s.doc().unwrap().refedit.is_some(), "the edit stays open");
}

/// Review of #2355: the "loaded" stamp was one per file for the whole process, so a second drawing
/// loading the xref later moved the first drawing's baseline. Here the file changes (same objects,
/// moved) after the first drawing loaded it: that drawing still shows the old geometry, and REFEDIT
/// there must refuse rather than pair the old geometry with the new file.
#[test]
fn each_drawing_keeps_its_own_xref_baseline() {
    let (mut first, dir, _, insert) = attached("baseline");
    let file = dir.join("site.dxf");
    let mut moved = read(&file);
    let h = moved.model.iter().find(|e| matches!(e.kind, EntityKind::Line(_))).unwrap().handle;
    moved
        .modify_entity(h, |e| {
            if let EntityKind::Line(l) = &mut e.kind {
                l.b.y += 1.0;
            }
        })
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    save(&moved, &file);
    // Another drawing loads the changed file.
    let other_host = dir.join("other.dxf");
    save(&Drawing::new_metric(), &other_host);
    let mut second = Session::new();
    second.execute("open", &json!({ "path": other_host.to_string_lossy() })).unwrap();
    let r = second.execute("xattach", &json!({ "path": "site.dxf", "at": [0, 0] })).unwrap();
    second.execute("refedit", &json!({ "handle": r["handle"] })).unwrap();
    second.execute("refclose", &json!({ "option": "discard" })).unwrap();
    // The first drawing loaded the earlier file.
    let e = first.execute("refedit", &json!({ "handle": insert })).unwrap_err().to_string();
    assert!(e.contains("changed since") && e.contains("reload"), "{e}");
}

/// Review of #2355: multileader and table styles of the host that objects drawn during the edit
/// use were not copied into the xref's file, so they lost their style there.
#[test]
fn host_multileader_and_table_styles_come_along() {
    let (mut s, dir, _, insert) = attached("styles");
    s.execute("mleaderstyle", &json!({ "name": "Callout", "textHeight": 7, "current": true })).unwrap();
    s.execute("tablestyle", &json!({ "name": "Schedule", "textHeight": 4, "current": true })).unwrap();
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    s.execute("mleader", &json!({ "points": [[100, 30], [110, 40]], "text": "A" })).unwrap();
    s.execute("table", &json!({ "at": [120, 30], "rows": 1, "cols": 1 })).unwrap();
    s.execute("refclose", &json!({ "option": "save" })).unwrap();
    let back = read(&dir.join("site.dxf"));
    let mleader = back.model.iter().find_map(|e| if let EntityKind::MLeader(m) = &e.kind { Some(m.style.clone()) } else { None }).unwrap();
    let table = back.model.iter().find_map(|e| if let EntityKind::Table(t) = &e.kind { Some(t.style.clone()) } else { None }).unwrap();
    assert_eq!((mleader.as_str(), table.as_str()), ("Callout", "Schedule"));
    let ml = back.mleader_styles.iter().find(|t| t.name == "Callout").expect("the multileader style is in the file");
    assert!((ml.text_height - 7.0).abs() < 1e-9);
    let ts = back.table_styles.iter().find(|t| t.name == "Schedule").expect("the table style is in the file");
    assert!((ts.text_height - 4.0).abs() < 1e-9);
}

/// Automation roots (#1472): an agent confined to its folders can't have REFCLOSE write an xref, or
/// the xref's `.bak`, outside its write root, and can't start an edit in place of one it may not
/// write. The refusals leave the xref, its backup and the host as they were.
#[test]
fn a_confined_session_does_not_write_an_xref_outside_its_write_root() {
    use cadcraft_engine::file_access::{AutomationRoots, confine};
    let (mut s, dir, _, insert) = attached("confined");
    let file = dir.join("site.dxf");
    let before = std::fs::read(&file).unwrap();
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    // The xref's folder may be read but not written.
    let roots = AutomationRoots::new(Some(&dir), Some(&out)).unwrap();

    // REFEDIT of an xref it couldn't save is refused before the edit begins.
    let e = confine(roots.as_ref(), || s.execute("refedit", &json!({ "handle": insert }))).unwrap_err().to_string();
    assert!(e.contains("outside") || e.contains("write"), "{e}");
    assert!(s.doc().unwrap().refedit.is_none(), "no edit began");

    // An edit begun without the roots isn't written once they are in force.
    s.execute("refedit", &json!({ "handle": insert })).unwrap();
    s.execute("line", &json!({ "points": [[100, 20], [110, 20]] })).unwrap();
    let e = confine(roots.as_ref(), || s.execute("refclose", &json!({ "option": "save" }))).unwrap_err().to_string();
    assert!(e.contains("outside") || e.contains("write"), "{e}");
    assert_eq!(std::fs::read(&file).unwrap(), before, "the xref is unchanged");
    assert!(std::fs::symlink_metadata(dir.join("site.dxf.bak")).is_err(), "no backup was written");
    assert!(s.doc().unwrap().refedit.is_some(), "the edit stays open");
    assert_eq!(s.doc().unwrap().block("site").unwrap().entities.len(), 2, "the host still shows the xref as it was");
    s.execute("refclose", &json!({ "option": "discard" })).unwrap();

    // Roots that don't reach the xref's folder: it isn't read for an edit either.
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(elsewhere.join("out")).unwrap();
    let roots = AutomationRoots::new(Some(&elsewhere), Some(&elsewhere.join("out"))).unwrap();
    let e = confine(roots.as_ref(), || s.execute("refedit", &json!({ "handle": insert }))).unwrap_err().to_string();
    assert!(s.doc().unwrap().refedit.is_none(), "no edit began: {e}");
    assert_eq!(std::fs::read(&file).unwrap(), before);
}
