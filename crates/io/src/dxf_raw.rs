//! What a DXF file holds that CADCraft does not model, kept as raw groups so that saving writes
//! it back (#528): entities and objects of other types, other applications' xdata, extension
//! dictionaries, persistent reactors, colour book colours and other unmodelled entity properties.
//!
//! Reading ([`read`]) keeps the data with the file's handles in [`Drawing::foreign`], plus the
//! role of every handle it refers to that CADCraft writes itself (a table record, a block, the
//! named object dictionary…). Writing ([`finish`]) runs over the finished tag list: it places the
//! entities of types CADCraft doesn't model among the written ones (the entity writer leaves them
//! out), puts the data back on the records that still exist and points each reference at what now
//! has the role it named. Kept objects get new handles above the written ones (so they never
//! collide with the handles the writer hands out). A reference to something that is gone is
//! dropped (from reactor lists and dictionaries) or written as the null handle `0`; objects whose
//! owner is gone go with it.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::foreign::{Foreign, ForeignExtra, ForeignKey, ForeignObject};
use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, RawTag, Unknown};
use cadcraft_dxf::{Section, Tag};

/// Named object dictionary entries CADCraft writes itself from its model (plus every key that
/// starts with `CADCRAFT`): they are read into the model, not kept raw. A change that starts
/// modelling another entry adds it here. (An entry the writer writes is also never written
/// twice: the kept one is left out.)
const MODELLED_KEYS: &[&str] = &[
    "ACAD_GROUP",
    "ACAD_LAYOUT",
    "ACAD_MLEADERSTYLE",
    "ACAD_MLINESTYLE",
    "ACAD_PLOTSTYLENAME",
    "ACAD_SCALELIST",
    "ACAD_TABLESTYLE",
    "ACAD_IMAGE_DICT",
    "ACAD_IMAGE_VARS",
];

/// Extension dictionary entries not kept raw: dimension associativity (CADCraft reads and
/// rewrites it), draw order and annotation scale contexts (written from their own models by the
/// changes that model them; stale copies would contradict the drawing).
const MODELLED_XDICT_KEYS: &[&str] = &["ACAD_DIMASSOC", "ACAD_SORTENTS", "ACDBCONTEXTDATAMANAGER"];

/// The annotation scale contexts of a block reference are kept, though: CADCraft doesn't model a
/// reference's scale at each annotation scale, and without them AutoCAD scales an annotative
/// reference once more by the current annotation scale. They are refreshed from the reference
/// when written ([`block_contexts`]).
const CONTEXTS_KEY: &str = "ACDBCONTEXTDATAMANAGER";

/// Whether extension dictionary entry `key` is left to CADCraft's model, in the extension
/// dictionary of a block reference (`of_insert`) or of something else.
fn modelled_xdict_key(key: &str, of_insert: bool) -> bool {
    let k = up(key);
    MODELLED_XDICT_KEYS.contains(&k.as_str()) && !(of_insert && k == CONTEXTS_KEY)
}

/// Applications whose xdata CADCraft reads and writes itself. `ACAD` holds dimension overrides
/// (DSTYLE) on dimensions and dimension styles; elsewhere it is kept.
const OWN_APPS: &[&str] = &["CADCRAFT", "ACADANNOTATIVE", "ACCMTRANSPARENCY", "ACAECLAYERSTANDARD"];

/// Most objects kept from one file.
const MAX_OBJECTS: usize = 1_000_000;
/// Most xdata groups kept per record (AutoCAD allows 16 KB of xdata per object).
const MAX_XDATA_GROUPS: usize = 16_384;
/// Longest ownership chain followed.
const MAX_DEPTH: usize = 64;

/// Record types that structure a file: never written as an entity of another type.
const STRUCTURAL: &[&str] = &["", "SECTION", "ENDSEC", "EOF", "TABLE", "ENDTAB", "BLOCK", "ENDBLK", "SEQEND", "VERTEX", "ATTRIB", "CLASS"];

fn up(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// A handle as text, normalised (`002a` → `2A`); other text upper-cased.
fn norm(s: &str) -> String {
    Handle::parse_hex(s).map_or_else(|| up(s), Handle::hex)
}

fn raw(t: &Tag) -> RawTag {
    RawTag { code: t.code, value: t.str() }
}

/// A kept group with the value type its code has (so binary output encodes it right); text that
/// doesn't parse as that type stays text.
fn tag(r: &RawTag) -> Tag {
    use cadcraft_dxf::{Kind, Value, kind_of};
    let v = r.value.trim();
    let value = match kind_of(r.code) {
        Kind::Real => v.parse::<f64>().ok().filter(|x| x.is_finite()).map(Value::Real),
        Kind::Int16 | Kind::Int32 | Kind::Int64 => v.parse::<i64>().ok().map(Value::Int),
        Kind::Bool => v.parse::<i64>().ok().map(|x| Value::Bool(x != 0)),
        Kind::Hex => Some(Value::Hex(v.to_string())),
        Kind::Str => None,
    };
    Tag::new(r.code, value.unwrap_or_else(|| Value::Str(r.value.clone())))
}

/// A handle value we believe: hexadecimal and not null. DXF handles are up to 16 hex digits and
/// AutoCAD writes some above 2^48 (e.g. 0xDAD1…), so the whole 64-bit range is believed.
fn handle(s: &str) -> Option<Handle> {
    Handle::parse_hex(s).filter(|h| h.0 != 0)
}

/// Group codes whose values are handles of other objects (DXF Reference, group code ranges).
fn is_pointer(code: i32) -> bool {
    matches!(code, 320..=369 | 390..=399 | 480..=481 | 1005)
}

/// The records of a run of tags: (type, index of the record's `0` group, groups after it).
fn split(tags: &[Tag]) -> Vec<(String, usize, &[Tag])> {
    let mut out = Vec::new();
    let mut open: Option<(String, usize)> = None;
    for (i, t) in tags.iter().enumerate() {
        if t.code == 0 {
            if let Some((k, s)) = open.take() {
                out.push((k, s, tags.get(s + 1..i).unwrap_or(&[])));
            }
            open = Some((t.str(), i));
        }
    }
    if let Some((k, s)) = open {
        out.push((k, s, tags.get(s + 1..).unwrap_or(&[])));
    }
    out
}

type Recs<'a> = Vec<(String, usize, &'a [Tag])>;

/// A record's handle, owner, reactors and extension dictionary (groups before its xdata).
#[derive(Default)]
struct Head {
    handle: Option<String>,
    owner: Option<String>,
    reactors: Vec<String>,
    xdict: Option<String>,
    has_reactors: bool,
}

fn head(tags: &[Tag]) -> Head {
    let mut h = Head::default();
    let mut group: Option<String> = None;
    for t in tags.iter().take_while(|t| t.code < 1000) {
        match (t.code, group.as_deref()) {
            (102, _) => {
                let v = t.str();
                group = v.trim().strip_prefix('{').map(str::to_string);
                h.has_reactors |= group.as_deref() == Some("ACAD_REACTORS");
            }
            (_, Some("ACAD_REACTORS")) => h.reactors.push(norm(&t.str())),
            (_, Some("ACAD_XDICTIONARY")) => {
                if h.xdict.is_none() {
                    h.xdict = Some(norm(&t.str()));
                }
            }
            (_, Some(_)) => {}
            (5 | 105, None) if h.handle.is_none() => h.handle = Some(norm(&t.str())),
            (330, None) if h.owner.is_none() => h.owner = Some(norm(&t.str())),
            _ => {}
        }
    }
    h
}

fn val(tags: &[Tag], code: i32) -> Option<String> {
    tags.iter().take_while(|t| t.code < 1000).find(|t| t.code == code).map(Tag::str)
}

/// A dictionary's entries: (key, upper-case handle).
fn entries(tags: &[Tag]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut key: Option<String> = None;
    for t in tags.iter().take_while(|t| t.code < 1000) {
        match t.code {
            3 => key = Some(t.str()),
            350 | 360 => {
                if let Some(k) = key.take() {
                    out.push((k, norm(&t.str())));
                }
            }
            _ => {}
        }
    }
    out
}

fn is_dictionary(kind: &str) -> bool {
    kind == "DICTIONARY" || kind == "ACDBDICTIONARYWDFLT"
}

fn modelled_key(key: &str) -> bool {
    let k = up(key);
    k.starts_with("CADCRAFT") || MODELLED_KEYS.contains(&k.as_str())
}

/// The roles of a file's handles (see [`ForeignKey`]): table records and tables by type and
/// name, blocks by name, the named object dictionary, its entries, and their entries.
fn identities(tables: &Recs, blocks: &Recs, entities: &Recs, objects: &Recs) -> HashMap<String, ForeignKey> {
    let mut out: HashMap<String, ForeignKey> = HashMap::new();
    let mut put = |h: Option<String>, k: ForeignKey| {
        if let Some(h) = h {
            out.entry(h).or_insert(k);
        }
    };
    // Attributes (written with new handles each time) by their reference and tag.
    let attribs: Vec<&[Tag]> = blocks.iter().chain(entities).filter(|(k, _, _)| k == "ATTRIB").map(|(_, _, t)| *t).collect();
    for (t, key) in attribs.iter().zip(attrib_keys(attribs.iter().copied())) {
        put(head(t).handle, ("ATTRIB".into(), key));
    }
    for (kind, _, t) in tables {
        let name = up(&val(t, 2).unwrap_or_default());
        if kind == "TABLE" || !name.is_empty() {
            put(head(t).handle, (if kind == "TABLE" { "TABLE".into() } else { kind.clone() }, name));
        }
    }
    let mut block = String::new();
    for (kind, _, t) in blocks {
        match kind.as_str() {
            "BLOCK" => {
                block = up(&val(t, 2).unwrap_or_default());
                put(head(t).handle, ("BLOCK".into(), block.clone()));
            }
            "ENDBLK" => put(head(t).handle, ("ENDBLK".into(), block.clone())),
            _ => {}
        }
    }
    // Annotation scales by name (written again from the model, maybe under other keys): what
    // annotation scale contexts refer to.
    for (kind, _, t) in objects.iter().filter(|(k, _, _)| k.eq_ignore_ascii_case("SCALE")) {
        if let Some(name) = val(t, 300).filter(|n| !n.trim().is_empty()) {
            put(head(t).handle, (kind.to_ascii_uppercase(), up(&name)));
        }
    }
    let Some((_, _, root)) = objects.first().filter(|(k, _, _)| is_dictionary(k)) else { return out };
    put(head(root).handle, ("NOD".into(), String::new()));
    let by_handle: HashMap<String, (&str, &[Tag])> = objects.iter().filter_map(|(k, _, t)| Some((head(t).handle?, (k.as_str(), *t)))).rev().collect();
    for (key, h) in entries(root) {
        put(Some(h.clone()), ("NOD".into(), up(&key)));
        if let Some((k, t)) = by_handle.get(&h)
            && is_dictionary(k)
        {
            for (name, eh) in entries(t) {
                put(Some(eh), (format!("NOD/{}", up(&key)), up(&name)));
            }
        }
    }
    out
}

/// Other applications' xdata of a record of type `kind`: whole applications, each with valid
/// groups (codes 1000 and up, balanced `1002` braces), up to [`MAX_XDATA_GROUPS`].
fn foreign_xdata(kind: &str, tags: &[Tag]) -> Vec<RawTag> {
    let Some(start) = tags.iter().position(|t| t.code == 1001) else { return Vec::new() };
    let rest = tags.get(start..).unwrap_or(&[]);
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(first) = rest.get(i) {
        let len = rest.get(i + 1..).map_or(0, |r| r.iter().position(|t| t.code == 1001).unwrap_or(r.len()));
        let chunk = rest.get(i..i + 1 + len).unwrap_or(&[]);
        i += 1 + len;
        let app = up(&first.str());
        let own = OWN_APPS.contains(&app.as_str()) || (app == "ACAD" && matches!(kind, "DIMENSION" | "DIMSTYLE"));
        if app.is_empty() || own || out.len() + chunk.len() > MAX_XDATA_GROUPS {
            continue;
        }
        let mut depth = 0i32;
        let mut ok = true;
        for t in chunk.iter().skip(1) {
            match (t.code, t.str().trim()) {
                (c, _) if c < 1000 => ok = false,
                (1002, "{") => depth += 1,
                (1002, "}") => {
                    depth -= 1;
                    ok &= depth >= 0;
                }
                _ => {}
            }
        }
        if ok && depth == 0 {
            out.extend(chunk.iter().map(raw));
        }
    }
    out
}

/// The groups of an entity's AcDbEntity subclass (after `100 AcDbEntity`, up to the next `100`).
fn entity_groups(tags: &[Tag]) -> &[Tag] {
    let Some(start) = tags.iter().position(|t| t.code == 100 && t.str().trim() == "AcDbEntity") else { return &[] };
    let rest = tags.get(start + 1..).unwrap_or(&[]);
    let end = rest.iter().position(|t| t.code == 100 || t.code >= 1000).unwrap_or(rest.len());
    rest.get(..end).unwrap_or(&[])
}

/// What an entity record (`entity`) or a table record carries beyond the model; reactors and
/// the extension dictionary only when kept (`kept`).
fn extra(kind: &str, tags: &[Tag], entity: bool, unknown: bool, kept: &HashSet<String>) -> ForeignExtra {
    let h = head(tags);
    let mut x = ForeignExtra {
        xdict: h.xdict.filter(|x| kept.contains(x)).and_then(|x| handle(&x)),
        reactors: h.reactors.iter().filter(|r| kept.contains(*r)).filter_map(|r| handle(r)).collect(),
        xdata: foreign_xdata(kind, tags),
        ..ForeignExtra::default()
    };
    // An unknown entity's own groups are written as they are; of table records, layers have a
    // colour book colour and a material.
    if !unknown && (entity || kind == "LAYER") {
        let props =
            if entity { entity_groups(tags) } else { tags.get(..tags.iter().position(|t| t.code >= 1000).unwrap_or(tags.len())).unwrap_or(&[]) };
        let book = props.iter().find(|t| t.code == 430).map(Tag::str);
        let rgb = props.iter().find(|t| t.code == 420).map(Tag::str);
        if let (Some(book), Some(rgb)) = (book, rgb)
            && !book.trim().is_empty()
        {
            x.color_book = Some((rgb, book));
        }
        x.props = props.iter().filter(|t| matches!(t.code, 347 | 284)).map(raw).collect();
    }
    x
}

/// Every entity of the drawing (model space, layouts, blocks).
fn all_entities(d: &Drawing) -> impl Iterator<Item = &Entity> {
    std::iter::once(&d.model)
        .chain(d.layouts.iter().map(|l| &l.entities))
        .chain(d.blocks.values().map(|b| &b.entities))
        .flat_map(|s| s.iter().map(|e| e.as_ref()))
}

/// Give each entity the xdata read for it (`xdata` by entity handle).
fn put_xdata(d: &mut Drawing, mut xdata: HashMap<Handle, Vec<RawTag>>) {
    if xdata.is_empty() {
        return;
    }
    let mut give = |store: &mut cadcraft_doc::EntityStore| {
        let hs: Vec<Handle> = store.iter().map(|e| e.handle).filter(|h| xdata.contains_key(h)).collect();
        for h in hs {
            if let Some(xd) = xdata.remove(&h) {
                store.modify(h, |e| e.common.xdata = xd);
            }
        }
    };
    give(&mut d.model);
    for l in &mut d.layouts {
        give(&mut l.entities);
    }
    for b in d.blocks.values_mut() {
        give(&mut Arc::make_mut(b).entities);
    }
}

/// Keep what `secs` holds that the reader did not model in `d.foreign`.
pub(crate) fn read(secs: &[Section], d: &mut Drawing) {
    let get = |name: &str| secs.iter().find(|s| s.name == name).map(|s| split(&s.tags)).unwrap_or_default();
    let (classes, tables, blocks, entities, objects) = (get("CLASSES"), get("TABLES"), get("BLOCKS"), get("ENTITIES"), get("OBJECTS"));
    // The drawing's entities by handle: whether each is of a type CADCraft doesn't model.
    let mut ents: HashMap<String, bool> = HashMap::new();
    for e in all_entities(d) {
        ents.entry(e.handle.hex()).or_insert(matches!(e.kind, EntityKind::Unknown(_)));
    }
    let ids = identities(&tables, &blocks, &entities, &objects);
    // Extension dictionaries of entities and records: owner → dictionary.
    let mut xdict_of: HashMap<String, String> = HashMap::new();
    for (kind, _, t) in tables.iter().chain(&blocks).chain(&entities) {
        if kind == "ENDBLK" {
            continue;
        }
        let h = head(t);
        if let (Some(o), Some(x)) = (h.handle, h.xdict) {
            xdict_of.entry(o).or_insert(x);
        }
    }
    // Objects by handle: the first of each, and none that has an entity's handle.
    let mut by_handle: HashMap<String, usize> = HashMap::new();
    for (i, (_, _, t)) in objects.iter().enumerate() {
        if let Some(h) = head(t).handle.filter(|h| handle(h).is_some() && !ents.contains_key(h)) {
            by_handle.entry(h).or_insert(i);
        }
    }
    let root = objects.first().filter(|(k, _, _)| is_dictionary(k)).and_then(|(_, _, t)| head(t).handle);
    let root_entries = objects.first().filter(|(k, _, _)| is_dictionary(k)).map(|(_, _, t)| entries(t)).unwrap_or_default();
    let root_key = |h: &str| root_entries.iter().find(|(_, eh)| eh == h).map(|(k, _)| k.clone());
    // An extension dictionary of an entity or record (not of another object).
    let is_xdict = |k: usize| {
        let Some((_, _, t)) = objects.get(k) else { return false };
        let hd = head(t);
        let (Some(h), Some(o)) = (hd.handle, hd.owner) else { return false };
        !by_handle.contains_key(&o) && xdict_of.get(&o) == Some(&h) && (ents.contains_key(&o) || ids.contains_key(&o))
    };
    // Block references and their attributes (their extension dictionaries keep annotation scale
    // contexts).
    let inserts: HashSet<String> = all_entities(d).filter(|e| matches!(e.kind, EntityKind::Insert(_))).map(|e| e.handle.hex()).collect();
    let of_insert = |t: &[Tag]| head(t).owner.is_some_and(|o| inserts.contains(&o) || ids.get(&o).is_some_and(|k| k.0 == "ATTRIB"));
    let modelled_entry = |t: &[Tag], h: &str| entries(t).iter().any(|(k, eh)| eh == h && modelled_xdict_key(k, of_insert(t)));
    // Which objects to keep: those under a named object dictionary entry CADCraft doesn't model,
    // under the extension dictionary of an entity or record (minus modelled entries; one left
    // empty goes), or owned by an entity of a type CADCraft doesn't model. Memoized along each
    // ownership chain; cycles and over-long chains are not kept.
    let mut memo: HashMap<usize, bool> = HashMap::new();
    for i in 0..objects.len() {
        let mut path: Vec<usize> = Vec::new();
        let mut cur = i;
        let verdict = loop {
            if let Some(&v) = memo.get(&cur) {
                break v;
            }
            if path.contains(&cur) || path.len() >= MAX_DEPTH {
                break false;
            }
            path.push(cur);
            let Some((_, _, t)) = objects.get(cur) else { break false };
            let hd = head(t);
            let (Some(h), Some(o)) = (hd.handle, hd.owner) else { break false };
            if by_handle.get(&h) != Some(&cur) {
                break false;
            }
            if Some(&o) == root.as_ref() {
                break root_key(&h).is_some_and(|k| !modelled_key(&k));
            }
            if let Some(&j) = by_handle.get(&o) {
                if is_xdict(j) && objects.get(j).is_some_and(|p| modelled_entry(p.2, &h)) {
                    break false;
                }
                cur = j;
                continue;
            }
            if is_xdict(cur) {
                break entries(t).iter().any(|(k, _)| !modelled_xdict_key(k, of_insert(t)));
            }
            break ents.get(&o) == Some(&true);
        };
        for p in path {
            memo.insert(p, verdict);
        }
    }
    let mut f = Foreign::default();
    let mut kept: HashSet<String> = HashSet::new();
    for (i, (kind, _, t)) in objects.iter().enumerate() {
        if memo.get(&i) != Some(&true) || f.objects.len() >= MAX_OBJECTS {
            continue;
        }
        let Some(h) = head(t).handle.and_then(|h| handle(&h)) else { continue };
        let mut tags: Vec<RawTag> = t.iter().map(raw).collect();
        if is_xdict(i) {
            strip_entries(&mut tags, of_insert(t));
        }
        if head(t).owner == root
            && let Some(key) = root_key(&h.hex())
        {
            f.dictionary.push((key, h));
        }
        kept.insert(h.hex());
        f.objects.push(ForeignObject { handle: h, kind: kind.clone(), tags });
    }
    // Extras of entities, tables, table records and blocks.
    let mut xdata_of: HashMap<Handle, Vec<RawTag>> = HashMap::new();
    for (kind, _, t) in blocks.iter().chain(&entities) {
        if matches!(kind.as_str(), "BLOCK" | "ENDBLK") {
            continue;
        }
        let Some(h) = head(t).handle else { continue };
        let Some(&unknown) = ents.get(&h) else { continue };
        let mut x = extra(kind, t, true, unknown, &kept);
        let Some(hh) = handle(&h) else { continue };
        // Xdata belongs to the entity itself (it follows the entity's copies).
        let xd = std::mem::take(&mut x.xdata);
        if !xd.is_empty() {
            xdata_of.entry(hh).or_insert(xd);
        }
        if !x.is_empty() {
            f.entities.entry(hh).or_insert(x);
        }
    }
    put_xdata(d, xdata_of);
    // Attributes' extension dictionaries (their annotation scale contexts), by attribute.
    let attribs: Vec<&[Tag]> = blocks.iter().chain(&entities).filter(|(k, _, _)| k == "ATTRIB").map(|(_, _, t)| *t).collect();
    for (t, key) in attribs.iter().zip(attrib_keys(attribs.iter().copied())) {
        if let Some(xd) = head(t).xdict.filter(|x| kept.contains(x)).and_then(|x| handle(&x)) {
            f.records.entry(("ATTRIB".into(), key)).or_insert(ForeignExtra { xdict: Some(xd), ..ForeignExtra::default() });
        }
    }
    for (kind, _, t) in tables.iter().chain(blocks.iter().filter(|(k, _, _)| k == "BLOCK")) {
        let Some(key) = head(t).handle.and_then(|h| ids.get(&h).cloned()) else { continue };
        let x = extra(kind, t, kind == "BLOCK", false, &kept);
        if !x.is_empty() {
            f.records.entry(key).or_insert(x);
        }
    }
    // Block references whose extension dictionaries are kept (their annotation scale contexts),
    // as read.
    for e in all_entities(d) {
        if let EntityKind::Insert(ins) = &e.kind
            && f.entities.get(&e.handle).is_some_and(|x| x.xdict.is_some())
        {
            let attribs = ins
                .attribs
                .iter()
                .map(|a| cadcraft_doc::foreign::KeptAttrib {
                    tag: a.tag.clone(),
                    insert: a.text.insert,
                    align: a.text.align_pt.unwrap_or(a.text.insert),
                    rotation: a.text.rotation,
                })
                .collect();
            f.placements
                .insert(e.handle, cadcraft_doc::foreign::KeptPlacement { insert: ins.insert, rotation: ins.rotation, scale: ins.scale, attribs });
        }
    }
    // The classes of kept objects and of entities of types CADCraft doesn't model.
    let mut types: HashSet<String> = f.objects.iter().map(|o| up(&o.kind)).collect();
    types.extend(all_entities(d).filter_map(|e| match &e.kind {
        EntityKind::Unknown(u) => Some(up(&u.dxf_type)),
        _ => None,
    }));
    for (kind, _, t) in &classes {
        if kind == "CLASS" && val(t, 1).is_some_and(|n| types.contains(&up(&n))) && f.classes.len() < MAX_OBJECTS {
            f.classes.push(t.iter().map(raw).collect());
        }
    }
    // The roles of the handles kept data refers to.
    let mut refs: Vec<String> = Vec::new();
    let mut note = |tags: &[RawTag]| refs.extend(tags.iter().filter(|r| is_pointer(r.code)).map(|r| norm(&r.value)));
    for o in &f.objects {
        note(&o.tags);
    }
    for x in f.entities.values().chain(f.records.values()) {
        note(&x.xdata);
        note(&x.props);
    }
    for e in all_entities(d) {
        note(&e.common.xdata);
    }
    for e in all_entities(d) {
        if let EntityKind::Unknown(u) = &e.kind {
            note(&u.tags);
        }
    }
    for r in &refs {
        if let (Some(h), Some(k), false) = (handle(r), ids.get(r), kept.contains(r)) {
            f.targets.insert(h, k.clone());
        }
    }
    // The data store (ACDSDATA): the ACIS geometry of solids, regions and bodies from 2013 on.
    if let Some(ds) = secs.iter().find(|s| s.name.eq_ignore_ascii_case("ACDSDATA")) {
        f.data_store = ds.tags.iter().take(MAX_DATA_STORE_GROUPS).map(raw).collect();
    }
    if !f.is_empty() {
        d.foreign = Arc::new(f);
    }
}

/// Most groups of a data store section kept (a damaged file's can be endless).
const MAX_DATA_STORE_GROUPS: usize = 1 << 26;

/// Write the drawing's kept data store section (ACDSDATA) before the end of `tags`, without the
/// records of entities that are no longer written. Only for files of AutoCAD 2013 and later
/// (`modern`), whose solids, regions and bodies keep their geometry there; the DWG bridge reads it
/// too. Nothing is written when no record's entity remains.
pub(crate) fn data_store(tags: &mut Vec<Tag>, d: &Drawing, modern: bool) {
    let ds = &d.foreign.data_store;
    if !modern || ds.is_empty() {
        return;
    }
    let written: HashSet<String> = tags.iter().filter(|t| t.code == 5).map(|t| norm(&t.str())).collect();
    // The section's own groups before its first record, then its schemas and records.
    let mut out: Vec<Tag> = vec![Tag::s(0, "SECTION"), Tag::s(2, "ACDSDATA")];
    let first = ds.iter().position(|t| t.code == 0).unwrap_or(ds.len());
    out.extend(ds.iter().take(first).map(tag));
    let mut records = 0usize;
    let mut i = first;
    while i < ds.len() {
        let end = ds.iter().skip(i + 1).position(|t| t.code == 0).map_or(ds.len(), |k| i + 1 + k);
        let rec = ds.get(i..end).unwrap_or(&[]);
        let is_record = rec.first().is_some_and(|t| t.value.trim().eq_ignore_ascii_case("ACDSRECORD"));
        let owner = rec.iter().find(|t| t.code == 320).map(|t| norm(&t.value));
        if !is_record || owner.as_ref().is_some_and(|h| written.contains(h)) {
            records += usize::from(is_record);
            out.extend(rec.iter().map(tag));
        }
        i = end;
    }
    if records == 0 {
        return;
    }
    out.push(Tag::s(0, "ENDSEC"));
    let at = tags.iter().rposition(|t| t.code == 0 && t.str() == "EOF").unwrap_or(tags.len());
    tags.splice(at..at, out);
}

/// The identities across saves (an attribute's handle is new each time) of ATTRIB records, in
/// order: the reference's handle, the tag and the attribute's place among the reference's
/// attributes with that tag (a block can have two attributes with one tag; reading and writing
/// keep their order).
fn attrib_keys<'a>(attribs: impl Iterator<Item = &'a [Tag]>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    attribs
        .map(|t| {
            let base = format!("{}/{}", head(t).owner.unwrap_or_default(), up(&val(t, 2).unwrap_or_default()));
            let n = seen.entry(base.clone()).or_insert(0);
            let key = format!("{base}/{n}");
            *n += 1;
            key
        })
        .collect()
}

/// An attribute's identity ([`attrib_keys`]) split: its reference, tag and place among the
/// reference's attributes with that tag.
fn split_attrib_key(key: &str) -> Option<(Handle, &str, usize)> {
    let (ins, rest) = key.split_once('/')?;
    let (tag, nth) = rest.rsplit_once('/')?;
    Some((Handle::parse_hex(ins)?, tag, nth.parse().ok()?))
}

/// The kept annotation scale contexts of block references, brought up to date with each
/// reference (see [`CONTEXTS_KEY`]). A context holds the reference's insertion point, rotation
/// (degrees) and scale at one annotation scale; the default context (group 290 = 1) was the
/// reference itself when the file was read, so each context moves, turns and scales by what the
/// reference did since. Only the contexts of references that changed are returned (by handle).
fn block_contexts(f: &Foreign, d: &Drawing, owners: &HashMap<String, String>) -> HashMap<String, ForeignObject> {
    let num = |o: &ForeignObject, code: i32| {
        o.tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    };
    // The entity a kept object belongs to: the owner past the kept dictionaries.
    let owner_entity = |h: &str| {
        let mut cur = h.to_string();
        for _ in 0..MAX_DEPTH {
            match owners.get(&cur) {
                Some(o) if owners.contains_key(o) => cur = o.clone(),
                Some(o) => return Handle::parse_hex(o),
                None => return None,
            }
        }
        None
    };
    // The contexts of each reference, together (they share their dictionary).
    let mut by_parent: BTreeMap<String, Vec<&ForeignObject>> = BTreeMap::new();
    for o in f.objects.iter().filter(|o| o.kind.eq_ignore_ascii_case("ACDB_BLKREFOBJECTCONTEXTDATA_CLASS")) {
        if let Some(p) = owners.get(&o.handle.hex()) {
            by_parent.entry(p.clone()).or_default().push(o);
        }
    }
    let mut out = HashMap::new();
    for list in by_parent.values() {
        let Some(first) = list.first() else { continue };
        let Some(h) = owner_entity(&first.handle.hex()) else { continue };
        let Some(EntityKind::Insert(ins)) = d.entity(h).map(|e| &e.kind) else { continue };
        // The reference as read: kept when it was read, else (data made before that was kept)
        // its default context.
        let (at0, rot0, scale0) = match f.placements.get(&h) {
            Some(p) => (p.insert.xy(), p.rotation, [p.scale.x, p.scale.y, p.scale.z]),
            None => {
                let def = list.iter().find(|o| is_default(o)).unwrap_or(first);
                let (Some(x0), Some(y0)) = (num(def, 10), num(def, 20)) else { continue };
                let s = |code: i32, now: f64| num(def, code).unwrap_or(now);
                let rot = num(def, 50).unwrap_or(0.0).to_radians();
                (cadcraft_doc::geom::Vec2::new(x0, y0), rot, [s(41, ins.scale.x), s(42, ins.scale.y), s(43, ins.scale.z)])
            }
        };
        let turn = ins.rotation - rot0;
        let ratio = |then: f64, now: f64| if then.abs() > 1e-12 { now / then } else { 1.0 };
        let ratios = [(41, ratio(scale0[0], ins.scale.x)), (42, ratio(scale0[1], ins.scale.y)), (43, ratio(scale0[2], ins.scale.z))];
        let unchanged = (ins.insert.xy() - at0).len() < 1e-9 && turn.abs() < 1e-12 && ratios.iter().all(|(_, r)| (r - 1.0).abs() < 1e-12);
        if unchanged || !ratios.iter().all(|(_, r)| r.is_finite()) || !turn.is_finite() {
            continue;
        }
        for c in list {
            let mut o = (*c).clone();
            // Its offset from where the reference was moves as the reference did: back into the
            // reference's frame then, scaled (a mirror flips it), turned to its frame now.
            let at = match (num(c, 10), num(c, 20)) {
                (Some(x), Some(y)) => {
                    let off = (cadcraft_doc::geom::Vec2::new(x, y) - at0).rotate(-rot0);
                    ins.insert.xy() + cadcraft_doc::geom::Vec2::new(off.x * ratios[0].1, off.y * ratios[1].1).rotate(ins.rotation)
                }
                _ => ins.insert.xy(),
            };
            for t in &mut o.tags {
                let v = match t.code {
                    10 => Some(at.x),
                    20 => Some(at.y),
                    30 => Some(ins.insert.z),
                    50 => num(c, 50).map(|r| r + turn.to_degrees()),
                    41..=43 => ratios.iter().find(|(k, _)| *k == t.code).and_then(|(k, r)| num(c, *k).map(|s| s * r)),
                    _ => None,
                };
                if let Some(v) = v.filter(|v| v.is_finite()) {
                    t.value = v.to_string();
                }
            }
            out.insert(o.handle.hex(), o);
        }
    }
    out
}

/// The kept annotation scale contexts of block references' attributes, brought up to date. A
/// context holds the attribute's insertion and alignment points (10, 11) and rotation (50) at one
/// annotation scale. It keeps its own place in the block at that scale (as read, through the
/// reference's context for the same scale), moved by what the attribute did in the block since
/// (seen in its default context, 290 = 1), and is placed by the reference's context as it is now
/// (`block`: the reference contexts [`block_contexts`] refreshed). Only changed contexts are
/// returned (by handle).
fn attribute_contexts(
    f: &Foreign,
    d: &Drawing,
    owners: &HashMap<String, String>,
    block: &HashMap<String, ForeignObject>,
) -> HashMap<String, ForeignObject> {
    use cadcraft_doc::geom::{Mat3, Vec2};
    let num = |o: &ForeignObject, code: i32| {
        o.tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    };
    let text = |o: &ForeignObject, code: i32| o.tags.iter().find(|t| t.code == code).map(|t| norm(&t.value));
    // The last kept object up the owner chain (an extension dictionary) and its owner.
    let top = |h: &str| {
        let mut cur = h.to_string();
        for _ in 0..MAX_DEPTH {
            match owners.get(&cur) {
                Some(o) if owners.contains_key(o) => cur = o.clone(),
                Some(o) => return Some((cur, o.clone())),
                None => return None,
            }
        }
        None
    };
    let attribs = attribute_xdicts(f);
    // A reference's context at each scale, as read and as written: (insertion, degrees, scale).
    type Place = (Vec2, f64, (f64, f64));
    let place =
        |o: &ForeignObject| -> Option<Place> { Some((Vec2::new(num(o, 10)?, num(o, 20)?), num(o, 50).unwrap_or(0.0), (num(o, 41)?, num(o, 42)?))) };
    let mut read: HashMap<(Handle, String), Place> = HashMap::new();
    let mut now: HashMap<(Handle, String), Place> = HashMap::new();
    for o in f.objects.iter().filter(|o| o.kind.eq_ignore_ascii_case("ACDB_BLKREFOBJECTCONTEXTDATA_CLASS")) {
        let (Some((_, r)), Some(s)) = (top(&o.handle.hex()), text(o, 340)) else { continue };
        let Some(r) = Handle::parse_hex(&r) else { continue };
        if let Some(p) = place(o) {
            read.insert((r, s.clone()), p);
        }
        if let Some(p) = place(block.get(&o.handle.hex()).unwrap_or(o)) {
            now.insert((r, s), p);
        }
    }
    // The block-to-drawing transform of a reference placed at `p` (its block's base `base`).
    let matrix = |ins: &cadcraft_doc::Insert, base: Vec2, p: Place| {
        let mut i = ins.clone();
        i.insert = p.0.to3(ins.insert.z);
        i.rotation = p.1.to_radians();
        i.scale = cadcraft_doc::geom::Vec3::new(p.2.0, p.2.1, ins.scale.z);
        i.transform(base)
    };
    // Each attribute's contexts, together.
    let mut by_attrib: BTreeMap<String, Vec<&ForeignObject>> = BTreeMap::new();
    for o in f.objects.iter().filter(|o| up(&o.kind).contains("ATTRIBUTEOBJECTCONTEXTDATA")) {
        if let Some((xd, _)) = top(&o.handle.hex()) {
            by_attrib.entry(xd).or_default().push(o);
        }
    }
    let mut out = HashMap::new();
    for (xd, list) in &by_attrib {
        let Some((r, tag, nth)) = attribs.get(xd) else { continue };
        let Some(EntityKind::Insert(ins)) = d.entity(*r).map(|e| &e.kind) else { continue };
        let Some(a) = ins.attribs.iter().filter(|a| up(&a.tag) == *tag).nth(*nth) else { continue };
        let base = d.block(&ins.block).map(|b| b.base.xy()).unwrap_or_default();
        let Some(to_block_now) = ins.transform(base).inverse() else { continue };
        let Some(def) = list.iter().find(|o| is_default(o)).or(list.first()) else { continue };
        let at = |o: &ForeignObject, x: i32| Some(Vec2::new(num(o, x)?, num(o, x + 10)?));
        // What the attribute did in the block since: its default context then, the attribute now.
        let Some(def_place) = text(def, 340).and_then(|s| read.get(&(*r, s))).copied() else { continue };
        let Some(to_block_then) = matrix(ins, base, def_place).inverse() else { continue };
        let now_ins = to_block_now.apply(a.text.insert.xy());
        let now_align = to_block_now.apply(a.text.align_pt.unwrap_or(a.text.insert).xy());
        // The attribute as read (the same tag and place among its tag), in its reference as read,
        // when that was kept: its points in the block and its rotation from the reference's
        // (degrees). Else its default context in the reference's.
        let kept = f.placements.get(r).and_then(|p| {
            let k = p.attribs.iter().filter(|k| up(&k.tag) == *tag).nth(*nth)?;
            let then = matrix(ins, base, (p.insert.xy(), p.rotation.to_degrees(), (p.scale.x, p.scale.y))).inverse()?;
            Some((then.apply(k.insert.xy()), then.apply(k.align.xy()), k.rotation.to_degrees() - p.rotation.to_degrees()))
        });
        let then =
            kept.or_else(|| Some((to_block_then.apply(at(def, 10)?), to_block_then.apply(at(def, 11)?), num(def, 50).unwrap_or(0.0) - def_place.1)));
        let Some((then_ins, then_align, then_turn)) = then else { continue };
        let turn = (a.text.rotation.to_degrees() - ins.rotation.to_degrees()) - then_turn;
        for c in list {
            let Some(s) = text(c, 340) else { continue };
            let (Some(was), Some(is)) = (read.get(&(*r, s.clone())).copied(), now.get(&(*r, s)).copied()) else { continue };
            let (Some(from), to): (Option<Mat3>, Mat3) = (matrix(ins, base, was).inverse(), matrix(ins, base, is)) else { continue };
            let moved = |x: i32, delta: Vec2| at(c, x).map(|p| to.apply(from.apply(p) + delta));
            let (new_ins, new_align) = (moved(10, now_ins - then_ins), moved(11, now_align - then_align));
            let rot = num(c, 50).map(|v| v + (is.1 - was.1) + turn);
            let mut o = (*c).clone();
            let mut changed = false;
            for t in &mut o.tags {
                let v = match t.code {
                    10 => new_ins.map(|p| p.x),
                    20 => new_ins.map(|p| p.y),
                    11 => new_align.map(|p| p.x),
                    21 => new_align.map(|p| p.y),
                    50 => rot,
                    _ => None,
                };
                if let Some(v) = v.filter(|v| v.is_finite())
                    && t.value.trim().parse::<f64>().map_or(true, |old| (old - v).abs() > 1e-9)
                {
                    t.value = v.to_string();
                    changed = true;
                }
            }
            if changed {
                out.insert(o.handle.hex(), o);
            }
        }
    }
    out
}

/// Whether a kept context is its object's default one: its first 290 group (of
/// AcDbObjectContextData) is 1. An attribute context has a later 290 group of its own.
fn is_default(o: &ForeignObject) -> bool {
    o.tags.iter().find(|t| t.code == 290).is_some_and(|t| t.value.trim() == "1")
}

fn is_blkref_context(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("ACDB_BLKREFOBJECTCONTEXTDATA_CLASS")
}

fn is_attribute_context(kind: &str) -> bool {
    up(kind).contains("ATTRIBUTEOBJECTCONTEXTDATA")
}

/// Attributes' kept extension dictionaries: (reference, tag, place among the reference's
/// attributes with that tag), by dictionary handle.
fn attribute_xdicts(f: &Foreign) -> HashMap<String, (Handle, String, usize)> {
    f.records
        .iter()
        .filter(|(k, _)| k.0 == "ATTRIB")
        .filter_map(|(k, x)| {
            let (ins, tag, nth) = split_attrib_key(&k.1)?;
            Some((x.xdict?.hex(), (ins, tag.to_string(), nth)))
        })
        .collect()
}

/// The kept contexts of block references and their attributes, in line with the drawing's
/// scales ([`reconcile`]).
#[derive(Default)]
struct Reconciled {
    /// Contexts not written: their scale is gone from the drawing's scale list, or their
    /// reference is annotative in CADCraft's model and no longer supports it.
    dropped: HashSet<String>,
    /// New contexts for the scales such a reference supports without a context, with stand-in
    /// handles above every handle in use (`above`); their dictionaries list them.
    added: Vec<ForeignObject>,
    /// The SCALE objects the added contexts name (group 340): stand-in handle → the scale's role.
    scales: HashMap<String, ForeignKey>,
}

/// Bring the kept contexts of block references and their attributes (`contexts`: those already
/// refreshed, by handle) in line with the scales: a context whose SCALE is not written (deleted
/// from the drawing's list) goes. A reference CADCraft's model keeps annotative (it supports
/// scales, [`cadcraft_doc::Common::anno_scales`]) has a context for exactly the scales it
/// supports: one per new scale is made from its default context, scaled by the ratio of the two
/// scales about the reference's insertion point (as AutoCAD adds a scale). One context of each
/// reference stays the default (290 = 1). Changed dictionaries and contexts go into `contexts`.
/// New objects take stand-in handles above `above` and the kept objects' and targets' handles
/// (AutoCAD writes handles above 2^48, so no range is free by itself).
fn reconcile(
    f: &Foreign,
    d: &Drawing,
    owners: &HashMap<String, String>,
    roles: &HashMap<ForeignKey, String>,
    contexts: &mut HashMap<String, ForeignObject>,
    above: u64,
) -> Reconciled {
    use cadcraft_doc::geom::Vec2;
    let mut out = Reconciled::default();
    let num = |o: &ForeignObject, code: i32| {
        o.tags.iter().find(|t| t.code == code).and_then(|t| t.value.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    };
    // The written scale a context names (upper case), if its SCALE is written.
    let scale_of = |o: &ForeignObject| -> Option<String> {
        let v = o.tags.iter().find(|t| t.code == 340)?;
        let k = f.targets.get(&handle(&norm(&v.value))?)?;
        (k.0 == "SCALE" && roles.contains_key(k)).then(|| k.1.clone())
    };
    let set_default = |o: &mut ForeignObject, on: bool| {
        if let Some(t) = o.tags.iter_mut().find(|t| t.code == 290) {
            t.value = if on { "1" } else { "0" }.into();
        }
    };
    // The last kept object up the owner chain (an extension dictionary) and its owner.
    let top = |h: &str| {
        let mut cur = h.to_string();
        for _ in 0..MAX_DEPTH {
            match owners.get(&cur) {
                Some(o) if owners.contains_key(o) => cur = o.clone(),
                Some(o) => return Some((cur, o.clone())),
                None => return None,
            }
        }
        None
    };
    let attribs = attribute_xdicts(f);
    // The contexts (as refreshed) by their dictionary, with the reference they belong to and
    // whether they are an attribute's.
    let mut groups: BTreeMap<String, (Handle, bool, Vec<ForeignObject>)> = BTreeMap::new();
    for o in f.objects.iter().filter(|o| is_blkref_context(&o.kind) || is_attribute_context(&o.kind)) {
        let h = o.handle.hex();
        let (Some(parent), Some((xd, owner))) = (owners.get(&h), top(&h)) else { continue };
        let attribute = is_attribute_context(&o.kind);
        let reference = if attribute { attribs.get(&xd).map(|a| a.0) } else { Handle::parse_hex(&owner) };
        let Some(reference) = reference else { continue };
        groups.entry(parent.clone()).or_insert((reference, attribute, Vec::new())).2.push(contexts.get(&h).unwrap_or(o).clone());
    }
    let mut next = f.objects.iter().map(|o| o.handle.0).chain(f.targets.keys().map(|h| h.0)).fold(above, u64::max);
    let mut fresh = || {
        next = next.saturating_add(1);
        format!("{next:X}")
    };
    // Each reference's template for new contexts: (scale, insertion point) of its default
    // context, else of its first, among those whose scale is still written.
    let mut ref_default: HashMap<Handle, (String, Vec2)> = HashMap::new();
    for (r, attribute, list) in groups.values() {
        let written: Vec<&ForeignObject> = list.iter().filter(|o| scale_of(o).is_some()).collect();
        if let (false, Some(def)) = (attribute, written.iter().find(|o| is_default(o)).or(written.first()))
            && let (Some(s), Some(x), Some(y)) = (scale_of(def), num(def, 10), num(def, 20))
        {
            ref_default.insert(*r, (s, Vec2::new(x, y)));
        }
    }
    let current = up(&d.current_anno_scale().name);
    for (dict, (r, attribute, list)) in &groups {
        let e = d.entity(*r).filter(|e| matches!(e.kind, EntityKind::Insert(_)) && cadcraft_doc::is_annotative(e));
        // The scales the reference supports, when CADCraft's model says (each written once).
        let mut wanted: Vec<String> = Vec::new();
        for s in e.map(|e| e.common.anno_scales.as_slice()).unwrap_or_default() {
            let n = up(&s.name);
            if s.is_valid() && roles.contains_key(&("SCALE".to_string(), n.clone())) && !wanted.contains(&n) {
                wanted.push(n);
            }
        }
        let mut have: Vec<(String, ForeignObject)> = Vec::new();
        for c in list {
            match scale_of(c) {
                Some(s) if e.is_none() || (wanted.contains(&s) && !have.iter().any(|(x, _)| *x == s)) => have.push((s, c.clone())),
                _ => {
                    out.dropped.insert(c.handle.hex());
                }
            }
        }
        // New contexts, from the context at the reference's default scale.
        let mut added: Vec<(String, ForeignObject)> = Vec::new();
        if let Some(e) = e
            && let Some((def_scale, at)) = ref_default.get(r)
            && let Some(template) = list.iter().find(|c| scale_of(c).as_ref() == Some(def_scale))
        {
            let factor = |name: &str| {
                d.scales
                    .iter()
                    .chain(&e.common.anno_scales)
                    .find(|s| up(&s.name) == name)
                    .map(cadcraft_doc::AnnoScale::factor)
                    .filter(|f| f.is_finite() && *f > 0.0)
            };
            for s in wanted.iter().filter(|s| !have.iter().any(|(x, _)| x == *s)) {
                let (Some(to), Some(from)) = (factor(s), factor(def_scale)) else { continue };
                let k = to / from;
                let (h, scale) = (fresh(), fresh());
                let mut o = template.clone();
                o.handle = Handle::parse_hex(&h).unwrap_or(o.handle);
                for t in &mut o.tags {
                    let v = t.value.trim().parse::<f64>().ok().filter(|v| v.is_finite());
                    let new = match (t.code, v) {
                        (5, _) => Some(h.clone()),
                        (340, _) => Some(scale.clone()),
                        (41..=43, Some(v)) if !attribute => Some((v * k).to_string()),
                        // An attribute's points scale about the reference's insertion point.
                        (10 | 11, Some(v)) if *attribute => Some((at.x + (v - at.x) * k).to_string()),
                        (20 | 21, Some(v)) if *attribute => Some((at.y + (v - at.y) * k).to_string()),
                        _ => None,
                    };
                    if let Some(new) = new {
                        t.value = new;
                    }
                }
                set_default(&mut o, false);
                out.scales.insert(scale, ("SCALE".into(), s.clone()));
                added.push((s.clone(), o));
            }
        }
        // One default: the one kept, else the current scale's, else the first.
        if !have.iter().chain(&added).any(|(_, o)| is_default(o)) {
            let pick = have.iter().chain(&added).position(|(s, _)| *s == current).unwrap_or(0);
            if let Some((_, o)) = have.get_mut(pick) {
                set_default(o, true);
                contexts.insert(o.handle.hex(), o.clone());
            } else if let Some((_, o)) = added.get_mut(pick.saturating_sub(have.len())) {
                set_default(o, true);
            }
        }
        if added.is_empty() {
            continue;
        }
        // The dictionary lists the new contexts (as `*A<n>`, after its own).
        let Some(mut dobj) = contexts.get(dict).or_else(|| f.objects.iter().find(|o| o.handle.hex() == *dict)).cloned() else { continue };
        let mut n = dobj
            .tags
            .iter()
            .filter(|t| t.code == 3)
            .filter_map(|t| t.value.trim().strip_prefix("*A").and_then(|n| n.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);
        let at = dobj.tags.iter().position(|t| t.code >= 1000).unwrap_or(dobj.tags.len());
        let mut entries = Vec::new();
        for (_, o) in &added {
            n = n.saturating_add(1);
            entries.push(RawTag { code: 3, value: format!("*A{n}") });
            entries.push(RawTag { code: 350, value: o.handle.hex() });
        }
        dobj.tags.splice(at..at, entries);
        contexts.insert(dict.clone(), dobj);
        out.added.extend(added.into_iter().map(|(_, o)| o));
    }
    out
}

/// Drop an extension dictionary's modelled entries (key and handle) from its groups (`of_insert`:
/// it is a block reference's).
fn strip_entries(tags: &mut Vec<RawTag>, of_insert: bool) {
    let mut out = Vec::with_capacity(tags.len());
    let mut i = 0;
    while let Some(t) = tags.get(i) {
        if t.code == 3 && modelled_xdict_key(&t.value, of_insert) && tags.get(i + 1).is_some_and(|n| matches!(n.code, 350 | 360)) {
            i += 2;
            continue;
        }
        out.push(t.clone());
        i += 1;
    }
    *tags = out;
}

/// An entity of a type CADCraft doesn't model, as read, with its handle, owner, space and layer
/// from the drawing. Its reactors, extension dictionary and xdata are added by [`finish`] (they
/// are kept with the drawing's other foreign data); its references are remapped there too.
pub(crate) fn unknown_entity(u: &Unknown, e: &Entity, owner: &str, paper: bool) -> Vec<Tag> {
    let kind = up(&u.dxf_type);
    if STRUCTURAL.contains(&kind.as_str()) || u.dxf_type.trim() != u.dxf_type {
        return Vec::new();
    }
    let mut out = vec![Tag::s(0, u.dxf_type.clone()), Tag::s(5, e.handle.hex()), Tag::s(330, owner)];
    let fixed = out.len();
    let mut in_group = false;
    let mut owner_seen = false;
    let mut layer = false;
    let mut paper_done = !paper;
    for r in u.tags.iter().take_while(|r| r.code < 1000) {
        match r.code {
            102 => {
                in_group = r.value.trim().starts_with('{');
                continue;
            }
            _ if in_group => continue,
            5 | 67 => continue,
            330 if !owner_seen => {
                owner_seen = true;
                continue;
            }
            8 => {
                layer = true;
                out.push(Tag::s(8, e.common.layer.clone()));
                continue;
            }
            _ => {}
        }
        out.push(tag(r));
        if r.code == 100 && r.value.trim() == "AcDbEntity" && !paper_done {
            out.push(Tag::i(67, 1));
            paper_done = true;
        }
    }
    let mut lead = Vec::new();
    if !paper_done {
        lead.push(Tag::i(67, 1));
    }
    if !layer {
        lead.push(Tag::s(8, e.common.layer.clone()));
    }
    out.splice(fixed..fixed, lead);
    out
}

/// A written record: section, type, the index of its `0` group and the end of its groups.
struct Rec {
    sec: String,
    kind: String,
    start: usize,
    end: usize,
}

fn written_records(tags: &[Tag]) -> Vec<Rec> {
    let mut out: Vec<Rec> = Vec::new();
    let mut sec = String::new();
    for (kind, start, body) in split(tags) {
        if kind == "SECTION" {
            sec = body.iter().find(|t| t.code == 2).map(Tag::str).unwrap_or_default();
        }
        out.push(Rec { sec: sec.clone(), kind, start, end: start + 1 + body.len() });
        if out.last().is_some_and(|r| r.kind == "ENDSEC") {
            sec.clear();
        }
    }
    out
}

/// Remaps the kept data's handles onto a written file.
struct Map<'a> {
    f: &'a Foreign,
    /// Kept objects that are written: file handle → new handle.
    alive: HashMap<String, String>,
    /// Kept objects, written or not (file handles).
    raw: HashSet<String>,
    /// Role → handle in the written file.
    roles: HashMap<ForeignKey, String>,
    /// Entities in the written file.
    entities: HashSet<String>,
    /// Stand-in handles of the scales new contexts name ([`Reconciled::scales`]).
    scales: &'a HashMap<String, ForeignKey>,
}

impl Map<'_> {
    /// The written handle for a kept reference, `Some("0")` for the null handle, `None` when what
    /// it named is gone.
    fn get(&self, old: &str) -> Option<String> {
        let v = norm(old);
        if v.is_empty() || v == "0" {
            return Some("0".into());
        }
        if self.raw.contains(&v) {
            return self.alive.get(&v).cloned();
        }
        if let Some(role) = self.scales.get(&v) {
            return self.roles.get(role).cloned();
        }
        let h = handle(&v)?;
        if let Some(role) = self.f.targets.get(&h) {
            return self.roles.get(role).cloned();
        }
        self.entities.contains(&v).then_some(v)
    }

    /// A kept group with its handle remapped (a dangling one becomes null).
    fn group(&self, r: &RawTag) -> Tag {
        if is_pointer(r.code) { Tag::s(r.code, self.get(&r.value).unwrap_or_else(|| "0".into())) } else { tag(r) }
    }

    /// A `102 {NAME` group of the handles that still exist (nothing if none does).
    fn handle_group(&self, out: &mut Vec<Tag>, name: &str, code: i32, handles: &[String]) {
        let live: Vec<String> = handles.iter().filter_map(|h| self.get(h)).filter(|h| h != "0").collect();
        if live.is_empty() {
            return;
        }
        out.push(Tag::s(102, format!("{{{name}")));
        out.extend(live.into_iter().map(|h| Tag::s(code, h)));
        out.push(Tag::s(102, "}"));
    }

    /// A kept object's groups, remapped.
    fn object(&self, o: &ForeignObject, out: &mut Vec<Tag>) {
        out.push(Tag::s(0, o.kind.clone()));
        let dict = is_dictionary(&o.kind);
        let mut i = 0;
        while let Some(r) = o.tags.get(i) {
            i += 1;
            if r.code == 102 && r.value.trim().starts_with('{') {
                // A `{NAME` group is a handle list (`{ACAD_REACTORS 330 … }`) only when every
                // group inside it is a reference. Application data groups also use 102 brackets
                // with mixed values and their own closing form — the AcDs thumbnail schema's
                // `{ATTRRECORD 341 h 2 name 280 1 291 1 ATTRRECORD}` — and collapsing those into
                // a handle list rewrote the handle under the LAST code read (291, a bool) and
                // dropped the rest, which strict DXF readers reject. Those are written as read,
                // references remapped, by the plain path below.
                let end = (i..o.tags.len()).find(|&k| o.tags[k].code == 102).unwrap_or(o.tags.len());
                let inner = &o.tags[i..end];
                if !inner.is_empty() && !inner.iter().all(|n| is_pointer(n.code)) {
                    out.push(tag(r));
                    continue;
                }
                let name = r.value.trim().trim_start_matches('{').to_string();
                let mut hs = Vec::new();
                let mut code = 330;
                while let Some(n) = o.tags.get(i) {
                    if n.code == 102 {
                        if n.value.trim() == "}" {
                            i += 1;
                        }
                        break;
                    }
                    code = n.code;
                    hs.push(n.value.clone());
                    i += 1;
                }
                self.handle_group(out, &name, code, &hs);
                continue;
            }
            if dict && r.code == 3 && o.tags.get(i).is_some_and(|n| matches!(n.code, 350 | 360)) {
                let target = o.tags.get(i).map(|n| (n.code, self.get(&n.value)));
                i += 1;
                if let Some((code, Some(h))) = target.filter(|(_, h)| h.as_deref().is_some_and(|h| h != "0")) {
                    out.push(tag(r));
                    out.push(Tag::s(code, h));
                }
                continue;
            }
            if r.code == 5 {
                out.push(Tag::s(5, self.get(&r.value).unwrap_or_else(|| "0".into())));
                continue;
            }
            out.push(self.group(r));
        }
    }
}

/// The extras of a written record, if the drawing keeps any for it.
fn extra_for<'a>(
    f: &'a Foreign,
    xd: &XdataOf,
    attribs: &HashMap<usize, String>,
    rec: &Rec,
    body: &[Tag],
    block: &str,
) -> Option<Cow<'a, ForeignExtra>> {
    match (rec.sec.as_str(), rec.kind.as_str()) {
        ("BLOCKS", "BLOCK") => f.records.get(&("BLOCK".to_string(), block.to_string())).map(Cow::Borrowed),
        ("BLOCKS", "ENDBLK") => None,
        // An attribute by its reference and tag (its handle is new).
        ("ENTITIES" | "BLOCKS", "ATTRIB") => f.records.get(&("ATTRIB".to_string(), attribs.get(&rec.start)?.clone())).map(Cow::Borrowed),
        ("ENTITIES" | "BLOCKS", _) => entity_extra(f, xd, handle(&head(body).handle?)?),
        ("TABLES", kind) => {
            let name = up(&val(body, 2).unwrap_or_default());
            f.records.get(&(kind.to_string(), name)).map(Cow::Borrowed)
        }
        _ => None,
    }
}

/// Entities' own xdata ([`cadcraft_doc::Common::xdata`]), by handle.
type XdataOf<'a> = HashMap<Handle, &'a [RawTag]>;

/// The extras of entity `h`: what the drawing keeps for it, with the entity's own xdata.
fn entity_extra<'a>(f: &'a Foreign, xd: &XdataOf, h: Handle) -> Option<Cow<'a, ForeignExtra>> {
    let kept = f.entities.get(&h);
    match xd.get(&h) {
        None => kept.map(Cow::Borrowed),
        Some(xdata) => {
            let mut x = kept.cloned().unwrap_or_default();
            x.xdata = xdata.to_vec();
            Some(Cow::Owned(x))
        }
    }
}

/// Every Unknown entity's handle (upper case).
fn unknown_handles(d: &Drawing) -> HashSet<String> {
    all_entities(d).filter(|e| matches!(e.kind, EntityKind::Unknown(_))).map(|e| e.handle.hex()).collect()
}

/// Put the drawing's foreign data back into a written file's tags (see the module comment).
pub(crate) fn finish(tags: &mut Vec<Tag>, d: &Drawing) {
    let f = d.foreign.as_ref();
    let unknown = unknown_handles(d);
    let xd: XdataOf = all_entities(d).filter(|e| !e.common.xdata.is_empty()).map(|e| (e.handle, e.common.xdata.as_slice())).collect();
    if f.is_empty() && unknown.is_empty() && xd.is_empty() {
        return;
    }
    let recs = written_records(tags);
    let body = |r: &Rec| tags.get(r.start + 1..r.end).unwrap_or(&[]);
    let by_sec = |name: &str| -> Recs {
        recs.iter().filter(|r| r.sec == name && r.kind != "SECTION" && r.kind != "ENDSEC").map(|r| (r.kind.clone(), r.start, body(r))).collect()
    };
    let (tables, blocks, entities, objects) = (by_sec("TABLES"), by_sec("BLOCKS"), by_sec("ENTITIES"), by_sec("OBJECTS"));
    let mut roles: HashMap<ForeignKey, String> = HashMap::new();
    for (h, k) in identities(&tables, &blocks, &entities, &objects) {
        roles.entry(k).or_insert(h);
    }
    let mut written: HashSet<String> = recs.iter().filter_map(|r| head(body(r)).handle).collect();
    // Entities of types CADCraft doesn't model, placed among the written ones.
    let planned = plan_unknown(d, &recs, tags, &roles);
    for list in planned.values() {
        for (_, t) in list {
            written.extend(head(t.get(1..).unwrap_or(&[])).handle);
        }
    }
    let root = objects.first().filter(|(k, _, _)| is_dictionary(k)).map(|(_, s, _)| *s);
    let root_keys: HashSet<String> =
        objects.iter().find(|(_, s, _)| Some(*s) == root).map(|(_, _, t)| entries(t).into_iter().map(|(k, _)| up(&k)).collect()).unwrap_or_default();
    // The written attributes' identities, by record.
    let attrib_recs: Vec<&Rec> = recs.iter().filter(|r| matches!(r.sec.as_str(), "ENTITIES" | "BLOCKS") && r.kind == "ATTRIB").collect();
    let attribs: HashMap<usize, String> = attrib_recs.iter().map(|r| r.start).zip(attrib_keys(attrib_recs.iter().map(|r| body(r)))).collect();
    // Extension dictionaries that go on a written record (one without its own).
    let mut block = String::new();
    let mut attached: HashSet<String> = HashSet::new();
    for r in &recs {
        if r.kind == "BLOCK" {
            block = up(&val(body(r), 2).unwrap_or_default());
        }
        if let Some(x) = extra_for(f, &xd, &attribs, r, body(r), &block)
            && let Some(xd) = x.xdict
            && head(body(r)).xdict.is_none()
        {
            attached.insert(xd.hex());
        }
    }
    for (_, t) in planned.values().flatten() {
        let x = head(t.get(1..).unwrap_or(&[])).handle.and_then(|h| handle(&h)).and_then(|h| entity_extra(f, &xd, h));
        attached.extend(x.and_then(|x| x.xdict).map(Handle::hex));
    }
    let dict_entries: HashMap<String, &str> = f.dictionary.iter().map(|(k, h)| (h.hex(), k.as_str())).collect();
    let nod = f.targets.iter().filter(|(_, k)| k.0 == "NOD" && k.1.is_empty()).map(|(h, _)| h.hex()).collect::<HashSet<_>>();
    let mut owners: HashMap<String, String> = f
        .objects
        .iter()
        .filter_map(|o| Some((o.handle.hex(), head(&o.tags.iter().take_while(|t| t.code < 1000).map(tag).collect::<Vec<_>>()).owner?)))
        .collect();
    // Block references' annotation scale contexts, up to date with the references.
    let mut contexts = block_contexts(f, d, &owners);
    let attribute = attribute_contexts(f, d, &owners, &contexts);
    contexts.extend(attribute);
    // In line with the drawing's scales and each reference's.
    let rec = reconcile(f, d, &owners, &roles, &mut contexts, seed(tags).max(max_handle(tags)));
    let all: Vec<&ForeignObject> = f.objects.iter().chain(&rec.added).collect();
    for o in &rec.added {
        if let Some(parent) = o.tags.iter().find(|t| t.code == 330) {
            owners.insert(o.handle.hex(), norm(&parent.value));
        }
    }
    // Which kept objects are written: those whose owner chain ends at a written owner.
    let objs: HashMap<String, &ForeignObject> = all.iter().map(|o| (o.handle.hex(), *o)).collect();
    let mut alive_memo: HashMap<String, bool> = HashMap::new();
    for o in &all {
        let mut path: Vec<String> = Vec::new();
        let mut cur = o.handle.hex();
        let verdict = loop {
            if let Some(&v) = alive_memo.get(&cur) {
                break v;
            }
            if path.contains(&cur) || path.len() >= MAX_DEPTH {
                break false;
            }
            path.push(cur.clone());
            let Some(owner) = owners.get(&cur).cloned() else { break false };
            if objs.contains_key(&owner) {
                cur = owner;
                continue;
            }
            if nod.contains(&owner) {
                break dict_entries.get(&cur).is_some_and(|k| !root_keys.contains(&up(k))) && root.is_some();
            }
            if attached.contains(&cur) {
                break true;
            }
            break unknown.contains(&owner) && written.contains(&owner);
        };
        for p in path {
            alive_memo.insert(p, verdict);
        }
    }
    for h in &rec.dropped {
        alive_memo.insert(h.clone(), false);
    }
    // New handles for the written kept objects, above every written handle and the seed.
    let mut next = seed(tags).max(max_handle(tags).saturating_add(1));
    let mut alive: HashMap<String, String> = HashMap::new();
    for o in all.iter().filter(|o| alive_memo.get(&o.handle.hex()) == Some(&true)) {
        alive.insert(o.handle.hex(), format!("{next:X}"));
        next = next.saturating_add(1);
    }
    let entities: HashSet<String> = all_entities(d).map(|e| e.handle.hex()).filter(|h| written.contains(h)).collect();
    let map = Map { f, alive, raw: objs.keys().cloned().collect(), roles, entities, scales: &rec.scales };
    let class_names: HashSet<String> = recs.iter().filter(|r| r.kind == "CLASS").filter_map(|r| val(body(r), 1)).map(|n| up(&n)).collect();
    let mut out: Vec<Tag> = Vec::with_capacity(tags.len() + f.objects.iter().map(|o| o.tags.len() + 1).sum::<usize>());
    let mut block = String::new();
    for r in &recs {
        let b = body(r);
        if r.kind == "BLOCK" {
            block = up(&val(b, 2).unwrap_or_default());
        }
        if r.kind == "ENDSEC" && r.sec == "CLASSES" {
            for c in f.classes.iter().filter(|c| c.iter().find(|t| t.code == 1).is_some_and(|n| !class_names.contains(&up(&n.value)))) {
                out.push(Tag::s(0, "CLASS"));
                out.extend(c.iter().map(tag));
            }
        }
        if r.kind == "ENDSEC" && r.sec == "OBJECTS" {
            for o in all.iter().filter(|o| map.alive.contains_key(&o.handle.hex())) {
                map.object(contexts.get(&o.handle.hex()).unwrap_or(*o), &mut out);
            }
        }
        for (sec, t) in planned.get(&r.start).into_iter().flatten() {
            let (Some(zero), Some(ub)) = (t.first(), t.get(1..)) else { continue };
            let ur = Rec { sec: sec.clone(), kind: zero.str(), start: 0, end: 0 };
            let x = head(ub).handle.and_then(|h| handle(&h)).and_then(|h| entity_extra(f, &xd, h));
            out.push(zero.clone());
            record(&map, &ur, ub, x.as_deref(), true, false, &mut out);
        }
        out.extend(tags.get(r.start).cloned());
        let x = extra_for(f, &xd, &attribs, r, b, &block);
        if x.is_none() && Some(r.start) != root {
            out.extend(b.iter().cloned());
            continue;
        }
        record(&map, r, b, x.as_deref(), false, Some(r.start) == root, &mut out);
    }
    add_appids(&mut out);
    let top = max_handle(&out).saturating_add(1);
    if let Some(t) = seed_index(&out).and_then(|i| out.get_mut(i))
        && Handle::parse_hex(&t.str()).is_none_or(|h| h.0 < top)
    {
        *t = Tag::s(5, format!("{top:X}"));
    }
    *tags = out;
}

/// Where the entities of types CADCraft doesn't model go: each before the next written entity of
/// its space or block (else at the end of it), as `(section, record)` lists by the index of the
/// record they precede. Owners come from the written block records.
fn plan_unknown(d: &Drawing, recs: &[Rec], tags: &[Tag], roles: &HashMap<ForeignKey, String>) -> HashMap<usize, Vec<(String, Vec<Tag>)>> {
    let mut out: HashMap<usize, Vec<(String, Vec<Tag>)>> = HashMap::new();
    let body = |r: &Rec| tags.get(r.start + 1..r.end).unwrap_or(&[]);
    let mut at: HashMap<String, usize> = HashMap::new();
    for r in recs.iter().filter(|r| matches!(r.sec.as_str(), "ENTITIES" | "BLOCKS") && !matches!(r.kind.as_str(), "BLOCK" | "ENDBLK")) {
        if let Some(h) = head(body(r)).handle {
            at.entry(h).or_insert(r.start);
        }
    }
    let ents_end = recs.iter().find(|r| r.sec == "ENTITIES" && r.kind == "ENDSEC").map(|r| r.start);
    let endblk = |name: &str| {
        let i = recs.iter().position(|r| r.sec == "BLOCKS" && r.kind == "BLOCK" && val(body(r), 2).is_some_and(|n| up(&n) == up(name)))?;
        recs.iter().skip(i).find(|r| r.kind == "ENDBLK").map(|r| r.start)
    };
    let br = |name: &str| roles.get(&("BLOCK_RECORD".to_string(), up(name))).cloned();
    let first_at = |s: &cadcraft_doc::EntityStore| s.iter().find_map(|e| at.get(&e.handle.hex()).copied());
    let mut layouts: Vec<&cadcraft_doc::Layout> = d.layouts.iter().collect();
    layouts.sort_by_key(|l| l.tab_order);
    // (entities, owner, paper space, end of the space, section)
    let mut stores: Vec<(&cadcraft_doc::EntityStore, Option<String>, bool, Option<usize>, &str)> = Vec::new();
    let model_end = layouts.first().and_then(|l| first_at(&l.entities)).or(ents_end);
    stores.push((&d.model, br("*Model_Space"), false, model_end, "ENTITIES"));
    for (i, l) in layouts.iter().enumerate() {
        if i == 0 {
            stores.push((&l.entities, br("*Paper_Space"), true, ents_end, "ENTITIES"));
        } else {
            let name = format!("*Paper_Space{}", i - 1);
            stores.push((&l.entities, br(&name), true, endblk(&name), "BLOCKS"));
        }
    }
    for b in d.blocks.values() {
        stores.push((&b.entities, br(&b.name), false, endblk(&b.name), "BLOCKS"));
    }
    for (store, owner, paper, end, sec) in stores {
        let (Some(owner), Some(end)) = (owner, end) else { continue };
        let mut pending: Vec<(String, Vec<Tag>)> = Vec::new();
        for e in store.iter() {
            match &e.kind {
                EntityKind::Unknown(u) => {
                    let t = unknown_entity(u, e, &owner, paper);
                    if !t.is_empty() {
                        pending.push((sec.to_string(), t));
                    }
                }
                _ => {
                    if let Some(&p) = at.get(&e.handle.hex())
                        && !pending.is_empty()
                    {
                        out.entry(p).or_default().append(&mut pending);
                    }
                }
            }
        }
        if !pending.is_empty() {
            out.entry(end).or_default().append(&mut pending);
        }
    }
    out
}

/// A written record with its kept extras, remapped references (unknown entities) and kept
/// dictionary entries (the named object dictionary).
fn record(map: &Map, r: &Rec, b: &[Tag], x: Option<&ForeignExtra>, unknown: bool, root: bool, out: &mut Vec<Tag>) {
    let h = head(b);
    let entity = matches!(r.sec.as_str(), "ENTITIES" | "BLOCKS") && r.kind != "BLOCK";
    let xdata_at = b.iter().position(|t| t.code == 1001).unwrap_or(b.len());
    let own_apps: HashSet<String> = b.iter().filter(|t| t.code == 1001).map(|t| up(&t.str())).collect();
    // Where unmodelled properties go: the end of AcDbEntity, else before the xdata.
    let props_at = if entity {
        b.iter().position(|t| t.code == 100 && t.str().trim() == "AcDbEntity").and_then(|s| {
            let rest = b.get(s + 1..xdata_at)?;
            Some(s + 1 + rest.iter().position(|t| t.code == 100).unwrap_or(rest.len()))
        })
    } else {
        None
    }
    .unwrap_or(xdata_at);
    let rgb = b.iter().take(xdata_at).find(|t| t.code == 420).map(Tag::str);
    let has_book = b.iter().take(xdata_at).any(|t| t.code == 430);
    let handle_at = b.iter().position(|t| matches!(t.code, 5 | 105));
    let mut seen_owner = false;
    for (i, t) in b.iter().enumerate() {
        if i == props_at
            && let Some(x) = x
        {
            if let Some((want, book)) = &x.color_book
                && !has_book
                && rgb.as_deref() == Some(want.as_str())
            {
                out.push(Tag::s(430, book.clone()));
            }
            out.extend(x.props.iter().map(|p| map.group(p)));
        }
        if i == xdata_at
            && let Some(x) = x
        {
            xdata(map, x, &own_apps, out);
        }
        let mut t = t.clone();
        if unknown && i < xdata_at && is_pointer(t.code) && t.code != 102 {
            if t.code == 330 && !seen_owner {
                seen_owner = true;
            } else {
                t = Tag::s(t.code, map.get(&t.str()).unwrap_or_else(|| "0".into()));
            }
        }
        out.push(t);
        if Some(i) == handle_at
            && let Some(x) = x
        {
            if !h.has_reactors {
                let rs: Vec<String> = x.reactors.iter().map(|r| r.hex()).collect();
                map.handle_group(out, "ACAD_REACTORS", 330, &rs);
            }
            if h.xdict.is_none()
                && let Some(xd) = x.xdict
            {
                map.handle_group(out, "ACAD_XDICTIONARY", 360, &[xd.hex()]);
            }
        }
    }
    if let Some(x) = x {
        if props_at == b.len() {
            if let Some((want, book)) = &x.color_book
                && !has_book
                && rgb.as_deref() == Some(want.as_str())
            {
                out.push(Tag::s(430, book.clone()));
            }
            out.extend(x.props.iter().map(|p| map.group(p)));
        }
        if xdata_at == b.len() {
            xdata(map, x, &own_apps, out);
        }
    }
    if root {
        for (key, h) in &map.f.dictionary {
            if let Some(nh) = map.alive.get(&h.hex())
                && !b.iter().any(|t| t.code == 3 && up(&t.str()) == up(key))
            {
                out.push(Tag::s(3, key.clone()));
                out.push(Tag::s(350, nh.clone()));
            }
        }
    }
}

/// Kept xdata of applications the record doesn't already carry, handles remapped.
fn xdata(map: &Map, x: &ForeignExtra, own: &HashSet<String>, out: &mut Vec<Tag>) {
    let mut skip = false;
    for r in &x.xdata {
        if r.code == 1001 {
            skip = own.contains(&up(&r.value));
        }
        if !skip {
            out.push(map.group(r));
        }
    }
}

/// APPID records for every application with xdata in the file, added to the APPID table.
fn add_appids(out: &mut Vec<Tag>) {
    let used: BTreeMap<String, String> = out.iter().filter(|t| t.code == 1001).map(|t| (up(&t.str()), t.str().trim().to_string())).collect();
    let recs = written_records(out);
    let Some(table) = recs.iter().position(|r| {
        r.sec == "TABLES" && r.kind == "TABLE" && val(out.get(r.start + 1..r.end).unwrap_or(&[]), 2).is_some_and(|n| up(&n) == "APPID")
    }) else {
        return;
    };
    let Some(t) = recs.get(table) else { return };
    let table_h = head(out.get(t.start + 1..t.end).unwrap_or(&[])).handle.unwrap_or_else(|| "0".into());
    let Some(end) = recs.iter().skip(table).find(|r| r.kind == "ENDTAB") else { return };
    let have: HashSet<String> = recs
        .iter()
        .skip(table + 1)
        .take_while(|r| r.kind == "APPID")
        .filter_map(|r| val(out.get(r.start + 1..r.end).unwrap_or(&[]), 2))
        .map(|n| up(&n))
        .collect();
    let missing: Vec<&String> = used.iter().filter(|(k, _)| !have.contains(*k)).map(|(_, v)| v).collect();
    if missing.is_empty() {
        return;
    }
    // New handles above the seed, which is then moved past them.
    let mut next = seed(out).max(max_handle(out).saturating_add(1));
    let mut add = Vec::new();
    for app in &missing {
        add.extend([
            Tag::s(0, "APPID"),
            Tag::s(5, format!("{next:X}")),
            Tag::s(330, table_h.clone()),
            Tag::s(100, "AcDbSymbolTableRecord"),
            Tag::s(100, "AcDbRegAppTableRecord"),
            Tag::s(2, (*app).clone()),
            Tag::i(70, 0),
        ]);
        next = next.saturating_add(1);
    }
    let end_at = end.start;
    if let Some(count) = out.get_mut(t.start + 1..t.end).and_then(|b| b.iter_mut().find(|x| x.code == 70)) {
        *count = Tag::i(70, count.i64().saturating_add(i64::try_from(missing.len()).unwrap_or(0)));
    }
    out.splice(end_at..end_at, add);
}

/// The index of the `$HANDSEED` value in the header.
fn seed_index(tags: &[Tag]) -> Option<usize> {
    tags.windows(2).position(|w| matches!(w, [a, b] if a.code == 9 && a.str() == "$HANDSEED" && b.code == 5)).map(|i| i + 1)
}

/// The header's next free handle.
fn seed(tags: &[Tag]) -> u64 {
    seed_index(tags).and_then(|i| tags.get(i)).and_then(|t| Handle::parse_hex(&t.str())).map_or(0, |h| h.0)
}

/// The largest handle of a record (groups 5 and 105).
fn max_handle(tags: &[Tag]) -> u64 {
    tags.iter().filter(|t| t.code == 5 || t.code == 105).filter_map(|t| Handle::parse_hex(&t.str())).map(|h| h.0).max().unwrap_or(0)
}
