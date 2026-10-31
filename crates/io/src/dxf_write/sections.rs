use super::assoc::std_assoc_refs;
use super::blocks::{arrow_block_entities, dim_block_entities, dim_block_plan, reuse_handles, table_block_entities};
use super::common::annotative_xdata;
use super::ctx::{AssocObj, Ctx};
use super::entity::{entity, hatch_reactors};
use super::header::{code_page, header_vars};
use super::layout::{paper_views, record_head, sheet_first, table_head, vport_table};
use super::objects::{cadcraft_fonts_name, group_obj, mleader_style_obj, scale_list, table_style_obj};
use super::{
    Block, Color, DimStyle, DimVal, Drawing, Entity, EntityKind, EntityStore, Group, HVal, Handle, HashMap, Layout, MLeaderStyle, PageSetup, Space,
    TableStyle, Tag, Vec2, Vec3, Version, W, dxf_ext,
};

struct TableWritePlan<'a> {
    d: &'a Drawing,
    version: Version,
    cx: &'a mut Ctx,
    ext: cadcraft_geom::Bounds2,
}

fn write_tables(w: &mut W, plan: TableWritePlan<'_>) -> HashMap<String, String> {
    let d = plan.d;
    let version = plan.version;
    let cx = plan.cx;
    let ext = plan.ext;
    // ---------------- TABLES ----------------
    w.s(0, "SECTION");
    w.s(2, "TABLES");
    vport_table(w, d, ext);
    // LTYPE. Embedded text and shapes point at STYLE records (340), which get their handles
    // in the STYLE table below: (tag position, style name, is a shape) are filled in there.
    let mut ltype_handles: HashMap<String, String> = HashMap::new();
    let mut ltype_style_refs: Vec<(usize, Option<String>, bool)> = Vec::new();
    let th = table_head(w, "LTYPE", d.linetypes.len());
    for lt in &d.linetypes {
        let h = record_head(w, "LTYPE", &th, "AcDbLinetypeTableRecord");
        ltype_handles.entry(lt.name.to_ascii_uppercase()).or_insert(h.clone());
        cx.ltypes.entry(lt.name.to_ascii_uppercase()).or_insert(h);
        w.s(2, &lt.name);
        w.i(70, if crate::dxf_xref::dependent(&lt.name) { 48 } else { 0 });
        w.s(3, &lt.description);
        w.i(72, 65);
        w.i(73, lt.pattern.len() as i64);
        w.f(40, lt.pattern_length());
        for el in &lt.pattern {
            w.f(49, el.length);
            // 74: 2 = text, 4 = shape, + 1 when the rotation is absolute (DXF Reference, LTYPE).
            let (kind, shape) = match (&el.text, el.shape) {
                (Some(_), _) => (2, 0),
                (None, Some(n)) => (4, n),
                (None, None) => (0, 0),
            };
            if kind == 0 {
                w.i(74, 0);
                continue;
            }
            w.i(74, kind | i64::from(el.absolute));
            w.i(75, i64::from(shape));
            ltype_style_refs.push((w.t.len(), el.style.clone(), kind == 4));
            w.s(340, "0");
            w.f(46, el.scale);
            w.f(50, el.rotation);
            w.f(44, el.offset.x);
            w.f(45, el.offset.y);
            if let Some(t) = &el.text {
                w.s(9, t.clone());
            }
        }
    }
    w.s(0, "ENDTAB");
    write_layer_table(w, d);
    // STYLE. Linetype shapes from a shape file that isn't a text style get an unnamed shape
    // style record (70 bit 1) naming the file.
    let mut shape_files: Vec<String> = Vec::new();
    for (_, style, shape) in &ltype_style_refs {
        let Some(f) = style.as_deref().map(str::trim).filter(|f| *shape && !f.is_empty() && d.text_style(f).is_none()) else { continue };
        if !shape_files.iter().any(|x| x.eq_ignore_ascii_case(f)) {
            shape_files.push(f.to_string());
        }
    }
    let th = table_head(w, "STYLE", d.text_styles.len() + shape_files.len());
    for s in &d.text_styles {
        let h = record_head(w, "STYLE", &th, "AcDbTextStyleTableRecord");
        cx.styles.entry(s.name.to_ascii_uppercase()).or_insert(h);
        w.s(2, &s.name);
        // 70 bit 4: vertical; 71 bits 2/4: backwards / upside down.
        w.i(70, if s.vertical { 4 } else { 0 });
        w.f(40, s.height);
        w.f(41, s.width_factor);
        w.f(50, s.oblique.to_degrees());
        w.i(71, if s.backwards { 2 } else { 0 } | if s.upside_down { 4 } else { 0 });
        w.f(42, 0.2);
        // The built-in font goes out as `txt`, which every reader has; CADCraft's own name
        // travels in `CADCRAFT` xdata (`1000 FONT`, name) so the style reopens with it.
        let file = if s.font == cadcraft_fonts_name() { "txt" } else { s.font.as_str() };
        w.s(3, file);
        w.s(4, &s.big_font);
        if s.annotative {
            annotative_xdata(w);
        }
        if file != s.font {
            w.s(1001, dxf_ext::APP);
            w.s(1000, "FONT");
            w.s(1000, dxf_ext::xdata_str(&s.font));
        }
    }
    let mut shape_handles: HashMap<String, String> = HashMap::new();
    for f in &shape_files {
        let h = record_head(w, "STYLE", &th, "AcDbTextStyleTableRecord");
        shape_handles.insert(f.to_ascii_uppercase(), h);
        w.s(2, "");
        w.i(70, 1);
        w.f(40, 0.0);
        w.f(41, 1.0);
        w.f(50, 0.0);
        w.i(71, 0);
        w.f(42, 0.2);
        w.s(3, f.clone());
        w.s(4, "");
    }
    w.s(0, "ENDTAB");
    // Linetype text and shapes without a known style use Standard (else the first style).
    let fallback_style = cx.style("Standard").or_else(|| d.text_styles.first().and_then(|s| cx.style(&s.name))).unwrap_or_else(|| "0".into());
    for (pos, style, shape) in ltype_style_refs {
        let name = style.as_deref().map(str::trim).unwrap_or_default();
        let h = shape.then(|| shape_handles.get(&name.to_ascii_uppercase()).cloned()).flatten().or_else(|| cx.style(name));
        if let Some(t) = w.t.get_mut(pos) {
            *t = Tag::s(340, h.unwrap_or_else(|| fallback_style.clone()));
        }
    }
    // VIEW: named views (DXF Reference, VIEW). A view's layer state is CADCraft xdata.
    let th = table_head(w, "VIEW", d.views.len());
    for v in &d.views {
        record_head(w, "VIEW", &th, "AcDbViewTableRecord");
        w.s(2, &v.name);
        w.i(70, i64::from(v.layout.is_some()));
        w.f(40, v.height);
        w.p2(10, v.center);
        w.f(41, v.width);
        w.p(11, Vec3::Z);
        w.p(12, Vec3::ZERO);
        w.f(42, 50.0);
        w.f(43, 0.0);
        w.f(44, 0.0);
        w.f(50, if v.twist.is_finite() { v.twist.to_degrees() } else { 0.0 });
        w.i(71, 0);
        w.i(281, 0);
        w.i(72, 0);
        let xdata = [("LAYERSTATE", v.layer_state.as_deref()), ("LAYOUT", v.layout.as_deref().filter(|l| !l.is_empty()))];
        if xdata.iter().any(|(_, x)| x.is_some()) {
            w.s(1001, dxf_ext::APP);
            for (key, value) in xdata {
                if let Some(value) = value {
                    w.s(1000, key);
                    w.s(1000, dxf_ext::xdata_str(value));
                }
            }
        }
    }
    w.s(0, "ENDTAB");
    // UCS: named user coordinate systems (DXF Reference, UCS).
    let th = table_head(w, "UCS", d.ucss.len());
    for u in &d.ucss {
        record_head(w, "UCS", &th, "AcDbUCSTableRecord");
        w.s(2, &u.name);
        w.i(70, 0);
        w.p(10, u.origin);
        w.p(11, u.x_axis);
        w.p(12, u.y_axis);
        w.i(79, 0);
        w.f(146, 0.0);
    }
    w.s(0, "ENDTAB");
    let apps = ["ACAD", dxf_ext::APP, "AcadAnnotative", dxf_ext::LAYER_TRANSPARENCY_APP, dxf_ext::LAYER_DESCRIPTION_APP];
    let th = table_head(w, "APPID", apps.len());
    for app in apps {
        record_head(w, "APPID", &th, "AcDbRegAppTableRecord");
        w.s(2, app);
        w.i(70, 0);
    }
    w.s(0, "ENDTAB");
    write_dimstyles(w, d, cx, version);
    ltype_handles
}

fn write_layer_table(w: &mut W, d: &Drawing) {
    let th = table_head(w, "LAYER", d.layers.len());
    for l in &d.layers {
        let lh = record_head(w, "LAYER", &th, "AcDbLayerTableRecord");
        w.layers.insert(l.name.to_ascii_uppercase(), lh);
        w.s(2, &l.name);
        // 16 | 32: an xref's layer (`xref|name`), externally dependent and resolved.
        let xref = if crate::dxf_xref::dependent(&l.name) { 48 } else { 0 };
        w.i(70, i64::from(l.frozen) | if l.vp_freeze_new { 2 } else { 0 } | if l.locked { 4 } else { 0 } | xref);
        let aci = match l.color {
            Color::True(rgb) => i64::from(cadcraft_color::nearest_aci(rgb)),
            c => i64::from(c.to_aci()).clamp(1, 255),
        };
        w.i(62, if l.on { aci } else { -aci });
        if let Color::True(rgb) = l.color {
            w.i(420, i64::from(rgb.to_u32()));
        }
        w.s(6, &l.linetype);
        if !l.plot {
            w.i(290, 0);
        }
        w.i(370, i64::from(l.lineweight.to_dxf()));
        let normal = w.pstyles.get(&cadcraft_doc::NORMAL_STYLE.to_ascii_uppercase()).cloned();
        if let Some(h) = w.pstyles.get(&l.plot_style.trim().to_ascii_uppercase()).cloned().or(normal) {
            w.s(390, h);
        }
        if l.transparency > 0 {
            w.s(1001, dxf_ext::LAYER_TRANSPARENCY_APP);
            w.i(1071, dxf_ext::transparency_to_dxf(l.transparency));
        }
        if !l.description.is_empty() {
            // The first string is the layer standard's name, unused here.
            w.s(1001, dxf_ext::LAYER_DESCRIPTION_APP);
            w.s(1000, "");
            w.s(1000, dxf_ext::xdata_str(&l.description));
        }
    }
    w.s(0, "ENDTAB");
}

fn write_dimstyles(w: &mut W, d: &Drawing, cx: &mut Ctx, version: Version) {
    // DIMSTYLE
    let th = {
        let h = w.h();
        w.s(0, "TABLE");
        w.s(2, "DIMSTYLE");
        w.s(5, h.clone());
        w.s(330, "0");
        w.s(100, "AcDbSymbolTable");
        w.i(70, d.dim_styles.len() as i64);
        w.s(100, "AcDbDimStyleTable");
        w.i(71, 0);
        h
    };
    for s in &d.dim_styles {
        record_head(w, "DIMSTYLE", &th, "AcDbDimStyleTableRecord");
        w.s(2, &s.name);
        w.i(70, 0);
        let v = serde_json::to_value(s).unwrap_or_default();
        let defaults = serde_json::to_value(DimStyle::default()).unwrap_or_default();
        let mut newer = W { t: Vec::new(), next: 0, pstyles: HashMap::new(), layers: HashMap::new(), reactors: HashMap::new() };
        for (field, code, kind) in dxf_ext::DIM_CODES {
            let Some(enc) = v.get(*field).and_then(|x| cx.encode(*kind, x)) else { continue };
            // DIMPOST is written even when empty: a reader that defaults a missing one to "<>"
            // (the DWG bridge's) would otherwise give radius and diameter dimensions no "R" or
            // "Ø" in AutoCAD (#1053).
            if *code == dxf_ext::DIMPOST
                && let DimVal::Str(t) = &enc
                && t.is_empty()
            {
                w.s(*code, "");
                continue;
            }
            if !dxf_ext::NEWER_CODES.contains(code) {
                w.dimval(*code, enc, false);
                continue;
            }
            // Table groups where the file version has them, else xdata. DIMTFILL and DIMTFILLCLR
            // are written only when set, as other writers do, so a style without a text
            // background has a single 70 (its flags).
            if version >= dxf_ext::newer_since(*code) {
                if matches!(*code, 69 | 70) && v.get(*field) == defaults.get(*field) {
                    continue;
                }
                w.dimval(*code, enc, false);
            } else if v.get(*field) != defaults.get(*field) {
                newer.i(1070, i64::from(*code));
                newer.dimval(*code, enc, true);
            }
        }
        if !s.arrow_block1.trim().is_empty() || !s.arrow_block2.trim().is_empty() {
            w.i(dxf_ext::DIMSAH, 1);
        }
        if s.annotative {
            annotative_xdata(w);
        }
        if !newer.t.is_empty() {
            w.s(1001, dxf_ext::APP);
            w.s(1000, dxf_ext::DIMVARS_LIST);
            w.s(1002, "{");
            w.xdata(newer.t);
            w.s(1002, "}");
        }
    }
    w.s(0, "ENDTAB");
}

fn write_header(w: &mut W, d: &Drawing, version: Version) -> usize {
    w.s(0, "SECTION");
    w.s(2, "HEADER");
    w.s(9, "$ACADVER");
    w.s(1, version.code());
    w.s(9, "$DWGCODEPAGE");
    w.s(3, code_page(d, version));
    let ext = d.extents(&Space::Model);
    if !ext.is_empty() {
        w.s(9, "$EXTMIN");
        w.p(10, ext.min.to3(0.0));
        w.s(9, "$EXTMAX");
        w.p(10, ext.max.to3(0.0));
    }
    header_vars(w, d);
    let seed_pos = w.t.len();
    w.s(9, "$HANDSEED");
    w.s(5, "0");
    w.s(0, "ENDSEC");

    seed_pos
}

fn write_classes(w: &mut W, cx: &Ctx, every: &[&Entity]) {
    w.s(0, "SECTION");
    w.s(2, "CLASSES");
    let mut classes = vec![
        ("TABLESTYLE", "AcDbTableStyle", 4095, false),
        ("MLEADERSTYLE", "AcDbMLeaderStyle", 4095, false),
        ("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault", 0, false),
        ("ACDBPLACEHOLDER", "AcDbPlaceHolder", 0, false),
    ];
    if !cx.tables.is_empty() {
        classes.push(("ACAD_TABLE", "AcDbTable", 1025, true));
    }
    if every.iter().any(|e| matches!(e.kind, EntityKind::Helix(_))) {
        classes.push(("HELIX", "AcDbHelix", 4095, true));
    }
    if !cx.assoc.is_empty() {
        classes.push(("DIMASSOC", "AcDbDimAssoc", 0, false));
    }
    if cx.native_mleader() && every.iter().any(|e| matches!(e.kind, EntityKind::MLeader(_))) {
        classes.push(("MULTILEADER", "AcDbMLeader", 1025, true));
    }
    if every.iter().any(|e| matches!(e.kind, EntityKind::Wipeout(_))) {
        classes.push(("WIPEOUT", "AcDbWipeout", 127, true));
    }
    if !cx.images.is_empty() {
        classes.push(("IMAGE", "AcDbRasterImage", 127, true));
        classes.push(("IMAGEDEF", "AcDbRasterImageDef", 0, false));
        classes.push(("IMAGEDEF_REACTOR", "AcDbRasterImageDefReactor", 1, false));
        classes.push(("RASTERVARIABLES", "AcDbRasterVariables", 0, false));
    }
    classes.push(("SCALE", "AcDbScale", 0, false));
    for (dxf_name, cpp, proxy, is_entity) in classes {
        w.s(0, "CLASS");
        w.s(1, dxf_name);
        w.s(2, cpp);
        w.s(3, "ObjectDBX Classes");
        w.i(90, proxy);
        w.i(280, 0);
        w.i(281, i64::from(is_entity));
    }
    w.s(0, "ENDSEC");
}

fn write_entities(w: &mut W, d: &Drawing, ms_br: &str, ps_brs: &[(String, String, &Layout)], cx: &Ctx) {
    w.s(0, "SECTION");
    w.s(2, "ENTITIES");
    for e in d.model.iter() {
        entity(w, d, e, ms_br, false, cx);
    }
    if let Some((_, h, l)) = ps_brs.first() {
        for e in sheet_first(l) {
            entity(w, d, e, h, true, cx);
        }
    }
    w.s(0, "ENDSEC");
}

struct LayoutWriteContext<'a> {
    drawing: &'a Drawing,
    dictionary: &'a str,
}

struct LayoutWriteData<'a> {
    handle: &'a str,
    name: &'a str,
    tab: i64,
    block_record: &'a str,
    flags: i64,
    page: &'a PageSetup,
    view: Option<(Vec2, f64)>,
}

fn layout_obj(w: &mut W, context: LayoutWriteContext<'_>, data: LayoutWriteData<'_>) {
    let d = context.drawing;
    let layout_dict = context.dictionary;
    let h = data.handle;
    let name = data.name;
    let tab = data.tab;
    let brh = data.block_record;
    let flags = data.flags;
    let page = data.page;
    let view = data.view;
    // Plot scale: a standard scale when fitting or 1:1, otherwise custom (142 paper units
    // per 143 drawing units).
    let scale = if page.scale.is_finite() && page.scale > 0.0 { page.scale } else { 1.0 };
    let standard = page.scale_to_fit || scale == 1.0;
    let plot_flags = 512
        | 32
        | if standard { 16 } else { 0 }
        | if page.lineweights { 128 } else { 0 }
        | if page.center { 4 } else { 0 }
        | if page.show_plot_styles { 2 } else { 0 };
    let plot_type = match page.plot_area.as_str() {
        "display" => 0,
        "extents" => 1,
        "limits" => 2,
        "window" => 4,
        _ => 5,
    };
    w.s(0, "LAYOUT");
    w.s(5, h);
    w.s(330, layout_dict.to_string());
    w.s(100, "AcDbPlotSettings");
    w.s(1, "");
    let device = page.device.trim();
    w.s(2, if device.is_empty() || device.eq_ignore_ascii_case("None") { "none_device" } else { device });
    w.s(4, page.paper.replace(' ', "_"));
    w.s(6, "");
    for (c, v) in [
        (40, page.margins_mm[0]),
        (41, page.margins_mm[1]),
        (42, page.margins_mm[2]),
        (43, page.margins_mm[3]),
        (44, page.width_mm),
        (45, page.height_mm),
        (46, 0.0),
        (47, 0.0),
        (48, page.plot_window[0]),
        (49, page.plot_window[1]),
        (140, page.plot_window[2]),
        (141, page.plot_window[3]),
        (142, scale),
        (143, 1.0),
    ] {
        w.f(c, v);
    }
    w.i(70, plot_flags);
    // Plot paper units: 1 millimetres, 0 inches (#1058).
    w.i(72, i64::from(cadcraft_render::paper::page_unit_mm(d, page) == 1.0));
    w.i(73, i64::from(page.landscape));
    w.i(74, plot_type);
    w.s(7, &page.plot_style_table);
    w.i(75, if page.scale_to_fit { 0 } else { 16 });
    w.f(147, scale);
    w.f(148, 0.0);
    w.f(149, 0.0);
    w.s(100, "AcDbLayout");
    w.s(1, name);
    w.i(70, flags);
    w.i(71, tab);
    w.p2(10, Vec2::ZERO);
    w.p2(11, Vec2::new(12.0, 9.0));
    w.p(12, Vec3::ZERO);
    w.p(14, Vec3::ZERO);
    w.p(15, Vec3::ZERO);
    w.f(146, 0.0);
    w.p(13, Vec3::ZERO);
    w.p(16, Vec3::new(1.0, 0.0, 0.0));
    w.p(17, Vec3::new(0.0, 1.0, 0.0));
    w.i(76, 0);
    w.s(330, brh);
    // The saved paper-space view (centre, height) as CADCraft xdata.
    if let Some((c, height)) = view.filter(|(c, v)| c.x.is_finite() && c.y.is_finite() && v.is_finite()) {
        w.s(1001, dxf_ext::APP);
        w.s(1000, "PSVIEW");
        w.s(1002, "{");
        w.p(1010, c.to3(0.0));
        w.f(1040, height);
        w.s(1002, "}");
    }
}

struct ObjectWritePlan<'a> {
    d: &'a Drawing,
    cx: &'a Ctx,
    root_dict: &'a String,
    group_dict: &'a String,
    layout_dict: &'a String,
    table_style_dict: &'a String,
    pstyle_dict: &'a String,
    scale_dict: &'a String,
    model_layout: &'a String,
    ms_br: &'a String,
    ps_brs: &'a [(String, String, &'a Layout)],
    layout_handles: &'a [String],
    xrecords: &'a [(&'static str, String, Vec<String>)],
    scales: &'a [(&'a cadcraft_doc::AnnoScale, String)],
    pstyles: &'a [(String, String)],
    table_styles: &'a [TableStyle],
    mleader_styles: &'a [MLeaderStyle],
    ltype_handles: &'a HashMap<String, String>,
}

fn write_layout_objects(w: &mut W, plan: &ObjectWritePlan<'_>) {
    let d = plan.d;
    let root_dict = plan.root_dict;
    let layout_dict = plan.layout_dict;
    let model_layout = plan.model_layout;
    let ms_br = plan.ms_br;
    let ps_brs = plan.ps_brs;
    let layout_handles = plan.layout_handles;
    w.s(0, "DICTIONARY");
    w.s(5, layout_dict.clone());
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    w.s(3, "Model");
    w.s(350, model_layout.clone());
    for (i, (_, _, l)) in ps_brs.iter().enumerate() {
        w.s(3, &l.name);
        w.s(350, layout_handles.get(i).cloned().unwrap_or_default());
    }
    // Model space has no page setup of its own: write what PLOT does for it, the drawing extents
    // fitted to the paper (plot type 1, scaled to fit). The default page's "layout" area is
    // plot type 5, "Layout information" (DXF Reference, PLOTSETTINGS group 74), which means
    // nothing for model space: another program's Plot dialog then shows no plot area or scale.
    let model_page = PageSetup { plot_area: "extents".into(), scale_to_fit: true, ..PageSetup::default() };
    layout_obj(
        w,
        LayoutWriteContext { drawing: d, dictionary: layout_dict },
        LayoutWriteData { handle: model_layout, name: "Model", tab: 0, block_record: ms_br, flags: 1, page: &model_page, view: None },
    );
    for (i, (_, brh, l)) in ps_brs.iter().enumerate() {
        let h = layout_handles.get(i).cloned().unwrap_or_default();
        layout_obj(
            w,
            LayoutWriteContext { drawing: d, dictionary: layout_dict },
            LayoutWriteData {
                handle: &h,
                name: &l.name,
                tab: i64::from(l.tab_order.max(1)),
                block_record: brh,
                flags: 1,
                page: &l.page,
                view: l.view,
            },
        );
    }
}

fn write_root_group_objects(
    w: &mut W,
    plan: &ObjectWritePlan<'_>,
    groups: &[(&Group, String)],
    group_owner: &str,
    written: &std::collections::HashSet<String>,
    mleader_dict: &str,
) {
    let cx = plan.cx;
    let root_dict = plan.root_dict;
    let group_dict = plan.group_dict;
    let layout_dict = plan.layout_dict;
    let table_style_dict = plan.table_style_dict;
    let pstyle_dict = plan.pstyle_dict;
    let scale_dict = plan.scale_dict;
    let xrecords = plan.xrecords;
    w.s(0, "SECTION");
    w.s(2, "OBJECTS");
    w.s(0, "DICTIONARY");
    w.s(5, root_dict.clone());
    w.s(330, "0");
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    w.s(3, "ACAD_GROUP");
    w.s(350, group_dict.clone());
    if !cx.images.is_empty() {
        w.s(3, "ACAD_IMAGE_DICT");
        w.s(350, cx.images.dict.clone());
        w.s(3, "ACAD_IMAGE_VARS");
        w.s(350, cx.images.vars.clone());
    }
    w.s(3, "ACAD_LAYOUT");
    w.s(350, layout_dict.clone());
    w.s(3, "ACAD_MLEADERSTYLE");
    w.s(350, mleader_dict.to_string());
    w.s(3, "ACAD_MLINESTYLE");
    w.s(350, cx.mlines.dict.clone());
    w.s(3, dxf_ext::PLOT_STYLE_NAMES_KEY);
    w.s(350, pstyle_dict.clone());
    w.s(3, "ACAD_TABLESTYLE");
    w.s(350, table_style_dict.clone());
    for (key, h, _) in xrecords {
        w.s(3, *key);
        w.s(350, h);
    }
    w.s(3, "ACAD_SCALELIST");
    w.s(350, scale_dict.clone());
    w.s(0, "DICTIONARY");
    w.s(5, group_dict);
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (g, h) in groups {
        w.s(3, &g.name);
        w.s(350, h);
    }
    for (g, h) in groups {
        group_obj(w, g, h, group_owner, written);
    }
}

fn write_style_objects(w: &mut W, plan: &ObjectWritePlan<'_>, mleader_dict: &str, mleader_handles: &[String]) {
    let d = plan.d;
    let cx = plan.cx;
    let root_dict = plan.root_dict;
    let table_style_dict = plan.table_style_dict;
    let mleader_styles = plan.mleader_styles;
    let table_styles = plan.table_styles;
    let ltype_handles = plan.ltype_handles;
    // Multileader styles.
    w.s(0, "DICTIONARY");
    w.s(5, mleader_dict.to_string());
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (s, h) in mleader_styles.iter().zip(mleader_handles) {
        w.s(3, &s.name);
        w.s(350, h);
    }
    let byblock_ltype = ltype_handles.get("BYBLOCK").cloned();
    for (s, h) in mleader_styles.iter().zip(mleader_handles) {
        mleader_style_obj(w, s, h, mleader_dict, byblock_ltype.as_deref(), cx.style(&s.text_style));
    }
    // Multiline styles.
    w.t.extend(crate::dxf_mline::objects(&cx.mlines, root_dict));
    // Table styles.
    w.s(0, "DICTIONARY");
    w.s(5, table_style_dict.clone());
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (name, h) in &cx.table_styles {
        w.s(3, name);
        w.s(350, h);
    }
    for (s, (_, h)) in table_styles.iter().zip(&cx.table_styles) {
        table_style_obj(w, d, s, h, table_style_dict);
    }
}

fn write_named_objects(w: &mut W, plan: &ObjectWritePlan<'_>) {
    let root_dict = plan.root_dict;
    let pstyle_dict = plan.pstyle_dict;
    let xrecords = plan.xrecords;
    let pstyles = plan.pstyles;
    let scales = plan.scales;
    let scale_dict = plan.scale_dict;
    // Plot style names: a dictionary with a default (Normal) of placeholder objects.
    w.s(0, "ACDBDICTIONARYWDFLT");
    w.s(5, pstyle_dict.clone());
    w.group("ACAD_REACTORS", 330, &[root_dict]);
    w.s(330, root_dict.clone());
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    for (n, h) in pstyles {
        w.s(3, n);
        w.s(350, h);
    }
    w.s(100, "AcDbDictionaryWithDefault");
    w.s(340, pstyles.first().map(|(_, h)| h.clone()).unwrap_or_default());
    for (_, h) in pstyles {
        w.s(0, "ACDBPLACEHOLDER");
        w.s(5, h);
        w.group("ACAD_REACTORS", 330, &[pstyle_dict]);
        w.s(330, pstyle_dict.clone());
    }
    // Parametric constraints and parameters, saved layer states, plot style tables (CADCraft
    // data).
    for (_, h, chunks) in xrecords {
        w.s(0, "XRECORD");
        w.s(5, h);
        w.group("ACAD_REACTORS", 330, &[root_dict]);
        w.s(330, root_dict.clone());
        w.s(100, "AcDbXrecord");
        w.i(280, 1);
        for c in chunks {
            w.s(1, c.clone());
        }
    }
    scale_list(w, scales, scale_dict, root_dict);
}

fn write_associative_objects(w: &mut W, cx: &Ctx) {
    // Dimension associativity: extension dictionary + DIMASSOC per dimension.
    let mut assoc: Vec<(&Handle, &AssocObj)> = cx.assoc.iter().collect();
    assoc.sort_by_key(|(h, _)| **h);
    for (dim, a) in assoc {
        let dh = dim.hex();
        w.s(0, "DICTIONARY");
        w.s(5, a.xdict.clone());
        w.group("ACAD_REACTORS", 330, &[&dh]);
        w.s(330, dh.clone());
        w.s(100, "AcDbDictionary");
        w.i(280, 1);
        w.i(281, 1);
        w.s(3, "ACAD_DIMASSOC");
        w.s(360, a.handle.clone());
        w.s(0, "DIMASSOC");
        w.s(5, a.handle.clone());
        w.group("ACAD_REACTORS", 330, &[&a.xdict]);
        w.s(330, a.xdict.clone());
        w.s(100, "AcDbDimAssoc");
        w.s(330, dh);
        w.i(90, a.flags);
        w.i(70, 0);
        w.i(71, 0);
        for r in &a.refs {
            w.s(1, "AcDbOsnapPointRef");
            w.i(72, r.osnap);
            w.s(331, r.main.hex());
            w.i(73, if r.segment > 0 { 2 } else { 0 });
            w.i(91, r.segment);
            w.s(301, "");
            w.f(40, r.param);
            w.p(10, r.point);
            if let Some(o) = r.other {
                w.s(332, o.hex());
                w.i(74, 0);
                w.i(92, 0);
                w.s(302, "");
            }
            w.i(75, 0);
        }
    }
}

fn write_objects(w: &mut W, plan: ObjectWritePlan<'_>) {
    // Groups (members that still exist) and multileader styles.
    let group_owner = plan.group_dict.clone();
    let written: std::collections::HashSet<String> = w.t.iter().filter(|t| t.code == 5).map(Tag::str).collect();
    let groups: Vec<(&Group, String)> = plan.d.groups.iter().filter(|g| !g.name.is_empty()).map(|g| (g, w.h())).collect();
    let mleader_dict = w.h();
    let mleader_handles: Vec<String> = plan.cx.mleader_styles.iter().map(|(_, h)| h.clone()).collect();
    write_root_group_objects(w, &plan, &groups, &group_owner, &written, &mleader_dict);
    write_style_objects(w, &plan, &mleader_dict, &mleader_handles);
    write_named_objects(w, &plan);
    write_associative_objects(w, plan.cx);
    write_layout_objects(w, &plan);
    // Raster image definitions.
    w.t.extend(crate::dxf_image::objects(&plan.cx.images, plan.root_dict));
    w.s(0, "ENDSEC");
}

fn dimension_block_definitions(
    d: &Drawing,
    all_spaces: &[&EntityStore],
    dim_plan: &super::blocks::DimBlockPlan,
    w: &mut W,
    cx: &mut Ctx,
) -> Vec<(String, String, Vec<Entity>)> {
    let mut dim_defs = Vec::new();
    let mut n = 1;
    let mut reused_handles = std::collections::HashSet::new();
    for st in all_spaces {
        for e in st.iter() {
            if let EntityKind::Dimension(dm) = &e.kind {
                let reused = dim_plan.reuse.get(&e.handle);
                let ents: Vec<Entity>;
                let name = match reused {
                    // A dimension still linked to the block it was read with hasn't changed since
                    // (edits drop the link, and the screen draws that block): the block is written
                    // as read, so other programs show it as they drew it.
                    Some(b) if !b.entities.is_empty() => {
                        ents = b.entities.iter().map(|x| x.as_ref().clone()).collect();
                        reused_handles.extend(ents.iter().map(|x| x.handle).filter(|h| h.0 != 0));
                        b.name.clone()
                    }
                    Some(b) => {
                        let mut fresh = dim_block_entities(d, dm, &e.common.layer);
                        reuse_handles(&mut fresh, b, &mut reused_handles);
                        ents = fresh;
                        b.name.clone()
                    }
                    None => {
                        ents = dim_block_entities(d, dm, &e.common.layer);
                        let mut name = format!("*D{n}");
                        while d.block(&name).is_some() {
                            n += 1;
                            name = format!("*D{n}");
                        }
                        n += 1;
                        name
                    }
                };
                let brh = w.h();
                cx.dim_blocks.insert(e.handle, name.clone());
                dim_defs.push((name, brh, ents));
            }
        }
    }
    dim_defs
}

fn arrow_block_definitions(
    d: &Drawing,
    every: &[&Entity],
    user_blocks: &[(String, String, &Block)],
    w: &mut W,
    cx: &mut Ctx,
) -> Vec<(String, String, Vec<Entity>)> {
    let mut arrow_names: Vec<String> = Vec::new();
    for s in &d.dim_styles {
        arrow_names.extend([s.arrow_block.clone(), s.arrow_block1.clone(), s.arrow_block2.clone(), s.leader_arrow_block.clone()]);
    }
    for e in every {
        if let EntityKind::Dimension(dm) = &e.kind {
            for (k, v) in &dm.overrides {
                let Some(f) = DimStyle::field_name(k).filter(|f| f.starts_with("arrowBlock") || *f == "leaderArrowBlock") else { continue };
                if let Some(s) = dxf_ext::canonical(f, v).as_ref().and_then(|v| v.as_str()) {
                    arrow_names.push(s.to_string());
                }
            }
        }
    }
    let mut arrow_defs: Vec<(String, String, Vec<Entity>)> = Vec::new();
    for name in arrow_names {
        let key = name.trim().to_ascii_uppercase();
        if key.is_empty() || cx.arrows.contains_key(&key) {
            continue;
        }
        if let Some((_, h, _)) = user_blocks.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name.trim())) {
            cx.arrows.insert(key, h.clone());
            continue;
        }
        let Some((bname, kind)) = dxf_ext::arrow_block_name(&name) else { continue };
        let existing = arrow_defs.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(&bname)).map(|(_, h, _)| h.clone());
        let existing = existing.or_else(|| user_blocks.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(&bname)).map(|(_, h, _)| h.clone()));
        let h = match existing {
            Some(h) => h,
            None => {
                let h = w.h();
                arrow_defs.push((bname, h.clone(), arrow_block_entities(kind)));
                h
            }
        };
        cx.arrows.insert(key, h);
    }

    arrow_defs
}

fn table_block_definitions(d: &Drawing, every: &[&Entity], w: &mut W, cx: &mut Ctx) -> Vec<(String, String, Vec<Entity>)> {
    let mut table_defs = Vec::new();
    let mut tn = 1;
    for e in every {
        if let EntityKind::Table(t) = &e.kind {
            if cx.tables.contains_key(&e.handle) {
                continue;
            }
            let mut name = format!("*T{tn}");
            while d.block(&name).is_some() {
                tn += 1;
                name = format!("*T{tn}");
            }
            tn += 1;
            let brh = w.h();
            cx.tables.insert(e.handle, (name.clone(), brh.clone()));
            table_defs.push((name, brh, table_block_entities(d, t, &e.common.layer)));
        }
    }

    table_defs
}

fn plan_dimension_associations(d: &Drawing, all_spaces: &[&EntityStore], w: &mut W, cx: &mut Ctx) {
    for st in all_spaces {
        for e in st.iter() {
            if let EntityKind::Dimension(dm) = &e.kind {
                let (flags, refs) = std_assoc_refs(d, dm);
                if !refs.is_empty() {
                    cx.assoc.insert(e.handle, AssocObj { xdict: w.h(), handle: w.h(), flags, refs });
                }
            }
        }
    }
}

/// The drawing's DXF tags, declaring `version` in `$ACADVER`, with what `target` (the version the
/// file ends up in: `version`, or the DWG version the bridge writes) has.
pub(super) fn tags_for(d: &Drawing, version: Version, target: Version) -> Vec<Tag> {
    // An in-place block edit in progress is not written: the drawing as before it.
    let committed = d.committed();
    let d: &Drawing = &committed;
    // Structural handles start above every entity handle.
    let mut w = W {
        t: Vec::new(),
        next: d.handseed.max(0x100).saturating_add(0x1000),
        pstyles: HashMap::new(),
        layers: HashMap::new(),
        reactors: hatch_reactors(d),
    };
    // Pre-allocate handles for block records.
    let paper_layouts = d.paper_space_layouts();
    let ms_br = w.h();
    // Xrefs are saved as references: without their objects and the symbols that came with them.
    let saved_view = crate::dxf_xref::saved_view(d);
    let d = saved_view.as_ref().unwrap_or(d);
    let ps_brs: Vec<(String, String, &Layout)> = paper_layouts
        .iter()
        .enumerate()
        .map(|(i, l)| (if i == 0 { "*Paper_Space".to_string() } else { format!("*Paper_Space{}", i - 1) }, String::new(), *l))
        .collect();
    let ps_brs: Vec<(String, String, &Layout)> = ps_brs.into_iter().map(|(n, _, l)| (n, w.h(), l)).collect();
    let dim_plan = dim_block_plan(d);
    let mut user_blocks: Vec<(String, String, &Block)> = d
        .blocks
        .values()
        .filter(|b| !dim_plan.skip.contains(&b.name.to_ascii_uppercase()))
        .map(|b| (b.name.clone(), String::new(), b.as_ref()))
        .collect();
    user_blocks.iter_mut().for_each(|b| b.1 = format!("{:X}", 0));
    let user_blocks: Vec<(String, String, &Block)> = user_blocks.into_iter().map(|(n, _, b)| (n, w.h(), b)).collect();
    let mut cx = Ctx { paper_views: paper_views(d), target, ..Ctx::default() };
    cx.block_records = user_blocks.iter().map(|(n, h, _)| (n.to_ascii_uppercase(), h.clone())).collect();
    let all_spaces: Vec<&EntityStore> = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).collect();
    let mut dim_defs = dimension_block_definitions(d, &all_spaces, &dim_plan, &mut w, &mut cx);
    // Every entity, including block contents (for arrows, tables and overrides).
    let every: Vec<&Entity> = all_spaces
        .iter()
        .flat_map(|s| s.iter().map(|e| e.as_ref()))
        .chain(d.blocks.values().flat_map(|b| b.entities.iter().map(|e| e.as_ref())))
        .collect();
    let mut arrow_defs = arrow_block_definitions(d, &every, &user_blocks, &mut w, &mut cx);
    let mut table_defs = table_block_definitions(d, &every, &mut w, &mut cx);
    // Image definitions (IMAGEDEF objects) of raster images anywhere in the drawing.
    cx.images = crate::dxf_image::plan(every.iter().copied(), || w.h());
    cx.mlines = crate::dxf_mline::plan(d, every.iter().copied(), || w.h());
    plan_dimension_associations(d, &all_spaces, &mut w, &mut cx);
    let default_table_style = [TableStyle::default()];
    let table_styles: &[TableStyle] = if d.table_styles.is_empty() { &default_table_style } else { &d.table_styles };
    cx.table_styles = table_styles.iter().map(|s| (s.name.clone(), w.h())).collect();
    let default_mleader_style = [MLeaderStyle::default()];
    let mleader_styles: &[MLeaderStyle] = if d.mleader_styles.is_empty() { &default_mleader_style } else { &d.mleader_styles };
    cx.mleader_styles = mleader_styles.iter().map(|s| (s.name.clone(), w.h())).collect();
    let root_dict = w.h();
    let group_dict = w.h();
    let layout_dict = w.h();
    let table_style_dict = w.h();
    // CADCraft data in XRECORDs of the named object dictionary: (key, handle, text chunks).
    let mut xrecords: Vec<(&str, String, Vec<String>)> = Vec::new();
    // The drawing's own next handle: table records and objects are numbered above it, so
    // reading it back (instead of $HANDSEED) keeps their handles across saves (#495).
    xrecords.push((dxf_ext::HANDSEED_KEY, w.h(), vec![format!("{:X}", d.handseed)]));
    if let Some(name) = d.header.get("CMLEADERSTYLE").and_then(HVal::as_str) {
        xrecords.push((dxf_ext::MLEADER_STYLE_KEY, w.h(), vec![name.to_string()]));
    }
    if let Some(chunks) = dxf_ext::constraint_chunks(&d.constraints, &d.parametric) {
        xrecords.push((dxf_ext::CONSTRAINTS_KEY, w.h(), chunks));
    }
    if let Some(chunks) = dxf_ext::layer_state_chunks(&d.layer_states) {
        xrecords.push((dxf_ext::LAYER_STATES_KEY, w.h(), chunks));
    }
    if let Some(chunks) = dxf_ext::plot_style_chunks(&d.plot_style_tables) {
        xrecords.push((dxf_ext::PLOT_STYLES_KEY, w.h(), chunks));
    }
    if let Some(chunks) = dxf_ext::anno_chunks(d) {
        xrecords.push((dxf_ext::ANNOTATION_KEY, w.h(), chunks));
    }
    // The annotation scale list: the ACAD_SCALELIST dictionary of SCALE objects.
    let scale_dict = w.h();
    let scales: Vec<(&cadcraft_doc::AnnoScale, String)> = d.scales.iter().filter(|s| s.is_valid()).map(|s| (s, w.h())).collect();
    // Plot style names used by layers and objects (Normal always), each a placeholder object.
    let pstyle_dict = w.h();
    let mut pstyle_names: Vec<String> = vec![cadcraft_doc::NORMAL_STYLE.to_string()];
    let named = d.layers.iter().map(|l| l.plot_style.as_str()).chain(every.iter().map(|e| e.common.plot_style.as_str()));
    for n in named {
        let n = n.trim();
        let logical = n.is_empty() || n.eq_ignore_ascii_case("bylayer") || n.eq_ignore_ascii_case("byblock");
        if !logical && pstyle_names.len() < dxf_ext::MAX_PLOT_STYLE_NAMES && !pstyle_names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
            pstyle_names.push(n.to_string());
        }
    }
    let pstyles: Vec<(String, String)> = pstyle_names.into_iter().map(|n| (n, w.h())).collect();
    w.pstyles = pstyles.iter().map(|(n, h)| (n.to_ascii_uppercase(), h.clone())).collect();
    let model_layout = w.h();
    let layout_handles: Vec<String> = ps_brs.iter().map(|_| w.h()).collect();

    let seed_pos = write_header(&mut w, d, version);

    write_classes(&mut w, &cx, &every);

    let ltype_handles = write_tables(&mut w, TableWritePlan { d, version, cx: &mut cx, ext: d.extents(&Space::Model) });

    // BLOCK_RECORD
    let th = table_head(&mut w, "BLOCK_RECORD", 1 + ps_brs.len() + user_blocks.len() + dim_defs.len() + arrow_defs.len() + table_defs.len());
    let br_rec = |w: &mut W, h: &str, name: &str, layout: Option<&str>, blk: Option<&Block>| {
        w.s(0, "BLOCK_RECORD");
        w.s(5, h);
        w.s(330, th.clone());
        w.s(100, "AcDbSymbolTableRecord");
        w.s(100, "AcDbBlockTableRecord");
        w.s(2, name);
        w.s(340, layout.unwrap_or("0"));
        // User blocks: insertion units (70) and explodability (280).
        if let Some(b) = blk {
            w.i(70, i64::from(b.units));
            w.i(280, i64::from(b.explodable));
            if let Some(a) = &b.array {
                w.s(1001, dxf_ext::APP);
                w.xdata(dxf_ext::array_xdata(a));
            }
        }
    };
    br_rec(&mut w, &ms_br, "*Model_Space", Some(&model_layout), None);
    for (i, (n, h, _)) in ps_brs.iter().enumerate() {
        br_rec(&mut w, h, n, layout_handles.get(i).map(String::as_str), None);
    }
    for (n, h, b) in &user_blocks {
        br_rec(&mut w, h, n, None, Some(b));
    }
    for (n, h, _) in dim_defs.iter().chain(arrow_defs.iter()).chain(table_defs.iter()) {
        br_rec(&mut w, h, n, None, None);
    }
    w.s(0, "ENDTAB");
    w.s(0, "ENDSEC");

    // ---------------- BLOCKS ----------------
    w.s(0, "SECTION");
    w.s(2, "BLOCKS");
    let block = |w: &mut W, (name, desc): (&str, &str), brh: &str, base: Vec3, flags: i64, ents: &mut dyn Iterator<Item = &Entity>, paper: bool| {
        let bh = w.h();
        w.s(0, "BLOCK");
        w.s(5, bh);
        w.s(330, brh);
        w.s(100, "AcDbEntity");
        if paper {
            w.i(67, 1);
        }
        w.s(8, "0");
        w.s(100, "AcDbBlockBegin");
        w.s(2, name);
        w.i(70, flags);
        w.p(10, base);
        w.s(3, name);
        w.s(1, "");
        if !desc.is_empty() {
            w.s(4, desc);
        }
        for e in ents {
            entity(w, d, e, brh, false, &cx);
        }
        let eh = w.h();
        w.s(0, "ENDBLK");
        w.s(5, eh);
        w.s(330, brh);
        w.s(100, "AcDbEntity");
        if paper {
            w.i(67, 1);
        }
        w.s(8, "0");
        w.s(100, "AcDbBlockEnd");
    };
    block(&mut w, ("*Model_Space", ""), &ms_br, Vec3::ZERO, 0, &mut std::iter::empty(), false);
    for (i, (n, h, l)) in ps_brs.iter().enumerate() {
        if i == 0 {
            block(&mut w, (n, ""), h, Vec3::ZERO, 0, &mut std::iter::empty(), true);
        } else {
            block(&mut w, (n, ""), h, Vec3::ZERO, 0, &mut sheet_first(l).into_iter(), true);
        }
    }
    for (n, h, b) in &user_blocks {
        block(&mut w, (n, &b.description), h, b.base, if b.anonymous { 1 } else { 0 }, &mut b.entities.iter().map(|e| e.as_ref()), false);
    }
    // Generated geometry entities (dimension, arrowhead and table blocks) need handles.
    for (_, _, ents) in dim_defs.iter_mut().chain(arrow_defs.iter_mut()).chain(table_defs.iter_mut()) {
        for e in ents.iter_mut().filter(|e| e.handle.0 == 0) {
            e.handle = Handle(w.next);
            w.next = w.next.saturating_add(1);
        }
    }
    for (n, h, ents) in &dim_defs {
        block(&mut w, (n, ""), h, Vec3::ZERO, 1, &mut ents.iter(), false);
    }
    for (n, h, ents) in &arrow_defs {
        block(&mut w, (n, ""), h, Vec3::ZERO, 0, &mut ents.iter(), false);
    }
    for (n, h, ents) in &table_defs {
        block(&mut w, (n, ""), h, Vec3::ZERO, 1, &mut ents.iter(), false);
    }
    w.s(0, "ENDSEC");

    write_entities(&mut w, d, &ms_br, &ps_brs, &cx);

    write_objects(
        &mut w,
        ObjectWritePlan {
            d,
            cx: &cx,
            root_dict: &root_dict,
            group_dict: &group_dict,
            layout_dict: &layout_dict,
            table_style_dict: &table_style_dict,
            pstyle_dict: &pstyle_dict,
            scale_dict: &scale_dict,
            model_layout: &model_layout,
            ms_br: &ms_br,
            ps_brs: &ps_brs,
            layout_handles: &layout_handles,
            xrecords: &xrecords,
            scales: &scales,
            pstyles: &pstyles,
            table_styles,
            mleader_styles,
            ltype_handles: &ltype_handles,
        },
    );

    w.s(0, "EOF");
    let seed = format!("{:X}", w.next);
    if let Some(t) = w.t.get_mut(seed_pos + 1) {
        *t = Tag::s(5, seed);
    }
    crate::dxf_raw::finish(&mut w.t, d);
    crate::dxf_raw::data_store(&mut w.t, d, target >= Version::R2013);
    add_class_instance_counts(&mut w.t, target);
    mark_xrefs(&mut w.t, d);
    w.t
}

/// DXF Reference group 91 of CLASS records, the instance count, which came with the 2004 format.
/// From 2004 every CLASS record gets its actual count, counted after `dxf_raw::finish` restores
/// foreign classes and objects (AutoCAD rejects a 2004+ file without it, #635). An R2000 file has
/// none, not even on a class kept from a newer file: AutoCAD LT 2026 stops responding when an
/// R2000 file's CLASS records carry a 91.
fn add_class_instance_counts(tags: &mut Vec<Tag>, target: Version) {
    if target < Version::R2004 {
        let mut in_class = false;
        tags.retain(|t| {
            if t.code == 0 {
                in_class = t.str() == "CLASS";
            }
            !(in_class && t.code == 91)
        });
        return;
    }
    let mut counts: HashMap<String, i64> = HashMap::new();
    for t in tags.iter().filter(|t| t.code == 0) {
        let kind = t.str();
        if kind != "CLASS" {
            *counts.entry(kind.to_ascii_uppercase()).or_default() += 1;
        }
    }

    let mut start = 0;
    while start < tags.len() {
        if tags.get(start).is_none_or(|t| t.code != 0 || t.str() != "CLASS") {
            start += 1;
            continue;
        }
        let end = tags[start + 1..].iter().position(|t| t.code == 0).map_or(tags.len(), |i| start + 1 + i);
        let name = tags[start + 1..end].iter().find(|t| t.code == 1).map(Tag::str).unwrap_or_default().to_ascii_uppercase();
        let count = counts.get(&name).copied().unwrap_or(0);
        let inserted = if let Some(i) = tags[start + 1..end].iter().position(|t| t.code == 91) {
            tags[start + 1 + i] = Tag::i(91, count);
            false
        } else if let Some(i) = tags[start + 1..end].iter().position(|t| t.code == 90) {
            tags.insert(start + 2 + i, Tag::i(91, count));
            true
        } else {
            false
        };
        // Inserting the group shifts the next record's start by one.
        start = end + usize::from(inserted);
    }
}

/// Xref blocks (DXF Reference, BLOCK): bit 4 of group 70 and the external drawing's path in
/// group 1, set on the written BLOCK records of blocks that have an `xref_path`.
fn mark_xrefs(tags: &mut [Tag], d: &Drawing) {
    // Overlays add bit 8.
    let xrefs: HashMap<String, (&str, i64)> = d
        .blocks
        .values()
        .filter_map(|b| Some((b.name.to_ascii_uppercase(), (b.xref_path.as_deref()?, if b.xref_overlay { 12 } else { 4 }))))
        .collect();
    if xrefs.is_empty() {
        return;
    }
    // Inside a BLOCK record: the path once its name (2) says it is an xref.
    let mut block: Option<Option<(&str, i64)>> = None;
    for t in tags.iter_mut() {
        match (t.code, block) {
            (0, _) => block = (t.str() == "BLOCK").then_some(None),
            (2, Some(None)) => block = Some(xrefs.get(&t.str().to_ascii_uppercase()).copied()),
            (70, Some(Some((_, flags)))) => *t = Tag::i(70, t.i64() | flags),
            (1, Some(Some((p, _)))) => *t = Tag::s(1, p),
            _ => {}
        }
    }
}
