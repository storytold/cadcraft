//! The Hatch Editor contextual Ribbon tab: it opens by itself when hatches alone are selected,
//! shows the hatch's settings, and gives the previous tab back once they are deselected.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Handle;
use cadcraft_ui_egui::{CadApp, Services, ribbon_hatch};
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn harness(french: bool) -> (Harness<'static, CadApp>, Handle, Handle) {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.run("ui.layout.pc", json!({})).unwrap();
    app.ui.start_tab = false;
    app.ui.in_window_menu = false;
    app.integrated_titlebar = true;
    if french {
        app.ui.interface_language = serde_json::from_value(json!("fr")).unwrap();
    }
    let r = app.run("rectang", json!({ "p1": [0, 0], "p2": [20, 10] })).unwrap();
    let rect = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    let h = app.run("hatch", json!({ "handles": [r["handle"]], "pattern": "SOLID" })).unwrap();
    let hatch = Handle::parse_hex(h["handle"].as_str().unwrap()).unwrap();
    app.ui.ribbon_tab = "annotate".into();
    let mut harness = Harness::builder().with_size(vec2(1500.0, 800.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    (harness, hatch, rect)
}

#[test]
fn selecting_a_hatch_opens_the_hatch_editor_and_deselecting_closes_it() {
    let (mut h, hatch, rect) = harness(false);
    assert_eq!(h.state().ui.ribbon_tab, "annotate");
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_hatch::TAB_ID);
    for label in ["Hatch Editor", "Boundaries", "Pattern", "Origin", "Options", "Close Hatch Editor", "Associative", "Pick Points"] {
        assert!(h.query_by_label(label).is_some(), "{label} shown");
    }

    // A hatch with another object: no Hatch Editor.
    h.state_mut().session.set_selection(vec![hatch, rect]);
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "annotate", "back to the tab shown before");
    assert!(h.query_by_label("Hatch Editor").is_none());

    // Close Hatch Editor deselects.
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    h.get_by_label("Close Hatch Editor").click();
    h.run_steps(3);
    assert!(h.state().session.selection().is_empty());
    assert_eq!(h.state().ui.ribbon_tab, "annotate");
}

#[test]
fn the_gallery_and_the_associative_toggle_edit_the_hatch() {
    let (mut h, hatch, _) = harness(false);
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    h.get_by_label("ANSI31").click();
    h.run_steps(3);
    let pattern = |h: &Harness<'static, CadApp>| match &h.state().session.doc().unwrap().entity(hatch).unwrap().kind {
        cadcraft_engine::doc::EntityKind::Hatch(x) => (x.pattern.clone(), x.associative),
        _ => panic!("not a hatch"),
    };
    assert_eq!(pattern(&h).0, "ANSI31");
    let assoc = pattern(&h).1;
    h.get_by_label("Associative").click();
    h.run_steps(3);
    assert_eq!(pattern(&h).1, !assoc);
    // The tab stays while the hatch is selected.
    assert_eq!(h.state().ui.ribbon_tab, ribbon_hatch::TAB_ID);
}

#[test]
fn each_slide_out_opens_under_its_panel_title() {
    let (mut h, hatch, _) = harness(false);
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    for (title, inside) in
        [("Boundaries", "Display Boundary Objects"), ("Properties", "Double"), ("Origin", "Store as Default Origin"), ("Options", "Separate Hatches")]
    {
        h.get_all_by_label(title).next().unwrap().click();
        h.run_steps(3);
        assert!(h.query_by_label(inside).is_some(), "{title} slides out {inside}");
        h.get_all_by_label(title).next().unwrap().click();
        h.run_steps(3);
    }
}

/// With CRAFT_UI_VISUAL_DIR set: the tab in French with each slide-out, to compare with the reference screenshots.
#[test]
fn french_pictures_of_the_hatch_editor() {
    let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") else { return };
    std::fs::create_dir_all(&directory).unwrap();
    let (mut h, hatch, _) = harness(true);
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    h.render().unwrap().save(format!("{directory}/hatch-editor.png")).unwrap();
    for title in ["Boundaries", "Propriétés", "Origin", "Options"] {
        h.get_all_by_label(title).next().unwrap().click();
        h.run_steps(3);
        h.render().unwrap().save(format!("{directory}/hatch-editor-{title}.png")).unwrap();
        h.get_all_by_label(title).next().unwrap().click();
        h.run_steps(3);
    }
}

#[test]
fn hatch_shows_the_hatch_creation_tab_and_close_makes_the_hatch() {
    let (mut h, _, _) = harness(false);
    // A free square to hatch.
    h.state_mut().run("rectang", json!({ "p1": [40, 0], "p2": [50, 10] })).unwrap();
    h.state_mut().cmd.buffer = "HATCH".into();
    cadcraft_ui_egui::cmdline::submit(h.state_mut());
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_hatch::CREATE_ID);
    assert!(h.query_by_label("Hatch Creation").is_some());
    assert!(h.state().ui.dialog.is_none(), "no dialog");
    h.get_by_label("ANSI31").click();
    h.run_steps(3);
    h.state_mut().session.input(cadcraft_engine::Input::Point(cadcraft_engine::geom::Vec2::new(45.0, 5.0))).unwrap();
    h.run_steps(3);
    h.get_by_label("Close Hatch Creation").click();
    h.run_steps(3);
    assert!(h.state().session.running.is_none());
    let made = h
        .state()
        .session
        .doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| matches!(&e.kind, cadcraft_engine::doc::EntityKind::Hatch(x) if x.pattern == "ANSI31"))
        .count();
    assert_eq!(made, 1);
    assert_eq!(h.state().ui.ribbon_tab, "annotate", "back to the tab shown before");
}

/// With CRAFT_UI_VISUAL_DIR set: Hatch Creation, the colour list and the full gallery in French.
#[test]
fn french_pictures_of_hatch_creation_and_lists() {
    let Ok(directory) = std::env::var("CRAFT_UI_VISUAL_DIR") else { return };
    std::fs::create_dir_all(&directory).unwrap();
    let (mut h, hatch, _) = harness(true);
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    h.get_by_label("Hatch Color").click();
    h.run_steps(3);
    h.render().unwrap().save(format!("{directory}/hatch-colors.png")).unwrap();
    // Escape would deselect the hatch: the list closes with a second click on it.
    h.get_by_label("Hatch Color").click();
    h.run_steps(3);
    h.get_by_label("More").click();
    h.run_steps(3);
    h.render().unwrap().save(format!("{directory}/hatch-gallery.png")).unwrap();
    h.key_press(egui::Key::Escape);
    h.state_mut().session.set_selection(Vec::new());
    h.run_steps(3);
    h.state_mut().cmd.buffer = "HATCH".into();
    cadcraft_ui_egui::cmdline::submit(h.state_mut());
    h.run_steps(3);
    h.render().unwrap().save(format!("{directory}/hatch-creation.png")).unwrap();
}

/// Hatch Creation's buttons called by name with no HATCH running do nothing: Close does not
/// repeat the last command, and the others send no keyword to another command.
#[test]
fn hatch_creation_buttons_refuse_without_hatch_running() {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", json!({})).unwrap();
    app.cmdline("circle 0,0 5");
    assert!(app.session.running.is_none());
    assert!(app.run("ui.hatchcreate.close", json!({})).is_err());
    assert!(app.session.running.is_none(), "CIRCLE is not repeated");

    app.start("line");
    for id in [
        "ui.hatchcreate.points",
        "ui.hatchcreate.select",
        "ui.hatchcreate.remove",
        "ui.hatchcreate.origin",
        "ui.hatchcreate.match",
        "ui.hatchcreate.close",
    ] {
        assert!(app.run(id, json!({})).is_err(), "{id}");
        assert_eq!(app.session.running.as_ref().map(|r| r.id.as_str()), Some("line"), "{id} leaves LINE alone");
    }
}

/// A Ribbon reduced to its tabs stays reduced when HATCH or a selected hatch opens a contextual
/// tab: the tab shows in the tab row, its panels don't.
#[test]
fn a_reduced_ribbon_stays_reduced_for_the_hatch_tabs() {
    let (mut h, hatch, _) = harness(false);
    h.state_mut().ui.ribbon_minimized = true;
    h.state_mut().session.set_selection(vec![hatch]);
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_hatch::TAB_ID);
    assert!(h.state().ui.ribbon_minimized, "Hatch Editor");
    assert!(h.query_by_label("Hatch Editor").is_some(), "the tab is in the tab row");
    assert!(h.query_by_label("Close Hatch Editor").is_none(), "its panels are not shown");

    h.state_mut().session.set_selection(Vec::new());
    h.run_steps(3);
    h.state_mut().cmd.buffer = "HATCH".into();
    cadcraft_ui_egui::cmdline::submit(h.state_mut());
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, ribbon_hatch::CREATE_ID);
    assert!(h.state().ui.ribbon_minimized, "Hatch Creation");
    assert!(h.query_by_label("Close Hatch Creation").is_none());
}
