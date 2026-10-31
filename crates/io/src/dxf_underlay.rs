//! PDF underlays in DXF (DXF Reference: PDFUNDERLAY entity, PDFDEFINITION object). The entity
//! holds the placement and display settings and points (group 340) to a PDFDEFINITION object
//! holding the file path (group 1) and the page (group 2), which the `ACAD_PDFDEFINITIONS`
//! dictionary names. The page's size is not stored: it is read from the PDF when the drawing
//! is opened. The scale (groups 41–43) is the files' own: at 1 the page measures its size in inches
//! in drawing units, whatever the drawing's units.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cadcraft_doc::{Drawing, EntityKind, EntityStore, Handle, PdfUnderlay, UNDERLAY_MAX_FADE};
use cadcraft_dxf::{Section, Tag};
use cadcraft_geom::{Vec2, Vec3};

use crate::dxf_image::{each_record, key};

/// Display flags (group 280): clipped, shown, monochrome, colours adjusted for the background,
/// clipped inside the boundary (inverted).
const CLIPPED: i64 = 1;
const ON: i64 = 2;
const MONOCHROME: i64 = 4;
const ADJUST: i64 = 8;
const INVERTED: i64 = 16;
/// The xdata application listing the PDF layers an underlay hides (one 1000 group each), on the
/// PDFUNDERLAY entity.
pub(crate) const LAYERS_APP: &str = "AdeskUnderlayLayerOverrideData";
/// Most clip boundary vertices read.
const MAX_CLIP_VERTICES: usize = 10_000;

/// The PDFUNDERLAY entity's own groups (after the `AcDbUnderlayReference` marker when present).
/// The file and page come from its PDFDEFINITION ([`resolve`]); the size from the file.
pub(crate) fn read_entity(tags: &[Tag]) -> PdfUnderlay {
    let start = tags.iter().position(|t| t.code == 100 && t.str() == "AcDbUnderlayReference").unwrap_or(0);
    let tags = tags.get(start..).unwrap_or(&[]);
    let f = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::f64).filter(|v| v.is_finite());
    let i = |c: i32| tags.iter().find(|t| t.code == c).map(Tag::i64);
    let def = PdfUnderlay::default();
    let flags = i(280).unwrap_or(ON | ADJUST);
    let scale = f(41).map(f64::abs).filter(|s| (1e-9..=1e9).contains(s)).unwrap_or(1.0);
    // Clip boundary: 11/21 vertices (two are a rectangle's opposite corners).
    let mut clip: Vec<Vec2> = Vec::new();
    let mut x = None;
    for t in tags {
        match t.code {
            11 => x = Some(t.f64()),
            21 => {
                if let Some(x) = x.take()
                    && clip.len() < MAX_CLIP_VERTICES
                {
                    clip.push(Vec2::new(x, t.f64()));
                }
            }
            _ => {}
        }
    }
    if !clip.iter().all(|p| p.is_finite()) {
        clip.clear();
    }
    if let [a, b] = clip[..] {
        clip = vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)];
    }
    if clip.len() < 3 {
        clip.clear();
    }
    PdfUnderlay {
        insert: Vec3::new(f(10).unwrap_or(0.0), f(20).unwrap_or(0.0), f(30).unwrap_or(0.0)),
        size: Vec2::ZERO,
        scale,
        rotation: f(50).unwrap_or(0.0).to_radians(),
        fade: i(282).and_then(|v| u8::try_from(v.clamp(0, i64::from(UNDERLAY_MAX_FADE))).ok()).unwrap_or(def.fade),
        contrast: i(281).and_then(|v| u8::try_from(v.clamp(0, 100)).ok()).unwrap_or(def.contrast),
        adjust_colors: flags & ADJUST != 0,
        monochrome: flags & MONOCHROME != 0,
        hidden: flags & ON == 0,
        clipping: flags & CLIPPED != 0,
        clip_inverted: flags & INVERTED != 0 && !clip.is_empty(),
        clip,
        layers: crate::dxf_ext::xdata(tags, LAYERS_APP)
            .iter()
            .filter(|t| t.code == 1000)
            .map(|t| t.str().trim().to_string())
            .filter(|n| !n.is_empty())
            .take(MAX_CLIP_VERTICES)
            .map(|name| cadcraft_doc::UnderlayLayer { name, on: false })
            .collect(),
        ..def
    }
}

fn has_underlays(store: &EntityStore) -> bool {
    store.iter().any(|e| matches!(e.kind, EntityKind::PdfUnderlay(_)))
}

/// Whether an underlay of `d` hides PDF layers, so the file needs [`LAYERS_APP`] registered.
pub(crate) fn hides_layers(d: &Drawing) -> bool {
    let hides = |store: &EntityStore| store.iter().any(|e| matches!(&e.kind, EntityKind::PdfUnderlay(u) if u.layers.iter().any(|l| !l.on)));
    hides(&d.model) || d.layouts.iter().any(|l| hides(&l.entities)) || d.blocks.values().any(|b| hides(&b.entities))
}

/// How many PDF underlays `d` holds, in model space, layouts and blocks.
fn count_underlays(d: &Drawing) -> usize {
    let n = |store: &EntityStore| store.iter().filter(|e| matches!(e.kind, EntityKind::PdfUnderlay(_))).count();
    n(&d.model) + d.layouts.iter().map(|l| n(&l.entities)).sum::<usize>() + d.blocks.values().map(|b| n(&b.entities)).sum::<usize>()
}

/// Fill in each underlay's file and page from the PDFDEFINITION it points to.
pub(crate) fn resolve(secs: &[Section], d: &mut Drawing) {
    if !has_underlays(&d.model) && !d.layouts.iter().any(|l| has_underlays(&l.entities)) && !d.blocks.values().any(|b| has_underlays(&b.entities)) {
        return;
    }
    // PDFUNDERLAY handle → PDFDEFINITION handle.
    let mut refs: HashMap<Handle, String> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "ENTITIES" || s.name == "BLOCKS") {
        each_record(&s.tags, |kind, tags| {
            if kind != "PDFUNDERLAY" {
                return;
            }
            let h = tags.iter().find(|t| t.code == 5).and_then(|t| Handle::parse_hex(&t.str()));
            let def = tags.iter().find(|t| t.code == 340).map(|t| key(&t.str()));
            if let (Some(h), Some(def)) = (h, def) {
                refs.insert(h, def);
            }
        });
    }
    // PDFDEFINITION handle → (file, page, unloaded).
    let mut defs: HashMap<String, (String, u32, bool)> = HashMap::new();
    for s in secs.iter().filter(|s| s.name == "OBJECTS") {
        each_record(&s.tags, |kind, tags| {
            if kind != "PDFDEFINITION" {
                return;
            }
            let Some(h) = tags.iter().find(|t| t.code == 5).map(|t| key(&t.str())) else { return };
            let path = tags.iter().find(|t| t.code == 1).map(Tag::str).unwrap_or_default();
            let page = tags.iter().find(|t| t.code == 2).and_then(|t| t.str().trim().parse::<u32>().ok()).filter(|p| *p >= 1).unwrap_or(1);
            // Unloaded: `1001 ACAD` `1000 NOLOAD` on the definition.
            let unloaded = crate::dxf_ext::xdata(tags, "ACAD").iter().any(|t| t.code == 1000 && t.str().trim().eq_ignore_ascii_case("NOLOAD"));
            defs.insert(h, (path, page, unloaded));
        });
    }
    let by_underlay: HashMap<Handle, &(String, u32, bool)> = refs.iter().filter_map(|(u, dh)| Some((*u, defs.get(dh)?))).collect();
    // An underlay whose definition is missing has no file: say why its page stays blank.
    match count_underlays(d).saturating_sub(by_underlay.len()) {
        0 => {}
        1 => d.read_notes.push("PDF underlays: 1 underlay names no PDF definition in the file, so its PDF is unknown".into()),
        n => d.read_notes.push(format!("PDF underlays: {n} underlays name no PDF definition in the file, so their PDF is unknown")),
    }
    if by_underlay.is_empty() {
        return;
    }
    // Aggregate budget on the definition paths copied into referring underlays, so a long path
    // referenced many times can't grow into gigabytes (as for images, CWE-400).
    let mut budget = MAX_RESOLVED_PATH_BYTES;
    fill(&mut d.model, &by_underlay, &mut budget);
    for l in &mut d.layouts {
        fill(&mut l.entities, &by_underlay, &mut budget);
    }
    for b in d.blocks.values_mut() {
        if b.entities.iter().any(|e| by_underlay.contains_key(&e.handle)) {
            fill(&mut Arc::make_mut(b).entities, &by_underlay, &mut budget);
        }
    }
}

/// Most bytes of definition paths copied into a drawing's underlays.
const MAX_RESOLVED_PATH_BYTES: usize = 64 << 20;

/// Give the underlays of `store` their definition's file, page and state. Once `budget` is spent
/// the paths are no longer copied; the page and state still are.
fn fill(store: &mut EntityStore, by_underlay: &HashMap<Handle, &(String, u32, bool)>, budget: &mut usize) {
    let hs: Vec<Handle> =
        store.iter().filter(|e| matches!(e.kind, EntityKind::PdfUnderlay(_)) && by_underlay.contains_key(&e.handle)).map(|e| e.handle).collect();
    for h in hs {
        let Some((path, page, unloaded)) = by_underlay.get(&h) else { continue };
        let copy_path = budget.checked_sub(path.len()).map(|left| *budget = left).is_some();
        store.modify(h, |e| {
            if let EntityKind::PdfUnderlay(u) = &mut e.kind {
                if copy_path {
                    u.path = path.clone();
                }
                u.page = *page;
                u.unloaded = *unloaded;
            }
        });
    }
}

/// One PDFDEFINITION to write, shared by the underlays of the same file, page and state.
struct PlannedDef {
    handle: String,
    name: String,
    path: String,
    page: u32,
    /// Unloaded: an underlay keeps no state of its own, so a loaded and an unloaded underlay of
    /// one page get a definition each.
    unloaded: bool,
    /// The underlays using it, gathered while planning so writing doesn't scan every underlay
    /// per definition (CWE-407).
    users: Vec<Handle>,
}

/// The objects behind a drawing's PDF underlays, with handles allocated before anything is written.
#[derive(Default)]
pub(crate) struct Plan {
    /// PDFUNDERLAY entity → PDFDEFINITION handle.
    pub by_entity: HashMap<Handle, String>,
    defs: Vec<PlannedDef>,
    /// The `ACAD_PDFDEFINITIONS` dictionary.
    pub dict: String,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

/// A definition's dictionary name: the file name without extension, then the page.
fn def_name(u: &PdfUnderlay) -> String {
    let file = u.path.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).trim();
    let stem = if stem.is_empty() { "PDF" } else { stem };
    format!("{stem} - {}", u.page)
}

/// Allocate the PDFDEFINITION and dictionary handles for every PDF underlay.
pub(crate) fn plan<'a>(entities: impl Iterator<Item = &'a cadcraft_doc::Entity>, mut next: impl FnMut() -> String) -> Plan {
    let mut p = Plan::default();
    let mut index: HashMap<(String, u32, bool), usize> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    // The next suffix to try per name, so many files of one name aren't probed from 2 each time.
    let mut suffix: HashMap<String, usize> = HashMap::new();
    for e in entities {
        let EntityKind::PdfUnderlay(u) = &e.kind else { continue };
        if p.by_entity.contains_key(&e.handle) {
            continue;
        }
        let k = (u.path.clone(), u.page, u.unloaded);
        let at = match index.get(&k) {
            Some(&at) => at,
            None => {
                // Dictionary keys are unique regardless of case.
                let base = def_name(u);
                let mut name = base.clone();
                let n = suffix.entry(key(&base)).or_insert(2);
                while !used.insert(key(&name)) {
                    name = format!("{base}_{n}");
                    *n = n.saturating_add(1);
                }
                p.defs.push(PlannedDef { handle: next(), name, path: u.path.clone(), page: u.page.max(1), unloaded: u.unloaded, users: Vec::new() });
                let at = p.defs.len().saturating_sub(1);
                index.insert(k, at);
                at
            }
        };
        if let Some(def) = p.defs.get_mut(at) {
            p.by_entity.insert(e.handle, def.handle.clone());
            def.users.push(e.handle);
        }
    }
    if !p.defs.is_empty() {
        p.dict = next();
    }
    p
}

/// The PDFUNDERLAY entity's groups after its common entity groups.
pub(crate) fn entity_tags(u: &PdfUnderlay, def: &str) -> Vec<Tag> {
    let mut flags = if u.hidden { 0 } else { ON };
    if u.monochrome {
        flags |= MONOCHROME;
    }
    if u.adjust_colors {
        flags |= ADJUST;
    }
    let clip = if u.clip.len() >= 3 { &u.clip[..] } else { &[] };
    // Files keep the clipping bit on an underlay without a boundary too (280 = 11 when
    // attached; "Show clip" No clears it).
    if u.clipping {
        flags |= CLIPPED;
    }
    if u.clip_inverted && !clip.is_empty() {
        flags |= INVERTED;
    }
    let mut t = vec![
        Tag::s(340, def),
        Tag::f(10, u.insert.x),
        Tag::f(20, u.insert.y),
        Tag::f(30, u.insert.z),
        Tag::f(41, u.scale),
        Tag::f(42, u.scale),
        Tag::f(43, u.scale),
        Tag::f(50, u.rotation.to_degrees()),
        Tag::f(210, 0.0),
        Tag::f(220, 0.0),
        Tag::f(230, 1.0),
        Tag::i(280, flags),
        Tag::i(281, i64::from(u.contrast.min(100))),
        Tag::i(282, i64::from(u.fade.min(UNDERLAY_MAX_FADE))),
    ];
    // A rectangle aligned with the page is written as two opposite corners, the usual form of
    // a rectangular boundary.
    let written: Vec<Vec2> = match clip {
        [a, b, c, d] if a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x => vec![*a, *c],
        _ => clip.to_vec(),
    };
    if !written.is_empty() {
        t.push(Tag::i(91, written.len() as i64));
        for p in &written {
            t.extend([Tag::f(11, p.x), Tag::f(21, p.y)]);
        }
    }
    // The PDF layers it hides (none: no xdata at all).
    let hidden: Vec<&str> = u.layers.iter().filter(|l| !l.on).map(|l| l.name.as_str()).collect();
    if !hidden.is_empty() {
        t.push(Tag::s(1001, LAYERS_APP));
        t.extend(hidden.into_iter().map(|n| Tag::s(1000, n)));
    }
    t
}

/// The `ACAD_PDFDEFINITIONS` dictionary and its PDFDEFINITION objects; `root` is the named
/// object dictionary, which lists the dictionary.
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
        // Its reactors: the dictionary, then the underlays using it (a PDF whose definition
        // names none is listed as unreferenced).
        t.extend([Tag::s(0, "PDFDEFINITION"), Tag::s(5, &d.handle), Tag::s(102, "{ACAD_REACTORS"), Tag::s(330, &p.dict)]);
        let mut users = d.users.clone();
        users.sort_unstable();
        t.extend(users.iter().map(|h| Tag::s(330, h.hex())));
        t.push(Tag::s(102, "}"));
        t.extend([Tag::s(330, &p.dict), Tag::s(100, "AcDbUnderlayDefinition"), Tag::s(1, &d.path), Tag::s(2, d.page.to_string())]);
        if d.unloaded {
            t.extend([Tag::s(1001, "ACAD"), Tag::s(1000, "NOLOAD")]);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Common, Entity, Space};

    fn drawing() -> Drawing {
        let mut d = Drawing::new_metric();
        let u = |path: &str, page, x| PdfUnderlay {
            insert: Vec3::new(x, 20.0, 0.0),
            size: Vec2::new(297.0, 210.0),
            scale: 50.0,
            rotation: 0.5,
            path: path.into(),
            page,
            fade: 30,
            adjust_colors: false,
            contrast: 70,
            monochrome: true,
            clip: Vec::new(),
            clipping: false,
            clip_inverted: false,
            layers: Vec::new(),
            unloaded: false,
            hidden: false,
        };
        // A polygonal clip, and an inverted one switched off.
        let tri = vec![Vec2::new(1.0, 1.0), Vec2::new(8.0, 1.5), Vec2::new(4.0, 6.25)];
        // It hides two PDF layers.
        let hidden = vec![
            cadcraft_doc::UnderlayLayer { name: "COTES".into(), on: false },
            cadcraft_doc::UnderlayLayer { name: "Mobilier 2".into(), on: false },
        ];
        let clipped = PdfUnderlay { clip: tri.clone(), clipping: true, layers: hidden, ..u("/plans/Façades.pdf", 2, 10.0) };
        let inverted = PdfUnderlay { clip: tri, clipping: false, clip_inverted: true, ..u("/plans/Façades.pdf", 2, 5000.0) };
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(clipped)).ok();
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(inverted)).ok();
        // Its file unloaded.
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(PdfUnderlay { unloaded: true, ..u(r"C:\plans\RDC.pdf", 1, 0.0) })).ok();
        d
    }

    fn underlays(d: &Drawing) -> Vec<PdfUnderlay> {
        d.model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::PdfUnderlay(u) => Some(u.clone()),
                _ => None,
            })
            .collect()
    }

    /// What a file keeps: everything but the page size, read again from the PDF on opening.
    fn saved(mut u: PdfUnderlay) -> PdfUnderlay {
        u.size = Vec2::ZERO;
        u
    }

    #[test]
    fn underlays_survive_dxf_save_and_reopen() {
        let d = drawing();
        let bytes = crate::write(&d, "plan.dxf").unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let objects = lines.windows(2).filter(|w| w == &["0", "PDFDEFINITION"]).count();
        assert_eq!(objects, 2, "one definition per file and page");
        assert!(text.contains("ACAD_PDFDEFINITIONS"));
        let back = crate::read(&bytes, "plan.dxf").unwrap();
        let (a, b) = (underlays(&d), underlays(&back));
        assert_eq!(b.len(), 3);
        for (a, b) in a.into_iter().zip(b) {
            let a = saved(a);
            assert!((a.rotation - b.rotation).abs() < 1e-9, "{a:?} {b:?}");
            assert_eq!(PdfUnderlay { rotation: 0.0, ..a }, PdfUnderlay { rotation: 0.0, ..b });
        }
    }

    #[test]
    fn underlays_survive_dwg_save_and_reopen() {
        let d = drawing();
        let bytes = crate::write(&d, "plan.dwg").unwrap();
        let back = crate::read(&bytes, "plan.dwg").unwrap();
        let b = underlays(&back);
        assert_eq!(b.len(), 3, "{:?}", back.model.iter().map(|e| e.kind.dxf_name().to_string()).collect::<Vec<_>>());
        let first = b.iter().find(|u| u.insert.x == 10.0).unwrap();
        assert_eq!((first.path.as_str(), first.page, first.scale, first.fade), ("/plans/Façades.pdf", 2, 50.0, 30));
        assert!(!first.adjust_colors && first.monochrome);
        assert!(first.clipping && !first.clip_inverted && first.clip.len() == 3, "{:?}", first.clip);
        let second = b.iter().find(|u| u.insert.x == 5000.0).unwrap();
        assert!(!second.clipping && second.clip_inverted && second.clip.len() == 3);
        assert_eq!(first.layers.iter().map(|l| (l.name.as_str(), l.on)).collect::<Vec<_>>(), vec![("COTES", false), ("Mobilier 2", false)]);
        assert!(second.layers.is_empty() && !second.unloaded && !first.unloaded);
        assert!(b.iter().find(|u| u.insert.x == 0.0).unwrap().unloaded, "the unloaded file stays unloaded");
    }

    #[test]
    fn hidden_layers_and_unloading_are_written_in_the_format_s_groups() {
        let bytes = crate::write(&drawing(), "plan.dxf").unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let pairs: Vec<(&str, &str)> = lines.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)).collect();
        // On the entity: 1001 with LAYERS_APP, then one 1000 per hidden layer.
        let at = pairs.iter().position(|p| *p == ("1001", LAYERS_APP)).unwrap();
        assert_eq!(&pairs[at + 1..at + 3], &[("1000", "COTES"), ("1000", "Mobilier 2")]);
        assert_eq!(pairs.iter().filter(|p| **p == ("1001", LAYERS_APP)).count(), 1, "only the underlay that hides layers carries it");
        // On the unloaded file's definition: 1001 ACAD, 1000 NOLOAD.
        assert!(pairs.windows(2).any(|w| w == [("1001", "ACAD"), ("1000", "NOLOAD")]));
        assert!(pairs.contains(&("2", LAYERS_APP)), "the application is registered");
    }

    /// A DWF or DGN underlay, which CADCraft keeps without reading, keeps its layer overrides.
    #[test]
    fn another_underlay_keeps_its_layer_override_xdata() {
        for kind in ["DWFUNDERLAY", "DGNUNDERLAY"] {
            let text = format!(
                "0\nSECTION\n2\nENTITIES\n0\n{kind}\n5\n2B\n100\nAcDbEntity\n8\n0\n100\nAcDbUnderlayReference\n\
                 10\n0.0\n20\n0.0\n30\n0.0\n1001\n{LAYERS_APP}\n1000\nCOTES\n0\nENDSEC\n0\nEOF\n"
            );
            let d = crate::read(text.as_bytes(), "plan.dxf").unwrap();
            let out = String::from_utf8(crate::write(&d, "plan.dxf").unwrap()).unwrap();
            let lines: Vec<&str> = out.lines().map(str::trim).collect();
            assert!(lines.windows(4).any(|w| w == ["1001", LAYERS_APP, "1000", "COTES"]), "{kind}: its overrides kept");
            assert!(lines.windows(2).any(|w| w == ["2", LAYERS_APP]), "{kind}: their application registered");
        }
    }

    /// A drawing with no underlay hiding PDF layers doesn't register their application.
    #[test]
    fn the_layers_application_is_registered_only_when_an_underlay_hides_layers() {
        let registered = |d: &Drawing| {
            let bytes = crate::write(d, "plan.dxf").unwrap();
            let text = String::from_utf8_lossy(&bytes).to_string();
            text.lines().map(str::trim).any(|l| l == LAYERS_APP)
        };
        assert!(!registered(&Drawing::new_metric()), "no underlay");
        let mut d = Drawing::new_metric();
        let u = PdfUnderlay { path: "/plans/a.pdf".into(), size: Vec2::new(297.0, 210.0), ..PdfUnderlay::default() };
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
        assert!(!registered(&d), "an underlay showing all its layers");
        assert!(registered(&drawing()), "an underlay hiding layers");
    }

    /// An underlay whose PDFDEFINITION is gone reads with a note saying so; a whole file reads without one.
    #[test]
    fn an_underlay_without_its_definition_is_noted() {
        let mut d = Drawing::new_metric();
        let u = PdfUnderlay { path: "/plans/a.pdf".into(), size: Vec2::new(297.0, 210.0), ..PdfUnderlay::default() };
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).unwrap();
        let text = String::from_utf8(crate::write(&d, "plan.dxf").unwrap()).unwrap();
        assert!(crate::read(text.as_bytes(), "plan.dxf").unwrap().read_notes.is_empty());
        // The PDFDEFINITION record removed, up to the next object.
        let lines: Vec<&str> = text.lines().collect();
        let at =
            lines.chunks(2).position(|p| p.first().map(|c| c.trim()) == Some("0") && p.get(1).map(|v| v.trim()) == Some("PDFDEFINITION")).unwrap();
        let len = lines.chunks(2).skip(at + 1).position(|p| p.first().map(|c| c.trim()) == Some("0")).unwrap() + 1;
        let cut = [&lines[..at * 2], &lines[(at + len) * 2..]].concat().join("\n");
        let back = crate::read(cut.as_bytes(), "plan.dxf").unwrap();
        assert_eq!(back.read_notes, ["PDF underlays: 1 underlay names no PDF definition in the file, so its PDF is unknown"]);
    }

    /// Two underlays of one page, one of them unloaded, each come back in their own state.
    #[test]
    fn a_loaded_and_an_unloaded_underlay_of_one_page_keep_their_own_state() {
        let mut d = Drawing::new_metric();
        let one = |x: f64, unloaded: bool| PdfUnderlay {
            insert: Vec3::new(x, 0.0, 0.0),
            path: "/plans/RDC.pdf".into(),
            page: 1,
            scale: 1.0,
            unloaded,
            ..PdfUnderlay::default()
        };
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(one(0.0, false))).ok();
        d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(one(100.0, true))).ok();
        for name in ["plan.dxf", "plan.dwg"] {
            let back = crate::read(&crate::write(&d, name).unwrap(), name).unwrap();
            let state: Vec<(f64, bool)> = underlays(&back).iter().map(|u| (u.insert.x, u.unloaded)).collect();
            assert_eq!(state, vec![(0.0, false), (100.0, true)], "{name}");
        }
    }

    /// An External References palette lists a PDF whose definition names no underlay as
    /// unreferenced: each PDFDEFINITION's reactors are its dictionary, then the underlays using it.
    #[test]
    fn each_definition_lists_the_underlays_using_it_as_reactors() {
        let bytes = crate::write(&drawing(), "plan.dxf").unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let pairs: Vec<(&str, &str)> = lines.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)).collect();
        // Each object's pairs, from its 0 group to the next.
        let objects: Vec<&[(&str, &str)]> = pairs.split_inclusive(|p| p.0 == "0").collect();
        let starting = |kind: &str| -> Vec<&[(&str, &str)]> {
            (1..objects.len()).filter(|i| objects[i - 1].last() == Some(&("0", kind))).map(|i| objects[i]).collect()
        };
        let value = |o: &[(&str, &str)], code: &str| o.iter().find(|p| p.0 == code).map(|p| p.1.to_string()).unwrap();
        let underlays: Vec<(String, String)> = starting("PDFUNDERLAY").iter().map(|o| (value(o, "5"), value(o, "340"))).collect();
        assert_eq!(underlays.len(), 3);
        let defs = starting("PDFDEFINITION");
        assert_eq!(defs.len(), 2);
        for o in defs {
            let def = value(o, "5");
            let at = o.iter().position(|p| *p == ("102", "{ACAD_REACTORS")).unwrap();
            let end = o.iter().position(|p| *p == ("102", "}")).unwrap();
            let reactors: Vec<String> = o[at + 1..end].iter().map(|p| p.1.to_string()).collect();
            let mut want = vec![value(o, "330")];
            want.extend(underlays.iter().filter(|(_, d)| *d == def).map(|(h, _)| h.clone()));
            assert_eq!(reactors, want, "definition {def}");
        }
    }

    #[test]
    fn a_two_vertex_clip_boundary_is_a_rectangle() {
        let tags = [Tag::i(280, CLIPPED | ON), Tag::i(91, 2), Tag::f(11, 1.0), Tag::f(21, 2.0), Tag::f(11, 5.0), Tag::f(21, 4.0)];
        let u = read_entity(&tags);
        assert!(u.clipping && !u.clip_inverted);
        assert_eq!(u.clip, vec![Vec2::new(1.0, 2.0), Vec2::new(5.0, 2.0), Vec2::new(5.0, 4.0), Vec2::new(1.0, 4.0)]);
        // Written back as two corners, the usual form of a rectangular boundary.
        let t = entity_tags(&u, "1F");
        assert!(t.iter().any(|t| t.code == 91 && t.i64() == 2));
        assert_eq!(t.iter().filter(|t| t.code == 11).count(), 2);
        // A boundary of one vertex encloses nothing: dropped, and with it the clipping.
        let tags = [Tag::i(280, CLIPPED | INVERTED | ON), Tag::i(91, 1), Tag::f(11, 5.0), Tag::f(21, 4.0), Tag::f(11, 7.0)];
        let u = read_entity(&tags);
        assert!(!u.clip_inverted && u.clip.is_empty());
    }

    #[test]
    fn an_underlay_not_shown_keeps_its_shown_bit_off() {
        // "Show underlay" No with monochrome on: 280 = 4.
        let u = read_entity(&[Tag::i(280, MONOCHROME)]);
        assert!(u.hidden && u.monochrome && !u.adjust_colors);
        let flags = |u: &PdfUnderlay| entity_tags(u, "1F").iter().find(|t| t.code == 280).map(Tag::i64);
        assert_eq!(flags(&u).map(|f| f & ON), Some(0));
        assert_eq!(flags(&PdfUnderlay { hidden: false, ..u }).map(|f| f & ON), Some(ON));
        assert!(!read_entity(&[Tag::i(280, ON | ADJUST | CLIPPED)]).hidden);
        // A new underlay shows its clip (280 = 11); "Show clip" No: 10.
        let new = PdfUnderlay::default();
        assert_eq!(flags(&new), Some(11));
        assert_eq!(flags(&PdfUnderlay { clipping: false, ..new }), Some(10));
        assert!(!read_entity(&[Tag::i(280, 10)]).clipping && read_entity(&[Tag::i(280, 11)]).clipping);
    }

    /// Each definition lists exactly its own underlays, whatever the mix of shared and distinct files.
    #[test]
    fn each_definition_lists_its_own_underlays_as_reactors() {
        let u = |path: &str, page| PdfUnderlay { path: path.into(), page, ..PdfUnderlay::default() };
        let es = [
            Entity::new(Handle(0x30), EntityKind::PdfUnderlay(u("/a.pdf", 1))),
            Entity::new(Handle(0x10), EntityKind::PdfUnderlay(u("/b.pdf", 1))),
            Entity::new(Handle(0x20), EntityKind::PdfUnderlay(u("/a.pdf", 1))),
            Entity::new(Handle(0x40), EntityKind::PdfUnderlay(u("/a.pdf", 2))),
            Entity::new(Handle(0x50), EntityKind::PdfUnderlay(PdfUnderlay { unloaded: true, ..u("/a.pdf", 1) })),
        ];
        let mut n = 0x100u64;
        let p = plan(es.iter(), || {
            n += 1;
            Handle(n).hex()
        });
        let tags = objects(&p, "C");
        // Per PDFDEFINITION: its handle, then the 330 groups inside its reactor block.
        let mut reactors: Vec<(String, Vec<String>)> = Vec::new();
        let mut inside = false;
        for (i, t) in tags.iter().enumerate() {
            if t.code == 0 && t.str() == "PDFDEFINITION" {
                let h = tags.get(i + 1).map(Tag::str).unwrap_or_default();
                reactors.push((h, Vec::new()));
            } else if t.code == 102 {
                inside = t.str() == "{ACAD_REACTORS";
            } else if inside
                && t.code == 330
                && let Some((_, r)) = reactors.last_mut()
            {
                r.push(t.str());
            }
        }
        let def_of = |h: u64| p.by_entity.get(&Handle(h)).cloned().unwrap_or_default();
        let users = |def: &str| reactors.iter().find(|(d, _)| d == def).map(|(_, r)| r.clone()).unwrap_or_default();
        // The dictionary first, then the underlays in handle order.
        assert_eq!(users(&def_of(0x30)), vec![p.dict.clone(), "20".into(), "30".into()]);
        assert_eq!(def_of(0x20), def_of(0x30));
        assert_eq!(users(&def_of(0x10)), vec![p.dict.clone(), "10".into()]);
        assert_eq!(users(&def_of(0x40)), vec![p.dict.clone(), "40".into()]);
        assert_eq!(users(&def_of(0x50)), vec![p.dict.clone(), "50".into()]);
        assert_eq!(reactors.len(), 4);
    }

    /// A file with one PDF per underlay (one definition each) is written in linear time: each
    /// definition's users are gathered while planning, not by scanning every underlay (CWE-407).
    #[test]
    fn many_underlays_of_distinct_files_are_written_quickly() {
        let es: Vec<Entity> = (1..=60_000u64)
            .map(|i| Entity::new(Handle(i), EntityKind::PdfUnderlay(PdfUnderlay { path: format!("/p{i}.pdf"), page: 1, ..PdfUnderlay::default() })))
            .collect();
        let mut n = 0x100_000u64;
        let t = std::time::Instant::now();
        let p = plan(es.iter(), || {
            n += 1;
            Handle(n).hex()
        });
        let tags = objects(&p, "C");
        assert!(t.elapsed().as_secs() < 5, "took {:?}", t.elapsed());
        assert_eq!(tags.iter().filter(|t| t.code == 0 && t.str() == "PDFDEFINITION").count(), 60_000);
    }

    #[test]
    fn a_long_path_shared_by_many_underlays_is_copied_within_a_budget() {
        let mut d = Drawing::new_metric();
        let path = format!("/{}.pdf", "p".repeat(1 << 20));
        for i in 0..100 {
            let u = PdfUnderlay { insert: Vec3::new(f64::from(i), 0.0, 0.0), path: path.clone(), page: 2, ..PdfUnderlay::default() };
            d.add(&Space::Model, Common::default(), EntityKind::PdfUnderlay(u)).ok();
        }
        let bytes = crate::write(&d, "plan.dxf").unwrap();
        let back = underlays(&crate::read(&bytes, "plan.dxf").unwrap());
        assert_eq!(back.len(), 100);
        assert!(back.iter().map(|u| u.path.len()).sum::<usize>() <= MAX_RESOLVED_PATH_BYTES);
        assert!(back.first().is_some_and(|u| u.path == path), "the first ones keep their path");
        assert!(back.iter().all(|u| u.page == 2), "every underlay keeps its page");
    }

    #[test]
    fn hostile_underlay_groups_are_tamed() {
        let tags = [Tag::f(41, f64::NAN), Tag::f(10, f64::INFINITY), Tag::i(282, 999), Tag::i(281, -5), Tag::i(280, -1)];
        let u = read_entity(&tags);
        assert_eq!((u.scale, u.insert.x, u.fade, u.contrast), (1.0, 0.0, UNDERLAY_MAX_FADE, 0));
    }
}
