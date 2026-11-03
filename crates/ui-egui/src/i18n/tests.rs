
    
  
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
            let chars = view.characters(&family);
            for c in "한국어".chars() {
                if !chars.contains_key(&c) {
                    missing.push(format!("{family:?}: {c}"));
                }
            }
        }
        missing
    });
    assert!(missing.is_empty(), "missing Korean picker glyphs: {}", missing.join(", "));
}

/// The production font chain covers alphanumeric ASCII in proportional and monospace, plus ⌘ ⇧ ↩ … − in proportional and ⇧ in monospace.
#[test]
fn production_font_chain_covers_latin_and_shortcut_symbols() {
    // Exercise the same chain the app installs: the workspace egui dependency enables
    // `default_fonts` and `monochrome_emoji_fonts` in production, so web and native builds
    // always start with bounded Latin and monochrome icon faces instead of an empty family.
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, "en");
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    ctx.fonts_mut(|view| {
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789".chars() {
                assert!(view.has_glyph(&egui::FontId::new(12.0, family.clone()), c), "Latin {c:?} missing in {family:?}");
            }
        }
        // Proportional shortcut labels (`Button::shortcut_text` and the translated menus) use
        // these symbols: ⌘ from the monochrome icon face, ⇧ from Hack as a bounded fallback.
        for c in ['\u{2318}', '\u{21e7}', '\u{21a9}', '\u{2026}', '\u{2212}'] {
            assert!(view.has_glyph(&egui::FontId::proportional(12.0), c), "U+{:04X} missing in Proportional", c as u32);
        }
        assert!(view.has_glyph(&egui::FontId::monospace(12.0), '\u{21e7}'), "U+21E7 shift symbol missing in Monospace");
    });
}

/// #1129: in Chinese every menu, submenu, command, tool palette and object-snap label is
/// translated, so no menu shows English entries.
#[test]
fn chinese_menus_and_commands_are_fully_translated() {
    assert_eq!(missing_commands_menus_tools_and_snap_labels("zh"), Vec::<String>::new());
}

/// Bosnian counts: 1, 21 `one`; 2–4, 22–24 (not 12–14) `few`; everything else `other`.
#[test]
fn bosnian_counts_take_one_few_other() {
    let forms: Vec<usize> = [0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 111].into_iter().map(plural_uk).collect();
    assert_eq!(forms, [2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 2, 2]);
    assert_eq!(Preference::parse("bs"), Some(Preference::Bosnian));
    assert_eq!(Preference::Bosnian.native_name(), "Bosanski");
}

/// Vietnamese has one form for every count, and shows only with an interface font that has its
/// tone-marked letters; without one it stays English and says why.
#[test]
fn vietnamese_takes_one_form_and_needs_a_font_with_its_letters() {
    assert_eq!(Preference::parse("vi"), Some(Preference::Vietnamese));
    assert_eq!(Preference::Vietnamese.native_name(), "Tiếng Việt");
    let vi = LANGUAGES.iter().find(|l| l.code == "vi").unwrap();
    assert_eq!(vi.plural_forms, 1);
    assert!([0, 1, 2, 5, 21, 111].into_iter().all(|n| (vi.plural)(n) == 0));
    let mut app = CadApp::new(Session::new(), Services::default());
    let r = app.run("ui.language", json!({"lang": "vi"})).unwrap();
    let shown = crate::theme::can_show("vi");
    assert_eq!(r["shown"], if shown { "vi" } else { "en" });
    if !shown {
        assert!(app.status.as_ref().is_some_and(|(s, _)| s.contains("Vietnamese letters")));
        assert_eq!(Preference::Vietnamese.name(), "Vietnamese");
    }
}

