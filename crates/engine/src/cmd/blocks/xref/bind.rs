//! Binding external references: XREF Bind makes a loaded xref part of the host drawing (a normal
//! block), XBIND copies single dependent symbols into it.
//!
//! A dependent symbol `xref|name` becomes `xref$0$name` (the first free number) when bound, or
//! plain `name` with the Insert bind type (a host symbol of that name is then used instead). Names
//! of nested xrefs (`xref|nested|name`) bind with every `|` written `$0$`.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityStore, MAX_BLOCK_DEPTH, XrefStatus};

use super::load::{Sym, has_prefix, rename_refs};

/// The xref a dependent name belongs to and the rest of the name (`xref|rest`).
fn split(name: &str) -> Option<(&str, &str)> {
    name.split_once('|').filter(|(k, r)| !k.is_empty() && !r.is_empty())
}

/// Whether table `sym` of `d` has a symbol called `name`.
fn taken(d: &Drawing, sym: Sym, name: &str) -> bool {
    match sym {
        Sym::Layer => d.layer(name).is_some(),
        Sym::Linetype => d.linetype(name).is_some(),
        Sym::Style => d.text_style(name).is_some(),
        Sym::DimStyle => d.dim_style(name).is_some(),
        Sym::Block => d.block(name).is_some(),
        Sym::MLeaderStyle => d.mleader_styles.iter().any(|t| t.name.eq_ignore_ascii_case(name)),
        Sym::TableStyle => d.table_styles.iter().any(|t| t.name.eq_ignore_ascii_case(name)),
    }
}

/// The bound name of `key|rest`: `key$n$rest` with the first free `n`, or `rest` (Insert type).
fn bound_name(d: &Drawing, sym: Sym, key: &str, rest: &str, insert: bool) -> String {
    let rest = rest.replace('|', "$0$");
    if insert {
        return rest;
    }
    (0..10_000).map(|n| format!("{key}${n}${rest}")).find(|c| !taken(d, sym, c)).unwrap_or_else(|| format!("{key}$${rest}"))
}

/// Every table's symbol names, for one table at a time.
fn names(d: &Drawing, sym: Sym) -> Vec<String> {
    match sym {
        Sym::Layer => d.layers.iter().map(|t| t.name.clone()).collect(),
        Sym::Linetype => d.linetypes.iter().map(|t| t.name.clone()).collect(),
        Sym::Style => d.text_styles.iter().map(|t| t.name.clone()).collect(),
        Sym::DimStyle => d.dim_styles.iter().map(|t| t.name.clone()).collect(),
        Sym::Block => d.blocks.keys().cloned().collect(),
        Sym::MLeaderStyle => d.mleader_styles.iter().map(|t| t.name.clone()).collect(),
        Sym::TableStyle => d.table_styles.iter().map(|t| t.name.clone()).collect(),
    }
}

/// Rename symbol `old` of table `sym` to `new`, or drop it (`new` already exists: Insert type).
fn rename_symbol(d: &mut Drawing, sym: Sym, old: &str, new: &str) {
    let merge = !new.eq_ignore_ascii_case(old) && taken(d, sym, new);
    macro_rules! table {
        ($v:expr) => {{
            if merge {
                $v.retain(|t| !t.name.eq_ignore_ascii_case(old));
            } else if let Some(t) = $v.iter_mut().find(|t| t.name.eq_ignore_ascii_case(old)) {
                t.name = new.to_string();
                cadcraft_doc::KeepsHandle::forget_handle(t);
            }
        }};
    }
    match sym {
        Sym::Layer => table!(d.layers),
        Sym::Linetype => table!(d.linetypes),
        Sym::Style => table!(d.text_styles),
        Sym::DimStyle => table!(d.dim_styles),
        Sym::MLeaderStyle => table!(d.mleader_styles),
        Sym::TableStyle => table!(d.table_styles),
        Sym::Block => {
            if let Some(b) = d.blocks.remove(old)
                && !merge
            {
                let mut b = (*b).clone();
                cadcraft_doc::KeepsHandle::forget_handle(&mut b);
                b.name = new.to_string();
                b.anonymous = new.starts_with('*');
                d.blocks.insert(new.to_string(), Arc::new(b));
            }
        }
    }
}

const TABLES: [Sym; 7] = [Sym::Linetype, Sym::Style, Sym::Layer, Sym::DimStyle, Sym::MLeaderStyle, Sym::TableStyle, Sym::Block];

/// XREF Bind: xref `key` (loaded) becomes a normal block, its dependent symbols the host's.
pub(crate) fn bind(d: &mut Drawing, key: &str, insert: bool) -> Result<(), String> {
    let b = d.block(key).ok_or_else(|| format!("no xref \"{key}\""))?;
    if b.xref_path.is_none() {
        return Err(format!("\"{key}\" is not an xref"));
    }
    if b.xref_status != XrefStatus::Loaded {
        return Err(format!("xref \"{key}\" is not loaded"));
    }
    let key = b.name.clone();
    let prefix = format!("{key}|");
    let mut map: HashMap<(Sym, String), String> = HashMap::new();
    let mut bound_blocks = vec![key.clone()];
    for sym in TABLES {
        for old in names(d, sym).into_iter().filter(|n| has_prefix(n, &prefix)) {
            let rest = old.get(prefix.len()..).unwrap_or_default();
            let new = bound_name(d, sym, &key, rest, insert);
            // Insert type: a host symbol of the bound name is used instead of the xref's.
            let merged = taken(d, sym, &new);
            rename_symbol(d, sym, &old, &new);
            if sym == Sym::Block && !merged {
                bound_blocks.push(new.clone());
            }
            map.insert((sym, old.to_ascii_uppercase()), new);
        }
    }
    let get = |sym: Sym, n: &str| map.get(&(sym, n.to_ascii_uppercase())).cloned();
    for l in &mut d.layers {
        if let Some(n) = get(Sym::Linetype, &l.linetype) {
            l.linetype = n;
        }
    }
    for t in &mut d.dim_styles {
        if let Some(n) = get(Sym::Style, &t.text_style) {
            t.text_style = n;
        }
    }
    for t in &mut d.mleader_styles {
        if let Some(n) = get(Sym::Style, &t.text_style) {
            t.text_style = n;
        }
    }
    for name in bound_blocks {
        if let Some(b) = d.blocks.get_mut(&name) {
            let b = Arc::make_mut(b);
            let mut store = EntityStore::new();
            for e in b.entities.iter() {
                let mut e = (**e).clone();
                rename_refs(&mut e, &get);
                store.push(e);
            }
            b.entities = store;
        }
    }
    if let Some(b) = d.blocks.get_mut(&key) {
        let b = Arc::make_mut(b);
        b.xref_path = None;
        b.xref_overlay = false;
        b.xref_status = XrefStatus::NotLoaded;
    }
    Ok(())
}

/// XBIND: copy dependent symbol `name` (`xref|name`, of a loaded xref) into the host as
/// `xref$0$name`, with the dependent symbols it uses (a layer's linetype, a style's text style, a
/// block's contents). A symbol bound before is used again. Returns the bound name.
pub(crate) fn xbind(d: &mut Drawing, sym: Sym, name: &str) -> Result<String, String> {
    xbind_in(d, sym, name, 0)
}

fn xbind_in(d: &mut Drawing, sym: Sym, name: &str, depth: usize) -> Result<String, String> {
    if depth > MAX_BLOCK_DEPTH {
        return Err("blocks are nested too deeply".into());
    }
    let (key, rest) = split(name).ok_or_else(|| format!("\"{name}\" is not an xref-dependent name"))?;
    let loaded = d.block(key).is_some_and(|b| b.xref_path.is_some() && b.xref_status == XrefStatus::Loaded);
    if !loaded {
        return Err(format!("xref \"{key}\" is not loaded"));
    }
    if !taken(d, sym, name) {
        return Err(format!("no symbol \"{name}\""));
    }
    let new = format!("{key}$0${}", rest.replace('|', "$0$"));
    if taken(d, sym, &new) {
        return Ok(new);
    }
    let dep = |n: &str| n.contains('|');
    match sym {
        Sym::Layer => {
            let Some(mut l) = d.layer(name).cloned() else { return Err(format!("no layer \"{name}\"")) };
            if dep(&l.linetype) {
                l.linetype = xbind_in(d, Sym::Linetype, &l.linetype, depth + 1)?;
            }
            l.name = new.clone();
            l.handle = cadcraft_doc::KeptHandle::default();
            d.layers.push(l);
        }
        Sym::Linetype => {
            let Some(mut t) = d.linetype(name).cloned() else { return Err(format!("no linetype \"{name}\"")) };
            t.name = new.clone();
            t.handle = cadcraft_doc::KeptHandle::default();
            d.linetypes.push(t);
        }
        Sym::Style => {
            let Some(mut t) = d.text_style(name).cloned() else { return Err(format!("no text style \"{name}\"")) };
            t.name = new.clone();
            t.handle = cadcraft_doc::KeptHandle::default();
            d.text_styles.push(t);
        }
        Sym::DimStyle => {
            let Some(mut t) = d.dim_style(name).cloned() else { return Err(format!("no dimension style \"{name}\"")) };
            if dep(&t.text_style) {
                t.text_style = xbind_in(d, Sym::Style, &t.text_style, depth + 1)?;
            }
            t.name = new.clone();
            t.handle = cadcraft_doc::KeptHandle::default();
            d.dim_styles.push(t);
        }
        Sym::Block => {
            let Some(b) = d.block(name).cloned() else { return Err(format!("no block \"{name}\"")) };
            // The dependent symbols its objects use are bound first.
            let used: RefCell<BTreeSet<(Sym, String)>> = RefCell::default();
            for e in b.entities.iter() {
                let mut e = (**e).clone();
                rename_refs(&mut e, &|s, n| {
                    if dep(n) {
                        used.borrow_mut().insert((s, n.to_string()));
                    }
                    None
                });
            }
            let mut map: HashMap<(Sym, String), String> = HashMap::new();
            for (s, n) in used.into_inner() {
                let to = xbind_in(d, s, &n, depth + 1)?;
                map.insert((s, n.to_ascii_uppercase()), to);
            }
            let get = |s: Sym, n: &str| map.get(&(s, n.to_ascii_uppercase())).cloned();
            let mut nb = cadcraft_doc::Block::new(&new);
            nb.base = b.base;
            nb.description = b.description.clone();
            nb.explodable = b.explodable;
            nb.units = b.units;
            for e in b.entities.iter() {
                let mut e = (**e).clone();
                e.handle = d.new_handle();
                rename_refs(&mut e, &get);
                nb.entities.push(e);
            }
            d.blocks.insert(new.clone(), Arc::new(nb));
        }
        Sym::MLeaderStyle | Sym::TableStyle => return Err("only blocks, dimension styles, layers, linetypes and text styles can be bound".into()),
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_names() {
        let mut d = Drawing::new_imperial();
        assert_eq!(bound_name(&d, Sym::Layer, "SITE", "Walls", false), "SITE$0$Walls");
        d.ensure_layer("SITE$0$Walls");
        assert_eq!(bound_name(&d, Sym::Layer, "SITE", "walls", false), "SITE$1$walls");
        assert_eq!(bound_name(&d, Sym::Layer, "SITE", "Walls", true), "Walls");
        assert_eq!(bound_name(&d, Sym::Layer, "SITE", "TREES|Oak", false), "SITE$0$TREES$0$Oak");
        assert_eq!(split("A|b|c"), Some(("A", "b|c")));
        assert_eq!(split("|b"), None);
    }
}
