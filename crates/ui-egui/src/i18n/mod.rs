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
    #[serde(rename = "uk")]
    Ukrainian,
}

impl Preference {
    pub const ALL: [Self; 3] = [Self::Auto, Self::English, Self::Ukrainian];

    pub fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::English => "en",
            Self::Ukrainian => "uk",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => t("Auto (system language)"),
            Self::English => "English",
            Self::Ukrainian => "Українська",
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
            Self::Ukrainian => "uk",
            Self::Auto => locales.iter().find_map(|s| locale_language(s)).unwrap_or("en"),
        }
    }
}

pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub source: &'static str,
    catalog: OnceLock<Catalog<'static>>,
}

pub static LANGUAGES: [Language; 2] = [
    Language { code: "en", name: "English", source: "", catalog: OnceLock::new() },
    Language { code: "uk", name: "Українська", source: include_str!("uk.tsv"), catalog: OnceLock::new() },
];

/// BCP 47 and POSIX locale tags, including `uk-UA` and `uk_UA.UTF-8`.
pub fn locale_language(tag: &str) -> Option<&'static str> {
    let primary = tag.trim().split(['-', '_', '.', '@']).next()?;
    LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(primary)).map(|l| l.code)
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

pub fn count(n: u64, one: &str, other: &str) -> String {
    let language = CURRENT.get();
    let source = catalog(language).and_then(|c| c.get(&("@plural", one))).copied();
    let form = source.and_then(|s| s.split('|').nth(plural_uk(n))).unwrap_or(if n == 1 { one } else { other });
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
