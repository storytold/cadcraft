//! Line-break opportunities inside Thai text, for MTEXT word wrap.
//!
//! Thai is written without spaces between words (a space ends a phrase or sentence), so a run of
//! Thai can only wrap where one word ends and the next begins. Those places come from the Unicode
//! line breaking algorithm (UAX #14), which leaves Thai to a word segmenter: `icu_segmenter`'s
//! compiled-in LSTM model for Southeast Asian scripts. Only breaks between two Thai characters
//! are used, so text in other scripts wraps at spaces exactly as before.
//!
//! The model rather than ICU's Thai dictionary: after a word the dictionary lacks (names,
//! loanwords such as เมทัลชีท) the dictionary segmenter loses its place and breaks inside the
//! words that follow, even between a consonant and its tone mark (ทแผ|่|นเร|ียบ), where the model
//! breaks at word boundaries and, when unsure, leaves words together (a line then wraps a little
//! early, never mid-word). The model is also a sixth of the dictionary's size. Any break inside
//! a character cluster is still dropped ([`cluster_boundary`]), so a line never starts with a
//! mark or a following vowel and never ends with a leading vowel.
//!
//! The model takes a few microseconds a character and text is laid out again on every display
//! rebuild, so results are kept in a small cache.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use icu_segmenter::LineSegmenter;
use icu_segmenter::options::LineBreakOptions;

use crate::complex::is_thai;

/// Longest text (characters) looked up; longer text (hostile contents) gets no Thai breaks and
/// wraps at spaces only.
const MAX_CHARS: usize = 4_000;

/// Most texts and bytes of text kept in the cache; it is emptied when either would be passed.
const MAX_CACHED: usize = 8_192;
const MAX_CACHED_BYTES: usize = 4 << 20;

/// Whether a line may break between Thai characters `prev` and `next` without splitting a
/// character cluster: `next` is no combining mark (vowels above and below, tone marks) and no
/// following vowel or sign (ะ า ำ ๅ, ๆ, ฯ), and `prev` is no leading vowel (เ แ โ ใ ไ), which is
/// written before the consonant it belongs to.
fn cluster_boundary(prev: char, next: char) -> bool {
    !crate::bidi::is_mark(next) && !matches!(next, 'ะ' | 'า' | 'ำ' | 'ๅ' | 'ๆ' | 'ฯ') && !matches!(prev, 'เ' | 'แ' | 'โ' | 'ใ' | 'ไ')
}

/// The line segmenter with the Southeast Asian LSTM models, built once.
fn segmenter() -> &'static icu_segmenter::LineSegmenterBorrowed<'static> {
    static S: OnceLock<icu_segmenter::LineSegmenterBorrowed<'static>> = OnceLock::new();
    S.get_or_init(|| LineSegmenter::new_lstm(LineBreakOptions::default()))
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, Arc<[usize]>>,
    bytes: usize,
}

/// A read-write lock: text is laid out on all cores and nearly every lookup is a hit.
fn cache() -> &'static RwLock<Cache> {
    static CACHE: OnceLock<RwLock<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Cache::default()))
}

/// Character offsets in `text`, ascending, where a line may break between two Thai characters
/// (between two Thai words, at a cluster boundary). Empty when `text` has no Thai, is longer than
/// [`MAX_CHARS`], or the segmenter fails.
pub(crate) fn thai_breaks(text: &str) -> Arc<[usize]> {
    if !text.chars().any(is_thai) || text.chars().nth(MAX_CHARS).is_some() {
        return Arc::default();
    }
    if let Some(found) = cache().read().unwrap_or_else(PoisonError::into_inner).entries.get(text) {
        return found.clone();
    }
    let breaks: Arc<[usize]> = segment(text).into();
    let mut c = cache().write().unwrap_or_else(PoisonError::into_inner);
    let bytes = text.len().saturating_add(breaks.len().saturating_mul(std::mem::size_of::<usize>()));
    if c.entries.len() >= MAX_CACHED || c.bytes.saturating_add(bytes) > MAX_CACHED_BYTES {
        c.entries.clear();
        c.bytes = 0;
    }
    if c.entries.insert(text.to_string(), breaks.clone()).is_none() {
        c.bytes = c.bytes.saturating_add(bytes);
    }
    breaks
}

/// [`thai_breaks`] without the cache.
fn segment(text: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    // A last-resort guard around the segmenter: a panic in it must not take the drawing down
    // (the text then wraps at spaces only).
    let bytes: Vec<usize> = catch_unwind(AssertUnwindSafe(|| segmenter().segment_str(text).collect())).unwrap_or_default();
    bytes
        .into_iter()
        .filter_map(|b| chars.binary_search_by_key(&b, |(at, _)| *at).ok())
        .filter(|&i| {
            let prev = i.checked_sub(1).and_then(|p| chars.get(p)).map(|(_, c)| *c);
            let next = chars.get(i).map(|(_, c)| *c);
            matches!((prev, next), (Some(p), Some(n)) if is_thai(p) && is_thai(n) && cluster_boundary(p, n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pieces of `text` between its Thai breaks.
    fn words(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut from = 0;
        for at in thai_breaks(text).iter().copied().chain(std::iter::once(chars.len())) {
            out.push(chars.get(from..at).unwrap_or_default().iter().collect());
            from = at;
        }
        out
    }

    #[test]
    fn thai_breaks_between_words() {
        assert_eq!(words("ภาษาไทย"), ["ภาษา", "ไทย"]);
        assert_eq!(words("เหล็กกล่อง"), ["เหล็ก", "กล่อง"]);
        assert_eq!(words("น้ำทำให้ไม่ใช่"), ["น้ำ", "ทำ", "ให้", "ไม่", "ใช่"]);
        // The sample drawing's specification text, with a loanword (metal sheet) inside: the
        // words after it are still whole.
        let spec = words("เหล็กกล่องขนาดมาตรฐานสำหรับโครงหลังคาเมทัลชีทแผ่นเรียบสีขาว");
        for w in ["เหล็ก", "กล่อง", "ขนาด", "มาตรฐาน", "สำหรับ", "เมทัล", "ชีท", "สี", "ขาว"]
        {
            assert!(spec.iter().any(|p| p == w), "{w}: {spec:?}");
        }
        let wrong = ["ทแผ", "นเร", "ยบ"];
        assert!(spec.iter().all(|p| !wrong.iter().any(|w| p.starts_with(w))), "{spec:?}");
    }

    /// Words no dictionary has (names, loanwords from the sample drawing): no break splits a
    /// character cluster there.
    #[test]
    fn breaks_never_split_a_character_cluster() {
        for text in ["จีรศักดิ์สุพรมวัน", "เมทัลชีทแผ่นเรียบสีขาว", "ศุภประดิษฐ์มาสงค์", "น้ำทำให้ไม่ใช่ๆฯ"]
        {
            let pieces = words(text);
            assert_eq!(pieces.concat(), text);
            for w in &pieces {
                let first = w.chars().next().unwrap_or(' ');
                assert!(!crate::bidi::is_mark(first) && !matches!(first, 'ะ' | 'า' | 'ำ' | 'ๅ' | 'ๆ' | 'ฯ'), "{text}: {pieces:?}");
                assert!(!w.ends_with(['เ', 'แ', 'โ', 'ใ', 'ไ']), "{text}: {pieces:?}");
            }
        }
    }

    #[test]
    fn breaks_inside_a_cluster_are_dropped() {
        assert!(cluster_boundary('ก', 'ข') && cluster_boundary('า', 'เ') && cluster_boundary('\u{0E48}', 'น'));
        for (prev, next) in [
            ('ก', '\u{0E48}'),
            ('ด', '\u{0E34}'),
            ('\u{0E34}', '\u{0E4C}'),
            ('ช', '\u{0E35}'),
            ('ก', 'า'),
            ('น', 'ำ'),
            ('ก', 'ๆ'),
            ('เ', 'ม'),
            ('ไ', 'ท'),
        ] {
            assert!(!cluster_boundary(prev, next), "{prev}|{next}");
        }
    }

    #[test]
    fn other_scripts_and_mixed_text_keep_their_breaks() {
        assert!(thai_breaks("hello world").is_empty());
        assert!(thai_breaks("").is_empty());
        // Between Thai and Latin or digits there is no break added (spaces still break).
        let text = "ขนาด50x50mm";
        assert!(thai_breaks(text).is_empty(), "{:?}", thai_breaks(text));
    }

    #[test]
    fn hostile_text_never_panics_and_stays_bounded() {
        for s in ["\u{0E33}\u{0E33}\u{0E33}", "\u{0E4C}\u{0E49}\u{0E34}", "ก\u{0E49}\u{0E49}\u{0E49}", "ก\u{200B}ข\u{FFFF}\u{10FFFF}ค"] {
            let n = s.chars().count();
            assert!(thai_breaks(s).iter().all(|&i| i > 0 && i < n));
        }
        assert!(thai_breaks(&"ภาษาไทย".repeat(MAX_CHARS)).is_empty(), "too long: no lookup");
        // The limit is on characters, not bytes (a Thai character is three bytes).
        assert!(!thai_breaks(&"ภาษาไทย".repeat(MAX_CHARS / 7)).is_empty());
        let long = "ภาษาไทย".repeat(MAX_CHARS / 8);
        let n = long.chars().count();
        let b = thai_breaks(&long);
        assert!(!b.is_empty() && b.windows(2).all(|w| w[0] < w[1]) && b.iter().all(|&i| i < n));
    }

    #[test]
    fn cached_breaks_match_and_the_cache_stays_bounded() {
        let text = "เหล็กกล่องขนาดมาตรฐาน";
        assert_eq!(thai_breaks(text), segment(text).into());
        assert_eq!(thai_breaks(text), thai_breaks(text));
        // Many different texts: the cache empties itself rather than growing.
        for i in 0..MAX_CACHED + 10 {
            let _ = thai_breaks(&format!("ภาษา{i}ไทย"));
        }
        let c = cache().read().unwrap_or_else(PoisonError::into_inner);
        assert!(c.entries.len() <= MAX_CACHED && c.bytes <= MAX_CACHED_BYTES);
    }
}
