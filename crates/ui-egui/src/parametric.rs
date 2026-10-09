//! Parametric display: geometric constraint bars and dynamic dimensional constraints drawn over
//! the canvas (from the engine's `constraints.inspect`), inline editing of a dimensional
//! constraint's expression, and the Parameters Manager.
//!
//! Every edit runs an engine command (`parameters {name, expr}` / `{delete}`); this module only
//! draws and collects input.

use cadcraft_doc::{ConstraintKind, DistAxis, Drawing, EntityKind, GeomRef, Handle, Sub};
use cadcraft_geom::Vec2;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::CadApp;
use crate::canvas::Xf;
use crate::theme::Tokens;

/// One constraint as reported by `constraints.inspect` (`glyphs`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Glyph {
    pub id: u32,
    pub kind: String,
    pub dimensional: bool,
    pub handles: Vec<String>,
    pub anchors: Vec<Vec2>,
    pub name: String,
    pub expr: String,
    pub value: Option<f64>,
    pub satisfied: bool,
}

/// Per-drawing-revision cache of the constraint glyphs (inspect is not free on big drawings).
#[derive(Default)]
pub struct Cache {
    key: Option<(u64, u64, usize)>,
    glyphs: Vec<Glyph>,
    /// The dimensional constraint whose expression is being edited: (name, buffer, screen pos).
    editing: Option<(String, String, Pos2)>,
}

fn doc_key(app: &CadApp) -> Option<(u64, u64, usize)> {
    let st = app.session.state().ok()?;
    Some((st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize))
}

fn refresh(app: &mut CadApp) {
    let key = doc_key(app);
    if key.is_none() || app.canvas.param.key == key {
        return;
    }
    app.canvas.param.key = key;
    app.canvas.param.glyphs.clear();
    if app.session.doc().is_ok_and(|d| d.constraints.is_empty()) {
        return;
    }
    if let Ok(v) = app.session.execute("constraints.inspect", &json!({}))
        && let Some(g) = v.get("glyphs")
    {
        app.canvas.param.glyphs = serde_json::from_value(g.clone()).unwrap_or_default();
    }
}

// ---------------- geometry of constraint references ----------------

fn ent_kind(d: &Drawing, h: Handle) -> Option<&EntityKind> {
    d.entity(h).map(|e| &e.kind)
}

fn poly_vertex(p: &cadcraft_doc::LwPolyline, i: u32) -> Option<Vec2> {
    p.vertices.get(usize::try_from(i).ok()?).map(|v| v.p)
}

fn ref_point(d: &Drawing, r: &GeomRef) -> Option<Vec2> {
    let k = ent_kind(d, r.handle)?;
    match (k, r.sub) {
        (EntityKind::Line(l), Sub::Start) => Some(l.a.xy()),
        (EntityKind::Line(l), Sub::End) => Some(l.b.xy()),
        (EntityKind::Line(l), Sub::Mid) => Some(l.a.xy().mid(l.b.xy())),
        (EntityKind::Arc(a), s) => {
            let g = cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
            match s {
                Sub::Start => Some(g.start_point()),
                Sub::End => Some(g.end_point()),
                Sub::Mid => Some(g.mid_point()),
                _ => Some(g.center),
            }
        }
        (EntityKind::Circle(c), _) => Some(c.center.xy()),
        (EntityKind::Point(p), _) => Some(p.p.xy()),
        (EntityKind::LwPolyline(p), Sub::Vertex(i)) => poly_vertex(p, i),
        _ => ref_line(d, r).map(|(a, b)| a.mid(b)),
    }
}

fn ref_line(d: &Drawing, r: &GeomRef) -> Option<(Vec2, Vec2)> {
    match (ent_kind(d, r.handle)?, r.sub) {
        (EntityKind::Line(l), Sub::Whole) => Some((l.a.xy(), l.b.xy())),
        (EntityKind::LwPolyline(p), Sub::Segment(i)) => {
            let n = p.vertices.len();
            let a = poly_vertex(p, i)?;
            let j = usize::try_from(i).ok()?.checked_add(1)?;
            let b = if j < n {
                p.vertices.get(j)?.p
            } else if p.closed {
                p.vertices.first()?.p
            } else {
                return None;
            };
            Some((a, b))
        }
        _ => None,
    }
}

fn ref_circle(d: &Drawing, r: &GeomRef) -> Option<(Vec2, f64)> {
    match ent_kind(d, r.handle)? {
        EntityKind::Circle(c) => Some((c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some((a.center.xy(), a.radius)),
        _ => None,
    }
}

// ---------------- glyph drawing ----------------

/// Draw a constraint type's glyph into `r` (code-drawn; no image assets).
pub fn paint_glyph(p: &egui::Painter, r: Rect, kind: &str, c: Color32) {
    let s = Stroke::new(1.4, c);
    let at = |x: f32, y: f32| pos2(r.left() + r.width() * x / 16.0, r.top() + r.height() * y / 16.0);
    let k = r.width() / 16.0;
    match kind {
        "Coincident" => {
            p.line_segment([at(2.0, 13.0), at(8.0, 8.0)], s);
            p.line_segment([at(8.0, 8.0), at(14.0, 12.0)], s);
            p.circle_filled(at(8.0, 8.0), 2.4 * k, c);
        }
        "Parallel" => {
            p.line_segment([at(3.0, 13.0), at(9.0, 3.0)], s);
            p.line_segment([at(7.0, 13.0), at(13.0, 3.0)], s);
        }
        "Perpendicular" => {
            p.line_segment([at(8.0, 3.0), at(8.0, 13.0)], s);
            p.line_segment([at(2.5, 13.0), at(13.5, 13.0)], s);
        }
        "Horizontal" => {
            p.line_segment([at(2.0, 8.0), at(14.0, 8.0)], s);
            p.line_segment([at(2.0, 5.0), at(2.0, 11.0)], s);
            p.line_segment([at(14.0, 5.0), at(14.0, 11.0)], s);
        }
        "Vertical" => {
            p.line_segment([at(8.0, 2.0), at(8.0, 14.0)], s);
            p.line_segment([at(5.0, 2.0), at(11.0, 2.0)], s);
            p.line_segment([at(5.0, 14.0), at(11.0, 14.0)], s);
        }
        "Tangent" => {
            p.circle_stroke(at(8.0, 10.0), 4.0 * k, s);
            p.line_segment([at(1.5, 5.6), at(14.5, 5.6)], s);
        }
        "Smooth" => {
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    at(2.0 + 12.0 * t, 8.0 - 4.5 * (t * std::f32::consts::TAU).sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
        }
        "Collinear" => {
            p.line_segment([at(2.0, 13.0), at(14.0, 3.0)], s);
            for (x, y) in [(4.0, 11.3), (8.0, 8.0), (12.0, 4.7)] {
                p.circle_filled(at(x, y), 1.6 * k, c);
            }
        }
        "Concentric" => {
            p.circle_stroke(at(8.0, 8.0), 6.0 * k, s);
            p.circle_stroke(at(8.0, 8.0), 2.6 * k, s);
        }
        "Equal" => {
            p.line_segment([at(3.0, 6.0), at(13.0, 6.0)], Stroke::new(1.6, c));
            p.line_segment([at(3.0, 10.5), at(13.0, 10.5)], Stroke::new(1.6, c));
        }
        "Symmetric" => {
            for y in [2.0, 6.0, 10.0] {
                p.line_segment([at(8.0, y), at(8.0, y + 2.4)], s);
            }
            p.add(Shape::convex_polygon(vec![at(6.5, 4.0), at(6.5, 12.0), at(1.5, 8.0)], c, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![at(9.5, 4.0), at(14.5, 8.0), at(9.5, 12.0)], c, Stroke::NONE));
        }
        "Fix" => lock(p, r, c),
        _ => {
            p.circle_stroke(r.center(), 5.0 * k, s);
        }
    }
}

/// A small padlock.
pub fn lock(p: &egui::Painter, r: Rect, c: Color32) {
    let k = r.width() / 16.0;
    let body = Rect::from_min_max(pos2(r.left() + 3.5 * k, r.top() + 7.0 * k), pos2(r.left() + 12.5 * k, r.top() + 14.0 * k));
    p.rect_filled(body, 1.0, c);
    let pts: Vec<Pos2> = (0..=10)
        .map(|i| {
            let a = std::f32::consts::PI * i as f32 / 10.0;
            pos2(r.left() + (8.0 + 3.0 * a.cos()) * k, r.top() + (7.0 - 4.0 * a.sin()) * k)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(1.4 * k.max(0.8), c)));
}

fn arrow(p: &egui::Painter, tip: Pos2, from: Pos2, c: Color32) {
    let d = tip - from;
    let l = d.length().max(1e-3);
    let u = d / l;
    let n = vec2(-u.y, u.x);
    p.add(Shape::convex_polygon(vec![tip, tip - u * 8.0 + n * 2.5, tip - u * 8.0 - n * 2.5], c, Stroke::NONE));
}

// ---------------- the canvas overlay ----------------

/// What the overlay asked for this frame.
enum Act {
    Edit(String, String, Pos2),
}

/// Draw constraint bars and dynamic constraints. `xf` maps model space to the screen.
pub fn draw_overlay(app: &mut CadApp, ui: &mut egui::Ui, painter: &egui::Painter, xf: &Xf) {
    refresh(app);
    let Ok(d) = app.session.doc() else { return };
    if d.constraints.is_empty() {
        app.canvas.param.editing = None;
        return;
    }
    let t = Tokens::get();
    let set = &d.parametric.settings;
    let mut act: Option<Act> = None;
    let glyphs = &app.canvas.param.glyphs;
    let (lu, lp) = (d.header.i64("LUNITS", 2), d.header.i64("LUPREC", 4));
    // ----- dynamic (dimensional) constraints -----
    let dim_c = Color32::from_rgb(0x9f, 0xb6, 0xd6);
    let off_px: f32 = 26.0;
    let off = f64::from(off_px) / xf.scale.max(1e-300);
    for c in d.constraints.iter().filter(|c| c.kind.is_dimensional()).take(2000) {
        let shown = set.dims_visible != c.refs.iter().any(|r| set.dim_exceptions.contains(&r.handle));
        if !shown {
            continue;
        }
        let g = glyphs.iter().find(|g| g.id == c.id);
        let value = g.and_then(|g| g.value);
        // A plain number shows the formatted value; an expression shows itself (`d2=d1/2`).
        let label_value = match (c.expr.trim().parse::<f64>().is_ok(), value) {
            (true, Some(v)) if c.kind == ConstraintKind::Angular => format!("{}°", trim_num(v, 2)),
            (true, Some(v)) => cadcraft_engine::units::format_distance(v, lu, lp),
            _ => c.expr.clone(),
        };
        let label = format!("{}={}", c.name, label_value);
        let ok = g.is_none_or(|g| g.satisfied);
        let col = if ok { dim_c } else { Color32::from_rgb(0xff, 0x8a, 0x6a) };
        let st = Stroke::new(1.0, col);
        // Where the label goes (screen) and the dimension graphics.
        let mut label_at: Option<Pos2> = None;
        match c.kind {
            ConstraintKind::Distance(axis) => {
                let (p1, p2) = match (c.refs.first(), c.refs.get(1)) {
                    (Some(a), Some(b)) => match (ref_point(d, a), ref_point(d, b)) {
                        (Some(p), Some(q)) => (p, q),
                        _ => continue,
                    },
                    (Some(a), None) => match ref_line(d, a) {
                        Some(l) => l,
                        None => continue,
                    },
                    _ => continue,
                };
                let (q1, q2) = match axis {
                    DistAxis::Horizontal => {
                        let y = p1.y.max(p2.y) + off;
                        (Vec2::new(p1.x, y), Vec2::new(p2.x, y))
                    }
                    DistAxis::Vertical => {
                        let x = p1.x.max(p2.x) + off;
                        (Vec2::new(x, p1.y), Vec2::new(x, p2.y))
                    }
                    DistAxis::Aligned => {
                        let dir = p2 - p1;
                        if dir.len() < 1e-12 {
                            continue;
                        }
                        let mut n = dir.normalized().perp();
                        // Keep the dimension above/left of the object.
                        if n.y < -1e-9 || (n.y.abs() <= 1e-9 && n.x > 0.0) {
                            n = n * -1.0;
                        }
                        (p1 + n * off, p2 + n * off)
                    }
                };
                let (s1, s2, e1, e2) = (xf.to_screen(p1), xf.to_screen(p2), xf.to_screen(q1), xf.to_screen(q2));
                for (a, b) in [(s1, e1), (s2, e2)] {
                    let v = b - a;
                    let ext = if v.length() > 1e-3 { b + v.normalized() * 4.0 } else { b };
                    p_line(painter, a, ext, st);
                }
                p_line(painter, e1, e2, st);
                if (e2 - e1).length() > 18.0 {
                    arrow(painter, e1, e2, col);
                    arrow(painter, e2, e1, col);
                }
                label_at = Some(e1 + (e2 - e1) * 0.5);
            }
            ConstraintKind::Radius | ConstraintKind::Diameter => {
                let Some((cen, r)) = c.refs.first().and_then(|r| ref_circle(d, r)) else { continue };
                let u = Vec2::new(std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
                let edge = xf.to_screen(cen + u * r);
                let start = if c.kind == ConstraintKind::Diameter { xf.to_screen(cen - u * r) } else { xf.to_screen(cen) };
                p_line(painter, start, edge, st);
                arrow(painter, edge, start, col);
                if c.kind == ConstraintKind::Diameter {
                    arrow(painter, start, edge, col);
                }
                label_at = Some(edge + vec2(18.0, -12.0));
            }
            ConstraintKind::Angular => {
                let (Some(l1), Some(l2)) = (c.refs.first().and_then(|r| ref_line(d, r)), c.refs.get(1).and_then(|r| ref_line(d, r))) else {
                    continue;
                };
                let Some(o) = intersect(l1, l2) else { continue };
                let far = |l: (Vec2, Vec2)| if l.0.dist(o) > l.1.dist(o) { l.0 } else { l.1 };
                let (a1, a2) = ((far(l1) - o).angle(), (far(l2) - o).angle());
                let mut sweep = a2 - a1;
                while sweep > std::f64::consts::PI {
                    sweep -= std::f64::consts::TAU;
                }
                while sweep < -std::f64::consts::PI {
                    sweep += std::f64::consts::TAU;
                }
                let rad = off * 1.6;
                let pts: Vec<Pos2> = (0..=24).map(|i| xf.to_screen(Vec2::polar(o, rad, a1 + sweep * f64::from(i) / 24.0))).collect();
                if let (Some(&f), Some(&l)) = (pts.first(), pts.last()) {
                    painter.add(Shape::line(pts.clone(), st));
                    if let (Some(&f2), Some(&l2)) = (pts.get(2), pts.get(pts.len().saturating_sub(3))) {
                        arrow(painter, f, f2, col);
                        arrow(painter, l, l2, col);
                    }
                }
                label_at = Some(xf.to_screen(Vec2::polar(o, rad * 1.35, a1 + sweep / 2.0)));
            }
            _ => {}
        }
        let Some(at) = label_at else { continue };
        if !xf.rect.expand(40.0).contains(at) {
            continue;
        }
        // The label: lock icon + `name=value`.
        let galley = painter.layout_no_wrap(label.clone(), crate::theme::small(), col);
        let size = galley.size() + vec2(22.0, 4.0);
        let r = Rect::from_center_size(at, size);
        painter.rect_filled(r, 2.0, t.canvas.gamma_multiply(0.85));
        lock(painter, Rect::from_min_size(r.min + vec2(3.0, (r.height() - 13.0) / 2.0), vec2(13.0, 13.0)), col);
        painter.galley(r.min + vec2(19.0, 2.0), galley, col);
        let resp = ui.interact(r, ui.id().with(("dimcon", c.id)), Sense::click());
        if resp.double_clicked() {
            act = Some(Act::Edit(c.name.clone(), c.expr.clone(), r.left_center()));
        }
        resp.on_hover_text(crate::tf!(
            "{kind} constraint {name} = {expression}  (double-click to edit)",
            kind = crate::i18n::t(c.kind.name()),
            name = c.name,
            expression = c.expr
        ));
    }
    // ----- geometric constraint bars -----
    let mut bars: Vec<(Handle, Vec2, Vec<(&str, u32)>)> = Vec::new();
    let mut dots: Vec<Vec2> = Vec::new();
    for g in glyphs.iter().filter(|g| !g.dimensional).take(5000) {
        let hs: Vec<Handle> = g.handles.iter().filter_map(|h| Handle::parse_hex(h)).collect();
        if g.kind == "Coincident" {
            if let Some(a) = g.anchors.first()
                && hs.iter().any(|h| set.bars_visible != set.bar_exceptions.contains(h))
            {
                dots.push(*a);
            }
            continue;
        }
        let aligned = g.anchors.len() == hs.len();
        let mut seen: Vec<Handle> = Vec::new();
        for (i, h) in hs.iter().enumerate() {
            if seen.contains(h) || set.bars_visible == set.bar_exceptions.contains(h) {
                continue;
            }
            seen.push(*h);
            let anchor = if aligned { g.anchors.get(i) } else { g.anchors.first() };
            let Some(anchor) = anchor.copied() else { continue };
            match bars.iter_mut().find(|b| b.0 == *h) {
                Some(b) => b.2.push((g.kind.as_str(), g.id)),
                None => bars.push((*h, anchor, vec![(g.kind.as_str(), g.id)])),
            }
        }
    }
    let alpha = 1.0 - f32::from(set.bar_transparency.min(90)) / 100.0 * 0.75;
    for a in &dots {
        let sp = xf.to_screen(*a);
        if xf.rect.contains(sp) {
            let r = Rect::from_center_size(sp, vec2(6.0, 6.0));
            painter.rect_filled(r, 0.0, Color32::from_rgb(0x3d, 0x8b, 0xfd).gamma_multiply(alpha.max(0.5)));
        }
    }
    let cell = 18.0;
    let mut placed: Vec<Rect> = Vec::new();
    for (h, anchor, kinds) in &bars {
        let sp = xf.to_screen(*anchor);
        if !xf.rect.contains(sp) {
            continue;
        }
        let n = kinds.len().min(16) as f32;
        let mut r = Rect::from_min_size(sp + vec2(8.0, 8.0), vec2(n * cell + 4.0, cell + 4.0));
        for _ in 0..12 {
            if placed.iter().any(|p| p.intersects(r)) {
                r = r.translate(vec2(0.0, cell + 6.0));
            } else {
                break;
            }
        }
        placed.push(r);
        let fill = Color32::from_rgb(0xb9, 0xbe, 0xc6).gamma_multiply(alpha);
        painter.rect_filled(r, 4.0, fill);
        painter.rect_stroke(r, 4.0, Stroke::new(1.0, Color32::from_rgb(0x6e, 0x75, 0x80).gamma_multiply(alpha)), egui::StrokeKind::Inside);
        for (i, (kind, id)) in kinds.iter().take(16).enumerate() {
            let cr = Rect::from_min_size(r.min + vec2(2.0 + i as f32 * cell, 2.0), vec2(cell, cell));
            let resp = ui.interact(cr, ui.id().with(("cbar", h.0, *id)), Sense::hover());
            if resp.hovered() {
                painter.rect_filled(cr.shrink(1.0), 3.0, Color32::from_rgb(0xf2, 0xf4, 0xf7));
            }
            paint_glyph(painter, cr.shrink(2.0), kind, Color32::from_rgb(0x26, 0x2b, 0x33));
            resp.on_hover_text(crate::tf!("{kind} (constraint {id})", kind = crate::i18n::t(kind), id = id));
        }
    }
    let _ = act_take(app, act, ui);
}

fn act_take(app: &mut CadApp, act: Option<Act>, ui: &mut egui::Ui) -> Option<()> {
    if let Some(Act::Edit(name, expr, at)) = act {
        app.canvas.param.editing = Some((name, expr, at));
    }
    // The inline expression editor.
    let (name, mut buf, at) = app.canvas.param.editing.clone()?;
    let mut done: Option<bool> = None;
    egui::Area::new(egui::Id::new("dimcon_edit")).order(egui::Order::Foreground).fixed_pos(at - vec2(0.0, 12.0)).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{name} ="));
                let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(140.0).id(egui::Id::new("dimcon_edit_te")));
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                if r.lost_focus() {
                    done = Some(ui.input(|i| i.key_pressed(egui::Key::Enter)));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
    });
    match done {
        Some(true) => {
            app.canvas.param.editing = None;
            let _ = app.run("parameters", json!({ "name": name, "expr": buf.trim() }));
        }
        Some(false) => app.canvas.param.editing = None,
        None => app.canvas.param.editing = Some((name, buf, at)),
    }
    Some(())
}

fn p_line(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.line_segment([a, b], s);
}

fn trim_num(v: f64, prec: usize) -> String {
    let s = format!("{v:.prec$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

fn intersect(l1: (Vec2, Vec2), l2: (Vec2, Vec2)) -> Option<Vec2> {
    let d1 = l1.1 - l1.0;
    let d2 = l2.1 - l2.0;
    let den = d1.x * d2.y - d1.y * d2.x;
    if den.abs() < 1e-12 * d1.len().max(1e-12) * d2.len().max(1e-12) {
        return None;
    }
    let w = l2.0 - l1.0;
    let t = (w.x * d2.y - w.y * d2.x) / den;
    let p = l1.0 + d1 * t;
    p.is_finite().then_some(p)
}

// ---------------- Parameters Manager ----------------

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Row {
    name: String,
    expr: String,
    value: Option<f64>,
    error: Option<String>,
    kind: String,
    description: String,
}

/// The Parameters Manager (PARAMETERS): name / expression / value; edit, add, delete.
pub fn parameters_dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let rows: Vec<Row> = app
        .session
        .execute("parameters", &json!({}))
        .ok()
        .and_then(|v| v.get("parameters").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let filter_id = egui::Id::new("par_filter");
    let mut filter = ctx.data_mut(|d| d.get_temp::<String>(filter_id)).unwrap_or_default();
    let mut action: Option<Value> = None;
    egui::Window::new(crate::tl!("Parameters Manager"))
        .id(egui::Id::new("Parameters Manager"))
        .open(open)
        .default_size(vec2(560.0, 300.0))
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(crate::tl!("ƒx  New User Parameter")).on_hover_text(crate::tl!("Creates a user parameter")).clicked() {
                    let mut n = 1;
                    while rows.iter().any(|r| r.name == format!("user{n}")) {
                        n += 1;
                    }
                    action = Some(json!({ "name": format!("user{n}"), "expr": "1" }));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut filter).hint_text(crate::tl!("Search for parameter")).desired_width(170.0));
                });
            });
            ui.separator();
            let f = filter.to_ascii_lowercase();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("par_grid").striped(true).num_columns(5).spacing(vec2(12.0, 4.0)).min_col_width(60.0).show(ui, |ui| {
                    for h in ["Name", "Expression", "Value", "Type", ""] {
                        ui.label(egui::RichText::new(crate::i18n::t(h)).strong());
                    }
                    ui.end_row();
                    for (kind, title) in [("dimensional", "Dimensional Constraint Parameters"), ("user", "User Parameters")] {
                        let group: Vec<&Row> =
                            rows.iter().filter(|r| r.kind == kind && (f.is_empty() || r.name.to_ascii_lowercase().contains(&f))).collect();
                        ui.label(egui::RichText::new(crate::i18n::t(title)).color(t.text_dim).small());
                        ui.end_row();
                        for r in group {
                            ui.label(&r.name);
                            let id = egui::Id::new(("par_expr", &r.name));
                            if let Some(v) = expr_field(ui, id, &r.expr) {
                                action = Some(json!({ "name": r.name, "expr": v }));
                            }
                            match (&r.value, &r.error) {
                                (Some(v), _) => ui.label(trim_num(*v, 4)),
                                (None, Some(e)) => {
                                    ui.label(egui::RichText::new(crate::tl!("error")).color(Color32::from_rgb(0xff, 0x8a, 0x6a))).on_hover_text(e)
                                }
                                _ => ui.label(""),
                            };
                            ui.label(
                                egui::RichText::new(crate::i18n::t(if kind == "user" { "User" } else { r.description.as_str() })).color(t.text_dim),
                            );
                            if kind == "user" {
                                if ui.small_button(crate::tl!("Delete")).clicked() {
                                    action = Some(json!({ "delete": r.name }));
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    }
                });
            });
        });
    ctx.data_mut(|d| d.insert_temp(filter_id, filter));
    if let Some(p) = action {
        let _ = app.run("parameters", p);
    }
}

/// A text field committing on Enter / focus loss when changed.
pub fn expr_field(ui: &mut egui::Ui, id: egui::Id, value: &str) -> Option<String> {
    let mut buf = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    let resp = ui.add(egui::TextEdit::singleline(&mut buf).id(id.with("te")).desired_width(150.0));
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    (resp.lost_focus() && buf.trim() != value && !buf.trim().is_empty()).then(|| buf.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_intersection() {
        let p = intersect((Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)), (Vec2::new(5.0, -1.0), Vec2::new(5.0, 3.0)));
        assert!(p.is_some_and(|p| (p.x - 5.0).abs() < 1e-9 && p.y.abs() < 1e-9));
        assert!(intersect((Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)), (Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0))).is_none());
    }

    #[test]
    fn number_trimming() {
        assert_eq!(trim_num(12.5, 4), "12.5");
        assert_eq!(trim_num(45.0, 2), "45");
    }
}
