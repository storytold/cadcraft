//! Construction geometry uses the engine command from every UI entry and the real Tool Sets.

use std::sync::Arc;

use cadcraft_engine::doc::Handle;
use cadcraft_engine::{DocState, Session};
use cadcraft_ui_egui::{CadApp, Services, context_menu, menus};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

/// Every way to start BCONSTRUCTION from the UI.
#[derive(Clone, Copy, Debug)]
enum Entry {
    Typed,
    Start,
    Menu,
    NativeMenu,
    Context,
}

/// All the entry routes.
const ENTRIES: &[Entry] = &[Entry::Typed, Entry::Start, Entry::Menu, Entry::NativeMenu, Entry::Context];

/// A drawing with the block "Door" of two lines, in the Block Editor when `editing`. Returns the app and the two lines, nothing selected.
fn fixture(editing: bool) -> (CadApp, Handle, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let regular = app.run("line", json!({ "points": [[0, 0], [100, 0]] })).unwrap()["handles"][0].clone();
    let construction = app.run("line", json!({ "points": [[0, 30], [100, 30]] })).unwrap()["handles"][0].clone();
    app.run("block", json!({ "name": "Door", "base": [0, 0], "handles": [regular, construction] })).unwrap();
    // BLOCK remaps model-space handles into the definition while preserving entity order.
    let handles = app.session.doc().unwrap().block("Door").unwrap().entities.handles();
    assert_eq!(handles.len(), 2);
    let (regular, construction) = (handles[0], handles[1]);
    if editing {
        app.run("bedit", json!({ "name": "Door" })).unwrap();
    }
    app.session.set_selection(Vec::new());
    (app, regular, construction)
}

/// Whether `h` is marked as construction geometry.
fn marked(app: &CadApp, h: Handle) -> bool {
    let tags = &app.session.doc().unwrap().entity(h).unwrap().common.xdata;
    let Some(start) = tags.iter().position(|tag| tag.code == 1001 && tag.value.eq_ignore_ascii_case("ACAD_BCONSTRUCTION")) else { return false };
    tags.iter()
        .skip(start + 1)
        .take_while(|tag| tag.code != 1001)
        .any(|tag| tag.code == 1070 && tag.value.trim().parse::<u16>().is_ok_and(|value| value & 1 != 0))
}

/// Asserts that nothing observable changed between the two states.
fn unchanged(before: &DocState, after: &DocState) {
    assert_eq!(before.uid, after.uid);
    assert_eq!(before.revision, after.revision);
    assert_eq!(before.selection, after.selection);
    assert_eq!(before.previous_selection, after.previous_selection);
    assert_eq!(before.block_editor(), after.block_editor());
    assert!(Arc::ptr_eq(&before.doc, &after.doc));
    assert!(Arc::ptr_eq(&before.saved, &after.saved));
    for (old, new) in [(&before.undo, &after.undo), (&before.redo, &after.redo)] {
        assert_eq!(old.len(), new.len());
        for (old, new) in old.iter().zip(new) {
            assert_eq!((&old.label, &old.selection), (&new.label, &new.selection));
            assert!(Arc::ptr_eq(&old.doc, &new.doc));
        }
    }
}

/// Starts BCONSTRUCTION through `entry`.
fn invoke(app: &mut CadApp, entry: Entry) {
    match entry {
        Entry::Typed => {
            app.cmdline_or("BCONSTRUCTION", None);
        }
        Entry::Start => app.start("bconstruction"),
        Entry::Menu => menus::activate(app, "bconstruction"),
        Entry::NativeMenu => menus::activate_native(app, &egui::Context::default(), "bconstruction"),
        Entry::Context => context_menu::choose(app, "bconstruction"),
    }
}

/// Answers the operation prompt with `option` and checks that the command finished without a dialog.
fn choose(app: &mut CadApp, option: &str) {
    assert_eq!(app.session.running.as_ref().map(|running| running.id.as_str()), Some("bconstruction"));
    assert!(app.cmdline_or(option, None).is_none(), "{option} was refused: {:?}", app.session.log);
    assert!(app.session.running.is_none(), "{option} did not finish with a preselection or a display option");
    assert!(app.ui.dialog.is_none(), "BCONSTRUCTION must use the existing command prompt");
}

/// Ui entry routes refuse outside the editor before any change.
#[test]
fn ui_entry_routes_refuse_outside_the_editor_before_any_change() {
    assert!(cadcraft_engine::find_command("bconstruction").is_some(), "BCONSTRUCTION must be registered");
    for entry in ENTRIES {
        let (mut app, _, _) = fixture(false);
        let before = app.session.state().unwrap().clone();
        let log_start = app.session.log.len();
        invoke(&mut app, *entry);
        assert!(app.session.running.is_none(), "{entry:?}");
        assert!(app.ui.dialog.is_none(), "{entry:?}");
        assert!(app.session.log[log_start..].iter().any(|line| line.contains("Block Editor") && !line.contains("unknown command")), "{entry:?}");
        unchanged(&before, app.session.state().unwrap());
    }
    let (mut app, _, _) = fixture(false);
    let before = app.session.state().unwrap().clone();
    let error = app.run("bconstruction", json!({ "mode": "convert", "handles": [] })).unwrap_err();
    assert!(error.contains("Block Editor") && !error.contains("unknown command"), "{error}");
    unchanged(&before, app.session.state().unwrap());
}

/// The enabled state of the menu item `id`, searching the sub-menus too.
fn entry_enabled(entries: &[menus::Entry], id: &str) -> Option<bool> {
    entries.iter().find_map(|entry| match entry {
        menus::Entry::Item { id: command, enabled, .. } if command == id => Some(*enabled),
        menus::Entry::Sub { children, .. } => entry_enabled(children, id),
        _ => None,
    })
}

/// The menu uses the engine editor enabled state.
#[test]
fn the_menu_uses_the_engine_editor_enabled_state() {
    for editing in [false, true] {
        let (app, _, _) = fixture(editing);
        let tools = menus::tree(&app).into_iter().find(|(name, _)| name == "Tools").unwrap().1;
        assert_eq!(entry_enabled(&tools, "bconstruction"), Some(editing));
    }
}

/// Typed start and menu dispatches convert and revert the same selected object.
#[test]
fn typed_start_and_menu_dispatches_convert_and_revert_the_same_selected_object() {
    for entry in ENTRIES {
        let (mut app, _, construction) = fixture(true);
        let before = app.session.state().unwrap().clone();
        app.session.set_selection(vec![construction]);
        invoke(&mut app, *entry);
        choose(&mut app, "Convert");
        assert!(marked(&app, construction), "{entry:?}");
        let after = app.session.state().unwrap();
        assert_eq!((after.uid, after.block_editor()), (before.uid, Some("Door")));
        assert!(Arc::ptr_eq(&before.block_edit.as_ref().unwrap().host.doc, &after.block_edit.as_ref().unwrap().host.doc));
        app.session.set_selection(vec![construction]);
        invoke(&mut app, *entry);
        choose(&mut app, "Revert");
        assert!(!marked(&app, construction), "{entry:?}");
        assert_eq!(app.session.state().unwrap().block_editor(), Some("Door"));
    }
}

/// Explicit ui json calls convert and revert without a dialog.
#[test]
fn explicit_ui_json_calls_convert_and_revert_without_a_dialog() {
    let (mut app, _, construction) = fixture(true);
    for (mode, expected) in [("convert", true), ("revert", false)] {
        app.run("bconstruction", json!({ "mode": mode, "handles": [construction.hex()] })).unwrap();
        assert_eq!(marked(&app, construction), expected);
        assert!(app.ui.dialog.is_none());
        assert!(app.session.running.is_none());
    }
}

/// A UI test harness around `app`, with the GPU renderer when `render`.
fn harness(app: CadApp, render: bool) -> Harness<'static, CadApp> {
    let builder = Harness::builder().with_size(egui::vec2(1600.0, 1000.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

/// Clicks the Construction Geometry button of Tool Sets and answers its prompt with `option`.
fn tool_option(h: &mut Harness<'static, CadApp>, option: &str) {
    let button = h.query_by_role_and_label(Role::Button, "Construction Geometry").expect("Construction Geometry Tool Sets button is missing");
    assert!(!button.accesskit_node().is_disabled());
    button.click();
    h.run_steps(3);
    choose(h.state_mut(), option);
    h.run_steps(5);
}

/// Saves a screenshot of the harness as `bconstruction-<name>.png` in `directory`, when there is one.
fn capture(h: &mut Harness<'static, CadApp>, directory: Option<&std::path::Path>, name: &str) {
    if let Some(directory) = directory {
        std::fs::create_dir_all(directory).unwrap();
        h.render().unwrap().save(directory.join(format!("bconstruction-{name}.png"))).unwrap();
    }
}

/// The tool sets button refuses outside the editor without a dialog or drawing change.
#[test]
fn the_tool_sets_button_refuses_outside_the_editor_without_a_dialog_or_drawing_change() {
    let (mut app, _, _) = fixture(false);
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    let mut h = harness(app, false);
    let before = h.state().session.state().unwrap().clone();
    let log_start = h.state().session.log.len();
    h.query_by_role_and_label(Role::Button, "Construction Geometry").expect("Construction Geometry Tool Sets button is missing").click();
    h.run_steps(5);
    assert!(h.state().session.running.is_none());
    assert!(h.state().ui.dialog.is_none());
    assert!(h.state().session.log[log_start..].iter().any(|line| line.contains("Block Editor") && !line.contains("unknown command")));
    unchanged(&before, h.state().session.state().unwrap());
}

/// Tool sets convert show hide and save construction without drawing it in references.
#[test]
fn tool_sets_convert_show_hide_and_save_construction_without_drawing_it_in_references() {
    let directory = std::env::var_os("CADCRAFT_BCONSTRUCTION_SHOTS").map(std::path::PathBuf::from);
    let (mut app, regular, construction) = fixture(true);
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    app.session.zoom_extents().unwrap();
    let mut h = harness(app, directory.is_some());
    let before = h.state().session.state().unwrap().clone();
    h.state_mut().session.set_selection(vec![construction]);
    tool_option(&mut h, "Convert");
    assert!(marked(h.state(), construction));
    h.state_mut().session.set_selection(Vec::new());
    h.run_steps(3);
    let list = h.state().canvas.list.as_ref().unwrap();
    let regular_color = list.prims.iter().find(|prim| prim.handle == regular).unwrap().color;
    let made: Vec<_> = list.prims.iter().filter(|prim| prim.handle == construction).collect();
    assert!(made.len() > 1, "construction must be dashed in the editor");
    assert!(made.iter().all(|prim| prim.color != regular_color && prim.color.0 == prim.color.1 && prim.color.1 == prim.color.2));
    assert_eq!(h.state().session.state().unwrap().block_editor(), Some("Door"));
    capture(&mut h, directory.as_deref(), "editor-shown");

    h.state_mut().session.set_selection(vec![construction]);
    tool_option(&mut h, "Hide all");
    assert!(!h.state().canvas.list.as_ref().unwrap().prims.iter().any(|prim| prim.handle == construction));
    let editor = h.state().session.doc().unwrap();
    assert!(!editor.is_visible(editor.entity(construction).unwrap()), "Hide all hides the construction geometry");
    assert!(editor.hidden_objects.is_empty(), "and leaves object isolation alone");
    assert!(!h.state().session.selection().contains(&construction), "hidden construction must lose its selection grips");
    let state = h.state().session.state().unwrap();
    assert_eq!(cadcraft_engine::select::pick(&state.doc, &state.edit_space(), cadcraft_engine::geom::Vec2::new(50.0, 30.0), 1.0), None);
    assert_eq!(h.state().session.state().unwrap().block_editor(), Some("Door"));
    capture(&mut h, directory.as_deref(), "editor-hidden");
    tool_option(&mut h, "Show all");
    assert!(h.state().canvas.list.as_ref().unwrap().prims.iter().any(|prim| prim.handle == construction));
    assert!(h.state().session.selection().is_empty());
    let after = h.state().session.state().unwrap();
    assert_eq!(after.uid, before.uid);
    assert!(Arc::ptr_eq(&before.block_edit.as_ref().unwrap().host.doc, &after.block_edit.as_ref().unwrap().host.doc));

    h.state_mut().run("bsave", json!({})).unwrap();
    h.state_mut().run("bclose", json!({ "save": false })).unwrap();
    h.state_mut().session.zoom_extents().unwrap();
    h.run_steps(5);
    assert!(h.state().session.state().unwrap().block_editor().is_none());
    let list = h.state().canvas.list.as_ref().unwrap();
    assert_eq!(list.prims.len(), 1, "the INSERT must contain only the regular solid line");
    assert!(list.bounds.height().abs() < 1e-9, "construction at y=30 must not affect reference extents");
    capture(&mut h, directory.as_deref(), "saved-reference");
}
