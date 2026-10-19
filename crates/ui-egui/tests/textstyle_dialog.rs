//! The Text Style dialog: STYLE typed or from the Format menu opens it (JSON calls and -STYLE
//! don't); edits wait for Apply, and leaving them asks first; the font is picked by family and
//! style; Set Current, New, Rename and Delete.

use cadcraft_engine::Session;
use cadcraft_engine::doc::TextStyle;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use serde_json::json;

#[test]
fn style_typed_or_from_the_menu_opens_the_dialog() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for typed in ["STYLE", "style", "ST"] {
        app.ui.dialog = None;
        assert!(app.cmdline(typed).is_none(), "bare {typed} is not an error");
        assert_eq!(app.ui.dialog.as_deref(), Some("textstyle"), "{typed}");
    }
    app.ui.dialog = None;
    menus::activate(&mut app, "style");
    assert_eq!(app.ui.dialog.as_deref(), Some("textstyle"));
    // Programmatic calls and the command-line version never open it.
    app.ui.dialog = None;
    app.run("style", json!({ "name": "Notes", "current": false })).unwrap();
    assert!(app.ui.dialog.is_none());
    assert!(app.cmdline("-STYLE").is_none());
    assert!(app.ui.dialog.is_none());
    assert!(app.session.prompt_text().contains("Enter name of text style"), "{}", app.session.prompt_text());
}

/// The dialog open on a new drawing after `setup`.
fn dialog(setup: impl FnOnce(&mut CadApp)) -> Harness<'static, CadApp> {
    harness(setup, false)
}

/// `dialog`, drawn with the GPU renderer and the dark theme when `render`, for pictures.
fn harness(setup: impl FnOnce(&mut CadApp), render: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    setup(&mut app);
    app.ui.dialog = Some("textstyle".into());
    let builder = Harness::builder().with_size(vec2(1000.0, 560.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        move |ui, app: &mut CadApp| {
            if render {
                cadcraft_ui_egui::theme::apply(ui.ctx(), egui::Theme::Dark, false);
            }
            app.logic(&ui.ctx().clone());
            dialogs::show(app, ui.ctx());
        },
        app,
    );
    h.run_steps(4);
    h
}

/// Clicks the button (or list entry) `label`.
fn click(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get(by().label(label).role(Role::Button)).click();
    h.run_steps(3);
}

/// Ticks or clears the checkbox `label`.
fn check(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get(by().label(label).role(Role::CheckBox)).click();
    h.run_steps(3);
}

fn enabled(h: &Harness<'static, CadApp>, label: &str) -> bool {
    !h.get(by().label(label).role(Role::Button)).accesskit_node().is_disabled()
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(Role::TextInput).value(shown))
}

fn type_into(h: &mut Harness<'static, CadApp>, shown: &str, text: &str) {
    field(h, shown).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, Key::A);
    h.run_steps(1);
    field(h, shown).type_text(text);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

fn style(h: &Harness<'static, CadApp>, name: &str) -> TextStyle {
    h.state().session.doc().unwrap().text_style(name).cloned().unwrap()
}

#[test]
fn edits_wait_for_apply() {
    let mut h = dialog(|_| {});
    assert!(!enabled(&h, "Apply"));
    type_into(&mut h, "1", "0.8");
    check(&mut h, "Upside down");
    assert_eq!(style(&h, "Standard").width_factor, 1.0, "nothing changes before Apply");
    assert!(enabled(&h, "Apply"));
    click(&mut h, "Apply");
    let st = style(&h, "Standard");
    assert_eq!(st.width_factor, 0.8);
    assert!(st.upside_down);
    assert!(!enabled(&h, "Apply"));
    // Apply is one undo step.
    h.state_mut().session.undo().unwrap();
    assert_eq!(style(&h, "Standard").width_factor, 1.0);
}

#[test]
fn leaving_edits_asks_first() {
    let mut h = dialog(|app| {
        app.run("style", json!({ "name": "Notes", "current": false })).unwrap();
    });
    type_into(&mut h, "0", "12");
    click(&mut h, "Notes");
    h.get_by_label("The style Standard has been modified. Do you want to save your changes?");
    click(&mut h, "Cancel");
    assert!(enabled(&h, "Apply"), "still editing Standard");
    click(&mut h, "Notes");
    click(&mut h, "No");
    assert_eq!(style(&h, "Standard").oblique, 0.0, "the edits were dropped");
    // Now Notes is chosen: an edit, then Close, then Yes saves it.
    check(&mut h, "Backwards");
    click(&mut h, "Close");
    click(&mut h, "Yes");
    assert!(style(&h, "Notes").backwards);
    assert!(h.state().ui.dialog.is_none(), "closed");
}

#[test]
fn annotative_styles_have_a_paper_height() {
    let mut h = dialog(|_| {});
    h.get_by_label("Height");
    check(&mut h, "Annotative");
    h.get_by_label("Paper Text Height");
    type_into(&mut h, "0.0000", "0.125");
    click(&mut h, "Apply");
    let st = style(&h, "Standard");
    assert!(st.annotative);
    assert_eq!(st.height, 0.125);
}

#[test]
fn new_copies_the_chosen_style() {
    let mut h = dialog(|app| {
        app.run("style", json!({ "name": "Notes", "widthFactor": 0.75, "current": false })).unwrap();
    });
    click(&mut h, "Notes");
    click(&mut h, "New...");
    field(&h, "style1");
    click(&mut h, "OK");
    let st = style(&h, "style1");
    assert_eq!(st.width_factor, 0.75);
    assert_eq!(h.state().session.doc().unwrap().header.str("TEXTSTYLE", ""), "Standard", "not made current");
    // A name in use can't be taken.
    click(&mut h, "New...");
    type_into(&mut h, "style2", "Notes");
    h.get_by_label("A text style with this name already exists.");
    assert!(!enabled(&h, "OK"));
    click(&mut h, "Cancel");
}

#[test]
fn set_current_rename_and_delete() {
    let mut h = dialog(|app| {
        app.run("style", json!({ "name": "Spare", "current": false })).unwrap();
        app.run("style", json!({ "name": "Unused", "current": false })).unwrap();
        // New text takes the current style.
        app.run("style", json!({ "name": "Used" })).unwrap();
        app.run("text", json!({ "at": [0, 0], "text": "x", "height": 1 })).unwrap();
        app.run("style.current", json!({ "name": "Standard" })).unwrap();
    });
    // Standard, the current style and styles in use can't be deleted.
    assert!(!enabled(&h, "Delete"));
    click(&mut h, "Used");
    assert!(!enabled(&h, "Delete"));
    click(&mut h, "Spare");
    click(&mut h, "Set Current");
    assert_eq!(h.state().session.doc().unwrap().header.str("TEXTSTYLE", ""), "Spare");
    assert!(!enabled(&h, "Delete"));
    click(&mut h, "Standard");
    assert!(!enabled(&h, "Delete"));
    h.state_mut().run("style.current", json!({ "name": "Standard" })).unwrap();
    h.run_steps(3);
    click(&mut h, "Spare");
    h.get(by().label("Spare").role(Role::Button)).click_secondary();
    h.run_steps(3);
    click(&mut h, "Rename");
    type_into(&mut h, "Spare", "Extra");
    assert!(h.state().session.doc().unwrap().text_style("Extra").is_some());
    // A name in use isn't offered: Extra keeps its name and stays chosen.
    h.get(by().label("Extra").role(Role::Button)).click_secondary();
    h.run_steps(3);
    click(&mut h, "Rename");
    type_into(&mut h, "Extra", "Used");
    assert!(!enabled(&h, "OK"));
    assert!(h.state().session.doc().unwrap().text_style("Extra").is_some());
    click(&mut h, "Delete");
    assert!(h.state().session.doc().unwrap().text_style("Extra").is_none());
    // In use only lists the current style and the styles in use.
    h.get(by().role(Role::ComboBox).value("All styles")).click();
    h.run_steps(3);
    click(&mut h, "Styles in use");
    assert!(h.query(by().label("Unused").role(Role::Button)).is_none());
    h.get(by().label("Used").role(Role::Button));
}

/// A font file with only a naming table, built here: no font file is committed.
fn font_with_names(family: &str, style: &str) -> Vec<u8> {
    let mut strings = Vec::new();
    let mut recs = Vec::new();
    for (id, s) in [(1u16, family), (2, style)] {
        let bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [3u16, 1, 0x0409, id, bytes.len() as u16, strings.len() as u16] {
            recs.extend_from_slice(&v.to_be_bytes());
        }
        strings.extend(bytes);
    }
    let mut name = Vec::new();
    for v in [0u16, 2, (6 + recs.len()) as u16] {
        name.extend_from_slice(&v.to_be_bytes());
    }
    name.extend(recs);
    name.extend(strings);
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for v in [1u16, 16, 0, 0] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(b"name");
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&28u32.to_be_bytes());
    out.extend_from_slice(&(name.len() as u32).to_be_bytes());
    out.extend(name);
    out
}

#[test]
fn fonts_are_picked_by_family_and_style() {
    cadcraft_fonts::ttf::register("cadcraft-dialog-test.ttf", font_with_names("CADCraft Dialog Test", "Regular"));
    cadcraft_fonts::ttf::register("cadcraft-dialog-test-bd.ttf", font_with_names("CADCraft Dialog Test", "Bold"));
    let mut h = dialog(|app| {
        app.run("style", json!({ "name": "Missing", "font": "nowhere-to-be-found.ttf", "current": false })).unwrap();
    });
    h.get(by().role(Role::ComboBox).value(cadcraft_fonts::BUILTIN_FONT)).click();
    h.run_steps(3);
    type_into(&mut h, "", "dialog test");
    click(&mut h, "CADCraft Dialog Test");
    h.get(by().role(Role::ComboBox).value("Regular")).click();
    h.run_steps(3);
    click(&mut h, "Bold");
    click(&mut h, "Apply");
    assert_eq!(style(&h, "Standard").font, "cadcraft-dialog-test-bd.ttf");
    // A font that isn't installed is shown as such.
    click(&mut h, "Missing");
    h.get(by().role(Role::ComboBox).value("nowhere-to-be-found.ttf (not installed)"));
}

/// With `CADCRAFT_TEXTSTYLE_SHOTS=<directory>`, pictures of the dialog to look at.
#[test]
fn pictures_of_the_dialog() {
    let Some(directory) = std::env::var_os("CADCRAFT_TEXTSTYLE_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    let setup = |app: &mut CadApp| {
        app.run("style", json!({ "name": "Annotations", "annotative": true, "height": 0.125, "current": false })).unwrap();
        app.run("style", json!({ "name": "Title", "font": "arial.ttf", "widthFactor": 0.8, "oblique": 15, "current": false })).unwrap();
    };
    let shots: [(&str, &[&str]); 3] = [("textstyle.png", &[]), ("textstyle-title.png", &["Title"]), ("textstyle-new.png", &["New..."])];
    for (name, clicks) in shots {
        let mut h = harness(setup, true);
        for c in clicks {
            click(&mut h, c);
        }
        h.run_steps(2);
        h.render().unwrap().save(directory.join(name)).unwrap();
    }
}
