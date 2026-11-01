//! Pluggable framework for transcoding legacy 8-bit CAD font encodings to Unicode NFC.
//!
//! Many regional CAD workflows from the 1990s–2000s paired custom 8-bit TrueType or SHX fonts
//! with single-byte text stored under `$DWGCODEPAGE ANSI_1252`. Those fonts repurpose Latin-1
//! code points for regional scripts and lack proper Unicode CMAP tables.
//!
//! This module separates the general dispatch mechanism (`LegacyProfile`, `font_hint`,
//! `transcode`, `substitute_face`) from language-specific rule sets. [`vietnamese`] (supporting
//! TCVN3, TCVN3H, and VNI Windows) is the first registered profile.

use std::borrow::Cow;

pub mod vietnamese;

/// Classification of the modern Unicode substitute typeface for a legacy 8-bit font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontClass {
    Serif,
    Sans,
}

/// A matched legacy encoding profile and its sub-variant identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileHit {
    pub profile: &'static str,
    pub variant: u8,
}

/// Pluggable profile for a legacy 8-bit CAD font encoding family.
pub trait LegacyProfile: Sync {
    /// Unique identifier of this profile (e.g. `"vietnamese"`).
    fn name(&self) -> &'static str;

    /// Check whether `font` (a font filename, family name, or CAD style name) belongs to this
    /// legacy profile.
    fn match_font(&self, font: &str) -> Option<ProfileHit>;

    /// Conservatively inspect text `s` when no recognized legacy font name was provided.
    fn detect(&self, s: &str) -> Option<ProfileHit>;

    /// Transcode legacy-encoded text `s` into Unicode NFC according to `hit`.
    fn decode<'a>(&self, s: &'a str, hit: ProfileHit) -> Cow<'a, str>;

    /// Choose the modern Unicode fallback class (`Serif` or `Sans`) for a matched legacy font.
    fn substitute(&self, font: &str) -> FontClass;
}

static PROFILES: &[&dyn LegacyProfile] = &[&vietnamese::Vietnamese];

/// Environment variable name to disable legacy font hint detection.
pub const DISABLE_LEGACY_FONT_HINT_ENV: &str = "CADCRAFT_DISABLE_LEGACY_FONT_HINT";

/// Check whether the legacy font hint mechanism is disabled via the
/// `CADCRAFT_DISABLE_LEGACY_FONT_HINT` environment variable.
pub fn is_font_hint_disabled() -> bool {
    is_font_hint_disabled_with(|k| std::env::var(k).ok())
}

pub(crate) fn is_font_hint_disabled_with(mut get_var: impl FnMut(&str) -> Option<String>) -> bool {
    match get_var(DISABLE_LEGACY_FONT_HINT_ENV) {
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "0" | "false" | "off" | "no" => false,
            _ => !v.trim().is_empty(),
        },
        None => false,
    }
}

/// Return the first candidate in `candidates` (such as `&[&ts.font, &style_name]`) that matches
/// a registered [`LegacyProfile::match_font`].
///
/// Returns `None` if the `CADCRAFT_DISABLE_LEGACY_FONT_HINT` environment variable is set.
pub fn font_hint<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    if is_font_hint_disabled() {
        return None;
    }
    font_hint_with(PROFILES, candidates)
}

/// Like [`font_hint`], using an explicit slice of profiles.
pub fn font_hint_with<'a>(profiles: &[&dyn LegacyProfile], candidates: &[&'a str]) -> Option<&'a str> {
    for &c in candidates {
        let trimmed = c.trim();
        if !trimmed.is_empty() && profiles.iter().any(|p| p.match_font(trimmed).is_some()) {
            return Some(c);
        }
    }
    None
}

/// Transcode legacy 8-bit CAD text to Unicode NFC using registered encoding profiles.
///
/// If `hint` matches a registered profile's font naming rules, that profile decodes the text;
/// otherwise each profile's conservative [`LegacyProfile::detect`] heuristic is tried in order.
/// Returns `Cow::Borrowed(s)` with zero allocation when no legacy encoding applies.
///
/// If the `CADCRAFT_DISABLE_LEGACY_FONT_HINT` environment variable is set, `hint` is ignored.
pub fn transcode<'a>(s: &'a str, hint: Option<&str>) -> Cow<'a, str> {
    let effective_hint = if is_font_hint_disabled() { None } else { hint };
    transcode_with(PROFILES, s, effective_hint)
}

/// Like [`transcode`], using an explicit slice of profiles.
pub fn transcode_with<'a>(profiles: &[&dyn LegacyProfile], s: &'a str, hint: Option<&str>) -> Cow<'a, str> {
    if s.is_empty() {
        return Cow::Borrowed(s);
    }
    if let Some(h) = hint.map(str::trim).filter(|h| !h.is_empty()) {
        for p in profiles {
            if let Some(hit) = p.match_font(h) {
                return p.decode(s, hit);
            }
        }
    }
    for p in profiles {
        if let Some(hit) = p.detect(s) {
            return p.decode(s, hit);
        }
    }
    Cow::Borrowed(s)
}

/// If `font` belongs to a registered legacy 8-bit encoding profile, return its modern Unicode
/// substitute typeface class (`Serif` or `Sans`).
///
/// Returns `None` if the `CADCRAFT_DISABLE_LEGACY_FONT_HINT` environment variable is set.
pub fn substitute_face(font: &str) -> Option<FontClass> {
    if is_font_hint_disabled() {
        return None;
    }
    substitute_face_with(PROFILES, font)
}

/// Like [`substitute_face`], using an explicit slice of profiles.
pub fn substitute_face_with(profiles: &[&dyn LegacyProfile], font: &str) -> Option<FontClass> {
    let n = font.trim();
    if n.is_empty() {
        return None;
    }
    for p in profiles {
        if p.match_font(n).is_some() {
            return Some(p.substitute(n));
        }
    }
    None
}

/// Return true if `text` contains legacy-encoded characters that would be
/// transcoded under `hint`.
///
/// UI and engine layers can query this to detect whether a text entity requires
/// transcoding before or after rendering.
pub fn is_legacy_text(text: &str, hint: Option<&str>) -> bool {
    transcode(text, hint) != text
}

/// Return a human-readable summary of the matched legacy encoding family.
pub fn transcode_summary(hint: Option<&str>) -> &'static str {
    match hint {
        Some(h) => {
            let lower = h.to_ascii_lowercase();
            if lower.contains("vni") {
                "VNI Windows"
            } else if lower.contains(".vn") || lower.contains("vnsimli") {
                "TCVN3"
            } else {
                "legacy 8-bit"
            }
        }
        None => "legacy 8-bit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyProfile;

    impl LegacyProfile for DummyProfile {
        fn name(&self) -> &'static str {
            "dummy"
        }

        fn match_font(&self, font: &str) -> Option<ProfileHit> {
            if font.trim().eq_ignore_ascii_case("dummy-serif.ttf") { Some(ProfileHit { profile: "dummy", variant: 1 }) } else { None }
        }

        fn detect(&self, s: &str) -> Option<ProfileHit> {
            if s.contains("\u{00BF}\u{00BF}") { Some(ProfileHit { profile: "dummy", variant: 1 }) } else { None }
        }

        fn decode<'a>(&self, s: &'a str, _hit: ProfileHit) -> Cow<'a, str> {
            if s.contains('\u{00BF}') { Cow::Owned(s.replace('\u{00BF}', "Ω")) } else { Cow::Borrowed(s) }
        }

        fn substitute(&self, _font: &str) -> FontClass {
            FontClass::Serif
        }
    }

    #[test]
    fn european_text_under_standard_and_arial_stays_borrowed() {
        let samples = ["Piñón", "Façade", "Übergang", "Straße", "Piñón, Façade, Übergang, Straße", "Größe, Österreich, Niño, Señora, Garçon"];
        for sample in samples {
            for hint in [None, Some("Standard"), Some("Arial"), Some("arial.ttf")] {
                let res = transcode(sample, hint);
                assert!(matches!(res, Cow::Borrowed(_)), "Expected Cow::Borrowed for {sample:?} with hint {hint:?}, got Owned({res:?})");
                assert_eq!(res, sample);
            }
        }
    }

    #[test]
    fn custom_profile_plugs_into_framework_without_changing_callers() {
        let profiles: &[&dyn LegacyProfile] = &[&vietnamese::Vietnamese, &DummyProfile];

        // Dummy profile matched via font_hint and substitute_face
        let hint = font_hint_with(profiles, &["Standard", "dummy-serif.ttf"]);
        assert_eq!(hint, Some("dummy-serif.ttf"));
        assert_eq!(substitute_face_with(profiles, hint.unwrap_or("")), Some(FontClass::Serif));
        assert_eq!(transcode_with(profiles, "R=10\u{00BF}", hint), "R=10Ω");

        // Dummy profile matched via unhinted detect
        assert_eq!(transcode_with(profiles, "VAL \u{00BF}\u{00BF}", Some("Standard")), "VAL ΩΩ");

        // Vietnamese profile continues to work alongside DummyProfile
        let vn_hint = font_hint_with(profiles, &[".VnTime", "Standard"]);
        assert_eq!(vn_hint, Some(".VnTime"));
        assert_eq!(substitute_face_with(profiles, vn_hint.unwrap_or("")), Some(FontClass::Serif));
        assert_eq!(transcode_with(profiles, "B¶n vÏ", vn_hint), "Bản vẽ");
    }

    #[test]
    fn font_hint_selects_first_matching_legacy_candidate() {
        assert_eq!(font_hint(&["", ".VnTime"]), Some(".VnTime"));
        assert_eq!(font_hint(&["CADCraft Stroke", "VNI-Helve"]), Some("VNI-Helve"));
        assert_eq!(font_hint(&[".VnArialH", "Standard"]), Some(".VnArialH"));
        assert_eq!(font_hint(&["arial.ttf", "Standard"]), None);
        assert_eq!(font_hint(&["", ""]), None);
    }

    #[test]
    fn font_hint_disabled_env_switch_parsing() {
        assert!(is_font_hint_disabled_with(|_| Some("1".into())));
        assert!(is_font_hint_disabled_with(|_| Some("true".into())));
        assert!(is_font_hint_disabled_with(|_| Some("on".into())));
        assert!(is_font_hint_disabled_with(|_| Some("yes".into())));
        assert!(is_font_hint_disabled_with(|_| Some("YES".into())));
        assert!(is_font_hint_disabled_with(|_| Some(" 1 \n".into())));

        assert!(!is_font_hint_disabled_with(|_| Some("0".into())));
        assert!(!is_font_hint_disabled_with(|_| Some("false".into())));
        assert!(!is_font_hint_disabled_with(|_| Some("off".into())));
        assert!(!is_font_hint_disabled_with(|_| Some("no".into())));
        assert!(!is_font_hint_disabled_with(|_| Some("".into())));
        assert!(!is_font_hint_disabled_with(|_| None));
    }

    #[test]
    fn is_legacy_text_and_transcode_summary_surface_status() {
        assert!(is_legacy_text("B¶n vÏ", Some(".VnTime")));
        assert!(!is_legacy_text("Bản vẽ", Some(".VnTime")));
        assert!(!is_legacy_text("Hello", None));

        assert_eq!(transcode_summary(Some(".VnTime")), "TCVN3");
        assert_eq!(transcode_summary(Some("vnsimli.shx")), "TCVN3");
        assert_eq!(transcode_summary(Some("VNI-Helve")), "VNI Windows");
        assert_eq!(transcode_summary(Some("Standard")), "legacy 8-bit");
        assert_eq!(transcode_summary(None), "legacy 8-bit");
    }
}
