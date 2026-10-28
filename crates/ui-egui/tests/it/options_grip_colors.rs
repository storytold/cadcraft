//! Options ▸ Selection ▸ Grip Colors...: the unselected, hover, selected and contour grip colours
//! (GRIPCOLOR, GRIPHOVER, GRIPHOT, GRIPCONTOUR) are set through the OPTIONS JSON form, kept in the
//! profile, painted on the drawing area and edited in their window.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::{Color32, Pos2, Rect, Shape, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn aci(i: u8) -> Color32 {
    let c = cadcraft_color::aci_rgb(i);
    Color32::from_rgb(c.0, c.1, c.2)
}

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// A drawing with one selected line, the drawing area showing it.
fn app_with_selected_line() -> CadApp {
    let mut a = app();
    a.ui.show_command_line = false;
    a.run("line", json!({"points": [[0, 0], [100, 50]]})).unwrap();
    a.run("zoom", json!({"mode": "extents"})).unwrap();
    let all = a.session.doc().unwrap().model.handles();
    a.session.set_selection(all);
    a
}

/// The fill and outline colours of every rectangle painted in one frame with the pointer at `at`.
fn painted_rects(app: &mut CadApp, ctx: &egui::Context, at: Pos2) -> (Vec<Color32>, Vec<Color32>) {
    fn collect(shape: &Shape, fills: &mut Vec<Color32>, outlines: &mut Vec<Color32>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, fills, outlines)),
            Shape::Rect(r) => {
                fills.push(r.fill);
                outlines.push(r.stroke.color);
            }
            _ => {}
        }
    }
    let (mut fills, mut outlines) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            events: vec![egui::Event::PointerMoved(at)],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        (fills, outlines) = (Vec::new(), Vec::new());
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut fills, &mut outlines));
    }
    (fills, outlines)
}

#[test]
fn grip_colors_are_set_checked_and_survive_a_restart() {
    let mut a = app();
    let all = a.run("options", json!({})).unwrap();
    for (key, v) in [("GRIPCOLOR", 150), ("GRIPHOVER", 11), ("GRIPHOT", 12), ("GRIPCONTOUR", 251)] {
        assert_eq!(all[key], json!(v), "{key}");
    }
    a.run("options", json!({"GRIPCOLOR": 3, "GRIPHOVER": 4, "GRIPHOT": 6, "GRIPCONTOUR": 7})).unwrap();
    assert!(a.ui.dialog.is_none(), "JSON calls never open dialogs");
    for bad in [json!({"GRIPCOLOR": 0}), json!({"GRIPHOT": 256}), json!({"GRIPCONTOUR": "blue"}), json!({"GRIPHOVER": -3, "GRIPCOLOR": 5})] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
    }
    let s = &a.session.settings;
    assert_eq!((s.gripcolor, s.griphover, s.griphot, s.gripcontour), (3, 4, 6, 7), "refused values change nothing");

    let mut b = app();
    b.load_profile(&a.profile_json());
    let s = &b.session.settings;
    assert_eq!((s.gripcolor, s.griphover, s.griphot, s.gripcontour), (3, 4, 6, 7), "kept in the profile");

    // A hand-edited profile with hostile grip colours never stops the app; good entries stay.
    let mut c = app();
    c.load_profile(r#"{"version": 1, "sysvars": {"GRIPCOLOR": 1e308, "GRIPHOVER": [], "GRIPHOT": "12", "GRIPCONTOUR": -251}}"#);
    let s = &c.session.settings;
    assert_eq!((s.gripcolor, s.griphover, s.griphot, s.gripcontour), (150, 11, 12, 251));

    a.run("options", json!({"reset": true})).unwrap();
    let s = &a.session.settings;
    assert_eq!((s.gripcolor, s.griphover, s.griphot, s.gripcontour), (150, 11, 12, 251), "reset");
}

#[test]
fn the_drawing_area_paints_grips_in_the_chosen_colors() {
    let mut a = app_with_selected_line();
    let ctx = egui::Context::default();
    let far = pos2(5.0, 795.0);
    // The first frame sizes the drawing area; zoom to the line in it.
    painted_rects(&mut a, &ctx, far);
    a.run("zoom", json!({"mode": "extents"})).unwrap();
    let (fills, outlines) = painted_rects(&mut a, &ctx, far);
    assert!(fills.contains(&aci(150)), "AutoCAD's blue unselected grips by default");
    assert!(outlines.contains(&aci(251)), "and their grey contour");

    a.run("options", json!({"GRIPCOLOR": 3, "GRIPCONTOUR": 1, "GRIPHOT": 5})).unwrap();
    let (fills, outlines) = painted_rects(&mut a, &ctx, far);
    assert!(fills.contains(&aci(3)), "unselected grips are green");
    assert!(outlines.contains(&aci(1)), "outlined red");
    assert!(!fills.contains(&aci(150)) && !fills.contains(&aci(5)), "no grip in another colour");

    // A hot (selected) grip takes GRIPHOT.
    let h = a.session.selection()[0];
    a.canvas.hot_grips = vec![(h, 0)];
    let (fills, _) = painted_rects(&mut a, &ctx, far);
    assert!(fills.contains(&aci(5)) && fills.contains(&aci(3)), "one hot grip, the others unselected");
}

/// With `CADCRAFT_OPTIONS_SHOTS` set to a folder, the dialog is rendered there for a look.
const SHOTS: &str = "CADCRAFT_OPTIONS_SHOTS";

fn harness(app: CadApp) -> Harness<'static, CadApp> {
    let builder = Harness::builder().with_size(egui::vec2(1400.0, 900.0));
    let builder = if std::env::var_os(SHOTS).is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h
}

fn shot(h: &mut Harness<'static, CadApp>, name: &str) {
    if let Some(dir) = std::env::var_os(SHOTS).map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(dir.join(name)).unwrap();
    }
}

#[test]
fn the_grip_colors_window_edits_the_four_colors() {
    let mut a = app_with_selected_line();
    a.run("options", json!({"GRIPHOVER": 30})).unwrap();
    menus::activate(&mut a, "options");
    a.ui.dialog = Some("options:selection".into());
    let mut h = harness(a);
    assert!(h.query_by_label("Unselected grip color:").is_none(), "closed until asked for");
    h.get_by_label("Grip Colors...").click();
    h.run_steps(3);
    for label in ["Unselected grip color:", "Hover grip color:", "Selected grip color:", "Grip contour color:"] {
        h.get_by_label(label);
    }
    shot(&mut h, "options-grip-colors.png");

    // Each colour is a number field named by its row: type a colour index into it.
    h.get_by_label("Selected grip color:").click();
    h.run_steps(2);
    h.get_by_label("Selected grip color:").type_text("3");
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let s = &h.state().session.settings;
    assert_eq!((s.griphot, s.griphover, s.gripcolor), (3, 30, 150), "only the selected grip colour changed");

    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");

    // Close shuts the window and leaves the Options dialog open. The window's Close comes after
    // the Options dialog's in the tree.
    if let Some(n) = h.get_all_by_label("Close").last() {
        n.click();
    }
    h.run_steps(3);
    assert!(h.query_by_label("Unselected grip color:").is_none());
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options:selection"));
}
