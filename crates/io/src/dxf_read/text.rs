use super::common::{T, common, ocs_n};
use super::{Attrib, AttribModes, AttribProps, Common, HAlign, MText, MultilineAttrib, Tag, Text, VAlign, Vec2, Vec3, mtext_dxf};

/// The properties an ATTRIB carries itself: those it has groups for. Its layer counts only when
/// it differs from its block reference's (an attribute defined on layer 0 goes on the reference's
/// layer, as it is drawn anyway).
pub(super) fn attrib_props(t: &T, insert_layer: &str) -> AttribProps {
    let c = common(t);
    AttribProps {
        layer: t.s(8).filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case(insert_layer)),
        color: (t.i(62).is_some() || t.i(420).is_some()).then_some(c.color),
        linetype: t.s(6).filter(|l| !l.is_empty()),
        lineweight: t.i(370).is_some().then_some(c.lineweight),
    }
}

/// An attribute's modes from its own groups: flags (70) 4 verify and 8 preset, and Lock
/// position, the 280 after its tag in its subclass (a 280 before the tag is the subclass version).
pub(super) fn attrib_modes(own: &[Tag]) -> AttribModes {
    let flags = own.iter().find(|t| t.code == 70).map(Tag::i64).unwrap_or(0);
    let sub = own.iter().position(|t| t.code == 100 && t.str().trim().starts_with("AcDbAttribute")).unwrap_or(0);
    let rest = own.get(sub..).unwrap_or_default();
    let lock_position = rest.iter().position(|t| t.code == 2).is_some_and(|at| rest.iter().skip(at + 1).any(|t| t.code == 280 && t.i64() != 0));
    AttribModes { verify: flags & 4 != 0, preset: flags & 8 != 0, lock_position }
}

/// An ATTRIB's or ATTDEF's groups split at its embedded multiline text (`101 Embedded Object`):
/// the attribute's own groups and, for a multiline attribute, the MTEXT groups after the marker.
pub(super) fn split_embedded(tags: &[Tag]) -> (&[Tag], Option<&[Tag]>) {
    match tags.iter().position(|t| t.code == 101) {
        Some(i) => (tags.get(..i).unwrap_or(tags), tags.get(i + 1..)),
        None => (tags, None),
    }
}

/// A multiline attribute's text, from the MTEXT groups embedded in it.
pub(super) fn multiline_attrib(embedded: Option<&[Tag]>, c: &Common) -> Option<MultilineAttrib> {
    let mtext = mtext_from(embedded.filter(|e| !e.is_empty())?, c);
    let read_as = cadcraft_fonts::plain_mtext(&mtext.contents);
    Some(MultilineAttrib { mtext, read_as })
}

/// Multiline text from MTEXT groups (an MTEXT entity's, or a multiline attribute's embedded ones).
pub(super) fn mtext_from(tags: &[Tag], c: &Common) -> MText {
    let t = T(tags);
    // Contents: code 3 chunks followed by code 1.
    let mut contents = String::new();
    for tg in tags.iter().filter(|x| x.code == 3) {
        contents.push_str(&tg.str());
    }
    contents.push_str(&t.s(1).unwrap_or_default());
    let contents = mtext_dxf::decode_contents(&contents);
    let columns = mtext_dxf::read_columns(tags);
    // The direction comes from the MTEXT's own groups only: in an R2018+ embedded object
    // (group 101), 11 is the insertion point, not the X-axis direction (#667, as in #834).
    let own = T(mtext_dxf::before_columns(tags));
    let mut rotation = match (own.f(11), own.f(21)) {
        (Some(x), Some(y)) if x != 0.0 || y != 0.0 => Vec2::new(x, y).angle(),
        _ => own.fd(50, 0.0).to_radians(),
    };
    let mut attach = t.i(71).unwrap_or(1).clamp(1, 9) as u8;
    if c.extrusion.z < 0.0 {
        // Insertion point and direction are WCS, but the line height runs along extrusion ×
        // direction: under (0,0,-1) the glyphs are mirrored. Drawn readable (MIRRTEXT = 0) with
        // the same footprint, the text turns round and left/right attachment swap.
        rotation = cadcraft_geom::norm_angle(rotation + std::f64::consts::PI);
        attach = match attach {
            1 | 4 | 7 => attach + 2,
            3 | 6 | 9 => attach - 2,
            a => a,
        };
    }
    MText {
        insert: t.p(10),
        height: t.fd(40, 0.2).max(1e-9),
        width: match &columns {
            Some((_, w)) if t.fd(41, 0.0) <= 0.0 => *w,
            _ => t.fd(41, 0.0).max(0.0),
        },
        attach,
        rotation,
        style: t.s(7).unwrap_or_else(|| "Standard".into()),
        contents,
        line_spacing: t.fd(44, 1.0),
        line_spacing_exact: t.i(73) == Some(2),
        columns: columns.map(|(c, _)| c),
    }
}

pub(super) fn text_from(t: &T) -> Text {
    let h = match t.i(72).unwrap_or(0) {
        1 => HAlign::Center,
        2 => HAlign::Right,
        3 => HAlign::Aligned,
        4 => HAlign::Middle,
        5 => HAlign::Fit,
        _ => HAlign::Left,
    };
    let v = match t.i(73).or_else(|| t.i(74)).unwrap_or(0) {
        1 => VAlign::Bottom,
        2 => VAlign::Middle,
        3 => VAlign::Top,
        _ => VAlign::Baseline,
    };
    let align = if t.f(11).is_some() { Some(t.p(11)) } else { None };
    let mut text = Text {
        insert: t.p(10),
        align_pt: if h == HAlign::Left && v == VAlign::Baseline { None } else { align },
        height: t.fd(40, 0.2).max(1e-9),
        value: t.s(1).unwrap_or_default(),
        rotation: t.fd(50, 0.0).to_radians(),
        width_factor: t.fd(41, 1.0),
        oblique: t.fd(51, 0.0).to_radians(),
        style: t.s(7).unwrap_or_else(|| "Standard".into()),
        halign: h,
        valign: v,
    };
    let n = Vec3::new(t.fd(210, 0.0), t.fd(220, 0.0), t.fd(230, 1.0));
    if n.z < 0.0 {
        mirror_text(&mut text, align.map(|a| ocs_n(n, a)), ocs_n(n, t.p(10)));
    }
    text
}

/// TEXT / ATTRIB / ATTDEF with extrusion (0,0,-1): the points are OCS and the text runs from its
/// insertion point towards -x in the WCS, with mirrored glyphs. CADCraft draws text readable
/// (MIRRTEXT = 0), so it keeps the mirrored footprint: the direction turns to -rotation and
/// left/right justification swap ends (start ↔ end point).
fn mirror_text(t: &mut Text, align: Option<Vec3>, insert: Vec3) {
    t.rotation = cadcraft_geom::norm_angle(-t.rotation);
    let baseline_left = |t: &Text, p: Vec3| if t.valign == VAlign::Baseline { None } else { Some(p) };
    match t.halign {
        HAlign::Left => {
            // TL/ML/BL are placed by their alignment point, baseline-left text by its start point.
            t.halign = HAlign::Right;
            t.insert = insert;
            t.align_pt = Some(if t.valign == VAlign::Baseline { insert } else { align.unwrap_or(insert) });
        }
        HAlign::Right => {
            t.halign = HAlign::Left;
            t.insert = align.unwrap_or(insert);
            t.align_pt = baseline_left(t, t.insert);
        }
        HAlign::Aligned | HAlign::Fit => {
            t.insert = align.unwrap_or(insert);
            t.align_pt = Some(insert);
        }
        HAlign::Center | HAlign::Middle => {
            t.insert = insert;
            t.align_pt = align;
        }
    }
}

/// An ATTDEF, whose multiline text is embedded MTEXT after its own groups.
pub(super) fn attdef(tags: &[Tag], c: &Common) -> Attrib {
    // A multiline definition has its text as embedded MTEXT after its own groups.
    let (own, embedded) = split_embedded(tags);
    let multiline = multiline_attrib(embedded, c);
    let own = T(own);
    let mut text = text_from(&own);
    if let Some(m) = &multiline {
        text.value.clone_from(&m.read_as);
    }
    Attrib {
        tag: own.s(2).unwrap_or_default(),
        text,
        invisible: own.i(70).unwrap_or(0) & 1 != 0,
        constant: own.i(70).unwrap_or(0) & 2 != 0,
        prompt: own.s(3).unwrap_or_default(),
        props: Default::default(),
        multiline,
        modes: attrib_modes(own.0),
    }
}
