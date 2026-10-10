//! Raster images in DXF (DXF Reference: IMAGE entity; IMAGEDEF, IMAGEDEF_REACTOR and
//! RASTERVARIABLES objects). An IMAGE entity holds the placement, clip boundary and display
//! settings; it points (group 340) to an IMAGEDEF object holding the file path, which the
//! `ACAD_IMAGE_DICT` dictionary names. CADCraft keeps this data so images survive save and
//! reopen; the pixels themselves are not loaded.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle, Image};
use cadcraft_dxf::{Section, Tag};
use cadcraft_geom::{Vec2, Vec3};

/// Most clip boundary vertices kept per image (hostile files).
const MAX_CLIP: usize = 100_000;

/// The IMAGE entity's own groups (after the `AcDbRasterImage` marker when present).
pub(crate) fn read_entity(tags: &[Tag]) -> Image {
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbRasterImage").unwrap_or(0);
    let tags = tags.get(start..).unwrap_or(&[]);
    let f = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::f64);
    let i = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::i64);
    let p = |c: i32| Vec3::new(f(c).unwrap_or(0.0), f(c + 10).unwrap_or(0.0), f(c + 20).unwrap_or(0.0));
    let pct = |c: i32, d: u8| i(c).and_then(|v| u8::try_from(v.clamp(0, 100)).ok()).unwrap_or(d);
    let xs = tags.iter().filter(|t| t.code == 14).map(Tag::f64);
    let ys = tags.iter().filter(|t| t.code == 24).map(Tag::f64);
    let mut clip: Vec<Vec2> = xs.zip(ys).take(MAX_CLIP).map(|(x, y)| Vec2::new(x, y)).collect();
    // A rectangular boundary (type 1) is two opposite corners.
    if i(71) == Some(1) {
        clip.truncate(2);
    }
    let def = Image::default();
    Image {
        insert: p(10),
        u: p(11),
        v: p(12),
        size: Vec2::new(f(13).unwrap_or(1.0), f(23).unwrap_or(1.0)),
        clip,
        clipping: i(280).is_some_and(|v| v != 0),
        display: i(70).and_then(|v| u16::try_from(v).ok()).unwrap_or(def.display),
        brightness: pct(281, def.brightness),
        contrast: pct(282, def.contrast),
        fade: pct(283, def.fade),
        ..def
    }
}

/// Call `f` with each record (`0 TYPE` and the groups after it) of a section, without copying.
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

fn key(s: &str) -> String {
    s.trim().to_ascii_uppercase()
}

/// An IMAGEDEF object's data.
#[derive(Default)]
struct Def {
    name: String,
    path: String,
    pixel_size: Option<Vec2>,
    units: u8,
}

fn has_images(store: &EntityStore) -> bool {
    store.iter().any(|e| matches!(e.kind, EntityKind::Image(_)))
}

/// Fill in each image's definition (path, name, pixel size) from the IMAGEDEF it points to.
pub(crate) fn resolve(secs: &[Section], d: &mut Drawing) {
    if !has_images(&d.model) && !d.layouts.iter().any(|l| has_images(&l.entities)) && !d.blocks.values().any(|b| has_images(&b.entities)) {
        return;
    }
    // IMAGE handle → IMAGEDEF handle.
    let mut refs: HashMap<Handle, String> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "ENTITIES" || s.name == "BLOCKS") {
        each_record(&s.tags, |kind, tags| {
            if kind != "IMAGE" {
                return;
            }
            let h = tags.iter().find(|t| t.code == 5).and_then(|t| Handle::parse_hex(&t.str()));
            let def = tags.iter().find(|t| t.code == 340).map(|t| key(&t.str()));
            if let (Some(h), Some(def)) = (h, def) {
                refs.insert(h, def);
            }
        });
    }
    // IMAGEDEF objects, and the names dictionaries give them.
    let mut defs: HashMap<String, Def> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "OBJECTS") {
        each_record(&s.tags, |kind, tags| match kind {
            "IMAGEDEF" => {
                let Some(h) = tags.iter().find(|t| t.code == 5).map(|t| key(&t.str())) else { return };
                let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbRasterImageDef").unwrap_or(0);
                let body = tags.get(start..).unwrap_or(&[]);
                let f = |c: i32| body.iter().find(|t| t.code == c).map(Tag::f64);
                let pixel = match (f(11), f(21)) {
                    (Some(x), Some(y)) if x > 0.0 && y > 0.0 => Some(Vec2::new(x, y)),
                    _ => None,
                };
                let units = body.iter().find(|t| t.code == 281).and_then(|t| u8::try_from(t.i64()).ok()).unwrap_or(0);
                let path = body.iter().find(|t| t.code == 1).map(Tag::str).unwrap_or_default();
                defs.insert(h, Def { path, pixel_size: pixel, units, ..Def::default() });
            }
            "DICTIONARY" => {
                let mut name: Option<String> = None;
                for t in tags {
                    match t.code {
                        3 => name = Some(t.str()),
                        350 | 360 => {
                            if let Some(n) = name.take() {
                                names.insert(key(&t.str()), n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        });
    }
    for (h, def) in &mut defs {
        if let Some(n) = names.get(h) {
            def.name = n.clone();
        }
    }
    let by_image: HashMap<Handle, &Def> = refs.iter().filter_map(|(img, dh)| Some((*img, defs.get(dh)?))).collect();
    if by_image.is_empty() {
        return;
    }
    fill(&mut d.model, &by_image);
    for l in &mut d.layouts {
        fill(&mut l.entities, &by_image);
    }
    for b in d.blocks.values_mut() {
        if b.entities.iter().any(|e| matches!(e.kind, EntityKind::Image(_)) && by_image.contains_key(&e.handle)) {
            fill(&mut Arc::make_mut(b).entities, &by_image);
        }
    }
}

fn fill(store: &mut EntityStore, by_image: &HashMap<Handle, &Def>) {
    let hs: Vec<Handle> =
        store.iter().filter(|e| matches!(e.kind, EntityKind::Image(_)) && by_image.contains_key(&e.handle)).map(|e| e.handle).collect();
    for h in hs {
        let Some(def) = by_image.get(&h) else { continue };
        store.modify(h, |e| {
            if let EntityKind::Image(i) = &mut e.kind {
                i.path = def.path.clone();
                i.name = def.name.clone();
                if let Some(p) = def.pixel_size {
                    i.pixel_size = p;
                }
                i.resolution_units = def.units;
            }
        });
    }
}

/// One IMAGEDEF to write, shared by the images with the same path and name.
struct PlannedDef {
    handle: String,
    name: String,
    path: String,
    size: Vec2,
    pixel_size: Vec2,
    units: u8,
    /// (IMAGEDEF_REACTOR handle, IMAGE handle) per image using it.
    reactors: Vec<(String, Handle)>,
}

/// The objects behind a drawing's images, with handles allocated before anything is written.
#[derive(Default)]
pub(crate) struct Plan {
    /// IMAGE entity → (IMAGEDEF handle, IMAGEDEF_REACTOR handle).
    pub by_entity: HashMap<Handle, (String, String)>,
    defs: Vec<PlannedDef>,
    /// The `ACAD_IMAGE_DICT` dictionary and the `ACAD_IMAGE_VARS` RASTERVARIABLES object.
    pub dict: String,
    pub vars: String,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

/// The image's definition name: its own, or the file name without extension.
fn def_name(i: &Image) -> String {
    let n = i.name.trim();
    if !n.is_empty() {
        return n.to_string();
    }
    let file = i.path.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).trim();
    if stem.is_empty() { "Image".to_string() } else { stem.to_string() }
}

/// Allocate the IMAGEDEF, IMAGEDEF_REACTOR and dictionary handles for every image entity.
pub(crate) fn plan<'a>(entities: impl Iterator<Item = &'a cadcraft_doc::Entity>, mut next: impl FnMut() -> String) -> Plan {
    let mut p = Plan::default();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    for e in entities {
        let EntityKind::Image(i) = &e.kind else { continue };
        if p.by_entity.contains_key(&e.handle) {
            continue;
        }
        let k = (i.path.clone(), i.name.clone());
        let at = match index.get(&k) {
            Some(&at) => at,
            None => {
                // Dictionary keys are unique regardless of case.
                let base = def_name(i);
                let mut name = base.clone();
                let mut n = 2usize;
                while !used.insert(key(&name)) {
                    name = format!("{base}_{n}");
                    n = n.saturating_add(1);
                }
                p.defs.push(PlannedDef {
                    handle: next(),
                    name,
                    path: i.path.clone(),
                    size: i.size,
                    pixel_size: i.pixel_size,
                    units: i.resolution_units,
                    reactors: Vec::new(),
                });
                let at = p.defs.len().saturating_sub(1);
                index.insert(k, at);
                at
            }
        };
        let reactor = next();
        if let Some(def) = p.defs.get_mut(at) {
            def.reactors.push((reactor.clone(), e.handle));
            p.by_entity.insert(e.handle, (def.handle.clone(), reactor));
        }
    }
    if !p.defs.is_empty() {
        p.dict = next();
        p.vars = next();
    }
    p
}

fn p3(t: &mut Vec<Tag>, c: i32, v: Vec3) {
    t.extend([Tag::f(c, v.x), Tag::f(c + 10, v.y), Tag::f(c + 20, v.z)]);
}

/// The IMAGE entity's groups after its common entity groups.
pub(crate) fn entity_tags(i: &Image, def: &str, reactor: &str) -> Vec<Tag> {
    let mut t = vec![Tag::i(90, 0)];
    p3(&mut t, 10, i.insert);
    p3(&mut t, 11, i.u);
    p3(&mut t, 12, i.v);
    t.extend([Tag::f(13, i.size.x), Tag::f(23, i.size.y), Tag::s(340, def), Tag::i(70, i64::from(i.display))]);
    t.extend([
        Tag::i(280, i64::from(i.clipping)),
        Tag::i(281, i64::from(i.brightness.min(100))),
        Tag::i(282, i64::from(i.contrast.min(100))),
        Tag::i(283, i64::from(i.fade.min(100))),
        Tag::s(360, reactor),
    ]);
    // Without a boundary of its own the image is clipped to its full extent (pixel corners).
    let full = [Vec2::new(-0.5, -0.5), Vec2::new(i.size.x - 0.5, i.size.y - 0.5)];
    let clip: &[Vec2] = if i.clip.len() >= 2 { &i.clip } else { &full };
    t.push(Tag::i(71, if clip.len() == 2 { 1 } else { 2 }));
    t.push(Tag::i(91, clip.len() as i64));
    for q in clip {
        t.extend([Tag::f(14, q.x), Tag::f(24, q.y)]);
    }
    t
}

/// The image dictionary, IMAGEDEF and IMAGEDEF_REACTOR objects and RASTERVARIABLES; `root` is
/// the named object dictionary, which lists `ACAD_IMAGE_DICT` and `ACAD_IMAGE_VARS`.
pub(crate) fn objects(p: &Plan, root: &str) -> Vec<Tag> {
    let mut t = Vec::new();
    if p.is_empty() {
        return t;
    }
    t.extend([Tag::s(0, "DICTIONARY"), Tag::s(5, &p.dict), Tag::s(330, root), Tag::s(100, "AcDbDictionary"), Tag::i(281, 1)]);
    for d in &p.defs {
        t.extend([Tag::s(3, &d.name), Tag::s(350, &d.handle)]);
    }
    for d in &p.defs {
        t.extend([Tag::s(0, "IMAGEDEF"), Tag::s(5, &d.handle), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, &p.dict)]);
        t.extend(d.reactors.iter().map(|(r, _)| Tag::s(330, r)));
        t.extend([Tag::s(102, "}"), Tag::s(330, &p.dict), Tag::s(100, "AcDbRasterImageDef"), Tag::i(90, 0), Tag::s(1, &d.path)]);
        t.extend([Tag::f(10, d.size.x), Tag::f(20, d.size.y), Tag::f(11, d.pixel_size.x), Tag::f(21, d.pixel_size.y)]);
        t.extend([Tag::i(280, 1), Tag::i(281, i64::from(d.units))]);
        for (r, img) in &d.reactors {
            let ih = img.hex();
            t.extend([Tag::s(0, "IMAGEDEF_REACTOR"), Tag::s(5, r), Tag::s(330, &ih), Tag::s(100, "AcDbRasterImageDefReactor")]);
            t.extend([Tag::i(90, 2), Tag::s(330, ih)]);
        }
    }
    // Image frame shown, high display quality, no insertion units.
    t.extend([Tag::s(0, "RASTERVARIABLES"), Tag::s(5, &p.vars), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, root), Tag::s(102, "}")]);
    t.extend([Tag::s(330, root), Tag::s(100, "AcDbRasterVariables"), Tag::i(90, 0), Tag::i(70, 1), Tag::i(71, 1), Tag::i(72, 0)]);
    t
}
