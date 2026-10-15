//! Annotative text, MText, dimensions and multileaders in DXF, as AutoCAD keeps them. An annotative object
//! carries `AcadAnnotative` xdata and, in its extension dictionary under
//! `AcDbContextDataManager`, a dictionary `ACDB_ANNOTATIONSCALES` with one "object context data"
//! object per annotation scale it supports (`*A1`, `*A2`…). Each context names its SCALE object
//! (340), says whether it is the default one (290 = 1, the scale the object's own groups are
//! written at) and holds what changes with the scale: a text's insertion and alignment points, an
//! MText's insertion point, width and columns, a dimension's text position and the anonymous `*D`
//! block with its geometry at that scale, a multileader's whole `CONTEXT_DATA{` body at that
//! scale.
//!
//! CADCraft keeps an annotative object at paper size with the list of scales it supports
//! ([`cadcraft_doc::Common::anno_scales`]). It writes the object at its default scale with a
//! context per scale ([`plan`], [`objects`]), and reads the scales and paper size back from the
//! contexts ([`resolve`]). A context can place the object somewhere else at its scale (moved with
//! grips at that scale in AutoCAD or CADCraft): that is the object's offset at the scale
//! ([`cadcraft_doc::AnnoMove`]), written into and read back from the context's position.
//!
//! Objects inside block definitions are left alone, and files older than AutoCAD 2007's format
//! (before annotation scaling) don't get contexts.

use std::collections::{BTreeSet, HashMap};

use cadcraft_doc::{AnnoScale, DimKind, Drawing, Entity, EntityKind, HAlign, Handle, MLeader, MTextColumnType, Text, VAlign};
use cadcraft_dxf::{Section, Tag};
use cadcraft_geom::Vec3;

/// The context classes: (DXF record name, C++ class name).
const TEXT: (&str, &str) = ("ACDB_TEXTOBJECTCONTEXTDATA_CLASS", "AcDbTextObjectContextData");
const MTEXT: (&str, &str) = ("ACDB_MTEXTOBJECTCONTEXTDATA_CLASS", "AcDbMTextObjectContextData");
const ALDIM: (&str, &str) = ("ACDB_ALDIMOBJECTCONTEXTDATA_CLASS", "AcDbAlignedDimensionObjectContextData");
const ANGDIM: (&str, &str) = ("ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS", "AcDbAngularDimensionObjectContextData");
const RADIM: (&str, &str) = ("ACDB_RADIMOBJECTCONTEXTDATA_CLASS", "AcDbRadialDimensionObjectContextData");
const DMDIM: (&str, &str) = ("ACDB_DMDIMOBJECTCONTEXTDATA_CLASS", "AcDbDiametricDimensionObjectContextData");
const ORDDIM: (&str, &str) = ("ACDB_ORDDIMOBJECTCONTEXTDATA_CLASS", "AcDbOrdinateDimensionObjectContextData");
const MLEADER: (&str, &str) = ("ACDB_MLEADEROBJECTCONTEXTDATA_CLASS", "AcDbMLeaderObjectContextData");

/// The extension dictionary entry and the dictionary of contexts.
const MANAGER_KEY: &str = "AcDbContextDataManager";
const SCALES_KEY: &str = "ACDB_ANNOTATIONSCALES";

/// Most contexts read per object (hostile files).
const MAX_CONTEXTS: usize = cadcraft_doc::MAX_SCALES;

/// Whether `e` (in model space or a layout) is written with annotation scale contexts: annotative
/// text, MText, dimensions and multileaders.
pub(crate) fn has_contexts(e: &Entity) -> bool {
    cadcraft_doc::is_annotative(e) && matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_) | EntityKind::MLeader(_))
}

/// The scales `e` is written at (valid, each name once) and which of them is the default: the
/// current annotation scale when `e` supports it, else its first.
pub(crate) fn scales(d: &Drawing, e: &Entity) -> (Vec<AnnoScale>, usize) {
    let mut out: Vec<AnnoScale> = Vec::new();
    for s in e.common.anno_scales.iter().filter(|s| s.is_valid() && !s.name.trim().is_empty()) {
        if !out.iter().any(|x| x.name.eq_ignore_ascii_case(&s.name)) {
            out.push(s.clone());
        }
    }
    let current = d.current_anno_scale();
    let default = out.iter().position(|s| s.name.eq_ignore_ascii_case(&current.name)).unwrap_or(0);
    (out, default)
}

/// Whether text placed by its alignment point (group 11) rather than its start point (10).
fn by_alignment(t: &Text) -> bool {
    t.align_pt.is_some() && !matches!((t.halign, t.valign), (HAlign::Left, VAlign::Baseline) | (HAlign::Aligned | HAlign::Fit, _))
}

/// `e` at annotation scale `f`, as its groups are written there: sizes scaled (see
/// [`cadcraft_doc::anno_scaled`]) and justified text's start point moved with its size about its
/// alignment point. A dimension's definition gets that scale (DIMSCALE). The object's offset at
/// that scale moves it there.
pub(crate) fn at_scale(e: &Entity, f: f64) -> Entity {
    let mut out = shared_at_scale(e, f);
    cadcraft_doc::shift_position(&mut out.kind, e.common.moved_at_factor(f));
    out
}

/// [`at_scale`] at the object's shared position (without its offset there).
fn shared_at_scale(e: &Entity, f: f64) -> Entity {
    let mut out = cadcraft_doc::anno_scaled(e, f);
    if let (EntityKind::Text(t), EntityKind::Text(paper)) = (&mut out.kind, &e.kind)
        && by_alignment(paper)
        && let Some(a) = paper.align_pt
    {
        t.insert = a + (paper.insert - a) * f;
    }
    out
}

/// The point that places `e` (as written at some scale): a text's start or alignment point, an
/// MText's insertion point, a dimension's text position, a multileader's landing.
fn anchor(d: &Drawing, e: &Entity) -> Option<cadcraft_geom::Vec2> {
    let p = match &e.kind {
        EntityKind::Text(t) if by_alignment(t) => t.align_pt?.xy(),
        EntityKind::Text(t) => t.insert.xy(),
        EntityKind::MText(t) => t.insert.xy(),
        EntityKind::Dimension(dm) if dm.user_text_pos => dm.text_mid.xy(),
        EntityKind::Dimension(dm) => cadcraft_render::dimension_in(d, dm).text_pos,
        EntityKind::MLeader(m) => m.landing.xy(),
        _ => return None,
    };
    p.is_finite().then_some(p)
}

/// The same point as a context of `e` holds it (`g`: the context's groups after its SCALE
/// handle).
fn context_anchor(e: &Entity, g: &[Tag]) -> Option<cadcraft_geom::Vec2> {
    let xy = |code: i32| -> Option<cadcraft_geom::Vec2> {
        let i = g.iter().position(|t| t.code == code)?;
        let y = g.get(i + 1).filter(|t| t.code == code + 10)?;
        let v = cadcraft_geom::Vec2::new(g.get(i)?.f64(), y.f64());
        v.is_finite().then_some(v)
    };
    match &e.kind {
        EntityKind::Text(t) if by_alignment(t) => xy(11),
        EntityKind::Text(_) => xy(10),
        EntityKind::MText(_) => xy(11),
        // Only a text the user placed (294 = 0) holds its own position.
        EntityKind::Dimension(_) => g.iter().find(|t| t.code == 294).is_some_and(|t| t.i64() == 0).then(|| xy(10)).flatten(),
        EntityKind::MLeader(_) => {
            let mut tags = vec![Tag::s(100, "AcDbMLeader"), Tag::s(300, "CONTEXT_DATA{")];
            tags.extend(g.iter().cloned());
            tags.push(Tag::s(301, "}"));
            let p = crate::dxf_mleader::read(&tags).landing.xy();
            p.is_finite().then_some(p)
        }
        _ => None,
    }
}

/// One context: its handle, the SCALE object's handle and the scale.
pub(crate) struct Context {
    handle: String,
    scale: String,
    factor: f64,
    default: bool,
    /// A dimension's `*D` block at this scale.
    block: Option<String>,
}

/// The objects written for one annotative object.
pub(crate) struct Obj {
    /// Its extension dictionary (shared with a dimension's DIMASSOC).
    pub xdict: String,
    /// Whether the extension dictionary is written here (not with the DIMASSOC).
    pub own_xdict: bool,
    pub manager: String,
    dict: String,
    contexts: Vec<Context>,
}

/// The annotative objects of a file being written, by entity handle.
#[derive(Default)]
pub(crate) struct Plan {
    pub objs: HashMap<Handle, Obj>,
}

impl Plan {
    /// The context classes the file uses (for its CLASSES section).
    pub fn classes(&self, d: &Drawing) -> Vec<(&'static str, &'static str)> {
        let mut out = BTreeSet::new();
        for h in self.objs.keys() {
            if let Some(e) = d.entity(*h) {
                out.insert(class_of(e));
            }
        }
        out.into_iter().flatten().collect()
    }
}

fn class_of(e: &Entity) -> Option<(&'static str, &'static str)> {
    Some(match &e.kind {
        EntityKind::Text(_) => TEXT,
        EntityKind::MText(_) => MTEXT,
        EntityKind::MLeader(_) => MLEADER,
        EntityKind::Dimension(dm) => match dm.kind {
            DimKind::Linear { .. } | DimKind::Aligned | DimKind::ArcLength => ALDIM,
            DimKind::Angular | DimKind::Angular3P => ANGDIM,
            DimKind::Radius => RADIM,
            DimKind::Diameter => DMDIM,
            DimKind::Ordinate { .. } => ORDDIM,
        },
        _ => return None,
    })
}

/// Plan the objects of `e` (annotative, see [`has_contexts`]): `scale_handle` gives each scale's
/// SCALE object, `xdict` the extension dictionary it already has (a dimension's DIMASSOC),
/// `blocks` a dimension's `*D` block at each of [`scales`], and `next` new handles. `None` when
/// no scale has a SCALE object.
pub(crate) fn plan_one(
    d: &Drawing,
    e: &Entity,
    scale_handle: impl Fn(&str) -> Option<String>,
    xdict: Option<String>,
    blocks: Option<&[String]>,
    mut next: impl FnMut() -> String,
) -> Option<Obj> {
    let (scales, default) = scales(d, e);
    let mut contexts = Vec::new();
    for (i, s) in scales.iter().enumerate() {
        let Some(scale) = scale_handle(&s.name) else { continue };
        let block = blocks.and_then(|b| b.get(i)).cloned();
        contexts.push(Context { handle: String::new(), scale, factor: s.factor(), default: i == default, block });
    }
    if contexts.is_empty() {
        return None;
    }
    if !contexts.iter().any(|c| c.default)
        && let Some(c) = contexts.first_mut()
    {
        c.default = true;
    }
    let own_xdict = xdict.is_none();
    let xdict = xdict.unwrap_or_else(&mut next);
    let (manager, dict) = (next(), next());
    for c in &mut contexts {
        c.handle = next();
    }
    Some(Obj { xdict, own_xdict, manager, dict, contexts })
}

/// The factor `e` is written at (its default context's).
pub(crate) fn default_factor(o: &Obj) -> f64 {
    o.contexts.iter().find(|c| c.default).map_or(1.0, |c| c.factor)
}

/// `AcadAnnotative` xdata marking an annotative object.
pub(crate) fn xdata() -> Vec<Tag> {
    vec![Tag::s(1001, "AcadAnnotative"), Tag::s(1000, "AnnotativeData"), Tag::s(1002, "{"), Tag::i(1070, 1), Tag::i(1070, 1), Tag::s(1002, "}")]
}

/// The extension dictionary entry of `o`'s context manager (key and handle groups).
pub(crate) fn xdict_entry(o: &Obj) -> Vec<Tag> {
    vec![Tag::s(3, MANAGER_KEY), Tag::s(360, o.manager.clone())]
}

fn p(out: &mut Vec<Tag>, c: i32, v: Vec3, z: bool) {
    out.push(Tag::f(c, v.x));
    out.push(Tag::f(c + 10, v.y));
    if z {
        out.push(Tag::f(c + 20, v.z));
    }
}

/// A dictionary record: handle, reactor and owner `owner`, hard-owner flag 281 = 1, `entries`.
fn dictionary(out: &mut Vec<Tag>, h: &str, owner: &str, cloning: bool, entries: &[(String, i32, String)]) {
    out.push(Tag::s(0, "DICTIONARY"));
    out.push(Tag::s(5, h));
    out.extend([Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, owner), Tag::s(102, "}")]);
    out.push(Tag::s(330, owner));
    out.push(Tag::s(100, "AcDbDictionary"));
    if cloning {
        out.push(Tag::i(280, 1));
    }
    out.push(Tag::i(281, 1));
    for (k, code, v) in entries {
        out.push(Tag::s(3, k.clone()));
        out.push(Tag::s(*code, v.clone()));
    }
}

/// The objects of annotative object `e` planned as `o`: its extension dictionary (unless the
/// DIMASSOC's holds the manager), the context manager, the dictionary of contexts and the
/// contexts. `mleader` gives a multileader's groups as the entity writer writes them.
pub(crate) fn objects(d: &Drawing, e: &Entity, o: &Obj, mleader: &dyn Fn(&MLeader) -> Vec<Tag>) -> Vec<Tag> {
    let mut out = Vec::new();
    let eh = e.handle.hex();
    if o.own_xdict {
        dictionary(&mut out, &o.xdict, &eh, true, &[(MANAGER_KEY.to_string(), 360, o.manager.clone())]);
    }
    dictionary(&mut out, &o.manager, &o.xdict, false, &[(SCALES_KEY.to_string(), 350, o.dict.clone())]);
    let keyed: Vec<(String, i32, String)> = o.contexts.iter().enumerate().map(|(i, c)| (format!("*A{}", i + 1), 350, c.handle.clone())).collect();
    dictionary(&mut out, &o.dict, &o.manager, false, &keyed);
    let Some((class, _)) = class_of(e) else { return out };
    for c in &o.contexts {
        out.push(Tag::s(0, class));
        out.push(Tag::s(5, c.handle.clone()));
        out.extend([Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, o.dict.clone()), Tag::s(102, "}")]);
        out.push(Tag::s(330, o.dict.clone()));
        out.push(Tag::s(100, "AcDbObjectContextData"));
        out.push(Tag::i(70, 4));
        out.push(Tag::i(290, i64::from(c.default)));
        out.push(Tag::s(100, "AcDbAnnotScaleObjectContextData"));
        out.push(Tag::s(340, c.scale.clone()));
        context_groups(&mut out, d, &at_scale(e, c.factor), c, mleader);
    }
    out
}

/// What a context holds of `e` (already at the context's scale).
fn context_groups(out: &mut Vec<Tag>, d: &Drawing, e: &Entity, c: &Context, mleader: &dyn Fn(&MLeader) -> Vec<Tag>) {
    match &e.kind {
        // The multileader's `CONTEXT_DATA{` body, without its braces.
        EntityKind::MLeader(m) => {
            let mut tags = mleader(m);
            mleader_at(&mut tags, c.factor);
            let open = tags.iter().position(|t| t.code == 300 && t.str().trim_start().starts_with("CONTEXT_DATA"));
            let close = tags.iter().position(|t| t.code == 301);
            if let (Some(a), Some(b)) = (open, close)
                && a < b
            {
                out.extend(tags.get(a + 1..b).unwrap_or(&[]).iter().cloned());
            }
        }
        EntityKind::Text(t) => {
            let h = match t.halign {
                HAlign::Left => 0,
                HAlign::Center => 1,
                HAlign::Right => 2,
                HAlign::Aligned => 3,
                HAlign::Middle => 4,
                HAlign::Fit => 5,
            };
            out.push(Tag::i(70, h));
            out.push(Tag::f(50, t.rotation.to_degrees()));
            p(out, 10, t.insert, false);
            p(out, 11, t.align_pt.unwrap_or(Vec3::ZERO), false);
        }
        EntityKind::MText(t) => {
            out.push(Tag::i(70, i64::from(t.attach)));
            p(out, 10, Vec3::new(t.rotation.cos(), t.rotation.sin(), 0.0), true);
            p(out, 11, t.insert, true);
            // Reference rectangle (40, 41) and an estimate of the text's extents (42, 43): AutoCAD
            // lays the text out again.
            let lines = t.contents.split("\\P").count().max(1) as f64;
            let longest = t.contents.split("\\P").map(|l| l.chars().count()).max().unwrap_or(1) as f64;
            out.push(Tag::f(40, t.width));
            out.push(Tag::f(41, 0.0));
            out.push(Tag::f(42, if t.width > 0.0 { t.width } else { t.height * 0.8 * longest }));
            out.push(Tag::f(43, t.height * 1.66 * lines));
            match &t.columns {
                None => out.push(Tag::i(71, 0)),
                Some(cols) => {
                    let dynamic = cols.kind == MTextColumnType::Dynamic;
                    let manual = dynamic && !cols.auto_height;
                    out.push(Tag::i(71, if dynamic { 2 } else { 1 }));
                    out.push(Tag::i(72, if manual { cols.heights.len() as i64 } else { i64::from(cols.count.max(1)) }));
                    out.push(Tag::f(44, t.width));
                    out.push(Tag::f(45, cols.gutter));
                    out.push(Tag::i(73, i64::from(cols.auto_height)));
                    out.push(Tag::i(74, i64::from(cols.flow_reversed)));
                    if manual {
                        out.extend(cols.heights.iter().map(|h| Tag::f(46, *h)));
                    }
                }
            }
        }
        EntityKind::Dimension(dm) => {
            out.push(Tag::s(100, "AcDbDimensionObjectContextData"));
            out.push(Tag::s(2, c.block.clone().unwrap_or_default()));
            out.push(Tag::i(293, 0));
            let text = if dm.user_text_pos { dm.text_mid } else { cadcraft_render::dimension_in(d, dm).text_pos.to3(0.0) };
            p(out, 10, text, false);
            out.push(Tag::i(294, i64::from(!dm.user_text_pos)));
            out.push(Tag::f(140, dm.text_rotation));
            for code in [298, 291, 70, 292, 71, 280, 295, 296, 297] {
                out.push(Tag::i(code, 0));
            }
            let Some((_, sub)) = class_of(e) else { return };
            out.push(Tag::s(100, sub));
            let pts: &[(i32, Vec3)] = match dm.kind {
                DimKind::Linear { .. } | DimKind::Aligned | DimKind::ArcLength | DimKind::Angular3P => &[(11, dm.defpt)],
                DimKind::Angular => &[(11, dm.p16)],
                DimKind::Radius => &[(11, dm.p15)],
                DimKind::Diameter => &[(11, dm.p15), (12, dm.defpt)],
                DimKind::Ordinate { .. } => &[(11, dm.defpt), (12, dm.p14)],
            };
            for (code, v) in pts {
                p(out, *code, *v, true);
            }
        }
        _ => {}
    }
}

/// A multileader's groups (as [`crate::dxf_mleader::entity_tags`] writes them) at annotation
/// scale `f`: its context's overall scale (40) is `f` and its landing gap (145) is at that scale.
pub(crate) fn mleader_at(tags: &mut [Tag], f: f64) {
    let Some(open) = tags.iter().position(|t| t.code == 300 && t.str().trim_start().starts_with("CONTEXT_DATA")) else { return };
    let (mut scale, mut gap) = (false, false);
    for t in tags.iter_mut().skip(open + 1) {
        match t.code {
            // The context's own groups end where its leaders start.
            302 | 301 => break,
            40 if !scale => {
                *t = Tag::f(40, f);
                scale = true;
            }
            145 if !gap => {
                *t = Tag::f(145, t.f64() * f);
                gap = true;
            }
            _ => {}
        }
    }
}

/// A multileader's groups (as [`crate::dxf_mleader::entity_tags`] writes them) marked annotative:
/// the entity's "enable annotation scale" (its 293, after the `CONTEXT_DATA{` block) is on.
/// Without it AutoCAD keeps the contexts but its ANNORESET passes the multileader over.
pub(crate) fn mleader_annotative(tags: &mut [Tag]) {
    let Some(close) = tags.iter().position(|t| t.code == 301) else { return };
    if let Some(t) = tags.iter_mut().skip(close + 1).find(|t| t.code == 293) {
        *t = Tag::i(293, 1);
    }
}

// ---------------------------------------------------------------------------------------------
// Reading

/// A record's groups before its xdata.
fn own(tags: &[Tag]) -> &[Tag] {
    tags.get(..tags.iter().position(|t| t.code >= 1000).unwrap_or(tags.len())).unwrap_or(&[])
}

/// A handle as the key of the maps below.
fn key(s: &str) -> Option<Handle> {
    Handle::parse_hex(s.trim()).filter(|h| h.0 != 0)
}

/// The extension dictionary of a record (`{ACAD_XDICTIONARY` group).
fn xdict_of(tags: &[Tag]) -> Option<Handle> {
    let i = own(tags).iter().position(|t| t.code == 102 && t.str().trim() == "{ACAD_XDICTIONARY")?;
    tags.get(i + 1).filter(|t| t.code == 360).and_then(|t| key(&t.str()))
}

/// A dictionary's entry `name` (case-insensitive).
fn entry(tags: &[Tag], name: &str) -> Option<Handle> {
    let g = own(tags);
    let i = g.iter().position(|t| t.code == 3 && t.str().trim().eq_ignore_ascii_case(name))?;
    g.get(i + 1).filter(|t| matches!(t.code, 350 | 360)).and_then(|t| key(&t.str()))
}

/// Every entry of a dictionary.
fn all_entries(tags: &[Tag]) -> Vec<Handle> {
    own(tags).iter().filter(|t| matches!(t.code, 350 | 360)).filter_map(|t| key(&t.str())).take(MAX_CONTEXTS).collect()
}

/// Read the annotation scale contexts of the text, MText and dimensions of model space and the
/// layouts: each becomes annotative with the scales of its contexts (in their order), and
/// its sizes are taken back to paper size from the default scale. Contexts of other objects, and
/// contexts whose scale is unknown, are passed over.
pub(crate) fn resolve(secs: &[Section], d: &mut Drawing) {
    let Some(objects) = secs.iter().find(|s| s.name == "OBJECTS") else { return };
    let mut objs: HashMap<Handle, (String, &[Tag])> = HashMap::new();
    each_record(&objects.tags, |kind, tags| {
        if let Some(h) = own(tags).iter().find(|t| t.code == 5).and_then(|t| key(&t.str())) {
            objs.entry(h).or_insert((kind.to_ascii_uppercase(), tags));
        }
    });
    current_scale(&objs, d);
    if !objs.values().any(|(k, _)| k.ends_with("OBJECTCONTEXTDATA_CLASS")) {
        return;
    }
    let scales: HashMap<Handle, AnnoScale> =
        objs.iter().filter(|(_, (k, _))| k == "SCALE").filter_map(|(h, (_, t))| Some((*h, crate::dxf_ext::read_scale(own(t))?))).collect();
    // The entities that may have contexts: their extension dictionaries.
    let mut found: Vec<(Handle, Handle)> = Vec::new();
    for s in secs.iter().filter(|s| s.name == "ENTITIES" || s.name == "BLOCKS") {
        each_record(&s.tags, |kind, tags| {
            if !matches!(kind, "TEXT" | "MTEXT" | "DIMENSION" | "ARC_DIMENSION" | "LARGE_RADIAL_DIMENSION" | "MULTILEADER") {
                return;
            }
            let h = own(tags).iter().find(|t| t.code == 5).and_then(|t| key(&t.str()));
            if let (Some(h), Some(x)) = (h, xdict_of(tags)) {
                found.push((h, x));
            }
        });
    }
    for (h, xdict) in found {
        let Some(e) = d.entity(h) else { continue };
        if !matches!(e.kind, EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_) | EntityKind::MLeader(_)) {
            continue;
        }
        let dict = objs
            .get(&xdict)
            .and_then(|(_, t)| entry(t, MANAGER_KEY))
            .and_then(|m| objs.get(&m))
            .and_then(|(_, t)| entry(t, SCALES_KEY))
            .and_then(|s| objs.get(&s));
        let Some((_, dict)) = dict else { continue };
        let mut list: Vec<(AnnoScale, bool)> = Vec::new();
        let mut placed: Vec<(String, f64, cadcraft_geom::Vec2)> = Vec::new();
        for c in all_entries(dict) {
            let Some((_, t)) = objs.get(&c).filter(|(k, _)| k.ends_with("OBJECTCONTEXTDATA_CLASS")) else { continue };
            let g = own(t);
            let scale = g.iter().find(|t| t.code == 340).and_then(|t| key(&t.str())).and_then(|s| scales.get(&s));
            let default = g.iter().find(|t| t.code == 290).is_some_and(|t| t.i64() != 0);
            if let Some(s) = scale
                && !list.iter().any(|(x, _)| x.name.eq_ignore_ascii_case(&s.name))
            {
                list.push((s.clone(), default));
                let after = g.iter().position(|t| t.code == 340).map_or(&[][..], |i| g.get(i + 1..).unwrap_or(&[]));
                if let Some(at) = context_anchor(e, after) {
                    placed.push((s.name.clone(), s.factor(), at));
                }
            }
        }
        if list.is_empty() {
            continue;
        }
        // The object's groups were written at the default context's scale.
        let f = list.iter().find(|(_, default)| *default).or(list.first()).map_or(1.0, |(s, _)| s.factor());
        let list: Vec<AnnoScale> = list.into_iter().map(|(s, _)| s).collect();
        let _ = d.modify_entity(h, |e| {
            e.common.anno_scales = list;
            to_paper(e, f);
        });
        // A context placing the object elsewhere than its scale would: its offset there.
        let Some(e) = d.entity(h).map(|e| (**e).clone()) else { continue };
        let moves: Vec<(String, cadcraft_geom::Vec2)> = placed
            .iter()
            .filter_map(|(name, f, at)| {
                let by = *at - anchor(d, &shared_at_scale(&e, *f))?;
                let tol = 1e-6 * (1.0 + at.len());
                (by.is_finite() && by.len() > tol).then(|| (name.clone(), by))
            })
            .collect();
        if !moves.is_empty() {
            let _ = d.modify_entity(h, |e| {
                for (name, by) in &moves {
                    e.common.move_at(name, *by);
                }
            });
        }
    }
}

/// The current annotation scale as AutoCAD keeps it: in the named object dictionary's
/// `AcDbVariableDictionary`, a DICTIONARYVAR `CANNOSCALE` with the scale's name (group 1). It is
/// newer than CADCraft's own record when AutoCAD saved the file last (AutoCAD keeps that record as
/// it was), and CADCraft writes the two alike. A name the scale list lacks is passed over.
fn current_scale(objs: &HashMap<Handle, (String, &[Tag])>, d: &mut Drawing) {
    let root = objs.iter().filter(|(_, (k, t))| k == "DICTIONARY" && own(t).iter().find(|t| t.code == 330).is_some_and(|o| o.str().trim() == "0"));
    let Some((_, (_, root))) = root.min_by_key(|(h, _)| h.0) else { return };
    let var = entry(root, "AcDbVariableDictionary").and_then(|v| objs.get(&v)).and_then(|(_, t)| entry(t, "CANNOSCALE")).and_then(|c| objs.get(&c));
    let Some(name) = var.and_then(|(_, t)| own(t).iter().find(|t| t.code == 1).map(Tag::str)) else { return };
    let name = name.trim();
    if d.anno_scale(name).is_some_and(AnnoScale::is_valid) {
        d.header.set_str("CANNOSCALE", name);
    }
}

/// Take an object written at annotation scale `f` back to paper size (the inverse of
/// [`at_scale`]).
fn to_paper(e: &mut Entity, f: f64) {
    if !(f.is_finite() && f > 0.0) || (f - 1.0).abs() < 1e-12 {
        return;
    }
    let inv = 1.0 / f;
    match &mut e.kind {
        EntityKind::Text(t) => {
            if by_alignment(t)
                && let Some(a) = t.align_pt
            {
                t.insert = a + (t.insert - a) * inv;
            }
            t.height *= inv;
        }
        EntityKind::MText(t) => {
            t.height *= inv;
            t.width *= inv;
            if let Some(c) = &mut t.columns {
                c.scale(inv);
            }
        }
        // Its sizes, and its content's place about the landing, scale linearly.
        EntityKind::MLeader(_) => e.kind = cadcraft_doc::anno_scaled(e, inv).kind,
        _ => {}
    }
}

/// Call `f` with each record (type, groups after its `0` group) of a section.
fn each_record<'a>(tags: &'a [Tag], mut f: impl FnMut(&str, &'a [Tag])) {
    let mut open: Option<(String, usize)> = None;
    for (i, t) in tags.iter().enumerate() {
        if t.code == 0 {
            if let Some((k, s)) = open.take() {
                f(&k, tags.get(s..i).unwrap_or(&[]));
            }
            open = Some((t.str(), i + 1));
        }
    }
    if let Some((k, s)) = open {
        f(&k, tags.get(s..).unwrap_or(&[]));
    }
}
