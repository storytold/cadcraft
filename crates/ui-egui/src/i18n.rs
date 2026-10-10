//! Polish user-facing labels; command IDs stay English.
/// Return a localized UI label, falling back to English.
pub fn tr<'a>(language: &str, english: &'a str) -> &'a str {
    if language != "pl" { return english; }
    for line in include_str!("i18n/pl.tsv").lines().skip(1) {
        if let Some((key, value)) = line.split_once('\t') {
            if key == english { return value; }
        }
    }
    english
}
