//! Block attributes from the UI: Edit Attributes on a selected block reference opens the Enhanced
//! Attribute Editor, and the Properties palette lists the attributes in their own group.

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, Handle};
use cadcraft_ui_egui::{CadApp, Services, attedit, context_menu, menus};
use serde_json::json;

/// A block "Tag" with attributes NO (prompt "Number") and NOTE, and one reference to it.
fn app_with_reference() -> (CadApp, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let h = add_reference(&mut app);
    (app, h)
}

/// Block "Tag" and its reference, as [`app_with_reference`], in the active drawing (selected).
fn add_reference(app: &mut CadApp) -> Handle {
    let mut hs = vec![app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone()];
    hs.push(app.run("attdef", json!({ "tag": "NO", "prompt": "Number", "default": "1", "at": [0, 1] })).unwrap()["handle"].clone());
    hs.push(app.run("attdef", json!({ "tag": "NOTE", "default": "n", "at": [0, 2] })).unwrap()["handle"].clone());
    app.run("block", json!({ "name": "Tag", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
    let r = app.run("insert", json!({ "name": "Tag", "at": [100, 0], "attribs": { "NO": "7" } })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.set_selection(vec![h]);
    h
}

fn value(app: &CadApp, h: Handle, tag: &str) -> String {
    match app.session.doc().unwrap().entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => i.attribs.iter().find(|a| a.tag == tag).unwrap().text.value.clone(),
        other => panic!("{other:?}"),
    }
}

/// The text painted in one frame.
fn painted(app: &mut CadApp, ctx: &egui::Context) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, text)),
            _ => {}
        }
    }
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    let mut text = String::new();
    for s in &output.shapes {
        collect(&s.shape, &mut text);
    }
    text
}

/// The application in an egui harness at a size that shows the Properties palette.
fn palette(app: CadApp) -> egui_kittest::Harness<'static, CadApp> {
    egui_kittest::Harness::builder().with_size(egui::vec2(1600.0, 1000.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    )
}

/// Focus the attribute field showing `shown` and key `text` into it, leaving it uncommitted.
fn type_pending(harness: &mut egui_kittest::Harness<'_, CadApp>, shown: &str, text: &str) {
    use egui_kittest::kittest::Queryable;
    let field =
        harness.get_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.value().as_deref() == Some(shown)).expect("the attribute's field");
    field.focus();
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text(text.into()));
    harness.run();
}

/// The texts the text fields show, the command line's included.
fn fields(harness: &egui_kittest::Harness<'_, CadApp>) -> Vec<String> {
    use egui_kittest::kittest::Queryable;
    let mut vs: Vec<String> = harness.get_all_by_role(egui::accesskit::Role::TextInput).filter_map(|n| n.value()).collect();
    vs.sort();
    vs
}

/// Whether the palette's fields show every one of `values`, and no `gone`.
fn shows(harness: &egui_kittest::Harness<'_, CadApp>, values: &[&str], gone: &str) -> bool {
    let now = fields(harness);
    values.iter().all(|v| now.iter().any(|f| f == v)) && !now.iter().any(|f| f == gone)
}

/// A dropped pending value is reported on the command line, as a rejected Properties value is.
fn told_the_value_was_not_applied(harness: &egui_kittest::Harness<'_, CadApp>) -> bool {
    harness.state().session.log.iter().any(|l| l.contains("the value was not applied"))
}

#[test]
fn edit_attributes_on_a_selected_reference_opens_the_dialog() {
    let (mut app, h) = app_with_reference();
    let labels: Vec<_> = context_menu::object_entries(&app)
        .into_iter()
        .filter_map(|e| match e {
            menus::Entry::Item { label, .. } => Some(label),
            menus::Entry::Sub { .. } => None,
        })
        .collect();
    assert!(labels.iter().any(|l| l == "Edit Attributes..."), "{labels:?}");

    menus::activate(&mut app, "attedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!(app.attedit.block, "Tag");
    let rows: Vec<_> = app.attedit.rows.iter().map(|r| (r.tag.as_str(), r.prompt.as_str(), r.value.as_str())).collect();
    assert_eq!(rows, [("NO", "Number", "7"), ("NOTE", "NOTE", "n")]);
    assert!(app.session.running.is_none(), "the dialog, not the command-line prompts");

    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    for s in ["Enhanced Attribute Editor", "Number", "NOTE", "OK", "Apply"] {
        assert!(text.contains(s), "{s} missing from:\n{text}");
    }

    // Apply changes only what was typed, in one undo step, and keeps the dialog open.
    app.attedit.rows[1].typed = "a note".into();
    assert!(attedit::apply(&mut app));
    assert_eq!((value(&app, h, "NO"), value(&app, h, "NOTE")), ("7".into(), "a note".into()));
    assert_eq!(app.attedit.rows[1].value, "a note", "rows reload after Apply");
    app.run("undo", json!({})).unwrap();
    assert_eq!(value(&app, h, "NOTE"), "n");
}

/// `attedit::open` (a double-click on an attribute) opens the dialog on a reference with a
/// chosen attribute current; its value box takes the focus in the first frame.
#[test]
fn open_makes_the_chosen_attribute_current() {
    let (mut app, h) = app_with_reference();
    app.session.set_selection(Vec::new());
    assert!(attedit::open(&mut app, h, Some(1)));
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!((app.attedit.current, app.attedit.focus), (Some(1), true));
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    assert!(!app.attedit.focus, "the focus was given in the first frame");
    assert!(ctx.memory(|m| m.focused().is_some()), "a value box has the focus");
    assert_eq!(app.attedit.current, Some(1));

    // Apply keeps the current attribute.
    app.attedit.rows[1].typed = "x".into();
    assert!(attedit::apply(&mut app));
    assert_eq!(app.attedit.current, Some(1));

    // No attribute, or one out of range: the first that can be changed.
    assert!(attedit::open(&mut app, h, None));
    assert_eq!(app.attedit.current, Some(0));
    assert!(attedit::open(&mut app, h, Some(99)));
    assert_eq!(app.attedit.current, Some(0));

    // Not a block reference with attributes: nothing opens.
    app.ui.dialog = None;
    let line = app.run("line", json!({ "points": [[0, 0], [1, 1]] })).unwrap()["handles"][0].as_str().map(str::to_string).unwrap();
    assert!(!attedit::open(&mut app, Handle::parse_hex(&line).unwrap(), Some(0)));
    assert!(!attedit::open(&mut app, Handle(0xFFFF_FFFF), None));
    assert!(app.ui.dialog.is_none());
}

#[test]
fn without_a_selected_reference_attedit_asks_at_the_command_line() {
    let (mut app, _) = app_with_reference();
    app.session.set_selection(Vec::new());
    app.start("eattedit");
    assert!(app.ui.dialog.is_none());
    assert!(app.session.prompt_text().contains("Select block reference"), "{}", app.session.prompt_text());
    app.session.cancel();

    // JSON calls never open the dialog.
    let (mut app, h) = app_with_reference();
    app.run("attedit", json!({ "handle": h.hex(), "values": { "NO": "9" } })).unwrap();
    assert!(app.ui.dialog.is_none());
    assert_eq!(value(&app, h, "NO"), "9");
}

/// A reference on a layer frozen in the active layout viewport is not drawn there, so it is not
/// the selected reference the dialog opens on; one the viewport does draw still opens it beside
/// the hidden one.
#[test]
fn the_active_viewport_decides_which_selected_reference_attedit_opens() {
    let (mut app, hidden) = app_with_reference();
    app.run("layer.new", json!({ "name": "Hidden" })).unwrap();
    app.run("properties.set", json!({ "handles": [hidden.hex()], "layer": "Hidden" })).unwrap();
    let shown = Handle::parse_hex(app.run("insert", json!({ "name": "Tag", "at": [300, 0] })).unwrap()["handle"].as_str().unwrap()).unwrap();
    app.run("layout.set", json!({ "name": "Layout1" })).unwrap();
    let vp = app.run("mview", json!({ "p1": [20, 20], "p2": [220, 120] })).unwrap()["viewports"][0].as_str().unwrap().to_string();
    app.run("viewport.set", json!({ "handle": vp, "freeze": ["Hidden"] })).unwrap();
    app.run("mspace", json!({ "handle": vp })).unwrap();
    assert_eq!(app.session.viewport_frozen(), ["Hidden".to_string()]);

    // The stored selection holds only the reference the viewport does not draw.
    app.session.set_selection(vec![hidden]);
    assert!(app.session.selection().is_empty());
    menus::activate(&mut app, "attedit");
    assert!(app.ui.dialog.is_none(), "a reference the viewport does not draw must not open the dialog");
    assert!(app.session.prompt_text().contains("Select block reference"), "{}", app.session.prompt_text());
    app.session.cancel();

    // Beside the hidden one, the one the viewport draws is the selected reference.
    app.session.set_selection(vec![hidden, shown]);
    assert_eq!(app.session.selection(), [shown]);
    menus::activate(&mut app, "attedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    assert_eq!(app.attedit.handle, Some(shown), "the reference the viewport draws");
}

#[test]
fn the_properties_palette_lists_the_attributes() {
    let (mut app, _) = app_with_reference();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    let text = painted(&mut app, &ctx);
    // (Text fields' contents are not painted shapes here, so only the labels are checked.)
    for s in ["Attributes", "NO", "NOTE"] {
        assert!(text.lines().any(|l| l == s), "{s} missing from:\n{text}");
    }
}

/// A value typed into a Properties attribute field reaches the drawing, as one undo step.
#[test]
fn a_properties_value_is_set_and_undone_as_one_step() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "n", "EG");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(value(harness.state(), h, "NOTE"), "EG");
    harness.state_mut().run("undo", json!({})).unwrap();
    assert_eq!(value(harness.state(), h, "NOTE"), "n", "one undo puts the value back");
}

/// The dialog belongs to the drawing it was opened for: with another drawing's tab chosen (where
/// the same handle is a reference with the same tags), Apply changes nothing and the dialog
/// closes.
#[test]
fn the_dialog_closes_in_another_drawing() {
    let (mut app, h) = app_with_reference();
    menus::activate(&mut app, "attedit");
    assert_eq!(app.ui.dialog.as_deref(), Some("attedit"));
    app.attedit.rows[1].typed = "a note".into();

    app.session.new_drawing(false);
    assert_eq!(add_reference(&mut app), h, "the same handle in the other drawing");
    assert!(!attedit::apply(&mut app));
    assert_eq!(value(&app, h, "NOTE"), "n", "the other drawing's reference is untouched");
    let ctx = egui::Context::default();
    app.logic(&ctx);
    painted(&mut app, &ctx);
    assert!(app.ui.dialog.is_none(), "closed");

    // Back in the first drawing, its reference is as it was too.
    app.session.active = 0;
    assert_eq!(value(&app, h, "NOTE"), "n");
}

/// Rows are addressed by place, so a dialog opened before the attributes were reordered must not
/// write its typed value into whichever attribute now has that place: Apply reloads and says so.
#[test]
fn a_stale_dialog_does_not_overwrite_another_attribute() {
    let (mut app, h) = app_with_reference();
    menus::activate(&mut app, "attedit");
    app.attedit.rows[1].typed = "EG".into();
    app.run("battman", json!({ "name": "Tag", "action": "order", "order": ["NOTE", "NO"] })).unwrap();
    assert!(!attedit::apply(&mut app));
    assert_eq!((value(&app, h, "NO"), value(&app, h, "NOTE")), ("7".into(), "n".into()), "nothing was written");
    assert!(app.attedit.error.is_some());
    let rows: Vec<_> = app.attedit.rows.iter().map(|r| (r.tag.as_str(), r.typed.as_str())).collect();
    assert_eq!(rows, [("NOTE", "n"), ("NO", "7")], "the dialog shows the drawing's current order");

    // Typing again after the reload applies to the right attribute.
    app.attedit.rows[0].typed = "EG".into();
    assert!(attedit::apply(&mut app));
    assert_eq!((value(&app, h, "NO"), value(&app, h, "NOTE")), ("7".into(), "EG".into()));

    // The same through undo: the dialog opens on the reordered attributes, then the order is undone.
    menus::activate(&mut app, "attedit");
    app.attedit.rows[0].typed = "again".into();
    app.run("undo", json!({})).unwrap();
    app.run("undo", json!({})).unwrap();
    assert!(!attedit::apply(&mut app));
    assert_eq!(value(&app, h, "NO"), "7");
}

/// A stale Apply is refused in the dialog and on the command line, so the refusal is not left
/// only in a window another command can cover.
#[test]
fn a_stale_dialog_refusal_reaches_the_command_line() {
    let (mut app, _) = app_with_reference();
    menus::activate(&mut app, "attedit");
    app.attedit.rows[1].typed = "EG".into();
    app.run("battman", json!({ "name": "Tag", "action": "order", "order": ["NOTE", "NO"] })).unwrap();
    assert!(!attedit::apply(&mut app));
    let error = app.attedit.error.clone().unwrap_or_default();
    assert!(error.contains("The attributes changed"), "the dialog says so: {error}");
    assert!(
        app.session.log.iter().any(|l| l.contains("The attributes changed while this dialog was open")),
        "the refusal is on the command line: {:?}",
        app.session.log.last()
    );
}

/// The Properties palette's pending text is the drawing's: typed in one drawing and committed
/// after another (with the same handle and tags) is chosen, it must not be applied there.
#[test]
fn a_pending_properties_value_does_not_follow_a_drawing_switch() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "n", "EG");

    harness.state_mut().session.new_drawing(false);
    assert_eq!(add_reference(harness.state_mut()), h);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(value(harness.state(), h, "NOTE"), "n", "the other drawing's reference is untouched");
}

/// A row is a place, not a name: a reorder under text typed into a Properties field must not
/// give that text to whichever attribute now has the place.
#[test]
fn a_pending_properties_value_does_not_follow_a_reorder() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "n", "EG");
    harness.state_mut().run("battman", json!({ "name": "Tag", "action": "order", "order": ["NOTE", "NO"] })).unwrap();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!((value(harness.state(), h, "NO"), value(harness.state(), h, "NOTE")), ("7".into(), "n".into()), "nothing was written");
    assert!(shows(&harness, &["7", "n"], "EG"), "the fields show the drawing's values again: {:?}", fields(&harness));
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
}

/// Text waiting in a Properties field does not overwrite a value edited from elsewhere.
#[test]
fn a_pending_properties_value_does_not_overwrite_an_edit_from_elsewhere() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "n", "EG");
    harness.state_mut().run("attedit", json!({ "handle": h.hex(), "at": { "1": "other" } })).unwrap();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(value(harness.state(), h, "NOTE"), "other", "the value edited from elsewhere stands");
    assert!(shows(&harness, &["7", "other"], "EG"), "the field shows the value edited from elsewhere: {:?}", fields(&harness));
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
}

/// Text waiting in a Properties field is reported when the drawing changes the value so that the
/// field can no longer edit it (here it gains a line break), rather than left unseen.
#[test]
fn a_pending_properties_value_goes_stale_when_the_value_gains_a_line_break() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "n", "EG");
    harness.state_mut().run("attedit", json!({ "handle": h.hex(), "at": { "1": "other\nline" } })).unwrap();
    harness.run();
    assert_eq!(value(harness.state(), h, "NOTE"), "other\nline", "the value edited from elsewhere stands");
    assert!(!fields(&harness).iter().any(|f| f == "EG"), "the stale text is no longer waiting: {:?}", fields(&harness));
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
    // Nothing is kept for a later Enter either.
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(value(harness.state(), h, "NOTE"), "other\nline", "nothing was applied");
}

/// Undo can put an older attribute, with its older value, where the typed-into one was: the
/// pending text must not go to it.
#[test]
fn a_pending_properties_value_does_not_follow_an_undo() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.state_mut().run("battman", json!({ "name": "Tag", "action": "order", "order": ["NOTE", "NO"] })).unwrap();
    harness.run();
    // The first row is NOTE now, and its text is left uncommitted.
    type_pending(&mut harness, "n", "EG");
    harness.state_mut().run("undo", json!({})).unwrap();
    // Undo leaves nothing selected; the reference is selected again before the next frame, so the
    // field with the pending text is drawn (and keeps its keyboard focus) over the older attributes.
    harness.state_mut().session.set_selection(vec![h]);
    let rows: Vec<_> = cadcraft_engine::attributes(harness.state().session.doc().unwrap(), h).into_iter().map(|a| (a.tag, a.value)).collect();
    assert_eq!(rows, [("NO".to_string(), "7".to_string()), ("NOTE".to_string(), "n".to_string())], "the undo put the order back");
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!((value(harness.state(), h, "NO"), value(harness.state(), h, "NOTE")), ("7".into(), "n".into()), "nothing was written");
    assert!(shows(&harness, &["7", "n"], "EG"), "the fields show the drawing's values again: {:?}", fields(&harness));
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
}

/// A block "Twin" with two editable `T=v` attribute definitions at different places, and one
/// reference to it (selected): the two attributes share a tag and a value, so only where their
/// texts are drawn tells them apart.
fn app_with_twins() -> (CadApp, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let mut hs = vec![app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone()];
    hs.push(app.run("attdef", json!({ "tag": "T", "default": "v", "at": [0, 1] })).unwrap()["handle"].clone());
    hs.push(app.run("attdef", json!({ "tag": "T", "default": "v", "at": [0, 2] })).unwrap()["handle"].clone());
    app.run("block", json!({ "name": "Twin", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
    let r = app.run("insert", json!({ "name": "Twin", "at": [100, 0] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.set_selection(vec![h]);
    (app, h)
}

/// Every attribute's value by where its text is drawn, top to bottom.
fn values_by_place(app: &CadApp, h: Handle) -> Vec<(f64, String)> {
    match app.session.doc().unwrap().entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => {
            let mut v: Vec<(f64, String)> = i.attribs.iter().map(|a| (a.text.insert.y, a.text.value.clone())).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v
        }
        other => panic!("{other:?}"),
    }
}

/// Two attributes with one tag and one value are two attributes: a reorder under text typed into
/// the first's Properties field must not give that text to the other one.
#[test]
fn equal_tag_and_value_attributes_are_told_apart_by_place() {
    let (app, h) = app_with_twins();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "v", "EG");
    harness.state_mut().run("battman", json!({ "name": "Twin", "action": "down", "tag": "T" })).unwrap();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(values_by_place(harness.state(), h), [(1.0, "v".to_string()), (2.0, "v".to_string())], "neither attribute changed");
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
}

/// The Enhanced Attribute Editor draws the same line: a reorder that leaves every tag and value
/// in place, but moves the attributes, is a stale dialog too.
#[test]
fn a_stale_dialog_tells_equal_tag_and_value_attributes_apart_by_place() {
    let (mut app, h) = app_with_twins();
    menus::activate(&mut app, "attedit");
    assert_eq!(app.attedit.rows.len(), 2);
    app.attedit.rows[0].typed = "EG".into();
    app.run("battman", json!({ "name": "Twin", "action": "down", "tag": "T" })).unwrap();
    assert!(!attedit::apply(&mut app), "the dialog refuses the stale row");
    assert_eq!(values_by_place(&app, h), [(1.0, "v".to_string()), (2.0, "v".to_string())], "nothing was written");
    let error = app.attedit.error.clone().unwrap_or_default();
    assert!(error.contains("The attributes changed"), "the dialog says so: {error}");
}

/// A block "Twin" with two editable `T=v` attribute definitions at the same insertion point, their
/// texts 1 and 2 high, and one reference to it (selected): the two attributes share every field
/// the palette and the dialog show, so only a property they don't show tells them apart.
fn app_with_same_place_twins() -> (CadApp, Handle) {
    let mut app = CadApp::new(Session::new(), Services::default());
    let mut hs = vec![app.run("line", json!({ "points": [[0, 0], [10, 0]] })).unwrap()["handles"][0].clone()];
    hs.push(app.run("attdef", json!({ "tag": "T", "default": "v", "at": [0, 1], "height": 1 })).unwrap()["handle"].clone());
    hs.push(app.run("attdef", json!({ "tag": "T", "default": "v", "at": [0, 1], "height": 2 })).unwrap()["handle"].clone());
    app.run("block", json!({ "name": "Twin", "base": [0, 0], "handles": hs, "keep": "delete" })).unwrap();
    let r = app.run("insert", json!({ "name": "Twin", "at": [100, 0] })).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    app.session.set_selection(vec![h]);
    (app, h)
}

/// Every attribute's value by the height its text is drawn at, smallest first: the property that
/// tells [`app_with_same_place_twins`]'s two attributes apart after a reorder.
fn values_by_height(app: &CadApp, h: Handle) -> Vec<(f64, String)> {
    match app.session.doc().unwrap().entity(h).map(|e| &e.kind) {
        Some(EntityKind::Insert(i)) => {
            let mut v: Vec<(f64, String)> = i.attribs.iter().map(|a| (a.text.height, a.text.value.clone())).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v
        }
        other => panic!("{other:?}"),
    }
}

/// A reorder can leave every field the palette shows equal — tag, prompt, value and insertion point
/// — and still put another attribute in the place: the text heights differ. Text waiting in the
/// first field must not go to the attribute now in that place.
#[test]
fn a_pending_properties_value_does_not_follow_a_reorder_only_a_hidden_property_shows() {
    let (app, h) = app_with_same_place_twins();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "v", "EG");
    harness.state_mut().run("battman", json!({ "name": "Twin", "action": "down", "tag": "T" })).unwrap();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(values_by_height(harness.state(), h), [(1.0, "v".to_string()), (2.0, "v".to_string())], "neither attribute changed");
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
}

/// The Enhanced Attribute Editor draws the same line: a reorder that leaves every field it shows
/// in place, but swaps the attributes' text heights, is a stale dialog too.
#[test]
fn a_stale_dialog_tells_attributes_apart_by_a_property_it_does_not_show() {
    let (mut app, h) = app_with_same_place_twins();
    menus::activate(&mut app, "attedit");
    assert_eq!(app.attedit.rows.len(), 2);
    app.attedit.rows[0].typed = "EG".into();
    app.run("battman", json!({ "name": "Twin", "action": "down", "tag": "T" })).unwrap();
    assert!(!attedit::apply(&mut app), "the dialog refuses the stale row");
    assert_eq!(values_by_height(&app, h), [(1.0, "v".to_string()), (2.0, "v".to_string())], "nothing was written");
    let error = app.attedit.error.clone().unwrap_or_default();
    assert!(error.contains("The attributes changed"), "the dialog says so: {error}");
}

/// Input queued for the frame that finds the row has another attribute belongs to the attribute
/// the field had: neither its text nor its Enter may reach the one now in that place, and nothing
/// may be kept for a later Enter either.
#[test]
fn input_in_the_frame_a_pending_value_goes_stale_reaches_no_attribute() {
    let (app, h) = app_with_reference();
    let mut harness = palette(app);
    harness.run();
    type_pending(&mut harness, "n", "EG");
    harness.state_mut().run("battman", json!({ "name": "Tag", "action": "order", "order": ["NOTE", "NO"] })).unwrap();
    // A text event and Enter, in the one frame that finds NOTE's field holding NO.
    harness.input_mut().events.push(egui::Event::Text("ZZ".into()));
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
        repeat: false,
        physical_key: None,
    });
    harness.run();
    assert_eq!((value(harness.state(), h, "NO"), value(harness.state(), h, "NOTE")), ("7".into(), "n".into()), "nothing was written");
    assert!(told_the_value_was_not_applied(&harness), "the user is told: {:?}", harness.state().session.log.last());
    // No pending buffer is left under the attribute now in that place: a later Enter writes nothing.
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!((value(harness.state(), h, "NO"), value(harness.state(), h, "NOTE")), ("7".into(), "n".into()), "nothing was kept for a later Enter");
    assert!(shows(&harness, &["7", "n"], "7ZZ"), "the fields show the drawing's values: {:?}", fields(&harness));
    // Normal editing resumes on the row, with the attribute it now holds.
    type_pending(&mut harness, "7", "ok");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        (value(harness.state(), h, "NO"), value(harness.state(), h, "NOTE")),
        ("ok".into(), "n".into()),
        "a later edit reaches the right attribute"
    );
}
