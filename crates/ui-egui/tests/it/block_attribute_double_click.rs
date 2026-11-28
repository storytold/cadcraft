//! Double-clicking a block reference with editable attributes opens the Enhanced Attribute Editor
//! on it (#2626), as typed EATTEDIT does with that reference picked: whether the click lands on the
//! reference's geometry or on one of its attributes' text. A reference with only constant
//! attributes opens nothing; text and the programmatic paths are unchanged.
//!
//! The candidate references come from a double-click-only list (not the selection index), each
//! with where its attribute text can reach from its own data; the renderer's own layout and
//! visibility rules then decide the hit. Formatted text beyond the document's extent estimate
//! (inline heights and widths, wide line spacing, long values, Fit text) is therefore found, and
//! a later edit, text style change or layer change is seen by the next double-click.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{
    AnnoScale, Attrib, ClipBoundary, Common, EntityKind, HAlign, HVal, Handle, MText, MultilineAttrib, Space, VAlign, Viewport,
};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_ui_egui::{CadApp, Services, canvas, framework};
use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

/// Block "Tag": a line and two attributes (NO, prompt "Number", and NOTE), both editable.
fn define_block(app: &mut CadApp) {
    let mut hs = vec![app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone()];
    hs.push(app.run("attdef", json!({ "tag": "NO", "prompt": "Number", "default": "1", "at": [0, 1] })).unwrap()["handle"].clone());
    hs.push(app.run("attdef", json!({ "tag": "NOTE", "default": "n", "at": [0, 2] })).unwrap()["handle"].clone());
    app.run("block", json!({ "name": "Tag", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
}

/// A reference to "Tag" at (100, 0) with NO=7, in a view that shows its geometry and attributes.
fn app_with_reference() -> (CadApp, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    define_block(&mut app);
    let r = app.run("insert", json!({ "name": "Tag", "at": [100, 0], "attribs": { "NO": "7" } })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.execute("view.set", &json!({ "center": [104, 1], "height": 8 })).unwrap();
    app.canvas.zoom_pending = false;
    (app, h)
}

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let mut h =
        Harness::builder().with_context_setup(framework::install_headless).with_size(vec2(1600.0, 1000.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut CadApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
    h.run_steps(5);
    h
}

fn double_click(h: &mut Harness<'_, CadApp>, at: Vec2) {
    let pos: Pos2 = h.state().canvas.xf.unwrap().to_screen(at);
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
    }
    h.run_steps(2);
}

/// The middle of the text drawn for attribute `tag` of reference `h`.
fn attribute_centre(app: &CadApp, h: Handle, tag: &str) -> Vec2 {
    let d = app.session.doc().unwrap();
    let EntityKind::Insert(i) = &d.entity(h).unwrap().kind else { panic!("not a block reference") };
    let a = i.attribs.iter().find(|a| a.tag == tag).unwrap();
    let b = match a.multiline_text() {
        Some(m) => cadcraft_render::layout_mtext_entity(d, &m).bounds,
        None => cadcraft_render::place_text_entity(d, &a.text, &a.text.value).1,
    };
    assert!(!b.is_empty(), "the attribute is drawn");
    b.center()
}

fn rows(app: &CadApp) -> Vec<(String, String, String)> {
    app.attedit.rows.iter().map(|r| (r.tag.clone(), r.prompt.clone(), r.value.clone())).collect()
}

fn assert_editor_on(app: &CadApp, reference: Handle) {
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!(app.attedit.handle, Some(reference));
    assert_eq!(app.attedit.block, "Tag");
    assert_eq!(rows(app), [("NO".to_string(), "Number".to_string(), "7".to_string()), ("NOTE".to_string(), "NOTE".to_string(), "n".to_string())]);
    assert_eq!(app.session.state().unwrap().selection, vec![reference], "the reference is picked first");
    assert!(app.session.running.is_none(), "the dialog opens, not the command-line prompts");
}

/// The editor opened on `reference` with the edited attribute value the test set.
fn assert_editor_on_attribute(app: &CadApp, reference: Handle, value: &str) {
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!(app.attedit.handle, Some(reference));
    assert_eq!(app.attedit.block, "Tag");
    assert_eq!(app.attedit.rows.first().map(|r| (r.tag.as_str(), r.value.as_str())), Some(("NO", value)));
    assert_eq!(app.session.state().unwrap().selection, vec![reference], "the reference is picked first");
    assert!(app.session.running.is_none(), "the dialog opens, not the command-line prompts");
}

#[test]
fn double_clicking_a_references_geometry_opens_the_enhanced_attribute_editor() {
    let (app, reference) = app_with_reference();
    let mut h = harness(app);
    double_click(&mut h, Vec2::new(105.0, 0.0));
    assert_editor_on(h.state(), reference);
}

#[test]
fn a_blank_attribute_opens_the_editor_only_on_the_references_geometry() {
    let (mut app, reference) = app_with_reference();
    with_attrib(&mut app, reference, "NO", |a| a.text.value = " ".into());
    with_attrib(&mut app, reference, "NOTE", |a| a.text.value.clear());
    let blank_box_center = attribute_centre(&app, reference, "NO");
    let mut h = harness(app);
    double_click(&mut h, blank_box_center);
    assert!(h.state().ui.dialog.is_none(), "the space advances the cursor but draws no text");
    // Let egui's multi-click window close before the next double-click.
    h.run_steps(30);
    double_click(&mut h, Vec2::new(105.0, 0.0));
    assert_editor_on_attribute(h.state(), reference, " ");
}

#[test]
fn double_clicking_an_attributes_text_opens_the_editor_too() {
    let (app, reference) = app_with_reference();
    let mut h = harness(app);
    // Between the strokes of the text, which lies outside the block's drawn geometry: a pick there
    // finds no object.
    let at = between_strokes(h.state(), reference, "NO", |picked| picked.is_none());
    assert_eq!(cadcraft_engine::cmd::curves::pick_at(&h.state().session, at), None);
    double_click(&mut h, at);
    assert_editor_on(h.state(), reference);
}

/// The first click between an attribute's strokes opens a selection window. When the second
/// press and release arrive in one frame, opening the editor must clear the window.
#[test]
fn opening_the_editor_leaves_no_selection_window_behind() {
    let (mut app, reference) = app_with_reference();
    let ctx = egui::Context::default();
    let screen_rect = Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0)));
    // The window's runtime splits input after each pointer press. Draw only the canvas so it
    // can receive both events in the same frame.
    let frame = |app: &mut CadApp, events, time| {
        app.logic(&ctx);
        let input = egui::RawInput { screen_rect, events, time: Some(time), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| canvas::show(app, ui));
        output.textures_delta.clear();
    };
    frame(&mut app, vec![], 0.0);
    frame(&mut app, vec![], 0.05);
    let at = between_strokes(&app, reference, "NO", |picked| picked.is_none());
    let pos = app.canvas.xf.unwrap().to_screen(at);
    let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame(&mut app, vec![Event::PointerMoved(pos)], 0.1);
    frame(&mut app, vec![button(true)], 0.15);
    frame(&mut app, vec![button(false)], 0.2);
    assert!(app.session.pending_window.is_some(), "the first click opens a window");

    frame(&mut app, vec![button(true), button(false)], 0.25);
    assert_editor_on(&app, reference);
    assert!(app.session.pending_window.is_none(), "opening the editor clears the window");
}

#[test]
fn a_reference_with_only_constant_attributes_opens_no_editor() {
    let (mut app, reference) = app_with_reference();
    // A constant attribute cannot be changed, so the reference has nothing to edit.
    app.session
        .doc_mut()
        .unwrap()
        .modify_entity(reference, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                for a in &mut i.attribs {
                    a.constant = true;
                }
            }
        })
        .unwrap();
    let mut h = harness(app);
    double_click(&mut h, Vec2::new(105.0, 0.0));
    assert!(h.state().ui.dialog.is_none(), "no editable attribute, no editor");
    let at = attribute_centre(h.state(), reference, "NO");
    double_click(&mut h, at);
    assert!(h.state().ui.dialog.is_none(), "not through the constant attribute's text either");
}

#[test]
fn a_minsert_with_attributes_opens_the_editor() {
    let mut app = CadApp::new(Session::new(), Services::default());
    define_block(&mut app);
    let r = app
        .run(
            "minsert",
            json!({ "name": "Tag", "at": [100, 0], "attribs": { "NO": "7" }, "rows": 2, "columns": 2, "rowSpacing": 5, "columnSpacing": 20 }),
        )
        .unwrap();
    let reference = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.execute("view.set", &json!({ "center": [104, 1], "height": 8 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = harness(app);
    double_click(&mut h, Vec2::new(105.0, 0.0));
    assert_editor_on(h.state(), reference);
}

#[test]
fn double_clicking_text_still_opens_the_text_editor() {
    let (mut app, _) = app_with_reference();
    // TEXT drawn over the reference's overall box is still the topmost object there.
    app.run("text", json!({ "at": [103, 0.5], "text": "HELLO", "height": 0.5 })).unwrap();
    let mut h = harness(app);
    double_click(&mut h, Vec2::new(103.5, 0.7));
    let s = &h.state().session;
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("textedit"));
    assert_eq!(s.current_prompt().unwrap().default.as_deref(), Some("HELLO"));
    assert!(h.state().ui.dialog.is_none(), "TEXTEDIT, not the attribute editor");
}

#[test]
fn a_double_click_while_a_command_runs_leaves_the_editor_closed() {
    let (app, _) = app_with_reference();
    let mut h = harness(app);
    h.state_mut().start("line");
    h.run_steps(2);
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("line"));
    double_click(&mut h, Vec2::new(105.0, 0.0));
    assert!(h.state().ui.dialog.is_none(), "a running command keeps the click");
    assert_eq!(h.state().session.running.as_ref().map(|r| r.id.as_str()), Some("line"), "the command goes on");
}

#[test]
fn a_programmatic_attedit_edits_without_a_dialog() {
    let mut app = CadApp::new(Session::new(), Services::default());
    define_block(&mut app);
    let r = app.run("insert", json!({ "name": "Tag", "at": [100, 0], "attribs": { "NO": "7" } })).unwrap();
    let reference = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let changed = app.run("attedit", json!({ "handle": reference.hex(), "values": { "NO": "9" } })).unwrap();
    assert_eq!(changed["changed"], 1);
    assert!(app.ui.dialog.is_none(), "JSON calls never open the dialog");
    let value = match &app.session.doc().unwrap().entity(reference).unwrap().kind {
        EntityKind::Insert(i) => i.attribs.iter().find(|a| a.tag == "NO").unwrap().text.value.clone(),
        other => panic!("not a block reference: {other:?}"),
    };
    assert_eq!(value, "9");
}

/// Change attribute `tag` of reference `r` (the tests set text and properties the commands don't).
fn with_attrib(app: &mut CadApp, r: Handle, tag: &str, f: impl FnOnce(&mut Attrib)) {
    app.session
        .doc_mut()
        .unwrap()
        .modify_entity(r, |e| {
            if let EntityKind::Insert(i) = &mut e.kind
                && let Some(a) = i.attribs.iter_mut().find(|a| a.tag == tag)
            {
                f(a);
            }
        })
        .unwrap();
}

/// The kind of the object a pick at `at` takes, if any.
fn picked_kind(app: &CadApp, at: Vec2) -> Option<EntityKind> {
    let h = cadcraft_engine::cmd::curves::pick_at(&app.session, at)?;
    Some(app.session.doc().ok()?.entity(h)?.kind.clone())
}

/// The box drawn for attribute `tag` of reference `r`.
fn drawn_box(app: &CadApp, r: Handle, tag: &str) -> cadcraft_geom::Bounds2 {
    let d = app.session.doc().unwrap();
    let EntityKind::Insert(i) = &d.entity(r).unwrap().kind else { panic!("not a block reference") };
    let a = i.attribs.iter().find(|a| a.tag == tag).unwrap();
    match a.multiline_text() {
        Some(m) => cadcraft_render::layout_mtext_entity(d, &m).bounds,
        None => cadcraft_render::place_text_entity(d, &a.text, &a.text.value).1,
    }
}

/// A point in the box drawn for attribute `tag` of reference `r` where what the ordinary pick takes
/// (`None` for no object) is what `wanted` accepts. A pick hits a block reference by what it draws,
/// text included, so the middle of an attribute's box is usually on a glyph stroke and the
/// reference itself is taken there. The tests about the double-click's own text hit click between
/// the strokes instead. Corners come first: strokes sit nearer the middle of a box.
fn between_strokes(app: &CadApp, r: Handle, tag: &str, wanted: impl Fn(Option<&EntityKind>) -> bool) -> Vec2 {
    let b = drawn_box(app, r, tag);
    let spots = [0.9, 0.1, 0.5, 0.7, 0.3];
    for fy in spots {
        for fx in spots {
            let p = Vec2::new(b.min.x + b.width() * fx, b.min.y + b.height() * fy);
            if wanted(picked_kind(app, p).as_ref()) {
                return p;
            }
        }
    }
    panic!("no probe of the box {b:?} has the ordinary pick this test needs")
}

#[test]
fn a_text_drawn_over_an_attribute_text_wins_the_double_click() {
    let (mut app, reference) = app_with_reference();
    let at = attribute_centre(&app, reference, "NO");
    // TEXT drawn after the reference, covering its attribute's text: the topmost object there.
    app.run("text", json!({ "at": [at.x - 0.3, at.y - 0.15], "text": "XX", "height": 0.6 })).unwrap();
    assert!(matches!(picked_kind(&app, at), Some(EntityKind::Text(_))), "the TEXT is under the click");
    let mut h = harness(app);
    double_click(&mut h, at);
    let s = &h.state().session;
    assert_eq!(s.running.as_ref().map(|r| r.id.as_str()), Some("textedit"), "the later TEXT takes the click");
    assert!(h.state().ui.dialog.is_none(), "TEXTEDIT, not the attribute editor");
}

#[test]
fn a_reference_drawn_over_a_text_keeps_the_attribute_editor() {
    // The other draw order: TEXT first, the reference over it. The reference is on top, so the
    // double-click edits its attributes, as it did before the draw-order fix.
    let mut app = CadApp::new(Session::new(), Services::default());
    define_block(&mut app);
    app.run("text", json!({ "at": [99.7, 0.95], "text": "XX", "height": 0.6 })).unwrap();
    let r = app.run("insert", json!({ "name": "Tag", "at": [100, 0], "attribs": { "NO": "7" } })).unwrap();
    let reference = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.execute("view.set", &json!({ "center": [104, 1], "height": 8 })).unwrap();
    app.canvas.zoom_pending = false;
    // A taller text leaves room between its strokes, inside the box of the TEXT under it.
    with_attrib(&mut app, reference, "NO", |a| a.text.height = 1.0);
    let mut h = harness(app);
    let at = between_strokes(h.state(), reference, "NO", |picked| matches!(picked, Some(EntityKind::Text(_))));
    assert!(matches!(picked_kind(h.state(), at), Some(EntityKind::Text(_))), "the TEXT is still the object a pick takes");
    double_click(&mut h, at);
    assert_editor_on(h.state(), reference);
}

#[test]
fn double_clicking_fit_attribute_text_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // A one-character attribute stretched from (100,20) to (200,20): the renderer spans both
    // points, which the extents' width estimate (0.9 units) does not.
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.halign = HAlign::Fit;
        a.text.valign = VAlign::Baseline;
        a.text.height = 1.0;
        a.text.width_factor = 1.0;
        a.text.insert = Vec3::new(100.0, 20.0, 0.0);
        a.text.align_pt = Some(Vec3::new(200.0, 20.0, 0.0));
        a.text.value = "7".into();
    });
    let at = Vec2::new(150.0, 20.5);
    assert!(drawn_box(&app, reference, "NO").contains(at), "the click is on the drawn text");
    {
        let d = app.session.doc().unwrap();
        let estimate = cadcraft_engine::doc::entity_bounds(d, d.entity(reference).unwrap(), 0);
        assert!(!estimate.contains(at), "the click is outside the estimated box, which used to reject it");
    }
    assert_eq!(picked_kind(&app, at), None, "no object is picked there");
    app.session.execute("view.set", &json!({ "center": [150, 20], "height": 30 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on(h.state(), reference);
}

#[test]
fn double_clicking_wrapped_multiline_attribute_text_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // Wrapped MTEXT: one paragraph, several drawn lines. The extents count its `\P` breaks, not
    // its wrapped lines, so their box stops at the first line.
    let contents = "AAAAA BBBBB CCCCC DDDDD EEEEE FFFFF";
    let m = MText {
        insert: Vec3::new(100.0, -10.0, 0.0),
        height: 1.0,
        width: 4.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: contents.into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = contents.into();
        a.multiline = Some(MultilineAttrib { mtext: m, read_as: contents.into() });
    });
    let drawn = drawn_box(&app, reference, "NO");
    let at = Vec2::new(drawn.center().x, drawn.min.y + 0.2);
    assert!(drawn.contains(at), "the click is on the last drawn line");
    {
        let d = app.session.doc().unwrap();
        let estimate = cadcraft_engine::doc::entity_bounds(d, d.entity(reference).unwrap(), 0);
        assert!(!estimate.contains(at), "the wrapped lines are below the estimated box: estimate {estimate:?}, drawn {drawn:?}, at {at:?}");
    }
    assert_eq!(picked_kind(&app, at), None, "no object is picked there");
    app.session.execute("view.set", &json!({ "center": [115, -8], "height": 40 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("attedit"));
    assert_eq!(h.state().attedit.handle, Some(reference));
}

#[test]
fn an_attribute_on_a_layer_turned_off_is_not_clickable() {
    let (mut app, reference) = app_with_reference();
    {
        let d = app.session.doc_mut().unwrap();
        d.ensure_layer("HID");
        d.layer_mut("HID").unwrap().on = false;
    }
    with_attrib(&mut app, reference, "NO", |a| a.props.layer = Some("HID".into()));
    let at = attribute_centre(&app, reference, "NO");
    let mut h = harness(app);
    double_click(&mut h, at);
    assert!(h.state().ui.dialog.is_none(), "the renderer does not draw the attribute's text: no editor");
    assert!(h.state().session.running.is_none(), "and no command starts");
}

#[test]
fn an_attribute_frozen_in_the_viewport_is_not_clickable() {
    let (mut app, reference) = app_with_reference();
    app.session.doc_mut().unwrap().ensure_layer("HID");
    with_attrib(&mut app, reference, "NO", |a| {
        a.props.layer = Some("HID".into());
        a.text.insert = Vec3::new(105.0, 1.0, 0.0);
        a.text.height = 1.0;
    });
    // A layout viewport looking at the reference, and MSPACE into it.
    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        id: 2,
        center: Vec3::new(70.0, 60.0, 0.0),
        width: 60.0,
        height: 40.0,
        view_center: Vec2::new(104.0, 1.0),
        view_height: 30.0,
        ..Viewport::default()
    };
    let viewport = app.session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    app.session.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    app.session.execute("view.set", &json!({ "center": [70, 60], "height": 40 })).unwrap();
    app.session.execute("mspace", &json!({})).unwrap();
    // VPLAYER VP Freeze, as the active viewport takes it.
    app.session
        .doc_mut()
        .unwrap()
        .modify_entity(viewport, |e| {
            if let EntityKind::Viewport(v) = &mut e.kind {
                v.frozen_layers = vec!["HID".into()];
            }
        })
        .unwrap();
    app.canvas.zoom_pending = false;
    assert_eq!(app.session.viewport_frozen(), ["HID".to_string()], "the active viewport freezes the attribute's layer");
    let at = {
        let d = app.session.doc().unwrap();
        let EntityKind::Insert(ins) = &d.entity(reference).unwrap().kind else { panic!("not a block reference") };
        let a = ins.attribs.iter().find(|a| a.tag == "NO").unwrap();
        let (shaped, _) = cadcraft_render::place_text_entity(d, &a.text, &a.text.value);
        let stroke = shaped.strokes.iter().find(|s| s.len() >= 2).unwrap();
        (stroke[0] + stroke[1]) / 2.0
    };
    let mut h = harness(app);
    assert_eq!(cadcraft_engine::cmd::curves::pick_at(&h.state().session, at), Some(reference), "ordinary pick still hits the stroke");
    double_click(&mut h, at);
    assert!(h.state().ui.dialog.is_none(), "a stroke on a viewport-frozen layer does not open the editor");
    assert!(h.state().session.running.is_none(), "no command starts");
    h.run_steps(30);
    double_click(&mut h, Vec2::new(108.0, 0.0));
    assert_editor_on(h.state(), reference);
}

#[test]
fn the_attribute_editor_uses_the_viewport_scale_for_reference_geometry() {
    for filler in [0, 300] {
        let (mut app, hidden) = app_with_reference();
        let r = app.run("insert", json!({ "name": "Tag", "at": [135, 0], "attribs": { "NO": "7" } })).unwrap();
        let shown = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        let paper = Space::Paper("Layout1".into());
        let viewport;
        {
            let d = app.session.doc_mut().unwrap();
            d.header.set("CANNOSCALE", HVal::Str("1:1".into()));
            d.header.set("ANNOALLVISIBLE", HVal::Int(0));
            d.modify_entity(hidden, |e| e.common.anno_scales = vec![AnnoScale::unit()]).unwrap();
            d.modify_entity(shown, |e| e.common.anno_scales = vec![AnnoScale::new("1:2", 1.0, 2.0)]).unwrap();
            for i in 0..filler {
                let x = 1_000.0 + f64::from(i) * 3.0;
                let line = cadcraft_engine::doc::Line { a: Vec3::new(x, 500.0, 0.0), b: Vec3::new(x + 1.0, 500.0, 0.0) };
                d.add(&Space::Model, Common::default(), EntityKind::Line(line)).unwrap();
            }
            let vp = Viewport {
                id: 2,
                center: Vec3::new(70.0, 60.0, 0.0),
                width: 60.0,
                height: 40.0,
                view_center: Vec2::new(126.0, 0.0),
                view_height: 80.0,
                ..Viewport::default()
            };
            viewport = d.add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
        }
        app.session.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
        app.session.execute("view.set", &json!({ "center": [70, 60], "height": 40 })).unwrap();
        app.session.execute("mspace", &json!({})).unwrap();
        assert_eq!(app.session.state().unwrap().mspace, Some(viewport));
        app.canvas.zoom_pending = false;
        let hidden_at = Vec2::new(105.0, 0.0);
        let shown_at = Vec2::new(150.0, 0.0);
        assert_eq!(cadcraft_engine::cmd::curves::pick_at(&app.session, hidden_at), Some(hidden), "ordinary pick still uses CANNOSCALE");
        let mut h = harness(app);
        double_click(&mut h, hidden_at);
        assert!(h.state().ui.dialog.is_none(), "the viewport does not show the 1:1 reference");
        h.run_steps(30);
        double_click(&mut h, shown_at);
        assert_editor_on(h.state(), shown);
        assert_eq!(cadcraft_engine::cmd::curves::pick_at(&h.state().session, shown_at), None, "ordinary pick still hides the 1:2 reference");
    }
}

#[test]
fn attribute_text_clipped_away_by_xclip_is_not_clickable() {
    let (mut app, reference) = app_with_reference();
    // XCLIP to the reference's line only: the attributes, drawn above it, are cut away.
    app.session
        .doc_mut()
        .unwrap()
        .modify_entity(reference, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.clip = Some(ClipBoundary::new(vec![Vec2::new(-1.0, -1.0), Vec2::new(11.0, 0.5)]));
            }
        })
        .unwrap();
    let at = attribute_centre(&app, reference, "NO");
    let mut h = harness(app);
    double_click(&mut h, at);
    assert!(h.state().ui.dialog.is_none(), "the clip boundary hides the attribute's text: no editor");
    assert!(h.state().session.running.is_none(), "and no command starts");
}

#[test]
fn double_clicking_a_paper_space_title_block_opens_the_editor() {
    let mut app = CadApp::new(Session::new(), Services::default());
    define_block(&mut app);
    // A layout viewport away from the title block: the double-click outside it edits the sheet.
    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        id: 2,
        center: Vec3::new(150.0, 100.0, 0.0),
        width: 100.0,
        height: 60.0,
        view_center: Vec2::new(0.0, 0.0),
        view_height: 100.0,
        ..Viewport::default()
    };
    app.session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    app.session.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let r = app.run("insert", json!({ "name": "Tag", "at": [30, 30], "attribs": { "NO": "7" } })).unwrap();
    let reference = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.execute("view.set", &json!({ "center": [100, 60], "height": 200 })).unwrap();
    app.canvas.zoom_pending = false;
    let at = attribute_centre(&app, reference, "NO");
    assert!(at.x < 100.0, "the title block is outside the viewport");
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on(h.state(), reference);
}

#[test]
fn a_paper_space_double_click_inside_a_viewport_still_enters_mspace() {
    let mut app = CadApp::new(Session::new(), Services::default());
    define_block(&mut app);
    let paper = Space::Paper("Layout1".into());
    let vp = Viewport {
        id: 2,
        center: Vec3::new(150.0, 100.0, 0.0),
        width: 100.0,
        height: 60.0,
        view_center: Vec2::new(0.0, 0.0),
        view_height: 100.0,
        ..Viewport::default()
    };
    app.session.doc_mut().unwrap().add(&paper, Common::default(), EntityKind::Viewport(vp)).unwrap();
    app.session.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    app.session.execute("view.set", &json!({ "center": [100, 60], "height": 200 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = harness(app);
    double_click(&mut h, Vec2::new(150.0, 100.0));
    let st = h.state().session.state().unwrap();
    assert!(st.mspace.is_some(), "the viewport still enters model space");
    assert!(h.state().ui.dialog.is_none(), "and no attribute editor opens");
}

/// Frame `b` in the view, with room around it, so a double-click on it is on screen.
fn show_box(app: &mut CadApp, b: cadcraft_geom::Bounds2) {
    let height = (b.height().max(b.width() / 1.6)).max(1.0) * 1.4;
    app.session.execute("view.set", &json!({ "center": [b.center().x, b.center().y], "height": height })).unwrap();
    app.canvas.zoom_pending = false;
}

/// A multiline attribute value with its contents, drawn as the renderer lays it out.
fn multiline(value: &str, mtext: MText) -> MultilineAttrib {
    MultilineAttrib { mtext, read_as: value.into() }
}

#[test]
fn double_clicking_multiline_text_with_an_inline_height_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // `\H1000x;` makes the line 1,000 units high, as the renderer draws it. A click on the
    // rendered line is far below the nine bytes the old region estimated.
    let m = MText {
        insert: Vec3::new(100.0, 0.0, 0.0),
        height: 1.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "\\H1000x;A".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = "A".into();
        a.multiline = Some(multiline("A", m));
    });
    let drawn = drawn_box(&app, reference, "NO");
    let value = "A";
    let at = Vec2::new(drawn.center().x, drawn.min.y + 20.0);
    assert!(drawn.contains(at), "the click is on the drawn line: {drawn:?}");
    {
        let d = app.session.doc().unwrap();
        let estimate = cadcraft_engine::doc::entity_bounds(d, d.entity(reference).unwrap(), 0);
        assert!(!estimate.contains(at), "the extent estimate does not reach the rendered line: {estimate:?}, at {at:?}");
    }
    show_box(&mut app, drawn);
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on_attribute(h.state(), reference, value);
}

#[test]
fn double_clicking_multiline_text_with_an_inline_width_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // `\W50;` is the inline width factor, which the extents' style width (1) does not know.
    let m = MText {
        insert: Vec3::new(100.0, 0.0, 0.0),
        height: 1.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "\\W50;AAAAAAAAAA".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = "AAAAAAAAAA".into();
        a.multiline = Some(multiline("AAAAAAAAAA", m));
    });
    let drawn = drawn_box(&app, reference, "NO");
    let value = "AAAAAAAAAA";
    let at = Vec2::new(drawn.max.x - 1.0, drawn.center().y);
    assert!(drawn.contains(at), "the click is on the drawn line: {drawn:?}");
    {
        let d = app.session.doc().unwrap();
        let estimate = cadcraft_engine::doc::entity_bounds(d, d.entity(reference).unwrap(), 0);
        assert!(!estimate.contains(at), "the click is past the extent estimate: {estimate:?}, at {at:?}");
    }
    show_box(&mut app, drawn);
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on_attribute(h.state(), reference, value);
}

#[test]
fn double_clicking_multiline_text_with_wide_line_spacing_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // Six lines at four times the line spacing: the last line is farther from the insertion
    // point than a byte-count estimate covers.
    let contents = "A\\PB\\PC\\PD\\PE\\PF";
    let m = MText {
        insert: Vec3::new(100.0, 0.0, 0.0),
        height: 1.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: contents.into(),
        line_spacing: 4.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = "A\nB\nC\nD\nE\nF".into();
        a.multiline = Some(multiline("A\nB\nC\nD\nE\nF", m));
    });
    let drawn = drawn_box(&app, reference, "NO");
    let value = "A\nB\nC\nD\nE\nF";
    let at = Vec2::new(drawn.center().x, drawn.min.y + 0.5);
    assert!(drawn.contains(at), "the click is on the last drawn line: {drawn:?}");
    show_box(&mut app, drawn);
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on_attribute(h.state(), reference, value);
}

#[test]
fn double_clicking_multiline_text_with_a_repaired_height_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // A non-positive height: the layout repairs it to 1, as the renderer draws it. The old
    // region rejected it outright.
    let m = MText {
        insert: Vec3::new(100.0, 0.0, 0.0),
        height: 0.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "A".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = "A".into();
        a.multiline = Some(multiline("A", m));
    });
    let drawn = drawn_box(&app, reference, "NO");
    let value = "A";
    let at = drawn.center();
    assert!(drawn.contains(at), "the click is on the drawn line: {drawn:?}");
    show_box(&mut app, drawn);
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on_attribute(h.state(), reference, value);
}

#[test]
fn double_clicking_long_multiline_text_opens_the_editor() {
    let (mut app, reference) = app_with_reference();
    // Over 10,000 bytes is valid text, drawn like any other; the old byte guard made it
    // unclickable rather than bounding the work.
    let contents = "A".repeat(12_000);
    let m = MText {
        insert: Vec3::new(100.0, 0.0, 0.0),
        height: 0.1,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: contents.clone(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = contents.clone();
        a.multiline = Some(multiline(&contents, m));
    });
    let drawn = drawn_box(&app, reference, "NO");
    let value = contents.as_str();
    let at = Vec2::new(drawn.max.x - 50.0, drawn.center().y);
    assert!(drawn.contains(at), "the click is on the drawn line: {drawn:?}");
    {
        let d = app.session.doc().unwrap();
        let estimate = cadcraft_engine::doc::entity_bounds(d, d.entity(reference).unwrap(), 0);
        assert!(!estimate.contains(at), "the click is past the extent estimate: {estimate:?}, at {at:?}");
    }
    show_box(&mut app, drawn);
    let mut h = harness(app);
    double_click(&mut h, at);
    assert_editor_on_attribute(h.state(), reference, value);
}

#[test]
fn a_text_style_change_is_seen_by_the_next_double_click() {
    let (mut app, reference) = app_with_reference();
    // Unwrapped multiline text: its width follows its text style's width factor.
    let m = MText {
        insert: Vec3::new(100.0, 0.0, 0.0),
        height: 1.0,
        width: 0.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "AAAA".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    with_attrib(&mut app, reference, "NO", |a| {
        a.text.value = "AAAA".into();
        a.multiline = Some(multiline("AAAA", m));
    });
    let narrow = drawn_box(&app, reference, "NO");
    app.session.execute("view.set", &json!({ "center": [120, 0], "height": 40 })).unwrap();
    app.canvas.zoom_pending = false;
    let mut h = harness(app);
    // A double-click on empty space first, as a user's earlier clicks would.
    double_click(&mut h, Vec2::new(130.0, 15.0));
    assert!(h.state().ui.dialog.is_none(), "nothing is under that click");
    // The style draws ten times wider now: nothing about the reference itself changed.
    h.state_mut().run("style", json!({ "name": "Standard", "widthFactor": 10 })).unwrap();
    let wide = drawn_box(h.state(), reference, "NO");
    let at = Vec2::new(f64::midpoint(narrow.max.x, wide.max.x), wide.center().y);
    assert!(wide.contains(at) && !narrow.expand(2.0).contains(at), "the click is on the widened text only: {narrow:?} {wide:?} {at:?}");
    // Half a second on: egui would take clicks this soon after the last double-click as a
    // triple click.
    h.run_steps(30);
    double_click(&mut h, at);
    assert_editor_on_attribute(h.state(), reference, "AAAA");
}
