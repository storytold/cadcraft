//! Fields in MTEXT, stored the way AutoCAD stores them (#551).
//!
//! CADCraft keeps a field as its code inside the text (`%<\AcVar Date \f "yyyy">%`, see
//! [`cadcraft_doc::entity::fields`]). A DXF file stores it as objects:
//!
//! * the MTEXT's contents (group 1) hold the text as shown, each field as its value;
//! * the MTEXT's extension dictionary has an `ACAD_FIELD` entry: a dictionary with the key `TEXT`
//!   naming a `_text` FIELD object;
//! * the `_text` field's code (group 2) is the text with each field as `%<\_FldIdx n>%`, and
//!   its children (groups 360) are one FIELD per field code, in that order; a field nested in a
//!   field (an operand of a formula) is a child of that field in the same way;
//! * the named object dictionary has `ACAD_FIELDLIST` naming a FIELDLIST object with the handle
//!   of every FIELD object.
//!
//! Reading ([`absorb`]) rebuilds the code text from the objects, for the fields CADCraft knows
//! (`AcVar` and `AcExpr`); the objects themselves stay in [`Drawing::foreign`] and are written back
//! as they were while the text is unchanged (they hold more than CADCraft models, such as the
//! time of a date). Writing ([`plan`]) makes the objects of a text that has no (or changed)
//! objects, and [`fix_list`] keeps the FIELDLIST in step with the FIELD objects written.
//! Fields of other kinds (an object's property, a sheet set field) stay as code in the text.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::fields::{self, split_value, with_value};
use cadcraft_doc::foreign::{Foreign, ForeignObject};
use cadcraft_doc::{Drawing, Entity, EntityKind, Handle, RawTag};
use craft_dxf::Tag;

/// Upper bound on FIELD objects of one text.
const MAX_NODES: usize = 1_000;
/// Upper bound on nesting of fields in fields.
const MAX_DEPTH: usize = 8;
/// Longest text template written to a FIELD's group 2 (DXF strings end near 2 KB).
const MAX_TEMPLATE: usize = 1_900;
/// The data keys of a field object that CADCraft reads past: a variable's name and the checksum
/// of the text.
const KNOWN_KEYS: &[&str] = &["Variable", "ACFD_FIELDTEXT_CHECKSUM"];

fn rt(code: i32, value: impl Into<String>) -> RawTag {
    RawTag { code, value: value.into() }
}

fn every_entity(d: &Drawing) -> impl Iterator<Item = &Entity> {
    std::iter::once(&d.model)
        .chain(d.layouts.iter().map(|l| &l.entities))
        .chain(d.blocks.values().map(|b| &b.entities))
        .flat_map(|s| s.iter().map(|e| e.as_ref()))
}

fn index_of(inner: &str) -> Option<usize> {
    inner.trim().strip_prefix("\\_FldIdx")?.trim().parse().ok()
}

// ----------------------------------------------------------------------------------------------
// Reading
// ----------------------------------------------------------------------------------------------

/// A field object's parts.
struct Parsed<'a> {
    evaluator: &'a str,
    code: &'a str,
    children: Vec<Handle>,
    /// The cached value, when the field has one (an empty value is a value).
    value: Option<&'a str>,
}

fn parse_object(o: &ForeignObject) -> Option<Parsed<'_>> {
    if !o.kind.eq_ignore_ascii_case("FIELD") || !o.tags.iter().any(|t| t.code == 100 && t.value.trim() == "AcDbField") {
        return None;
    }
    let evaluator = o.tags.iter().find(|t| t.code == 1)?.value.as_str();
    let code_at = o.tags.iter().position(|t| t.code == 2)?;
    let code = o.tags.get(code_at)?.value.as_str();
    // The child count (90), then that many child handles (360).
    let count_at = code_at + 1;
    let count = usize::try_from(o.tags.get(count_at).filter(|t| t.code == 90)?.value.trim().parse::<i64>().ok()?).ok()?;
    let children: Vec<Handle> =
        o.tags.get(count_at + 1..)?.iter().take_while(|t| t.code == 360).filter_map(|t| Handle::parse_hex(t.value.trim())).collect();
    if children.len() != count || count > MAX_NODES {
        return None;
    }
    // Data keys (group 6) beyond the ones read are data CADCraft cannot write back.
    if o.tags.iter().filter(|t| t.code == 6).any(|t| !KNOWN_KEYS.contains(&t.value.trim())) {
        return None;
    }
    // The value record (group 7) says whether there is a value: type 4 or 2 (flag 93 = 4), not 3.
    let has_value = o
        .tags
        .iter()
        .position(|t| t.code == 7)
        .and_then(|i| o.tags.get(i + 1..)?.iter().find(|t| t.code == 93))
        .is_some_and(|t| t.value.trim() == "4");
    let value = o.tags.iter().rev().find(|t| t.code == 301).map(|t| t.value.as_str()).filter(|_| has_value);
    Some(Parsed { evaluator, code, children, value })
}

/// The code (between `%<` and `>%`, with the cached values) of a FIELD object and the objects
/// below it; for the `_text` field the text.
fn rebuild(objs: &HashMap<Handle, &ForeignObject>, h: Handle, depth: usize, seen: &mut HashSet<Handle>) -> Option<String> {
    if depth > MAX_DEPTH || !seen.insert(h) {
        return None;
    }
    let p = parse_object(objs.get(&h)?)?;
    let text = p.evaluator == "_text";
    if !text && !matches!(p.evaluator, "AcVar" | "AcExpr") {
        return None;
    }
    let template = crate::dxf_read::mtext_dxf::decode_contents(p.code);
    let mut out = String::with_capacity(template.len());
    let mut last = 0;
    let mut used = 0;
    for sp in fields::find(&template) {
        let inner = template.get(sp.range.start + 2..sp.range.end.checked_sub(2)?)?;
        let Some(n) = index_of(inner) else { continue };
        let child = rebuild(objs, *p.children.get(n)?, depth + 1, seen)?;
        out.push_str(template.get(last..sp.range.start)?);
        out.push_str("%<");
        out.push_str(&child);
        out.push_str(">%");
        last = sp.range.end;
        used += 1;
    }
    if used != p.children.len() {
        return None;
    }
    out.push_str(template.get(last..)?);
    Some(match p.value {
        Some(v) if !text => with_value(&out, v),
        _ => out,
    })
}

/// The entries (key, object) of a dictionary object.
fn dictionary_entries(o: &ForeignObject) -> Vec<(String, Handle)> {
    let mut key: Option<&str> = None;
    let mut out = Vec::new();
    for t in o.tags.iter().take_while(|t| t.code < 1000) {
        match t.code {
            3 => key = Some(t.value.as_str()),
            350 | 360 => {
                if let (Some(k), Some(h)) = (key.take(), Handle::parse_hex(t.value.trim())) {
                    out.push((k.to_string(), h));
                }
            }
            _ => {}
        }
    }
    out
}

/// The `_text` field of an MTEXT's extension dictionary: (`ACAD_FIELD` dictionary, field).
fn text_field(objs: &HashMap<Handle, &ForeignObject>, xdict: Handle) -> Option<(Handle, Handle)> {
    let named = |o: &ForeignObject, key: &str| dictionary_entries(o).into_iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, h)| h);
    let acad_field = named(objs.get(&xdict)?, "ACAD_FIELD")?;
    let text = named(objs.get(&acad_field)?, "TEXT")?;
    Some((acad_field, text))
}

/// Give each MTEXT whose field objects CADCraft knows the code text they make.
pub(crate) fn absorb(d: &mut Drawing) {
    let f = &d.foreign;
    if f.entities.is_empty() {
        return;
    }
    let objs: HashMap<Handle, &ForeignObject> = f.objects.iter().map(|o| (o.handle, o)).collect();
    let mut found: Vec<(Handle, String)> = Vec::new();
    for e in every_entity(d) {
        let EntityKind::MText(t) = &e.kind else { continue };
        let Some(xdict) = f.entities.get(&e.handle).and_then(|x| x.xdict) else { continue };
        let Some((_, text)) = text_field(&objs, xdict) else { continue };
        let Some(code) = rebuild(&objs, text, 0, &mut HashSet::new()) else { continue };
        // The objects must describe the text the MTEXT shows.
        if fields::has_fields(&code) && fields::display_mtext(&code) == t.contents {
            found.push((e.handle, code));
        }
    }
    if found.is_empty() {
        return;
    }
    let by: BTreeMap<Handle, String> = found.into_iter().collect();
    let give = |store: &mut cadcraft_doc::EntityStore| {
        for (h, code) in &by {
            store.modify(*h, |e| {
                if let EntityKind::MText(t) = &mut e.kind {
                    t.contents = code.clone();
                }
            });
        }
    };
    give(&mut d.model);
    for l in &mut d.layouts {
        give(&mut l.entities);
    }
    for b in d.blocks.values_mut() {
        give(&mut Arc::make_mut(b).entities);
    }
    Arc::make_mut(&mut d.foreign).fields.extend(by);
}

// ----------------------------------------------------------------------------------------------
// Writing
// ----------------------------------------------------------------------------------------------

/// One field of a text, with the fields nested in its code.
struct Node {
    evaluator: &'static str,
    /// The code with each nested field as `%<\_FldIdx n>%` and without its cached value.
    code: String,
    variable: Option<String>,
    format: Option<String>,
    value: Option<String>,
    children: Vec<Node>,
}

/// The text with each field as `%<\_FldIdx n>%`, and the fields. `None` for a text without
/// fields or with a field CADCraft does not write as objects.
fn tree(contents: &str) -> Option<(String, Vec<Node>)> {
    if !fields::has_fields(contents) {
        return None;
    }
    let mut budget = MAX_NODES;
    let (template, nodes) = split(contents, 0, &mut budget)?;
    (fits(&template) && nodes.iter().all(Node::fits)).then_some((template, nodes))
}

/// Whether a string fits a DXF group: non-ASCII characters are written as `\U+XXXX`.
fn fits(s: &str) -> bool {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 7 }).sum::<usize>() <= MAX_TEMPLATE
}

impl Node {
    fn fits(&self) -> bool {
        fits(&self.code)
            && self.variable.as_deref().is_none_or(fits)
            && self.format.as_deref().is_none_or(fits)
            && self.value.as_deref().is_none_or(fits)
            && self.children.iter().all(Node::fits)
    }
}

/// `text` with its outermost fields replaced by indexes, and those fields as nodes.
fn split(text: &str, depth: usize, budget: &mut usize) -> Option<(String, Vec<Node>)> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut nodes = Vec::new();
    let mut last = 0;
    for sp in fields::find(text) {
        *budget = budget.checked_sub(1)?;
        out.push_str(text.get(last..sp.range.start)?);
        out.push_str(&format!("%<\\_FldIdx {}>%", nodes.len()));
        nodes.push(node(text.get(sp.range.start + 2..sp.range.end.checked_sub(2)?)?, depth, budget)?);
        last = sp.range.end;
    }
    out.push_str(text.get(last..)?);
    Some((out, nodes))
}

fn node(inner: &str, depth: usize, budget: &mut usize) -> Option<Node> {
    let (code, value) = split_value(inner);
    let word: String = code.trim_start().strip_prefix('\\')?.chars().take_while(char::is_ascii_alphanumeric).collect();
    let evaluator = match word.as_str() {
        "AcVar" => "AcVar",
        "AcExpr" => "AcExpr",
        _ => return None,
    };
    let (code, children) = split(&code, depth + 1, budget)?;
    let variable = (evaluator == "AcVar")
        .then(|| code.trim_start().strip_prefix("\\AcVar").unwrap_or("").trim_start().split([' ', '\\']).next().unwrap_or("").to_string())
        .filter(|v| !v.is_empty());
    if evaluator == "AcVar" && variable.is_none() {
        return None;
    }
    let format = fields::parse(&code).format;
    Some(Node { evaluator, code, variable, format, value, children })
}

/// Hands out handles for the objects made.
struct Alloc(u64);

impl Alloc {
    fn next(&mut self) -> Handle {
        self.0 = self.0.saturating_add(1);
        Handle(self.0)
    }
}

/// A DICTIONARY object with one entry; `reactor` adds the persistent reactor AutoCAD writes on
/// the dictionaries below an extension dictionary.
fn dictionary(h: Handle, owner: Handle, reactor: bool, key: &str, entry: Handle) -> ForeignObject {
    let mut tags = vec![rt(5, h.hex())];
    if reactor {
        tags.extend([rt(102, "{ACAD_REACTORS"), rt(330, owner.hex()), rt(102, "}")]);
    }
    tags.extend([rt(330, owner.hex()), rt(100, "AcDbDictionary"), rt(280, "1"), rt(281, "1"), rt(3, key), rt(360, entry.hex())]);
    ForeignObject { handle: h, kind: "DICTIONARY".into(), tags }
}

/// The FIELD object of a field, then the objects of its nested fields.
fn field_objects(n: &Node, h: Handle, owner: Handle, ids: &mut Alloc, out: &mut Vec<ForeignObject>) {
    let handles: Vec<Handle> = n.children.iter().map(|_| ids.next()).collect();
    let mut t = vec![
        rt(5, h.hex()),
        rt(330, owner.hex()),
        rt(100, "AcDbField"),
        rt(1, n.evaluator),
        rt(2, crate::dxf_read::mtext_dxf::encode_contents(&n.code)),
    ];
    t.push(rt(90, handles.len().to_string()));
    t.extend(handles.iter().map(|c| rt(360, c.hex())));
    let date = n.variable.as_deref().is_some_and(|v| v.eq_ignore_ascii_case("Date"));
    t.extend([rt(97, "0"), rt(91, if date { "32" } else { "63" }), rt(92, "0"), rt(94, "59"), rt(95, "2"), rt(96, "0"), rt(300, "")]);
    match &n.variable {
        // The variable's name is the field's one data entry.
        Some(v) => t.extend([
            rt(93, "1"),
            rt(6, "Variable"),
            rt(93, "2"),
            rt(90, "4"),
            rt(1, v.as_str()),
            rt(94, "0"),
            rt(300, ""),
            rt(302, ""),
            rt(304, "ACVALUE_END"),
        ]),
        None => t.push(rt(93, "0")),
    }
    t.push(rt(7, "ACFD_FIELD_VALUE"));
    value_record(&mut t, n);
    out.push(ForeignObject { handle: h, kind: "FIELD".into(), tags: t });
    for (c, ch) in n.children.iter().zip(handles) {
        field_objects(c, ch, h, ids, out);
    }
}

/// The cached value of a field (type 4 text, 2 real), its format and its text.
fn value_record(t: &mut Vec<RawTag>, n: &Node) {
    let shown = n.value.clone().unwrap_or_default();
    match n.value.as_deref() {
        None => t.extend([rt(93, "3"), rt(90, "0")]),
        Some(v) => match (n.evaluator, v.trim().parse::<f64>().ok().filter(|x| x.is_finite())) {
            ("AcExpr", Some(x)) => t.extend([rt(93, "4"), rt(90, "2"), rt(140, craft_dxf::format_real(x))]),
            _ => t.extend([rt(93, "4"), rt(90, "4"), rt(1, v)]),
        },
    }
    t.extend([rt(94, "0"), rt(300, n.format.clone().unwrap_or_default()), rt(302, shown.as_str()), rt(304, "ACVALUE_END")]);
    t.extend([rt(301, shown.as_str()), rt(98, shown.chars().count().to_string())]);
}

/// The objects below an MTEXT's extension dictionary `xdict`: the `ACAD_FIELD` dictionary (key
/// `TEXT`), the `_text` field and the fields below it. The `ACAD_FIELD` dictionary comes first.
fn text_objects(template: &str, nodes: &[Node], xdict: Handle, ids: &mut Alloc) -> Vec<ForeignObject> {
    let (acad_field, text) = (ids.next(), ids.next());
    let mut out = vec![dictionary(acad_field, xdict, true, "TEXT", text)];
    let handles: Vec<Handle> = nodes.iter().map(|_| ids.next()).collect();
    let mut t = vec![
        rt(5, text.hex()),
        rt(102, "{ACAD_REACTORS"),
        rt(330, acad_field.hex()),
        rt(102, "}"),
        rt(330, acad_field.hex()),
        rt(100, "AcDbField"),
        rt(1, "_text"),
        rt(2, crate::dxf_read::mtext_dxf::encode_contents(template)),
        rt(90, handles.len().to_string()),
    ];
    t.extend(handles.iter().map(|c| rt(360, c.hex())));
    // No data entries (AutoCAD also writes the checksum of the text, which is not reproduced).
    t.extend([rt(97, "0"), rt(91, "63"), rt(92, "0"), rt(94, "13"), rt(95, "2"), rt(96, "0"), rt(300, ""), rt(93, "0")]);
    t.extend([rt(7, "ACFD_FIELD_VALUE"), rt(93, "3"), rt(90, "0"), rt(94, "0"), rt(300, ""), rt(302, ""), rt(304, "ACVALUE_END")]);
    t.extend([rt(301, ""), rt(98, "0")]);
    out.push(ForeignObject { handle: text, kind: "FIELD".into(), tags: t });
    for (n, h) in nodes.iter().zip(handles) {
        field_objects(n, h, text, ids, &mut out);
    }
    out
}

/// What saving needs for the fields of a drawing.
#[derive(Default)]
pub(crate) struct Plan {
    /// MTEXTs written with their text as shown (their field objects are written).
    pub(crate) fields: HashSet<Handle>,
    /// The drawing's foreign data with the field objects made or dropped, when they changed.
    pub(crate) foreign: Option<Foreign>,
}

/// The objects under `handle`'s extension dictionary entry `ACAD_FIELD`, taken out of `f`: the
/// entry goes from the extension dictionary, which goes too when it is empty and `keep_empty`
/// is not set.
fn drop_objects(f: &mut Foreign, handle: Handle, keep_empty: bool) {
    let Some(xdict) = f.entities.get(&handle).and_then(|x| x.xdict) else { return };
    let Some(root) = f.objects.iter().find(|o| o.handle == xdict) else { return };
    let Some(top) = dictionary_entries(root).into_iter().find(|(k, _)| k.eq_ignore_ascii_case("ACAD_FIELD")).map(|(_, h)| h) else { return };
    // Everything below the ACAD_FIELD dictionary (dictionaries and fields).
    let mut gone: HashSet<Handle> = HashSet::new();
    let mut todo = vec![top];
    while let Some(h) = todo.pop() {
        if !gone.insert(h) || gone.len() > 4 * MAX_NODES {
            continue;
        }
        // Only the entries of a dictionary and the children of a field belong to the tree.
        if let Some(o) = f.objects.iter().find(|o| o.handle == h) {
            match o.kind.as_str() {
                "DICTIONARY" => todo.extend(dictionary_entries(o).into_iter().map(|(_, h)| h)),
                "FIELD" => todo.extend(parse_object(o).map(|p| p.children).unwrap_or_default()),
                _ => {}
            }
        }
    }
    f.objects.retain(|o| !gone.contains(&o.handle) || !matches!(o.kind.as_str(), "DICTIONARY" | "FIELD"));
    if let Some(o) = f.objects.iter_mut().find(|o| o.handle == xdict) {
        // The entry's two groups: key (3) and object (360).
        let at = o.tags.iter().position(|t| t.code == 3 && t.value.eq_ignore_ascii_case("ACAD_FIELD"));
        if let Some(i) = at {
            o.tags.drain(i..(i + 2).min(o.tags.len()));
        }
        if !keep_empty && !o.tags.iter().any(|t| t.code == 3) {
            f.objects.retain(|o| o.handle != xdict);
            if let Some(x) = f.entities.get_mut(&handle) {
                x.xdict = None;
                if x.is_empty() {
                    f.entities.remove(&handle);
                }
            }
        }
    }
}

/// The DXF version whose FIELD groups [`plan`] writes (AutoCAD 2010).
pub(crate) const OBJECT_VERSION: &str = "AC1024";

/// Whether saving the drawing (as 2004 or later) writes FIELD objects for its texts.
pub(crate) fn writes_fields(d: &Drawing) -> bool {
    every_entity(d).any(|e| matches!(&e.kind, EntityKind::MText(t) if tree(&t.contents).is_some()))
}

/// The file's field objects for the drawing's MTEXTs.
pub(crate) fn plan(d: &Drawing) -> Plan {
    let mut plan = Plan::default();
    let mut make: Vec<(Handle, String, Vec<Node>)> = Vec::new();
    let mut drop: Vec<Handle> = Vec::new();
    for e in every_entity(d) {
        let EntityKind::MText(t) = &e.kind else { continue };
        let old = d.foreign.fields.get(&e.handle);
        match tree(&t.contents) {
            Some((template, nodes)) => {
                plan.fields.insert(e.handle);
                // The objects read with this very text are written back as they were.
                if old != Some(&t.contents) {
                    make.push((e.handle, template, nodes));
                }
            }
            None if old.is_some() => drop.push(e.handle),
            None => {}
        }
    }
    if make.is_empty() && drop.is_empty() {
        return plan;
    }
    let mut f = (*d.foreign).clone();
    for h in &drop {
        f.fields.remove(h);
        drop_objects(&mut f, *h, false);
    }
    // Handles in the file's own space for the objects made (the writer renumbers them).
    let top = f
        .objects
        .iter()
        .map(|o| o.handle.0)
        .chain(f.entities.keys().map(|h| h.0))
        .chain(f.targets.keys().map(|h| h.0))
        .chain(every_entity(d).map(|e| e.handle.0))
        .chain(std::iter::once(d.handseed))
        .max()
        .unwrap_or(0);
    let mut ids = Alloc(top);
    for (h, template, nodes) in make {
        drop_objects(&mut f, h, true);
        let xdict = match f.entities.get(&h).and_then(|x| x.xdict) {
            Some(x) if f.objects.iter().any(|o| o.handle == x) => x,
            _ => {
                let x = ids.next();
                f.objects.push(ForeignObject {
                    handle: x,
                    kind: "DICTIONARY".into(),
                    tags: vec![rt(5, x.hex()), rt(330, h.hex()), rt(100, "AcDbDictionary"), rt(280, "1"), rt(281, "1")],
                });
                f.entities.entry(h).or_default().xdict = Some(x);
                x
            }
        };
        let objects = text_objects(&template, &nodes, xdict, &mut ids);
        if let (Some(top), Some(o)) = (objects.first(), f.objects.iter_mut().find(|o| o.handle == xdict)) {
            o.tags.extend([rt(3, "ACAD_FIELD"), rt(360, top.handle.hex())]);
        }
        f.objects.extend(objects);
    }
    ensure_list(&mut f, &mut ids);
    plan.foreign = Some(f);
    plan
}

/// The FIELDLIST object and the classes of FIELD and FIELDLIST, when the foreign data has FIELD
/// objects (and none of them when it has not).
fn ensure_list(f: &mut Foreign, ids: &mut Alloc) {
    let any = f.objects.iter().any(|o| o.kind == "FIELD");
    let list = f.objects.iter().position(|o| o.kind == "FIELDLIST");
    match (any, list) {
        (false, Some(i)) => {
            let h = f.objects.remove(i).handle;
            f.dictionary.retain(|(_, d)| *d != h);
        }
        (true, None) => {
            let nod = f.targets.iter().find(|(_, k)| k.0 == "NOD" && k.1.is_empty()).map(|(h, _)| *h).unwrap_or_else(|| {
                let h = ids.next();
                f.targets.insert(h, ("NOD".into(), String::new()));
                h
            });
            let h = ids.next();
            let tags = vec![
                rt(5, h.hex()),
                rt(102, "{ACAD_REACTORS"),
                rt(330, nod.hex()),
                rt(102, "}"),
                rt(330, nod.hex()),
                rt(100, "AcDbIdSet"),
                rt(90, "0"),
                rt(100, "AcDbFieldList"),
            ];
            f.objects.push(ForeignObject { handle: h, kind: "FIELDLIST".into(), tags });
            f.dictionary.push(("ACAD_FIELDLIST".into(), h));
        }
        _ => {}
    }
    for (name, dxf) in [("FIELD", "AcDbField"), ("FIELDLIST", "AcDbFieldList")] {
        let present = f.classes.iter().any(|c| c.iter().any(|t| t.code == 1 && t.value.eq_ignore_ascii_case(name)));
        if any && !present {
            f.classes.push(vec![rt(1, name), rt(2, dxf), rt(3, "ObjectDBX Classes"), rt(90, "1152"), rt(91, "0"), rt(280, "0"), rt(281, "0")]);
        }
    }
}

/// Make the written FIELDLIST list exactly the FIELD objects written.
pub(crate) fn fix_list(tags: &mut Vec<Tag>) {
    let fields: Vec<String> = tags
        .iter()
        .enumerate()
        .filter(|(_, t)| t.code == 0 && t.str() == "FIELD")
        .filter_map(|(i, _)| tags.get(i + 1..)?.iter().take_while(|t| t.code != 0).find(|t| t.code == 5).map(|t| t.str()))
        .collect();
    if fields.is_empty() {
        drop_list(tags);
        return;
    }
    let mut i = 0;
    while i < tags.len() {
        if !tags.get(i).is_some_and(|t| t.code == 0 && t.str() == "FIELDLIST") {
            i += 1;
            continue;
        }
        let end = tags.get(i + 1..).and_then(|r| r.iter().position(|t| t.code == 0)).map_or(tags.len(), |p| i + 1 + p);
        let body = tags.get(i + 1..end).unwrap_or(&[]);
        let set = body.iter().position(|t| t.code == 100 && t.str() == "AcDbIdSet");
        let tail = body.iter().position(|t| t.code == 100 && t.str() == "AcDbFieldList");
        if let (Some(set), Some(tail)) = (set, tail)
            && set < tail
        {
            let mut list = vec![Tag::i(90, i64::try_from(fields.len()).unwrap_or(0))];
            list.extend(fields.iter().map(|h| Tag::s(330, h.clone())));
            let (from, to) = (i + 1 + set + 1, i + 1 + tail);
            let n = list.len();
            tags.splice(from..to, list);
            i = end - (to - from) + n;
        } else {
            i = end;
        }
    }
}

/// No FIELD object is written: no FIELDLIST either, and no entry for it in the named object
/// dictionary.
fn drop_list(tags: &mut Vec<Tag>) {
    let mut i = 0;
    while i < tags.len() {
        match tags.get(i) {
            Some(t) if t.code == 0 && t.str() == "FIELDLIST" => {
                let end = tags.get(i + 1..).and_then(|r| r.iter().position(|t| t.code == 0)).map_or(tags.len(), |p| i + 1 + p);
                tags.drain(i..end);
            }
            Some(t) if t.code == 3 && t.str().eq_ignore_ascii_case("ACAD_FIELDLIST") && tags.get(i + 1).is_some_and(|n| n.code == 350) => {
                tags.drain(i..i + 2);
            }
            _ => i += 1,
        }
    }
}
