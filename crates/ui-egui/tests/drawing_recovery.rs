//! Drawing Recovery after a crash (#406): autosave files a stopped app left behind are listed;
//! Open makes an unsaved copy whose file stays until it is saved, Discard deletes the file.
//! SAVETIME and SAVEFILEPATH are process-wide, so everything runs in one test.

use cadcraft_engine::Session;
use cadcraft_engine::autosave;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services};

#[test]
fn recovery_list_opens_copies_and_discards_files() {
    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let dir = std::env::temp_dir().join(format!("cadcraft-recovery-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let plan = dir.join("Plan.dxf_1-a_3.sv$");
    let site = dir.join("Site.dwg_1-a_4.sv$");
    for f in [&plan, &site] {
        std::fs::write(f, b"0\nEOF\n").unwrap();
    }
    // A copy of the app that is still running keeps its lock fresh: its files are not offered.
    std::fs::write(dir.join("Other.dxf_2-b_1.sv$"), b"0\nEOF\n").unwrap();
    std::fs::write(dir.join("2-b.lock"), b"2").unwrap();

    let mut app = CadApp::new(Session::empty(), Services::default());
    app.enable_autosave(&dir);
    assert_eq!(autosave::savefilepath(), dir.to_string_lossy());
    let names: Vec<_> = app.recovery.items.iter().map(|r| r.drawing.clone()).collect();
    assert_eq!(names.len(), 2, "{names:?}");

    let i = names.iter().position(|n| n == "Plan.dxf").unwrap();
    app.recover_item(i).unwrap();
    assert_eq!(app.recovery.items.len(), 1);
    let st = app.session.state().unwrap();
    assert_eq!((st.title.as_str(), st.path.as_deref()), ("Plan.dxf", None));
    assert!(st.is_dirty(), "a recovered drawing is an unsaved copy");
    assert!(plan.exists(), "the file stays until the recovered drawing is saved");
    assert!(app.autosave.as_ref().unwrap().files().contains(&plan.to_string_lossy().to_string()));

    app.discard_item(0);
    assert!(app.recovery.items.is_empty());
    assert!(!site.exists(), "Discard deletes the file");
    assert!(app.recover_item(0).is_err());

    // Saving the recovered drawing (no unsaved changes left) deletes its file.
    let st = app.session.state_mut().unwrap();
    st.saved = st.doc.clone();
    app.autosave.as_mut().unwrap().tick(&app.session);
    assert!(!plan.exists());

    // Exiting normally removes this app's files; the running copy's file stays.
    app.shutdown_autosave();
    assert!(dir.join("Other.dxf_2-b_1.sv$").exists());
    let _ = std::fs::remove_dir_all(&dir);

    // SAVETIME and SAVEFILEPATH survive restarts (process-wide, so checked in this one test).
    cadcraft_ui_egui::autosave::load_prefs(r#"{"savetime": 25, "saveFilePath": "/tmp/cc-autosave-prefs"}"#);
    assert_eq!(autosave::savetime(), 25);
    let saved = cadcraft_ui_egui::autosave::prefs_json();
    cadcraft_ui_egui::autosave::load_prefs("not json");
    cadcraft_ui_egui::autosave::load_prefs(r#"{"savetime": "often"}"#);
    assert_eq!(autosave::savetime(), 25, "bad values are ignored");
    cadcraft_ui_egui::autosave::load_prefs(r#"{"savetime": 0}"#);
    assert_eq!(autosave::savetime(), 0);
    cadcraft_ui_egui::autosave::load_prefs(&saved);
    assert_eq!((autosave::savetime(), autosave::savefilepath().as_str()), (25, "/tmp/cc-autosave-prefs"));
}

/// The web build's Drawing Recovery: autosaves live in the browser's storage (here a store in
/// memory, installed for this thread only), and the same list opens and discards them. Nothing
/// here depends on SAVETIME or SAVEFILEPATH, so it runs beside the test above.
#[test]
fn recovery_list_reads_the_browser_store() {
    use cadcraft_engine::autosave::store::{self, MemoryStore};

    set_io(IoHooks { read: |_, _| Ok(Drawing::new_metric()), write: |_, _| Ok(b"0\nEOF\n".to_vec()), plot: None });
    let plan = store::key("Plan.dxf_1-a_3.sv$");
    let site = store::key("Site.dwg_1-a_4.sv$");
    let mut mem = MemoryStore { clock: 10_000.0, ..MemoryStore::default() };
    for k in [&plan, &site] {
        mem.map.insert(k.clone(), store::encode(9_000, b"0\nEOF\n"));
    }
    // A tab still running: its lock is fresh, so its record is not offered.
    mem.map.insert(store::key("Other.dxf_2-b_1.sv$"), store::encode(9_000, b"0\nEOF\n"));
    mem.map.insert(store::lock_key("2-b"), "9990".into());
    store::install(Box::new(mem));

    let mut app = CadApp::new(Session::empty(), Services::default());
    app.enable_store_autosave("3-c".into());
    assert!(app.autosave.as_ref().unwrap().uses_store());
    let names: Vec<_> = app.recovery.items.iter().map(|r| r.drawing.clone()).collect();
    assert_eq!(names, ["Plan.dxf", "Site.dwg"]);

    app.recover_item(0).unwrap();
    let st = app.session.state().unwrap();
    assert_eq!((st.title.as_str(), st.path.as_deref()), ("Plan.dxf", None));
    assert!(st.is_dirty(), "a recovered drawing is an unsaved copy");
    assert!(app.autosave.as_ref().unwrap().files().contains(&plan), "later autosaves overwrite its record");
    let has = |k: &str| store::with(|s| s.get(k).unwrap().is_some()).unwrap();
    assert!(has(&plan), "the record stays until the recovered drawing is saved");

    app.discard_item(0);
    assert!(app.recovery.items.is_empty());
    assert!(!has(&site), "Discard deletes the record");

    // Saving the recovered drawing (no unsaved changes left) deletes its record.
    let st = app.session.state_mut().unwrap();
    st.saved = st.doc.clone();
    app.autosave.as_mut().unwrap().tick_store(&app.session);
    assert!(!has(&plan));
    assert!(has(&store::key("Other.dxf_2-b_1.sv$")), "the running tab's record stays");
    store::uninstall();
}
