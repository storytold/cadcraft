//! IMAGE's `path`, `reload` and `detach` options (#3045): an image whose file is missing is pointed
//! at a file that exists, read again after the file changes, or removed with its references; the
//! command-line form `-IMAGE` reaches the same options.

use std::path::{Path, PathBuf};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_image_decoder, set_io};
use cadcraft_engine::doc::geom::{Vec2, Vec3};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Image, Raster, Space};
use serde_json::{Value, json};

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_image_decoder(|b| cadcraft_io::raster::decode(b).map_err(|e| e.to_string()));
}

/// A fresh folder for one test.
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadcraft-imagepath-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A PNG file of `w` x 2 pixels.
fn png(w: u32) -> Vec<u8> {
    let px: Vec<u8> = (0..w * 2).flat_map(|i| [(i % 250) as u8, 90, 7, 255]).collect();
    cadcraft_io::raster::encode_png(&Raster::new(w, 2, px).unwrap()).unwrap()
}

fn image(path: &str, name: &str) -> EntityKind {
    EntityKind::Image(Image {
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 1.0, 0.0),
        size: Vec2::new(4.0, 2.0),
        path: path.into(),
        name: name.into(),
        ..Image::default()
    })
}

/// A drawing saved in `dir` that names images the files of `dir` don't have, opened in a session.
fn session_in(dir: &Path) -> Session {
    hooks();
    let host = dir.join("host.dxf");
    std::fs::write(&host, cadcraft_io::write(&Drawing::new_metric(), "host.dxf").unwrap()).unwrap();
    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_string_lossy() })).unwrap();
    s
}

fn put(s: &mut Session, path: &str, name: &str) {
    s.doc_mut().unwrap().add(&Space::Model, Common::default(), image(path, name)).unwrap();
}

fn row(s: &mut Session, name: &str) -> Value {
    let l = s.execute("image", &json!({})).unwrap()["images"].clone();
    l.as_array().unwrap().iter().find(|r| r["name"] == name).cloned().unwrap_or(Value::Null)
}

fn image_count(s: &Session) -> usize {
    s.doc().unwrap().model.iter().filter(|e| matches!(e.kind, EntityKind::Image(_))).count()
}

#[test]
fn path_points_a_missing_image_at_a_file_that_exists() {
    let dir = folder("path");
    let mut s = session_in(&dir);
    put(&mut s, "old/logo.png", "logo");
    assert_eq!(row(&mut s, "logo")["status"], "Not Found");

    std::fs::write(dir.join("logo.png"), png(4)).unwrap();
    let r = s.execute("image", &json!({ "option": "path", "names": "logo", "path": "logo.png" })).unwrap();
    assert_eq!((r["images"][0]["status"].clone(), r["images"][0]["path"].clone()), (json!("Loaded"), json!("logo.png")));
    let listed = row(&mut s, "logo");
    assert_eq!((listed["status"].clone(), listed["path"].clone()), (json!("Loaded"), json!("logo.png")));
    assert!(listed["foundAt"].as_str().is_some_and(|f| f.ends_with("logo.png")));

    // A file that is not there is reported as it is listed, not refused.
    let r = s.execute("image", &json!({ "option": "path", "names": "logo", "path": "nowhere.png" })).unwrap();
    assert_eq!(r["images"][0]["status"], "Not Found");
}

#[test]
fn reload_reads_the_file_again() {
    let dir = folder("reload");
    std::fs::write(dir.join("logo.png"), png(4)).unwrap();
    let mut s = session_in(&dir);
    put(&mut s, "logo.png", "logo");
    s.execute("image", &json!({ "option": "reload", "names": "logo" })).unwrap();
    let before = row(&mut s, "logo")["pixels"].clone();
    assert_eq!(row(&mut s, "logo")["status"], "Loaded");

    std::fs::write(dir.join("logo.png"), png(8)).unwrap();
    let r = s.execute("image", &json!({ "option": "reload", "names": "logo" })).unwrap();
    assert_eq!(r["images"][0]["status"], "Loaded");
    assert_ne!(row(&mut s, "logo")["pixels"], before, "the new file's pixels");

    // The file gone: the status says so.
    std::fs::remove_file(dir.join("logo.png")).unwrap();
    let r = s.execute("image", &json!({ "option": "reload", "names": "logo" })).unwrap();
    assert_eq!(r["images"][0]["status"], "Not Found");
}

#[test]
fn detach_removes_the_definition_and_its_images_only() {
    let dir = folder("detach");
    let mut s = session_in(&dir);
    put(&mut s, "a.png", "a");
    put(&mut s, "a.png", "a");
    put(&mut s, "b.png", "b");
    s.cmdline("LINE 0,0 5,5").unwrap();
    let others = s.doc().unwrap().model.len() - image_count(&s);

    let r = s.execute("image", &json!({ "option": "detach", "names": "a" })).unwrap();
    assert_eq!(r["erased"], 2);
    assert_eq!(row(&mut s, "a"), Value::Null);
    assert_ne!(row(&mut s, "b"), Value::Null);
    assert_eq!(image_count(&s), 1);
    assert_eq!(s.doc().unwrap().model.len() - image_count(&s), others, "what is not an image stays");
}

#[test]
fn unknown_options_names_and_missing_arguments_are_refused() {
    let dir = folder("refused");
    let mut s = session_in(&dir);
    put(&mut s, "a.png", "a");
    let bad = |s: &mut Session, p: Value| s.execute("image", &p).is_err();
    assert!(bad(&mut s, json!({ "option": "frobnicate" })));
    assert!(bad(&mut s, json!({ "option": "detach", "names": "nope" })));
    assert!(bad(&mut s, json!({ "option": "reload", "names": "nope" })));
    assert!(bad(&mut s, json!({ "option": "path", "names": "nope", "path": "x.png" })));
    assert!(bad(&mut s, json!({ "option": "path", "names": "a" })), "no path");
    assert!(bad(&mut s, json!({ "option": "path", "path": "x.png" })), "no names");
    assert!(bad(&mut s, json!({ "option": "detach" })), "no names");
    let e = s.execute("image", &json!({ "option": "detach", "names": "nope" })).unwrap_err().to_string();
    assert!(e.contains("nope"), "the error names the image: {e}");
    assert_eq!(image_count(&s), 1, "nothing was changed");
    assert_eq!(row(&mut s, "a")["path"], "a.png");
}

#[test]
fn the_command_line_form_reaches_the_options() {
    let dir = folder("cmdline");
    std::fs::write(dir.join("logo.png"), png(4)).unwrap();
    let mut s = session_in(&dir);
    put(&mut s, "old/logo.png", "logo");
    s.cmdline("-image path \"logo\" \"logo.png\"").unwrap();
    assert!(s.current_prompt().is_none(), "the command ended");
    assert_eq!(row(&mut s, "logo")["status"], "Loaded");

    s.cmdline("-image detach logo").unwrap();
    assert_eq!(image_count(&s), 0);

    // A name nothing has is reported, and the drawing is left as it was.
    put(&mut s, "a.png", "a");
    s.cmdline("-image detach nope").unwrap();
    assert_eq!(image_count(&s), 1);
    assert!(s.log.iter().any(|l| l.contains("nope")), "{:?}", s.log);
}

#[test]
fn path_and_detach_are_undone() {
    let dir = folder("undo");
    std::fs::write(dir.join("logo.png"), png(4)).unwrap();
    let mut s = session_in(&dir);
    put(&mut s, "old/gone.png", "logo");
    // Settle the prepared state as the last step to go back to.
    s.execute("image", &json!({ "option": "path", "names": "logo", "path": "logo.png" })).unwrap();
    assert_eq!(row(&mut s, "logo")["status"], "Loaded");
    s.execute("undo", &json!({})).unwrap();
    let r = row(&mut s, "logo");
    assert_eq!((r["path"].clone(), r["status"].clone()), (json!("old/gone.png"), json!("Not Found")));

    s.execute("image", &json!({ "option": "detach", "names": "logo" })).unwrap();
    assert_eq!(image_count(&s), 0);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(image_count(&s), 1);
    assert_ne!(row(&mut s, "logo"), Value::Null);
}

#[test]
fn reload_and_listing_leave_no_undo_step_and_the_saved_drawing_unmodified() {
    let dir = folder("reload-clean");
    std::fs::write(dir.join("logo.png"), png(4)).unwrap();
    let mut s = session_in(&dir);
    put(&mut s, "logo.png", "logo");
    s.execute("qsave", &json!({})).unwrap();
    assert!(!s.state().unwrap().is_dirty(), "saved");
    let steps = s.state().unwrap().undo.len();
    let before = row(&mut s, "logo")["pixels"].clone();

    std::fs::write(dir.join("logo.png"), png(8)).unwrap();
    s.execute("image", &json!({ "option": "reload", "names": "logo" })).unwrap();
    assert_ne!(row(&mut s, "logo")["pixels"], before, "the file was read again");
    s.execute("image", &json!({ "option": "?" })).unwrap();
    s.cmdline("-image reload logo").unwrap();
    let st = s.state().unwrap();
    assert!(!st.is_dirty(), "reloading is not a change to the drawing");
    assert_eq!(st.undo.len(), steps, "and no step to undo");

    // path and detach still are changes, each one step.
    s.execute("image", &json!({ "option": "detach", "names": "logo" })).unwrap();
    let st = s.state().unwrap();
    assert!(st.is_dirty());
    assert!(st.undo.len() > steps);
}
