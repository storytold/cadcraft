//! Bringing another drawing's symbol tables and blocks into this one (INSERT of a file, IMPORT,
//! paste, COMPARE export).
//!
//! Both drawings are hostile input, so the work is linear in their sizes: names are looked up in a
//! set built once per table, and a generated block name continues from the last one allocated for
//! its prefix instead of probing every taken number again (CWE-407).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadcraft_doc::Drawing;

/// Add the entries of `theirs` whose names `ours` lacks (compared without letter case; a name that
/// exists keeps `ours`' definition). Returns how many were added.
pub(crate) fn merge_named<T: Clone>(ours: &mut Vec<T>, theirs: &[T], name: fn(&T) -> &str) -> usize {
    let mut have: HashSet<String> = ours.iter().map(|o| name(o).to_ascii_uppercase()).collect();
    let mut added = 0;
    for t in theirs {
        if have.insert(name(t).to_ascii_uppercase()) {
            ours.push(t.clone());
            added += 1;
        }
    }
    added
}

/// Which blocks of `src` to copy into `d`, and under which name: a new name keeps its own, a
/// generated (anonymous `*…`) block whose name is taken gets a fresh generated name, and a named
/// block `d` already has (or `defining`, the block being defined from `src`) keeps `d`'s
/// definition (not copied).
pub(crate) fn block_renames(d: &Drawing, src: &Drawing, defining: Option<&str>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut taken: BTreeSet<String> = d.blocks.keys().map(|k| k.to_ascii_uppercase()).collect();
    taken.extend(defining.map(str::to_ascii_uppercase));
    // The next number to try per generated prefix: numbers below it are taken.
    let mut next: HashMap<String, usize> = HashMap::new();
    for (name, b) in &src.blocks {
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("*MODEL_SPACE") || upper.starts_with("*PAPER_SPACE") {
            continue;
        }
        let to = if !taken.contains(&upper) {
            name.clone()
        } else if b.anonymous || name.starts_with('*') {
            let prefix: String = name.chars().take_while(|c| !c.is_ascii_digit()).collect();
            let cursor = next.entry(prefix.to_ascii_uppercase()).or_insert(1);
            match (*cursor..=1_000_000).map(|i| (i, format!("{prefix}{i}"))).find(|(_, n)| !taken.contains(&n.to_ascii_uppercase())) {
                Some((i, n)) => {
                    *cursor = i + 1;
                    n
                }
                None => continue,
            }
        } else {
            continue;
        };
        taken.insert(to.to_ascii_uppercase());
        out.insert(name.clone(), to);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cadcraft_doc::{Block, Layer};

    use super::*;

    #[test]
    fn merge_keeps_ours_and_adds_the_rest_without_regard_to_case() {
        let mut ours = vec![Layer::new("Walls")];
        let theirs = vec![Layer::new("WALLS"), Layer::new("Doors"), Layer::new("doors")];
        assert_eq!(merge_named(&mut ours, &theirs, |l| &l.name), 1);
        assert_eq!(ours.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Walls", "Doors"]);
    }

    /// Many colliding anonymous blocks get consecutive names in one pass, not a rescan each.
    #[test]
    fn anonymous_collisions_take_consecutive_numbers() {
        let mut d = Drawing::new_imperial();
        let mut src = Drawing::new_imperial();
        for i in 1..=5000 {
            let mut b = Block::new(&format!("*U{i}"));
            b.anonymous = true;
            d.blocks.insert(b.name.clone(), Arc::new(b.clone()));
            src.blocks.insert(b.name.clone(), Arc::new(b));
        }
        let started = std::time::Instant::now();
        let renames = block_renames(&d, &src, None);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(renames.len(), 5000);
        assert_eq!(renames.get("*U1").map(String::as_str), Some("*U5001"));
        let distinct: HashSet<&String> = renames.values().collect();
        assert_eq!(distinct.len(), 5000);
        // Every new name is the next free number: nothing below 5001 is free, nothing above 10000 is needed.
        assert!(renames.values().all(|n| n.strip_prefix("*U").and_then(|i| i.parse::<u32>().ok()).is_some_and(|i| (5001..=10000).contains(&i))));
    }
}
