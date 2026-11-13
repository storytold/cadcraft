//! Saving a dynamic block under another name (BSAVEAS): the copy gets its own evaluation graph,
//! made from the original's objects with new handles, and its own copies of the block's objects
//! (no two blocks share a handle), so references to either change as AutoCAD would change them.

use std::collections::{HashMap, HashSet};

use crate::foreign::{ForeignExtra, ForeignObject};
use crate::{Drawing, EntityKind, EntityStore, Handle, RawTag};

use super::write::{ETAG, dict_entry, guid, record_handle, strip_app};
use super::{MAX_NODES, dynamic_block};

/// Most objects of one block's dynamic data that are copied.
const MAX_COPIED: usize = 8 * MAX_NODES;

/// Group codes whose values are handles: the object's own (5), owners and pointers (330 to 369)
/// and 390 to 399.
fn is_handle_code(code: i32) -> bool {
    code == 5 || (330..=369).contains(&code) || (390..=399).contains(&code)
}

/// The owner of an object: its first 330 group outside the `{ACAD_REACTORS` ... `}` groups.
fn owner_of(o: &ForeignObject) -> Option<Handle> {
    let mut inside = false;
    for t in &o.tags {
        match t.code {
            102 => inside = t.value.starts_with('{'),
            330 if !inside => return Handle::parse_hex(&t.value),
            _ => {}
        }
    }
    None
}

/// `root` and the objects it owns, directly or not (at most [`MAX_COPIED`]).
fn owned_subtree(d: &Drawing, root: Handle) -> Vec<Handle> {
    let mut children: HashMap<Handle, Vec<Handle>> = HashMap::new();
    for o in &d.foreign.objects {
        if let Some(owner) = owner_of(o) {
            children.entry(owner).or_default().push(o.handle);
        }
    }
    let mut out = vec![root];
    let mut seen: HashSet<Handle> = HashSet::from([root]);
    let mut at = 0;
    while let Some(h) = out.get(at).copied() {
        at += 1;
        for c in children.get(&h).into_iter().flatten() {
            if out.len() < MAX_COPIED && seen.insert(*c) {
                out.push(*c);
            }
        }
    }
    out
}

/// The handle `h` names, as `map` has it.
fn mapped(map: &HashMap<Handle, Handle>, value: &str) -> Option<Handle> {
    Handle::parse_hex(value).and_then(|h| map.get(&h)).copied()
}

/// Every handle in `tags` that `map` has, as the new one.
fn retarget(tags: &mut [RawTag], map: &HashMap<Handle, Handle>) {
    for t in tags.iter_mut().filter(|t| is_handle_code(t.code)) {
        if let Some(n) = mapped(map, &t.value) {
            t.value = n.hex();
        }
    }
}

/// The objects of `store` again with new handles (added to `map`).
fn rehandle(d: &mut Drawing, store: &EntityStore, map: &mut HashMap<Handle, Handle>) -> EntityStore {
    for e in store.iter() {
        let n = d.new_handle();
        map.insert(e.handle, n);
    }
    remap_handles(store, map)
}

/// The objects of `store` again with the handles `map` gives them: each one's element tag names
/// the object itself, and a hatch's sources are the new copies.
pub fn remap_handles(store: &EntityStore, map: &HashMap<Handle, Handle>) -> EntityStore {
    let mut out = EntityStore::new();
    for e in store.iter() {
        let mut c = (**e).clone();
        c.handle = map.get(&e.handle).copied().unwrap_or(e.handle);
        for t in c.common.xdata.iter_mut().filter(|t| t.code == 1005) {
            if let Some(n) = mapped(map, &t.value) {
                t.value = n.hex();
            }
        }
        if let EntityKind::Hatch(h) = &mut c.kind {
            for l in &mut h.loops {
                l.sources = l.sources.iter().map(|s| map.get(s).copied().unwrap_or(*s)).collect();
            }
        }
        out.push(c);
    }
    out
}

/// The kept extras of the objects of `store` (reactors within the block; not their extension
/// dictionaries), for their new handles.
fn entity_extras(d: &Drawing, store: &EntityStore, map: &HashMap<Handle, Handle>) -> Vec<(Handle, ForeignExtra)> {
    store
        .iter()
        .filter_map(|e| {
            let mut x = d.foreign.entities.get(&e.handle)?.clone();
            x.xdict = None;
            x.reactors = x.reactors.iter().filter_map(|r| map.get(r).copied()).collect();
            Some((map.get(&e.handle).copied()?, x))
        })
        .collect()
}

/// The applications whose record xdata make a block dynamic.
const DYNAMIC_APPS: [&str; 3] = ["AcDbDynamicBlockTrueName", "AcDbDynamicBlockGUID", ETAG];

/// Block `name`'s (upper case) dynamic data removed: the objects its extension dictionary owns
/// and its record's dynamic data. What a redefined block's old graph would point at is gone.
fn remove_definition(d: &mut Drawing, name: &str) {
    let key = ("BLOCK_RECORD".to_string(), name.to_string());
    let Some(xdict) = d.foreign.records.get(&key).and_then(|x| x.xdict) else { return };
    let owned: HashSet<Handle> = owned_subtree(d, xdict).into_iter().collect();
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    f.objects.retain(|o| !owned.contains(&o.handle));
    let empty = f.records.get_mut(&key).is_some_and(|x| {
        x.xdict = None;
        DYNAMIC_APPS.iter().for_each(|app| strip_app(&mut x.xdata, app));
        x.is_empty()
    });
    if empty {
        f.records.remove(&key);
    }
}

/// Block `name`'s dynamic data in `d` as `host` has it (BSAVEAS leaves the edited block as it
/// was: the editor's changes to its graph belong to the copy only).
pub fn restore_definition(d: &mut Drawing, host: &Drawing, name: &str) {
    let name = name.trim().to_ascii_uppercase();
    remove_definition(d, &name);
    let key = ("BLOCK_RECORD".to_string(), name);
    let Some(record) = host.foreign.records.get(&key) else { return };
    let owned: HashSet<Handle> = record.xdict.map(|x| owned_subtree(host, x)).unwrap_or_default().into_iter().collect();
    let objects: Vec<ForeignObject> = host.foreign.objects.iter().filter(|o| owned.contains(&o.handle)).cloned().collect();
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    f.objects.extend(objects);
    f.records.insert(key, record.clone());
}

/// Block record xdata `xdata` with the dynamic block's true name and GUID replaced.
fn rename_in_xdata(xdata: &mut [RawTag], name: &str, guid: &str) {
    let mut app = String::new();
    for t in xdata {
        match t.code {
            1001 => app = t.value.to_ascii_uppercase(),
            1000 if app == "ACDBDYNAMICBLOCKTRUENAME" => t.value = name.to_string(),
            1000 if app == "ACDBDYNAMICBLOCKGUID" => t.value = guid.to_string(),
            _ => {}
        }
    }
}

/// Make block `to` a copy of dynamic block `from` (BSAVEAS): `store`, the objects it is saved
/// with, get new handles; the copy has its own evaluation graph and objects, made from the
/// original's with new handles, and its own true name and GUID. What `to` had of dynamic data goes
/// first (a block replaced by one that isn't dynamic keeps none). Returns the handles the objects
/// of `store` got, by their old ones; nothing changes when `from` has no dynamic data or is `to`.
pub fn copy_definition(d: &mut Drawing, from: &str, to: &str, store: &mut EntityStore) -> HashMap<Handle, Handle> {
    let (from_name, to_name) = (from.trim().to_ascii_uppercase(), to.trim().to_ascii_uppercase());
    let key = |n: &str| ("BLOCK_RECORD".to_string(), n.to_string());
    if from_name == to_name {
        return HashMap::new();
    }
    remove_definition(d, &to_name);
    if dynamic_block(d, from).is_none() {
        return HashMap::new();
    }
    let Some(source) = d.foreign.records.get(&key(&from_name)).cloned() else { return HashMap::new() };
    let Some(xdict) = source.xdict else { return HashMap::new() };
    let graph = dict_entry(d, xdict, "ACAD_ENHANCEDBLOCK");
    let owned = owned_subtree(d, xdict);
    let owned_set: HashSet<Handle> = owned.iter().copied().collect();
    let mut map: HashMap<Handle, Handle> = owned.iter().map(|h| (*h, d.new_handle())).collect();
    let old_record = d.foreign.targets.iter().find(|(_, (k, n))| k == "BLOCK_RECORD" && *n == from_name).map(|(h, _)| *h);
    let new_record = record_handle(d, &to_name);
    map.extend(old_record.map(|h| (h, new_record)));
    let entities = rehandle(d, store, &mut map);
    let extras = entity_extras(d, store, &map);
    let copies: Vec<ForeignObject> = d
        .foreign
        .objects
        .iter()
        .filter(|o| owned_set.contains(&o.handle))
        .map(|o| {
            let mut c = o.clone();
            retarget(&mut c.tags, &map);
            c.handle = map.get(&o.handle).copied().unwrap_or(o.handle);
            c
        })
        .collect();
    let mut record = source;
    record.xdict = map.get(&xdict).copied();
    let new_graph = graph.and_then(|g| map.get(&g).copied()).unwrap_or(new_record);
    rename_in_xdata(&mut record.xdata, to.trim(), &guid(to.trim(), new_graph));
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    f.objects.extend(copies);
    f.records.insert(key(&to_name), record);
    f.entities.extend(extras);
    let made = store.iter().filter_map(|e| Some((e.handle, *map.get(&e.handle)?))).collect();
    *store = entities;
    made
}
