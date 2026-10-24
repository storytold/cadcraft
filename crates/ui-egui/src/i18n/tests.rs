use std::collections::BTreeSet;

use super::*;
use crate::{CadApp, Services};
use cadcraft_engine::Session;
use serde_json::json;

fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(key, _)| key)).collect()
}

/// Languages with a catalog (every one but English).
fn translated() -> impl Iterator<Item = &'static Language> {
    LANGUAGES.iter().filter(|l| !l.source.is_empty())
}

#[test]
fn catalog_is_valid_and_preserves_placeholders_and_ellipses() {
    for language in &LANGUAGES {
        let (catalog, errors) = parse(language.source);
        assert!(errors.is_empty(), "{}: {errors:?}", language.code);
        for ((context, key), value) in catalog {
            assert!(["", "menu", "@plural"].contains(&context), "unknown context: {context}");
            let forms: Vec<_> = if context == "@plural" { value.split('|').collect() } else { vec![value] };
            if context == "@plural" {
                assert_eq!(forms.len(), language.plural_forms, "{}: {key}", language.code);
            }
            for form in forms {
                assert_eq!(placeholders(key), placeholders(form), "{key}");
                for suffix in ["...", "…"] {
                    assert_eq!(key.ends_with(suffix), form.ends_with(suffix), "{key}");
                }
            }
        }
        assert_eq!(tr(language.code, "a future untranslated label"), "a future untranslated label");
    }
    let (parsed, errors) = parse("broken\n\tOpen\tВідкрити\n\tOpen\tВідкрити\n\tEmpty\t\n");
    assert_eq!(parsed.len(), 1);
    assert_eq!(errors.len(), 3);
    assert_eq!(tr("unknown", "Open"), "Open");
}

/// What each language shows in English for now. Missing translations never fail a build:
/// `cargo xtask i18n`, a step of `cargo xtask ci`, runs this to print the list.
#[test]
#[ignore = "a report, printed by `cargo xtask i18n`"]
fn translation_gaps() {
    for language in translated() {
        let mut missing: BTreeSet<String> = missing_commands_menus_tools_and_snap_labels(language.code).into_iter().collect();
        missing.extend(missing_marked_ui_literals(language.code));
        if missing.is_empty() {
            println!("{}: nothing missing", language.code);
            continue;
        }
        println!("{}: {} strings fall back to English", language.code, missing.len());
        for source in &missing {
            println!("  {source}");
        }
    }
}

fn missing_commands_menus_tools_and_snap_labels(language: &str) -> Vec<String> {
    let catalog = catalog(language).unwrap();
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
    sources.into_iter().filter(|s| !s.is_empty() && !catalog.contains_key(&("", *s))).map(str::to_string).collect()
}

fn missing_marked_ui_literals(language: &str) -> BTreeSet<String> {
    let catalog = catalog(language).unwrap();
    let sources = [
        include_str!("../xrefs.rs"),
        include_str!("../canvas.rs"),
        include_str!("../chrome.rs"),
        include_str!("../cmdline.rs"),
        include_str!("../credits.rs"),
        include_str!("../palettes/quickcalc.rs"),
        include_str!("../palettes/patterns.rs"),
        include_str!("../dialogs.rs"),
        include_str!("../hatchdlg.rs"),
        include_str!("../aliasdlg.rs"),
        include_str!("../mtext_editor.rs"),
        include_str!("../layers.rs"),
        include_str!("../canvas/compare.rs"),
        include_str!("../canvas/blockedit.rs"),
        include_str!("../bedit.rs"),
        include_str!("../palettes.rs"),
        include_str!("../parametric.rs"),
        include_str!("../quick.rs"),
        include_str!("../vpscale.rs"),
        include_str!("../options/dialog.rs"),
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
    missing
}

#[test]
fn locale_and_saved_preference_resolution() {
    for tag in ["uk", "uk-UA", "uk_UA.UTF-8", "UK-ua", " uk-UA ", "uk_UA@euro"] {
        assert_eq!(locale_language(tag), Some("uk"), "{tag}");
    }
    for tag in ["de", "de-DE", "de_DE.UTF-8", "DE-at", "de-CH"] {
        assert_eq!(locale_language(tag), Some("de"), "{tag}");
    }
    for (tag, code) in [("ru", "ru"), ("ru-RU", "ru"), ("ru_RU.UTF-8", "ru"), ("fr", "fr"), ("fr-FR", "fr"), ("fr_CA.UTF-8", "fr")] {
        assert_eq!(locale_language(tag), Some(code), "{tag}");
    }
    for tag in ["pt", "pt-BR", "pt_BR.UTF-8", "PT-br", "pt-PT"] {
        assert_eq!(locale_language(tag), Some("pt-BR"), "{tag}");
    }
    for tag in ["es", "es-ES", "es_MX.UTF-8", "ES-ar"] {
        assert_eq!(locale_language(tag), Some("es"), "{tag}");
    }
    for (code, tags) in [
        ("cs", &["cs", "cs-CZ", "cs_CZ.UTF-8", "CS-cz", " cs-CZ ", "cs_CZ@euro"][..]),
        ("it", &["it", "it-IT", "it_IT.UTF-8", "IT-ch", " it-CH ", "it_IT@euro"]),
        ("ko", &["ko", "ko-KR", "ko_KR.UTF-8", "KO-kr", " ko-KR ", "ko_KR@euro"]),
        ("sv", &["sv", "sv-SE", "sv_SE.UTF-8", "SV-se", " sv-SE ", "sv-FI"]),
        ("hu", &["hu", "hu-HU", "hu_HU.UTF-8", "HU-hu", " hu-HU "]),
        ("tr", &["tr", "tr-TR", "tr_TR.UTF-8", "TR-tr", " tr-TR ", "tr-CY"]),
        ("el", &["el", "el-GR", "el_GR.UTF-8", "EL-cy"]),
        ("zh", &["zh", "zh-CN"]),
    ] {
        for tag in tags {
            assert_eq!(locale_language(tag), Some(code), "{tag}");
        }
    }
    // Look-alike codes, three-letter codes and endonyms pick nothing: `se` is Northern Sami.
    let look_alikes = ["ua", "ukulele", "pl-PL", "", "garbage", "💙", "ptolemy", "cz", "csharp", "csomething", "italic"];
    let others = ["kr", "korean", "kotlin", "se", "swe", "svenska", "hun", "magyar", "tur", "türkçe", "elvish"];
    for tag in look_alikes.into_iter().chain(others) {
        assert_eq!(locale_language(tag), None, "{tag}");
    }
    assert_eq!(Preference::Auto.resolve(&["pl-PL".into(), "pt-BR".into()]), "pt-BR");
    assert_eq!(Preference::Auto.resolve(&["fr-FR".into(), "pt-BR".into()]), "fr");
    assert_eq!(Preference::Auto.resolve(&["es-ES".into(), "pt-BR".into()]), "es");
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
        assert_eq!(saved, format!("\"{}\"", preference.code()));
        assert_eq!(serde_json::from_str::<Preference>(&saved).unwrap(), preference);
        if preference == Preference::Auto {
            continue;
        }
        assert_eq!(Preference::parse(preference.code()), Some(preference));
        // A chosen language wins over the system's; Auto takes the first system language there is.
        assert_eq!(preference.resolve(&["uk-UA".into()]), preference.code());
        if preference != Preference::English {
            assert_eq!(Preference::Auto.resolve(&["pl-PL".into(), preference.code().into(), "en-US".into()]), preference.code());
        }
        // Saved with the host's other preferences; a bad saved value keeps the current choice.
        let mut a = CadApp::new(Session::new(), Services::default());
        a.ui.interface_language = preference;
        let mut b = CadApp::new(Session::new(), Services::default());
        b.load_prefs(&a.prefs_json());
        assert_eq!(b.ui.interface_language, preference);
        b.load_prefs(r#"{"interfaceLanguage":"klingon"}"#);
        assert_eq!(b.ui.interface_language, preference);
    }
    assert!(serde_json::from_str::<Preference>("\"unsupported\"").is_err());
    assert_eq!(serde_json::from_value::<crate::UiState>(json!({})).unwrap().interface_language, Preference::Auto);
}

#[test]
fn plural_rules_pick_the_cldr_form() {
    let uk = [0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 111, 114, u64::MAX].map(plural_uk);
    assert_eq!(uk, [2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 2, 0, 2, 2, 2]);
    assert_eq!([0, 1, 2, 5, 21, u64::MAX].map(plural_pt), [0, 0, 1, 1, 1, 1]);
    assert_eq!([0, 1, 2, 4, 5, 11, 21, 22, 25, u64::MAX].map(plural_cs), [2, 0, 1, 1, 2, 2, 2, 2, 2, 2]);
    for language in &LANGUAGES {
        for n in [0, 1, 2, 3, 5, 11, 21, 22, 101, 111, u64::MAX] {
            assert!((language.plural)(n) < language.plural_forms, "{}: {n}", language.code);
        }
    }
}

#[test]
fn counts_and_templates_follow_each_catalog_and_keep_inserted_text() {
    for language in &LANGUAGES {
        with_language(language.code, || {
            for (one, other) in [("{n} contributor", "{n} contributors"), ("{n} commit", "{n} commits")] {
                for n in [0, 1, 2, 3, 5, 11, 21, 22, 101, u64::MAX] {
                    let form =
                        catalog(language.code).and_then(|c| c.get(&("@plural", one))).and_then(|forms| forms.split('|').nth((language.plural)(n)));
                    let expected = form.unwrap_or(if n == 1 { one } else { other }).replace("{n}", &n.to_string());
                    assert_eq!(count(n, one, other), expected, "{}: {n}", language.code);
                }
            }
            // Inserted values are never looked up or substituted again.
            assert_eq!(format("Current: {color}", &[("color", "{n} Open".into())]), t("Current: {color}").replace("{color}", "{n} Open"));
            assert_eq!(format("{b}/{a}", &[("a", "А".into()), ("b", "Б".into())]), "Б/А");
        });
    }
    with_language("uk", || {
        let layers = t("Layers");
        with_language("en", || assert_eq!(t("Layers"), "Layers"));
        assert_eq!(t("Layers"), layers);
    });
    assert_eq!(t("Layers"), "Layers", "language must not leak out of a render scope");
    assert_eq!(count(2, "{n} contributor", "{n} contributors"), "2 contributors");
}

#[test]
fn the_wcs_pill_draws_its_arrow_instead_of_a_glyph_a_font_may_lack() {
    // #1541: "WCS ⌄" showed a box where the UI font had no "⌄"; the chevron is now drawn as
    // lines, so no catalog and no language paints the glyph.
    let ctx = egui::Context::default();
    let mut app = CadApp::new(Session::new(), Services::default());
    for language in &LANGUAGES {
        assert!(!language.source.contains('\u{2304}'), "{}.tsv still has the \u{2304} glyph", language.code);
        app.run("ui.language", json!({ "lang": language.code })).unwrap();
        app.logic(&ctx);
        let label = tr(app.language(), "WCS");
        let text = render(&mut app, &ctx);
        assert!(text.lines().any(|l| l == label), "{}: the pill reads {label}", language.code);
        assert!(!text.contains('\u{2304}'), "{}: no \u{2304} glyph is painted", language.code);
    }
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
    assert!(render(&mut app, &ctx).contains("Properties"));
    let commands = crate::control::all_commands(&app);
    let (menu_request, _) = crate::ControlRequest::new("ui.menu.list", json!({}));
    let crate::control::Outcome::Done(menu_before) = crate::control::handle(&mut app, &ctx, &menu_request) else { panic!("expected menu reply") };
    app.start("line");
    let prompt = crate::control::cmdline_state(&app);
    for language in translated() {
        app.run("ui.language", json!({ "lang": language.code })).unwrap();
        app.logic(&ctx);
        // Without a font for its script, a language stays chosen and the interface shows English.
        let shown = app.language();
        assert_eq!(shown, if crate::theme::can_show(language.code) { language.code } else { "en" });
        assert_eq!(app.fonts_language, Some(shown));
        let text = render(&mut app, &ctx);
        for source in ["Properties", "File"] {
            assert!(text.contains(tr(shown, source)), "{}: {source}: {text}", language.code);
        }
        assert!(text.contains("Open"), "the drawing title must remain unchanged");
        assert_eq!(crate::control::cmdline_state(&app), prompt);
        assert_eq!(crate::control::all_commands(&app), commands);
        let crate::control::Outcome::Done(menu_after) = crate::control::handle(&mut app, &ctx, &menu_request) else { panic!("expected menu reply") };
        assert_eq!(menu_before, menu_after);
        assert_eq!(app.session.state().unwrap().title, "Open");
        assert_eq!(t("Properties"), "Properties", "language must not leak out of the render scope");
        assert!(app.run("ui.language", json!({"lang":"bad"})).is_err());
        app.run("ui.resetpalettes", json!({})).unwrap();
        assert_eq!(app.language(), shown);
    }
    // The general ui.set route saves the same preference and installs the same fonts.
    for code in ["ko", "zh", "en"] {
        let (request, _) = crate::ControlRequest::new("ui.set", json!({ "interfaceLanguage": code }));
        let _ = crate::control::handle(&mut app, &ctx, &request);
        assert_eq!(app.ui.interface_language, Preference::parse(code).unwrap());
        app.logic(&ctx);
        assert_eq!(app.fonts_language, Some(app.language()));
        render(&mut app, &ctx);
    }
    assert!(render(&mut app, &ctx).contains("Properties"));
    // The menu tree used by the control channel keeps its original labels and IDs.
    assert_eq!(crate::menus::tree(&app).first().unwrap().0, "File");
}

#[test]
fn dialogs_render_in_every_language_and_keep_their_source_ids() {
    for language in translated() {
        for (dialog, heading) in [
            ("dsettings", "Drafting Settings"),
            ("about", "About CADCraft"),
            ("commands", "Command Reference"),
            ("blocks", "Blocks"),
            ("layers", "Layer Properties Manager"),
            ("qselect", "Quick Select"),
            ("parameters", "Parameters Manager"),
            ("language", "Interface Language"),
            ("hatch", "Hatch and Gradient"),
            ("options:preferences", "User Preferences"),
        ] {
            let mut app = CadApp::new(Session::new(), Services::default());
            app.ui.interface_language = Preference::parse(language.code).unwrap();
            app.ui.dialog = Some(dialog.into());
            let ctx = egui::Context::default();
            app.logic(&ctx);
            let text = render(&mut app, &ctx);
            assert!(text.contains(tr(app.language(), heading)), "{}: {dialog}: {text}", language.code);
            assert_eq!(app.ui.dialog.as_deref(), Some(dialog));
        }
    }
}

/// The letters of every catalog have glyphs in both UI families. Latin, Cyrillic and Greek come
/// from egui's bundled fonts alone, as in the web build; the scripts they lack need a font that
/// `install_fonts` finds, and are checked only where one is there.
#[test]
fn interface_fonts_cover_every_letter_in_each_catalog() {
    for language in translated() {
        let extra_font = ["ko", "zh", "ar"].contains(&language.code);
        if extra_font && !crate::theme::can_show(language.code) {
            continue;
        }
        let ctx = egui::Context::default();
        if extra_font {
            crate::theme::install_fonts(&ctx, language.code);
        }
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        let letters: BTreeSet<char> = language
            .source
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split('\t').nth(2))
            .flat_map(str::chars)
            .filter(|c| !c.is_ascii() && c.is_alphabetic())
            .collect();
        let missing = ctx.fonts_mut(|view| {
            let mut missing = Vec::new();
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                // In egui 0.36, has_glyph can report a false negative for glyphs in the same
                // face as the replacement character. Check the family's actual character maps.
                let mut font = view.fonts.font(&family);
                for c in &letters {
                    if !font.characters().contains_key(c) {
                        missing.push(format!("{family:?}: {c} (U+{:04X})", u32::from(*c)));
                    }
                }
            }
            missing
        });
        assert!(missing.is_empty(), "{}: missing glyphs: {}", language.code, missing.join(", "));
    }
}

/// Chinese shows only with a font for it: an installed CJK font, or one this build embedded from
/// craft-fonts (the web build has no system fonts). Without either the interface stays English
/// instead of drawing boxes, and choosing it says why.
#[test]
fn chinese_needs_a_cjk_font() {
    assert_eq!(locale_language("zh-CN"), Some("zh"));
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("ui.language", json!({"lang": "zh"})).unwrap();
    assert_eq!(r["interfaceLanguage"], "zh");
    let shown = crate::theme::can_show("zh");
    assert_eq!(app.language(), if shown { "zh" } else { "en" });
    assert_eq!(r["shown"], app.language());
    assert!(crate::theme::can_show("uk") && crate::theme::can_show("en"));
    if !shown {
        assert!(app.status.as_ref().is_some_and(|(s, _)| s.contains("No Chinese font")));
    }
}

#[test]
fn korean_without_a_hangul_font_stays_english_and_says_why() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("ui.language", json!({"lang": "ko"})).unwrap();
    let shown = crate::theme::can_show("ko");
    assert_eq!(r["shown"], if shown { "ko" } else { "en" });
    assert_eq!(app.ui.interface_language, Preference::Korean);
    if !shown {
        assert!(app.status.as_ref().is_some_and(|(s, _)| s.contains("No font covers the Korean interface")));
        assert_eq!(Preference::Korean.name(), "Korean");
    }
}

#[test]
fn korean_ui_fonts_cover_the_language_picker_in_both_families() {
    if !crate::theme::can_show("ko") {
        return;
    }
    // The picker shows 한국어 in an English interface only when the CJK fonts loaded in every
    // language cover Hangul; the Korean interface itself always does, with a fallback font.
    let ctx = egui::Context::default();
    let english = crate::theme::cjk_covers_hangul();
    assert_eq!(with_language("en", || Preference::Korean.name()), if english { "한국어" } else { "Korean" });
    assert_eq!(with_language("ko", || Preference::Korean.name()), "한국어");
    crate::theme::install_fonts(&ctx, if english { "en" } else { "ko" });
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    let missing = ctx.fonts_mut(|view| {
        let mut missing = Vec::new();
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let mut font = view.fonts.font(&family);
            for c in "한국어".chars() {
                if !font.characters().contains_key(&c) {
                    missing.push(format!("{family:?}: {c}"));
                }
            }
        }
        missing
    });
    assert!(missing.is_empty(), "missing Korean picker glyphs: {}", missing.join(", "));
}

#[test]
fn traditional_chinese_locale_selection_respects_script_and_region() {
    for tag in ["zh-Hant", "zh-Hant-TW", "zh_Hant_HK.UTF-8", "zh-TW", "ZH-hk", "zh-MO", "zh-Hant-CN", "zh-Hant-TW-u-nu-latn"] {
        assert_eq!(locale_language(tag), Some("zh-Hant"), "{tag}");
    }
    for tag in ["zh", "zh-CN", "zh-SG", "zh-Hans", "zh_Hans_TW", "zh-Hans-TW"] {
        assert_eq!(locale_language(tag), Some("zh"), "{tag}");
    }
    for tag in ["zh-Latn-TW", "zh-Hant-Hans", "zh--TW"] {
        assert_eq!(locale_language(tag), None, "{tag}");
    }
    let locales = vec!["zh-TW".into(), "zh-Hans-CN".into(), "en-US".into()];
    assert_eq!(Preference::Auto.resolve(&locales), "zh-Hant");
    assert_eq!(Preference::English.resolve(&locales), "en");
    assert_eq!(Preference::TraditionalChinese.resolve(&["en-US".into()]), "zh-Hant");
    assert_eq!(Preference::restore(Some("zh-Hant")), Preference::TraditionalChinese);
    assert_eq!(Preference::restore(Some("zh-Hans")), Preference::Auto);
    let mut saved = CadApp::new(Session::new(), Services::default());
    saved.ui.interface_language = Preference::TraditionalChinese;
    let mut restored = CadApp::new(Session::new(), Services::default());
    restored.load_prefs(&saved.prefs_json());
    assert_eq!(restored.ui.interface_language, Preference::TraditionalChinese);
    restored.load_prefs(r#"{"interfaceLanguage":"unsupported"}"#);
    assert_eq!(restored.ui.interface_language, Preference::TraditionalChinese);
}

#[test]
fn duplicate_chinese_regions_are_not_locale_candidates() {
    for tag in
        ["zh-TW-CN", "zh-CN-TW", "zh-HK-MO", "zh-TW-TW", "zh-419-TW", "zh-TW-419", "zh-419-001", "zh-Hant-TW-CN", "zh-Hans-CN-TW", "ZH_tw_cn.UTF-8"]
    {
        assert_eq!(locale_language(tag), None, "{tag}");
    }
}

#[test]
fn auto_skips_duplicate_regions_and_preserves_valid_locale_priority() {
    for invalid in ["zh-TW-CN", "zh-Hans-CN-TW", "zh-419-TW", "zh-TW-419"] {
        for (later, expected) in [("de-DE", "de"), ("zh-Hant-HK", "zh-Hant"), ("zh-Hans-TW", "zh"), ("xx-XX", "en")] {
            let locales = [invalid.into(), later.into(), "en-US".into()];
            assert_eq!(Preference::Auto.resolve(&locales), expected, "{invalid}, {later}");
        }
        let locales = [invalid.into(), "zz-ZZ".into(), "zh-TW".into(), "de-DE".into()];
        assert_eq!(Preference::Auto.resolve(&locales), "zh-Hant");
        assert_eq!(Preference::Auto.resolve(&[invalid.into()]), "en");
    }
    for (tag, expected) in [
        ("zh-419", "zh"),
        ("zh-Hant-419", "zh-Hant"),
        ("zh-Hans-HK", "zh"),
        ("zh-Hant-TW-variant", "zh-Hant"),
        ("zh-TW-u-rg-cnzzzz", "zh-Hant"),
        ("zh-TW-x-cn", "zh-Hant"),
    ] {
        assert_eq!(locale_language(tag), Some(expected), "{tag}");
    }
    assert_eq!(Preference::Auto.resolve(&["de-DE".into(), "zh-TW".into()]), "de");
}

#[test]
fn traditional_chinese_counts_and_user_values_are_preserved() {
    with_language("zh-Hant", || {
        for n in [0, 1, 2, 11, 21, 100, u64::MAX] {
            assert_eq!(plural_zh(n), 0);
            assert_eq!(count(n, "{n} contributor", "{n} contributors"), format!("{n} 位貢獻者"));
            assert_eq!(count(n, "{n} commit", "{n} commits"), format!("{n} 次提交"));
        }
        assert_eq!(t("Layers"), "圖層");
        assert_eq!(t("uncatalogued text"), "uncatalogued text");
        assert_eq!(format("Current: {color}", &[("color", "{n} Open".into())]), "目前：{n} Open");
        with_language("en", || assert_eq!(t("Layers"), "Layers"));
        assert_eq!(t("Layers"), "圖層");
    });
    assert_eq!(t("Layers"), "Layers");
}

#[test]
fn traditional_chinese_live_switch_preserves_english_automation_and_user_data() {
    if !crate::theme::can_show("zh-Hant") {
        eprintln!("SKIP: no installed or embedded CJK font; Chinese render remains unverified");
        return;
    }
    let mut app = CadApp::new(Session::new(), Services::default());
    app.ui.in_window_menu = true;
    app.session.state_mut().unwrap().title = "Layers {n}".into();
    let ctx = egui::Context::default();
    app.logic(&ctx);
    assert!(render(&mut app, &ctx).contains("Properties"));
    let commands = crate::control::all_commands(&app);
    let (menu_request, _) = crate::ControlRequest::new("ui.menu.list", json!({}));
    let crate::control::Outcome::Done(menu_before) = crate::control::handle(&mut app, &ctx, &menu_request) else { panic!("expected menu reply") };
    app.start("line");
    let prompt = crate::control::cmdline_state(&app);
    app.run("ui.language", json!({"lang":"zh-Hant"})).unwrap();
    let text = render(&mut app, &ctx);
    assert!(text.contains("性質"), "{text}");
    assert!(text.contains("檔案"), "{text}");
    assert!(text.contains("指定第一點"), "{text}");
    assert!(text.contains("Layers {n}"), "user title must remain unchanged");
    assert_eq!(crate::control::cmdline_state(&app), prompt);
    assert_eq!(crate::control::all_commands(&app), commands);
    let crate::control::Outcome::Done(menu_after) = crate::control::handle(&mut app, &ctx, &menu_request) else { panic!("expected menu reply") };
    assert_eq!(menu_before, menu_after);
    assert_eq!(app.session.state().unwrap().title, "Layers {n}");
    assert_eq!(t("Properties"), "Properties");
    app.run("ui.resetpalettes", json!({})).unwrap();
    assert_eq!(app.language(), "zh-Hant");
    app.run("ui.language", json!({"lang":"en"})).unwrap();
    assert!(render(&mut app, &ctx).contains("Properties"));
}

#[test]
fn available_fonts_cover_traditional_chinese_catalog() {
    if !crate::theme::can_show("zh-Hant") {
        eprintln!("SKIP: no installed or embedded CJK font; glyph coverage remains unverified");
        return;
    }
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "zh-Hant");
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    let letters: BTreeSet<_> = include_str!("zh-Hant.tsv").chars().filter(|c| !c.is_ascii() && !c.is_whitespace()).collect();
    ctx.fonts_mut(|view| {
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let mut font = view.fonts.font(&family);
            for c in &letters {
                assert!(font.characters().contains_key(c), "missing {c} in {family:?}");
            }
        }
    });
}

#[test]
fn traditional_chinese_rule_is_registered_without_changing_other_languages() {
    let language = LANGUAGES.iter().find(|language| language.code == "zh-Hant").unwrap();
    assert_eq!(language.plural_forms, 1);
    for n in [0, 1, 2, 11, 21, 100, u64::MAX] {
        assert_eq!((language.plural)(n), 0);
    }
    for (code, forms) in [("en", 2), ("uk", 3), ("pt-BR", 2), ("de", 2), ("es", 2), ("cs", 3), ("ru", 3), ("fr", 2)] {
        assert_eq!(LANGUAGES.iter().find(|language| language.code == code).unwrap().plural_forms, forms);
    }
    assert_eq!(Preference::BrazilianPortuguese.resolve(&["zh-TW".into()]), "pt-BR");
    assert_eq!(Preference::Auto.resolve(&["pt-PT".into()]), "pt-BR");
    assert_eq!(Preference::Auto.resolve(&["de-DE".into()]), "de");
    assert_eq!(Preference::Auto.resolve(&["es-MX".into()]), "es");
}

#[test]
fn traditional_chinese_shares_cjk_availability_and_keeps_saved_choice() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let reply = app.run("ui.language", json!({"lang": "zh-Hant"})).unwrap();
    assert_eq!(reply["interfaceLanguage"], "zh-Hant");
    assert_eq!(app.ui.interface_language, Preference::TraditionalChinese);
    assert_eq!(app.language(), if crate::theme::can_show("zh-Hant") { "zh-Hant" } else { "en" });
    assert_eq!(reply["shown"], app.language());
    if !crate::theme::can_show("zh-Hant") {
        assert!(app.status.as_ref().is_some_and(|(text, _)| text.contains("No Chinese font")));
    }
    let mut restored = CadApp::new(Session::new(), Services::default());
    restored.load_prefs(&app.prefs_json());
    assert_eq!(restored.ui.interface_language, Preference::TraditionalChinese);
    assert_eq!(restored.language(), app.language());
}

#[test]
fn chinese_font_cache_tracks_tc_sc_and_other_language_switches() {
    let mut app = CadApp::new(Session::new(), Services::default());
    let ctx = egui::Context::default();
    for choice in [Preference::Chinese, Preference::TraditionalChinese, Preference::Chinese, Preference::English, Preference::TraditionalChinese] {
        app.ui.interface_language = choice;
        app.logic(&ctx);
        assert_eq!(app.fonts_language, Some(app.language()));
        assert_eq!(app.ui.interface_language, choice);
    }
    app.ui.interface_language = Preference::Auto;
    for tag in ["zh-TW", "zh-CN", "zh-HK", "it-IT"] {
        app.system_languages = vec![tag.into()];
        app.logic(&ctx);
        assert_eq!(app.fonts_language, Some(app.language()));
    }
}

fn assert_no_stale_catalog_keys(reference_source: &str, translated_source: &str) {
    let (reference, reference_errors) = parse(reference_source);
    let (translated, translated_errors) = parse(translated_source);
    assert!(reference_errors.is_empty() && translated_errors.is_empty());
    let reference_keys: BTreeSet<_> = reference.keys().copied().collect();
    let translated_keys: BTreeSet<_> = translated.keys().copied().collect();
    let stale: Vec<_> = translated_keys.difference(&reference_keys).collect();
    assert!(stale.is_empty(), "catalog keys absent from reference: {stale:?}");
}

#[test]
fn traditional_chinese_has_no_stale_reference_keys() {
    // Coverage gaps stay non-fatal and are listed by translation_gaps for every catalog.
    assert_no_stale_catalog_keys(include_str!("uk.tsv"), include_str!("zh-Hant.tsv"));
}

#[test]
fn traditional_chinese_gaps_are_reported_not_required() {
    assert!(translated().any(|language| language.code == "zh-Hant"));
    assert_no_stale_catalog_keys("\tExisting\tExisting\n\tNew command\tNew command\n", "\tExisting\t現有\n");
    assert_eq!(tr("zh-Hant", "a future untranslated label"), "a future untranslated label");
}

#[test]
#[should_panic(expected = "catalog keys absent from reference")]
fn traditional_chinese_stale_keys_are_rejected() {
    assert_no_stale_catalog_keys("\tExisting\tExisting\n", "\tRemoved command\t已移除的指令\n");
}
