//! Autosave: SAVETIME timing, which drawings are written, files deleted on save and close, and
//! the files Drawing Recovery offers after a crash (#406, docs/gaps.md F4).

use std::time::{Duration, SystemTime};

use cadcraft_engine::Session;
use cadcraft_engine::autosave::{self, Autosaver, Job};
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use serde_json::json;

fn writes(jobs: &[Job]) -> Vec<String> {
    jobs.iter()
        .filter_map(|j| match j {
            Job::Write { path, .. } => Some(path.clone()),
            Job::Delete { .. } => None,
        })
        .collect()
}

fn deletes(jobs: &[Job]) -> Vec<String> {
    jobs.iter()
        .filter_map(|j| match j {
            Job::Delete { path } => Some(path.clone()),
            Job::Write { .. } => None,
        })
        .collect()
}

fn draw(s: &mut Session) {
    s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
}

#[test]
fn a_changed_drawing_is_written_savetime_minutes_after_the_change() {
    let mut s = Session::new();
    let uid = s.state().unwrap().uid;
    let mut a = Autosaver::new("77-a");
    assert!(a.plan(&s, 0.0, 10, "/auto").is_empty(), "nothing to save in a clean drawing");

    draw(&mut s);
    assert!(a.plan(&s, 30.0, 10, "/auto").is_empty(), "the change starts the clock");
    assert_eq!(a.next_due(100.0, 10), Some(530.0));
    assert!(a.plan(&s, 629.0, 10, "/auto").is_empty());
    let jobs = a.plan(&s, 630.0, 10, "/auto");
    let path = std::path::Path::new("/auto").join(format!("Drawing1_77-a_{uid}.sv$")).to_string_lossy().to_string();
    assert_eq!(writes(&jobs), vec![path.clone()]);
    assert!(matches!(&jobs[0], Job::Write { doc, .. } if std::sync::Arc::ptr_eq(doc, &s.state().unwrap().doc)));
    assert!(a.plan(&s, 5000.0, 10, "/auto").is_empty(), "no change since the autosave");
    assert_eq!(a.next_due(5000.0, 10), None);

    // The next change restarts the clock; the same file is written again.
    draw(&mut s);
    assert!(a.plan(&s, 6000.0, 10, "/auto").is_empty());
    assert_eq!(writes(&a.plan(&s, 6600.0, 10, "/auto")), vec![path.clone()]);
    assert_eq!(a.files(), vec![path.clone()]);

    // Saving (nothing unsaved left) deletes the file.
    let st = s.state_mut().unwrap();
    st.saved = st.doc.clone();
    assert_eq!(deletes(&a.plan(&s, 6601.0, 10, "/auto")), vec![path]);
    assert!(a.files().is_empty());
}

#[test]
fn closing_deletes_and_savetime_zero_or_no_folder_writes_nothing() {
    let mut s = Session::new();
    s.execute("new", &json!({})).unwrap();
    let mut a = Autosaver::new("5-b");
    draw(&mut s);
    a.plan(&s, 0.0, 1, "/auto");
    assert!(a.plan(&s, 1e6, 0, "/auto").is_empty(), "SAVETIME 0 is off");
    assert!(a.plan(&s, 1e6, 1, " ").is_empty(), "no folder, no autosave");
    let path = writes(&a.plan(&s, 1e6, 1, "/auto")).pop().unwrap();
    assert!(path.contains("Drawing2_5-b_"), "{path}");

    // Undo back to the saved state: no unsaved changes, no file.
    s.undo().unwrap();
    assert_eq!(deletes(&a.plan(&s, 1e6 + 1.0, 1, "/auto")), vec![path.clone()]);

    draw(&mut s);
    a.plan(&s, 0.0, 1, "/auto");
    assert_eq!(writes(&a.plan(&s, 60.0, 1, "/auto")).len(), 1);
    s.execute("close", &json!({})).unwrap();
    assert_eq!(deletes(&a.plan(&s, 61.0, 1, "/auto")).len(), 1, "closing deletes the file");
}

#[test]
fn file_names_round_trip() {
    let n = autosave::file_name("My plan_v2.dwg", "123-abc", 7);
    assert_eq!(n, "My plan_v2.dwg_123-abc_7.sv$");
    assert_eq!(autosave::parse_file_name(&n), Some(("My plan_v2.dwg", "123-abc", 7)));
    assert_eq!(autosave::file_name("a/b:c*", "1-2", 3), "a-b-c-_1-2_3.sv$");
    assert_eq!(autosave::file_name("..", "1-2", 3), "Drawing_1-2_3.sv$");
    for bad in
        ["plan.dxf", "x_1.sv$", "_1-2_3.sv$", "a_1-2_x.sv$", "x.sv$", "notes_foo_3.sv$", "a_1-xyz_3.sv$", "a_-1_3.sv$", "a_1-2_+3.sv$", "a_1-2_.sv$"]
    {
        assert_eq!(autosave::parse_file_name(bad), None, "{bad}");
    }
    assert!(autosave::is_instance_id(&autosave::instance_id()));
    assert_eq!(autosave::parse_lock_name("123-18f3a2b4c5d.lock"), Some("123-18f3a2b4c5d"));
    for bad in ["Cargo.lock", "foo.lock", "package-lock.json", "yarn-lock.lock", "1-2.lock.bak", ".lock", "-1.lock", "1-.lock", "1-2-3.lock"] {
        assert_eq!(autosave::parse_lock_name(bad), None, "{bad}");
    }
}

#[test]
fn files_of_other_programs_in_the_autosave_folder_are_never_deleted() {
    // SAVEFILEPATH may point at a project folder: only CADCraft's own lock and autosave names
    // (built from an instance id) are ever pruned or discarded.
    let dir = temp_dir("foreign");
    let old = SystemTime::now() - Duration::from_secs(3600);
    let foreign = ["Cargo.lock", "package-lock.json", "foo.lock", "yarn-lock.lock", "x.sv$", "notes_foo_3.sv$", "a_b-c_1.sv$"];
    for name in foreign.iter().chain(&["1-a.lock"]) {
        let f = std::fs::File::create(dir.join(name)).unwrap();
        f.set_modified(old).unwrap();
    }

    assert!(autosave::recoverable(&dir, "9-f").is_empty(), "foreign files are not offered");
    autosave::prune_locks(&dir);
    assert!(!dir.join("1-a.lock").exists(), "a stopped app's own lock is pruned");
    for name in foreign {
        autosave::discard(&dir.join(name).to_string_lossy());
    }
    autosave::prune_locks(&dir);
    for name in foreign {
        assert!(dir.join(name).exists(), "{name} must survive");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cadcraft-autosave-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn recovery_offers_files_of_apps_that_stopped() {
    let dir = temp_dir("scan");
    let touch = |name: &str| std::fs::write(dir.join(name), b"0\nEOF\n").unwrap();
    touch("Crashed.dxf_1-a_3.sv$"); // no lock: the app went down
    touch("Live.dxf_2-b_4.sv$"); // fresh lock: still running
    std::fs::write(dir.join("2-b.lock"), b"2").unwrap();
    touch("Stale.dxf_3-c_5.sv$"); // lock not refreshed for 10 minutes
    let lock = std::fs::File::create(dir.join("3-c.lock")).unwrap();
    lock.set_modified(SystemTime::now() - Duration::from_secs(600)).unwrap();
    drop(lock);
    touch("Mine.dxf_9-f_1.sv$"); // this app's own
    touch("notes.txt");
    std::fs::write(dir.join("Half.dxf_4-d_6.sv$.tmp"), b"").unwrap();

    let found = autosave::recoverable(&dir, "9-f");
    let names: Vec<&str> = found.iter().map(|r| r.drawing.as_str()).collect();
    assert_eq!(names.len(), 2, "{found:?}");
    assert!(names.contains(&"Crashed.dxf") && names.contains(&"Stale.dxf"));
    assert!(found.iter().all(|r| r.bytes == 6 && r.modified > 0));

    // Discarding deletes the file and the dead app's lock once it has no files left.
    let stale = found.iter().find(|r| r.drawing == "Stale.dxf").unwrap();
    autosave::discard(&stale.path);
    assert!(!std::path::Path::new(&stale.path).exists());
    assert!(!dir.join("3-c.lock").exists());
    assert!(dir.join("2-b.lock").exists(), "a running app's lock stays");
    assert_eq!(autosave::recoverable(&dir, "9-f").len(), 1);
    assert!(autosave::recoverable(&dir.join("missing"), "9-f").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn autosave_files_are_written_in_the_background_and_removed_on_exit() {
    hooks();
    let dir = temp_dir("write");
    let mut s = Session::new();
    draw(&mut s);
    let mut a = Autosaver::new(autosave::instance_id());
    a.plan(&s, 0.0, 1, &dir.to_string_lossy());
    let jobs = a.plan(&s, 60.0, 1, &dir.to_string_lossy());
    let path = writes(&jobs).pop().unwrap();
    a.perform(jobs);
    for _ in 0..500 {
        if std::path::Path::new(&path).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(std::fs::read(&path).unwrap().ends_with(b"EOF\n"));
    // The file exists before the write finishes (the folder sync comes after the rename); until
    // it has, the drawing is busy and the change drawn below would not be planned.
    for _ in 0..500 {
        if !a.writing() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // While the app runs, its lock keeps its files out of recovery. SAVETIME and SAVEFILEPATH
    // are system variables (process-wide, so set in this test only).
    assert_eq!(cadcraft_engine::sysvars::get(&s, "SAVETIME"), Some(json!(10)));
    s.execute("setvar", &json!({"name": "SAVETIME", "value": 3})).unwrap();
    assert_eq!(autosave::savetime(), 3);
    assert!(s.execute("setvar", &json!({"name": "SAVETIME", "value": -1})).is_err());
    assert!(s.execute("setvar", &json!({"name": "SAVETIME", "value": 601})).is_err());
    s.execute("setvar", &json!({"name": "SAVEFILEPATH", "value": format!(" {} ", dir.display())})).unwrap();
    assert_eq!(cadcraft_engine::sysvars::get(&s, "SAVEFILEPATH"), Some(json!(dir.to_string_lossy())));
    a.tick(&s);
    assert!(dir.join(format!("{}.lock", a.instance)).exists());
    assert!(autosave::recoverable(&dir, "someone-else").is_empty());

    // After a GPU error a change made a moment ago is autosaved right away, not SAVETIME later.
    assert!(a.tick(&s).is_none(), "no change waiting");
    draw(&mut s);
    assert!(a.tick(&s).is_some(), "SAVETIME (3 minutes) is not up yet");
    assert_eq!(a.save_now(&s), 1);
    assert_eq!(a.save_now(&s), 0, "being written or written: nothing more to save");

    a.shutdown();
    assert!(!std::path::Path::new(&path).exists());
    assert!(!dir.join(format!("{}.lock", a.instance)).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Autosave files are unsaved drawings: on Unix an autosave folder that doesn't exist yet is made
/// owner-only, and one that already exists (SAVEFILEPATH can be any folder) keeps its mode.
#[cfg(unix)]
#[test]
fn a_new_autosave_folder_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    hooks();
    let base = temp_dir("modes");
    let picked = base.join("picked");
    std::fs::create_dir_all(&picked).unwrap();
    std::fs::set_permissions(&picked, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (dir, expected) in [(base.join("new").join("autosave"), 0o700), (picked, 0o755)] {
        let mut s = Session::new();
        draw(&mut s);
        // Not instance_id(): a test started in the same millisecond would get the same id, and
        // shutdown() would then remove its lock from the process-wide SAVEFILEPATH.
        let mut a = Autosaver::new("41-c");
        a.plan(&s, 0.0, 1, &dir.to_string_lossy());
        let jobs = a.plan(&s, 60.0, 1, &dir.to_string_lossy());
        let path = writes(&jobs).pop().unwrap();
        a.perform(jobs);
        for _ in 0..500 {
            if std::path::Path::new(&path).exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(std::path::Path::new(&path).exists());
        assert_eq!(mode(&dir), expected, "{}", dir.display());
        a.shutdown();
    }
    assert_eq!(mode(&base.join("new")), 0o700);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_failed_autosave_is_tried_again_without_another_change() {
    hooks();
    let root = temp_dir("retry");
    // The folder can't be created while a file holds its name: the write fails (#717).
    let out = root.join("out");
    std::fs::write(&out, b"not a folder").unwrap();
    let dir = out.to_string_lossy().to_string();
    let mut s = Session::new();
    draw(&mut s);
    let mut a = Autosaver::new("31-e");
    a.plan(&s, 0.0, 1, &dir);
    let jobs = a.plan(&s, 60.0, 1, &dir);
    let path = writes(&jobs).pop().unwrap();
    a.perform(jobs);

    // Once the write has failed, the unchanged drawing waits SAVETIME again.
    let mut rearmed = false;
    for _ in 0..500 {
        a.reap();
        assert!(writes(&a.plan(&s, 120.0, 1, &dir)).is_empty());
        if a.next_due(120.0, 1) == Some(60.0) {
            rearmed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(rearmed, "a failed autosave must be tried again");

    std::fs::remove_file(&out).unwrap();
    let jobs = a.plan(&s, 180.0, 1, &dir);
    assert_eq!(writes(&jobs), vec![path.clone()]);
    a.perform(jobs);
    for _ in 0..500 {
        if std::path::Path::new(&path).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(std::fs::read(&path).unwrap().ends_with(b"EOF\n"));

    // A write that succeeded is not repeated.
    for _ in 0..50 {
        a.reap();
        assert!(writes(&a.plan(&s, 1e4, 1, &dir)).is_empty());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(a.next_due(1e4, 1), None);
    a.shutdown();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn recover_as_copy_opens_an_unsaved_untitled_drawing() {
    hooks();
    let mut s = Session::new();
    let dir = temp_dir("copy");
    let file = dir.join("Plan.dxf_1-a_2.sv$");
    std::fs::write(&file, b"0\nEOF\n").unwrap();
    let r = s.execute("recover", &json!({"path": file.to_string_lossy(), "copy": true, "title": "Plan.dxf"})).unwrap();
    let st = s.state().unwrap();
    assert_eq!((st.title.as_str(), st.path.as_deref()), ("Plan.dxf", None));
    assert!(st.is_dirty(), "a recovered copy has unsaved changes");
    assert_eq!(r["uid"], st.uid);
    let _ = std::fs::remove_dir_all(&dir);
}

/// File hooks for every test in this file (the first call installs them).
fn hooks() {
    set_io(IoHooks {
        read: |_, _| Ok(cadcraft_engine::doc::Drawing::new_metric()),
        write: |_, _| Ok(b"0\nSECTION\n0\nENDSEC\n0\nEOF\n".to_vec()),
        plot: None,
    });
}

#[test]
fn a_block_editor_that_cannot_be_composed_is_not_autosaved_as_the_old_host() {
    use cadcraft_engine::doc::{Entity, EntityKind, Insert};
    use cadcraft_engine::geom::Vec3;

    let mut s = Session::new();
    let l = s.execute("line", &json!({ "points": [[10, 10], [12, 10]] })).unwrap()["handles"][0].clone();
    s.execute("block", &json!({ "name": "Door", "base": [10, 10], "handles": [l] })).unwrap();
    s.execute("bedit", &json!({ "name": "Door" })).unwrap();
    // An unsaved change in the editor: the block would contain a reference to itself.
    let d = s.doc_mut().unwrap();
    let h = d.new_handle();
    let kind = EntityKind::Insert(Insert {
        block: "Door".into(),
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    d.model.push(Entity::new(h, kind));
    assert!(s.state().unwrap().recovery_drawing().is_err());

    let mut a = Autosaver::new("55-b");
    a.plan(&s, 0.0, 1, "/auto");
    assert!(a.plan(&s, 60.0, 1, "/auto").is_empty(), "no recovery file without the editor's changes");
    assert!(a.plan(&s, 120.0, 1, "/auto").is_empty());
    assert_eq!(a.next_due(120.0, 1), Some(0.0), "still waiting to be written");

    // Once the reference is gone, the next autosave writes the editor's changes.
    let d = s.doc_mut().unwrap();
    d.model.remove(h);
    assert_eq!(writes(&a.plan(&s, 180.0, 1, "/auto")).len(), 1);
}
