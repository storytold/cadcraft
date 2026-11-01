//! The Blocks palette: BLOCKSPALETTE opens it, it lists the drawing's blocks, a click
//! starts INSERT for the block, and the options settle the scale and rotation prompts.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, docking::Panel};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

/// Make block `name` from everything in model space now (the objects are removed).
fn make_block(app: &mut CadApp, name: &str, description: &str) {
    let handles: Vec<String> = app.session.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    app.run("block", json!({ "name": name, "base": [0, 0], "handles": handles, "keep": "delete", "description": description })).unwrap();
}

/// Blocks "Bolt" (a circle) and "Door" (a line, described "Hinged door").
fn drawing() -> CadApp {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("circle", json!({ "center": [0, 0], "radius": 1 })).unwrap();
    make_block(&mut app, "Bolt", "");
    app.run("line", json!({ "points": [[0, 0], [2, 0]] })).unwrap();
    make_block(&mut app, "Door", "Hinged door");
    app
}

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h = Harness::builder().with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn blockspalette_opens_the_palette_and_lists_the_blocks() {
    let mut app = drawing();
    assert!(app.cmdline("BLOCKSPALETTE").is_none());
    assert!(app.ui.docking.layout.contains(&Panel::Blocks));
    app.ui.in_window_menu = false;
    let h = harness(app);
    h.get_by_label("Door");
    h.get_by_label("Bolt");
}

#[test]
fn json_blockspalette_lists_and_never_opens_the_palette() {
    let mut app = drawing();
    app.run("insert", json!({ "name": "Door", "at": [5, 5] })).unwrap();
    let r = app.run("blockspalette", json!({})).unwrap();
    assert_eq!(
        r["blocks"],
        json!([
            { "name": "Bolt", "description": "", "references": 0 },
            { "name": "Door", "description": "Hinged door", "references": 1 },
        ])
    );
    assert!(!app.ui.docking.layout.contains(&Panel::Blocks));
}

/// A click on a tile starts INSERT and answers the prompts the options settle.
#[test]
fn a_click_starts_insert_for_the_block() {
    let mut app = drawing();
    // Alone in the dock the palette has the whole column.
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);
    h.get_by_label("Door").click();
    h.run_steps(3);
    let prompt = h.state().session.prompt_text();
    assert!(prompt.starts_with("INSERT Specify insertion point"), "{prompt}");
}

/// The options wait while a block is being placed, and Repeat Placement goes on until Escape.
#[test]
fn options_are_off_while_placing_and_the_block_is_placed_again_until_cancelled() {
    use cadcraft_engine::Input;
    let mut app = drawing();
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);
    let inserts = |h: &Harness<'static, CadApp>| {
        h.state().session.doc().unwrap().model.iter().filter(|e| matches!(e.kind, cadcraft_engine::doc::EntityKind::Insert(_))).count()
    };
    assert!(!h.get_by_label("Repeat Placement").accesskit_node().is_disabled());
    h.get_by_label("Door").click();
    h.run_steps(3);
    // Placing: the options are off.
    assert!(h.get_by_label("Repeat Placement").accesskit_node().is_disabled());
    // The first block is placed and the same block is attached again (Repeat Placement is on).
    h.state_mut().session.input(Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 5.0))).unwrap();
    h.run_steps(3);
    assert_eq!(inserts(&h), 1);
    assert!(h.state().session.prompt_text().starts_with("INSERT Specify insertion point"), "{}", h.state().session.prompt_text());
    h.state_mut().session.input(Input::Point(cadcraft_engine::geom::Vec2::new(9.0, 5.0))).unwrap();
    h.run_steps(3);
    assert_eq!(inserts(&h), 2);
    // Cancelling ends it: nothing is placed again and the options are back.
    h.state_mut().session.input(Input::Cancel).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");
    assert_eq!(inserts(&h), 2);
    assert!(!h.get_by_label("Repeat Placement").accesskit_node().is_disabled());
}

/// Commands that run from start to end between two frames (queued control requests do) add to
/// the drawing too, but that is no placement of the block: nothing starts placing it again.
#[test]
fn another_command_between_two_frames_ends_the_repeat() {
    use cadcraft_engine::Input;
    use cadcraft_engine::geom::Vec2;
    let mut app = drawing();
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);
    h.get_by_label("Door").click();
    h.run_steps(3);
    assert!(h.state().session.prompt_text().starts_with("INSERT"), "{}", h.state().session.prompt_text());

    let session = &mut h.state_mut().session;
    session.start("line").unwrap();
    session.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
    session.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
    session.input(Input::Enter).unwrap();
    assert_eq!(session.prompt_text(), "Command:");
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");

    // INSERT cancelled, then a JSON command that goes through no prompt.
    h.get_by_label("Door").click();
    h.run_steps(3);
    assert!(h.state().session.prompt_text().starts_with("INSERT"), "{}", h.state().session.prompt_text());
    h.state_mut().session.input(Input::Cancel).unwrap();
    h.state_mut().run("line", json!({ "points": [[2, 2], [3, 3]] })).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");

    // The block placed, then a command of its own that asks nothing: the user has moved on.
    h.get_by_label("Door").click();
    h.run_steps(3);
    let session = &mut h.state_mut().session;
    session.input(Input::Point(Vec2::new(5.0, 5.0))).unwrap();
    session.start("regen").unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");

    // A JSON command never stops a running one, so it does not end the repeat here either.
    h.get_by_label("Door").click();
    h.run_steps(3);
    h.state_mut().session.input(Input::Point(Vec2::new(6.0, 6.0))).unwrap();
    h.state_mut().run("line", json!({ "points": [[4, 4], [5, 5]] })).unwrap();
    h.run_steps(3);
    assert!(h.state().session.prompt_text().starts_with("INSERT"), "{}", h.state().session.prompt_text());
    h.state_mut().session.input(Input::Cancel).unwrap();
}

/// INSERT started as the palette starts it stops at the insertion point.
#[test]
fn insert_started_by_the_palette_flow_waits_for_the_point() {
    use cadcraft_engine::Input;
    let mut app = drawing();
    app.session.start("insert").unwrap();
    app.session.input(Input::Text("Door".into())).unwrap();
    app.session.input(Input::Keyword("Scale".into())).unwrap();
    app.session.input(Input::Text("2".into())).unwrap();
    app.session.input(Input::Keyword("Rotate".into())).unwrap();
    app.session.input(Input::Text("90".into())).unwrap();
    assert!(app.session.prompt_text().starts_with("INSERT Specify insertion point"), "{}", app.session.prompt_text());
    app.session.input(Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 5.0))).unwrap();
    let ins = app.session.doc().unwrap().model.iter().find_map(|e| match &e.kind {
        cadcraft_engine::doc::EntityKind::Insert(i) => Some(i.clone()),
        _ => None,
    });
    let ins = ins.expect("the block was inserted");
    assert_eq!(ins.block, "Door");
    assert!((ins.scale.x - 2.0).abs() < 1e-9);
    assert!((ins.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}

#[test]
fn the_palette_closes_and_reopens_where_it_was() {
    let mut app = drawing();
    app.cmdline("BLOCKSPALETTE");
    assert!(app.ui.docking.layout.contains(&Panel::Blocks));
    app.run("ui.dock", json!({ "operation": "close", "panel": "blocks" })).unwrap();
    assert!(!app.ui.docking.layout.contains(&Panel::Blocks));
    app.cmdline("BLOCKSPALETTE");
    assert!(app.ui.docking.layout.contains(&Panel::Blocks));
}

/// How many times a library file was parsed.
static READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Every library file reads as a drawing with a block "Valve" (a line), whatever its bytes, and
/// takes long enough to see the palette go on drawing meanwhile.
fn read_library(_bytes: &[u8], _name: &str) -> Result<cadcraft_engine::doc::Drawing, String> {
    use cadcraft_engine::doc::geom::Vec3;
    use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Handle, Line};
    READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mut d = Drawing::new_metric();
    let mut valve = Block::new("Valve");
    valve.entities.push(Entity::new(Handle(0x70), EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(4.0, 0.0, 0.0) })));
    d.blocks.insert("Valve".into(), std::sync::Arc::new(valve));
    Ok(d)
}

/// Draw frames until `label` shows: a library is read on a worker thread.
fn wait_for(h: &mut Harness<'static, CadApp>, label: &str) {
    for _ in 0..1000 {
        if h.query_by_label(label).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.run_steps(1);
    }
    panic!("{label} never showed");
}

/// The whole life of a library: added, read in the background, refreshed, placed from, removed.
#[test]
fn a_library_is_added_listed_placed_and_removed() {
    use cadcraft_engine::cmd::file::{IoHooks, set_io};
    set_io(IoHooks { read: read_library, write: |_, _| Err("no writer in this test".into()), plot: None });
    // No settings folder: the real library list is never written.
    cadcraft_engine::settings_dir::set(None);
    let dir = std::env::temp_dir().join(format!("cadcraft-blocks-palette-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("valves.dxf");
    std::fs::write(&file, b"x").unwrap();
    let mut app = drawing();
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.run("blocklibraries", json!({ "op": "add", "path": file.to_string_lossy() })).unwrap();
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);

    // The Libraries tab lists the library; choosing it shows its blocks.
    h.get_by_label("Libraries").click();
    h.run_steps(3);
    h.get_by_label("valves.dxf").click();
    h.run_steps(2);
    // The palette goes on drawing while the file is read, and says so.
    h.get_by_label("Reading the library...");
    assert!(h.get_by_label("Refresh").accesskit_node().is_disabled());
    wait_for(&mut h, "Valve");
    assert!(h.state().session.doc().unwrap().block("Valve").is_none());

    // Refresh parses the file again, even though its size and time have not changed.
    let reads = READS.load(std::sync::atomic::Ordering::SeqCst);
    h.get_by_label("Refresh").click();
    h.run_steps(2);
    wait_for(&mut h, "Valve");
    assert_eq!(READS.load(std::sync::atomic::Ordering::SeqCst), reads + 1);

    // Placing it defines the block here and waits for the insertion point.
    h.get_by_label("Valve").click();
    h.run_steps(3);
    assert!(h.state().session.doc().unwrap().block("Valve").is_some(), "{}", h.state().session.prompt_text());
    let prompt = h.state().session.prompt_text();
    assert!(prompt.starts_with("INSERT"), "{prompt}");
    h.state_mut().session.input(cadcraft_engine::Input::Cancel).unwrap();
    assert!(!h.state().status.as_ref().is_some_and(|(m, _)| m.contains("already in the drawing")));

    // Placing it again uses the definition now in the drawing, and says so.
    h.get_by_label("Valve").click();
    h.run_steps(3);
    assert!(h.state().status.as_ref().is_some_and(|(m, _)| m.contains("already in the drawing")), "{:?}", h.state().status);
    // A repeat placement of that block does not say it again.
    h.state_mut().status = None;
    h.state_mut().session.input(cadcraft_engine::Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 5.0))).unwrap();
    h.run_steps(3);
    assert!(h.state().status.is_none(), "{:?}", h.state().status);
    h.state_mut().session.input(cadcraft_engine::Input::Cancel).unwrap();

    // Removing the library empties the list again.
    h.get_by_label("Remove valves.dxf").click();
    h.run_steps(3);
    let r = h.state_mut().run("blocklibraries", json!({})).unwrap();
    assert_eq!(r["libraries"], json!([]));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_toolbar_icon_and_the_window_menu_open_the_palette_not_a_dialog() {
    let mut app = drawing();
    app.run("ui.dialog.blocks", serde_json::Value::Null).unwrap();
    assert!(app.ui.docking.layout.contains(&Panel::Blocks));
    assert!(app.ui.dialog.is_none(), "the old Blocks window is gone");
    // Like the other ui.dialog.* ids, it opens whatever the parameters are.
    let mut app = drawing();
    app.run("ui.dialog.blocks", json!({})).unwrap();
    assert!(app.ui.docking.layout.contains(&Panel::Blocks));
}

/// An exploded block leaves no reference, and it repeats all the same.
#[test]
fn repeat_placement_goes_on_when_the_block_is_placed_exploded() {
    use cadcraft_engine::Input;
    let mut app = drawing();
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);
    let count = |h: &Harness<'static, CadApp>, refs: bool| {
        h.state().session.doc().unwrap().model.iter().filter(|e| matches!(e.kind, cadcraft_engine::doc::EntityKind::Insert(_)) == refs).count()
    };
    h.get(egui_kittest::kittest::by().role(egui::accesskit::Role::CheckBox).label("Explode")).click();
    h.run_steps(2);
    h.get_by_label("Door").click();
    h.run_steps(3);
    // An exploded block leaves no reference, only its objects, and the next one is attached.
    h.state_mut().session.input(Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 5.0))).unwrap();
    h.run_steps(3);
    assert_eq!((count(&h, true), count(&h, false)), (0, 1));
    assert!(h.state().session.prompt_text().starts_with("INSERT Specify insertion point"), "{}", h.state().session.prompt_text());
    h.state_mut().session.input(Input::Point(cadcraft_engine::geom::Vec2::new(9.0, 5.0))).unwrap();
    h.run_steps(3);
    assert_eq!((count(&h, true), count(&h, false)), (0, 2));
    // Cancelling still ends it.
    h.state_mut().session.input(Input::Cancel).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");
    assert_eq!(count(&h, false), 2);
}

/// What ran while the palette was hidden went unwatched: nothing repeats when it comes back.
#[test]
fn a_hidden_palette_does_not_start_placing_again_when_it_comes_back() {
    use cadcraft_engine::Input;
    let mut app = drawing();
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);
    h.get_by_label("Door").click();
    h.run_steps(3);
    // The palette goes away while the block is being placed, and other commands run.
    h.state_mut().run("ui.dock", json!({ "operation": "close", "panel": "blocks" })).unwrap();
    h.run_steps(3);
    h.state_mut().session.input(Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 5.0))).unwrap();
    h.state_mut().run("line", json!({ "points": [[0, 0], [2, 0]] })).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");
    h.state_mut().run("blockspalette", serde_json::Value::Null).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:", "nothing is attached to the cursor");
}

/// Switching drawings ends the repeat.
#[test]
fn repeat_placement_does_not_carry_over_to_another_drawing() {
    let mut app = drawing();
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    let mut h = harness(app);
    h.get_by_label("Door").click();
    h.run_steps(3);
    // The user moves to a new drawing, whose handle counter is well past the first one's.
    h.state_mut().session.new_drawing(true);
    h.state_mut().session.running = None;
    for _ in 0..20 {
        h.state_mut().run("line", json!({ "points": [[0, 0], [2, 0]] })).unwrap();
    }
    make_block(h.state_mut(), "Door", "");
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:", "nothing is attached in the other drawing");
}

/// How many INSERT references model space holds.
fn references(app: &CadApp) -> usize {
    app.session.doc().unwrap().model.iter().filter(|e| matches!(e.kind, cadcraft_engine::doc::EntityKind::Insert(_))).count()
}

/// `app` with the palette open and the panels above it closed, so its gallery has room.
fn open_palette(mut app: CadApp) -> Harness<'static, CadApp> {
    for panel in ["layers", "properties"] {
        app.run("ui.dock", json!({ "operation": "close", "panel": panel })).unwrap();
    }
    app.cmdline("BLOCKSPALETTE");
    app.ui.in_window_menu = false;
    harness(app)
}

#[test]
fn escape_in_the_middle_of_a_drop_places_nothing_and_frees_the_options() {
    let mut h = open_palette(drawing());
    h.get_by_label("Door").click();
    h.run_steps(3);
    assert!(h.state().session.prompt_text().starts_with("INSERT"));
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");
    assert_eq!(references(h.state()), 0);
    assert!(!h.get_by_label("Repeat Placement").accesskit_node().is_disabled());
    // And nothing is attached again afterwards.
    h.run_steps(3);
    assert_eq!(h.state().session.prompt_text(), "Command:");
}

#[test]
fn a_placed_block_is_one_undo_step() {
    let mut h = open_palette(drawing());
    // Repeat off, so one click places one block.
    h.get_by_label("Repeat Placement").click();
    h.run_steps(2);
    h.get_by_label("Door").click();
    h.run_steps(3);
    h.state_mut().session.input(cadcraft_engine::Input::Point(cadcraft_engine::geom::Vec2::new(5.0, 5.0))).unwrap();
    h.run_steps(3);
    assert_eq!(references(h.state()), 1);
    h.state_mut().run("undo", serde_json::Value::Null).unwrap();
    assert_eq!(references(h.state()), 0, "one undo takes the placed block away");
    assert!(h.state().session.doc().unwrap().block("Door").is_some(), "the block itself is still defined");
}

#[test]
fn a_tile_is_reachable_and_placed_from_the_keyboard() {
    let mut h = open_palette(drawing());
    h.get_by_label("Door").focus();
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().session.prompt_text().starts_with("INSERT"), "{}", h.state().session.prompt_text());
}

/// A big gallery builds only the visible tiles, a few per frame, so it opens quickly.
#[test]
fn a_drawing_of_a_thousand_blocks_stays_responsive_and_bounded() {
    let mut app = CadApp::new(Session::default(), Services::default());
    for i in 0..1000 {
        app.run("circle", json!({ "center": [i, 0], "radius": 1 })).unwrap();
        make_block(&mut app, &format!("Part{i:04}"), "");
    }
    let mut h = open_palette(app);
    let started = std::time::Instant::now();
    h.run_steps(60);
    // Only the visible tiles are drawn, a few per frame: 60 frames are nowhere near 1000 builds.
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "{:?}", started.elapsed());
    h.get_by_label("Part0000");
    assert!(h.state().session.prompt_text() == "Command:");
}
