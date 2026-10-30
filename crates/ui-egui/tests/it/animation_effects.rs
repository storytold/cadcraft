//! Animation effects: with the operating system set to show none, dialogs and menus appear at
//! once and wheel scrolling doesn't glide. View ▸ Accessibility ▸ Animation Effects follows the OS
//! by default, or turns them on or off whatever it says.

use cadcraft_engine::Session;
use cadcraft_ui_egui::animation::AnimationPref;
use cadcraft_ui_egui::{CadApp, Services, UiState, menus};
use serde_json::json;

fn app() -> CadApp {
    CadApp::new(Session::new(), Services::default())
}

/// One frame of the whole app with `events`; `look` reads the frame's input after `logic`.
fn frame<R>(
    app: &mut CadApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    look: impl Fn(&egui::Context) -> R,
) -> (R, Vec<egui::epaint::ClippedShape>) {
    let raw =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), events, ..Default::default() };
    let mut seen = None;
    let mut out = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx());
        app.ui(ui);
        seen = Some(look(ui.ctx()));
    });
    out.textures_delta.clear(); // no renderer here
    (seen.unwrap(), out.shapes)
}

/// The opacity (0–255) `text` is painted with, if it is painted.
fn opacity_of(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<u8> {
    fn find(shape: &egui::Shape, text: &str) -> Option<u8> {
        match shape {
            egui::Shape::Text(s) if s.galley.text().contains(text) => Some(s.fallback_color.a()),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    shapes.iter().find_map(|s| find(&s.shape, text))
}

/// The opacity the About dialog's text has in the first frame it is painted, and a second later.
fn about_fade(app: &mut CadApp) -> (u8, u8) {
    const TEXT: &str = "clean-room CAD application";
    let ctx = egui::Context::default();
    frame(app, &ctx, Vec::new(), |_| ()); // the app itself, before the dialog opens
    app.start("ui.dialog.about");
    let mut first = None;
    for _ in 0..60 {
        let (_, shapes) = frame(app, &ctx, Vec::new(), |_| ());
        first = first.or(opacity_of(&shapes, TEXT));
    }
    let (_, shapes) = frame(app, &ctx, Vec::new(), |_| ());
    app.ui.dialog = None;
    (first.unwrap(), opacity_of(&shapes, TEXT).unwrap())
}

#[test]
fn dialogs_fade_in_only_while_animation_effects_show() {
    // The OS shows animations (or says nothing): egui's fade, as before.
    for system in [None, Some(true)] {
        let mut a = app();
        a.system_animations = system;
        let (first, settled) = about_fade(&mut a);
        assert!(first < 255, "{system:?}: the dialog fades in ({first})");
        assert_eq!(settled, 255, "{system:?}");
    }
    // The OS shows none: the dialog is there, whole, in its first frame.
    let mut a = app();
    a.system_animations = Some(false);
    assert_eq!(about_fade(&mut a), (255, 255));
    // The user's own choice outranks the OS, either way.
    a.start("ui.animations.on");
    assert!(about_fade(&mut a).0 < 255);
    a.system_animations = Some(true);
    a.start("ui.animations.off");
    assert_eq!(about_fade(&mut a), (255, 255));
}

#[test]
fn the_system_choice_follows_the_os_live() {
    let mut a = app();
    let ctx = egui::Context::default();
    let animated = egui::Style::default().animation_time;
    assert_eq!(a.ui.animations, AnimationPref::System, "the default");
    for (system, time) in [(Some(false), 0.0), (Some(true), animated), (None, animated), (Some(false), 0.0)] {
        a.system_animations = system;
        frame(&mut a, &ctx, Vec::new(), |_| ());
        assert_eq!(ctx.global_style().animation_time, time, "{system:?}");
        assert_eq!(a.shows_animations(), time > 0.0, "{system:?}");
    }
    assert_eq!(a.ui.animations, AnimationPref::System, "following the OS never changes the saved choice");
}

#[test]
fn wheel_scrolling_glides_only_while_animation_effects_show() {
    let line = egui::InputOptions::default().line_scroll_speed;
    let notches = || {
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            source: egui::MouseWheelSource::Unknown,
            delta: egui::vec2(0.0, -3.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }]
    };
    let scroll = |ctx: &egui::Context| ctx.input(|i| i.smooth_scroll_delta.y);

    let (mut a, ctx) = (app(), egui::Context::default());
    let (first, _) = frame(&mut a, &ctx, notches(), scroll);
    assert!(first < 0.0 && first > -3.0 * line, "egui spreads the notches over several frames: {first}");

    let (mut a, ctx) = (app(), egui::Context::default());
    a.system_animations = Some(false);
    assert_eq!(frame(&mut a, &ctx, notches(), scroll).0, -3.0 * line, "all of it in the frame it arrives");
    assert_eq!(frame(&mut a, &ctx, Vec::new(), scroll).0, 0.0, "and nothing afterwards");
}

/// How far the drawing zooms in, frame by frame, when the wheel turns one notch over the drawing
/// area with `modifiers` held: the view height before over the height after each of 40 frames.
fn wheel_zoom(app: &mut CadApp, modifiers: egui::Modifiers) -> Vec<f64> {
    let ctx = egui::Context::default();
    let height = |app: &CadApp| app.session.state().map(|s| s.view().height).unwrap();
    frame(app, &ctx, Vec::new(), |_| ());
    let over = app.canvas.rect.unwrap().center();
    // A few frames: layout settles and the pointer hover is known from the second frame on.
    for _ in 0..3 {
        frame(app, &ctx, vec![egui::Event::PointerMoved(over)], |_| ());
    }
    let before = height(app);
    let notch = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers,
    };
    let mut events = vec![notch];
    (0..40)
        .map(|_| {
            frame(app, &ctx, std::mem::take(&mut events), |_| ());
            before / height(app)
        })
        .collect()
}

#[test]
fn zooming_the_drawing_with_the_wheel_never_glides_with_animation_effects_off() {
    let o = egui::InputOptions::default();
    for system in [Some(true), Some(false)] {
        // The wheel alone zooms a whole notch in the frame it arrives, animation effects or not.
        let mut a = app();
        a.system_animations = system;
        let notch = a.session.wheel_zoom_factor(f64::from(o.line_scroll_speed));
        let zoom = wheel_zoom(&mut a, egui::Modifiers::NONE);
        assert!(notch > 1.0 && zoom.iter().all(|z| (z - notch).abs() < 1e-9), "{system:?}: {zoom:?}");
    }
    // ⌘/Ctrl+wheel (a touchpad pinch, on Windows) is egui's zoom gesture, which glides there…
    let whole = f64::from((o.scroll_zoom_speed * o.line_scroll_speed).exp());
    let mut a = app();
    let glide = wheel_zoom(&mut a, egui::Modifiers::COMMAND);
    assert!(glide[0] > 1.0 && glide[0] < glide[1] && glide[1] < whole, "{glide:?}");
    assert!((glide[39] - whole).abs() < 1e-3, "{glide:?}");
    // …and with animation effects off gets to the same zoom at once.
    a.start("ui.animations.off");
    let zoom = wheel_zoom(&mut a, egui::Modifiers::COMMAND);
    assert!(zoom.iter().all(|z| (z - whole).abs() < 1e-5), "{zoom:?}");
}

#[test]
fn the_menu_offers_the_three_choices_and_they_reach_the_setting() {
    let mut a = app();
    let view = menus::tree(&a).into_iter().find(|(m, _)| m == "View").map(|(_, e)| e).unwrap();
    let sub = |entries: &[menus::Entry], name: &str| {
        entries.iter().find_map(|e| match e {
            menus::Entry::Sub { label, children } if label == name => Some(children.clone()),
            _ => None,
        })
    };
    let choices = sub(&sub(&view, "Accessibility").unwrap(), "Animation Effects").unwrap();
    let items: Vec<(String, String)> = choices
        .iter()
        .filter_map(|e| match e {
            menus::Entry::Item { label, id, .. } => Some((label.clone(), id.clone())),
            menus::Entry::Sub { .. } => None,
        })
        .collect();
    let expected = [("Use System Setting", "ui.animations.system"), ("On", "ui.animations.on"), ("Off", "ui.animations.off")];
    assert_eq!(items, expected.map(|(label, id)| (label.to_string(), id.to_string())));

    a.system_animations = Some(false);
    for (id, pref, shown) in [
        ("ui.animations.on", AnimationPref::On, true),
        ("ui.animations.off", AnimationPref::Off, false),
        ("ui.animations.system", AnimationPref::System, false),
    ] {
        menus::activate(&mut a, id);
        assert_eq!((a.ui.animations, a.shows_animations()), (pref, shown), "{id}");
    }
    // The JSON form sets and reports it; a bad choice changes nothing.
    assert_eq!(a.run("ui.animations", json!({ "animations": "on" })).ok(), Some(json!({ "animations": "on", "shown": true })));
    assert_eq!(a.run("ui.animations", json!({})).ok(), Some(json!({ "animations": "on", "shown": true })), "no choice: report it");
    assert_eq!(a.run("ui.animations", json!({ "animations": "system" })).ok(), Some(json!({ "animations": "system", "shown": false })));
    assert!(a.run("ui.animations", json!({ "animations": "wobbly" })).is_err());
    assert_eq!(a.ui.animations, AnimationPref::System);
}

#[test]
fn the_choice_is_a_saved_preference() {
    let mut a = app();
    a.start("ui.animations.off");
    // Saved and restored by the host; bad values are ignored.
    let mut b = app();
    b.load_prefs(&a.prefs_json());
    assert_eq!(b.ui.animations, AnimationPref::Off);
    b.load_prefs(r#"{"animations": "wobbly"}"#);
    b.load_prefs(r#"{"animations": 7}"#);
    assert_eq!(b.ui.animations, AnimationPref::Off);
    // Resetting the palettes keeps it, as it keeps the theme.
    b.start("ui.resetpalettes");
    assert_eq!(b.ui.animations, AnimationPref::Off);
    // OPTIONS reports and sets it, all or nothing, and its reset goes back to following the OS.
    assert_eq!(b.run("options", json!({})).unwrap()["animations"], "off");
    assert!(b.run("options", json!({ "animations": "on", "PICKBOX": "big" })).is_err());
    assert!(b.run("options", json!({ "animations": true })).is_err());
    assert_eq!(b.ui.animations, AnimationPref::Off);
    assert_eq!(b.run("options", json!({ "animations": "on" })).unwrap()["animations"], "on");
    assert_eq!(b.run("options", json!({ "reset": true })).unwrap()["animations"], "system");
    // `ui.set` (the UiState JSON) reaches it too.
    let ui: UiState = serde_json::from_value(json!({ "animations": "off" })).unwrap();
    assert_eq!(ui.animations, AnimationPref::Off);
    assert!(serde_json::from_value::<UiState>(json!({ "animations": "wobbly" })).is_err());
}
