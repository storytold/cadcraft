//! The `CloudApp` contract (`craft_ui::cloud`), driven through the real interface against
//! `craft-cloud-local`'s reference backend: a folder on disk, not a mock that merely echoes what
//! it was handed. No concrete provider lives in `cadcraft-ui-egui`, in `apps/cadcraft` or in
//! `apps/cadcraft-web`; this is the backend a host would connect, the same way the tests below
//! do, through `craft_ui::cloud::CloudFiles::connect`.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::Drawing;
use cadcraft_ui_egui::{CadApp, Services, control, menus};
use craft_cloud_local::LocalBackend;
use craft_ui::cloud::{CloudApp, Destination, available, open_entry, save_as, start};
use serde_json::json;

/// Bytes [`set_io`]'s mocked reader accepts; what they decode to doesn't matter to these tests.
const MAGIC: &[u8] = b"0\nSECTION\n2\nHEADER\n";

fn mock_reader() {
    set_io(IoHooks {
        read: |b, _| if b.starts_with(MAGIC) { Ok(Drawing::new_metric()) } else { Err("not a drawing".into()) },
        write: |_, _| Ok(Vec::new()),
        plot: None,
    });
}

/// Drains the cloud worker's answers until `done` holds or ten seconds pass (the reference
/// backend answers on its own thread, like a real provider would).
fn settle(app: &mut CadApp, ctx: &egui::Context, done: impl Fn(&CadApp) -> bool) {
    let t = std::time::Instant::now();
    while !done(app) && t.elapsed() < std::time::Duration::from_secs(10) {
        craft_ui::cloud::pump(app, ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    craft_ui::cloud::pump(app, ctx);
}

#[test]
fn nothing_cloud_shaped_is_offered_without_a_backend() {
    let mut app = CadApp::new(Session::new(), Services::default());
    assert!(!available(&app), "no backend was connected");
    assert!(!menus::ui_command_enabled(&app, "cloud.open"));
    assert!(!menus::ui_command_enabled(&app, "cloud.saveAs"));
    let file_menu = menus::tree(&app).into_iter().find(|(m, _)| m == "File").map(|(_, e)| e).unwrap_or_default();
    let open_item = file_menu.iter().find_map(|e| match e {
        menus::Entry::Item { id, enabled, .. } if id == "cloud.open" => Some(*enabled),
        _ => None,
    });
    assert_eq!(open_item, Some(false), "the File menu lists the command disabled, not left out, with no backend");

    assert_eq!(app.run("cloud.open", json!({})).unwrap_err(), craft_ui::cloud::UNAVAILABLE);
    assert_eq!(app.run("cloud.saveAs", json!({})).unwrap_err(), craft_ui::cloud::UNAVAILABLE);

    // The control channel's own command catalog must not lie about this either (a leak sibling
    // apps' reviews found: an "enabled" flag hardcoded true regardless of availability).
    let reported = control::all_commands(&app);
    let cloud_open = reported.as_array().unwrap().iter().find(|c| c["id"] == "cloud.open").unwrap();
    assert_eq!(cloud_open["enabled"], json!(false));

    // The Start tab's cloud section draws nothing: an account badge over an empty list reads as
    // a broken window. Collected from a real (if renderer-less) frame, so removing the
    // availability guard in `cloud::open_section` would make this fail, not merely look right.
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| cadcraft_ui_egui::cloud::open_section(&mut app, ui));
    out.textures_delta.clear(); // no renderer here
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(t) => text.push_str(t.galley.text()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let mut text = String::new();
    out.shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    assert!(text.is_empty(), "nothing is drawn with no backend connected: {text:?}");
}

#[test]
fn a_connected_backend_lists_files_and_cloud_open_reaches_them() {
    mock_reader();
    let root = tempfile::tempdir().expect("temp dir");
    std::fs::write(root.path().join("Plan.dxf"), MAGIC).expect("seed a drawing in the reference backend's folder");

    let mut app = CadApp::new(Session::new(), Services::default());
    app.cloud.connect(LocalBackend::new(root.path().to_path_buf()));
    assert!(available(&app));
    assert!(menus::ui_command_enabled(&app, "cloud.open"));
    let reported = control::all_commands(&app);
    let cloud_open = reported.as_array().unwrap().iter().find(|c| c["id"] == "cloud.open").unwrap();
    assert_eq!(cloud_open["enabled"], json!(true));

    assert_eq!(app.run("cloud.open", json!({})).unwrap(), serde_json::Value::Null);
    assert_eq!(app.cloud_page, Some(cadcraft_ui_egui::cloud::Page::Open));

    let ctx = egui::Context::default();
    start(&mut app);
    settle(&mut app, &ctx, |a| a.cloud.mine.loaded);
    let entry = app.cloud.mine.entries.iter().find(|e| e.name == "Plan.dxf").cloned().expect("the seeded file is listed");

    let before = app.session.docs.len();
    open_entry(&mut app, entry, None);
    settle(&mut app, &ctx, |a| a.session.docs.len() > before);
    let opened = app.session.docs.last().expect("the cloud entry opened a drawing");
    // Never adopts a temp path for a cloud-opened document, and never marks it clean with a
    // recents entry: both would be mistaken for a document that lives on this computer.
    assert!(opened.path.is_none(), "a cloud-opened drawing has no local path");
    assert!(app.ui.recent_files.is_empty(), "a cloud open never adds a recents entry");
    assert!(app.cloud.origins.contains_key(&opened.uid), "the drawing remembers where it came from");
}

#[test]
fn a_cloud_save_only_marks_clean_once_the_write_lands() {
    let root = tempfile::tempdir().expect("temp dir");
    let mut app = CadApp::new(Session::new(), Services::default());
    app.cloud.connect(LocalBackend::new(root.path().to_path_buf()));
    let ctx = egui::Context::default();

    let uid = app.session.state().unwrap().uid;
    assert!(!app.session.state().unwrap().is_dirty(), "a fresh drawing starts clean");
    // A change, so the save below has something worth writing back and clearing.
    app.run("circle", json!({"center": [0, 0], "radius": 1})).unwrap();
    assert!(app.session.state().unwrap().is_dirty());

    save_as(&mut app, "", "Plan.dxf").unwrap();
    // The write is in flight, not finished: a quit or close here must still see this drawing as
    // unsaved. Marking it clean synchronously, before the service actually answered, is exactly
    // the defect sibling apps' reviews found between an async save and a synchronous dirty check.
    assert!(app.session.docs.iter().find(|d| d.uid == uid).unwrap().is_dirty(), "still dirty while the save is in flight");

    settle(&mut app, &ctx, |a| a.cloud.busy.is_none());
    assert!(!app.session.docs.iter().find(|d| d.uid == uid).unwrap().is_dirty(), "clean once the write actually landed");
    assert!(app.cloud.origins.contains_key(&uid));
}

#[test]
fn cancel_save_as_drops_an_abandoned_destination_and_name() {
    let mut app = CadApp::new(Session::new(), Services::default());
    // Kept alive for the test (dropped, and the folder removed, at the end of the function); this
    // test never touches disk, so the backend only ever needs a path that is valid while it runs.
    let root = tempfile::tempdir().expect("temp dir");
    app.cloud.connect(LocalBackend::new(root.path().to_path_buf()));
    app.cloud.save_name = "Abandoned Copy.dxf".to_owned();

    // The File menu's `cloud.saveAs` is the host's "about to open an unrelated Save As" moment;
    // it must clear whatever an earlier, abandoned attempt left typed.
    app.run("cloud.saveAs", json!({})).unwrap();
    assert_eq!(app.cloud_page, Some(cadcraft_ui_egui::cloud::Page::Save));
    assert!(app.cloud.save_name.is_empty(), "an abandoned Save As name does not carry into the next one");
}

/// `craft_ui::cloud::Destination::extension` and this crate's own `output_name` are exercised
/// indirectly by the save test above; this covers `encode` refusing nothing a plain drawing can
/// always produce, whatever name the destination gives it.
#[test]
fn encode_writes_the_destinations_own_name() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let uid = app.session.state().unwrap().uid;
    let bytes = CloudApp::encode(&mut app, uid, Destination { name: "Report.dxf", mime: None }).expect("a fresh drawing always encodes");
    assert!(!bytes.is_empty());
}
