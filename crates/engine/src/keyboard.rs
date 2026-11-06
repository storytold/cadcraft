//! Words typed on a Greek keyboard layout.
//!
//! With the Greek layout active, the keys that type LINE produce ΛΙΝΕ. Command names, aliases and
//! prompt keywords are Latin, so a Greek speaker would have to switch layouts for every command.
//! [`latin_keys`] reads a word as the Latin keys that produced it (the layout puts each Greek
//! letter on the key of the Latin letter it sounds like): the command line then accepts ΛΙΝΕ and
//! ΣΤΡΕΤΨΗ as LINE and STRETCH. Accents (the layout's dead key) are ignored.

/// The Latin key a Greek letter is typed on (lower case, accents removed).
fn key_of(c: char) -> Option<char> {
    Some(match c {
        'α' | 'ά' => 'a',
        'β' => 'b',
        'γ' => 'g',
        'δ' => 'd',
        'ε' | 'έ' => 'e',
        'ζ' => 'z',
        'η' | 'ή' => 'h',
        'θ' => 'u',
        'ι' | 'ί' | 'ϊ' | 'ΐ' => 'i',
        'κ' => 'k',
        'λ' => 'l',
        'μ' => 'm',
        'ν' => 'n',
        'ξ' => 'j',
        'ο' | 'ό' => 'o',
        'π' => 'p',
        'ρ' => 'r',
        'σ' => 's',
        // The W key types the final sigma.
        'ς' => 'w',
        'τ' => 't',
        'υ' | 'ύ' | 'ϋ' | 'ΰ' => 'y',
        'φ' => 'f',
        'χ' => 'x',
        'ψ' => 'c',
        'ω' | 'ώ' => 'v',
        // The Q key types the semicolon, which Greek text writes as ";" or U+037E.
        ';' | '\u{37e}' => 'q',
        _ => return None,
    })
}

/// Whether `c` is a Greek letter (the basic and accented ranges the layout types).
fn is_greek_letter(c: char) -> bool {
    matches!(c, '\u{386}'..='\u{3ce}') && c != '\u{387}'
}

/// `word` read as the Latin keys of a Greek-layout keyboard, lower case: `Some` only when the word
/// has a Greek letter and every character is a Greek letter the layout types, the semicolon on
/// the Q key, or ASCII (kept as it is). A combining tonos or dialytika, the accent of decomposed
/// text, is ignored like the accent of a precomposed letter, but only straight after the Greek
/// letter it accents and once each: anywhere else the word is not Greek-layout typing.
pub fn latin_keys(word: &str) -> Option<String> {
    let mut greek = false;
    // What the previous Greek letter already carries (tonos, dialytika); `None` when the previous
    // character is not a Greek letter, which leaves a combining mark nothing to accent.
    let mut marks: Option<(bool, bool)> = None;
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        if matches!(c, '\u{301}' | '\u{308}') {
            let (tonos, dialytika) = marks.as_mut()?;
            if std::mem::replace(if c == '\u{301}' { tonos } else { dialytika }, true) {
                return None;
            }
            continue;
        }
        if c.is_ascii() {
            // The Q key types the ASCII semicolon in the Greek layout; every other ASCII character is itself.
            out.push(if c == ';' { 'q' } else { c.to_ascii_lowercase() });
            marks = None;
            continue;
        }
        let lower = c.to_lowercase().next().unwrap_or(c);
        out.push(key_of(lower)?);
        let letter = is_greek_letter(lower);
        greek |= letter;
        marks = letter.then_some((false, false));
    }
    greek.then_some(out)
}

/// `word` with its case, accents and final sigma folded away, when it is Greek-layout typing: every
/// spelling of one Greek word gives the same string.
pub fn greek_fold(word: &str) -> Option<String> {
    latin_keys(&word.to_uppercase())
}

/// Whether `a` and `b` are one Greek word, whatever their case, accents or final sigma.
pub fn same_greek_word(a: &str, b: &str) -> bool {
    greek_fold(a).is_some_and(|fold| greek_fold(b).as_ref() == Some(&fold))
}

#[cfg(test)]
mod tests {
    use super::{latin_keys, same_greek_word};

    #[test]
    fn one_greek_word_is_the_same_in_any_case_accent_or_final_sigma() {
        for (a, b) in [("ΣΟΛΙΔ", "σολιδ"), ("ΣΟΛΙΔ", "σολίδ"), ("ΛΑΣ", "λας"), ("ΛΑΣ", "λασ"), ("ΛΙ\u{301}ΝΕ", "λινε")]
        {
            assert!(same_greek_word(a, b), "{a} {b}");
        }
        for (a, b) in [("ΣΟΛΙΔ", "ΣΟΛΙ"), ("ΣΟΛΙΔ", "solid"), ("solid", "SOLID")] {
            assert!(!same_greek_word(a, b), "{a} {b}");
        }
    }

    #[test]
    fn greek_layout_commands_read_as_their_latin_keys() {
        assert_eq!(latin_keys("ΛΙΝΕ").as_deref(), Some("line"));
        assert_eq!(latin_keys("λινε").as_deref(), Some("line"));
        assert_eq!(latin_keys("ΣΤΡΕΤΨΗ").as_deref(), Some("stretch"));
        assert_eq!(latin_keys("ΜΙΡΡΟΡ").as_deref(), Some("mirror"));
        assert_eq!(latin_keys("ΘΝΔΟ").as_deref(), Some("undo"));
        // The Q key types ";", the W key the final sigma.
        assert_eq!(latin_keys(";ΣΑΩΕ").as_deref(), Some("qsave"));
        assert_eq!(latin_keys("ςΒΛΟΨΚ").as_deref(), Some("wblock"));
        // Accented letters (the layout's dead key) read as the plain letter.
        assert_eq!(latin_keys("ΛΊΝΕ").as_deref(), Some("line"));
        assert_eq!(latin_keys("ΆΡΡΑΥ").as_deref(), Some("array"));
        // The same accents written as a combining mark after the letter (decomposed text).
        assert_eq!(latin_keys("ΛΙ\u{301}ΝΕ").as_deref(), Some("line"));
        assert_eq!(latin_keys("ΛΙΝΕ\u{301}").as_deref(), Some("line"));
        assert_eq!(latin_keys("ΛΙ\u{308}ΝΕ").as_deref(), Some("line"));
        assert_eq!(latin_keys("ΛΙ\u{308}\u{301}ΝΕ").as_deref(), Some("line"), "dialytika and tonos on one letter");
        // Prefixes and digits are kept.
        assert_eq!(latin_keys("-ΛΑΥΕΡ").as_deref(), Some("-layer"));
        assert_eq!(latin_keys("_ΛΙΝΕ").as_deref(), Some("_line"));
    }

    #[test]
    fn words_that_are_not_greek_layout_typing_are_left_alone() {
        assert_eq!(latin_keys("line"), None, "no Greek letter");
        assert_eq!(latin_keys(""), None);
        assert_eq!(latin_keys(";"), None, "a semicolon alone is not a word");
        assert_eq!(latin_keys("ΛΙΝΕ Ж"), None, "a letter the layout does not type");
        assert_eq!(latin_keys("ΛΙΝΕ 日本"), None);
        assert_eq!(latin_keys("\u{387}"), None, "the ano teleia is punctuation, not a letter");
    }

    #[test]
    fn a_combining_mark_must_follow_the_greek_letter_it_accents() {
        assert_eq!(latin_keys("\u{301}ΛΙΝΕ"), None, "no letter before the mark");
        assert_eq!(latin_keys("\u{308}ΛΙΝΕ"), None);
        assert_eq!(latin_keys("ΛΙΝΕ-\u{301}"), None, "after punctuation");
        assert_eq!(latin_keys("a\u{301}ΛΙΝΕ"), None, "after a Latin letter");
        assert_eq!(latin_keys("Λ\u{301}\u{301}ΙΝΕ"), None, "a second tonos on one letter");
        assert_eq!(latin_keys("Λ\u{308}\u{308}ΙΝΕ"), None, "a second dialytika on one letter");
    }

    #[test]
    fn hostile_text_never_panics() {
        let long = "Ω".repeat(100_000);
        assert_eq!(latin_keys(&long).map(|s| s.len()), Some(100_000));
        for s in ["\u{0}", "\u{301}", "ΛΙΝΕ\u{301}", "\u{10ffff}", "ς\u{3c2}\u{3a3}"] {
            let _ = latin_keys(s);
        }
    }
}
