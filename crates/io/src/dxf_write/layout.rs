use super::ctx::Ctx;
use super::{Bounds2, Drawing, Entity, EntityKind, Handle, HashMap, Layout, Vec2, Version, VportConfig, VportTile, W, dxf_ext};

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
pub(super) fn vport_table(w: &mut W, d: &Drawing, ext: Bounds2, target: Version) {
    // A lone `*Active` tile the file held: its settings, handle and window shape are kept; the view
    // itself is the drawing's extents (CADCraft zooms to them on opening).
    let held = d.active_vport.as_deref();
    let single = VportTile {
        settings: held.map(|t| t.settings.clone()).unwrap_or_default(),
        handle: held.map(|t| t.handle).unwrap_or_default(),
        min: Vec2::ZERO,
        max: Vec2::new(1.0, 1.0),
        center: if ext.is_empty() { Vec2::new(6.0, 4.5) } else { ext.center() },
        height: if ext.is_empty() { 9.0 } else { (ext.height() * 1.1).max(1e-6) },
        aspect: held.map_or(1.6, |t| t.aspect),
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
        let v = &t.settings;
        record_head(w, "VPORT", name, t.handle, &th, "AcDbViewportTableRecord");
        w.s(2, name);
        w.i(70, 0);
        w.p2(10, finite(t.min, Vec2::ZERO));
        w.p2(11, finite(t.max, Vec2::new(1.0, 1.0)));
        w.p2(12, finite(t.center, Vec2::ZERO));
        // Snap grid origin and rotation (SNAPBASE, SNAPANG in degrees): R13+ files keep them here.
        w.p2(13, d.header.point("SNAPBASE").map(|p| p.xy()).filter(|p| p.is_finite()).unwrap_or(Vec2::ZERO));
        w.p2(14, d.header.snap_unit());
        w.p2(15, d.header.grid_unit());
        w.p(16, v.direction);
        w.p(17, v.target);
        w.f(40, if t.height.is_finite() && t.height > 0.0 { t.height } else { 1.0 });
        w.f(41, if t.aspect.is_finite() && t.aspect > 0.0 { t.aspect } else { 1.0 });
        w.f(42, v.lens);
        w.f(43, v.front_clip);
        w.f(44, v.back_clip);
        w.f(50, d.header.f64("SNAPANG", 0.0));
        w.f(51, v.twist);
        vport_settings(w, v, target);
    }
    w.s(0, "ENDTAB");
}

/// A viewport record's view settings after its view (DXF Reference, VPORT): modes, UCS, shading,
/// grid and lighting, each as the file held them.
fn vport_settings(w: &mut W, v: &cadcraft_doc::VportSettings, target: Version) {
    for (c, x) in [
        (71, v.view_mode),
        (72, v.circle_zoom),
        (73, v.fast_zoom),
        (74, v.ucs_icon),
        (75, v.snap_on),
        (76, v.grid_on),
        (77, v.snap_style),
        (78, v.snap_isopair),
    ] {
        w.i(c, i64::from(x));
    }
    if let Some(sheet) = &v.style_sheet {
        w.s(1, sheet.clone());
    }
    w.i(281, i64::from(v.render_mode));
    w.i(65, i64::from(v.ucs_per_viewport));
    w.p(110, v.ucs_origin);
    w.p(111, v.ucs_x);
    w.p(112, v.ucs_y);
    w.i(79, i64::from(v.ortho_type));
    w.f(146, v.elevation);
    for (c, h) in [(345, v.named_ucs), (346, v.base_ucs)] {
        if !h.is_none() {
            w.s(c, h.hex());
        }
    }
    if v.shade_plot != 0 {
        w.i(170, i64::from(v.shade_plot));
    }
    // Grid flags and the default lighting came with AutoCAD 2007.
    if target >= Version::R2007 {
        w.i(60, i64::from(v.grid_flags));
        w.i(61, i64::from(v.major_grid));
    }
    for (c, h) in [(332, v.background), (333, v.shade_plot_object), (348, v.visual_style)] {
        if !h.is_none() {
            w.s(c, h.hex());
        }
    }
    if target >= Version::R2007 {
        w.i(292, i64::from(v.default_lighting));
        w.i(282, i64::from(v.lighting_type));
        w.f(141, v.brightness);
        w.f(142, v.contrast);
    }
    if let Some(a) = v.ambient {
        w.i(63, i64::from(a));
    }
    if let Some(rgb) = v.ambient_rgb {
        w.i(421, rgb);
    }
}

pub(super) fn table_head(w: &mut W, name: &str, count: usize) -> String {
    let h = w.keep(Handle(0), ("TABLE", name));
    w.s(0, "TABLE");
    w.s(2, name);
    w.s(5, h.clone());
    w.s(330, "0");
    w.s(100, "AcDbSymbolTable");
    w.i(70, count as i64);
    h
}

/// A table record's first groups; `kept` is its own handle (0: the one its name had when read).
pub(super) fn record_head(w: &mut W, kind: &str, name: &str, kept: impl Into<Handle>, owner: &str, subclass: &str) -> String {
    let h = w.keep(kept, (kind, name));
    w.s(0, kind);
    w.s(if kind == "DIMSTYLE" { 105 } else { 5 }, h.clone());
    w.s(330, owner);
    w.s(100, "AcDbSymbolTableRecord");
    w.s(100, subclass);
    h
}
