use super::ctx::Ctx;
use super::{Bounds2, Drawing, Entity, EntityKind, Handle, HashMap, Layout, Vec2, Vec3, VportConfig, VportTile, W, dxf_ext};

/// Layouts with viewports but without the paper-space viewport (id 1, the sheet itself), keyed
/// by their first viewport. AutoCAD and DWG take a layout's first viewport as its paper-space
/// viewport (DWG does not store viewport ids; readers number them in order), so a layout made in
/// CADCraft, whose paper-space viewport is implicit, gets one written ahead of its viewports.
pub(super) fn paper_views(d: &Drawing) -> HashMap<Handle, usize> {
    let mut out = HashMap::new();
    'layouts: for (i, l) in d.layouts.iter().enumerate() {
        let mut first = None;
        for e in l.entities.iter() {
            if let EntityKind::Viewport(v) = &e.kind {
                if v.id == 1 {
                    continue 'layouts;
                }
                first.get_or_insert(e.handle);
            }
        }
        if let Some(h) = first {
            out.insert(h, i);
        }
    }
    out
}

/// A layout's entities with its paper-space viewport (id 1) first. DWG stores no viewport ids and
/// readers number a layout's viewports in order, so the sheet must lead even when the draw order
/// (SORTENTS) put it behind a model viewport; otherwise that viewport becomes the sheet.
pub(super) fn sheet_first(l: &Layout) -> Vec<&Entity> {
    let sheet = |e: &&Entity| matches!(&e.kind, EntityKind::Viewport(v) if v.id == 1);
    let all = || l.entities.iter().map(|e| e.as_ref());
    all().filter(sheet).chain(all().filter(|e| !sheet(e))).collect()
}

/// The paper-space viewport (id 1) of the layout whose first viewport is `e`, if it needs one
/// ([`paper_views`]): the whole sheet, viewed 1:1. Marked with CADCraft xdata so that our reader
/// leaves it implicit again.
pub(super) fn paper_view(w: &mut W, d: &Drawing, e: &Entity, owner: &str, cx: &Ctx) {
    let Some(l) = cx.paper_views.get(&e.handle).and_then(|i| d.layouts.get(*i)) else { return };
    let size = cadcraft_render::paper::sheet(d, &l.name).map(|s| s.size).unwrap_or(Vec2::new(12.0, 9.0));
    let center = size * 0.5;
    let h = w.h();
    w.s(0, "VIEWPORT");
    w.s(5, h);
    w.s(330, owner);
    w.s(100, "AcDbEntity");
    w.i(67, 1);
    w.s(8, "0");
    w.s(100, "AcDbViewport");
    w.p(10, center.to3(0.0));
    w.f(40, size.x);
    w.f(41, size.y);
    w.i(68, 1);
    w.i(69, 1);
    w.p2(12, center);
    w.f(45, size.y);
    w.i(90, 32768);
    w.s(1001, dxf_ext::APP);
    w.s(1000, dxf_ext::PAPER_VIEW);
}

/// The VPORT table: the `*Active` model space tiles (one viewport on the extents when there is no
/// tiled configuration) and the saved configurations, each tile a record, the current one first.
pub(super) fn vport_table(w: &mut W, d: &Drawing, ext: Bounds2) {
    let single = VportTile {
        min: Vec2::ZERO,
        max: Vec2::new(1.0, 1.0),
        center: if ext.is_empty() { Vec2::new(6.0, 4.5) } else { ext.center() },
        height: if ext.is_empty() { 9.0 } else { (ext.height() * 1.1).max(1e-6) },
        aspect: 1.6,
    };
    let tiled = d.vports.iter().find(|c| c.is_active() && c.tiles.len() > 1);
    let mut records: Vec<(&str, &VportTile)> = match tiled {
        Some(c) => c.tiles.iter().map(|t| (VportConfig::ACTIVE, t)).collect(),
        None => vec![(VportConfig::ACTIVE, &single)],
    };
    records.extend(d.vports.iter().filter(|c| !c.is_active() && !c.name.is_empty()).flat_map(|c| c.tiles.iter().map(|t| (c.name.as_str(), t))));
    let th = table_head(w, "VPORT", records.len());
    let finite = |p: Vec2, or: Vec2| if p.is_finite() { p } else { or };
    for (name, t) in records {
        record_head(w, "VPORT", &th, "AcDbViewportTableRecord");
        w.s(2, name);
        w.i(70, 0);
        w.p2(10, finite(t.min, Vec2::ZERO));
        w.p2(11, finite(t.max, Vec2::new(1.0, 1.0)));
        w.p2(12, finite(t.center, Vec2::ZERO));
        // Snap grid origin and rotation (SNAPBASE, SNAPANG in degrees): R13+ files keep them here.
        w.p2(13, d.header.point("SNAPBASE").map(|p| p.xy()).filter(|p| p.is_finite()).unwrap_or(Vec2::ZERO));
        w.p2(14, d.header.snap_unit());
        w.p2(15, d.header.grid_unit());
        w.p(16, Vec3::Z);
        w.p(17, Vec3::ZERO);
        w.f(40, if t.height.is_finite() && t.height > 0.0 { t.height } else { 1.0 });
        w.f(41, if t.aspect.is_finite() && t.aspect > 0.0 { t.aspect } else { 1.0 });
        w.f(42, 50.0);
        w.f(50, d.header.f64("SNAPANG", 0.0));
    }
    w.s(0, "ENDTAB");
}

pub(super) fn table_head(w: &mut W, name: &str, count: usize) -> String {
    let h = w.h();
    w.s(0, "TABLE");
    w.s(2, name);
    w.s(5, h.clone());
    w.s(330, "0");
    w.s(100, "AcDbSymbolTable");
    w.i(70, count as i64);
    h
}

pub(super) fn record_head(w: &mut W, kind: &str, owner: &str, subclass: &str) -> String {
    let h = w.h();
    w.s(0, kind);
    w.s(if kind == "DIMSTYLE" { 105 } else { 5 }, h.clone());
    w.s(330, owner);
    w.s(100, "AcDbSymbolTableRecord");
    w.s(100, subclass);
    h
}
