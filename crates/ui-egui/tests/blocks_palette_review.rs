//! Regression for a selection menu click arriving as INSERT starts (#2122).
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_ui_egui::{CadApp, Services};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn release(h: &mut Harness<'_, CadApp>, pos: Pos2) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
}

/// The Blocks palette participates in the column-based DockContent renderer.
#[test]
fn docked_blocks_render_previews_and_reopen() {
    use cadcraft_ui_egui::{blockmgr, docking::Panel};
    let mut session = Session::new();
    let circle = session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [circle["handle"]], "keep": "delete"})).unwrap();
    let mut app = CadApp::new(session, Services::default());
    app.ui.start_tab = false;
    blockmgr::open(&mut app).unwrap();
    let shots = std::env::var_os("CADCRAFT_BLOCK_SHOTS").map(std::path::PathBuf::from);
    let builder = Harness::builder().with_size(vec2(1600.0, 1000.0));
    let builder = if shots.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    assert!(h.state().ui.docking.layout.contains(&Panel::Blocks));
    assert!(h.state().ui.docking.layout.floating.is_empty());
    assert_eq!(blockmgr::cached_preview_count(&h.ctx), 1);
    h.state_mut().run("ui.dock", json!({"operation": "close", "panel": "blocks"})).unwrap();
    h.run_steps(3);
    assert!(!h.state().ui.docking.layout.contains(&Panel::Blocks));
    blockmgr::open(h.state_mut()).unwrap();
    h.run_steps(5);
    assert!(h.state().ui.docking.layout.contains(&Panel::Blocks));
    assert_eq!(blockmgr::cached_preview_count(&h.ctx), 1);
    if let Some(dir) = shots {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("docked-blocks-merge.png")).unwrap();
    }
}

#[test]
fn selection_menu_click_cannot_place_a_block_when_insert_starts() {
    let mut session = Session::new();
    let c = session.execute("circle", &json!({"center": [0, 0], "radius": 2})).unwrap();
    session.execute("block", &json!({"name": "Pin", "base": [0, 0], "handles": [c["handle"]], "keep": "delete"})).unwrap();
    for end in [10, 20] {
        session.execute("line", &json!({"points": [[0, 0], [end, 0]]})).unwrap();
    }
    session.execute("setvar", &json!({"name": "SELECTIONCYCLING", "value": 2})).unwrap();
    session.set_selection(Vec::new());
    let mut app = CadApp::new(session, Services::default());
    app.canvas.zoom_pending = true;
    let shots = std::env::var_os("CADCRAFT_BLOCK_SHOTS").map(std::path::PathBuf::from);
    let builder = Harness::builder().with_size(vec2(1600.0, 1000.0));
    let builder = if shots.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let at = h.state().canvas.xf.unwrap().to_screen(Vec2::new(5.0, 0.0));
    h.event(Event::PointerMoved(at));
    h.run_steps(1);
    release(&mut h, at);
    h.run_steps(2);
    let rows = h.state().canvas.cycle_rows();
    assert_eq!(rows.len(), 2);
    let row = rows[0].0.center();
    h.event(Event::PointerMoved(row));
    h.run_steps(2);
    // Start placement between frames, while the old selection list is still on screen.
    let st = cadcraft_ui_egui::blockmgr::State { repeat: true, ..Default::default() };
    cadcraft_ui_egui::blockmgr::grab_block(h.state_mut(), "Pin", &st);
    release(&mut h, row);
    h.run_steps(2);
    assert_eq!(cadcraft_ui_egui::blockmgr::reference_count(h.state().session.doc().unwrap(), "Pin"), 0);
    assert!(h.state().session.current_prompt().unwrap().accept.point);
    // A subsequent click on bare canvas still places the block.
    let clear = h.state().canvas.xf.unwrap().to_screen(Vec2::new(7.0, 3.0));
    h.event(Event::PointerMoved(clear));
    h.run_steps(1);
    release(&mut h, clear);
    h.run_steps(2);
    assert_eq!(cadcraft_ui_egui::blockmgr::reference_count(h.state().session.doc().unwrap(), "Pin"), 1);
    h.state_mut().session.cancel();
    h.state_mut().start("insert");
    h.run_steps(3);
    if let Some(dir) = shots {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("blocks-palette.png")).unwrap();
    }
}

/// Hatched blocks remain visible in the palette instead of empty placeholders.
#[test]
fn hatched_blocks_have_visible_palette_previews() {
    use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Handle, Hatch, HatchLoop};
    use std::sync::Arc;
    let mut drawing = Drawing::default();
    for (index, (name, solid, curved)) in
        [("Solid hatch", true, false), ("Pattern hatch", false, false), ("Curved hatch", true, true)].into_iter().enumerate()
    {
        let mut hatch: Hatch =
            serde_json::from_value(json!({"pattern": if solid { "SOLID" } else { "ANSI31" }, "solid": solid, "loops": []})).unwrap();
        let mut vertices: Vec<_> = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]
            .into_iter()
            .map(cadcraft_geom::PolyVertex::new)
            .collect();
        if curved {
            vertices[0].bulge = 1.0;
        }
        hatch.loops.push(HatchLoop { vertices, outer: true, sources: Vec::new() });
        let mut block = Block::new(name);
        block.entities.push(Entity::new(Handle(index as u64 + 1), EntityKind::Hatch(hatch)));
        drawing.blocks.insert(name.into(), Arc::new(block));
    }
    let mut session = Session::empty();
    session.open_drawing(drawing, "Hatch previews", None);
    let mut app = CadApp::new(session, Services::default());
    app.ui.blocks_window = true;
    app.start("insert");
    let shots = std::env::var_os("CADCRAFT_BLOCK_SHOTS").map(std::path::PathBuf::from);
    let builder = Harness::builder().with_size(vec2(1000.0, 760.0));
    let builder = if shots.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            cadcraft_ui_egui::theme::apply(ui.ctx(), egui::Theme::Dark, false);
            cadcraft_ui_egui::dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(5);
    assert_eq!(cadcraft_ui_egui::blockmgr::cached_preview_count(&h.ctx), 3);
    if let Some(dir) = shots {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join("hatched-block-previews.png")).unwrap();
    }
}
