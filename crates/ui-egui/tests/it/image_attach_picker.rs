//! IMAGEATTACH from the Insert menu or typed (#1702): the host's image picker asks for the file,
//! then the command line asks where to put it. Cancelling the picker attaches nothing, and JSON
//! calls never open it.

use std::cell::Cell;
use std::rc::Rc;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::set_image_decoder;
use cadcraft_engine::doc::{EntityKind, Raster};
use cadcraft_ui_egui::{CadApp, Services, menus};
use serde_json::json;

/// An app whose image picker counts its calls and answers `answer`; any file decodes to 4 x 2
/// pixels.
fn app(answer: Option<String>) -> (CadApp, Rc<Cell<u32>>) {
    set_image_decoder(|_| Raster::new(4, 2, vec![255; 32]).ok_or_else(|| "bad test raster".to_string()));
    let picks = Rc::new(Cell::new(0));
    let counted = picks.clone();
    let services = Services {
        pick_image: Some(Box::new(move || {
            counted.set(counted.get() + 1);
            answer.clone()
        })),
        ..Services::default()
    };
    (CadApp::new(Session::new(), services), picks)
}

fn images(app: &CadApp) -> usize {
    app.session.doc().map_or(0, |d| d.model.iter().filter(|e| matches!(e.kind, EntityKind::Image(_))).count())
}

#[test]
fn the_menu_picks_the_file_and_the_command_line_places_it() {
    let dir = std::env::temp_dir().join(format!("cadcraft-imageattach-picker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("photo.png");
    std::fs::write(&file, b"pixels").unwrap();
    let (mut app, picks) = app(Some(file.to_string_lossy().into_owned()));

    menus::activate(&mut app, "imageattach");
    assert_eq!(picks.get(), 1);
    let prompt = app.session.prompt_text();
    assert!(prompt.contains("Specify insertion point"), "{prompt}");
    app.cmdline("2,3");
    app.cmdline("8");
    app.cmdline("0");
    assert!(app.session.running.is_none());
    assert_eq!(images(&app), 1);
    let path = app.session.doc().unwrap().model.iter().find_map(|e| if let EntityKind::Image(i) = &e.kind { Some(i.path.clone()) } else { None });
    assert_eq!(path.as_deref(), Some(file.to_string_lossy().as_ref()), "an unsaved drawing keeps the full path");

    // Typed, it asks too.
    app.cmdline("IAT");
    assert_eq!(picks.get(), 2);
    app.session.cancel();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelling_the_picker_attaches_nothing_and_json_never_picks() {
    let (mut app, picks) = app(None);
    menus::activate(&mut app, "imageattach");
    assert_eq!(picks.get(), 1);
    assert!(app.session.running.is_none(), "no prompt is left waiting");
    assert_eq!(images(&app), 0);

    let r = app.run("imageattach", json!({ "path": "nowhere.png" }));
    assert!(r.is_err(), "{r:?}");
    assert_eq!(picks.get(), 1, "a JSON call runs the command as it is");
}

/// With no file system (the web) there is no name to ask for: IMAGEATTACH says what to do instead
/// of leaving a prompt that could never read the file.
#[test]
fn without_a_file_system_it_says_what_to_do() {
    let mut app = CadApp::new(Session::new(), Services { no_file_system: true, ..Services::default() });
    menus::activate(&mut app, "imageattach");
    assert!(app.session.running.is_none());
    let log = &app.session.log;
    assert!(log.iter().any(|l| l.contains("no file system")) && log.iter().any(|l| l.contains("`data`")), "{log:?}");
    // Each line fits a narrow command history.
    assert!(log.iter().filter(|l| l.contains("IMAGEATTACH") || l.contains("Attach the image")).all(|l| l.chars().count() <= 60), "{log:?}");
}

#[test]
fn without_a_picker_the_command_line_asks_for_the_file() {
    let mut app = CadApp::new(Session::new(), Services::default());
    menus::activate(&mut app, "imageattach");
    let prompt = app.session.prompt_text();
    assert!(prompt.contains("Enter image file name"), "{prompt}");
    app.session.cancel();
}
