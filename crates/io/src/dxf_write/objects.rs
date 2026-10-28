use super::ctx::Ctx;
use super::{Drawing, Group, HashMap, MLeaderStyle, TableStyle, Version, W, dxf_ext};

/// A TABLESTYLE object (DXF Reference, OBJECTS: TABLESTYLE): margins, title/header
/// suppression and per row type (data, column header, title) text and border settings.
pub(super) fn table_style_obj(w: &mut W, d: &Drawing, s: &TableStyle, h: &str, dict: &str) {
    w.s(0, "TABLESTYLE");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbTableStyle");
    if let Some(v) = s.props.version {
        w.i(280, i64::from(v));
    }
    w.s(3, s.props.description.clone());
    w.i(70, i64::from(s.props.flow_up));
    w.i(71, i64::from(s.props.flags));
    w.f(40, s.margin);
    w.f(41, s.margin);
    w.i(280, i64::from(!s.title));
    w.i(281, i64::from(!s.header));
    // Data, column header and title cell styles; CADCraft's complete cell styles in xdata.
    for (kind, data) in crate::dxf_table::CELL_TYPES.into_iter().zip(&s.props.cells) {
        let cs = s.cell_style(kind);
        let style = if d.text_style(&cs.text_style).is_some() { cs.text_style.clone() } else { "Standard".to_string() };
        w.t.extend(crate::dxf_table::cell_type_tags(&cs, &style, data));
    }
    let x = crate::dxf_table::cell_styles_xdata(s);
    if !x.is_empty() {
        w.s(1001, dxf_ext::APP);
        for c in x {
            w.s(1000, c);
        }
    }
}

/// A GROUP object (DXF Reference, OBJECTS: GROUP): description, unnamed (`*A…`) and selectable
/// flags, and the member entities that were written to this file.
pub(super) fn group_obj(w: &mut W, g: &Group, h: &str, dict: &str, written: &std::collections::HashSet<String>) {
    w.s(0, "GROUP");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbGroup");
    w.s(300, &g.description);
    w.i(70, i64::from(g.name.starts_with('*')));
    w.i(71, i64::from(g.selectable));
    for m in g.members.iter().map(|m| m.hex()).filter(|m| written.contains(m)) {
        w.s(340, m);
    }
}

/// A MLEADERSTYLE object (DXF Reference, OBJECTS: MLEADERSTYLE): MText content and straight
/// ByBlock leaders with the style's landing gap, dogleg length, arrow size, text style and
/// text height.
pub(super) fn mleader_style_obj(w: &mut W, s: &MLeaderStyle, h: &str, dict: &str, cx: &Ctx, text_style: Option<String>) {
    let p = &s.props;
    // A linetype or block the style names, by its record's handle.
    let ltype = |name: &str| cx.ltypes.get(&name.to_ascii_uppercase()).or_else(|| cx.ltypes.get("BYBLOCK")).cloned();
    let block = |name: &str| (!name.is_empty()).then(|| cx.block_records.get(&name.to_ascii_uppercase()).cloned()).flatten();
    w.s(0, "MLEADERSTYLE");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbMLeaderStyle");
    // The style version (and the attachment direction at the end) came with AutoCAD 2010.
    if cx.target >= Version::R2010 {
        w.i(179, i64::from(p.version));
    }
    w.i(170, i64::from(p.content_type));
    w.i(171, i64::from(p.draw_mleader_order));
    w.i(172, i64::from(p.draw_leader_order));
    w.i(90, i64::from(p.max_leader_points));
    w.f(40, p.first_angle);
    w.f(41, p.second_angle);
    w.i(173, i64::from(p.leader_type));
    w.i(91, p.leader_color);
    if let Some(lt) = ltype(&p.leader_linetype) {
        w.s(340, lt);
    }
    w.i(92, i64::from(p.leader_lineweight));
    w.i(290, i64::from(p.landing));
    w.f(42, s.landing_gap);
    w.i(291, i64::from(p.dogleg_on));
    w.f(43, s.dogleg);
    w.s(3, p.description.clone());
    // A user block by name, otherwise the arrowhead block written for it.
    if let Some(a) = block(&p.arrow_block).or_else(|| (!p.arrow_block.trim().is_empty()).then(|| cx.arrow(&p.arrow_block)).flatten()) {
        w.s(341, a);
    }
    w.f(44, s.arrow_size);
    w.s(300, p.default_text.clone());
    if let Some(ts) = text_style {
        w.s(342, ts);
    }
    w.i(174, i64::from(p.text_left_attachment));
    w.i(178, i64::from(p.text_right_attachment));
    w.i(175, i64::from(p.text_angle));
    w.i(176, i64::from(p.text_alignment));
    w.i(93, p.text_color);
    w.f(45, s.text_height);
    w.i(292, i64::from(p.text_frame));
    w.i(297, i64::from(p.text_align_left));
    w.f(46, p.align_space);
    if let Some(b) = block(&p.block) {
        w.s(343, b);
    }
    w.i(94, p.block_color);
    for (c, v) in [(47, p.block_scale[0]), (49, p.block_scale[1]), (140, p.block_scale[2])] {
        w.f(c, v);
    }
    w.i(293, i64::from(p.block_scale_on));
    w.f(141, p.block_rotation);
    w.i(294, i64::from(p.block_rotation_on));
    w.i(177, i64::from(p.block_connection));
    w.f(142, p.scale);
    w.i(295, i64::from(p.overwrite));
    w.i(296, i64::from(s.annotative));
    w.f(143, p.break_gap);
    if cx.target >= Version::R2010 {
        w.i(271, i64::from(p.attachment_direction));
        w.i(272, i64::from(p.bottom_attachment));
        w.i(273, i64::from(p.top_attachment));
    }
    if let Some(g) = p.group_298 {
        w.i(298, i64::from(g));
    }
}

/// The ACAD_SCALELIST dictionary and its SCALE objects (DXF Reference, OBJECTS section): name
/// (300), paper units (140), drawing units (141), unit scale flag (290).
pub(super) fn scale_list(w: &mut W, scales: &[(&cadcraft_doc::AnnoScale, String)], dict: &str, root: &str) {
    w.s(0, "DICTIONARY");
    w.s(5, dict);
    w.group("ACAD_REACTORS", 330, &[root]);
    w.s(330, root);
    w.s(100, "AcDbDictionary");
    w.i(281, 1);
    // Each scale under the key it had in the file (AutoCAD's `A0`, `B1`…), others `S0`, `S1`…
    let kept: HashMap<String, String> =
        w.roles.iter().filter(|((kind, _), _)| kind == "NOD/ACAD_SCALELIST").map(|((_, key), h)| (h.hex(), key.clone())).collect();
    let mut keys: Vec<String> = scales.iter().filter_map(|(_, h)| kept.get(h).cloned()).collect();
    let mut n = 0;
    for (_, h) in scales {
        let key = match kept.get(h) {
            Some(k) => k.clone(),
            None => loop {
                let k = format!("S{n}");
                n += 1;
                if !keys.contains(&k) {
                    keys.push(k.clone());
                    break k;
                }
            },
        };
        w.s(3, key);
        w.s(350, h.clone());
    }
    for (s, h) in scales {
        w.s(0, "SCALE");
        w.s(5, h.clone());
        w.group("ACAD_REACTORS", 330, &[dict]);
        w.s(330, dict);
        w.s(100, "AcDbScale");
        w.i(70, 0);
        w.s(300, dxf_ext::xdata_str(&s.name));
        w.f(140, s.paper_units);
        w.f(141, s.drawing_units);
        w.i(290, i64::from(s.unit_scale.unwrap_or_else(|| cadcraft_doc::same_factor(s.factor(), 1.0))));
    }
}

pub(super) fn cadcraft_fonts_name() -> &'static str {
    "CADCraft Stroke"
}
