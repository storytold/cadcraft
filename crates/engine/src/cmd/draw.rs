//! Draw commands: LINE, PLINE, CIRCLE, ARC, RECTANG, POLYGON, ELLIPSE, POINT, XLINE, RAY,
//! SPLINE, DONUT, TEXT, MTEXT. Each has a JSON form and an interactive prompt sequence.

use cadcraft_doc::{EntityKind, HAlign, MText, Point, RayLine, Text, VAlign};
use cadcraft_geom::{Arc, Circle, Ellipse, PolyVertex, Polyline, Spline, Vec2, Vec3, arc_to_bulge, bulge_to_arc};
use serde_json::{Value, json};

use super::helpers::*;
use super::machines::number;
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step, snap};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("line", "Line", run_line)
            .menu(&["Draw", "Line"])
            .alias(&["l"])
            .params("{points: [[x,y],...], closed?: bool}")
            .interactive(|_| Ok(Box::new(LineM::default()))),
        CommandSpec::new("pline", "Polyline", run_pline)
            .menu(&["Draw", "Polyline"])
            .alias(&["pl"])
            .params("{vertices: [[x,y] | {p:[x,y], bulge, startWidth, endWidth}], closed?, width?}")
            .interactive(|_| Ok(Box::new(PlineM::default()))),
        CommandSpec::new("circle", "Circle", run_circle)
            .menu(&["Draw", "Circle", "Center, Radius"])
            .alias(&["c"])
            .params("{center, radius} | {center, diameter} | {p1, p2} | {p1, p2, p3}")
            .interactive(|_| Ok(Box::new(CircleM::default()))),
        CommandSpec::new("arc", "Arc", run_arc)
            .menu(&["Draw", "Arc", "3 Points"])
            .alias(&["a"])
            .params("{p1, p2, p3} | {center, radius, start, end (degrees)} | {start, center, end}")
            .interactive(|_| Ok(super::draw2::arc_machine())),
        CommandSpec::new("rectang", "Rectangle", super::draw2::run_rectang2)
            .menu(&["Draw", "Rectangle"])
            .alias(&["rec", "rectangle"])
            .params("{p1, p2 | dimensions: [l, w] (p2 picks the quadrant) | area + length|breadth, rotation? (deg), fillet?, chamfer?: [d1, d2], width?, elevation?, thickness?}")
            .interactive(|_| Ok(Box::new(super::draw2::RectM2::default()))),
        CommandSpec::new("polygon", "Polygon", run_polygon)
            .menu(&["Draw", "Polygon"])
            .alias(&["pol"])
            .params("{sides, center, radius, inscribed?: bool, angle?} | {sides, edge: [[x,y],[x,y]]}")
            .interactive(|_| Ok(Box::new(PolygonM::default()))),
        CommandSpec::new("ellipse", "Ellipse", run_ellipse)
            .menu(&["Draw", "Ellipse", "Center"])
            .alias(&["el"])
            .params("{center, major: [dx,dy], ratio, start?, end? (degrees)}")
            .interactive(|_| Ok(Box::new(EllipseM::default()))),
        CommandSpec::new("point", "Point", run_point)
            .menu(&["Draw", "Point", "Single Point"])
            .alias(&["po"])
            .params("{at: [x,y]} | {points: [...]}")
            .interactive(|_| Ok(Box::new(PointM { multiple: false }))),
        CommandSpec::new("point.multiple", "Multiple Point", run_point)
            .menu(&["Draw", "Point", "Multiple Point"])
            .params("{points: [...]}")
            .interactive(|_| Ok(Box::new(PointM { multiple: true }))),
        CommandSpec::new("xline", "Construction Line", super::draw2::run_xline2)
            .menu(&["Draw", "Construction Line"])
            .alias(&["xl"])
            .params("{base, through} | {base, angle (degrees)} | {base, hor|ver: true} | {vertex, start, end} (bisect) | {handle, distance, side} | {handle, through} (offset)")
            .interactive(|_| Ok(Box::new(super::draw2::XlineM2::default()))),
        CommandSpec::new("ray", "Ray", run_ray)
            .menu(&["Draw", "Ray"])
            .params("{base, through}")
            .interactive(|_| Ok(Box::new(XlineM { ray: true, base: None, fixed_angle: None }))),
        CommandSpec::new("spline", "Spline", run_spline)
            .menu(&["Draw", "Spline", "Fit Points"])
            .alias(&["spl"])
            .params("{fit: [[x,y],...]} | {control: [...], degree?}")
            .interactive(|_| Ok(Box::new(SplineM::default()))),
        CommandSpec::new("donut", "Donut", run_donut)
            .menu(&["Draw", "Donut"])
            .alias(&["do", "doughnut"])
            .params("{center, inside, outside}")
            .interactive(|_| Ok(Box::new(DonutM::default()))),
        CommandSpec::new("text", "Single Line Text", run_text)
            .menu(&["Draw", "Text", "Single Line Text"])
            .alias(&["dt", "dtext"])
            .params("{at, text, height?, rotation? (degrees), justify?: L|C|R|M|TL|TC|TR|ML|MC|MR|BL|BC|BR}")
            .interactive(|s| Ok(Box::new(TextM::new(s)))),
        CommandSpec::new("mtext", "Multiline Text", run_mtext)
            .menu(&["Draw", "Text", "Multiline Text..."])
            .alias(&["t", "mt"])
            .params("{at, text, height?, width?, attach?: 1..9, rotation?}")
            .interactive(|_| Ok(Box::new(MTextM::default()))),
    ]
}

fn added(h: cadcraft_doc::Handle) -> Result<Value> {
    Ok(json!({ "handle": h.hex() }))
}

// ---------------- JSON forms ----------------

fn run_line(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = points_param(p, "points").ok_or_else(|| bad("line", "`points` [[x,y],...] (2 or more) is required"))?;
    if pts.len() < 2 {
        return Err(bad("line", "need at least 2 points"));
    }
    let mut hs = Vec::new();
    for w in pts.windows(2) {
        if let [a, b] = w {
            hs.push(s.add_entity(line(*a, *b))?.hex());
        }
    }
    if bool_or(p, "closed", false)
        && pts.len() > 2
        && let (Some(a), Some(b)) = (pts.last(), pts.first())
    {
        hs.push(s.add_entity(line(*a, *b))?.hex());
    }
    Ok(json!({ "handles": hs }))
}

fn vertex_value(v: &Value) -> Option<PolyVertex> {
    if let Some(p) = point_value(v) {
        return Some(PolyVertex::new(p));
    }
    let p = point_value(v.get("p")?)?;
    Some(PolyVertex { p, bulge: f64_or(v, "bulge", 0.0), start_width: f64_or(v, "startWidth", 0.0), end_width: f64_or(v, "endWidth", 0.0) })
}

fn run_pline(s: &mut Session, p: &Value) -> Result<Value> {
    let vs: Vec<PolyVertex> = p
        .get("vertices")
        .or_else(|| p.get("points"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(vertex_value).collect())
        .unwrap_or_default();
    if vs.len() < 2 {
        return Err(bad("pline", "`vertices` needs at least 2 points"));
    }
    let mut k = lwpoly(vs, bool_or(p, "closed", false));
    if let EntityKind::LwPolyline(pl) = &mut k {
        pl.const_width = f64_or(p, "width", 0.0).max(0.0);
    }
    added(s.add_entity(k)?)
}

fn run_circle(s: &mut Session, p: &Value) -> Result<Value> {
    let c = if let Some(center) = point_param(p, "center") {
        let r = p
            .get("radius")
            .and_then(Value::as_f64)
            .or_else(|| p.get("diameter").and_then(Value::as_f64).map(|d| d / 2.0))
            .ok_or_else(|| bad("circle", "`radius` or `diameter` is required"))?;
        Circle::new(center, r)
    } else if let (Some(a), Some(b)) = (point_param(p, "p1"), point_param(p, "p2")) {
        match point_param(p, "p3") {
            Some(c3) => Circle::from_3_points(a, b, c3).ok_or_else(|| bad("circle", "points are collinear"))?,
            None => Circle::from_2_points(a, b),
        }
    } else {
        return Err(bad("circle", "give {center, radius} or {p1, p2[, p3]}"));
    };
    if c.radius <= 0.0 || !c.radius.is_finite() {
        return Err(bad("circle", "radius must be positive"));
    }
    added(s.add_entity(circle(c.center, c.radius))?)
}

fn run_arc(s: &mut Session, p: &Value) -> Result<Value> {
    let a = if let (Some(a), Some(b), Some(c)) = (point_param(p, "p1"), point_param(p, "p2"), point_param(p, "p3")) {
        Arc::from_3_points(a, b, c).ok_or_else(|| bad("arc", "points are collinear"))?
    } else if let (Some(st), Some(c), Some(e)) = (point_param(p, "start"), point_param(p, "center"), point_param(p, "end")) {
        Arc::from_start_center_end(st, c, e)
    } else if let Some(c) = point_param(p, "center") {
        let r = f64_req("arc", p, "radius")?;
        Arc::new(c, r, f64_req("arc", p, "start")?.to_radians(), f64_req("arc", p, "end")?.to_radians())
    } else {
        return Err(bad("arc", "give {p1,p2,p3}, {start,center,end} or {center,radius,start,end}"));
    };
    // Zero or non-finite radius (e.g. start == center): refuse, like the interactive ARC and the
    // arc.* variants, before anything is added (#85).
    let a = super::curves::finite_arc(a).ok_or_else(|| bad("arc", "these values do not define an arc"))?;
    added(s.add_entity(arc(&a))?)
}

fn run_polygon(s: &mut Session, p: &Value) -> Result<Value> {
    let n = p.get("sides").and_then(Value::as_u64).unwrap_or(4) as usize;
    if !(3..=1024).contains(&n) {
        return Err(bad("polygon", "sides must be 3..1024"));
    }
    let vs = if let Some(e) = points_param(p, "edge") {
        let (Some(a), Some(b)) = (e.first(), e.get(1)) else { return Err(bad("polygon", "edge needs 2 points")) };
        polygon_from_edge(*a, *b, n)
    } else {
        let c = point_req("polygon", p, "center")?;
        let r = f64_req("polygon", p, "radius")?;
        let ang = f64_or(p, "angle", 90.0).to_radians();
        let inscribed = bool_or(p, "inscribed", true);
        polygon_vertices(
            c,
            n,
            Vec2::polar(c, r, if inscribed { ang } else { ang - std::f64::consts::PI / n as f64 + std::f64::consts::PI / n as f64 }),
            inscribed,
        )
    };
    added(s.add_entity(lwpoly(vs, true))?)
}

fn run_ellipse(s: &mut Session, p: &Value) -> Result<Value> {
    let c = point_req("ellipse", p, "center")?;
    let m = point_req("ellipse", p, "major")?;
    let ratio = f64_req("ellipse", p, "ratio")?;
    if !(ratio > 0.0 && ratio <= 1.0) || m.len() < 1e-12 {
        return Err(bad("ellipse", "ratio must be in (0, 1] and major non-zero"));
    }
    let start = f64_or(p, "start", 0.0).to_radians();
    let end = f64_or(p, "end", 360.0).to_radians();
    added(s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse { center: v3(c), major: v3(m), ratio, start, end }))?)
}

fn run_point(s: &mut Session, p: &Value) -> Result<Value> {
    let pts = match point_param(p, "at") {
        Some(a) => vec![a],
        None => points_param(p, "points").ok_or_else(|| bad("point", "`at` or `points` is required"))?,
    };
    let mut hs = Vec::new();
    for q in pts {
        hs.push(s.add_entity(EntityKind::Point(Point { p: v3(q), angle: 0.0 }))?.hex());
    }
    Ok(json!({ "handles": hs }))
}

fn run_ray(s: &mut Session, p: &Value) -> Result<Value> {
    let b = point_req("ray", p, "base")?;
    let t = point_req("ray", p, "through")?;
    let dir = (t - b).normalized();
    if dir == Vec2::ZERO {
        return Err(bad("ray", "through point equals base"));
    }
    added(s.add_entity(EntityKind::Ray(RayLine { base: v3(b), dir: v3(dir) }))?)
}

fn run_spline(s: &mut Session, p: &Value) -> Result<Value> {
    let sp = if let Some(f) = points_param(p, "fit") {
        if f.len() < 2 {
            return Err(bad("spline", "need 2+ fit points"));
        }
        Spline::from_fit_points(&f)
    } else if let Some(c) = points_param(p, "control") {
        if c.len() < 2 {
            return Err(bad("spline", "need 2+ control points"));
        }
        Spline::from_control(c, p.get("degree").and_then(Value::as_u64).unwrap_or(3) as usize)
    } else {
        return Err(bad("spline", "`fit` or `control` points are required"));
    };
    added(s.add_entity(EntityKind::Spline(sp))?)
}

fn donut(center: Vec2, inside: f64, outside: f64) -> EntityKind {
    let r = (inside.abs() + outside.abs()) / 4.0;
    let w = (outside.abs() - inside.abs()).abs() / 2.0;
    let mut k = lwpoly(vec![PolyVertex::with_bulge(center - Vec2::X * r, 1.0), PolyVertex::with_bulge(center + Vec2::X * r, 1.0)], true);
    if let EntityKind::LwPolyline(pl) = &mut k {
        pl.const_width = w;
    }
    k
}

fn run_donut(s: &mut Session, p: &Value) -> Result<Value> {
    let c = point_req("donut", p, "center")?;
    added(s.add_entity(donut(c, f64_or(p, "inside", 0.5), f64_or(p, "outside", 1.0)))?)
}

/// Justification code → (halign, valign).
pub(crate) fn justify(j: &str) -> Option<(HAlign, VAlign)> {
    Some(match j.to_ascii_uppercase().as_str() {
        "L" | "LEFT" => (HAlign::Left, VAlign::Baseline),
        "C" | "CENTER" => (HAlign::Center, VAlign::Baseline),
        "R" | "RIGHT" => (HAlign::Right, VAlign::Baseline),
        "A" | "ALIGN" => (HAlign::Aligned, VAlign::Baseline),
        "M" | "MIDDLE" => (HAlign::Middle, VAlign::Baseline),
        "F" | "FIT" => (HAlign::Fit, VAlign::Baseline),
        "TL" => (HAlign::Left, VAlign::Top),
        "TC" => (HAlign::Center, VAlign::Top),
        "TR" => (HAlign::Right, VAlign::Top),
        "ML" => (HAlign::Left, VAlign::Middle),
        "MC" => (HAlign::Center, VAlign::Middle),
        "MR" => (HAlign::Right, VAlign::Middle),
        "BL" => (HAlign::Left, VAlign::Bottom),
        "BC" => (HAlign::Center, VAlign::Bottom),
        "BR" => (HAlign::Right, VAlign::Bottom),
        _ => return None,
    })
}

pub(crate) fn make_text(s: &Session, at: Vec2, value: &str, height: f64, rotation: f64, j: (HAlign, VAlign)) -> EntityKind {
    let style = s.doc().map(|d| d.header.str("TEXTSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into());
    let (h, v) = j;
    let left = h == HAlign::Left && v == VAlign::Baseline;
    EntityKind::Text(Text {
        insert: v3(at),
        align_pt: if left { None } else { Some(v3(at)) },
        height,
        value: value.into(),
        rotation,
        width_factor: 1.0,
        oblique: 0.0,
        style,
        halign: h,
        valign: v,
    })
}

fn run_text(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("text", p, "at")?;
    let text = str_param(p, "text").ok_or_else(|| bad("text", "`text` is required"))?;
    let height = f64_or(p, "height", s.doc()?.header.f64("TEXTSIZE", 0.2));
    if height <= 0.0 {
        return Err(bad("text", "height must be positive"));
    }
    let j = justify(str_param(p, "justify").unwrap_or("L")).ok_or_else(|| bad("text", "unknown justification"))?;
    let k = make_text(s, at, text, height, f64_or(p, "rotation", 0.0).to_radians(), j);
    added(s.add_entity(k)?)
}

fn run_mtext(s: &mut Session, p: &Value) -> Result<Value> {
    let at = point_req("mtext", p, "at")?;
    let text = str_param(p, "text").ok_or_else(|| bad("mtext", "`text` is required"))?;
    let height = f64_or(p, "height", s.doc()?.header.f64("TEXTSIZE", 0.2));
    if height <= 0.0 {
        return Err(bad("mtext", "height must be positive"));
    }
    let style = s.doc()?.header.str("TEXTSTYLE", "Standard");
    let k = EntityKind::MText(MText {
        insert: v3(at),
        height,
        width: f64_or(p, "width", 0.0).max(0.0),
        attach: p.get("attach").and_then(Value::as_u64).unwrap_or(1).clamp(1, 9) as u8,
        rotation: f64_or(p, "rotation", 0.0).to_radians(),
        style,
        contents: text.replace('\n', "\\P"),
        line_spacing: f64_or(p, "lineSpacing", 1.0),
    });
    added(s.add_entity(k)?)
}

// ---------------- interactive ----------------

#[derive(Default)]
struct LineM {
    pts: Vec<Vec2>,
    handles: Vec<cadcraft_doc::Handle>,
    /// A first point picked as a deferred tangent/perpendicular, resolved by the next point.
    first: Option<snap::Deferred>,
}

impl LineM {
    /// Draw the first segment from the deferred first point to `end`.
    fn resolve_first(&mut self, s: &mut Session, first: snap::Deferred, end: snap::LineEnd) -> Result<Step> {
        let Some((a, b)) = snap::resolve_deferred(snap::LineEnd::Deferred(first), end) else {
            let what = if first.mode == snap::mode::TAN { "tangent" } else { "perpendicular" };
            return Err(crate::EngineError::Other(format!("No {what} line through that point; pick another point.")));
        };
        self.handles.push(s.add_entity(line(a, b))?);
        self.pts = vec![a, b];
        self.first = None;
        s.last_point = b;
        Ok(Step::Continue)
    }
}

impl Interactive for LineM {
    fn name(&self) -> &'static str {
        "LINE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.pts.len() {
            0 if self.first.is_some() => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).deferred(),
            0 => Prompt::new("Specify first point", Accept::POINT).deferred(),
            1 | 2 => Prompt::new("Specify next point", Accept::POINT).kw(&["Undo"]).base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify next point", Accept::POINT).kw(&["Close", "Undo"]).base_opt(self.pts.last().copied()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.pts.is_empty()
            && let Some(first) = self.first
        {
            match i {
                Input::Point(p) => return self.resolve_first(s, first, snap::LineEnd::Point(p)),
                Input::Deferred(d) => return self.resolve_first(s, first, snap::LineEnd::Deferred(d)),
                _ => {}
            }
        }
        match i {
            Input::Deferred(d) if self.pts.is_empty() => {
                self.first = Some(d);
                Ok(Step::Continue)
            }
            // A deferred pick with a known last point (not offered by the prompt, but harmless).
            Input::Deferred(d) => match self.pts.last().copied() {
                Some(last) => match snap::resolve_deferred(snap::LineEnd::Point(last), snap::LineEnd::Deferred(d)) {
                    Some((_, p)) => self.input(s, Input::Point(p)),
                    None => Err(crate::EngineError::Other("No tangent or perpendicular from the last point.".into())),
                },
                None => Ok(Step::Continue),
            },
            Input::Point(p) => {
                if let Some(last) = self.pts.last().copied() {
                    if last.near(p, 1e-12) {
                        return Ok(Step::Continue);
                    }
                    self.handles.push(s.add_entity(line(last, p))?);
                }
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                if self.first.take().is_some() {
                    return Ok(Step::Continue);
                }
                if let Some(h) = self.handles.pop() {
                    s.doc_mut()?.remove_entity(h);
                }
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Close" => {
                if let (Some(a), Some(b)) = (self.pts.last().copied(), self.pts.first().copied()) {
                    s.add_entity(line(a, b))?;
                }
                Ok(Step::Done)
            }
            Input::Enter => Ok(Step::Done),
            _ => Err(crate::EngineError::Other("Point or option keyword required.".into())),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(first) = self.first {
            // Over a second curve, show the tangent-to-tangent line the click would draw.
            let end = s.cursor_deferred.map_or(snap::LineEnd::Point(c), snap::LineEnd::Deferred);
            return snap::resolve_deferred(snap::LineEnd::Deferred(first), end).map(|(a, b)| vec![line(a, b)]).unwrap_or_default();
        }
        self.pts.last().map(|l| vec![line(*l, c)]).unwrap_or_default()
    }
}

#[derive(Default)]
struct PlineM {
    verts: Vec<PolyVertex>,
    arc_mode: bool,
    /// Total width at the start and end of the next segment (Width / Halfwidth answers).
    start_w: f64,
    end_w: f64,
    handle: Option<cadcraft_doc::Handle>,
    /// 1 = asking the starting width, 2 = the ending width.
    asking_width: u8,
    /// The widths are being asked as half-widths (centre line to edge).
    half: bool,
}

impl PlineM {
    fn entity(&self, closed: bool) -> EntityKind {
        let mut k = lwpoly(self.verts.clone(), closed);
        if let EntityKind::LwPolyline(pl) = &mut k {
            // Every segment the same uniform width: store it as the constant width, as before.
            let segs = if closed { pl.vertices.len() } else { pl.vertices.len().saturating_sub(1) };
            let w = pl.vertices.first().map_or(0.0, |v| v.start_width);
            if pl.vertices.iter().take(segs).all(|v| v.start_width == w && v.end_width == w) {
                pl.const_width = w;
                for v in &mut pl.vertices {
                    (v.start_width, v.end_width) = (0.0, 0.0);
                }
            }
        }
        k
    }
    /// The next segment, starting at the last vertex, takes the requested widths (#104); the
    /// segments after it are uniform at the ending width, as in AutoCAD.
    fn begin_segment(&mut self) {
        if let Some(last) = self.verts.last_mut() {
            (last.start_width, last.end_width) = (self.start_w, self.end_w);
            self.start_w = self.end_w;
        }
    }
    fn sync(&mut self, s: &mut Session, closed: bool) -> Result<()> {
        if self.verts.len() < 2 {
            if let Some(h) = self.handle.take() {
                s.doc_mut()?.remove_entity(h);
            }
            return Ok(());
        }
        let k = self.entity(closed);
        match self.handle {
            Some(h) => {
                s.doc_mut()?.modify_entity(h, |e| e.kind = k)?;
            }
            None => self.handle = Some(s.add_entity(k)?),
        }
        Ok(())
    }
    /// Bulge for a tangent-continuing arc from the last vertex to `p`.
    fn tangent_bulge(&self, p: Vec2) -> f64 {
        let n = self.verts.len();
        let Some(last) = self.verts.last() else { return 0.0 };
        let dir = if n >= 2 {
            let prev = self.verts.get(n - 2).copied().unwrap_or_default();
            match bulge_to_arc(prev.p, last.p, prev.bulge) {
                Some((a, ccw)) => {
                    let r = (last.p - a.center).perp().normalized();
                    if ccw { r } else { -r }
                }
                None => (last.p - prev.p).normalized(),
            }
        } else {
            Vec2::X
        };
        let chord = p - last.p;
        // Included angle = 2 × angle between tangent and chord.
        let ang = dir.cross(chord).atan2(dir.dot(chord));
        arc_to_bulge(2.0 * ang)
    }
}

impl Interactive for PlineM {
    fn name(&self) -> &'static str {
        "PLINE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if self.asking_width > 0 {
            let unit = if self.half { "half-width" } else { "width" };
            let (which, w) = if self.asking_width == 1 { ("starting", self.start_w) } else { ("ending", self.end_w) };
            let shown = if self.half { w / 2.0 } else { w };
            return Prompt::new(format!("Specify {which} {unit}"), Accept::NUMBER).default(format!("{shown:.4}"));
        }
        let base = self.verts.last().map(|v| v.p);
        match (self.verts.len(), self.arc_mode) {
            (0, _) => Prompt::new("Specify start point", Accept::POINT),
            (n, false) => {
                let mut kw = vec!["Arc", "Halfwidth", "Length", "Undo", "Width"];
                if n >= 2 {
                    kw.insert(1, "Close");
                }
                Prompt::new("Specify next point", Accept::POINT).kw(&kw).base_opt(base)
            }
            (_, true) => Prompt::new("Specify endpoint of arc", Accept::POINT)
                .kw(&["Angle", "CEnter", "CLose", "Direction", "Halfwidth", "Line", "Radius", "Second pt", "Undo", "Width"])
                .base_opt(base),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_width > 0 {
            match i {
                Input::Text(t) => {
                    let w = number(&t).filter(|w| w.is_finite()).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
                    // Halfwidth is centre line to edge: the total width is twice the answer (#83).
                    let w = w.max(0.0) * if self.half { 2.0 } else { 1.0 };
                    if self.asking_width == 1 {
                        self.start_w = w;
                    } else {
                        self.end_w = w;
                    }
                }
                Input::Enter => {}
                _ => return Ok(Step::Continue),
            }
            if self.asking_width == 1 {
                // The ending width defaults to the starting width.
                self.end_w = self.start_w;
            }
            self.asking_width = if self.asking_width == 1 { 2 } else { 0 };
            return Ok(Step::Continue);
        }
        match i {
            Input::Point(p) => {
                if self.verts.is_empty() {
                    s.echo(format!("Current line-width is {:.4}", self.start_w));
                }
                self.begin_segment();
                if self.arc_mode && !self.verts.is_empty() {
                    let b = self.tangent_bulge(p);
                    if let Some(last) = self.verts.last_mut() {
                        last.bulge = b;
                    }
                }
                self.verts.push(PolyVertex::new(p));
                self.sync(s, false)?;
                Ok(Step::Continue)
            }
            Input::Keyword(k) => match k.as_str() {
                "Arc" => {
                    self.arc_mode = true;
                    Ok(Step::Continue)
                }
                "Line" => {
                    self.arc_mode = false;
                    Ok(Step::Continue)
                }
                "Close" | "CLose" => {
                    if self.arc_mode
                        && let Some(first) = self.verts.first().map(|v| v.p)
                    {
                        let b = self.tangent_bulge(first);
                        if let Some(last) = self.verts.last_mut() {
                            last.bulge = b;
                        }
                    }
                    // The closing segment starts at the last vertex.
                    self.begin_segment();
                    self.sync(s, true)?;
                    Ok(Step::Done)
                }
                "Undo" => {
                    self.verts.pop();
                    if let Some(l) = self.verts.last_mut() {
                        l.bulge = 0.0;
                        (l.start_width, l.end_width) = (0.0, 0.0);
                    }
                    self.sync(s, false)?;
                    Ok(Step::Continue)
                }
                "Width" | "Halfwidth" => {
                    self.half = k == "Halfwidth";
                    self.asking_width = 1;
                    Ok(Step::Continue)
                }
                _ => {
                    s.echo(format!("{k}: not available yet"));
                    Ok(Step::Continue)
                }
            },
            Input::Enter => Ok(Step::Done),
            _ => Err(crate::EngineError::Other("Point or option keyword required.".into())),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let Some(last) = self.verts.last() else { return Vec::new() };
        if self.arc_mode {
            let b = self.tangent_bulge(c);
            vec![lwpoly(vec![PolyVertex::with_bulge(last.p, b), PolyVertex::new(c)], false)]
        } else {
            vec![line(last.p, c)]
        }
    }
}

#[derive(Default)]
struct CircleM {
    mode: u8, // 0 center-radius, 1 = 3P, 2 = 2P, 3 = TTR
    pts: Vec<Vec2>,
    diameter: bool,
    /// The `Ttr` option hands over to the tangent-circle machine.
    delegate: Option<Box<dyn Interactive>>,
}

impl Interactive for CircleM {
    fn name(&self) -> &'static str {
        "CIRCLE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        if let Some(d) = &self.delegate {
            return d.prompt(s);
        }
        let n = self.pts.len();
        let last_r = s.doc().map(|d| d.header.f64("CIRCLERAD", 0.0)).unwrap_or(0.0);
        match (self.mode, n) {
            (0, 0) => Prompt::new("Specify center point for circle", Accept::POINT).kw(&["3P", "2P", "Ttr (tan tan radius)"]),
            (0, _) if self.diameter => Prompt::new("Specify diameter of circle", Accept::POINT_OR_NUMBER).base_opt(self.pts.first().copied()),
            (0, _) => {
                let p = Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).kw(&["Diameter"]).base_opt(self.pts.first().copied());
                if last_r > 0.0 { p.default(format!("{last_r:.4}")) } else { p }
            }
            (1, k) => Prompt::new(
                ["Specify first point on circle", "Specify second point on circle", "Specify third point on circle"].get(k).copied().unwrap_or(""),
                Accept::POINT,
            )
            .base_opt(self.pts.last().copied()),
            (2, k) => Prompt::new(
                if k == 0 { "Specify first end point of circle's diameter" } else { "Specify second end point of circle's diameter" },
                Accept::POINT,
            )
            .base_opt(self.pts.last().copied()),
            _ => Prompt::new("Specify center point for circle", Accept::POINT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(d) = &mut self.delegate {
            return d.input(s, i);
        }
        match i {
            Input::Keyword(k) => {
                self.mode = match k.as_str() {
                    "3P" => 1,
                    "2P" => 2,
                    "Diameter" => {
                        self.diameter = true;
                        self.mode
                    }
                    _ => {
                        self.delegate = Some(Box::new(super::draw2::TanCircleM::new(true)));
                        3
                    }
                };
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                self.pts.push(p);
                let done = match self.mode {
                    0 if self.pts.len() == 2 => {
                        let (Some(c), Some(e)) = (self.pts.first(), self.pts.get(1)) else { return Ok(Step::Continue) };
                        let r = if self.diameter { c.dist(*e) / 2.0 } else { c.dist(*e) };
                        Some(Circle::new(*c, r))
                    }
                    1 if self.pts.len() == 3 => Circle::from_3_points(self.pts[0], self.pts[1], self.pts[2]),
                    2 if self.pts.len() == 2 => Some(Circle::from_2_points(self.pts[0], self.pts[1])),
                    _ => None,
                };
                if let Some(c) = done {
                    finish_circle(s, c)?;
                    return Ok(Step::Done);
                }
                Ok(Step::Continue)
            }
            Input::Text(t) if self.mode == 0 && self.pts.len() == 1 => {
                let v = number(&t).ok_or_else(|| crate::EngineError::Other("Requires numeric distance or second point.".into()))?;
                let r = if self.diameter { v / 2.0 } else { v };
                if r <= 0.0 {
                    return Err(crate::EngineError::Other("Value must be positive and nonzero.".into()));
                }
                finish_circle(s, Circle::new(self.pts[0], r))?;
                Ok(Step::Done)
            }
            Input::Enter if self.mode == 0 && self.pts.len() == 1 => {
                let r = s.doc()?.header.f64("CIRCLERAD", 0.0);
                if r > 0.0 {
                    finish_circle(s, Circle::new(self.pts[0], r))?;
                    Ok(Step::Done)
                } else {
                    Ok(Step::Continue)
                }
            }
            Input::Enter => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(d) = &self.delegate {
            return d.preview(s, c);
        }
        match (self.mode, self.pts.as_slice()) {
            (0, [center]) => {
                let r = if self.diameter { center.dist(c) / 2.0 } else { center.dist(c) };
                vec![circle(*center, r), line(*center, c)]
            }
            (1, [a, b]) => Circle::from_3_points(*a, *b, c).map(|ci| vec![circle(ci.center, ci.radius)]).unwrap_or_default(),
            (1, [a]) | (2, [a]) => {
                let ci = Circle::from_2_points(*a, c);
                vec![circle(ci.center, ci.radius)]
            }
            _ => Vec::new(),
        }
    }
}

fn finish_circle(s: &mut Session, c: Circle) -> Result<()> {
    s.add_entity(circle(c.center, c.radius))?;
    s.doc_mut()?.header.set_f64("CIRCLERAD", c.radius);
    Ok(())
}

#[derive(Default)]
struct PolygonM {
    sides: Option<usize>,
    center: Option<Vec2>,
    edge: bool,
    edge_pts: Vec<Vec2>,
    inscribed: Option<bool>,
}

impl Interactive for PolygonM {
    fn name(&self) -> &'static str {
        "POLYGON"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let def = s.doc().map(|d| d.header.i64("POLYSIDES", 4)).unwrap_or(4);
        match (self.sides, self.edge, self.center, self.inscribed) {
            (None, _, _, _) => Prompt::new("Enter number of sides", Accept::NUMBER).default(def.to_string()),
            (Some(_), true, _, _) => Prompt::new(
                if self.edge_pts.is_empty() { "Specify first endpoint of edge" } else { "Specify second endpoint of edge" },
                Accept::POINT,
            )
            .base_opt(self.edge_pts.first().copied()),
            (Some(_), false, None, _) => Prompt::new("Specify center of polygon", Accept::POINT).kw(&["Edge"]),
            (Some(_), false, Some(_), None) => Prompt::new("Enter an option", Accept::TEXT)
                .kw(&["Inscribed in circle", "Circumscribed about circle"])
                .default(if s.last_used.polygon_circumscribed { "C" } else { "I" }),
            (Some(_), false, Some(c), Some(_)) => Prompt::new("Specify radius of circle", Accept::POINT_OR_NUMBER).base(c),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.sides.is_none() {
            let n = match &i {
                Input::Text(t) => t
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (3..=1024).contains(n))
                    .ok_or_else(|| crate::EngineError::Other("Requires an integer between 3 and 1024.".into()))?,
                Input::Enter => s.doc()?.header.i64("POLYSIDES", 4).clamp(3, 1024) as usize,
                _ => return Ok(Step::Continue),
            };
            self.sides = Some(n);
            s.doc_mut()?.header.set_i64("POLYSIDES", n as i64);
            return Ok(Step::Continue);
        }
        let n = self.sides.unwrap_or(4);
        if self.edge {
            if let Input::Point(p) = i {
                self.edge_pts.push(p);
                if self.edge_pts.len() == 2 {
                    s.add_entity(lwpoly(polygon_from_edge(self.edge_pts[0], self.edge_pts[1], n), true))?;
                    return Ok(Step::Done);
                }
            }
            return Ok(Step::Continue);
        }
        match (self.center, self.inscribed, i) {
            (None, _, Input::Keyword(k)) if k == "Edge" => self.edge = true,
            (None, _, Input::Point(p)) => self.center = Some(p),
            (Some(_), None, inp @ (Input::Keyword(_) | Input::Enter | Input::Text(_))) => {
                // Enter takes the option used last time (the prompt's default).
                let ins = match inp {
                    Input::Keyword(k) => k.starts_with('I'),
                    Input::Text(t) => !t.trim().to_ascii_lowercase().starts_with('c'),
                    _ => !s.last_used.polygon_circumscribed,
                };
                self.inscribed = Some(ins);
                s.last_used.polygon_circumscribed = !ins;
            }
            (Some(c), Some(ins), Input::Point(p)) => {
                s.add_entity(lwpoly(polygon_vertices(c, n, p, ins), true))?;
                return Ok(Step::Done);
            }
            (Some(c), Some(ins), Input::Text(t)) => {
                let r = number(&t).ok_or_else(|| crate::EngineError::Other("Requires numeric distance or point.".into()))?;
                let rp = if ins { c + Vec2::Y * r } else { c - Vec2::Y * r };
                s.add_entity(lwpoly(polygon_vertices(c, n, rp, ins), true))?;
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let n = self.sides.unwrap_or(4);
        if self.edge {
            return self.edge_pts.first().map(|a| vec![lwpoly(polygon_from_edge(*a, c, n), true)]).unwrap_or_default();
        }
        match (self.center, self.inscribed) {
            (Some(ctr), Some(ins)) => vec![lwpoly(polygon_vertices(ctr, n, c, ins), true)],
            _ => Vec::new(),
        }
    }
}

/// ELLIPSE prompt machine starting in axis-end or center mode, optionally for an elliptical arc.
pub(crate) fn ellipse_machine(center: bool, arc: bool) -> Box<dyn Interactive> {
    Box::new(EllipseM { center_mode: center, arc_mode: arc, ..Default::default() })
}

#[derive(Default)]
struct EllipseM {
    center_mode: bool,
    arc_mode: bool,
    pts: Vec<Vec2>,
    /// The `Rotation` option: the next value is the rotation around the major axis.
    rotation: bool,
    ellipse: Option<Ellipse>,
    /// The `Parameter` option: arc start/end values are ellipse parameters, not true angles.
    param: bool,
    start: Option<f64>,
}

impl EllipseM {
    fn axis(&self) -> Option<(Vec2, Vec2)> {
        // (center, major vector)
        match (self.center_mode, self.pts.as_slice()) {
            (true, [c, e, ..]) => Some((*c, *e - *c)),
            (false, [a, b, ..]) => Some((a.mid(*b), *b - a.mid(*b))),
            _ => None,
        }
    }
    fn build(&self, other: Vec2) -> Option<Ellipse> {
        let (c, m) = self.axis()?;
        let ml = m.len();
        if ml < 1e-12 {
            return None;
        }
        let u = m / ml;
        let minor = ((other - c).dot(u.perp())).abs();
        let (major, ratio) = if minor > ml { (u.perp() * minor, ml / minor) } else { (m, minor / ml) };
        (ratio > 1e-9).then_some(Ellipse::full(c, major, ratio.min(1.0)))
    }
    /// The ellipse seen as a circle on the first axis rotated by `a` around it: ratio = cos(a),
    /// for rotations up to 89.4°.
    fn rotated(&self, a: f64) -> Option<Ellipse> {
        let (c, m) = self.axis()?;
        let ratio = a.cos().abs();
        (m.len() >= 1e-12 && ratio >= 89.4f64.to_radians().cos()).then_some(Ellipse::full(c, m, ratio.min(1.0)))
    }
    /// The full ellipse is known: draw it, or go on to the arc angles.
    fn place(&mut self, s: &mut Session, e: Ellipse) -> Result<Step> {
        if self.arc_mode {
            self.ellipse = Some(e);
            return Ok(Step::Continue);
        }
        s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse {
            center: v3(e.center),
            major: v3(e.major),
            ratio: e.ratio,
            start: 0.0,
            end: cadcraft_geom::TAU,
        }))?;
        Ok(Step::Done)
    }
}

impl Interactive for EllipseM {
    fn name(&self) -> &'static str {
        "ELLIPSE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if let Some(e) = &self.ellipse {
            let msg = match (self.start.is_none(), self.param) {
                (true, false) => "Specify start angle",
                (false, false) => "Specify end angle",
                (true, true) => "Specify start parameter",
                (false, true) => "Specify end parameter",
            };
            return Prompt::new(msg, Accept::POINT_OR_NUMBER).kw(if self.param { &["Angle"] } else { &["Parameter"] }).base(e.center);
        }
        match (self.center_mode, self.pts.len()) {
            (false, 0) => Prompt::new("Specify axis endpoint of ellipse", Accept::POINT).kw(&["Arc", "Center"]),
            (false, 1) => Prompt::new("Specify other endpoint of axis", Accept::POINT).base_opt(self.pts.first().copied()),
            (true, 0) => Prompt::new("Specify center of ellipse", Accept::POINT),
            (true, 1) => Prompt::new("Specify endpoint of axis", Accept::POINT).base_opt(self.pts.first().copied()),
            _ if self.rotation => Prompt::new("Specify rotation around major axis", Accept::POINT_OR_NUMBER).base_opt(self.axis().map(|a| a.0)),
            _ => Prompt::new("Specify distance to other axis", Accept::POINT_OR_NUMBER).kw(&["Rotation"]).base_opt(self.axis().map(|a| a.0)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if let Some(e) = self.ellipse {
            let ge = Ellipse { start: 0.0, end: cadcraft_geom::TAU, ..e };
            let ang = match i {
                Input::Keyword(k) => {
                    match k.as_str() {
                        "Parameter" => self.param = true,
                        "Angle" => self.param = false,
                        _ => {}
                    }
                    return Ok(Step::Continue);
                }
                // A parameter is the angle from the major axis taken as the parameter itself.
                Input::Point(p) if self.param => cadcraft_geom::norm_angle((p - e.center).angle() - e.major.angle()),
                Input::Point(p) => ge.param_of(p),
                Input::Text(t) => {
                    let a = crate::units::parse_angle(&t).ok_or_else(|| crate::EngineError::Other("Requires an angle.".into()))?;
                    if self.param {
                        cadcraft_geom::norm_angle(a)
                    } else {
                        // A typed angle is a true angle from the major axis; convert it to an ellipse parameter like a picked point.
                        ge.param_of(e.center + Vec2::from_angle(a + e.major.angle()))
                    }
                }
                _ => return Ok(Step::Continue),
            };
            match self.start {
                None => {
                    self.start = Some(ang);
                    return Ok(Step::Continue);
                }
                Some(st) => {
                    s.add_entity(EntityKind::Ellipse(cadcraft_doc::Ellipse {
                        center: v3(e.center),
                        major: v3(e.major),
                        ratio: e.ratio,
                        start: st,
                        end: ang,
                    }))?;
                    return Ok(Step::Done);
                }
            }
        }
        match i {
            Input::Keyword(k) if k == "Center" => self.center_mode = true,
            Input::Keyword(k) if k == "Arc" => self.arc_mode = true,
            Input::Keyword(k) if k == "Rotation" && self.pts.len() == 2 => self.rotation = true,
            Input::Point(p) if self.pts.len() < 2 => self.pts.push(p),
            Input::Point(p) if self.rotation => {
                if let Some(e) = self.axis().and_then(|(c, _)| self.rotated((p - c).angle())) {
                    return self.place(s, e);
                }
            }
            Input::Point(p) => {
                if let Some(e) = self.build(p) {
                    return self.place(s, e);
                }
            }
            Input::Text(t) if self.pts.len() == 2 && self.rotation => {
                let e = crate::units::parse_angle(&t)
                    .and_then(|a| self.rotated(a))
                    .ok_or_else(|| crate::EngineError::Other("Requires an angle between 0 and 89.4 degrees.".into()))?;
                return self.place(s, e);
            }
            Input::Text(t) if self.pts.len() == 2 => {
                let d = number(&t).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?;
                if let Some((c, m)) = self.axis()
                    && let Some(e) = self.build(c + m.normalized().perp() * d)
                {
                    return self.place(s, e);
                }
            }
            Input::Enter => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.len() < 2 {
            return self.pts.first().map(|a| vec![line(*a, c)]).unwrap_or_default();
        }
        let e = if self.rotation { self.axis().and_then(|(ctr, _)| self.rotated((c - ctr).angle())) } else { self.build(c) };
        e.map(|e| {
            vec![EntityKind::Ellipse(cadcraft_doc::Ellipse {
                center: v3(e.center),
                major: v3(e.major),
                ratio: e.ratio,
                start: 0.0,
                end: cadcraft_geom::TAU,
            })]
        })
        .unwrap_or_default()
    }
}

struct PointM {
    multiple: bool,
}

impl Interactive for PointM {
    fn name(&self) -> &'static str {
        "POINT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let d = s.doc().ok();
        let _ = d;
        Prompt::new("Specify a point", Accept::POINT)
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let (m, sz) = s.doc().map(|d| (d.header.i64("PDMODE", 0), d.header.f64("PDSIZE", 0.0))).unwrap_or((0, 0.0));
        s.echo(format!("Current point modes:  PDMODE={m}  PDSIZE={sz:.4}"));
        Ok(Step::Continue)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                s.add_entity(EntityKind::Point(Point { p: v3(p), angle: 0.0 }))?;
                Ok(if self.multiple { Step::Continue } else { Step::Done })
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
}

struct XlineM {
    ray: bool,
    base: Option<Vec2>,
    fixed_angle: Option<f64>,
}

impl Interactive for XlineM {
    fn name(&self) -> &'static str {
        if self.ray { "RAY" } else { "XLINE" }
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.base, self.ray) {
            (None, false) => Prompt::new("Specify a point", Accept::POINT).kw(&["Hor", "Ver", "Ang", "Bisect", "Offset"]),
            (None, true) => Prompt::new("Specify start point", Accept::POINT),
            (Some(b), _) => Prompt::new("Specify through point", Accept::POINT).base(b),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Keyword(k) => {
                match k.as_str() {
                    "Hor" => self.fixed_angle = Some(0.0),
                    "Ver" => self.fixed_angle = Some(std::f64::consts::FRAC_PI_2),
                    _ => s.echo(format!("{k}: not available yet")),
                }
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                if let Some(a) = self.fixed_angle {
                    s.add_entity(EntityKind::XLine(RayLine { base: v3(p), dir: v3(Vec2::from_angle(a)) }))?;
                    return Ok(Step::Continue);
                }
                match self.base {
                    None => self.base = Some(p),
                    Some(b) => {
                        let d = (p - b).normalized();
                        if d != Vec2::ZERO {
                            let rl = RayLine { base: v3(b), dir: v3(d) };
                            s.add_entity(if self.ray { EntityKind::Ray(rl) } else { EntityKind::XLine(rl) })?;
                        }
                    }
                }
                Ok(Step::Continue)
            }
            Input::Enter => Ok(Step::Done),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if let Some(a) = self.fixed_angle {
            return vec![EntityKind::XLine(RayLine { base: v3(c), dir: v3(Vec2::from_angle(a)) })];
        }
        match self.base {
            Some(b) if !b.near(c, 1e-12) => {
                let rl = RayLine { base: v3(b), dir: v3((c - b).normalized()) };
                vec![if self.ray { EntityKind::Ray(rl) } else { EntityKind::XLine(rl) }]
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Default)]
struct SplineM {
    pts: Vec<Vec2>,
}

impl Interactive for SplineM {
    fn name(&self) -> &'static str {
        "SPLINE"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.pts.len() {
            0 => Prompt::new("Specify first point", Accept::POINT).kw(&["Method", "Knots", "Object"]),
            1 => Prompt::new("Enter next point", Accept::POINT).kw(&["start Tangency", "toLerance"]).base_opt(self.pts.last().copied()),
            2 => Prompt::new("Enter next point", Accept::POINT).kw(&["end Tangency", "toLerance", "Undo"]).base_opt(self.pts.last().copied()),
            _ => {
                Prompt::new("Enter next point", Accept::POINT).kw(&["end Tangency", "toLerance", "Undo", "Close"]).base_opt(self.pts.last().copied())
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match i {
            Input::Point(p) => {
                self.pts.push(p);
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Undo" => {
                self.pts.pop();
                Ok(Step::Continue)
            }
            Input::Keyword(k) if k == "Close" => {
                if let Some(f) = self.pts.first().copied() {
                    self.pts.push(f);
                }
                let mut sp = Spline::from_fit_points(&self.pts);
                sp.closed = true;
                s.add_entity(EntityKind::Spline(sp))?;
                Ok(Step::Done)
            }
            Input::Enter => {
                if self.pts.len() >= 2 {
                    s.add_entity(EntityKind::Spline(Spline::from_fit_points(&self.pts)))?;
                }
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut pts = self.pts.clone();
        pts.push(c);
        vec![EntityKind::Spline(Spline::from_fit_points(&pts))]
    }
}

#[derive(Default)]
struct DonutM {
    inside: Option<f64>,
    outside: Option<f64>,
}

impl Interactive for DonutM {
    fn name(&self) -> &'static str {
        "DONUT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let d = s.doc().ok();
        let di = d.map(|d| d.header.f64("DONUTID", 0.5)).unwrap_or(0.5);
        let dout = d.map(|d| d.header.f64("DONUTOD", 1.0)).unwrap_or(1.0);
        match (self.inside, self.outside) {
            (None, _) => Prompt::new("Specify inside diameter of donut", Accept::NUMBER).default(format!("{di:.4}")),
            (Some(_), None) => Prompt::new("Specify outside diameter of donut", Accept::NUMBER).default(format!("{dout:.4}")),
            _ => Prompt::new("Specify center of donut", Accept::POINT).kw(&[]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let hdr = |s: &Session, k: &str, d: f64| s.doc().map(|x| x.header.f64(k, d)).unwrap_or(d);
        match (self.inside, self.outside, i) {
            (None, _, Input::Text(t)) => {
                self.inside = Some(number(&t).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?.abs())
            }
            (None, _, Input::Enter) => self.inside = Some(hdr(s, "DONUTID", 0.5)),
            (Some(_), None, Input::Text(t)) => {
                self.outside = Some(number(&t).ok_or_else(|| crate::EngineError::Other("Requires a distance.".into()))?.abs())
            }
            (Some(_), None, Input::Enter) => self.outside = Some(hdr(s, "DONUTOD", 1.0)),
            (Some(a), Some(b), Input::Point(p)) => {
                s.add_entity(donut(p, a, b))?;
                let d = s.doc_mut()?;
                d.header.set_f64("DONUTID", a);
                d.header.set_f64("DONUTOD", b);
            }
            (Some(_), Some(_), Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.inside, self.outside) {
            (Some(a), Some(b)) => vec![donut(c, a, b)],
            _ => Vec::new(),
        }
    }
}

struct TextM {
    at: Option<Vec2>,
    height: Option<f64>,
    rotation: Option<f64>,
    justify: (HAlign, VAlign),
    asking_justify: bool,
    line: usize,
}

impl TextM {
    fn new(_s: &Session) -> Self {
        TextM { at: None, height: None, rotation: None, justify: (HAlign::Left, VAlign::Baseline), asking_justify: false, line: 0 }
    }
}

impl Interactive for TextM {
    fn name(&self) -> &'static str {
        "TEXT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let th = s.doc().map(|d| d.header.f64("TEXTSIZE", 0.2)).unwrap_or(0.2);
        if self.asking_justify {
            return Prompt::new("Enter an option", Accept::TEXT)
                .kw(&["Left", "Center", "Right", "Align", "Middle", "Fit", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"]);
        }
        match (self.at, self.height, self.rotation) {
            (None, _, _) => Prompt::new("Specify start point of text", Accept::POINT).kw(&["Justify", "Style"]),
            (Some(a), None, _) => Prompt::new("Specify height", Accept::POINT_OR_NUMBER).default(format!("{th:.4}")).base(a),
            (Some(a), Some(_), None) => Prompt::new("Specify rotation angle of text", Accept::POINT_OR_NUMBER).default("0").base(a),
            _ => Prompt::new("Enter text", Accept::TEXT),
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        let d = s.doc()?;
        let msg = format!(
            "Current text style:  \"{}\"  Text height:  {:.4}  Annotative:  No  Justify:  Left",
            d.header.str("TEXTSTYLE", "Standard"),
            d.header.f64("TEXTSIZE", 0.2)
        );
        s.echo(msg);
        Ok(Step::Continue)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if self.asking_justify {
            let code = match &i {
                Input::Keyword(k) | Input::Text(k) => k.clone(),
                _ => return Ok(Step::Continue),
            };
            self.justify = justify(&code).unwrap_or(self.justify);
            self.asking_justify = false;
            return Ok(Step::Continue);
        }
        let th = s.doc()?.header.f64("TEXTSIZE", 0.2);
        match (self.at, self.height, self.rotation, i) {
            (None, _, _, Input::Keyword(k)) if k == "Justify" => self.asking_justify = true,
            (None, _, _, Input::Point(p)) => self.at = Some(p),
            (Some(_), None, _, Input::Enter) => self.height = Some(th),
            (Some(a), None, _, Input::Point(p)) => self.height = Some(a.dist(p).max(1e-9)),
            (Some(_), None, _, Input::Text(t)) => {
                let h = number(&t).filter(|h| *h > 0.0).ok_or_else(|| crate::EngineError::Other("Requires a positive height.".into()))?;
                self.height = Some(h);
                s.doc_mut()?.header.set_f64("TEXTSIZE", h);
            }
            (Some(_), Some(_), None, Input::Enter) => self.rotation = Some(0.0),
            (Some(a), Some(_), None, Input::Point(p)) => self.rotation = Some(a.angle_to(p)),
            (Some(_), Some(_), None, Input::Text(t)) => {
                self.rotation = Some(crate::units::parse_angle(&t).ok_or_else(|| crate::EngineError::Other("Requires an angle.".into()))?)
            }
            (Some(a), Some(h), Some(r), Input::Text(t)) => {
                let at = a + Vec2::from_angle(r - std::f64::consts::FRAC_PI_2) * (h * 5.0 / 3.0 * self.line as f64);
                let k = make_text(s, at, &t, h, r, self.justify);
                s.add_entity(k)?;
                self.line += 1;
            }
            (Some(_), Some(_), Some(_), Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
}

#[derive(Default)]
struct MTextM {
    first: Option<Vec2>,
    second: Option<Vec2>,
    /// Paragraphs typed so far: one per input line, an empty line ends the text (as on
    /// AutoCAD's command line and in scripts).
    lines: Vec<String>,
}

impl Interactive for MTextM {
    fn name(&self) -> &'static str {
        "MTEXT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.first, self.second) {
            (None, _) => Prompt::new("Specify first corner", Accept::POINT),
            (Some(a), None) => Prompt::new("Specify opposite corner", Accept::POINT)
                .kw(&["Height", "Justify", "Line spacing", "Rotation", "Style", "Width", "Columns"])
                .base(a),
            _ if self.lines.is_empty() => Prompt::new("Enter text (use \\P for new paragraphs)", Accept::TEXT),
            _ => Prompt::new("Enter next line of text (empty line to finish)", Accept::TEXT),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, self.second, i) {
            (None, _, Input::Point(p)) => self.first = Some(p),
            (Some(_), None, Input::Point(p)) => self.second = Some(p),
            (Some(_), Some(_), Input::Text(t)) if !t.is_empty() => self.lines.push(t),
            (Some(a), Some(b), Input::Text(_) | Input::Enter) => {
                if self.lines.is_empty() {
                    return Ok(Step::Done);
                }
                let d = s.doc()?;
                let h = d.header.f64("TEXTSIZE", 0.2);
                let style = d.header.str("TEXTSTYLE", "Standard");
                let tl = Vec2::new(a.x.min(b.x), a.y.max(b.y));
                s.add_entity(EntityKind::MText(MText {
                    insert: Vec3::new(tl.x, tl.y, 0.0),
                    height: h,
                    width: (a.x - b.x).abs(),
                    attach: 1,
                    rotation: 0.0,
                    style,
                    contents: self.lines.join("\\P"),
                    line_spacing: 1.0,
                }))?;
                return Ok(Step::Done);
            }
            (_, _, Input::Enter) => return Ok(Step::Done),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.first, self.second) {
            (Some(a), None) => vec![lwpoly(rect_vertices(a, c), true)],
            _ => Vec::new(),
        }
    }
}

/// Re-export for tests.
pub(crate) fn _unused(_: Polyline) {}
