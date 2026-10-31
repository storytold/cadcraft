//! AutoCAD fields in text: a TEXT or MTEXT whose extension dictionary holds `ACAD_FIELD` →
//! `TEXT` → a FIELD object. That field's code is the text's template (`Date: %<\_FldIdx 0>%`)
//! and its child fields fill the `\_FldIdx` places (`\AcVar date \f "dd.MM.yyyy"`). The
//! resolved template becomes the text's formula, which the engine works out when the drawing is
//! opened (dates follow the clock); the field objects themselves are kept and written back as
//! they were read.

use std::collections::HashMap;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle};
use cadcraft_dxf::{Section, Tag, records};

/// Deepest nesting of fields followed.
const MAX_DEPTH: usize = 8;
/// Longest template kept.
const MAX_LEN: usize = 16_384;

fn handle_of(tags: &[Tag]) -> Option<String> {
    tags.iter().find(|t| t.code == 5).map(|t| t.str().trim().to_ascii_uppercase())
}

/// The extension dictionary of a record (`102 {ACAD_XDICTIONARY` `360 handle`).
fn xdict(tags: &[Tag]) -> Option<String> {
    let at = tags.iter().position(|t| t.code == 102 && t.str().trim().eq_ignore_ascii_case("{ACAD_XDICTIONARY"))?;
    tags.get(at + 1).filter(|t| t.code == 360).map(|t| t.str().trim().to_ascii_uppercase())
}

/// A dictionary's entry (`3 name` then `350`/`360 handle`).
fn entry(tags: &[Tag], name: &str) -> Option<String> {
    let at = tags.iter().position(|t| t.code == 3 && t.str().trim().eq_ignore_ascii_case(name))?;
    tags.get(at + 1).filter(|t| t.code == 350 || t.code == 360).map(|t| t.str().trim().to_ascii_uppercase())
}

/// A field's code (2, continued in 3s) with its `\_FldIdx n` places filled from its children.
fn code(objects: &HashMap<String, Vec<Tag>>, h: &str, depth: usize) -> Option<String> {
    code_in(objects, h, depth, &mut HashMap::new())
}

/// [`code`], with the codes already worked out (by handle and depth) in `seen`: a file whose
/// fields share children is expanded once per child, not once per reference.
fn code_in(objects: &HashMap<String, Vec<Tag>>, h: &str, depth: usize, seen: &mut HashMap<(String, usize), Option<String>>) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(done) = seen.get(&(h.to_string(), depth)) {
        return done.clone();
    }
    let out = expand(objects, h, depth, seen);
    seen.insert((h.to_string(), depth), out.clone());
    out
}

fn expand(objects: &HashMap<String, Vec<Tag>>, h: &str, depth: usize, seen: &mut HashMap<(String, usize), Option<String>>) -> Option<String> {
    let tags = objects.get(h)?;
    let start = tags.iter().position(|t| t.code == 100 && t.str().trim() == "AcDbField").unwrap_or(0);
    let body = tags.get(start..)?;
    // The code and its overflow come before the value data (the first 7 ACFD_FIELD_VALUE).
    let end = body.iter().position(|t| t.code == 7).unwrap_or(body.len());
    let own = body.get(..end)?;
    let mut s = String::new();
    for t in own.iter().filter(|t| t.code == 2 || t.code == 3) {
        s.push_str(&t.str());
        if s.len() > MAX_LEN {
            return None;
        }
    }
    let children: Vec<String> = own.iter().filter(|t| t.code == 360).map(|t| t.str().trim().to_ascii_uppercase()).collect();
    for (i, c) in children.iter().enumerate().take(64) {
        let place = format!("%<\\_FldIdx {i}>%");
        if s.contains(&place) {
            let inner = code_in(objects, c, depth + 1, seen)?;
            // The filled-in code stays within the limit too (nested fields can multiply it).
            if inner.len() > MAX_LEN {
                return None;
            }
            s = s.replace(&place, &format!("%<{inner}>%"));
            if s.len() > MAX_LEN {
                return None;
            }
        }
    }
    Some(s)
}

/// Give the texts with AutoCAD fields their templates as formulas.
pub(crate) fn resolve(secs: &[Section], d: &mut Drawing) {
    let mut objects: HashMap<String, Vec<Tag>> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "OBJECTS") {
        for (kind, tags) in records(&s.tags) {
            if (kind == "DICTIONARY" || kind == "FIELD")
                && let Some(h) = handle_of(&tags)
            {
                objects.insert(h, tags);
            }
        }
    }
    if !objects.values().any(|t| t.iter().any(|g| g.code == 100 && g.str().trim() == "AcDbField")) {
        return;
    }
    let mut formulas: HashMap<Handle, String> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "ENTITIES" || s.name == "BLOCKS") {
        for (kind, tags) in records(&s.tags) {
            if !matches!(kind.as_str(), "TEXT" | "MTEXT") {
                continue;
            }
            let found = (|| {
                let x = xdict(&tags)?;
                let fd = entry(objects.get(&x)?, "ACAD_FIELD")?;
                let f = entry(objects.get(&fd)?, "TEXT")?;
                let c = code(&objects, &f, 0)?;
                let h = Handle::parse_hex(&handle_of(&tags)?)?;
                Some((h, c))
            })();
            if let Some((h, c)) = found.filter(|(_, c)| c.contains("%<\\")) {
                formulas.insert(h, c);
            }
        }
    }
    if formulas.is_empty() {
        return;
    }
    let set = |store: &mut EntityStore| {
        let hs: Vec<Handle> = store.iter().filter(|e| formulas.contains_key(&e.handle)).map(|e| e.handle).collect();
        for h in hs {
            let f = formulas.get(&h).cloned();
            store.modify(h, |e| match &mut e.kind {
                EntityKind::Text(t) => t.formula = f,
                EntityKind::MText(t) => t.formula = f,
                _ => {}
            });
        }
    };
    set(&mut d.model);
    for l in &mut d.layouts {
        set(&mut l.entities);
    }
    let names: Vec<String> =
        d.blocks.iter().filter(|(_, b)| b.entities.iter().any(|e| formulas.contains_key(&e.handle))).map(|(k, _)| k.clone()).collect();
    for k in names {
        if let Some(b) = d.blocks.get_mut(&k) {
            set(&mut std::sync::Arc::make_mut(b).entities);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_dxf::Tag;

    fn field(code: &str, children: &[&str]) -> Vec<Tag> {
        let mut t = vec![Tag::s(100, "AcDbField"), Tag::s(2, code)];
        t.extend(children.iter().map(|c| Tag::s(360, *c)));
        t
    }

    #[test]
    fn nested_fields_fill_their_places() {
        let mut o = HashMap::new();
        o.insert("A".to_string(), field("Date: %<\\_FldIdx 0>%", &["B"]));
        o.insert("B".to_string(), field("\\AcVar date", &[]));
        assert_eq!(code(&o, "A", 0).as_deref(), Some("Date: %<\\AcVar date>%"));
    }

    /// A hostile file whose fields each use the next one 60 times, 8 deep: the expansion would
    /// be 60^8 times the text. It is refused, quickly and without the memory.
    #[test]
    fn fields_that_multiply_are_refused_without_blowing_up() {
        let places: String = (0..60).map(|i| format!("%<\\_FldIdx {i}>%")).collect();
        let mut o = HashMap::new();
        for k in 0..=MAX_DEPTH {
            let next = format!("L{}", k + 1);
            let kids: Vec<&str> = (0..60).map(|_| next.as_str()).collect();
            o.insert(format!("L{k}"), field(&places, &kids));
        }
        o.insert(format!("L{}", MAX_DEPTH + 1), field("x", &[]));
        let t = std::time::Instant::now();
        assert_eq!(code(&o, "L0", 0), None);
        assert!(t.elapsed().as_secs() < 5, "took {:?}", t.elapsed());
    }

    /// The length cap alone: one level whose filled-in code would pass MAX_LEN is refused.
    #[test]
    fn a_filled_in_code_past_the_limit_is_refused() {
        let mut o = HashMap::new();
        o.insert("A".to_string(), field("%<\\_FldIdx 0>%%<\\_FldIdx 1>%%<\\_FldIdx 2>%", &["B", "B", "B"]));
        o.insert("B".to_string(), field(&"x".repeat(MAX_LEN / 2), &[]));
        assert_eq!(code(&o, "A", 0), None);
    }

    /// Shared children alone: eight levels of two places using one child each fit the limit and
    /// are worked out (each child once per level, not 2^8 times).
    #[test]
    fn shared_children_are_filled_in() {
        let mut o = HashMap::new();
        for k in 0..MAX_DEPTH {
            let next = format!("L{}", k + 1);
            o.insert(format!("L{k}"), field("%<\\_FldIdx 0>%%<\\_FldIdx 1>%", &[next.as_str(), next.as_str()]));
        }
        o.insert(format!("L{MAX_DEPTH}"), field("x", &[]));
        let c = code(&o, "L0", 0).expect("fits the limit");
        assert_eq!(c.matches('x').count(), 1 << MAX_DEPTH);
    }
}
