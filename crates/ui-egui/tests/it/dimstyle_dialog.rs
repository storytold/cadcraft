//! The Dimension Style Manager (#850): DIMSTYLE typed or from the menus opens it (JSON calls
//! don't); Modify, New, Override, Set Current, Compare and Delete do what AutoCAD's do.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, dialogs, menus};
use egui::{Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable, by};
use serde_json::json;

#[test]
fn dimstyle_typed_or_from_the_menus_opens_the_manager() {
    let mut app = CadApp::new(Session::new(), Services::default());
    for typed in ["DIMSTYLE", "dimstyle", "D", "DST", "DDIM"] {
        app.ui.dialog = None;
        assert!(app.cmdline(typed).is_none(), "bare {typed} is not an error");
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{typed}");
    }
    for item in ["dimstyle", "dimstyle.dimension"] {
        app.ui.dialog = None;
        menus::activate(&mut app, item);
        assert_eq!(app.ui.dialog.as_deref(), Some("dimstyle"), "{item}");
    }
    // Programmatic calls never open it.
    app.ui.dialog = None;
    app.run("dimstyle", json!({ "name": "Plan", "current": false })).unwrap();
    assert!(app.ui.dialog.is_none());
}

/// The manager open on a new drawing after `setup`.
fn manager(setup: impl FnOnce(&mut CadApp)) -> Harness<'static, CadApp> {
    harness(setup, false)
}

/// `manager`, drawn with the GPU renderer and the dark theme when `render`, for pictures.
fn harness(setup: impl FnOnce(&mut CadApp), render: bool) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::new(), Services::default());
    setup(&mut app);
    app.ui.dialog = Some("dimstyle".into());
    let builder = Harness::builder().with_size(vec2(1000.0, 640.0));
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

/// Clicks the button (or list entry or tab) `label`.
fn click(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get(by().label(label).role(egui::accesskit::Role::Button)).click();
    h.run_steps(3);
}

fn field<'h>(h: &'h Harness<'static, CadApp>, shown: &'h str) -> egui_kittest::Node<'h> {
    h.get(by().role(egui::accesskit::Role::TextInput).value(shown))
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

fn enabled(h: &Harness<'static, CadApp>, label: &str) -> bool {
    !h.get(by().label(label).role(egui::accesskit::Role::Button)).accesskit_node().is_disabled()
}

fn text_height(h: &Harness<'static, CadApp>, style: &str) -> f64 {
    h.state().session.doc().unwrap().dim_style(style).unwrap().text_height
}

#[test]
fn modify_edits_the_style() {
    let mut h = manager(|_| {});
    click(&mut h, "Modify...");
    click(&mut h, "Text");
    type_into(&mut h, "0.1800", "0.25");
    // Nothing changes until OK.
    assert_eq!(text_height(&h, "Standard"), 0.18);
    click(&mut h, "OK");
    assert_eq!(text_height(&h, "Standard"), 0.25);
    assert_eq!(h.state().ui.dialog.as_deref(), Some("dimstyle"), "back to the list");
}

#[test]
fn cancel_leaves_the_style_alone() {
    let mut h = manager(|_| {});
    click(&mut h, "Modify...");
    click(&mut h, "Text");
    type_into(&mut h, "0.1800", "0.5");
    click(&mut h, "Cancel");
    assert_eq!(text_height(&h, "Standard"), 0.18);
}

#[test]
fn new_starts_with_a_style_and_creates_it_on_ok() {
    let mut h = manager(|app| {
        app.run("dimstyle", json!({ "name": "Plan", "DIMTXT": 0.3, "current": false })).unwrap();
    });
    click(&mut h, "Plan");
    click(&mut h, "New...");
    click(&mut h, "Continue");
    assert!(h.state().session.doc().unwrap().dim_style("Copy of Plan").is_none(), "made on OK");
    click(&mut h, "OK");
    assert_eq!(text_height(&h, "Copy of Plan"), 0.3);
    // The current style is unchanged.
    assert_eq!(h.state().session.doc().unwrap().header.str("DIMSTYLE", ""), "Standard");
}

#[test]
fn override_changes_the_current_settings_only() {
    let mut h = manager(|_| {});
    click(&mut h, "Override...");
    click(&mut h, "Text");
    type_into(&mut h, "0.1800", "0.3");
    click(&mut h, "OK");
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.dim_overrides().get("textHeight"), Some(&json!(0.3)));
    assert_eq!(d.dim_style("Standard").unwrap().text_height, 0.18, "the style keeps its value");
    // The list shows the overrides under the current style.
    h.get_by_label("<style overrides>");
}

#[test]
fn override_drops_a_value_set_back_to_the_styles() {
    let mut h = manager(|app| {
        app.run("dimstyle.overrides", json!({ "textHeight": 0.3, "textGap": 0.05 })).unwrap();
    });
    click(&mut h, "Override...");
    click(&mut h, "Text");
    type_into(&mut h, "0.3000", "0.18");
    click(&mut h, "OK");
    let o = h.state().session.doc().unwrap().dim_overrides();
    assert_eq!((o.get("textHeight"), o.get("textGap")), (None, Some(&json!(0.05))), "{o:?}");
}

#[test]
fn a_refused_value_keeps_the_editor_and_its_edits() {
    let mut h = manager(|_| {});
    click(&mut h, "Override...");
    click(&mut h, "Text");
    type_into(&mut h, "0.1800", "-1");
    click(&mut h, "OK");
    // dimstyle.overrides refuses a negative text height: the page stays, with what was typed.
    assert!(h.state().session.doc().unwrap().dim_overrides().is_empty());
    field(&h, "-1.0000");
}

#[test]
fn a_new_style_takes_a_name_dimstyle_takes() {
    let mut h = manager(|_| {});
    click(&mut h, "New...");
    // `~` is -DIMSTYLE Restore's "compare with".
    type_into(&mut h, "Copy of Standard", "~Plan");
    assert!(!enabled(&h, "Continue"));
}

#[test]
fn the_style_overrides_compare_with_their_style() {
    let mut h = manager(|app| {
        app.run("dimstyle.overrides", json!({ "textHeight": 0.3 })).unwrap();
    });
    click(&mut h, "<style overrides>");
    click(&mut h, "Compare...");
    // The current settings against the style they override: the one difference.
    h.get_by_label("1 differences");
}

#[test]
fn set_current_compare_and_delete() {
    let mut h = manager(|app| {
        app.run("dimstyle", json!({ "name": "Temp", "DIMASZ": 0.5, "current": false })).unwrap();
    });
    click(&mut h, "Temp");
    click(&mut h, "Set Current");
    assert_eq!(h.state().session.doc().unwrap().header.str("DIMSTYLE", ""), "Temp");
    // Neither the current style nor Standard can be deleted.
    assert!(!enabled(&h, "Delete"));
    click(&mut h, "Standard");
    assert!(!enabled(&h, "Delete"));
    click(&mut h, "Temp");
    click(&mut h, "Compare...");
    h.get_by_label("All properties of Temp");
    click(&mut h, "Close");
    // The current style can't be deleted; another unused one can.
    h.state_mut().run("dimstyle.current", json!({ "name": "Standard" })).unwrap();
    h.run_steps(3);
    click(&mut h, "Temp");
    click(&mut h, "Delete");
    assert!(h.state().session.doc().unwrap().dim_style("Temp").is_none());
}

/// Opens the shortcut menu of list entry `label`.
fn right_click(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get(by().label(label).role(egui::accesskit::Role::Button)).click_secondary();
    h.run_steps(3);
}

/// Clicks the enabled button `label` (the shortcut menu's, when the dialog's own is off).
fn click_enabled(h: &mut Harness<'static, CadApp>, label: &str) {
    h.get_all(by().label(label).role(egui::accesskit::Role::Button)).find(|n| !n.accesskit_node().is_disabled()).unwrap().click();
    h.run_steps(3);
}

#[test]
fn shortcut_menus_save_or_discard_overrides_and_rename() {
    let mut h = manager(|app| {
        app.run("dimstyle", json!({ "name": "Plan", "current": false })).unwrap();
        app.run("setvar", json!({ "name": "DIMTXT", "value": 0.3 })).unwrap();
    });
    right_click(&mut h, "<style overrides>");
    click(&mut h, "Save to current style");
    let d = h.state().session.doc().unwrap();
    assert!(d.dim_overrides().is_empty());
    assert_eq!(d.dim_style("Standard").unwrap().text_height, 0.3);
    // Deleting the overrides leaves the style as it was.
    h.state_mut().run("setvar", json!({ "name": "DIMTXT", "value": 0.5 })).unwrap();
    h.run_steps(3);
    right_click(&mut h, "<style overrides>");
    click_enabled(&mut h, "Delete");
    let d = h.state().session.doc().unwrap();
    assert!(d.dim_overrides().is_empty());
    assert_eq!(d.dim_style("Standard").unwrap().text_height, 0.3);
    right_click(&mut h, "Plan");
    click(&mut h, "Rename");
    type_into(&mut h, "Plan", "Site");
    let d = h.state().session.doc().unwrap();
    assert!(d.dim_style("Plan").is_none() && d.dim_style("Site").is_some());
}

/// With `CADCRAFT_DIMSTYLE_SHOTS=<directory>`, pictures of the manager's pages to look at.
#[test]
fn pictures_of_the_pages() {
    let Some(directory) = std::env::var_os("CADCRAFT_DIMSTYLE_SHOTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&directory).unwrap();
    let setup = |app: &mut CadApp| {
        app.run("dimstyle", json!({ "name": "Plan", "DIMTXT": 0.25, "DIMBLK": "_ARCHTICK", "current": false })).unwrap();
        app.run("setvar", json!({ "name": "DIMDEC", "value": 2 })).unwrap();
    };
    let shots: [(&str, &[&str]); 10] = [
        ("dimstyle-list.png", &[]),
        ("dimstyle-overrides.png", &["<style overrides>"]),
        ("dimstyle-new.png", &["New..."]),
        ("dimstyle-lines.png", &["Modify..."]),
        ("dimstyle-symbols.png", &["Modify...", "Symbols and Arrows"]),
        ("dimstyle-text.png", &["Modify...", "Text"]),
        ("dimstyle-fit.png", &["Modify...", "Fit"]),
        ("dimstyle-units.png", &["Modify...", "Primary Units"]),
        ("dimstyle-tolerances.png", &["Override...", "Tolerances"]),
        ("dimstyle-compare.png", &["Plan", "Compare..."]),
    ];
    for (name, clicks) in shots {
        let mut h = harness(setup, true);
        for c in clicks {
            click(&mut h, c);
        }
        h.run_steps(2);
        h.render().unwrap().save(directory.join(name)).unwrap();
    }
}
