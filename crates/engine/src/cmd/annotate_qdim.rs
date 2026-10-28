//! QDIM: quick dimensions of the selected geometry. Continuous, Staggered, Baseline, Ordinate,
//! Radius and Diameter, a datum point for Baseline and Ordinate, and Edit (add or remove
//! dimension points). The prompt sequence runs the JSON form.

use cadcraft_doc::{DimKind, EntityKind, Handle, Prim};
use cadcraft_geom::{Segment, Vec2};
use serde_json::{Value, json};

use super::super::machines::{SelOutcome, SelectPhase};
use super::super::{bad, point_param, point_req, points_param, str_param, targets};
use super::{MAX_CHAIN, assoc_enabled, dim};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub(super) const PARAMS: &str = "{handles?, at: [x,y] (dimension line position), mode?: continuous|staggered|baseline|ordinate|radius|diameter (default continuous), vertical?: bool (default: from `at`), datum?: [x,y] (baseline and ordinate; default the first point, or the origin for ordinate), add?: [[x,y]], remove?: [[x,y]] (dimension points; removes those within `tolerance`)}";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Continuous,
    Staggered,
    Baseline,
    Ordinate,
    Radius,
    Diameter,
}

impl Mode {
    fn parse(s: &str) -> Option<Mode> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "continuous" | "c" => Mode::Continuous,
            "staggered" | "s" => Mode::Staggered,
            "baseline" | "b" => Mode::Baseline,
            "ordinate" | "o" => Mode::Ordinate,
            "radius" | "r" => Mode::Radius,
            "diameter" | "d" => Mode::Diameter,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Mode::Continuous => "Continuous",
            Mode::Staggered => "Staggered",
            Mode::Baseline => "Baseline",
            Mode::Ordinate => "Ordinate",
            Mode::Radius => "Radius",
            Mode::Diameter => "Diameter",
        }
    }
}

/// The dimension points (segment ends, circle centres, points) and the circles and arcs of the
/// objects.
fn geometry(s: &Session, hs: &[Handle]) -> Result<(Vec<Vec2>, Vec<(Vec2, f64)>)> {
    let d = s.doc()?;
    let mut pts = Vec::new();
    let mut circles = Vec::new();
    for h in hs {
        let Some(e) = d.entity(*h) else { continue };
        for pr in e.kind.prims() {
            match pr {
                Prim::Seg(seg) => {
                    pts.extend([seg.start(), seg.end()]);
                    if let Segment::Arc { arc, .. } = seg {
                        circles.push((arc.center, arc.radius));
                    }
                }
                Prim::Circle(c) => {
                    pts.push(c.center);
                    circles.push((c.center, c.radius));
                }
                Prim::Point(q) => pts.push(q),
                _ => {}
            }
            if pts.len() > MAX_CHAIN * 2 {
                return Err(bad("qdim", format!("at most {MAX_CHAIN} distinct points per call")));
            }
        }
    }
    Ok((pts, circles))
}

/// The quick dimensions, without adding them. `pts` are the dimension points (unsorted).
fn plan(
    s: &Session,
    pts: &[Vec2],
    circles: &[(Vec2, f64)],
    mode: Mode,
    at: Vec2,
    vertical: Option<bool>,
    datum: Option<Vec2>,
) -> Result<Vec<EntityKind>> {
    if matches!(mode, Mode::Radius | Mode::Diameter) {
        let mut out = Vec::new();
        for (c, r) in circles.iter().take(MAX_CHAIN) {
            let dir = (at - *c).normalized();
            let dir = if dir == Vec2::ZERO || !dir.is_finite() { Vec2::from_angle(std::f64::consts::FRAC_PI_4) } else { dir };
            let p = *c + dir * *r;
            out.push(if mode == Mode::Diameter {
                dim(s, DimKind::Diameter, *c - dir * *r, Vec2::ZERO, Vec2::ZERO, p, "")
            } else {
                dim(s, DimKind::Radius, *c, Vec2::ZERO, Vec2::ZERO, p, "")
            });
        }
        if out.is_empty() {
            return Err(bad("qdim", "select circles or arcs for radius or diameter dimensions"));
        }
        return Ok(out);
    }
    // Horizontal dimensions measure along x; vertical ones along y. Without a choice: vertical
    // when the line position is beside the geometry, as with DIMLINEAR.
    let bounds = cadcraft_geom::Bounds2::from_points(pts.iter().copied());
    let vertical = vertical.unwrap_or_else(|| {
        let beside = at.x < bounds.min.x || at.x > bounds.max.x;
        let within = at.y >= bounds.min.y && at.y <= bounds.max.y;
        beside && within
    });
    let key = |q: &Vec2| if vertical { q.y } else { q.x };
    let mut pts = pts.to_vec();
    pts.sort_by(|a, b| key(a).total_cmp(&key(b)));
    pts.dedup_by(|a, b| (key(a) - key(b)).abs() < 1e-9);
    if pts.len() > MAX_CHAIN {
        return Err(bad("qdim", format!("at most {MAX_CHAIN} distinct points per call")));
    }
    if mode == Mode::Ordinate {
        if pts.is_empty() {
            return Err(bad("qdim", "select objects with points to dimension"));
        }
        let origin = datum.unwrap_or(Vec2::ZERO);
        // X datum (horizontal): leaders run to the line position's y; Y datum: to its x.
        return Ok(pts
            .iter()
            .map(|p| {
                let end = if vertical { Vec2::new(at.x, p.y) } else { Vec2::new(p.x, at.y) };
                dim(s, DimKind::Ordinate { x_type: !vertical }, origin, *p, end, Vec2::ZERO, "")
            })
            .collect());
    }
    if pts.len() < 2 {
        return Err(bad("qdim", "select objects with at least two distinct points"));
    }
    let rot = if vertical { std::f64::consts::FRAC_PI_2 } else { 0.0 };
    let d = s.doc()?;
    let st = d.dim_style(&d.header.str("DIMSTYLE", "Standard")).cloned().unwrap_or_default().with_overrides(&d.dim_overrides());
    let step = st.baseline_spacing * st.effective_scale(d.header.f64("DIMSCALE", 1.0));
    let step = if step.is_finite() && step > 0.0 { step } else { 0.38 };
    // Away from the geometry: stacked dimensions go further out from the line position.
    let mid = bounds.center();
    let out_dir =
        if vertical { Vec2::new(if at.x >= mid.x { 1.0 } else { -1.0 }, 0.0) } else { Vec2::new(0.0, if at.y >= mid.y { 1.0 } else { -1.0 }) };
    let lin = |a: Vec2, b: Vec2, k: usize| dim(s, DimKind::Linear { rotation: rot }, at + out_dir * (step * k as f64), a, b, Vec2::ZERO, "");
    Ok(match mode {
        Mode::Baseline => {
            // From the datum (the point nearest it, by default the first) to every other point.
            let base = match datum {
                Some(dp) => pts.iter().copied().min_by(|a, b| a.dist(dp).total_cmp(&b.dist(dp))).unwrap_or(dp),
                None => pts.first().copied().unwrap_or_default(),
            };
            let mut others: Vec<Vec2> = pts.iter().copied().filter(|p| (key(p) - key(&base)).abs() > 1e-9).collect();
            others.sort_by(|a, b| (key(a) - key(&base)).abs().total_cmp(&(key(b) - key(&base)).abs()));
            others.iter().enumerate().map(|(k, p)| lin(base, *p, k)).collect()
        }
        Mode::Staggered => {
            // Symmetric pairs nested from the middle out; the innermost at the line position.
            let n = pts.len();
            let pairs = n / 2;
            (0..pairs).rev().enumerate().filter_map(|(k, i)| Some(lin(*pts.get(i)?, *pts.get(n - 1 - i)?, k))).collect()
        }
        _ => pts.windows(2).filter_map(|w| Some(lin(*w.first()?, *w.get(1)?, 0))).collect(),
    })
}

/// The nearest point of `pts` to `q` within `tol`, by index.
fn nearest(pts: &[Vec2], q: Vec2, tol: f64) -> Option<usize> {
    pts.iter().enumerate().filter(|(_, p)| p.dist(q) <= tol).min_by(|a, b| a.1.dist(q).total_cmp(&b.1.dist(q))).map(|(i, _)| i)
}

pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.len() > MAX_CHAIN {
        return Err(bad("qdim", format!("at most {MAX_CHAIN} objects per call")));
    }
    let at = point_req("qdim", p, "at")?;
    let mode = match str_param(p, "mode") {
        None => Mode::Continuous,
        Some(m) => {
            Mode::parse(m).ok_or_else(|| bad("qdim", format!("unknown mode `{m}` (continuous, staggered, baseline, ordinate, radius, diameter)")))?
        }
    };
    let vertical = p.get("vertical").and_then(Value::as_bool);
    let datum = point_param(p, "datum");
    let (mut pts, circles) = geometry(s, &hs)?;
    // Edit: points to add and to remove (the nearest within a small distance).
    for q in points_param(p, "add").unwrap_or_default().into_iter().take(MAX_CHAIN) {
        pts.push(q);
    }
    let tol = p.get("tolerance").and_then(Value::as_f64).filter(|t| t.is_finite() && *t >= 0.0).unwrap_or(1e-6 * (1.0 + at.len()));
    for q in points_param(p, "remove").unwrap_or_default().into_iter().take(MAX_CHAIN) {
        while let Some(i) = nearest(&pts, q, tol) {
            pts.swap_remove(i);
        }
    }
    let kinds = plan(s, &pts, &circles, mode, at, vertical, datum)?;
    // One snap index for all the dimensions, as DIMCONTINUE does: looking up each dimension's
    // objects by scanning the drawing made QDIM quadratic (the added dimensions are no snap targets).
    let snaps = if assoc_enabled(s) { Some(crate::assoc::SnapIndex::new(s.doc()?, &s.space())) } else { None };
    let mut out = Vec::new();
    for mut k in kinds {
        if let (Some(ix), EntityKind::Dimension(dm)) = (&snaps, &mut k) {
            dm.assoc = match dm.kind {
                DimKind::Radius | DimKind::Diameter => ix.auto_assoc(&[("p15", dm.p15.xy())]),
                DimKind::Ordinate { .. } => ix.auto_assoc(&[("p13", dm.p13.xy())]),
                _ => ix.auto_assoc(&[("p13", dm.p13.xy()), ("p14", dm.p14.xy())]),
            };
        }
        let h = s.add_entity(k)?;
        s.last_dim = Some(h);
        out.push(h.hex());
    }
    Ok(json!({ "handles": out }))
}

/// What the QDIM prompt is asking.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Ask {
    #[default]
    Position,
    Datum,
    Remove,
    Add,
    Settings,
}

/// QDIM at the command line: "Select geometry to dimension", then the dimension line position
/// or a mode, datumPoint, Edit (Remove/Add points) or seTtings.
pub(super) struct QDimM {
    sel: SelectPhase,
    objs: Vec<Handle>,
    mode: Mode,
    datum: Option<Vec2>,
    add: Vec<Vec2>,
    remove: Vec<Vec2>,
    /// How near a picked point must be to the dimension point it removes (the pick box).
    tol: f64,
    ask: Ask,
}

impl QDimM {
    pub(super) fn new() -> Self {
        QDimM {
            sel: SelectPhase::default(),
            objs: Vec::new(),
            mode: Mode::Continuous,
            datum: None,
            add: Vec::new(),
            remove: Vec::new(),
            tol: 1e-6,
            ask: Ask::Position,
        }
    }
    fn params(&self, at: Vec2) -> Value {
        let pts = |v: &[Vec2]| v.iter().map(|q| json!([q.x, q.y])).collect::<Vec<_>>();
        let mut p = json!({
            "handles": self.objs.iter().map(|h| h.hex()).collect::<Vec<_>>(),
            "at": [at.x, at.y],
            "mode": self.mode.name().to_ascii_lowercase(),
            "add": pts(&self.add),
            "remove": pts(&self.remove),
            "tolerance": self.tol,
        });
        if let (Some(dp), Some(o)) = (self.datum, p.as_object_mut()) {
            o.insert("datum".into(), json!([dp.x, dp.y]));
        }
        p
    }
}

impl Interactive for QDimM {
    fn name(&self) -> &'static str {
        "QDIM"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return Prompt::new(if self.sel.removing { "Remove objects" } else { "Select geometry to dimension" }, Accept::SELECT);
        }
        match self.ask {
            Ask::Position => Prompt::new("Specify dimension line position", Accept::POINT)
                .kw(&["Continuous", "Staggered", "Baseline", "Ordinate", "Radius", "Diameter", "datumPoint", "Edit", "seTtings"])
                .default(self.mode.name()),
            Ask::Datum => Prompt::new("Select new datum point", Accept::POINT),
            Ask::Remove => Prompt::new("Indicate dimension point to remove", Accept::POINT).kw(&["Add", "eXit"]).default("eXit"),
            Ask::Add => Prompt::new("Indicate dimension point to add", Accept::POINT).kw(&["Remove", "eXit"]).default("eXit"),
            Ask::Settings => Prompt::new("Associative dimension priority", Accept::TEXT).kw(&["Endpoint", "Intersection"]).default("Endpoint"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    Ok(Step::Continue)
                }
            };
        }
        match (self.ask, i) {
            (Ask::Position, Input::Point(at)) => {
                let r = run(s, &self.params(at))?;
                let n = r.get("handles").and_then(Value::as_array).map_or(0, Vec::len);
                s.echo(format!("{n} dimension(s) created."));
                s.set_selection(Vec::new());
                return Ok(Step::Done);
            }
            (Ask::Position, Input::Keyword(k)) => match k.as_str() {
                "datumPoint" => self.ask = Ask::Datum,
                "Edit" => self.ask = Ask::Remove,
                "seTtings" => self.ask = Ask::Settings,
                m => {
                    if let Some(m) = Mode::parse(m) {
                        self.mode = m;
                    }
                }
            },
            (Ask::Datum, Input::Point(p)) => {
                self.datum = Some(p);
                self.ask = Ask::Position;
            }
            (Ask::Remove, Input::Point(p)) => {
                self.tol = self.tol.max(s.pixel_size() * s.settings.pickbox.max(1.0));
                self.remove.push(p);
            }
            (Ask::Add, Input::Point(p)) => self.add.push(p),
            (Ask::Remove, Input::Keyword(k)) if k == "Add" => self.ask = Ask::Add,
            (Ask::Add, Input::Keyword(k)) if k == "Remove" => self.ask = Ask::Remove,
            (Ask::Remove | Ask::Add | Ask::Settings, Input::Keyword(_) | Input::Enter) => self.ask = Ask::Position,
            (Ask::Settings, Input::Text(_)) => self.ask = Ask::Position,
            (_, Input::Enter | Input::Cancel) => return Ok(Step::Cancel),
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        if !self.sel.done || self.ask != Ask::Position || self.objs.len() > 1000 {
            return Vec::new();
        }
        let Ok((mut pts, circles)) = geometry(s, &self.objs) else { return Vec::new() };
        if pts.len() > 2000 {
            return Vec::new();
        }
        pts.extend(&self.add);
        for q in &self.remove {
            while let Some(i) = nearest(&pts, *q, self.tol) {
                pts.swap_remove(i);
            }
        }
        plan(s, &pts, &circles, self.mode, c, None, self.datum).unwrap_or_default()
    }
}
