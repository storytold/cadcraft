use std::collections::BTreeSet;

use super::*;
use crate::{CadApp, Services};
use cadcraft_engine::Session;
use serde_json::json;

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(key, _)| key)).collect()
}

#[test]
fn catalog_is_valid_and_preserves_placeholders_and_ellipses() {
    for language in &LANGUAGES {
        let (catalog, errors) = parse(language.source);
        assert!(errors.is_empty(), "{}: {errors:?}", language.code);
        for ((context, key), value) in catalog {
            assert!(["", "@plural"].contains(&context), "unknown context: {context}");
            let forms: Vec<_> = if context == "@plural" { value.split('|').collect() } else { vec![value] };
            if context == "@plural" {
                assert_eq!(forms.len(), 3, "{key}");
            }
            for form in forms {
                assert_eq!(placeholders(key), placeholders(form), "{key}");
                for suffix in ["...", "…"] {
                    assert_eq!(key.ends_with(suffix), form.ends_with(suffix), "{key}");
                }
            }
        }
    }
    let (parsed, errors) = parse("broken\n\tOpen\tВідкрити\n\tOpen\tВідкрити\n\tEmpty\t\n");
    assert_eq!(parsed.len(), 1);
    assert_eq!(errors.len(), 3);
    assert_eq!(tr("uk", "a future untranslated label"), "a future untranslated label");
    assert_eq!(tr("unknown", "Open"), "Open");
}

#[test]
fn ukrainian_covers_commands_menus_tools_and_snap_labels() {
    let catalog = catalog("uk").unwrap();
    let mut sources = BTreeSet::new();
    sources.extend(crate::menus::MENUS.iter().copied());
    for c in cadcraft_engine::command_specs() {
        sources.insert(c.label);
        sources.extend(c.menu.iter().copied());
    }
    for (_, label, path, _) in crate::menus::UI_COMMANDS {
        sources.insert(*label);
        sources.extend(path.iter().copied());
    }
    for groups in [crate::palettes::DRAFTING, crate::palettes::MODELING] {
        for (name, large, small) in groups {
            sources.insert(*name);
            sources.extend(large.iter().chain(*small).map(|(_, _, label)| *label));
        }
    }
    sources.extend(cadcraft_engine::snap::mode::ALL.iter().map(|(_, name)| *name));
    let missing: Vec<_> = sources.into_iter().filter(|s| !s.is_empty() && !catalog.contains_key(&("", *s))).collect();
    assert!(missing.is_empty(), "missing translations: {missing:?}");
}

#[test]
fn every_marked_ui_literal_has_a_translation() {
    let catalog = catalog("uk").unwrap();
    let sources = [
        include_str!("../canvas.rs"),
        include_str!("../chrome.rs"),
        include_str!("../cmdline.rs"),
        include_str!("../credits.rs"),
        include_str!("../dialogs.rs"),
        include_str!("../layers.rs"),
        include_str!("../palettes.rs"),
        include_str!("../parametric.rs"),
        include_str!("../quick.rs"),
    ];
    let mut missing = BTreeSet::new();
    for source in sources {
        for marker in ["crate::tl!(", "crate::tf!("] {
            for rest in source.split(marker).skip(1) {
                // These macros require a quoted literal. Use JSON's string decoding for escapes.
                let rest = rest.trim_start();
                let mut escape = false;
                let end = rest
                    .char_indices()
                    .skip(1)
                    .find_map(|(i, c)| {
                        if escape {
                            escape = false;
                            None
                        } else if c == '\\' {
                            escape = true;
                            None
                        } else if c == '"' {
                            Some(i + 1)
                        } else {
                            None
                        }
                    })
                    .unwrap();
                let key: String = serde_json::from_str(&rest[..end]).unwrap();
                // Coordinate axes, glyph buttons and product names are not translated.
                if !["X", "Y", "B", "CADCraft"].contains(&key.as_str()) && !catalog.contains_key(&("", key.as_str())) {
                    missing.insert(key);
                }
            }
        }
    }
    assert!(missing.is_empty(), "missing UI translations: {missing:?}");
}

#[test]
fn locale_and_saved_preference_resolution() {
    for tag in ["uk", "uk-UA", "uk_UA.UTF-8", "UK-ua", " uk-UA ", "uk_UA@euro"] {
        assert_eq!(locale_language(tag), Some("uk"), "{tag}");
    }
    for tag in ["ua", "ukulele", "ru-RU", "", "garbage", "💙"] {
        assert_eq!(locale_language(tag), None, "{tag}");
    }
    let locales = vec!["pl-PL".into(), "uk-UA".into(), "en-US".into()];
    assert_eq!(Preference::Auto.resolve(&locales), "uk");
    assert_eq!(Preference::English.resolve(&locales), "en");
    assert_eq!(Preference::Auto.resolve(&["en-US".into(), "uk-UA".into()]), "en");
    assert_eq!(Preference::Auto.resolve(&[]), "en");
    assert_eq!(Preference::restore(None), Preference::Auto);
    assert_eq!(Preference::restore(Some("obsolete")), Preference::Auto);
    for preference in Preference::ALL {
        assert_eq!(Preference::restore(Some(preference.code())), preference);
        let saved = serde_json::to_string(&preference).unwrap();
        assert_eq!(serde_json::from_str::<Preference>(&saved).unwrap(), preference);
    }
    assert!(serde_json::from_str::<Preference>("\"unsupported\"").is_err());
    assert_eq!(serde_json::from_value::<crate::UiState>(json!({})).unwrap().interface_language, Preference::Auto);
    // Saved with the host's other preferences; a bad saved value keeps the current choice.
    let mut a = CadApp::new(Session::new(), Services::default());
    a.ui.interface_language = Preference::Ukrainian;
    let mut b = CadApp::new(Session::new(), Services::default());
    b.load_prefs(&a.prefs_json());
    assert_eq!(b.ui.interface_language, Preference::Ukrainian);
    b.load_prefs(r#"{"interfaceLanguage":"klingon"}"#);
    assert_eq!(b.ui.interface_language, Preference::Ukrainian);
}

#[test]
fn plural_forms_and_templates_preserve_inserted_user_data() {
    with_language("uk", || {
        for (n, form) in [
            (0, 2),
            (1, 0),
            (2, 1),
            (4, 1),
            (5, 2),
            (11, 2),
            (12, 2),
            (14, 2),
            (21, 0),
            (22, 1),
            (25, 2),
            (101, 0),
            (111, 2),
            (114, 2),
            (u64::MAX, 2),
        ] {
            assert_eq!(plural_uk(n), form, "{n}");
        }
        assert_eq!(count(1, "{n} contributor", "{n} contributors"), "1 учасник");
        assert_eq!(count(22, "{n} contributor", "{n} contributors"), "22 учасники");
        assert_eq!(count(11, "{n} contributor", "{n} contributors"), "11 учасників");
        assert_eq!(format("Current: {color}", &[("color", "{n} Open".into())]), "Поточний: {n} Open");
        assert_eq!(format("{b}/{a}", &[("a", "А".into()), ("b", "Б".into())]), "Б/А");
    });
    assert_eq!(t("Layers"), "Layers");
    with_language("uk", || {
        assert_eq!(t("Layers"), "Шари");
        with_language("en", || assert_eq!(t("Layers"), "Layers"));
        assert_eq!(t("Layers"), "Шари");
    });
    assert_eq!(count(2, "{n} contributor", "{n} contributors"), "2 contributors");
}

fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn collect(shape: &egui::Shape, text: &mut String) {
        match shape {
            egui::Shape::Text(s) => {
                text.push_str(s.galley.text());
                text.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    for shape in shapes {
        collect(&shape.shape, &mut text);
    }
    text
}

fn render(app: &mut CadApp, ctx: &egui::Context) -> String {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    // Windows fade in; a second pass makes their text visible without a time dependency.
    let mut text = String::new();
    for _ in 0..2 {
        let mut output = ctx.run_ui(input.clone(), |ui| app.ui(ui));
        text = painted_text(&output.shapes);
        output.textures_delta.clear();
    }
    text
}

#[test]
fn live_switch_translates_the_shell_without_mutating_drawing_or_automation() {
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.in_window_menu = true;
    app.session.state_mut().unwrap().title = "Open".into();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    let before = render(&mut app, &ctx);
    assert!(before.contains("Properties"));
    let commands = crate::control::all_commands(&app);
    let (menu_request, _) = crate::ControlRequest::new("ui.menu.list", json!({}));
    let crate::control::Outcome::Done(menu_before) = crate::control::handle(&mut app, &ctx, &menu_request) else { panic!("expected menu reply") };
    app.start("line");
    let prompt = crate::control::cmdline_state(&app);
    app.run("ui.language", json!({"lang":"uk"})).unwrap();
    let after = render(&mut app, &ctx);
    assert!(after.contains("Властивості"), "{after}");
    assert!(after.contains("Файл"), "{after}");
    assert!(after.contains("Укажіть першу точку"), "{after}");
    assert!(after.contains("Open"), "the drawing title must remain unchanged");
    assert_eq!(crate::control::cmdline_state(&app), prompt);
    assert_eq!(crate::control::all_commands(&app), commands);
    let crate::control::Outcome::Done(menu_after) = crate::control::handle(&mut app, &ctx, &menu_request) else { panic!("expected menu reply") };
    assert_eq!(menu_before, menu_after);
    assert_eq!(app.session.state().unwrap().title, "Open");
    assert_eq!(t("Properties"), "Properties", "language must not leak out of the render scope");
    assert!(app.run("ui.language", json!({"lang":"bad"})).is_err());
    assert_eq!(app.language(), "uk");
    app.run("ui.resetpalettes", json!({})).unwrap();
    assert_eq!(app.language(), "uk");
    app.run("ui.language", json!({"lang":"en"})).unwrap();
    assert!(render(&mut app, &ctx).contains("Properties"));
    // The menu tree used by the control channel keeps its original labels and IDs.
    assert_eq!(crate::menus::tree(&app).first().unwrap().0, "File");
}

#[test]
fn dialogs_render_in_ukrainian_and_keep_their_source_ids() {
    for (dialog, heading) in [
        ("dsettings", "Налаштування креслення"),
        ("about", "Про CADCraft"),
        ("commands", "Довідник команд"),
        ("blocks", "Блоки"),
        ("layers", "Диспетчер властивостей шарів"),
        ("qselect", "Швидкий вибір"),
        ("parameters", "Диспетчер параметрів"),
        ("language", "Мова інтерфейсу"),
    ] {
        let mut app = CadApp::new(Session::new(), Services::default());
        app.ui.interface_language = Preference::Ukrainian;
        app.ui.dialog = Some(dialog.into());
        let ctx = egui::Context::default();
        app.logic(&ctx);
        let text = render(&mut app, &ctx);
        assert!(text.contains(heading), "{dialog}: {text}");
        assert_eq!(app.ui.dialog.as_deref(), Some(dialog));
    }
}

#[test]
fn default_fonts_cover_every_ukrainian_letter_in_the_catalog() {
    let ctx = egui::Context::default();
    // Use only egui's bundled fonts, exactly as the web build does (no system-font loader).
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    let letters: BTreeSet<_> = include_str!("uk.tsv").chars().filter(|c| ('\u{0400}'..='\u{04ff}').contains(c)).collect();
    ctx.fonts_mut(|view| {
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            // In egui 0.36, has_glyph can report a false negative for glyphs in the same
            // face as the replacement character. Check the family's actual character maps.
            let mut font = view.fonts.font(&family);
            for c in &letters {
                assert!(font.characters().contains_key(c), "missing {c} in {family:?}");
            }
        }
    });
}
