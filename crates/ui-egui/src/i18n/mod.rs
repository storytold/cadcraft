//! Display-time localization, following the sibling Craft apps' TSV convention.
//! English strings remain lookup keys; command IDs, drawing data and automation stay unchanged.
//! Add a catalog, a `LANGUAGES` row and a preference variant to introduce another language.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preference {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "el")]
    Greek,
    #[serde(rename = "uk")]
    Ukrainian,
    #[serde(rename = "pt-BR")]
    BrazilianPortuguese,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "cs")]
    Czech,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "zh")]
    Chinese,
    #[serde(rename = "it")]
    Italian,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "sv")]
    Swedish,
    #[serde(rename = "ar")]
    Arabic,
    #[serde(rename = "hu")]
    Hungarian,
    #[serde(rename = "tr")]
    Turkish,
}

impl Preference {
    pub const ALL: [Self; 17] = [
        Self::Auto,
        Self::English,
        Self::Greek,
        Self::Ukrainian,
        Self::BrazilianPortuguese,
        Self::German,
        Self::Spanish,
        Self::Czech,
        Self::Russian,
        Self::French,
        Self::Chinese,
        Self::Italian,
        Self::Korean,
        Self::Swedish,
        Self::Arabic,
        Self::Hungarian,
        Self::Turkish,
    ];

    /// Every preference in the order the Interface Language dialog lists them: Auto, then English
    /// (the source language), then the rest by the language's own name with accents folded, so
    /// Čeština sits under C and Magyar under M. Names in other scripts follow the Latin ones in
    /// code point order: Greek, Cyrillic, Arabic, then the CJK languages. The order follows the
    /// registry's native name even where the dialog shows an English label instead (Korean
    /// without a Hangul face), so the list does not reorder when a font appears.
    pub fn for_picker() -> Vec<Self> {
        let mut rest: Vec<Self> = Self::ALL.into_iter().filter(|p| !matches!(p, Self::Auto | Self::English)).collect();
        rest.sort_by_cached_key(|p| name_sort_key(p.native_name()));
        [Self::Auto, Self::English].into_iter().chain(rest).collect()
    }

    /// The language's own name from the registry, whatever the dialog currently shows for it.
    fn native_name(self) -> &'static str {
        LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(self.code())).map_or_else(|| self.name(), |l| l.name)
    }

    /// The codes a preference can be given as, for messages ("auto, en or uk").
    pub fn codes() -> String {
        let codes: Vec<&str> = Self::ALL.iter().map(|p| p.code()).collect();
        match codes.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
            _ => codes.join(""),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => "en",
            Self::Greek => "el",
            Self::Ukrainian => "uk",
            Self::BrazilianPortuguese => "pt-BR",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Czech => "cs",
            Self::Russian => "ru",
            Self::French => "fr",
            Self::Chinese => "zh",
            Self::Italian => "it",
            Self::Korean => "ko",
            Self::Swedish => "sv",
            Self::Arabic => "ar",
            Self::Hungarian => "hu",
            Self::Turkish => "tr",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => t("Auto (system language)"),
            Self::English => "English",
            Self::Greek => "Ελληνικά",
            Self::Ukrainian => "Українська",
            Self::BrazilianPortuguese => "Português (Brasil)",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Czech => "Čeština",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::Chinese => "简体中文",
            Self::Italian => "Italiano",
            Self::Korean => {
                if crate::theme::shows_hangul(CURRENT.with(Cell::get)) {
                    "한국어"
                } else {
                    "Korean"
                }
            }
            Self::Swedish => "Svenska",
            Self::Arabic => "العربية",
            Self::Hungarian => "Magyar",
            Self::Turkish => "Türkçe",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.code() == code)
    }

    /// An invalid or obsolete saved choice follows the system language.
    pub fn restore(saved: Option<&str>) -> Self {
        saved.and_then(Self::parse).unwrap_or_default()
    }

    pub fn resolve(self, locales: &[String]) -> &'static str {
        match self {
            Self::English => "en",
            Self::Greek => "el",
            Self::Ukrainian => "uk",
            Self::BrazilianPortuguese => "pt-BR",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Czech => "cs",
            Self::Russian => "ru",
            Self::French => "fr",
            Self::Chinese => "zh",
            Self::Italian => "it",
            Self::Korean => "ko",
            Self::Swedish => "sv",
            Self::Arabic => "ar",
            Self::Hungarian => "hu",
            Self::Turkish => "tr",
            Self::Auto => locales.iter().find_map(|s| locale_language(s)).unwrap_or("en"),
        }
    }
}

pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub source: &'static str,
    /// Index of the `@plural` form for an integer count.
    pub plural: fn(u64) -> usize,
    /// Number of `|`-separated forms every `@plural` row must carry.
    pub plural_forms: usize,
    catalog: OnceLock<Catalog<'static>>,
}

pub static LANGUAGES: [Language; 16] = [
    Language { code: "en", name: "English", source: "", plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    // Greek: one for exactly 1; other for zero and all other integer counts.
    Language { code: "el", name: "Ελληνικά", source: include_str!("el.tsv"), plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    Language {
        code: "uk", name: "Українська", source: include_str!("uk.tsv"), plural: plural_uk, plural_forms: 3, catalog: OnceLock::new()
    },
    Language {
        code: "pt-BR",
        name: "Português (Brasil)",
        source: include_str!("pt_br.tsv"),
        plural: plural_pt,
        plural_forms: 2,
        catalog: OnceLock::new(),
    },
    // German plurals follow the English rule: one (1), other.
    Language { code: "de", name: "Deutsch", source: include_str!("de.tsv"), plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    // Spanish integer plurals follow the English rule too.
    Language { code: "es", name: "Español", source: include_str!("es.tsv"), plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    Language { code: "cs", name: "Čeština", source: include_str!("cs.tsv"), plural: plural_cs, plural_forms: 3, catalog: OnceLock::new() },
    // Russian integer plurals follow the Ukrainian rule: one (1, 21), few (2–4), many.
    Language { code: "ru", name: "Русский", source: include_str!("ru.tsv"), plural: plural_uk, plural_forms: 3, catalog: OnceLock::new() },
    // French integer plurals (CLDR `fr`): one (0, 1), other, the Portuguese rule.
    Language { code: "fr", name: "Français", source: include_str!("fr.tsv"), plural: plural_pt, plural_forms: 2, catalog: OnceLock::new() },
    // Chinese has no grammatical plural: both `@plural` forms read the same. Its glyphs come from
    // an installed CJK font (`theme::cjk_font`); without one the interface stays English.
    Language { code: "zh", name: "简体中文", source: include_str!("zh.tsv"), plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    // Italian integer plurals (CLDR `it`) follow the English rule: one (1), other.
    Language { code: "it", name: "Italiano", source: include_str!("it.tsv"), plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    // Korean has one integer plural form and needs a Hangul-capable interface font.
    Language { code: "ko", name: "한국어", source: include_str!("ko.tsv"), plural: plural_ko, plural_forms: 1, catalog: OnceLock::new() },
    // Swedish plurals follow the English rule: one (1), other.
    Language { code: "sv", name: "Svenska", source: include_str!("sv.tsv"), plural: plural_en, plural_forms: 2, catalog: OnceLock::new() },
    // العربية (Arabic) is right-to-left; an installed Arabic font (`theme::arabic_font`)
    // covers its glyphs, and the shell keeps egui's left-to-right run order. Six plural
    // forms: zero, one, two, few, many, other.
    Language { code: "ar", name: "العربية", source: include_str!("ar.tsv"), plural: plural_arabic, plural_forms: 6, catalog: OnceLock::new() },
    // Hungarian uses the singular after a numeral, so one form covers every count (the Korean rule).
    Language { code: "hu", name: "Magyar", source: include_str!("hu.tsv"), plural: plural_ko, plural_forms: 1, catalog: OnceLock::new() },
    // Turkish keeps a noun singular after a number ("3 commit"), so like Korean it has one form.
    Language { code: "tr", name: "Türkçe", source: include_str!("tr.tsv"), plural: plural_ko, plural_forms: 1, catalog: OnceLock::new() },
];

/// BCP 47 and POSIX locale tags, including `uk-UA`, `uk_UA.UTF-8` and `pt_BR.UTF-8`.
/// A catalog is chosen by the tag's primary language, so any `pt-*` tag selects `pt-BR`.
pub fn locale_language(tag: &str) -> Option<&'static str> {
    let primary = tag.trim().split(['-', '_', '.', '@']).next()?;
    LANGUAGES.iter().find(|l| l.code.split('-').next().is_some_and(|p| p.eq_ignore_ascii_case(primary))).map(|l| l.code)
}

thread_local! {
    // Scoped to a single app's render. Headless calls and parallel tests default to English.
    static CURRENT: Cell<&'static str> = const { Cell::new("en") };
}

pub fn with_language<R>(language: &'static str, draw: impl FnOnce() -> R) -> R {
    struct Restore(&'static str);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.set(self.0);
        }
    }
    let _restore = Restore(CURRENT.replace(language));
    draw()
}

type Catalog<'a> = HashMap<(&'a str, &'a str), &'a str>;

/// Malformed rows are ignored at runtime and diagnosed by the catalog tests.
fn parse(source: &str) -> (Catalog<'_>, Vec<String>) {
    let mut entries = HashMap::new();
    let mut errors = Vec::new();
    for (i, line) in source.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let columns: Vec<_> = line.split('\t').collect();
        match columns.as_slice() {
            [context, key, value] if !key.is_empty() && !value.is_empty() => {
                if entries.insert((*context, *key), *value).is_some() {
                    errors.push(format!("line {}: duplicate key {key}", i + 1));
                }
            }
            _ => errors.push(format!("line {}: expected context, source, translation", i + 1)),
        }
    }
    (entries, errors)
}

fn catalog(language: &str) -> Option<&'static Catalog<'static>> {
    LANGUAGES.iter().find(|l| l.code == language && !l.source.is_empty()).map(|l| l.catalog.get_or_init(|| parse(l.source).0))
}

pub fn tr<'a>(language: &str, source: &'a str) -> &'a str {
    catalog(language).and_then(|c| c.get(&("", source))).copied().unwrap_or(source)
}

pub fn t(source: &str) -> &str {
    CURRENT.with(|l| tr(l.get(), source))
}

/// Menu headings can differ from command or object labels (e.g. Czech Kótování / Kóta).
/// Catalogs without a `menu` entry retain their ordinary translation.
pub fn tr_menu<'a>(language: &str, source: &'a str) -> &'a str {
    catalog(language).and_then(|c| c.get(&("menu", source))).copied().unwrap_or_else(|| tr(language, source))
}

pub fn menu(source: &str) -> &str {
    CURRENT.with(|l| tr_menu(l.get(), source))
}

/// Named placeholders allow translations to reorder values. Inserted values are never looked up
/// or substituted again, so a user's name containing `{n}` is preserved verbatim.
pub fn format(source: &str, values: &[(&str, String)]) -> String {
    let text = t(source);
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, tail)) = rest.split_once('{') {
        result.push_str(before);
        let Some((key, after)) = tail.split_once('}') else {
            result.push('{');
            rest = tail;
            break;
        };
        if let Some((_, value)) = values.iter().find(|(name, _)| *name == key) {
            result.push_str(value);
        } else {
            result.push('{');
            result.push_str(key);
            result.push('}');
        }
        rest = after;
    }
    result.push_str(rest);
    result
}

/// Ukrainian integer plurals: one (1, 21), few (2–4, 22–24), many (0, 5–20, 25…).
pub fn plural_uk(n: u64) -> usize {
    match (n % 10, n % 100) {
        (1, 11) | (2..=4, 12..=14) => 2,
        (1, _) => 0,
        (2..=4, _) => 1,
        _ => 2,
    }
}

/// Portuguese integer plurals (CLDR `pt`): one (0, 1), other (2 and up).
pub fn plural_pt(n: u64) -> usize {
    usize::from(n > 1)
}

/// Korean uses the same count form for every integer.
fn plural_ko(_n: u64) -> usize {
    0
}

/// Arabic integer plurals (CLDR `ar`): zero (0), one (1), two (2), few (3–10, 103–110),
/// many (11–99) — the same rule pdfcraft uses, so the sibling apps agree.
pub fn plural_arabic(n: u64) -> usize {
    match (n, n % 100) {
        (0, _) => 0,
        (1, _) => 1,
        (2, _) => 2,
        (_, 3..=10) => 3,
        (_, 11..=99) => 4,
        _ => 5,
    }
}

fn plural_en(n: u64) -> usize {
    usize::from(n != 1)
}

/// Czech integer plurals: one (1), few (2–4), other (0 and 5 or more).
pub fn plural_cs(n: u64) -> usize {
    match n {
        1 => 0,
        2..=4 => 1,
        _ => 2,
    }
}

pub fn count(n: u64, one: &str, other: &str) -> String {
    let language = CURRENT.get();
    let source = catalog(language).and_then(|c| c.get(&("@plural", one))).copied();
    let plural = LANGUAGES.iter().find(|l| l.code == language).map_or(plural_en as fn(u64) -> usize, |l| l.plural);
    let form = source.and_then(|s| s.split('|').nth(plural(n))).unwrap_or(if n == 1 { one } else { other });
    format(form, &[("n", n.to_string())])
}

#[macro_export]
macro_rules! tl {
    ($source:literal) => {
        $crate::i18n::t($source)
    };
}

#[macro_export]
macro_rules! tf {
    ($source:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::i18n::format($source, &[$((stringify!($name), ($value).to_string())),*])
    };
}

#[cfg(test)]
mod tests;

/// `name` lowercased with the Latin accents that occur in language names folded away, so a
/// byte-wise sort reads alphabetically. Other scripts are left as they are.
fn name_sort_key(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ē' | 'ě' | 'ę' | 'ế' | 'ệ' => 'e',
            'ì'..='ï' | 'ī' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' | 'ő' => 'o',
            'ś' | 'š' => 's',
            'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod picker_order_tests {
    use super::*;

    #[test]
    fn picker_lists_auto_and_english_first_then_names_alphabetically() {
        let order = Preference::for_picker();
        assert_eq!(&order[..2], &[Preference::Auto, Preference::English]);
        assert_eq!(order.len(), Preference::ALL.len());
        let keys: Vec<String> = order[2..].iter().map(|p| name_sort_key(p.native_name())).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]), "{keys:?}");
        let pos = |p: Preference| order.iter().position(|q| *q == p).unwrap_or(usize::MAX);
        assert!(pos(Preference::Czech) < pos(Preference::German), "Čeština sorts under C");
        assert!(pos(Preference::Italian) < pos(Preference::Hungarian), "Magyar sorts under M");
        assert!(pos(Preference::Turkish) < pos(Preference::Greek), "Latin names come before Greek");
        assert_eq!(name_sort_key("Türkçe"), "turkce");
    }
}
