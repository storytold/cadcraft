use super::{Drawing, Group, MLeaderStyle, TableStyle, W, dxf_ext};

/// A TABLESTYLE object (DXF Reference, OBJECTS: TABLESTYLE): margins, title/header
/// suppression and per row type (data, column header, title) text and border settings.
pub(super) fn table_style_obj(w: &mut W, d: &Drawing, s: &TableStyle, h: &str, dict: &str) {
    w.s(0, "TABLESTYLE");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbTableStyle");
    w.s(3, "");
    w.i(70, 0);
    w.i(71, 0);
    w.f(40, s.margin);
    w.f(41, s.margin);
    w.i(280, i64::from(!s.title));
    w.i(281, i64::from(!s.header));
    // Data, column header and title cell styles; CADCraft's complete cell styles in xdata.
    for kind in crate::dxf_table::CELL_TYPES {
        let cs = s.cell_style(kind);
        let style = if d.text_style(&cs.text_style).is_some() { cs.text_style.clone() } else { "Standard".to_string() };
        w.t.extend(crate::dxf_table::cell_type_tags(&cs, &style));
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

/// What a MLEADERSTYLE object points at in the file being written (record handles).
pub(super) struct StyleRefs {
    pub leader_linetype: Option<String>,
    pub text_style: Option<String>,
    pub block: Option<String>,
}

/// A MLEADERSTYLE object (DXF Reference, OBJECTS: MLEADERSTYLE): content type, leader type,
/// colours, linetype and lineweight, landing, arrowhead size, text and block content settings.
pub(super) fn mleader_style_obj(w: &mut W, s: &MLeaderStyle, h: &str, dict: &str, r: &StyleRefs) {
    use crate::dxf_mleader::color_value;
    use cadcraft_doc::{MLeaderContent, MLeaderLineType};
    let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
    w.s(0, "MLEADERSTYLE");
    w.s(5, h);
    w.group("ACAD_REACTORS", 330, &[dict]);
    w.s(330, dict);
    w.s(100, "AcDbMLeaderStyle");
    w.i(179, 2);
    w.i(
        170,
        match s.content {
            MLeaderContent::None => 0,
            MLeaderContent::Block => 1,
            MLeaderContent::MText => 2,
        },
    );
    w.i(171, 1);
    w.i(172, 0);
    w.i(90, i64::from(s.max_points.clamp(2, 64)));
    w.f(40, finite(s.first_angle, 0.0));
    w.f(41, finite(s.second_angle, 0.0));
    w.i(
        173,
        match s.leader_type {
            MLeaderLineType::None => 0,
            MLeaderLineType::Straight => 1,
            MLeaderLineType::Spline => 2,
        },
    );
    w.i(91, color_value(s.leader_color));
    if let Some(lt) = &r.leader_linetype {
        w.s(340, lt);
    }
    w.i(92, i64::from(s.leader_lineweight.to_dxf()));
    w.i(290, i64::from(s.landing));
    w.f(42, s.landing_gap);
    w.i(291, i64::from(s.landing));
    w.f(43, s.dogleg);
    w.s(3, &s.description);
    w.f(44, s.arrow_size);
    w.s(300, &s.default_text);
    if let Some(ts) = &r.text_style {
        w.s(342, ts);
    }
    w.i(174, 1);
    w.i(178, 1);
    w.i(175, 1);
    w.i(176, 0);
    w.i(93, color_value(s.text_color));
    w.f(45, s.text_height);
    w.i(292, i64::from(s.text_frame));
    w.i(297, 0);
    w.f(46, 4.0);
    if let Some(b) = &r.block {
        w.s(343, b);
    }
    w.i(94, color_value(s.block_color));
    let scale = finite(s.block_scale, 1.0);
    for c in [47, 49, 140] {
        w.f(c, scale);
    }
    w.i(293, 1);
    w.f(141, finite(s.block_rotation, 0.0));
    w.i(294, 1);
    w.i(177, i64::from(!s.block_center));
    w.f(142, s.size_factor());
    w.i(295, 0);
    w.i(296, i64::from(s.annotative));
    w.f(143, finite(s.break_size, 0.125));
    w.i(271, 0);
    w.i(272, 9);
    w.i(273, 9);
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
    for (i, (_, h)) in scales.iter().enumerate() {
        w.s(3, format!("S{i}"));
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
        w.i(290, i64::from(cadcraft_doc::same_factor(s.factor(), 1.0)));
    }
}

pub(super) fn cadcraft_fonts_name() -> &'static str {
    "CADCraft Stroke"
}
