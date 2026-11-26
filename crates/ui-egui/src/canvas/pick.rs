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
    fn right_clicking_an_unselected_block_reference_selects_it_and_offers_bedit() {
        let mut app = CadApp::new(Session::new(), Services::default());
        assert!(!app.options.time_sensitive_right_click, "standard CAD shortcut menu is the default");
        let l = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": "PART", "base": [0, 0], "handles": [l] })).unwrap();
        app.session.set_selection(Vec::new());
        let _ = app.session.zoom_extents();

        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new(), 0.0);
        let at = app.canvas.xf.unwrap().to_screen(Vec2::new(5.0, 0.0));
        let rbtn =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::default() };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
        run_frame(&mut app, &ctx, vec![rbtn(true), rbtn(false)], 0.2);

        assert_eq!(app.session.selection().len(), 1, "right-click on unselected block selects it");
        let has_bedit =
            crate::context_menu::entries(&app).into_iter().flatten().any(|e| matches!(e, crate::menus::Entry::Item { id, .. } if id == "bedit"));
        assert!(has_bedit, "context menu offers Block Editor");
        crate::context_menu::choose(&mut app, "bedit");
        assert_eq!(app.session.state().unwrap().block_editor(), Some("PART"), "choosing Block Editor enters BEDIT");
    }

    #[test]
    fn right_click_inside_block_editor_opens_menu_and_never_kicks_user_out() {
        let mut app = CadApp::new(Session::new(), Services::default());
        let l = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": "PART", "base": [0, 0], "handles": [l] })).unwrap();
        app.run("bedit", json!({ "name": "PART" })).unwrap();

        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new(), 0.0);
        let at = egui::pos2(800.0, 500.0);
        let rbtn =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::default() };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
        run_frame(&mut app, &ctx, vec![rbtn(true), rbtn(false)], 0.15);
        run_frame(&mut app, &ctx, Vec::new(), 0.2);

        assert_eq!(app.session.state().unwrap().block_editor(), Some("PART"), "right-click stays inside Block Editor");
        assert!(app.session.running.is_none(), "quick right-click does not start a command");
        let be_items: Vec<(String, bool)> = crate::context_menu::entries(&app)
            .into_iter()
            .flatten()
            .filter_map(|e| match e {
                crate::menus::Entry::Item { id, enabled, .. } => Some((id, enabled)),
                _ => None,
            })
            .collect();
        assert!(be_items.iter().any(|(id, en)| id == "bsave" && !en), "bsave is disabled when clean: {be_items:?}");
        assert!(be_items.iter().any(|(id, en)| id == "bclose" && *en), "bclose is enabled: {be_items:?}");
        app.run("line", json!({ "points": [[0, 2], [5, 2]] })).unwrap();
        let bsave_on = crate::context_menu::entries(&app)
            .into_iter()
            .flatten()
            .any(|e| matches!(e, crate::menus::Entry::Item { id, enabled, .. } if id == "bsave" && enabled));
        assert!(bsave_on, "bsave enables once block has unsaved changes");
        app.start("bclose");
        app.session.input(cadcraft_engine::Input::Keyword("No".into())).unwrap();
        assert_eq!(app.session.state().unwrap().block_editor(), None);
        assert_ne!(app.session.last_command.as_deref(), Some("bclose"), "interactive bclose does not pollute last_command");
        let has_repeat_bclose =
            crate::context_menu::entries(&app).into_iter().flatten().any(|e| matches!(e, crate::menus::Entry::Item { id, .. } if id == "bclose"));
        assert!(!has_repeat_bclose, "Model Space menu never offers Repeat BCLOSE");
    }

    #[test]
    fn time_sensitive_right_click_opt_in_does_not_kick_user_out_of_block_editor() {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.run("options", json!({"timeSensitiveRightClick": true})).unwrap();
        let l = app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone();
        app.run("block", json!({ "name": "PART", "base": [0, 0], "handles": [l] })).unwrap();
        app.run("bedit", json!({ "name": "PART" })).unwrap();
        app.run("bclose", json!({ "save": false })).unwrap();
        assert_ne!(app.session.last_command.as_deref(), Some("bclose"), "bclose must not linger as repeatable last_command");
        app.run("bedit", json!({ "name": "PART" })).unwrap();
        assert_eq!(app.session.last_command, None, "entering BEDIT clears last_command");

        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx, Vec::new(), 0.0);
        let at = egui::pos2(800.0, 500.0);
        let rbtn =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::default() };
        run_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(at)], 0.1);
        run_frame(&mut app, &ctx, vec![rbtn(true)], 0.2);
        run_frame(&mut app, &ctx, vec![rbtn(false)], 0.25);
        run_frame(&mut app, &ctx, Vec::new(), 0.3);

        assert_eq!(app.session.state().unwrap().block_editor(), Some("PART"), "quick right-click never kicks user out of BEDIT");
    }
}
