//! OPTIONS: the dialog opens only when typed or chosen from the menu; the JSON form reads and sets
//! the preferences; the user profile survives a restart (`profile_json` → `load_profile`).

use cadcraft_engine::Session;
use cadcraft_ui_egui::theme::Tokens;
use cadcraft_ui_egui::{CadApp, Services, menus, options};
use egui::Color32;
use serde_json::{Value, json};

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
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
    let mut text = String::new();
    shapes.iter().for_each(|s| collect(&s.shape, &mut text));
    text
}

/// Two frames (windows fade in), returning the painted text of the last.
fn render(app: &mut CadApp, ctx: &egui::Context) -> String {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut text = String::new();
    for _ in 0..2 {
        app.logic(ctx);
        let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
        text = painted_text(&output.shapes);
        output.textures_delta.clear();
    }
    text
}

#[test]
fn typed_and_menu_open_the_dialog_json_never_does() {
    let mut a = app();
    menus::activate(&mut a, "options");
    assert_eq!(a.ui.dialog.as_deref(), Some("options"));
    for typed in ["OPTIONS", "op", "Preferences"] {
        a.ui.dialog = None;
        a.cmdline(typed);
        assert_eq!(a.ui.dialog.as_deref(), Some("options"), "{typed}");
    }
    a.ui.dialog = None;
    let all = a.run("options", json!({})).unwrap();
    assert!(a.ui.dialog.is_none(), "JSON calls never open dialogs");
    for key in ["theme", "saveFormat", "systemCursor", "modelBackground", "paperBackground", "snapMarkerSize", "OSMODE", "PICKBOX", "CURSORSIZE"] {
        assert!(all.get(key).is_some(), "{key} is reported");
    }
    a.cmdline(r#"options {"PICKBOX": 4}"#);
    assert!(a.ui.dialog.is_none());
    assert_eq!(a.session.settings.pickbox, 4.0);
    // Tools ▸ Options… is in the menu.
    let tools = menus::tree(&a).into_iter().find(|(m, _)| m == "Tools").map(|(_, e)| e).unwrap_or_default();
    assert!(tools.iter().any(|e| matches!(e, menus::Entry::Item { id, .. } if id == "options")));
}

#[test]
fn json_form_sets_interface_and_engine_preferences_all_or_nothing() {
    let mut a = app();
    let r = a
        .run(
            "options",
            json!({"theme": "light", "modelBackground": "#FFFFFF", "snapMarkerSize": 15, "systemCursor": true, "GRIPSIZE": 8, "PICKADD": 0}),
        )
        .unwrap();
    assert_eq!(r["theme"], "light");
    assert_eq!(r["modelBackground"], "#ffffff");
    assert_eq!(r["snapMarkerSize"], json!(15.0));
    assert_eq!(r["GRIPSIZE"], json!(8.0));
    assert_eq!(r["PICKADD"], json!(0));
    assert!(a.ui.system_cursor);

    // A bad interface value or engine name changes nothing.
    let before = a.run("options", json!({})).unwrap();
    for bad in [
        json!({"theme": "sepia", "PICKBOX": 9}),
        json!({"modelBackground": "#12345", "PICKBOX": 9}),
        json!({"modelBackground": 7}),
        json!({"snapMarkerSize": 400}),
        json!({"snapMarkerSize": f64::NAN}),
        json!({"historyLines": 99}),
        json!({"saveFormat": "dxf", "LUNITS": 4}),
        json!({"systemCursor": false, "PICKBOX": "big"}),
        json!("options"),
    ] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
        assert_eq!(a.run("options", json!({})).unwrap(), before, "{bad} changed nothing");
    }

    // null puts a colour back to the default; reset restores everything but the language.
    a.run("options", json!({"modelBackground": null, "interfaceLanguage": "uk"})).unwrap();
    assert_eq!(a.options.model_background, None);
    let r = a.run("options", json!({"reset": true})).unwrap();
    assert_eq!((r["theme"].clone(), r["GRIPSIZE"].clone(), r["snapMarkerSize"].clone()), (json!("dark"), json!(5.0), json!(11.0)));
    assert_eq!(r["interfaceLanguage"], "uk");
}

#[test]
fn profile_survives_a_restart_and_keeps_the_prefs_format() {
    let mut a = app();
    let prefs = a.prefs_json();
    a.run(
        "options",
        json!({"OSMODE": 7, "ORTHOMODE": 1, "AUTOSNAP": 37, "POLARANG": 45, "SNAPMODE": 1, "SNAPUNIT": [2, 2], "GRIDUNIT": [5, 5],
               "DYNMODE": 0, "DYNPIFORMAT": 1, "DYNPICOORDS": 1, "PICKBOX": 6, "APERTURE": 14, "GRIPSIZE": 9, "CURSORSIZE": 100,
               "PICKFIRST": 0, "PICKADD": 0, "QPMODE": 1, "systemCursor": true, "modelBackground": "#000000",
               "paperBackground": "#f0f0e8", "snapMarkerSize": 21}),
    )
    .unwrap();
    assert_eq!(a.prefs_json(), prefs, "the interface prefs keep their own key and format");
    let saved = a.profile_json();

    let mut b = app();
    b.load_profile(&saved);
    let (ra, rb) = (a.run("options", json!({})).unwrap(), b.run("options", json!({})).unwrap());
    assert_eq!(ra, rb);
    assert_eq!(rb["POLARANG"], json!(45.0));
    assert!(!b.session.settings.polarmode, "AUTOSNAP 37 is polar tracking off");
    assert!(b.ui.system_cursor);
}

#[test]
fn hostile_saved_profiles_are_ignored() {
    let defaults = app().run("options", json!({})).unwrap();
    for bad in [
        "".to_string(),
        "not json".into(),
        "[]".into(),
        r#"{"sysvars": "PICKBOX"}"#.into(),
        r#"{"sysvars": {"PICKBOX": "x", "LUNITS": 4, "FONTALT": "evil.ttf"}, "modelBackground": "red", "snapMarkerSize": -1}"#.into(),
        format!(r#"{{"sysvars": {{"PICKBOX": 5}}, "pad": "{}"}}"#, "x".repeat(70_000)),
    ] {
        let mut b = app();
        b.load_profile(&bad);
        assert_eq!(b.run("options", json!({})).unwrap(), defaults, "{}", bad.get(..60).unwrap_or(&bad));
    }
}

#[test]
fn model_background_reaches_the_drawing_area_tokens() {
    let mut a = app();
    let ctx = egui::Context::default();
    let frame = |a: &mut CadApp| {
        let mut out = ctx.run_ui(Default::default(), |ui| a.ui(ui));
        out.textures_delta.clear();
    };
    frame(&mut a);
    assert_eq!(Tokens::get().canvas, Tokens::DARK.canvas);
    a.run("options", json!({"modelBackground": "#ffffff", "paperBackground": "#202020"})).unwrap();
    frame(&mut a);
    let t = Tokens::get();
    assert_eq!(t.canvas, Color32::WHITE);
    assert_eq!(t.paper, Color32::from_gray(0x20));
    assert!(cadcraft_ui_egui::theme::luminance(t.crosshair) < 0.5, "a dark crosshair on a white background");
    assert_ne!(t.grid_minor, Tokens::DARK.grid_minor);
    assert_eq!(t.chrome, Tokens::DARK.chrome, "the interface keeps its theme");
    a.run("options", json!({"reset": true})).unwrap();
    frame(&mut a);
    assert_eq!(Tokens::get(), Tokens::DARK);
}

#[test]
fn every_tab_renders() {
    let mut a = app();
    let ctx = egui::Context::default();
    for (tab, label) in [
        ("display", "Crosshair size"),
        ("files", "Save new drawings as"),
        ("drafting", "Object snap aperture"),
        ("selection", "Pickbox size"),
        ("preferences", "Interface language"),
        ("profiles", "Reset to Defaults"),
    ] {
        a.ui.dialog = Some(format!("options:{tab}"));
        let text = render(&mut a, &ctx);
        assert!(text.contains(label), "{tab}: {text}");
        assert!(text.contains("User Preferences"), "the tab row shows");
    }
    // Unknown tabs show the first one.
    a.ui.dialog = Some("options:nope".into());
    assert!(render(&mut a, &ctx).contains("Crosshair size"));
    assert_eq!(options::PROFILE_KEY, "cadcraft.profile");
    let _: Value = serde_json::from_str(&a.profile_json()).unwrap();
}

#[test]
fn selection_highlight_settings_set_save_and_show_in_the_selection_tab() {
    let mut a = app();
    let r = a.run("options", json!({"selectionColor": "#FF8000", "SELECTIONEFFECT": 0, "SELECTIONPREVIEW": 1, "PREVIEWEFFECT": 1})).unwrap();
    assert_eq!((r["selectionColor"].clone(), r["SELECTIONEFFECT"].clone(), r["PREVIEWEFFECT"].clone()), (json!("#ff8000"), json!(0), json!(1)));
    // The colour reaches the drawing area's palette.
    cadcraft_ui_egui::theme::set_canvas(a.options.canvas());
    assert_eq!(Tokens::get().selection, Color32::from_rgb(0xff, 0x80, 0x00));
    for bad in [json!({"selectionColor": "orange"}), json!({"SELECTIONEFFECT": 3}), json!({"SELECTIONPREVIEW": 9})] {
        assert!(a.run("options", bad.clone()).is_err(), "{bad}");
    }

    let mut b = app();
    b.load_profile(&a.profile_json());
    assert_eq!(b.options.selection_color, Some(Color32::from_rgb(0xff, 0x80, 0x00)));
    assert_eq!((b.session.settings.selectioneffect, b.session.settings.selectionpreview, b.session.settings.previeweffect), (0, 1, 1));
    a.run("options", json!({"selectionColor": null})).unwrap();
    assert_eq!(a.options.selection_color, None);
    cadcraft_ui_egui::theme::set_canvas(cadcraft_ui_egui::theme::Canvas::default());

    // The Selection tab is laid out as AutoCAD's.
    let ctx = egui::Context::default();
    a.cmdline("options");
    a.ui.dialog = Some("options:selection".into());
    let text = render(&mut a, &ctx);
    for label in [
        "Selection modes",
        "Object grouping",
        "Selection effect color",
        "Grips",
        "Show grips",
        "Object selection limit for display of grips",
        "Selection preview",
        "When a command is active",
        "When no command is active",
        "Visual Effect Settings...",
    ] {
        assert!(text.contains(label), "{label} missing from:\n{text}");
    }
    // HIGHLIGHT, SELECTIONEFFECT and PREVIEWEFFECT are typed, not offered (as in AutoCAD), so
    // the highlight can't be switched off or changed by accident.
    for label in ["Highlight selected objects", "Preview effect", "Dashed"] {
        assert!(!text.contains(label), "{label} offered");
    }
    // Visual Effect Settings: the selection area and the preview filter.
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("cc_options_visual_effects"), true));
    let text = render(&mut a, &ctx);
    for label in [
        "Selection Area Effect",
        "Indicate selection area",
        "Window selection color:",
        "Selection area opacity:",
        "Selection Preview Filter",
        "Objects on locked layers",
        "Hatches",
    ] {
        assert!(text.contains(label), "{label} missing from:\n{text}");
    }
}
