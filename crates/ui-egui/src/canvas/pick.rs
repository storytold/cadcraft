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
/// Start TEXTEDIT on the text, mtext, attribute definition, dimension or multileader text at `p`
/// (the objects its shortcut menu offers Edit Text for, multileaders, #1081, and dimensions, #842);
/// false when there is none.
pub(super) fn edit_text_at(app: &mut CadApp, p: Vec2) -> bool {
    use cadcraft_engine::doc::EntityKind;
    let is_text = cadcraft_engine::cmd::curves::pick_at(&app.session, p).and_then(|h| {
        app.session.doc().ok()?.entity(h).map(|e| match &e.kind {
            EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::AttDef(_) | EntityKind::Dimension(_) => true,
            EntityKind::MLeader(m) => m.text.is_some(),
            _ => false,
        })
    });
    if is_text != Some(true) {
        return false;
    }
    app.start("textedit");
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
    let h = cadcraft_engine::select::pick_in(&st.doc, &st.edit_space(), st.viewport_frozen(), w, aperture)?;
    matches!(st.doc.entity(h)?.kind, cadcraft_doc::EntityKind::Hatch(_)).then_some(h)
}

/// The block reference under world point `w` within the pick aperture (PICKBOX × 1.5 pixels at
/// `scale` pixels per unit), if any.
pub(super) fn insert_at(app: &CadApp, w: Vec2, scale: f64) -> Option<Handle> {
    let st = app.session.state().ok()?;
    let aperture = app.session.settings.pickbox.max(1.0) * 1.5 / scale.max(1e-12);
    let h = cadcraft_engine::select::pick_in(&st.doc, &st.edit_space(), st.viewport_frozen(), w, aperture)?;
    matches!(st.doc.entity(h)?.kind, cadcraft_doc::EntityKind::Insert(_)).then_some(h)
}

#[cfg(test)]
mod hatch_pick_tests {
    use cadcraft_engine::Session;
    use cadcraft_geom::Vec2;
    use serde_json::json;

    use crate::{CadApp, Services};

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

    fn run_frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<egui::Event>, time: f64) {
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        app.logic(ctx);
        let input = egui::RawInput { screen_rect: screen, events, time: Some(time), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| app.ui(ui));
        out.textures_delta.clear();
    }

    #[test]
    fn double_clicking_a_block_reference_in_model_space_opens_bedit_and_edits_cleanly() {
        let mut app = CadApp::new(Session::new(), Services::default());
        // Create a block whose local geometry sits far from (0, 0).
        let l = app.run("line", json!({ "points": [[4400, -1200], [4500, -1200]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": "FRAME", "base": [0, 0], "handles": [l] })).unwrap();
        app.session.set_selection(Vec::new());
        let _ = app.session.zoom_extents();

        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(Vec2::new(4450.0, -1200.0));
        let btn =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
        run_frame(&mut app, &ctx, vec![btn(true), btn(false)], 0.2);
        run_frame(&mut app, &ctx, vec![btn(true), btn(false)], 0.3);
        run_frame(&mut app, &ctx, Vec::new(), 0.4);

        let st = app.session.state().unwrap();
        assert_eq!(st.block_editor(), Some("FRAME"), "double-click opens Block Editor");
        assert!(app.session.running.is_none(), "no stray grip edit on double-click");
        assert!(!st.block_unsaved(), "freshly opened block has no unsaved edits");
        // Block geometry at (4400..4500, -1200) is framed near the view center.
        let c = st.view().center;
        assert!((c.x - 4450.0).abs() < 100.0 && (c.y - (-1200.0)).abs() < 100.0, "view should frame offset block geometry: {c:?}");

        // Edit inside the Block Editor, verify unsaved state, then save and close.
        app.run("line", json!({ "points": [[4400, -1180], [4500, -1180]] })).unwrap();
        assert!(app.session.state().unwrap().block_unsaved(), "adding geometry flips block_unsaved");
        app.run("bsave", json!({})).unwrap();
        assert!(!app.session.state().unwrap().block_unsaved(), "bsave clears block_unsaved");
        app.run("bclose", json!({})).unwrap();
        assert_eq!(app.session.state().unwrap().block_editor(), None);
        assert_eq!(app.session.doc().unwrap().block("FRAME").unwrap().entities.len(), 2);
    }

    #[test]
    fn double_clicking_a_paper_space_block_overlapping_a_viewport_opens_bedit() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
        // Place a Paper Space title block line directly across the default viewport.
        let vp_center = app
            .session
            .doc()
            .unwrap()
            .layout("Layout1")
            .unwrap()
            .entities
            .iter()
            .find_map(|e| match &e.kind {
                cadcraft_doc::EntityKind::Viewport(v) if v.id != 1 => Some(v.center.xy()),
                _ => None,
            })
            .unwrap_or(Vec2::new(5.0, 4.0));
        let l =
            app.run("line", json!({ "points": [[vp_center.x - 2.0, vp_center.y], [vp_center.x + 2.0, vp_center.y]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": "TITLE", "base": [vp_center.x, vp_center.y], "handles": [l] })).unwrap();
        app.session.set_selection(Vec::new());
        let _ = app.session.zoom_extents();

        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(vp_center);
        let btn =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
        run_frame(&mut app, &ctx, vec![btn(true), btn(false)], 0.2);
        run_frame(&mut app, &ctx, vec![btn(true), btn(false)], 0.3);
        run_frame(&mut app, &ctx, Vec::new(), 0.4);

        assert_eq!(app.session.state().unwrap().block_editor(), Some("TITLE"), "opens BEDIT instead of MSPACE");
        assert!(app.session.state().unwrap().mspace.is_none(), "must not activate MSPACE");
        app.run("bclose", json!({ "save": false })).unwrap();
        assert_eq!(app.session.layout_space(), cadcraft_doc::Space::Paper("Layout1".into()));
    }

    #[test]
    fn double_clicking_an_xref_refuses_cleanly_with_reason() {
        use cadcraft_doc::{Block, Common, EntityKind, Insert, Line, Space};
        use cadcraft_geom::Vec3;
        use std::sync::Arc;

        let mut app = CadApp::new(Session::new(), Services::default());
        {
            let d = app.session.doc_mut().unwrap();
            let mut xb = Block::new("EXT_TB");
            xb.xref_path = Some("ext_tb.dwg".into());
            let h = d.new_handle();
            xb.entities.push(cadcraft_doc::Entity {
                handle: h,
                common: Common::default(),
                kind: EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) }),
            });
            d.blocks.insert("EXT_TB".into(), Arc::new(xb));
            d.add(&Space::Model, Common::default(), EntityKind::Insert(Insert { block: "EXT_TB".into(), ..Insert::default() })).unwrap();
        }
        let _ = app.session.zoom_extents();

        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(Vec2::new(5.0, 0.0));
        let btn =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
        run_frame(&mut app, &ctx, vec![btn(true), btn(false)], 0.2);
        run_frame(&mut app, &ctx, vec![btn(true), btn(false)], 0.3);
        run_frame(&mut app, &ctx, Vec::new(), 0.4);

        assert_eq!(app.session.state().unwrap().block_editor(), None, "xref cannot open in BEDIT");
        let last_msg = app.session.log.last().map(String::as_str).unwrap_or("");
        assert!(last_msg.contains("external reference") || last_msg.contains("XOPEN"), "refusal reason echoed: {last_msg}");
    }

    #[test]
    fn block_editor_paints_distinct_light_background_with_dark_aci7_contrast() {
        let dark = crate::theme::Tokens::DARK;
        let bedit_tokens = dark.for_block_editor();
        assert_ne!(bedit_tokens.canvas, dark.canvas, "distinct from dark Model Space");
        assert_ne!(bedit_tokens.canvas, dark.paper, "distinct from pure-white Paper Space sheet");
        assert!(bedit_tokens.canvas.r() > 180 && bedit_tokens.canvas.g() > 180 && bedit_tokens.canvas.b() > 180, "light background");
        assert!(bedit_tokens.crosshair.r() < 80, "dark high-contrast crosshair on light Block Editor background");

        let mut app = CadApp::new(Session::new(), Services::default());
        let l = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": "SYM", "base": [0, 0], "handles": [l] })).unwrap();
        app.run("bedit", json!({ "name": "SYM" })).unwrap();

        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0)));
        app.logic(&ctx);
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: screen, ..Default::default() }, |ui| app.ui(ui));
        out.textures_delta.clear();
        let mut saw_light_bg = false;
        let mut saw_dark_stroke = false;
        fn scan(shape: &egui::Shape, bg: egui::Color32, saw_bg: &mut bool, saw_dark: &mut bool) {
            match shape {
                egui::Shape::Rect(r) if r.fill == bg => *saw_bg = true,
                egui::Shape::LineSegment { stroke, .. } if stroke.color.r() < 40 && stroke.color.g() < 40 && stroke.color.b() < 40 => {
                    *saw_dark = true;
                }
                egui::Shape::Path(egui::epaint::PathShape { stroke, .. }) if matches!(stroke.color, egui::epaint::ColorMode::Solid(c) if c.r() < 40 && c.g() < 40 && c.b() < 40) =>
                {
                    *saw_dark = true;
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| scan(s, bg, saw_bg, saw_dark)),
                _ => {}
            }
        }
        for s in &out.shapes {
            scan(&s.shape, dark.block_editor_canvas, &mut saw_light_bg, &mut saw_dark_stroke);
        }
        assert!(saw_light_bg, "Block Editor canvas should be filled with the light block_editor_canvas token");
        assert!(saw_dark_stroke, "ACI-7 entity geometry should render dark against the light Block Editor background");
    }
}
