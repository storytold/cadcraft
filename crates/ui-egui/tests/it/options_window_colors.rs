//! Options ▸ Display ▸ Colors... (Drawing Window Colors): the colours of model space's crosshair,
//! grid lines, AutoSnap marker and AutoTrack vector and of a layout's background are set through
//! the OPTIONS JSON form, kept in the profile, painted on the drawing area and edited in the
//! dialog.

use cadcraft_engine::Session;
use cadcraft_ui_egui::theme::{self, Tokens};
use cadcraft_ui_egui::{CadApp, Services, menus};
use egui::{Color32, Pos2, Rect, Shape, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

const RED: Color32 = Color32::from_rgb(0xff, 0x00, 0x00);
const TEAL: Color32 = Color32::from_rgb(0x00, 0x80, 0x80);
const OLIVE: Color32 = Color32::from_rgb(0x80, 0x80, 0x00);

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// The colours of every filled rectangle and line painted in one frame with the pointer at `at`.
fn paint(app: &mut CadApp, ctx: &egui::Context, at: Pos2) -> (Vec<Color32>, Vec<Color32>) {
    fn collect(shape: &Shape, fills: &mut Vec<Color32>, lines: &mut Vec<Color32>) {
        match shape {
            Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, fills, lines)),
            Shape::Rect(r) => fills.push(r.fill),
            Shape::LineSegment { stroke, .. } => lines.push(stroke.color),
            _ => {}
        }
    }
    let (mut fills, mut lines) = (Vec::new(), Vec::new());
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
        (fills, lines) = (Vec::new(), Vec::new());
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut fills, &mut lines));
        out.textures_delta.clear();
    }
    (fills, lines)
}

#[test]
fn window_colors_are_set_checked_and_survive_a_restart() {
    let mut a = app();
    let r = a
        .run(
            "options",
            json!({"crosshairColor": "#FF0000", "gridMajorColor": "#008080", "gridMinorColor": "#808000", "autosnapColor": "#0000ff",
                   "autotrackColor": "#ff00ff", "layoutBackground": "#101010"}),
        )
        .unwrap();
    assert_eq!(r["crosshairColor"], "#ff0000");
    assert_eq!(r["layoutBackground"], "#101010");
    assert_eq!(a.options.crosshair_color, Some(RED));

    // A bad colour changes nothing.
    let before = a.run("options", json!({})).unwrap();
    for bad in [json!({"crosshairColor": "red"}), json!({"gridMinorColor": 7}), json!({"autotrackColor": "#12345", "PICKBOX": 9})] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
        assert_eq!(a.run("options", json!({})).unwrap(), before, "{bad} changed nothing");
    }

    // The drawing-area palette takes them.
    theme::set_canvas(a.options.canvas());
    let t = Tokens::get();
    assert_eq!((t.crosshair, t.grid_major, t.grid_minor), (RED, TEAL, OLIVE));
    assert_eq!((t.snap, t.track, t.layout_bg), (Color32::from_rgb(0, 0, 0xff), Color32::from_rgb(0xff, 0, 0xff), Color32::from_gray(0x10)));
    theme::set_canvas(theme::Canvas::default());

    // Kept in the profile, and a hostile saved colour is skipped.
    let mut b = app();
    b.load_profile(&a.profile_json());
    assert_eq!(b.options, a.options);
    let mut c = app();
    c.load_profile(r##"{"crosshairColor": "#zzzzzz", "layoutBackground": 3}"##);
    assert_eq!((c.options.crosshair_color, c.options.layout_background), (None, None));

    // null and reset put them back.
    a.run("options", json!({"crosshairColor": null})).unwrap();
    assert_eq!(a.options.crosshair_color, None);
    a.run("options", json!({"reset": true})).unwrap();
    assert_eq!(a.options.layout_background, None);
    assert_eq!(a.options.autotrack_color, None);
}

#[test]
fn the_drawing_area_paints_the_chosen_colors() {
    let previous = theme::active();
    let mut session = Session::new();
    session.new_drawing(true);
    session.execute("view.set", &json!({"center": [100, 100], "height": 400})).unwrap();
    session.settings.gridmode = true;
    session.settings.dynmode = false;
    let mut a = CadApp::new(session, Services::default());
    a.canvas.zoom_pending = false;
    a.ui.show_command_line = false;
    a.ui.show_ucs_icon = false;
    a.ui.show_viewcube = false;
    a.run("options", json!({"theme": "dark", "systemCursor": false, "crosshairColor": "#ff0000", "gridMajorColor": "#008080"})).unwrap();
    let ctx = egui::Context::default();
    let (_, lines) = paint(&mut a, &ctx, pos2(640.0, 400.0));
    assert!(lines.contains(&RED), "the crosshair is drawn red");
    assert!(lines.contains(&TEAL), "the major grid lines are drawn teal");
    assert!(!lines.contains(&Tokens::DARK.crosshair), "no crosshair in the default colour");

    // On a layout, the area around the sheet takes the layout background.
    a.run("layout.set", json!({"name": "Layout1"})).unwrap();
    a.run("options", json!({"layoutBackground": "#808000"})).unwrap();
    let (fills, _) = paint(&mut a, &ctx, pos2(20.0, 400.0));
    assert!(fills.contains(&OLIVE), "the layout background is painted olive");
    assert!(!fills.contains(&Tokens::DARK.layout_bg));
    theme::set_active(previous);
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
fn the_colors_window_restores_elements_and_contexts() {
    let mut a = app();
    a.run("options", json!({"crosshairColor": "#ff0000", "gridMinorColor": "#808000", "paperBackground": "#f0f0e8"})).unwrap();
    menus::activate(&mut a, "options");
    a.ui.dialog = Some("options:display".into());
    let mut h = harness(a);
    h.get_by_label("Colors...").click();
    h.run_steps(3);
    h.get_by_label("Interface element:");
    h.get_by_label("Autotrack vector");

    // The selected element back to its default.
    h.get_by_label("Crosshairs").click();
    h.run_steps(3);
    h.get_by_label("#ff0000");
    shot(&mut h, "options-window-colors.png");
    h.get_by_label("Restore Element").click();
    h.run_steps(3);
    assert_eq!(h.state().options.crosshair_color, None);
    assert_eq!(h.state().options.grid_minor_color, Some(OLIVE), "only the selected element");

    // The layout context lists the paper; Restore all contexts resets every element.
    h.get_by_label("Sheet / layout").click();
    h.run_steps(3);
    h.get_by_label("Paper").click();
    h.run_steps(3);
    assert_eq!(h.get_all_by_label("#f0f0e8").count(), 2, "on the Display tab and in the window");
    h.get_by_label("Restore all contexts").click();
    h.run_steps(3);
    assert_eq!((h.state().options.grid_minor_color, h.state().options.paper_background), (None, None));

    // Close shuts the window and leaves the Options dialog open.
    // The window's Close comes after the Options dialog's in the tree.
    if let Some(n) = h.get_all_by_label("Close").last() {
        n.click();
    }
    h.run_steps(3);
    assert!(h.query_by_label("Interface element:").is_none());
    assert_eq!(h.state().ui.dialog.as_deref(), Some("options:display"));
    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls: {unnamed:?}");
}
