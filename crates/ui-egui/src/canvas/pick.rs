use super::{CadApp, Handle, Input, Vec2};

/// The vertical wheel scroll this frame that zooms, in points, read from the raw wheel events so
/// each notch zooms by its whole step at once, as in standard CAD (#660). egui's smoothed delta
/// spreads a notch over several frames, so quick notches lag behind the wheel. Units convert to
/// points as egui does (a line is `line_scroll_speed` points, a page the view's height), and
/// events egui turns into a pinch zoom (⌘/Ctrl) or a sideways scroll (⇧) are left out, as it does.
pub(super) fn wheel_zoom_points(events: &[egui::Event], opts: &egui::InputOptions, page: f32) -> f32 {
    let points: f32 = events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. }
                if !modifiers.matches_any(opts.zoom_modifier) && !modifiers.matches_any(opts.horizontal_scroll_modifier) =>
            {
                let per = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => opts.line_scroll_speed,
                    egui::MouseWheelUnit::Page => page,
                };
                Some(delta.y * per)
            }
            _ => None,
        })
        .sum();
    if points.is_finite() { points } else { 0.0 }
}

/// The model space tiles on screen (none without tiling): each tile's rectangle in `full`.
/// Start TEXTEDIT on the text, mtext or attribute definition at `p` (the objects its shortcut menu
/// offers Edit Text for); false when there is none.
pub(super) fn edit_text_at(app: &mut CadApp, p: Vec2) -> bool {
    use cadcraft_engine::doc::EntityKind;
    // A block reference: its attributes (Enhanced Attribute Editor), or the Block Editor for a
    // block without them, as in AutoCAD. External references are left alone.
    let block = cadcraft_engine::cmd::curves::pick_at(&app.session, p).and_then(|h| {
        let d = app.session.doc().ok()?;
        let EntityKind::Insert(i) = &d.entity(h)?.kind else { return None };
        let xref = d.block(&i.block).is_some_and(|b| b.xref_path.is_some());
        (!xref).then(|| (h, i.attribs.iter().any(|a| !a.constant)))
    });
    if let Some((h, attributes)) = block {
        if attributes {
            app.start("attedit");
            if let Err(e) = app.session.input(Input::Pick(vec![h])) {
                app.session.echo(e.to_string());
            }
        } else if let Err(e) = app.run("bedit", serde_json::json!({ "handle": h.hex() })) {
            app.session.echo(e);
        }
        return true;
    }
    let picked = cadcraft_engine::cmd::curves::pick_at(&app.session, p).and_then(|h| {
        app.session.doc().ok()?.entity(h).map(|e| match &e.kind {
            EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) | EntityKind::Dimension(_) => Some("textedit.object"),
            EntityKind::MLeader(m) if m.text.is_some() => Some("textedit.object"),
            EntityKind::Table(t) if t.cell_at_point(p).is_some() => Some("tabledit.cell"),
            _ => None,
        })
    });
    // Inside a table cell, away from its lines: the cell.
    let command = picked.flatten().or_else(|| {
        let d = app.session.doc().ok()?;
        let space = d.space(&app.session.space())?;
        space.iter().any(|e| matches!(&e.kind, EntityKind::Table(t) if t.cell_at_point(p).is_some())).then_some("tabledit.cell")
    });
    let Some(command) = command else { return false };
    app.start(command);
    if let Err(e) = app.session.input(Input::Point(p)) {
        app.session.echo(e.to_string());
    }
    true
}

/// The hatch under world point `w` within the pick aperture (PICKBOX × 1.5 pixels at `scale`
/// pixels per unit), if any.
pub(super) fn hatch_at(app: &CadApp, w: Vec2, scale: f64) -> Option<Handle> {
    let st = app.session.state().ok()?;
    let aperture = app.session.settings.pickbox.max(1.0) * 1.5 / scale.max(1e-12);
    let h = cadcraft_engine::select::pick(&st.doc, &st.edit_space(), w, aperture)?;
    matches!(st.doc.entity(h)?.kind, cadcraft_doc::EntityKind::Hatch(_)).then_some(h)
}

#[cfg(test)]
mod hatch_pick_tests {
    use cadcraft_engine::Session;
    use cadcraft_geom::Vec2;
    use serde_json::json;

    use crate::{CadApp, Services};

    /// An external reference whose insert names its block in other letter case is still left
    /// alone (no Block Editor), as block names are matched everywhere else (#2326 review).
    #[test]
    fn an_xref_named_in_other_case_is_left_alone() {
        use cadcraft_engine::doc::{Block, Common, EntityKind, Insert, Space};
        use cadcraft_geom::Vec3;
        let mut app = CadApp::new(Session::new(), Services::default());
        let d = app.session.doc_mut().unwrap();
        let mut b = Block::new("SITEPLAN");
        b.xref_path = Some("siteplan.dxf".into());
        let line = cadcraft_engine::doc::Line { a: Vec3::new(-5.0, 0.0, 0.0), b: Vec3::new(5.0, 0.0, 0.0) };
        b.entities.push(cadcraft_engine::doc::Entity::new(cadcraft_engine::doc::Handle(0x7001), EntityKind::Line(line)));
        d.blocks.insert("SITEPLAN".into(), std::sync::Arc::new(b));
        let ins = Insert {
            block: "siteplan".into(),
            insert: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        };
        d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
        assert!(cadcraft_engine::cmd::curves::pick_at(&app.session, Vec2::ZERO).is_some(), "the reference is picked");
        assert!(!super::edit_text_at(&mut app, Vec2::ZERO), "nothing to edit: no Block Editor attempt for an xref");
        assert!(app.session.state().ok().and_then(|st| st.block_editor()).is_none());
    }

    #[test]
    fn hatch_at_finds_a_hatch_under_the_cursor_only() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
        app.session.execute("line", &json!({ "points": [[20, 0], [30, 0]] })).unwrap();
        assert_eq!(super::hatch_at(&app, Vec2::new(5.0, 5.0), 10.0).map(|h| h.hex()), Some(hx));
        assert_eq!(super::hatch_at(&app, Vec2::new(25.0, 0.0), 10.0), None, "a line is not a hatch");
        assert_eq!(super::hatch_at(&app, Vec2::new(60.0, 60.0), 10.0), None);
    }

    #[test]
    fn double_clicking_text_dimensions_and_table_cells_opens_the_in_place_editor() {
        let mut app = CadApp::new(Session::new(), Services::default());
        // The text sits away from the drawing area's top-left corner (the viewport label).
        app.session.execute("line", &json!({ "points": [[-10, 20], [-9, 20]] })).unwrap();
        app.session.execute("mtext", &json!({ "at": [0, 10], "text": "hello world", "height": 1.0 })).unwrap();
        app.session.execute("table", &json!({ "at": [20, 10], "rows": 2, "cols": 2, "colWidth": 6, "rowHeight": 2 })).unwrap();
        let _ = app.session.zoom_extents();
        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        let mut time = 0.0;
        let mut frame = |app: &mut CadApp, events: Vec<egui::Event>| {
            time += 0.1;
            app.logic(&ctx);
            let input = egui::RawInput { screen_rect: screen, events, time: Some(time), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| app.ui(ui));
            out.textures_delta.clear();
        };
        let mut double_click = |app: &mut CadApp, world: Vec2| {
            // Far enough from any earlier click not to count as its second or third.
            for _ in 0..10 {
                frame(app, Vec::new());
            }
            let at = app.canvas.xf.unwrap().to_screen(world);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            frame(app, vec![egui::Event::PointerMoved(at)]);
            frame(app, vec![button(true), button(false)]);
            frame(app, vec![button(true), button(false)]);
            frame(app, Vec::new());
        };
        double_click(&mut app, Vec2::new(1.0, 9.6));
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("textedit.object"));
        assert!(matches!(app.session.text_editor().map(|e| e.kind), Some(cadcraft_engine::EditorKind::MText)));
        app.session.cancel();
        app.session.set_selection(Vec::new());
        double_click(&mut app, Vec2::new(23.0, 7.0));
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("tabledit.cell"));
        assert!(matches!(app.session.text_editor().map(|e| e.kind), Some(cadcraft_engine::EditorKind::TableCell { .. })));
    }

    #[test]
    fn double_clicking_a_hatch_opens_the_hatch_edit_dialog() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("rectang", &json!({ "p1": [0, 0], "p2": [10, 10] })).unwrap();
        let hx = app.session.execute("hatch", &json!({ "points": [[5, 5]], "pattern": "SOLID" })).unwrap()["handle"].as_str().unwrap().to_string();
        let _ = app.session.zoom_extents();
        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        let frame = |app: &mut CadApp, events: Vec<egui::Event>, time: f64| {
            app.logic(&ctx);
            let input = egui::RawInput { screen_rect: screen, events, time: Some(time), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| app.ui(ui));
            out.textures_delta.clear();
        };
        frame(&mut app, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(Vec2::new(5.0, 5.0));
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(at)], 0.1);
        frame(&mut app, vec![button(true), button(false)], 0.2);
        frame(&mut app, vec![button(true), button(false)], 0.3);
        frame(&mut app, Vec::new(), 0.4);
        assert_eq!(app.ui.dialog.as_deref(), Some("hatch"));
        assert_eq!(app.hatch_dlg.as_ref().map(|s| s.edit.iter().map(|h| h.hex()).collect::<Vec<_>>()), Some(vec![hx]));
        // The double-click (at the hatch's centre grip) only opens the dialog: no grip edit.
        assert!(app.session.running.is_none(), "{:?}", app.session.running.as_ref().map(|r| r.machine.name()));
    }
}
