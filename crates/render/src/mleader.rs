//! MULTILEADER: the leader lines of every root with their arrowheads, the doglegs, and the
//! content (MText, with its frame, or a block reference). The leader lines and doglegs take the
//! multileader's leader colour, linetype and lineweight, the text its text colour and the block
//! its block colour; ByBlock in any of them is the multileader's own (as a dimension's DIMCLRD
//! ByBlock is the dimension's).

use std::borrow::Cow;

use cadcraft_color::Color;
use cadcraft_doc::{Entity, Lineweight, MLeader, MText};
use cadcraft_geom::{Mat3, Vec2};

use super::blocks::insert;
use super::draw::stroke_with;
use super::text_entity::mtext;
use super::{Builder, Ctx, Ink, default_lineweight, dim, ink};

/// Draw `m` (of entity `e`) whose own colour, lineweight and linetype are `rgb`, `lw` and `lt`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(b: &mut Builder, ctx: &Ctx, e: &Entity, m: &MLeader, rgb: Ink, lw: f32, lt: Option<&cadcraft_doc::Linetype>, ltscale: f64) {
    let d = ctx.d;
    let layer_name = if e.common.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { e.common.layer.as_str() };
    let layer_color = ctx.layer_color(layer_name).unwrap_or(Color::Index(7));
    let layer = d.layer(layer_name);
    let own = |c: Color| if c == Color::ByBlock { rgb } else { ink(c, layer_color, ctx.block_color) };
    let line_rgb = own(m.leader_color);
    let line_lw = if !b.opts.lineweights {
        0.0
    } else {
        match m.leader_lineweight {
            Lineweight::ByBlock => lw,
            Lineweight::Mm100(x) => f32::from(x) / 100.0,
            Lineweight::ByLayer => match layer.map(|l| l.lineweight) {
                Some(Lineweight::Mm100(x)) => f32::from(x) / 100.0,
                _ => default_lineweight(d),
            },
            Lineweight::Default => default_lineweight(d),
        }
    };
    let line_lt = match m.leader_linetype.trim().to_ascii_lowercase().as_str() {
        "" | "byblock" => lt.cloned(),
        "bylayer" => layer.and_then(|l| d.linetype(&l.linetype)).filter(|l| !l.pattern.is_empty()).cloned(),
        name => d.linetype(name).filter(|l| !l.pattern.is_empty()).cloned(),
    };
    // Straight or spline leader lines of every root, with their arrowheads; each root's dogleg
    // towards the content.
    for pts in m.leader_paths() {
        stroke_with(b, ctx, line_rgb, line_lw, line_lt.as_ref(), ltscale, &pts);
        if let (Some(a), Some(n)) = (pts.first(), pts.get(1)) {
            b.tris(ctx, line_rgb, &dim::arrow(*a, (*a - *n).normalized(), m.arrow_size));
        }
    }
    for dogleg in m.dogleg_lines() {
        stroke_with(b, ctx, line_rgb, line_lw, line_lt.as_ref(), ltscale, &dogleg);
    }
    if let Some(t) = &m.text {
        let text_rgb = own(m.text_color);
        mtext(b, ctx, t, text_rgb, lw);
        if m.text_frame {
            let gap = d.mleader_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&m.style)).map_or(0.09, |s| s.landing_gap);
            if let Some(frame) = text_frame(d, t, gap) {
                b.polyline(ctx, text_rgb, lw, &frame);
            }
        }
    }
    if let Some(ins) = &m.block {
        insert(b, ctx, &colored(e, m.block_color), ins, own(m.block_color));
    }
}

/// `e` with colour `c` when it isn't ByBlock: what the block content takes for its ByBlock
/// objects.
fn colored(e: &Entity, c: Color) -> Cow<'_, Entity> {
    if c == Color::ByBlock {
        Cow::Borrowed(e)
    } else {
        let mut x = e.clone();
        x.common.color = c;
        Cow::Owned(x)
    }
}

/// The text frame: the text's box, `gap` away from it, as a closed polyline (turned with the
/// text). None for a text that lays out to nothing.
pub(crate) fn text_frame(d: &cadcraft_doc::Drawing, t: &MText, gap: f64) -> Option<Vec<Vec2>> {
    let flat = MText { rotation: 0.0, ..t.clone() };
    let bx = super::layout_mtext_entity(d, &flat).bounds;
    if bx.is_empty() || !bx.min.is_finite() || !bx.max.is_finite() {
        return None;
    }
    let g = if gap.is_finite() { gap.abs() } else { 0.0 };
    let (lo, hi) = (bx.min - Vec2::new(g, g), bx.max + Vec2::new(g, g));
    let turn = Mat3::rotate_about(t.insert.xy(), if t.rotation.is_finite() { t.rotation } else { 0.0 });
    Some([lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y), lo].iter().map(|p| turn.apply(*p)).collect())
}

#[cfg(test)]
mod tests {
    use crate::{Kind, Options, build};
    use cadcraft_color::{Color, Rgb};
    use cadcraft_doc::{Common, Drawing, EntityKind, Lineweight, MLeader, MLeaderRoot, MText, Space};
    use cadcraft_geom::{Vec2, Vec3};

    fn text_mleader() -> MLeader {
        MLeader {
            leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)]],
            landing: Vec3::new(10.0, 5.0, 0.0),
            dogleg: 2.0,
            text: Some(MText {
                insert: Vec3::new(12.5, 5.0, 0.0),
                height: 2.5,
                width: 0.0,
                attach: 4,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "NOTE".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }),
            arrow_size: 2.5,
            roots: vec![MLeaderRoot { landing: Vec3::new(30.0, 5.0, 0.0), dogleg: 2.0, leaders: vec![vec![Vec3::new(40.0, 0.0, 0.0)]] }],
            ..MLeader::default()
        }
    }

    fn polylines(d: &Drawing, rgb: Rgb) -> Vec<Vec<Vec2>> {
        let l = build(d, &Space::Model, &Options::default());
        l.prims.iter().filter(|p| p.kind == Kind::Polyline && p.color == rgb).map(|p| l.points(p).to_vec()).collect()
    }

    #[test]
    fn leader_lines_and_doglegs_take_the_leader_colour_and_text_its_own() {
        let mut d = Drawing::new_metric();
        let m = MLeader { leader_color: Color::Index(1), text_color: Color::Index(5), ..text_mleader() };
        d.add(&Space::Model, Common { color: Color::Index(3), ..Common::default() }, EntityKind::MLeader(m)).unwrap();
        let red = polylines(&d, Rgb(255, 0, 0));
        // Two leader lines and two doglegs, the second dogleg running left from (30, 5).
        assert_eq!(red.len(), 4, "{red:?}");
        assert!(red.iter().any(|p| p.as_slice() == [Vec2::new(30.0, 5.0), Vec2::new(28.0, 5.0)]));
        let l = build(&d, &Space::Model, &Options::default());
        assert!(l.prims.iter().filter(|p| p.kind == Kind::Tris).all(|p| p.color == Rgb(255, 0, 0)), "red arrowheads");
        assert!(!polylines(&d, Rgb(0, 0, 255)).is_empty(), "blue text");
        assert!(polylines(&d, Rgb(0, 255, 0)).is_empty(), "nothing in the multileader's green");
    }

    #[test]
    fn byblock_overrides_take_the_multileader_colour_and_a_frame_surrounds_the_text() {
        let mut d = Drawing::new_metric();
        let m = MLeader { text_frame: true, ..text_mleader() };
        d.add(&Space::Model, Common { color: Color::Index(3), ..Common::default() }, EntityKind::MLeader(m.clone())).unwrap();
        let l = build(&d, &Space::Model, &Options::default());
        assert!(l.prims.iter().all(|p| p.color == Rgb(0, 255, 0)), "everything ByBlock is green");
        let gap = d.mleader_styles.first().map_or(0.0, |s| s.landing_gap);
        let frame = super::text_frame(&d, m.text.as_ref().unwrap(), gap).unwrap();
        assert_eq!(frame.len(), 5);
        assert!(frame.iter().all(|p| p.x >= 12.5 - gap - 1e-9 && (p.y - 5.0).abs() <= 1.25 + gap + 1e-9), "{frame:?}");
        assert!(polylines(&d, Rgb(0, 255, 0)).iter().any(|p| p.as_slice() == frame.as_slice()));
        // A frame of an empty text is nothing, and a hostile gap or rotation does not panic.
        let empty = MText { contents: String::new(), rotation: f64::NAN, ..m.text.clone().unwrap() };
        let _ = super::text_frame(&d, &empty, f64::INFINITY);
    }

    #[test]
    fn leader_linetype_and_lineweight_override_the_multileaders() {
        let mut d = Drawing::new_metric();
        d.linetypes.extend(cadcraft_doc::library::standard_linetypes().into_iter().filter(|l| l.name == "DASHED"));
        let m = MLeader { leader_linetype: "DASHED".into(), leader_lineweight: Lineweight::Mm100(70), roots: Vec::new(), ..text_mleader() };
        d.add(&Space::Model, Common::default(), EntityKind::MLeader(m)).unwrap();
        let opts = Options { lineweights: true, ..Options::default() };
        let l = build(&d, &Space::Model, &opts);
        let lines: Vec<_> = l.prims.iter().filter(|p| p.kind == Kind::Polyline && p.lw > 0.6).collect();
        // The leader from (0, 0) to (10, 5) is dashed: more than one piece, all 0.7 mm wide.
        assert!(lines.len() > 2, "{}", lines.len());
    }
}
