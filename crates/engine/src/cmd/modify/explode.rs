use cadcraft_geom::{Ellipse as GEllipse, Mat3};

use super::{Entity, EntityKind, Handle, PolyVertex, Polyline, Result, Segment, Session, Value, Vec2, arc, json, line, lwpoly, targets, v3};

// ---------------- explode / array / order / break / join ----------------

/// `ell` placed by `m` as an ELLIPSE at elevation `z`; `None` when `m` flattens it.
fn placed_ellipse(ell: GEllipse, m: &Mat3, z: f64) -> Option<EntityKind> {
    let g = ell.transformed(m);
    let len = g.major.len();
    (len.is_finite() && len > 0.0 && g.ratio.is_finite() && g.ratio > 0.0).then(|| {
        EntityKind::Ellipse(cadcraft_doc::Ellipse { center: g.center.to3(z), major: g.major.to3(0.0), ratio: g.ratio, start: g.start, end: g.end })
    })
}

/// The pieces of block object `be` placed by `m`, a transform that stretches unevenly (a block
/// reference whose X and Y scales differ), where its own kind can't hold the result: a circle
/// or an arc becomes an ellipse or an elliptical arc, a polyline with arc segments (and no
/// widths) its lines and elliptical arcs. `None`: the object transforms as itself.
fn stretched_pieces(be: &Entity, m: &Mat3) -> Option<Vec<Entity>> {
    let circular = |center: Vec2, radius: f64, start: f64, end: f64| GEllipse { center, major: Vec2::X * radius, ratio: 1.0, start, end };
    let kinds = match &be.kind {
        EntityKind::Circle(c) => vec![placed_ellipse(GEllipse::full(c.center.xy(), Vec2::X * c.radius, 1.0), m, c.center.z)?],
        EntityKind::Arc(a) => vec![placed_ellipse(circular(a.center.xy(), a.radius, a.start, a.end), m, a.center.z)?],
        EntityKind::Ellipse(el) => {
            let g = GEllipse { center: el.center.xy(), major: el.major.xy(), ratio: el.ratio, start: el.start, end: el.end };
            vec![placed_ellipse(g, m, el.center.z)?]
        }
        EntityKind::LwPolyline(p)
            if p.vertices.iter().any(|v| v.bulge != 0.0)
                && p.const_width == 0.0
                && p.vertices.iter().all(|v| v.start_width == 0.0 && v.end_width == 0.0) =>
        {
            let z = p.elevation;
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            pl.segments()
                .into_iter()
                .map(|s| match s {
                    Segment::Line(l) => Some(EntityKind::Line(cadcraft_doc::Line { a: m.apply(l.a).to3(z), b: m.apply(l.b).to3(z) })),
                    Segment::Arc { arc: a, .. } => placed_ellipse(circular(a.center, a.radius, a.start, a.end), m, z),
                })
                .collect::<Option<Vec<_>>>()?
        }
        _ => return None,
    };
    // The pieces of a polyline are new objects, as when it explodes itself: its xdata goes.
    let mut common = be.common.clone();
    if matches!(be.kind, EntityKind::LwPolyline(_)) {
        common.xdata.clear();
    }
    Some(kinds.into_iter().map(|kind| Entity { handle: Handle(0), common: common.clone(), kind }).collect())
}

fn explode_block_ref(d: &cadcraft_doc::Drawing, e: &Entity, ins: &cadcraft_doc::Insert) -> Option<Vec<Entity>> {
    if let Some(items) = crate::cmd::array::explode_array(d, e) {
        return Some(items);
    }
    let blk = d.block(&ins.block)?;
    let m = ins.transform(blk.base.xy());
    let stretched = m.similarity_scale().is_none();
    Some(
        blk.entities
            .iter()
            .flat_map(|be| {
                stretched.then(|| stretched_pieces(be, &m)).flatten().unwrap_or_else(|| {
                    let mut ne = (**be).clone();
                    ne.kind.transform(&m);
                    vec![ne]
                })
            })
            .map(|mut ne| {
                if ne.common.layer == "0" {
                    ne.common.layer = e.common.layer.clone();
                }
                if ne.common.color == cadcraft_color::Color::ByBlock {
                    ne.common.color = e.common.color;
                }
                ne
            })
            .collect(),
    )
}

fn explode_mtext(e: &Entity, m: &cadcraft_doc::MText) -> Vec<Entity> {
    cadcraft_fonts::plain_mtext(&m.contents)
        .split('\n')
        .enumerate()
        .map(|(i, l)| {
            let at = m.insert.xy() + Vec2::from_angle(m.rotation - std::f64::consts::FRAC_PI_2) * (m.height * (1.0 + 5.0 / 3.0 * i as f64));
            Entity {
                handle: Handle(0),
                common: e.common.clone(),
                kind: EntityKind::Text(cadcraft_doc::Text {
                    insert: v3(at),
                    align_pt: None,
                    height: m.height,
                    value: l.into(),
                    rotation: m.rotation,
                    width_factor: 1.0,
                    oblique: 0.0,
                    style: m.style.clone(),
                    halign: Default::default(),
                    valign: Default::default(),
                }),
            }
        })
        .collect()
}

/// The entities of a boundary's lines and arcs, on the object's layer and style.
fn segment_entities(e: &Entity, segs: impl IntoIterator<Item = Segment>) -> Vec<Entity> {
    segs.into_iter()
        .map(|s| {
            let k = match s {
                Segment::Line(l) => line(l.a, l.b),
                Segment::Arc { arc: a, .. } => arc(&a),
            };
            Entity { handle: Handle(0), common: e.common.clone(), kind: k }
        })
        .collect()
}

fn explode_dimension(d: &cadcraft_doc::Drawing, e: &Entity, dm: &cadcraft_doc::Dimension) -> Vec<Entity> {
    // The dimension as drawn: its overrides, DIMSCALE (0 = the drawing's) and text font.
    let style = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
    let g = cadcraft_render::dimension_in(d, dm);
    let line_entity = |l: &Vec<Vec2>| {
        if l.len() == 2 {
            Some(Entity { handle: Handle(0), common: e.common.clone(), kind: line(*l.first()?, *l.get(1)?) })
        } else {
            Some(Entity { handle: Handle(0), common: e.common.clone(), kind: lwpoly(l.iter().map(|p| PolyVertex::new(*p)).collect(), false) })
        }
    };
    // The text's marks (arc length symbol, DIMINSPECT frame) go after the text background.
    let text_marks = &g.text_marks;
    let marks = |want: bool| g.lines.iter().enumerate().filter(move |(i, _)| text_marks.contains(i) == want).map(|(_, l)| l);
    let mut v: Vec<Entity> = marks(false).filter_map(&line_entity).collect();
    for t in &g.fills {
        if let [a, b, c] = t.as_slice() {
            v.push(Entity {
                handle: Handle(0),
                common: e.common.clone(),
                kind: EntityKind::Solid(cadcraft_doc::Solid { corners: [v3(*a), v3(*b), v3(*c), v3(*c)] }),
            });
        }
    }
    // DIMTFILL 2: the text background as a SOLID in DIMTFILLCLR, over the lines and
    // arrowheads, under the text and its marks (SOLID corners go 1, 2, 4, 3).
    if style.text_fill == 2
        && let [p0, p1, p2, p3] = g.text_fill.as_slice()
    {
        let fill = style.text_fill_color;
        let color = if fill == cadcraft_doc::color::Color::ByBlock { e.common.color } else { fill };
        v.push(Entity {
            handle: Handle(0),
            common: cadcraft_doc::Common { color, ..e.common.clone() },
            kind: EntityKind::Solid(cadcraft_doc::Solid { corners: [v3(*p0), v3(*p1), v3(*p3), v3(*p2)] }),
        });
    }
    v.extend(marks(true).filter_map(&line_entity));
    if !g.value.is_empty() {
        v.push(Entity {
            handle: Handle(0),
            common: e.common.clone(),
            kind: EntityKind::MText(cadcraft_doc::MText {
                insert: v3(g.text_pos),
                height: g.text_height,
                width: 0.0,
                attach: 5,
                rotation: g.text_angle,
                style: style.text_style.clone(),
                contents: g.mtext_contents(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }),
        });
    }
    v
}

pub(crate) fn explode_kind(d: &cadcraft_doc::Drawing, e: &Entity) -> Option<Vec<Entity>> {
    // An annotative object explodes into plain objects as it draws at the current annotation scale.
    if cadcraft_doc::is_annotative(e) {
        let mut se = match d.anno_draw(e) {
            cadcraft_doc::AnnoDraw::Scaled { factor, by } => cadcraft_doc::anno_placed(e, factor, by),
            _ => e.clone(),
        };
        se.common.anno_scales.clear();
        se.common.anno_moves.clear();
        return explode_kind(d, &se);
    }
    match &e.kind {
        EntityKind::LwPolyline(p) => {
            let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
            Some(segment_entities(e, pl.segments()))
        }
        EntityKind::Insert(ins) => explode_block_ref(d, e, ins),
        // A helix explodes into its spline (seen from above).
        EntityKind::Helix(h) => Some(vec![Entity { handle: Handle(0), common: e.common.clone(), kind: EntityKind::Spline(h.spline()) }]),
        EntityKind::Polyline3d(p) => Some(
            p.points
                .windows(2)
                .filter_map(|w| {
                    let kind = EntityKind::Line(cadcraft_doc::Line { a: *w.first()?, b: *w.get(1)? });
                    Some(Entity { handle: Handle(0), common: e.common.clone(), kind })
                })
                .collect(),
        ),
        // A polyface mesh explodes into its 3D faces; one without any stays as it is.
        EntityKind::PolyfaceMesh(m) => {
            let faces = m.faces3d();
            (!faces.is_empty())
                .then(|| faces.into_iter().map(|f| Entity { handle: Handle(0), common: e.common.clone(), kind: EntityKind::Face3d(f) }).collect())
        }
        EntityKind::MLine(m) => Some(crate::cmd::draw2::mline::explode(e, m)),
        EntityKind::MText(m) => Some(explode_mtext(e, m)),
        // A region explodes into its boundary's lines and arcs.
        EntityKind::Region(r) => Some(segment_entities(e, r.segments())),
        EntityKind::Dimension(dm) => Some(explode_dimension(d, e, dm)),
        EntityKind::Tolerance(t) => Some(crate::cmd::tolerance::explode(d, e, t)),
        _ => None,
    }
}

pub(super) fn explode(s: &mut Session, hs: &[Handle]) -> Result<Vec<Handle>> {
    explode_with(s, hs, explode_kind)
}

/// Replace each of `hs` (off locked layers) by the pieces `pieces` gives for it; objects it gives
/// none for are left alone. EXPLODE, and BURST with its own pieces for block references.
pub(crate) fn explode_with(
    s: &mut Session,
    hs: &[Handle],
    pieces: impl Fn(&cadcraft_doc::Drawing, &Entity) -> Option<Vec<Entity>>,
) -> Result<Vec<Handle>> {
    let space = s.space();
    let mut out = Vec::new();
    for h in hs {
        if crate::cmd::curves::is_locked(s, *h) {
            continue;
        }
        let Some(e) = s.doc()?.entity(*h).map(|e| (**e).clone()) else { continue };
        let Some(parts) = pieces(s.doc()?, &e) else { continue };
        // The pieces of an object are new objects: its xdata goes with it. A block reference's
        // pieces are copies of the block's objects and keep theirs.
        let own_pieces = !matches!(e.kind, EntityKind::Insert(_));
        let d = s.doc_mut()?;
        d.remove_entity(*h);
        for mut p in parts {
            if own_pieces {
                p.common.xdata.clear();
            }
            p.handle = d.new_handle();
            out.push(p.handle);
            if let Some(st) = d.space_mut(&space) {
                st.push(p);
            }
        }
    }
    s.set_selection(Vec::new());
    Ok(out)
}

pub(super) fn run_explode(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let r = explode(s, &hs)?;
    Ok(json!({ "handles": r.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}
