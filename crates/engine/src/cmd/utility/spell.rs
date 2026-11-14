//! SPELL: check the words of texts, multiline texts, attributes and multileader texts against
//! a word list: the user dictionary file (the SPELLDICTIONARY user setting, else `user.dic` in
//! the settings folder; one word per line), a file named in the JSON form or words passed with
//! it. CADCRAFT bundles no dictionary. The path is never taken from the drawing: a drawing's
//! `$DCTCUST` is data from the file and is ignored.

use std::collections::{BTreeSet, HashSet};

use cadcraft_doc::fields;
use cadcraft_doc::{EntityKind, Handle};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase};
use super::super::{CommandSpec, bad, str_param, targets};
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("spell", "Spelling", run_spell)
            .menu(&["Tools", "Spelling"])
            .alias(&["sp"])
            .params(
                "{handles?, words?: [..], dictionary?: path (default: the SPELLDICTIONARY setting), replace?: {word: replacement}} \
                 (no handles: every object in the current space)",
            )
            .interactive(|_| Ok(Box::new(SpellM::default()))),
    ]
}

/// Largest word list file read.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILE: u64 = 16 * 1024 * 1024;
/// Most words kept from a word list.
const MAX_WORDS: usize = 2_000_000;
/// Most unknown words reported (with suggestions) by the JSON form.
const MAX_REPORTED: usize = 500;

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// Words of a word list: one per line, `#` comments, compared without case.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn parse_words(text: &str) -> HashSet<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).take(MAX_WORDS).map(str::to_lowercase).collect()
}

fn read_list(path: &str) -> Result<HashSet<String>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let meta = std::fs::metadata(path).map_err(|e| bad("spell", format!("{path}: {e}")))?;
        if meta.len() > MAX_FILE {
            return Err(bad("spell", format!("{path}: the word list is too large")));
        }
        let bytes = std::fs::read(path).map_err(|e| bad("spell", format!("{path}: {e}")))?;
        Ok(parse_words(&String::from_utf8_lossy(&bytes)))
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("spell", format!("{path}: files can't be read here: pass `words`")))
}

/// Add a word to the user dictionary file.
fn append_word(path: &str, word: &str) -> Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        use std::io::{Read, Seek, SeekFrom};
        let io = |e: std::io::Error| other(format!("{path}: {e}"));
        // A word is one line: never let one carry a line break.
        let word: String = word.chars().filter(|c| !c.is_control()).collect();
        // A hand-edited file may lack its last newline: start the word on its own line. A file
        // that can't be read for this check is appended to as before.
        let lead = match std::fs::File::open(path) {
            Ok(mut r) => {
                let len = r.metadata().map(|m| m.len()).unwrap_or(0);
                let mut last = [0u8];
                let ok = len > 0 && r.seek(SeekFrom::Start(len - 1)).is_ok() && r.read_exact(&mut last).is_ok();
                if ok && last[0] != b'\n' { "\n" } else { "" }
            }
            Err(_) => "",
        };
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(io)?;
        writeln!(f, "{lead}{word}").map_err(io)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, word);
        Ok(())
    }
}

/// The user dictionary path: the SPELLDICTIONARY profile setting, else `user.dic` in the
/// settings folder. Never read from the drawing (a downloaded drawing must not choose which
/// file SPELL reads or appends to).
fn user_dictionary(s: &Session) -> Option<String> {
    let set = s.settings.spelldictionary.trim();
    if !set.is_empty() {
        return Some(set.to_string());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::settings_dir::current().map(|r| r.join("user.dic").to_string_lossy().into_owned())
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// The words of a text, in order: runs of letters (apostrophes inside), two letters or more.
pub(crate) fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut flush = |cur: &mut String| {
        let w = cur.trim_matches('\'');
        if w.chars().count() >= 2 {
            out.push(w.to_string());
        }
        cur.clear();
    };
    for c in text.chars() {
        if c.is_alphabetic() || (c == '\'' && !cur.is_empty()) {
            cur.push(c);
        } else {
            flush(&mut cur);
        }
    }
    flush(&mut cur);
    out
}

/// The texts of an object as shown (fields as their values, MTEXT codes stripped).
fn shown_texts(k: &EntityKind) -> Vec<String> {
    match k {
        EntityKind::Text(t) => vec![fields::display(&t.value).into_owned()],
        EntityKind::MText(t) => vec![cadcraft_fonts::plain_mtext(&fields::display_mtext(&t.contents))],
        EntityKind::Insert(i) => i.attribs.iter().map(|a| fields::display(&a.text.value).into_owned()).collect(),
        EntityKind::MLeader(m) => m.text.iter().map(|t| cadcraft_fonts::plain_mtext(&fields::display_mtext(&t.contents))).collect(),
        _ => Vec::new(),
    }
}

/// Byte ranges of the MTEXT formatting commands whose payload is not shown text: the codes with
/// an argument ended by `;` (`\f`, `\H`, `\C`, ...) and `\U+XXXX`. The one-letter codes carry no
/// payload and `\S` stacks hold shown text, so those stay open to replacement.
fn mtext_payloads(t: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut it = t.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c != '\\' {
            continue;
        }
        let Some((_, code)) = it.next() else { break };
        let rest = t.get(i + 2..).unwrap_or("");
        let end = match code {
            'f' | 'F' | 'H' | 'W' | 'Q' | 'T' | 'A' | 'C' | 'c' | 'p' => rest.find(';').map(|e| i + 2 + e + 1),
            'U' if rest.starts_with('+') => Some((i + 2 + rest.char_indices().nth(5).map_or(rest.len(), |(k, _)| k)).min(t.len())),
            _ => None,
        };
        if let Some(end) = end {
            out.push(i..end);
            while it.peek().is_some_and(|(k, _)| *k < end) {
                it.next();
            }
        }
    }
    out
}

/// Replace a whole word in an object's texts; returns the count. Field codes and (for MTEXT)
/// formatting payloads such as a `\f` font name stay untouched.
fn replace_word(k: &mut EntityKind, word: &str, rep: &str) -> usize {
    let mut n = 0;
    let mut apply = |t: &mut String, mtext: bool| {
        let before = n;
        let mut skip = fields::find(t).into_iter().map(|sp| sp.range).collect::<Vec<_>>();
        if mtext {
            skip.extend(mtext_payloads(t));
        }
        skip.sort_by_key(|r| r.start);
        let mut out = String::new();
        let mut last = 0;
        for r in &skip {
            if r.start < last {
                continue;
            }
            let (seg, c) = super::replace_in(t.get(last..r.start).unwrap_or(""), word, rep, true, true);
            n += c;
            out.push_str(&seg);
            out.push_str(t.get(r.clone()).unwrap_or(""));
            last = r.end;
        }
        let (seg, c) = super::replace_in(t.get(last..).unwrap_or(""), word, rep, true, true);
        n += c;
        out.push_str(&seg);
        if n > before {
            *t = out;
        }
    };
    match k {
        EntityKind::Text(t) => apply(&mut t.value, false),
        EntityKind::MText(t) => apply(&mut t.contents, true),
        EntityKind::Insert(i) => i.attribs.iter_mut().for_each(|a| apply(&mut a.text.value, false)),
        EntityKind::MLeader(m) => {
            if let Some(t) = &mut m.text {
                apply(&mut t.contents, true)
            }
        }
        _ => {}
    }
    n
}

/// Restricted Damerau–Levenshtein distance, giving up past `max`. `rows` is scratch space that
/// callers reuse across calls, so a scan over a whole dictionary does not allocate per word.
fn distance(a: &[char], b: &[char], max: usize, rows: &mut [Vec<usize>; 3]) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let w = b.len() + 1;
    let [prev2, prev, cur] = rows;
    prev2.clear();
    prev2.resize(w, 0);
    prev.clear();
    prev.extend(0..w);
    cur.clear();
    cur.resize(w, 0);
    for i in 1..=a.len() {
        if let Some(c0) = cur.first_mut() {
            *c0 = i;
        }
        let mut best = i;
        for j in 1..=b.len() {
            let (ai, bj) = (a.get(i - 1), b.get(j - 1));
            let cost = usize::from(ai != bj);
            let mut v = (prev.get(j).copied().unwrap_or(usize::MAX) + 1)
                .min(cur.get(j - 1).copied().unwrap_or(usize::MAX) + 1)
                .min(prev.get(j - 1).copied().unwrap_or(usize::MAX) + cost);
            if i > 1 && j > 1 && ai == b.get(j - 2) && a.get(i - 2) == bj {
                v = v.min(prev2.get(j - 2).copied().unwrap_or(usize::MAX) + 1);
            }
            if let Some(c) = cur.get_mut(j) {
                *c = v;
            }
            best = best.min(v);
        }
        if best > max {
            return None;
        }
        std::mem::swap(prev2, prev);
        std::mem::swap(prev, cur);
    }
    prev.get(b.len()).copied().filter(|d| *d <= max)
}

/// Up to five dictionary words close to `word`, nearest first. One scan of the dictionary:
/// callers compute this once per word, never per frame (see `SpellM::sug`).
pub(crate) fn suggestions(dict: &HashSet<String>, word: &str) -> Vec<String> {
    let w: Vec<char> = word.to_lowercase().chars().collect();
    let max = if w.len() <= 4 { 1 } else { 2 };
    let mut rows: [Vec<usize>; 3] = Default::default();
    let mut buf: Vec<char> = Vec::new();
    let mut found: Vec<(usize, &String)> = Vec::new();
    for d in dict {
        // Cheap length test first: no allocation for words that cannot be close.
        if d.chars().count().abs_diff(w.len()) > max {
            continue;
        }
        buf.clear();
        buf.extend(d.chars());
        if let Some(n) = distance(&w, &buf, max, &mut rows) {
            found.push((n, d));
        }
    }
    found.sort();
    found.into_iter().take(5).map(|(_, d)| d.clone()).collect()
}

fn known(dict: &HashSet<String>, word: &str) -> bool {
    dict.contains(&word.to_lowercase())
}

/// The objects to check: `handles`, or every object in the current space.
fn objects(s: &Session, p: &Value) -> Result<Vec<Handle>> {
    match p.get("handles").or_else(|| p.get("handle")) {
        Some(v) if !v.is_null() => targets(s, p),
        _ => {
            let space = s.space();
            Ok(s.doc()?.space(&space).map(|st| st.handles()).unwrap_or_default())
        }
    }
}

fn run_spell(s: &mut Session, p: &Value) -> Result<Value> {
    let mut dict: HashSet<String> = HashSet::new();
    if let Some(ws) = p.get("words") {
        let a = ws.as_array().ok_or_else(|| bad("spell", "`words` must be an array of words"))?;
        dict.extend(a.iter().filter_map(Value::as_str).take(MAX_WORDS).map(str::to_lowercase));
    }
    let path = str_param(p, "dictionary").map(str::to_string).or_else(|| if p.get("words").is_some() { None } else { user_dictionary(s) });
    if let Some(path) = &path {
        // The default list may not exist yet; an explicitly named one must.
        if str_param(p, "dictionary").is_some() || std::path::Path::new(path).exists() {
            dict.extend(read_list(path)?);
        }
    }
    if dict.is_empty() {
        return Err(bad(
            "spell",
            "no word list: set the SPELLDICTIONARY option to a word list file (one word per line), or pass `words` or `dictionary`",
        ));
    }
    let hs = objects(s, p)?;
    let replace: Vec<(String, String)> = match p.get("replace") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(m)) => m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect(),
        Some(_) => return Err(bad("spell", "`replace` must map words to replacements")),
    };
    let mut unknown = Vec::new();
    let mut checked = 0;
    let mut edits = Vec::new();
    let mut replaced = 0;
    {
        let d = s.doc()?;
        for h in hs {
            let Some(e) = d.entity(h) else { continue };
            let texts = shown_texts(&e.kind);
            if texts.is_empty() {
                continue;
            }
            checked += 1;
            let mut seen = BTreeSet::new();
            for w in texts.iter().flat_map(|t| words(t)) {
                if !known(&dict, &w) && seen.insert(w.clone()) && unknown.len() < MAX_REPORTED {
                    unknown.push(json!({ "handle": h.hex(), "word": w, "suggestions": suggestions(&dict, &w) }));
                }
            }
            if !replace.is_empty() {
                let mut k = e.kind.clone();
                let n: usize = replace.iter().map(|(w, r)| replace_word(&mut k, w, r)).sum();
                if n > 0 {
                    replaced += n;
                    edits.push((h, k));
                }
            }
        }
    }
    // A check without edits must leave the drawing alone: `doc_mut` would copy it and count as a change.
    if !edits.is_empty() {
        let d = s.doc_mut()?;
        for (h, k) in edits {
            d.modify_entity(h, |e| e.kind = k)?;
        }
    }
    let message = if unknown.is_empty() { "Spelling check complete.".to_string() } else { format!("{} unknown word(s).", unknown.len()) };
    Ok(json!({ "checked": checked, "unknown": unknown, "replaced": replaced, "message": message }))
}

/// SPELL at the command line: select objects, then each unknown word in turn: type a
/// replacement, or Ignore it, ignore All of it, or Add it to the user dictionary.
#[derive(Default)]
struct SpellM {
    sel: SelectPhase,
    dict: HashSet<String>,
    /// Words to ask about, with the object they are in.
    queue: Vec<(Handle, String)>,
    ignored: HashSet<String>,
    changed: usize,
    /// Suggestions for the word being asked about, computed once when it becomes the current
    /// one: `prompt` runs every frame and must stay a cheap read.
    sug: Vec<String>,
}

impl SpellM {
    fn start(&mut self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        let path =
            user_dictionary(s).ok_or_else(|| other("No word list: set the SPELLDICTIONARY option to a word list file (one word per line)."))?;
        // Only an implicit default list may be absent; a configured one must be readable.
        let implicit = s.settings.spelldictionary.trim().is_empty();
        self.dict =
            if implicit && !std::path::Path::new(&path).exists() { HashSet::new() } else { read_list(&path).map_err(|e| other(e.to_string()))? };
        let d = s.doc()?;
        for h in hs {
            let Some(e) = d.entity(*h) else { continue };
            let mut seen = BTreeSet::new();
            for w in shown_texts(&e.kind).iter().flat_map(|t| words(t)) {
                if !known(&self.dict, &w) && seen.insert(w.clone()) && self.queue.len() < 10_000 {
                    self.queue.push((*h, w));
                }
            }
        }
        self.queue.reverse();
        self.next(s)
    }

    /// Drop words no longer to ask about; done when none is left.
    fn next(&mut self, s: &mut Session) -> Result<Step> {
        while let Some((_, w)) = self.queue.last() {
            if self.ignored.contains(&w.to_lowercase()) || known(&self.dict, w) {
                self.queue.pop();
            } else {
                self.sug = suggestions(&self.dict, w);
                return Ok(Step::Continue);
            }
        }
        self.sug.clear();
        s.echo(format!("Spelling check complete. {} word(s) changed.", self.changed));
        Ok(Step::Done)
    }
}

impl Interactive for SpellM {
    fn name(&self) -> &'static str {
        "SPELL"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.start(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.queue.last() {
            None => self.sel.prompt(),
            Some((_, w)) => {
                let hint = if self.sug.is_empty() { String::new() } else { format!(" (suggestions: {})", self.sug.join(", ")) };
                Prompt::new(format!("Unknown word \"{w}\"{hint}. Enter replacement"), Accept::TEXT).kw(&["Ignore", "All", "Add"]).default("Ignore")
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some((h, w)) = self.queue.last().cloned() else {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.start(s, &hs),
            };
        };
        let answer = match &i {
            Input::Keyword(k) | Input::Text(k) => k.trim().to_string(),
            Input::Enter => "Ignore".into(),
            _ => return Ok(Step::Continue),
        };
        match answer.to_ascii_lowercase().as_str() {
            "" | "ignore" | "i" => {}
            "all" => {
                self.ignored.insert(w.to_lowercase());
            }
            "add" => {
                if let Some(path) = user_dictionary(s) {
                    append_word(&path, &w)?;
                }
                self.dict.insert(w.to_lowercase());
            }
            _ => {
                let d = s.doc()?;
                if let Some(e) = d.entity(h) {
                    let mut k = e.kind.clone();
                    let n = replace_word(&mut k, &w, &answer);
                    if n > 0 {
                        self.changed += n;
                        s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
                    }
                }
            }
        }
        // Only now: a failed `Add` write leaves the word queued.
        self.queue.pop();
        self.next(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn append_word_starts_on_a_new_line() {
        let dir = std::env::temp_dir().join(format!("cadcraft-spell-nonl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dic = dir.join("user.dic");
        let path = dic.to_str().unwrap();
        std::fs::write(&dic, "foo").unwrap();
        append_word(path, "bar").unwrap();
        append_word(path, "baz").unwrap();
        assert_eq!(std::fs::read_to_string(&dic).unwrap(), "foo\nbar\nbaz\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn words_and_suggestions() {
        assert_eq!(words("Don't BREAK 6061-T6 edges, x"), vec!["Don't", "BREAK", "edges"]);
        let dict = parse_words("# list\nthe\nten\nedges\nanodize\n");
        assert_eq!(suggestions(&dict, "teh"), vec!["ten", "the"]);
        assert_eq!(suggestions(&dict, "anodise"), vec!["anodize"]);
        assert!(suggestions(&dict, "zzzzzz").is_empty());
        assert_eq!(distance(&['a', 'b'], &['b', 'a'], 2, &mut Default::default()), Some(1));
    }

    /// The prompt (asked for every frame) reads the suggestions cached when the word became
    /// current: it does not touch the dictionary, however large.
    #[test]
    fn prompt_does_not_rescan_the_dictionary() {
        let mut s = Session::new();
        let mut m = SpellM { dict: (0..300_000).map(|i| format!("word{i:06}")).collect(), ..SpellM::default() };
        m.dict.insert("anodize".into());
        m.queue.push((Handle(1), "anodise".into()));
        assert!(matches!(m.next(&mut s), Ok(Step::Continue)));
        // Gone dictionary: a prompt that scanned it would lose the suggestion.
        m.dict = HashSet::new();
        for _ in 0..1000 {
            assert!(m.prompt(&s).message.contains("suggestions: anodize"));
        }
    }
}
