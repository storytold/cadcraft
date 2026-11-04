//! BREAKLINE (Express Tools): one polyline from the first point to the second with a break
//! symbol at a location along it, the line optionally extended past both points.
//!
//! The symbol is a run of vertices between two attachment points. The built-in one is our own
//! zigzag; the Block option takes a block of the drawing that marks its attachment points with
//! two POINT objects on the Defpoints layer and draws the symbol with lines and polylines.

use cadcraft_doc::{Drawing, EntityKind};
use cadcraft_geom::{PolyVertex, Vec2};
use serde_json::Value;

use super::super::machines::number;
use super::added;
use super::curves::MAX_GEN;
use super::{CommandSpec, line, lwpoly, point_param, point_req, size_param, str_param};

use crate::cmd::modify2::as_lwpoly;
use crate::{Accept, EngineError, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("breakline", "Break-line Symbol", run_breakline)
            .menu(&["Tools", "Express Tools", "Break-line Symbol"])
            .params(
                "{p1, p2, at?: [x,y] (symbol location; default the midpoint), size? (default 1, times DIMSCALE), \
                 extension? (past both points; default 0), block? (a block with two points on Defpoints; default the built-in symbol)} → {handle}",
            )
            .interactive(|s| Ok(Box::new(BreaklineM::new(s)))),
    ]
}

/// The values BREAKLINE offers as defaults: the symbol block (empty = built-in), its size and
/// how far the line runs past the two points.
#[derive(Clone, Debug, PartialEq)]
pub struct BreaklineOptions {
    pub block: String,
    pub size: f64,
    pub extension: f64,
}

impl Default for BreaklineOptions {
    fn default() -> Self {
        BreaklineOptions { block: String::new(), size: 1.0, extension: 0.0 }
    }
}

fn other(m: impl Into<String>) -> EngineError {
    EngineError::Other(m.into())
}

/// A break symbol in its own coordinates: the vertices from one attachment point to the other.
struct Symbol {
    path: Vec<PolyVertex>,
}

impl Symbol {
    /// Our own zigzag: up and across to the other side of the line, one unit between the
    /// attachment points and one unit tall.
    fn built_in() -> Self {
        let pts = [(-0.5, 0.0), (-0.25, 0.5), (0.25, -0.5), (0.5, 0.0)];
        Symbol { path: pts.into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).collect() }
    }

    fn of_block(d: &Drawing, name: &str) -> Result<Self> {
        let b = d.block(name).ok_or_else(|| other(format!("Block \"{name}\" not found.")))?;
        let mut anchors = Vec::new();
        let mut pieces: Vec<Vec<PolyVertex>> = Vec::new();
        for e in b.entities.iter() {
            match &e.kind {
                EntityKind::Point(p) if e.common.layer.eq_ignore_ascii_case("Defpoints") => anchors.push(p.p.xy()),
                k => {
                    if let Some(pl) = as_lwpoly(k).filter(|pl| !pl.closed && pl.vertices.len() >= 2)
                        && pieces.len() < MAX_GEN
                    {
                        pieces.push(pl.vertices);
                    }
                }
            }
        }
        let [a, z] = anchors[..] else {
            return Err(other(format!("Block \"{name}\" needs exactly two points on the Defpoints layer.")));
        };
        let tol = a.dist(z) * 1e-6;
        if !(tol > 0.0 && tol.is_finite()) {
            return Err(other(format!("The two Defpoints points of block \"{name}\" coincide.")));
        }
        // Chain the pieces end to end from one attachment point until the other is reached.
        let mut path = vec![PolyVertex::new(a)];
        let mut at = a;
        while !at.near(z, tol) {
            let next = pieces.iter().position(|p| p.first().is_some_and(|v| v.p.near(at, tol)) || p.last().is_some_and(|v| v.p.near(at, tol)));
            let Some(i) = next else {
                return Err(other(format!("The lines of block \"{name}\" must run from one Defpoints point to the other.")));
            };
            let mut piece = pieces.swap_remove(i);
            if !piece.first().is_some_and(|v| v.p.near(at, tol)) {
                piece = reversed(&piece);
            }
            if let Some(last) = path.last_mut() {
                last.bulge = piece.first().map_or(0.0, |v| v.bulge);
            }
            path.extend(piece.iter().skip(1).copied());
            at = path.last().map_or(z, |v| v.p);
        }
        Ok(Symbol { path })
    }

    fn span(&self) -> f64 {
        match (self.path.first(), self.path.last()) {
            (Some(a), Some(z)) => a.p.dist(z.p),
            _ => 0.0,
        }
    }

    /// The symbol scaled by `k`, its attachment points on the line through `from` along `u`.
    fn placed(&self, from: Vec2, u: Vec2, k: f64) -> Vec<PolyVertex> {
        let (Some(a), Some(z)) = (self.path.first(), self.path.last()) else { return Vec::new() };
        let ax = (z.p - a.p).normalized();
        self.path
            .iter()
            .map(|v| {
                let q = v.p - a.p;
                let (x, y) = (q.dot(ax) * k, q.dot(ax.perp()) * k);
                PolyVertex::with_bulge(from + u * x + u.perp() * y, v.bulge)
            })
            .collect()
    }
}

/// The same vertices run backwards, each arc keeping its side.
fn reversed(vs: &[PolyVertex]) -> Vec<PolyVertex> {
    let n = vs.len();
    (0..n)
        .rev()
        .filter_map(|i| {
            let v = vs.get(i)?;
            let bulge = if i == 0 { 0.0 } else { -vs.get(i - 1).map_or(0.0, |p| p.bulge) };
            Some(PolyVertex::with_bulge(v.p, bulge))
        })
        .collect()
}

fn dimscale(s: &Session) -> f64 {
    s.doc().ok().map(|d| d.header.f64("DIMSCALE", 1.0)).filter(|k| k.is_finite() && *k > 0.0).unwrap_or(1.0)
}

fn symbol(s: &Session, block: &str) -> Result<Symbol> {
    if block.is_empty() { Ok(Symbol::built_in()) } else { Symbol::of_block(s.doc()?, block) }
}

/// The breakline's vertices: the extension, the line up to the symbol, the symbol and the rest of
/// the line. The symbol is centred on `at` projected onto the line (the midpoint without it) and
/// kept between the two points.
fn breakline_vertices(sym: &Symbol, p1: Vec2, p2: Vec2, at: Option<Vec2>, k: f64, ext: f64) -> Result<Vec<PolyVertex>> {
    let len = p1.dist(p2);
    let u = (p2 - p1).normalized();
    if !(len.is_finite() && len > 0.0) || u == Vec2::ZERO {
        return Err(other("The two points of the breakline must differ."));
    }
    let span = sym.span() * k;
    if !span.is_finite() || span <= 0.0 {
        return Err(other("The break symbol has no size."));
    }
    if span > len {
        return Err(other(format!("The breakline must be at least as long as the symbol ({span:.4}).")));
    }
    let t = at.map_or(len / 2.0, |c| (c - p1).dot(u)).clamp(span / 2.0, len - span / 2.0);
    let mut pts = vec![PolyVertex::new(p1 - u * ext)];
    pts.extend(sym.placed(p1 + u * (t - span / 2.0), u, k));
    pts.push(PolyVertex::new(p2 + u * ext));
    let mut out: Vec<PolyVertex> = Vec::with_capacity(pts.len());
    for v in pts {
        if !v.p.is_finite() {
            return Err(other("The breakline is too large."));
        }
        match out.last_mut() {
            Some(l) if l.p.near(v.p, len * 1e-12) => l.bulge = v.bulge,
            _ => out.push(v),
        }
    }
    if let Some(l) = out.last_mut() {
        l.bulge = 0.0;
    }
    Ok(out)
}

fn run_breakline(s: &mut Session, p: &Value) -> Result<Value> {
    let p1 = point_req("breakline", p, "p1")?;
    let p2 = point_req("breakline", p, "p2")?;
    let size = size_param("breakline", p, "size", false)?.unwrap_or(1.0);
    let ext = size_param("breakline", p, "extension", true)?.unwrap_or(0.0);
    let sym = symbol(s, str_param(p, "block").unwrap_or(""))?;
    let vs = breakline_vertices(&sym, p1, p2, point_param(p, "at"), size * dimscale(s), ext)?;
    added(s.add_entity(lwpoly(vs, false))?)
}

#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Points,
    Block,
    Size,
    Extension,
}

struct BreaklineM {
    opts: BreaklineOptions,
    ask: Ask,
    p1: Option<Vec2>,
    p2: Option<Vec2>,
}

impl BreaklineM {
    fn new(s: &Session) -> Self {
        BreaklineM { opts: s.last_used.breakline.clone(), ask: Ask::Points, p1: None, p2: None }
    }
    fn block_label(&self) -> String {
        if self.opts.block.is_empty() { "(built-in)".into() } else { self.opts.block.clone() }
    }
    fn finish(&self, s: &mut Session, at: Option<Vec2>) -> Result<Step> {
        let (Some(p1), Some(p2)) = (self.p1, self.p2) else { return Ok(Step::Cancel) };
        let vs = breakline_vertices(&symbol(s, &self.opts.block)?, p1, p2, at, self.opts.size * dimscale(s), self.opts.extension)?;
        s.add_entity(lwpoly(vs, false))?;
        Ok(Step::Done)
    }
}

impl Interactive for BreaklineM {
    fn name(&self) -> &'static str {
        "BREAKLINE"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        s.echo(format!("Block= {}, Size= {:.4}, Extension= {:.4}", self.block_label(), self.opts.size, self.opts.extension));
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.ask, self.p1, self.p2) {
            (Ask::Block, ..) => {
                Prompt::new("Enter name of breakline symbol block (. for the built-in symbol)", Accept::TEXT).default(self.block_label())
            }
            (Ask::Size, ..) => Prompt::new("Breakline symbol size", Accept::NUMBER).default(format!("{:.4}", self.opts.size)),
            (Ask::Extension, ..) => Prompt::new("Breakline extension distance", Accept::NUMBER).default(format!("{:.4}", self.opts.extension)),
            (Ask::Points, None, _) => Prompt::new("Specify first point for breakline", Accept::POINT).kw(&["Block", "Size", "Extension"]),
            (Ask::Points, Some(a), None) => Prompt::new("Specify second point for breakline", Accept::POINT).base(a),
            (Ask::Points, Some(_), Some(_)) => Prompt::new("Specify location for break symbol", Accept::POINT).default("Midpoint"),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.ask, i) {
            (Ask::Block, Input::Text(t) | Input::Keyword(t)) => {
                let t = t.trim();
                let name = if t == "." { String::new() } else { t.to_string() };
                symbol(s, &name)?;
                self.opts.block = name;
                self.ask = Ask::Points;
            }
            (Ask::Size, Input::Text(t)) => {
                self.opts.size = number(&t).filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| other("Requires a positive value."))?;
                self.ask = Ask::Points;
            }
            (Ask::Extension, Input::Text(t)) => {
                self.opts.extension = number(&t).filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| other("Requires zero or a positive value."))?;
                self.ask = Ask::Points;
            }
            (Ask::Block | Ask::Size | Ask::Extension, Input::Enter) => self.ask = Ask::Points,
            (Ask::Points, Input::Keyword(k)) if self.p1.is_none() => {
                self.ask = match k.as_str() {
                    "Block" => Ask::Block,
                    "Size" => Ask::Size,
                    "Extension" => Ask::Extension,
                    _ => Ask::Points,
                };
            }
            (Ask::Points, Input::Point(p)) => match (self.p1, self.p2) {
                (None, _) => self.p1 = Some(p),
                (Some(a), None) => {
                    let span = symbol(s, &self.opts.block)?.span() * self.opts.size * dimscale(s);
                    let len = a.dist(p);
                    if len.is_nan() || len < span {
                        return Err(other(format!("The breakline must be at least as long as the symbol ({span:.4}).")));
                    }
                    self.p2 = Some(p);
                }
                (Some(_), Some(_)) => return self.finish(s, Some(p)),
            },
            (Ask::Points, Input::Enter) if self.p2.is_some() => return self.finish(s, None),
            (Ask::Points, Input::Enter) => return Ok(Step::Cancel),
            _ => {}
        }
        s.last_used.breakline = self.opts.clone();
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, c: Vec2) -> Vec<EntityKind> {
        match (self.ask, self.p1, self.p2) {
            (Ask::Points, Some(a), None) => vec![line(a, c)],
            (Ask::Points, Some(a), Some(b)) => symbol(s, &self.opts.block)
                .and_then(|sym| breakline_vertices(&sym, a, b, Some(c), self.opts.size * dimscale(s), self.opts.extension))
                .map(|vs| vec![lwpoly(vs, false)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}
