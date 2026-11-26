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

/// The block reference with an attribute that can be changed that a double-click at world point
/// `p` lands on: on its geometry, as a pick takes it, or on one of its attributes' text where the
/// renderer draws it (attribute text is drawn as part of the reference, not as an object of its
/// own). The topmost of those wins, so an object drawn over the reference takes the click. `None`
/// when there is none or the reference has only constant attributes.
///
/// The MSPACE geometry pick uses the viewport's annotation scale. The attribute text comes from
/// a double-click-only candidate list ([`cadcraft_render::attribute_reference_at`]) under one
/// work budget. When the budget runs out, a confirmed geometry pick stands.
pub(super) fn editable_block_at(app: &mut CadApp, p: Vec2) -> Option<Handle> {
    editable_block_at_with(app, p, cadcraft_render::MAX_ATTRIB_PICK_WORK).0
}

/// [`editable_block_at`] with a work budget of `budget` units, and what it spent.
fn editable_block_at_with(app: &mut CadApp, p: Vec2, budget: u64) -> (Option<Handle>, cadcraft_render::AttribPickStats) {
    use cadcraft_engine::doc::Space;
    let mut pick = cadcraft_render::AttribPick::with_budget(budget);
    let aperture = cadcraft_engine::cmd::curves::pick_aperture(&app.session);
    let Ok(st) = app.session.state() else { return (None, pick.stats()) };
    // Inside the active MSPACE viewport the edited space is model space, seen at the viewport's
    // scale (read in place, as `DocState::edit_space` finds that viewport).
    let viewport = st.mspace.and_then(|h| st.doc.space(&st.space)?.get(h)).and_then(|e| match &e.kind {
        cadcraft_engine::doc::EntityKind::Viewport(v) if v.height > 1e-12 && v.view_height > 1e-12 => Some(v.view_height / v.height),
        _ => None,
    });
    let space = if viewport.is_some() { &Space::Model } else { &st.space };
    let Some(store) = st.doc.space(space) else { return (None, pick.stats()) };
    // The scale annotative references are drawn at, as the renderer draws them: the viewport's
    // inside it, 1:1 on a sheet, CANNOSCALE in model space.
    let anno = match (viewport, space) {
        (Some(scale), _) => cadcraft_render::AnnoView::Viewport(scale),
        (None, Space::Paper(_)) => cadcraft_render::AnnoView::Sheet,
        (None, _) => cadcraft_render::AnnoView::Model,
    };
    let frozen = st.viewport_frozen();
    let picked = match viewport {
        Some(scale) => cadcraft_engine::select::pick_at_viewport_scale(&st.doc, frozen, scale, p, aperture),
        None => cadcraft_engine::cmd::curves::pick_at(&app.session, p),
    };
    let click = cadcraft_render::AttribClick { at: p, aperture, picked, frozen, anno };
    let found = cadcraft_render::attribute_reference_at(&st.doc, store, click, &mut app.canvas.attrib_targets, &mut pick);
    (found, pick.stats())
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
        crate::framework::install_headless(&ctx);
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

#[cfg(test)]
mod block_pick_tests {
    use cadcraft_doc::{Attrib, Block, Common, Entity, EntityKind, Handle, Insert, Line, MText, Space, Text, VAlign};
    use cadcraft_engine::Session;
    use cadcraft_geom::{Vec2, Vec3};
    use cadcraft_render::{AttribPickStats, MAX_ATTRIB_PICK_WORK};
    use serde_json::json;

    use crate::{CadApp, Services};

    /// A double-click's answer and work at `p` with the full budget.
    fn click(app: &mut CadApp, p: Vec2) -> (Option<Handle>, AttribPickStats) {
        super::editable_block_at_with(app, p, MAX_ATTRIB_PICK_WORK)
    }

    /// An app whose view makes the pick aperture small (0.01 units a pixel).
    fn app() -> CadApp {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.session.execute("view.set", &json!({ "center": [0, 0], "height": 8 })).unwrap();
        let aperture = cadcraft_engine::cmd::curves::pick_aperture(&app.session);
        assert!(aperture < 0.1, "aperture {aperture}");
        app
    }

    /// An attribute (definition or reference value) for `tag`, its text at `text_at`.
    fn attrib(tag: &str, text_at: Vec3, value: &str) -> Attrib {
        let text = Text {
            insert: text_at,
            align_pt: None,
            height: 0.2,
            value: value.into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: cadcraft_doc::HAlign::Left,
            valign: VAlign::Baseline,
        };
        Attrib {
            tag: tag.into(),
            text,
            invisible: false,
            constant: false,
            prompt: String::new(),
            props: cadcraft_doc::AttribProps::default(),
            multiline: None,
            modes: cadcraft_doc::AttribModes::default(),
        }
    }

    /// A reference to `block` at the origin with `attribs` and the clip `boundary`.
    fn reference_with_attribs(block: &str, attribs: Vec<Attrib>, boundary: Option<cadcraft_doc::ClipBoundary>) -> EntityKind {
        EntityKind::Insert(Insert { block: block.into(), insert: Vec3::ZERO, attribs, clip: boundary, ..Insert::default() })
    }

    /// A reference to `block` at `at`, its one attribute's text at `text_at`.
    fn reference_with_text_at(block: &str, at: Vec3, text_at: Vec3, value: &str) -> EntityKind {
        EntityKind::Insert(Insert { block: block.into(), insert: at, attribs: vec![attrib("T", text_at, value)], ..Insert::default() })
    }

    /// A block of one line out of the way of its attribute text.
    fn holder_block(name: &str) -> Block {
        let mut blk = Block::new(name);
        blk.entities.push(Entity::new(Handle(60_000), EntityKind::Line(Line { a: Vec3::new(-10.0, -10.0, 0.0), b: Vec3::new(-9.0, -10.0, 0.0) })));
        blk
    }

    /// A block of `defs` vertical lines.
    fn bulk_block(defs: usize) -> Block {
        let mut blk = Block::new("Bulk");
        for i in 0..defs {
            let x = i as f64;
            blk.entities
                .push(Entity::new(Handle(10_000 + i as u64), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 1.0, 0.0) })));
        }
        blk
    }

    /// The middle of the box the renderer draws single-line attribute `a` in.
    fn centre(app: &CadApp, a: &Attrib) -> Vec2 {
        cadcraft_render::place_text_entity(app.session.doc().unwrap(), &a.text, &a.text.value).1.center()
    }

    /// A point in the box the renderer draws single-line attribute `a` in where the ordinary pick
    /// takes nothing. The ordinary pick hits a block reference by what it draws, text included, so
    /// a click on a glyph stroke is taken by the reference itself and the candidate scan has little
    /// left to decide. The tests that measure what the scan costs click between the strokes, where
    /// it decides alone. Corners come first: strokes sit nearer the middle of a box.
    fn between_strokes(app: &CadApp, a: &Attrib) -> Vec2 {
        let b = cadcraft_render::place_text_entity(app.session.doc().unwrap(), &a.text, &a.text.value).1;
        let spots = [0.9, 0.1, 0.5, 0.7, 0.3];
        for fy in spots {
            for fx in spots {
                let p = Vec2::new(b.min.x + b.width() * fx, b.min.y + b.height() * fy);
                if cadcraft_engine::cmd::curves::pick_at(&app.session, p).is_none() {
                    return p;
                }
            }
        }
        panic!("the ordinary pick takes something at every probe of the box {b:?}")
    }

    /// A distant double-click over many references: the first builds the candidate list (each
    /// object and attribute read once, nothing laid out, no definition read); the next reads
    /// nothing at all, however many references there are.
    #[test]
    fn a_distant_double_click_lays_nothing_out_and_a_kept_list_reads_nothing() {
        const DEFS: usize = 10_000;
        const REFS: usize = 10_000;
        let mut app = app();
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Bulk".into(), std::sync::Arc::new(bulk_block(DEFS)));
            for i in 0..REFS {
                let x = i as f64 * 20.0;
                d.add(&Space::Model, Common::default(), reference_with_text_at("Bulk", Vec3::new(x, 0.0, 0.0), Vec3::new(x, 1.0, 0.0), "1")).unwrap();
            }
        }
        let far = Vec2::new(-1.0e6, -1.0e6);
        let (hit, cold) = click(&mut app, far);
        assert_eq!(hit, None);
        assert_eq!(cold.entities_visited, REFS as u64, "the list is built once, one read per object");
        assert_eq!(cold.attributes_visited, REFS as u64);
        assert_eq!((cold.text_layouts, cold.definition_entities, cold.candidates_visited), (0, 0, 0), "a distant click tests no candidate");
        assert!(!cold.exhausted);
        let (hit, warm) = click(&mut app, far);
        assert_eq!(hit, None);
        assert_eq!(
            (warm.entities_visited, warm.attributes_visited, warm.chunks_built, warm.candidates_visited),
            (0, 0, 0, 0),
            "a kept list reads no object"
        );
        assert!(warm.work < 100, "only the list's chunks are looked at: {} units", warm.work);
    }

    /// A blank double-click over many ordinary objects: they are read once to build the list (none
    /// is a candidate), and never again.
    #[test]
    fn a_blank_click_over_many_ordinary_objects_reads_each_once() {
        const OBJECTS: usize = 10_000;
        let mut app = app();
        {
            let d = app.session.doc_mut().unwrap();
            for i in 0..OBJECTS {
                let x = i as f64 * 10.0;
                d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 20.0, 0.0), b: Vec3::new(x + 1.0, 20.0, 0.0) }))
                    .unwrap();
            }
        }
        let (hit, cold) = click(&mut app, Vec2::new(5.0, 5.0));
        assert_eq!(hit, None);
        assert_eq!(cold.entities_visited, OBJECTS as u64);
        assert_eq!((cold.candidates_visited, cold.text_layouts), (0, 0));
        let (_, warm) = click(&mut app, Vec2::new(5.0, 5.0));
        assert_eq!((warm.entities_visited, warm.candidates_visited), (0, 0), "no walk over the space on a later click");
    }

    /// A nearby miss over overlapping references: visible candidates whose reach holds the click
    /// are laid out until the shared budget stops the scan.
    #[test]
    fn a_nearby_miss_over_many_overlapping_references_stops_at_the_budget() {
        const REFS: usize = 10_000;
        let mut app = app();
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Miss".into(), std::sync::Arc::new(holder_block("Miss")));
            for _ in 0..REFS {
                d.add(&Space::Model, Common::default(), reference_with_text_at("Miss", Vec3::ZERO, Vec3::new(1.0, 0.5, 0.0), "1")).unwrap();
            }
        }
        // Within the text's reach, outside its box.
        let (hit, stats) = click(&mut app, Vec2::new(0.0, 0.5));
        assert_eq!(hit, None);
        assert!(stats.candidates_visited > 0 && stats.text_layouts > 0, "{stats:?}");
        assert!(stats.text_layouts < REFS as u64, "the budget stopped the layouts: {stats:?}");
        assert_eq!(stats.definition_entities, 1, "visibility checks share one pass over the block");
        assert!(stats.exhausted && stats.work <= MAX_ATTRIB_PICK_WORK, "{stats:?}");
    }

    /// Every reference above the hit is tested against the same block: its definition is read
    /// once for the click, not once per reference.
    #[test]
    fn an_attribute_scan_reads_each_block_definition_once() {
        const DEFS: usize = 50;
        const HIDDEN: usize = 100;
        let mut app = app();
        let (bottom, at);
        {
            let d = app.session.doc_mut().unwrap();
            let mut blk = Block::new("Tag");
            for i in 0..DEFS {
                let y = i as f64 * 0.02;
                blk.entities
                    .push(Entity::new(Handle(20_000 + i as u64), EntityKind::Line(Line { a: Vec3::new(5.0, y, 0.0), b: Vec3::new(6.0, y, 0.0) })));
            }
            blk.entities.push(Entity::new(Handle(30_000), EntityKind::AttDef(attrib("T", Vec3::new(0.0, 1.0, 0.0), "1"))));
            d.blocks.insert("Tag".into(), std::sync::Arc::new(blk));
            d.ensure_layer("HID");
            d.layer_mut("HID").unwrap().on = false;
            // The visible reference, drawn first (bottom), and hidden attributes above it: each
            // hidden reference's text can reach the click and can be skipped before layout.
            let a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
            bottom = d.add(&Space::Model, Common::default(), reference_with_attribs("Tag", vec![a.clone()], None)).unwrap();
            for _ in 0..HIDDEN {
                let mut hidden = a.clone();
                hidden.props.layer = Some("HID".into());
                d.add(&Space::Model, Common::default(), reference_with_attribs("Tag", vec![hidden], None)).unwrap();
            }
            at = a;
        }
        let p = between_strokes(&app, &at);
        let (hit, stats) = click(&mut app, p);
        assert_eq!(hit, Some(bottom), "the one reference whose attribute is drawn wins");
        assert_eq!(stats.definition_entities, (DEFS + 1) as u64, "one pass over the definition for all {HIDDEN} references");
        assert_eq!(stats.text_layouts, 1, "the hidden attributes skip layout");
    }

    #[test]
    fn hidden_multiline_attribute_text_leaves_the_layout_budget_for_visible_text() {
        let mut app = app();
        let visible = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        let bottom;
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Tag".into(), std::sync::Arc::new(holder_block("Tag")));
            d.ensure_layer("HID");
            d.layer_mut("HID").unwrap().on = false;
            bottom = d.add(&Space::Model, Common::default(), reference_with_attribs("Tag", vec![visible.clone()], None)).unwrap();
            let value = "A".repeat(1_000);
            let mtext = MText {
                insert: Vec3::new(0.0, 1.0, 0.0),
                height: 1.0,
                width: 10.0,
                attach: 1,
                rotation: 0.0,
                style: "Standard".into(),
                contents: value.clone(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            };
            let mut hidden = attrib("T", mtext.insert, &value);
            hidden.props.layer = Some("HID".into());
            hidden.multiline = Some(cadcraft_doc::MultilineAttrib { mtext, read_as: value });
            d.add(&Space::Model, Common::default(), reference_with_attribs("Tag", vec![hidden], None)).unwrap();
        }
        let at = between_strokes(&app, &visible);
        let (hit, stats) = super::editable_block_at_with(&mut app, at, 4_000);
        assert_eq!(hit, Some(bottom), "the hidden attribute cannot spend the budget on layout: {stats:?}");
        assert_eq!(stats.text_layouts, 1, "only the visible text is laid out: {stats:?}");
        assert!(!stats.exhausted, "{stats:?}");
    }

    /// When the budget runs out before a text hit, the ordinary pick stands: bounded, and never a
    /// different object.
    #[test]
    fn an_exhausted_attribute_scan_falls_back_to_the_ordinary_pick() {
        const ABOVE: usize = 2_000;
        let mut app = app();
        let near;
        {
            let d = app.session.doc_mut().unwrap();
            let mut b = Block::new("Near");
            b.entities.push(Entity::new(Handle(40_000), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(10.0, 0.0, 0.0) })));
            d.blocks.insert("Near".into(), std::sync::Arc::new(b));
            d.blocks.insert("Far".into(), std::sync::Arc::new(holder_block("Far")));
            near = d.add(&Space::Model, Common::default(), reference_with_text_at("Near", Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), "1")).unwrap();
            // Above it, references whose text is near the click (within its reach) but not under
            // it: they spend the budget without a hit.
            for _ in 0..ABOVE {
                d.add(&Space::Model, Common::default(), reference_with_text_at("Far", Vec3::ZERO, Vec3::new(5.0, 1.0, 0.0), "1")).unwrap();
            }
        }
        let at = Vec2::new(5.0, 0.0);
        assert_eq!(cadcraft_engine::cmd::curves::pick_at(&app.session, at), Some(near), "the ordinary pick takes Near's line");
        // The full budget tests every reference above it and falls back to Near.
        let (hit, full) = click(&mut app, at);
        assert_eq!(hit, Some(near));
        assert_eq!(full.text_layouts, ABOVE as u64, "{full:?}");
        assert!(!full.exhausted);
        // A small budget stops early, with the same answer.
        let (hit, small) = super::editable_block_at_with(&mut app, at, 20_000);
        assert_eq!(hit, Some(near), "the ordinary pick of the reference's geometry stands");
        assert!(small.exhausted && small.text_layouts < ABOVE as u64, "{small:?}");
    }

    /// Constant-only references: every attribute is read and paid for once, when the list is
    /// built; they are not candidates, so later clicks read none of them.
    #[test]
    fn constant_only_references_are_paid_for_when_the_list_is_built() {
        const REFS: usize = 200;
        const CONSTANTS: usize = 100;
        let build = || {
            let mut app = app();
            {
                let d = app.session.doc_mut().unwrap();
                d.blocks.insert("Const".into(), std::sync::Arc::new(holder_block("Const")));
                for _ in 0..REFS {
                    let attribs = (0..CONSTANTS)
                        .map(|_| {
                            let mut a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
                            a.constant = true;
                            a
                        })
                        .collect();
                    d.add(&Space::Model, Common::default(), reference_with_attribs("Const", attribs, None)).unwrap();
                }
            }
            app
        };
        let mut app = build();
        let at = between_strokes(&app, &attrib("T", Vec3::new(0.0, 1.0, 0.0), "1"));
        let (hit, cold) = click(&mut app, at);
        assert_eq!(hit, None, "constant attributes cannot be edited");
        assert_eq!(cold.attributes_visited, (REFS * CONSTANTS) as u64, "each constant attribute is read and paid for once");
        assert_eq!((cold.candidates_visited, cold.text_layouts, cold.definition_entities), (0, 0, 0));
        let (hit, warm) = click(&mut app, at);
        assert_eq!(hit, None);
        assert_eq!((warm.attributes_visited, warm.candidates_visited), (0, 0), "a kept list reads none of them again");
        // A budget smaller than the attribute lists runs out while they are read.
        let mut app = build();
        let (hit, small) = super::editable_block_at_with(&mut app, at, 5_000);
        assert_eq!(hit, None);
        assert!(small.exhausted && small.attributes_visited < (REFS * CONSTANTS) as u64, "{small:?}");
    }

    /// Every attribute is paid for before it is skipped, invisible ones included: a reference
    /// whose invisible attributes alone outrun the budget cannot reach its visible text.
    #[test]
    fn invisible_attributes_are_paid_for_before_they_are_skipped() {
        const INVISIBLE: usize = 21_000;
        let mut app = app();
        let visible = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        let reference;
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Inv".into(), std::sync::Arc::new(holder_block("Inv")));
            let mut attribs: Vec<Attrib> = (0..INVISIBLE)
                .map(|_| {
                    let mut a = attrib("I", Vec3::ZERO, "1");
                    a.invisible = true;
                    a
                })
                .collect();
            attribs.push(visible.clone());
            reference = d.add(&Space::Model, Common::default(), reference_with_attribs("Inv", attribs, None)).unwrap();
        }
        let at = between_strokes(&app, &visible);
        let (hit, full) = click(&mut app, at);
        assert_eq!(hit, Some(reference), "the full budget reaches the visible text");
        assert_eq!(full.attributes_visited, 2 * (INVISIBLE as u64 + 1), "read to build the list, then to test it");
        // The list is kept: a budget the invisible attributes alone outrun stops before the text.
        let (hit, small) = super::editable_block_at_with(&mut app, at, 20_000);
        assert_eq!(hit, None, "the invisible visits spent the budget before the visible text");
        assert_eq!(small.text_layouts, 0);
        assert!(small.exhausted);
    }

    /// ATTMODE 2 draws an invisible attribute's text, so a double-click on it opens the editor.
    #[test]
    fn an_invisible_attribute_is_clickable_when_attmode_draws_it() {
        let mut app = app();
        let mut a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        a.invisible = true;
        let reference;
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Mode".into(), std::sync::Arc::new(holder_block("Mode")));
            reference = d.add(&Space::Model, Common::default(), reference_with_attribs("Mode", vec![a.clone()], None)).unwrap();
        }
        let at = centre(&app, &a);
        assert_eq!(click(&mut app, at).0, None, "ATTMODE 1 does not draw it");
        app.session.doc_mut().unwrap().header.set("ATTMODE", cadcraft_doc::HVal::Int(2));
        let (hit, stats) = click(&mut app, at);
        assert_eq!(hit, Some(reference), "ATTMODE 2 draws the invisible attribute's text");
        assert_eq!(stats.text_layouts, 1);
        app.session.doc_mut().unwrap().header.set("ATTMODE", cadcraft_doc::HVal::Int(0));
        assert_eq!(click(&mut app, at).0, None, "ATTMODE 0 draws no attribute");
    }

    /// An inverted clip hides what is inside it: attribute text inside its boundary is not
    /// clickable, and text outside another reference's boundary is.
    #[test]
    fn an_inverted_clip_hides_the_inside_and_keeps_the_outside() {
        let mut app = app();
        let a = attrib("T", Vec3::new(0.5, 0.5, 0.0), "1");
        let lower;
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Inv2".into(), std::sync::Arc::new(holder_block("Inv2")));
            let inverted_square = |x: f64, y: f64| {
                let mut c =
                    cadcraft_doc::ClipBoundary::new(vec![Vec2::new(x, y), Vec2::new(x + 2.0, y), Vec2::new(x + 2.0, y + 2.0), Vec2::new(x, y + 2.0)]);
                c.inverted = true;
                c
            };
            // The lower reference's inverted boundary misses the text; the upper one covers it.
            lower =
                d.add(&Space::Model, Common::default(), reference_with_attribs("Inv2", vec![a.clone()], Some(inverted_square(10.0, 10.0)))).unwrap();
            d.add(&Space::Model, Common::default(), reference_with_attribs("Inv2", vec![a.clone()], Some(inverted_square(0.0, 0.0)))).unwrap();
        }
        let at = between_strokes(&app, &a);
        let (hit, stats) = click(&mut app, at);
        assert_eq!(hit, Some(lower), "the inside of the top reference's inverted boundary is hidden");
        assert_eq!(stats.clip_regions, 2, "both text hits tested their boundary");
    }

    /// A manual column-height array is part of a layout's payload: paid for before the layout, so
    /// a one-character value with a huge array spends the budget instead of being copied.
    #[test]
    fn a_large_column_height_array_is_paid_for_before_the_layout() {
        const HEIGHTS: usize = 500_000;
        for read_as in ["A", "B"] {
            let mut app = app();
            {
                let d = app.session.doc_mut().unwrap();
                d.blocks.insert("Cols".into(), std::sync::Arc::new(holder_block("Cols")));
                let mtext = MText {
                    insert: Vec3::new(0.0, 0.0, 0.0),
                    height: 1.0,
                    width: 10.0,
                    attach: 1,
                    rotation: 0.0,
                    style: "Standard".into(),
                    contents: "A".into(),
                    line_spacing: 1.0,
                    line_spacing_exact: false,
                    columns: Some(cadcraft_doc::MTextColumns {
                        kind: cadcraft_doc::MTextColumnType::Dynamic,
                        count: 1,
                        gutter: 0.0,
                        height: 1.0,
                        auto_height: false,
                        heights: vec![1.0; HEIGHTS],
                        flow_reversed: false,
                    }),
                };
                let mut a = attrib("T", Vec3::ZERO, "A");
                a.multiline = Some(cadcraft_doc::MultilineAttrib { mtext, read_as: read_as.into() });
                d.add(&Space::Model, Common::default(), reference_with_attribs("Cols", vec![a], None)).unwrap();
            }
            // Attach top-left: the drawn box hangs below the insertion point. The click is between
            // the legs of the "A", where the ordinary pick takes nothing (its crossbar, at the
            // middle of the box, would be taken as part of the reference).
            let at = Vec2::new(0.3, -0.9);
            assert_eq!(cadcraft_engine::cmd::curves::pick_at(&app.session, at), None, "read as {read_as}: the ordinary pick takes nothing there");
            let (hit, stats) = click(&mut app, at);
            assert_eq!(hit, None, "read as {read_as}: the {HEIGHTS}-entry array spends the budget");
            assert_eq!(stats.text_layouts, 0, "read as {read_as}: nothing is laid out before the array is paid for");
            assert!(stats.exhausted && stats.work <= MAX_ATTRIB_PICK_WORK, "read as {read_as}: {stats:?}");
        }
    }

    /// A definition's tag is compared, without case, to find an attribute's definition: paid for
    /// by its bytes before the compare, so an oversized tag spends the budget.
    #[test]
    fn a_large_definition_tag_is_paid_for_before_it_is_compared() {
        const TAG_BYTES: usize = 2_000_000;
        let mut app = app();
        let tag = "T".repeat(TAG_BYTES);
        let a = attrib(&tag, Vec3::new(0.0, 1.0, 0.0), "1");
        let reference;
        {
            let d = app.session.doc_mut().unwrap();
            let mut blk = holder_block("Big");
            blk.entities.push(Entity::new(Handle(60_002), EntityKind::AttDef(attrib(&tag, Vec3::new(0.0, 1.0, 0.0), "1"))));
            d.blocks.insert("Big".into(), std::sync::Arc::new(blk));
            reference = d.add(&Space::Model, Common::default(), reference_with_attribs("Big", vec![a.clone()], None)).unwrap();
        }
        let at = between_strokes(&app, &a);
        let (hit, full) = click(&mut app, at);
        assert_eq!(hit, Some(reference));
        assert!(full.work > (TAG_BYTES / 64) as u64, "the compare is paid for by its bytes: {full:?}");
        // With the list kept, the comparison must fit before the text can be laid out.
        let (hit, small) = super::editable_block_at_with(&mut app, at, 20_000);
        assert_eq!(hit, None, "the {TAG_BYTES}-byte tag spends the budget before it is compared");
        assert_eq!(small.text_layouts, 0, "the visibility check spends the budget before layout");
        assert!(small.exhausted);
    }

    /// A clip boundary is tested only for a text box under the click, and paid for by its
    /// vertices: distant clicks test none, and many large boundaries spend the budget.
    #[test]
    fn large_clipped_references_pay_for_the_boundary_only_on_a_text_hit() {
        const REFS: usize = 200;
        const VERTICES: usize = 4096;
        let mut app = app();
        let a = attrib("T", Vec3::new(10.0, 0.5, 0.0), "1");
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Clip".into(), std::sync::Arc::new(holder_block("Clip")));
            let circle: Vec<Vec2> = (0..VERTICES)
                .map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / VERTICES as f64;
                    Vec2::new(t.cos(), t.sin())
                })
                .collect();
            let boundary = cadcraft_doc::ClipBoundary::new(circle);
            for _ in 0..REFS {
                d.add(&Space::Model, Common::default(), reference_with_attribs("Clip", vec![a.clone()], Some(boundary.clone()))).unwrap();
            }
        }
        let (hit, far) = click(&mut app, Vec2::new(-1.0e6, -1.0e6));
        assert_eq!(hit, None);
        assert_eq!(far.clip_regions, 0, "no text hit tests no boundary");
        // On the text, outside every boundary: each reference pays for its point test and is cut.
        let at = centre(&app, &a);
        let (hit, on) = click(&mut app, at);
        assert_eq!(hit, None);
        assert_eq!(on.clip_regions, REFS as u64, "{on:?}");
        assert_eq!(on.clip_vertices, (REFS * VERTICES) as u64);
        let (hit, small) = super::editable_block_at_with(&mut app, at, 100_000);
        assert_eq!(hit, None);
        assert!(small.exhausted && small.clip_regions < REFS as u64, "the boundaries spend a small budget: {small:?}");
    }

    /// The visibility test reads layer names in place and pays for comparing them: attributes on
    /// a hidden layer with a very long name spend the budget instead of being copied. The
    /// reference's linetype is never read for it.
    #[test]
    fn a_hidden_layer_name_is_paid_for_before_it_is_compared() {
        const ATTRIBS: usize = 200;
        const NAME_BYTES: usize = 1_000_000;
        let mut app = app();
        let name = "L".repeat(NAME_BYTES);
        let a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        {
            let d = app.session.doc_mut().unwrap();
            d.ensure_layer(&name);
            d.layer_mut(&name).unwrap().on = false;
            // The definition puts the attributes on that layer; each attribute takes it from there.
            let mut blk = holder_block("Hid");
            let mut def = Entity::new(Handle(60_003), EntityKind::AttDef(attrib("T", Vec3::new(0.0, 1.0, 0.0), "1")));
            def.common.layer = name.clone();
            blk.entities.push(def);
            d.blocks.insert("Hid".into(), std::sync::Arc::new(blk));
            let common = Common { linetype: "D".repeat(NAME_BYTES), ..Common::default() };
            d.add(&Space::Model, common, reference_with_attribs("Hid", vec![a.clone(); ATTRIBS], None)).unwrap();
        }
        let at = centre(&app, &a);
        let (hit, stats) = click(&mut app, at);
        assert_eq!(hit, None, "the attributes are on a layer turned off");
        assert!(stats.exhausted, "comparing the long name for each attribute spends the budget: {stats:?}");
        assert!(stats.work <= MAX_ATTRIB_PICK_WORK);
    }

    /// Building the list lays no text out and copies no text: long invisible attributes are read
    /// (and paid for) one unit each; ATTMODE 0 builds nothing; an edit rebuilds only its chunk.
    #[test]
    fn building_the_list_lays_no_text_out() {
        const REFS: usize = 10_000;
        let mut app = app();
        let mut handles = Vec::new();
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Long".into(), std::sync::Arc::new(holder_block("Long")));
            for i in 0..REFS {
                let x = i as f64 * 20.0;
                let mut a = attrib("T", Vec3::new(x, 1.0, 0.0), &"L".repeat(1_000));
                a.invisible = true;
                let kind = EntityKind::Insert(Insert { block: "Long".into(), insert: Vec3::new(x, 0.0, 0.0), attribs: vec![a], ..Insert::default() });
                handles.push(d.add(&Space::Model, Common::default(), kind).unwrap());
            }
        }
        let far = Vec2::new(-1.0e6, -1.0e6);
        let (_, cold) = click(&mut app, far);
        assert_eq!((cold.entities_visited, cold.attributes_visited), (REFS as u64, REFS as u64));
        assert_eq!((cold.text_layouts, cold.payload_bytes), (0, 0), "the list measures no text");
        // ATTMODE 0 draws no attribute: the double-click builds and scans nothing.
        app.session.doc_mut().unwrap().header.set("ATTMODE", cadcraft_doc::HVal::Int(0));
        let (_, off) = click(&mut app, far);
        assert_eq!((off.entities_visited, off.candidates_visited, off.text_layouts), (0, 0, 0));
        app.session.doc_mut().unwrap().header.set("ATTMODE", cadcraft_doc::HVal::Int(1));
        // An edit to one reference rebuilds its chunk only.
        let edited = handles[REFS / 2];
        app.session
            .doc_mut()
            .unwrap()
            .modify_entity(edited, |e| {
                if let EntityKind::Insert(i) = &mut e.kind
                    && let Some(a) = i.attribs.first_mut()
                {
                    a.text.value = "M".repeat(2_000);
                }
            })
            .unwrap();
        let (_, changed) = click(&mut app, far);
        assert_eq!(changed.chunks_built, 1, "{changed:?}");
        assert!(changed.entities_visited <= 2_048, "one chunk's objects: {changed:?}");
        assert_eq!((changed.text_layouts, changed.payload_bytes), (0, 0));
    }

    /// Review case: attributes on a layer turned off, under the click, on a reference with a huge
    /// linetype name. The visibility test reads layer names only, so the linetype is never copied
    /// or paid for: the scan stays cheap and finds nothing.
    #[test]
    fn a_huge_reference_linetype_is_never_read() {
        const ATTRIBS: usize = 1_000;
        let mut app = app();
        let a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        {
            let d = app.session.doc_mut().unwrap();
            d.ensure_layer("OFF");
            d.layer_mut("OFF").unwrap().on = false;
            d.blocks.insert("Lt".into(), std::sync::Arc::new(holder_block("Lt")));
            let mut hidden = a.clone();
            hidden.props.layer = Some("OFF".into());
            let common = Common { linetype: "D".repeat(1_000_000), ..Common::default() };
            d.add(&Space::Model, common, reference_with_attribs("Lt", vec![hidden; ATTRIBS], None)).unwrap();
        }
        let at = centre(&app, &a);
        let (hit, stats) = click(&mut app, at);
        assert_eq!(hit, None, "the attributes are on a layer turned off");
        assert_eq!(stats.text_layouts, 0, "the hidden attributes skip layout");
        assert!(!stats.exhausted);
        assert!(stats.work < ATTRIBS as u64 * 2_000, "no unit pays for the 1,000,000-byte linetype: {stats:?}");
    }

    /// A double-click whose budget runs out while the list is built keeps what it built: the
    /// next double-click goes on from there, and once the list is whole the text is found.
    #[test]
    fn a_list_the_budget_cut_short_is_finished_by_later_double_clicks() {
        const OBJECTS: usize = 5_000;
        let mut app = app();
        let a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        let reference;
        {
            let d = app.session.doc_mut().unwrap();
            d.blocks.insert("Top".into(), std::sync::Arc::new(holder_block("Top")));
            for i in 0..OBJECTS {
                let x = i as f64 * 10.0 + 100.0;
                d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 20.0, 0.0), b: Vec3::new(x + 1.0, 20.0, 0.0) }))
                    .unwrap();
            }
            reference = d.add(&Space::Model, Common::default(), reference_with_attribs("Top", vec![a.clone()], None)).unwrap();
        }
        let at = between_strokes(&app, &a);
        let mut built = 0;
        let mut clicks = 0;
        let found = loop {
            clicks += 1;
            assert!(clicks <= 20, "the list never finished");
            let (hit, stats) = super::editable_block_at_with(&mut app, at, 1_500);
            built += stats.chunks_built;
            if hit.is_some() {
                break hit;
            }
            assert!(stats.exhausted, "a click that finds nothing ran out of budget: {stats:?}");
        };
        assert_eq!(found, Some(reference));
        assert!(clicks > 1, "the first click could not build the whole list");
        assert!(built >= (OBJECTS / 1024) as u64, "each chunk was built once: {built}");
    }

    /// The list follows edits: a moved attribute is found where it is drawn now, not where it
    /// was; a layer turned off, ATTMODE 0 and a block redefinition are seen at once (they are
    /// read on each click, never kept).
    #[test]
    fn edits_reach_the_double_click_at_once() {
        let mut app = app();
        let a = attrib("T", Vec3::new(0.0, 1.0, 0.0), "1");
        let reference;
        {
            let d = app.session.doc_mut().unwrap();
            let mut blk = holder_block("Ed");
            blk.entities.push(Entity::new(Handle(60_004), EntityKind::AttDef(attrib("T", Vec3::new(0.0, 1.0, 0.0), "1"))));
            d.blocks.insert("Ed".into(), std::sync::Arc::new(blk));
            reference = d.add(&Space::Model, Common::default(), reference_with_attribs("Ed", vec![a.clone()], None)).unwrap();
        }
        let old = centre(&app, &a);
        assert_eq!(click(&mut app, old).0, Some(reference));
        // Move the attribute's text 50 units right.
        let mut moved = a.clone();
        moved.text.insert = Vec3::new(50.0, 1.0, 0.0);
        app.session
            .doc_mut()
            .unwrap()
            .modify_entity(reference, |e| {
                if let EntityKind::Insert(i) = &mut e.kind {
                    i.attribs = vec![moved.clone()];
                }
            })
            .unwrap();
        let new = centre(&app, &moved);
        assert_eq!(click(&mut app, old).0, None, "nothing is drawn where the text was");
        assert_eq!(click(&mut app, new).0, Some(reference), "the text is found where it is drawn now");
        // The definition moves to a layer turned off: the attribute, which takes its layer, hides.
        {
            let d = app.session.doc_mut().unwrap();
            d.ensure_layer("OFF");
            d.layer_mut("OFF").unwrap().on = false;
            let mut blk = holder_block("Ed");
            let mut def = Entity::new(Handle(60_004), EntityKind::AttDef(attrib("T", Vec3::new(0.0, 1.0, 0.0), "1")));
            def.common.layer = "OFF".into();
            blk.entities.push(def);
            d.blocks.insert("Ed".into(), std::sync::Arc::new(blk));
        }
        assert_eq!(click(&mut app, new).0, None, "the redefined block hides the attribute");
        app.session.doc_mut().unwrap().layer_mut("OFF").unwrap().on = true;
        assert_eq!(click(&mut app, new).0, Some(reference), "the layer turned back on shows it");
        app.session.doc_mut().unwrap().header.set("ATTMODE", cadcraft_doc::HVal::Int(0));
        assert_eq!(click(&mut app, new).0, None, "ATTMODE 0 draws no attribute text");
    }

    /// An annotative reference draws its attributes scaled about its insertion point at the
    /// annotation scale: the double-click finds the text where it is drawn, not where it is
    /// stored.
    #[test]
    fn annotative_attribute_text_is_found_where_it_is_drawn() {
        let mut app = app();
        let a = attrib("T", Vec3::new(2.0, 1.0, 0.0), "1");
        let reference;
        {
            let d = app.session.doc_mut().unwrap();
            let half = cadcraft_doc::AnnoScale { name: "1:2".into(), paper_units: 1.0, drawing_units: 2.0 };
            d.scales.push(half.clone());
            d.header.set("CANNOSCALE", cadcraft_doc::HVal::Str("1:2".into()));
            d.blocks.insert("Anno".into(), std::sync::Arc::new(holder_block("Anno")));
            let common = Common { anno_scales: vec![half], ..Common::default() };
            reference = d.add(&Space::Model, common, reference_with_attribs("Anno", vec![a.clone()], None)).unwrap();
        }
        let stored = centre(&app, &a);
        let mut drawn = a.clone();
        drawn.text.insert = Vec3::new(4.0, 2.0, 0.0);
        drawn.text.height = 0.4;
        let drawn_at = centre(&app, &drawn);
        assert_eq!(click(&mut app, drawn_at).0, Some(reference), "the text is drawn at twice its distance and height");
        assert_eq!(click(&mut app, stored).0, None, "nothing is drawn where it is stored");
    }
}
