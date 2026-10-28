//! Options ▸ Display ▸ Show ToolTips (and their delay), Options ▸ Open and Save ▸ Number of
//! recently-used files, Display full path in title and the Start tab at start-up (STARTMODE):
//! set through the OPTIONS JSON form, kept in the profile, applied to the interface and shown in
//! the dialog.

use cadcraft_engine::Session;
use cadcraft_ui_egui::{CadApp, Services, chrome, menus, options};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

#[test]
fn the_json_form_sets_and_checks_them_and_the_profile_keeps_them() {
    let mut a = app();
    let all = a.run("options", json!({})).unwrap();
    for (key, v) in [
        ("tooltips", json!(true)),
        ("tooltipDelay", json!(0.5)),
        ("fullPathInTitle", json!(false)),
        ("recentFiles", json!(9)),
        ("STARTMODE", json!(0)),
    ] {
        assert_eq!(all[key], v, "{key}");
    }
    a.ui.recent_files = (0..9).map(|i| format!("/drawings/{i}.dxf")).collect();
    let r = a.run("options", json!({"tooltips": false, "tooltipDelay": 1.5, "fullPathInTitle": true, "recentFiles": 3, "STARTMODE": 1})).unwrap();
    assert!(a.ui.dialog.is_none(), "JSON calls never open dialogs");
    assert_eq!((r["tooltips"].clone(), r["tooltipDelay"].clone(), r["STARTMODE"].clone()), (json!(false), json!(1.5), json!(1)));
    assert_eq!(a.ui.recent_files.len(), 3, "a smaller list drops the oldest drawings");

    // All or nothing: one bad value changes nothing.
    for bad in [
        json!({"tooltipDelay": 11}),
        json!({"tooltipDelay": -1}),
        json!({"tooltipDelay": "soon"}),
        json!({"tooltips": 1}),
        json!({"fullPathInTitle": "yes"}),
        json!({"recentFiles": 51}),
        json!({"recentFiles": -1}),
        json!({"recentFiles": 2.5}),
        json!({"STARTMODE": 2}),
        json!({"startmode": "1"}),
        json!({"recentFiles": 1, "tooltipDelay": 1e300}),
    ] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
    }
    let o = &a.options;
    assert_eq!((o.tooltips, o.tooltip_delay, o.full_path_in_title, o.recent_files, o.start_tab), (false, 1.5, true, 3, true));

    // Kept with the profile; the recent list saved in the prefs follows it.
    let mut b = app();
    b.load_profile(&a.profile_json());
    let o = &b.options;
    assert_eq!((o.tooltips, o.tooltip_delay, o.full_path_in_title, o.recent_files, o.start_tab), (false, 1.5, true, 3, true));
    a.ui.recent_files = (0..9).map(|i| format!("/drawings/{i}.dxf")).collect();
    b.load_prefs(&a.prefs_json());
    assert_eq!(b.ui.recent_files.len(), 3);

    // A hand-edited profile with hostile values never stops the app; they are skipped.
    let mut c = app();
    c.load_profile(
        r#"{"version": 1, "tooltips": "no", "tooltipDelay": 1e308, "recentFiles": 18446744073709551615, "fullPathInTitle": null, "STARTMODE": -7}"#,
    );
    let o = &c.options;
    assert_eq!((o.tooltips, o.tooltip_delay, o.full_path_in_title, o.recent_files, o.start_tab), (true, 0.5, false, 9, false));

    a.run("options", json!({"reset": true})).unwrap();
    let o = &a.options;
    assert_eq!((o.tooltips, o.tooltip_delay, o.full_path_in_title, o.recent_files, o.start_tab), (true, 0.5, false, 9, false), "reset");
}

/// One frame; the window title commands it sent to the host.
fn frame(app: &mut CadApp, ctx: &egui::Context) -> Vec<String> {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
    });
    out.textures_delta.clear();
    out.viewport_output
        .values()
        .flat_map(|v| v.commands.iter())
        .filter_map(|c| match c {
            egui::ViewportCommand::Title(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_title_shows_the_drawing_and_with_full_path_its_file() {
    let mut a = app();
    let ctx = egui::Context::default();
    assert_eq!(chrome::title_drawing(&a).as_deref(), Some("Drawing1.dwg"));
    assert_eq!(frame(&mut a, &ctx), ["CADCraft - Drawing1.dwg"]);
    assert!(frame(&mut a, &ctx).is_empty(), "sent only when it changes");

    let path = std::env::temp_dir().join("cadcraft-title").join("plan.dxf").to_string_lossy().into_owned();
    {
        let st = a.session.state_mut().unwrap();
        st.path = Some(path.clone());
        st.title = "plan.dxf".into();
    }
    assert_eq!(chrome::title_drawing(&a).as_deref(), Some("plan.dxf"));
    a.run("options", json!({"fullPathInTitle": true})).unwrap();
    assert_eq!(chrome::title_drawing(&a).as_deref(), Some(path.as_str()));
    assert_eq!(frame(&mut a, &ctx), [format!("CADCraft - {path}")]);

    // A drawing with no file yet has no path to show.
    a.run("new", json!({})).unwrap();
    assert!(chrome::title_drawing(&a).is_some_and(|t| t.ends_with(".dwg") && !t.contains(std::path::MAIN_SEPARATOR)));
    a.ui.start_tab = true;
    assert_eq!(chrome::title_drawing(&a), None, "the Start tab");
    assert_eq!(frame(&mut a, &ctx), ["CADCraft"]);
}

#[test]
fn show_tooltips_and_their_delay_reach_egui() {
    let mut a = app();
    let ctx = egui::Context::default();
    frame(&mut a, &ctx);
    assert_eq!(ctx.global_style().interaction.tooltip_delay, 0.5);
    a.run("options", json!({"tooltipDelay": 2})).unwrap();
    frame(&mut a, &ctx);
    assert_eq!(ctx.global_style().interaction.tooltip_delay, 2.0);
    a.run("options", json!({"tooltips": false})).unwrap();
    frame(&mut a, &ctx);
    assert_eq!(ctx.global_style().interaction.tooltip_delay, options::NO_TOOLTIPS, "no tooltips");
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
fn the_dialog_shows_and_changes_them() {
    let mut a = app();
    menus::activate(&mut a, "options");
    a.ui.dialog = Some("options:display".into());
    let mut h = harness(a);
    h.get_by_label("Show ToolTips").click();
    h.run_steps(3);
    assert!(!h.state().options.tooltips);
    h.get_by_label("Show ToolTips").click();
    h.run_steps(3);
    h.get_by_label("Number of seconds before display").click();
    h.run_steps(2);
    h.get_by_label("Number of seconds before display").type_text("2");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().options.tooltip_delay, 2.0);
    shot(&mut h, "options-display-tooltips.png");
    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls on Display: {unnamed:?}");

    h.get_by_label("Open and Save").click();
    h.run_steps(3);
    h.get_by_label("Display full path in title").click();
    h.run_steps(3);
    assert!(h.state().options.full_path_in_title);
    h.get_by_label("Show the Start tab when CADCraft starts").click();
    h.run_steps(3);
    assert!(h.state().options.start_tab);
    h.get_by_label("Number of recently-used files").click();
    h.run_steps(2);
    h.get_by_label("Number of recently-used files").type_text("4");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().options.recent_files, 4);
    shot(&mut h, "options-open-and-save-recent.png");
    let unnamed = h.unnamed_widgets();
    assert!(unnamed.is_empty(), "unnamed controls on Open and Save: {unnamed:?}");
}
