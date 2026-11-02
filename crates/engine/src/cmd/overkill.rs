//! OVERKILL (Delete Duplicate Objects): deletes duplicates within a tolerance, deletes lines and
//! arcs that lie on another one, combines collinear lines and arcs on the same circle that
//! partially overlap (and, as an option, ones that meet end to end), and optimizes polylines
//! (zero-length and collinear segments).
//!
//! Work stays near O(n log n): duplicates are hashed, lines are bucketed by direction and offset,
//! arcs by circle.

use std::collections::{HashMap, HashSet};

use cadcraft_doc::{Circle, Common, Entity, EntityKind, Handle};
use cadcraft_geom::{TAU, Vec2, Vec3, ccw_sweep, norm_angle};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase, number};
use super::{CommandSpec, bad, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("overkill", "Delete Duplicate Objects", run_overkill)
            .menu(&["Modify", "Delete Duplicate Objects"])
            .alias(&["-overkill"])
            .params(
                "{handles? (default: the whole current space), tolerance? (0..1e6, default 1e-6), ignore?: [color|layer|linetype|ltscale|lineweight|thickness|transparency|plotStyle], plines?: bool (default true), partialOverlap?: bool (default true), endToEnd?: bool (default false), associativity?: bool (keep associated objects, default true)} → {deleted, duplicates, overlapping, segments}",
            )
            .interactive(|_| Ok(Box::new(OverkillM::default()))),
    ]
}

const DEFAULT_TOL: f64 = 1e-6;
const MAX_TOL: f64 = 1e6;
/// Where line directions wrap (radians): an angle real drawings rarely use, so collinear lines
/// almost never straddle it.
const SEAM: f64 = -1.234_567;

/// Properties OVERKILL can ignore when comparing objects: keyword, JSON name.
const IGNORABLE: [(&str, &str); 8] = [
    ("Color", "color"),
    ("LAyer", "layer"),
    ("LType", "linetype"),
    ("LTScale", "ltscale"),
    ("LWeight", "lineweight"),
    ("THickness", "thickness"),
    ("TRansparency", "transparency"),
    ("PLotstyle", "plotStyle"),
];

#[derive(Clone, Debug, PartialEq)]
struct Opts {
    tol: f64,
    ignore: [bool; 8],
    plines: bool,
    partial: bool,
    end_to_end: bool,
    keep_assoc: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { tol: DEFAULT_TOL, ignore: [false; 8], plines: true, partial: true, end_to_end: false, keep_assoc: true }
    }
}

fn check_tol(t: f64) -> std::result::Result<f64, String> {
    if t.is_finite() && (0.0..=MAX_TOL).contains(&t) { Ok(t) } else { Err(format!("tolerance must be a number from 0 to {MAX_TOL}")) }
}

impl Opts {
    fn from_json(p: &Value) -> Result<Self> {
        let mut o = Opts::default();
        match p.get("tolerance") {
            None | Some(Value::Null) => {}
            Some(v) => {
                o.tol = v
                    .as_f64()
                    .ok_or_else(|| bad("overkill", "`tolerance` must be a number"))
                    .and_then(|t| check_tol(t).map_err(|e| bad("overkill", e)))?
            }
        }
        match p.get("ignore") {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) => {
                for x in a {
                    let n = x.as_str().unwrap_or_default();
                    let i = IGNORABLE
                        .iter()
                        .position(|(_, name)| name.eq_ignore_ascii_case(n))
                        .ok_or_else(|| bad("overkill", format!("`ignore`: unknown property {x}")))?;
                    if let Some(b) = o.ignore.get_mut(i) {
                        *b = true;
                    }
                }
            }
            Some(v) => return Err(bad("overkill", format!("`ignore` must be a list of property names, got {v}"))),
        }
        let flag = |key: &str, default: bool| match p.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(v) => v.as_bool().ok_or_else(|| bad("overkill", format!("`{key}` must be true or false"))),
        };
        o.plines = flag("plines", o.plines)?;
        o.partial = flag("partialOverlap", o.partial)?;
        o.end_to_end = flag("endToEnd", o.end_to_end)?;
        o.keep_assoc = flag("associativity", o.keep_assoc)?;
        Ok(o)
    }
    /// The comparison quantum: the tolerance, or a hair above zero.
    fn q(&self) -> f64 {
        self.tol.max(1e-12)
    }
    /// The properties compared, as a key (ignored ones reset to their defaults).
    fn prop_key(&self, c: &Common) -> String {
        let mut c = c.clone();
        let d = Common::default();
        let on = |i: usize| self.ignore.get(i).copied().unwrap_or(false);
        if on(0) {
            c.color = d.color;
        }
        if on(1) {
            c.layer.clone_from(&d.layer);
        }
        if on(2) {
            c.linetype.clone_from(&d.linetype);
        }
        if on(3) {
            c.ltscale = d.ltscale;
        }
        if on(4) {
            c.lineweight = d.lineweight;
        }
        if on(5) {
            c.thickness = d.thickness;
        }
        if on(6) {
            c.transparency = d.transparency;
        }
        if on(7) {
            c.plot_style.clone_from(&d.plot_style);
        }
        serde_json::to_string(&c).unwrap_or_default()
    }
    fn ignored_names(&self) -> String {
        let v: Vec<&str> = IGNORABLE.iter().zip(self.ignore).filter(|(_, on)| *on).map(|((k, _), _)| *k).collect();
        if v.is_empty() { "None".into() } else { v.join(", ") }
    }
}

/// Round every number of a JSON value to a multiple of `q`.
fn quantize(v: &mut Value, q: f64) {
    match v {
        Value::Number(n) => {
            if let Some(x) = n.as_f64() {
                *v = json!((x / q).round());
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| quantize(x, q)),
        Value::Object(o) => o.values_mut().for_each(|x| quantize(x, q)),
        _ => {}
    }
}

#[derive(Default)]
struct Report {
    duplicates: usize,
    overlapping: usize,
    segments: usize,
}

/// One candidate object.
struct Cand {
    e: Entity,
    prop: u32,
    protected: bool,
}

/// Run OVERKILL on `hs` (or the whole current space). Objects on locked layers are left alone.
fn overkill(s: &mut Session, hs: Option<Vec<Handle>>, o: &Opts) -> Result<Report> {
    let space = s.space();
    let d = s.doc()?;
    let only: Option<HashSet<Handle>> = hs.map(|v| v.into_iter().collect());
    // Objects other objects follow (associative dimensions), kept as they are when asked.
    let mut followed: HashSet<Handle> = HashSet::new();
    if o.keep_assoc {
        let all = d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter()));
        for e in all {
            if let EntityKind::Dimension(dm) = &e.kind {
                followed.extend(dm.assoc.iter().map(|a| a.handle));
            }
        }
    }
    let mut props: HashMap<String, u32> = HashMap::new();
    let mut cands: Vec<Cand> = Vec::new();
    for e in d.space(&space).map(|st| st.iter()).into_iter().flatten() {
        if only.as_ref().is_some_and(|set| !set.contains(&e.handle)) || d.layer(&e.common.layer).is_some_and(|l| l.locked) {
            continue;
        }
        let key = o.prop_key(&e.common);
        let n = props.len() as u32;
        let prop = *props.entry(key).or_insert(n);
        cands.push(Cand { e: (**e).clone(), prop, protected: followed.contains(&e.handle) });
    }
    let mut rep = Report::default();
    let mut gone = vec![false; cands.len()];
    let mut changed: HashMap<usize, EntityKind> = HashMap::new();
    duplicates(&cands, o, &mut gone, &mut rep);
    lines(&cands, o, &mut gone, &mut changed, &mut rep);
    arcs(&cands, o, &mut gone, &mut changed, &mut rep);
    if o.plines {
        for (i, c) in cands.iter().enumerate() {
            if gone.get(i).copied().unwrap_or(true) || c.protected {
                continue;
            }
            if let EntityKind::LwPolyline(pl) = &c.e.kind {
                let mut pl = pl.clone();
                let n = optimize_pline(&mut pl, o.q());
                if n > 0 {
                    rep.segments += n;
                    changed.insert(i, EntityKind::LwPolyline(pl));
                }
            }
        }
    }
    let d = s.doc_mut()?;
    for (i, c) in cands.iter().enumerate() {
        if gone.get(i).copied().unwrap_or(false) {
            d.remove_entity(c.e.handle);
        } else if let Some(k) = changed.remove(&i) {
            d.modify_entity(c.e.handle, |e| e.kind = k)?;
        }
    }
    Ok(rep)
}

/// Equal objects (numbers compared to the tolerance): the first in draw order stays, unless a
/// later one is followed by an associative object.
fn duplicates(cands: &[Cand], o: &Opts, gone: &mut [bool], rep: &mut Report) {
    let mut seen: HashMap<(u32, String), usize> = HashMap::new();
    for (i, c) in cands.iter().enumerate() {
        let Ok(mut v) = serde_json::to_value(&c.e.kind) else { continue };
        if o.tol > 0.0 {
            quantize(&mut v, o.q());
        }
        let key = (c.prop, v.to_string());
        match seen.get(&key).copied() {
            None => {
                seen.insert(key, i);
            }
            Some(k) => {
                let keeper_protected = cands.get(k).is_some_and(|x| x.protected);
                let victim = match (keeper_protected, c.protected) {
                    (_, false) => i,
                    (false, true) => {
                        seen.insert(key, i);
                        k
                    }
                    (true, true) => continue,
                };
                if let Some(g) = gone.get_mut(victim) {
                    *g = true;
                    rep.duplicates += 1;
                }
            }
        }
    }
}

/// A line as a direction-normalised record.
struct LineRec {
    i: usize,
    prop: u32,
    theta: f64,
    u: Vec2,
    a: Vec2,
    b: Vec2,
    z: f64,
    len: f64,
}

/// Lines on one infinite line: delete those lying on another, combine partial overlaps (and
/// end-to-end ones when asked).
fn lines(cands: &[Cand], o: &Opts, gone: &mut [bool], changed: &mut HashMap<usize, EntityKind>, rep: &mut Report) {
    let q = o.q();
    let mut recs: Vec<LineRec> = Vec::new();
    for (i, c) in cands.iter().enumerate() {
        let EntityKind::Line(l) = &c.e.kind else { continue };
        if gone.get(i).copied().unwrap_or(true) || c.protected || !l.a.is_finite() || !l.b.is_finite() || (l.a.z - l.b.z).abs() > q {
            continue;
        }
        let (a, b) = (l.a.xy(), l.b.xy());
        let len = a.dist(b);
        if len.is_nan() || len <= q {
            continue;
        }
        let mut u = (b - a) * (1.0 / len);
        let mut theta = u.angle();
        if theta < SEAM {
            theta += std::f64::consts::PI;
            u = u * -1.0;
        } else if theta >= SEAM + std::f64::consts::PI {
            theta -= std::f64::consts::PI;
            u = u * -1.0;
        }
        recs.push(LineRec { i, prop: c.prop, theta, u, a, b, z: l.a.z, len });
    }
    if recs.len() < 2 {
        return;
    }
    let min_len = recs.iter().map(|r| r.len).fold(f64::INFINITY, f64::min);
    let angtol = (2.0 * q / min_len).clamp(1e-12, 1e-2);
    recs.sort_by(|x, y| x.prop.cmp(&y.prop).then(x.theta.total_cmp(&y.theta)));
    // Runs of (about) the same direction…
    let mut start = 0;
    while start < recs.len() {
        let mut end = start + 1;
        while let (Some(p), Some(n)) = (recs.get(end - 1), recs.get(end)) {
            if n.prop != p.prop || n.theta - p.theta > angtol {
                break;
            }
            end += 1;
        }
        if let Some(run) = recs.get(start..end)
            && run.len() > 1
        {
            same_direction(run, cands, o, gone, changed, rep);
        }
        start = end;
    }
}

/// Lines of one direction run: split by offset, then merge each collinear set.
fn same_direction(run: &[LineRec], cands: &[Cand], o: &Opts, gone: &mut [bool], changed: &mut HashMap<usize, EntityKind>, rep: &mut Report) {
    let q = o.q();
    let Some(r0) = run.first() else { return };
    // Each line's own offset from a common origin: collinear lines agree even when the run is
    // wide in angle.
    let mut by_offset: Vec<(f64, &LineRec)> = run.iter().map(|r| (r.u.perp().dot(r.a - r0.a), r)).collect();
    by_offset.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut start = 0;
    while start < by_offset.len() {
        let mut end = start + 1;
        while let (Some(p), Some(n)) = (by_offset.get(end - 1), by_offset.get(end)) {
            if n.0 - p.0 > 2.0 * q {
                break;
            }
            end += 1;
        }
        if let Some(group) = by_offset.get(start..end)
            && group.len() > 1
        {
            let members: Vec<&LineRec> = group.iter().map(|g| g.1).collect();
            collinear(&members, cands, o, gone, changed, rep);
        }
        start = end;
    }
}

fn collinear(members: &[&LineRec], cands: &[Cand], o: &Opts, gone: &mut [bool], changed: &mut HashMap<usize, EntityKind>, rep: &mut Report) {
    let q = o.q();
    let Some(r) = members.iter().max_by(|x, y| x.len.total_cmp(&y.len)) else { return };
    let (origin, u, n) = (r.a, r.u, r.u.perp());
    // (t0, t1, index into cands)
    let mut iv: Vec<(f64, f64, usize)> = members
        .iter()
        .filter(|m| (n.dot(m.a - origin)).abs() <= q && (n.dot(m.b - origin)).abs() <= q && (m.z - r.z).abs() <= q)
        .map(|m| {
            let (ta, tb) = (u.dot(m.a - origin), u.dot(m.b - origin));
            (ta.min(tb), ta.max(tb), m.i)
        })
        .collect();
    if iv.len() < 2 {
        return;
    }
    iv.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.2.cmp(&y.2)));
    let mut it = iv.into_iter();
    let Some((mut k_t0, mut k_t1, mut keeper)) = it.next() else { return };
    let mut grown = false;
    let finish = |keeper: usize, t0: f64, t1: f64, grown: bool, changed: &mut HashMap<usize, EntityKind>| {
        if !grown {
            return;
        }
        let Some(c) = cands.get(keeper) else { return };
        let EntityKind::Line(l) = &c.e.kind else { return };
        let (lo, hi) = (origin + u * t0, origin + u * t1);
        let forward = u.dot(l.b.xy() - l.a.xy()) >= 0.0;
        let (a, b) = if forward { (lo, hi) } else { (hi, lo) };
        let mut l = l.clone();
        l.a = Vec3::new(a.x, a.y, l.a.z);
        l.b = Vec3::new(b.x, b.y, l.b.z);
        changed.insert(keeper, EntityKind::Line(l));
    };
    for (t0, t1, i) in it {
        let absorb = if t1 <= k_t1 + q {
            true
        } else if t0 < k_t1 - q {
            o.partial
        } else if t0 <= k_t1 + q {
            o.end_to_end
        } else {
            false
        };
        if absorb {
            if t1 > k_t1 {
                k_t1 = t1;
                grown = true;
            }
            if let Some(g) = gone.get_mut(i) {
                *g = true;
                rep.overlapping += 1;
            }
        } else {
            finish(keeper, k_t0, k_t1, grown, changed);
            (k_t0, k_t1, keeper, grown) = (t0, t1, i, false);
        }
    }
    finish(keeper, k_t0, k_t1, grown, changed);
}

/// Arcs and circles on the same circle: a circle covers every arc on it; arcs lying on another
/// are deleted, overlapping ones combined (and end-to-end ones when asked).
fn arcs(cands: &[Cand], o: &Opts, gone: &mut [bool], changed: &mut HashMap<usize, EntityKind>, rep: &mut Report) {
    let q = o.q();
    let g = q.max(1e-9);
    let cell = |x: f64| (x / g).round() as i64;
    let mut buckets: HashMap<(u32, i64, i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, c) in cands.iter().enumerate() {
        if gone.get(i).copied().unwrap_or(true) || c.protected {
            continue;
        }
        let (center, r) = match &c.e.kind {
            EntityKind::Arc(a) => (a.center, a.radius),
            EntityKind::Circle(c) => (c.center, c.radius),
            _ => continue,
        };
        if !center.is_finite()
            || !r.is_finite()
            || r <= q
            || (center.x / g).abs() > 1e15
            || (center.y / g).abs() > 1e15
            || (r / g) > 1e15
            || (center.z / g).abs() > 1e15
        {
            continue;
        }
        buckets.entry((c.prop, cell(center.x), cell(center.y), cell(r), cell(center.z))).or_default().push(i);
    }
    let mut keys: Vec<_> = buckets.keys().copied().collect();
    keys.sort_unstable();
    for key in keys {
        let Some(group) = buckets.get(&key) else { continue };
        if group.len() < 2 {
            continue;
        }
        let circle = group.iter().copied().find(|i| cands.get(*i).is_some_and(|c| matches!(c.e.kind, EntityKind::Circle(_))));
        if let Some(keep) = circle {
            for i in group.iter().copied().filter(|i| *i != keep) {
                if let Some(gn) = gone.get_mut(i) {
                    *gn = true;
                    if cands.get(i).is_some_and(|c| matches!(c.e.kind, EntityKind::Circle(_))) {
                        rep.duplicates += 1;
                    } else {
                        rep.overlapping += 1;
                    }
                }
            }
            continue;
        }
        same_circle(group, cands, o, gone, changed, rep);
    }
}

fn same_circle(group: &[usize], cands: &[Cand], o: &Opts, gone: &mut [bool], changed: &mut HashMap<usize, EntityKind>, rep: &mut Report) {
    // (start, end ≥ start, index); angles on [0, 2π) + sweep.
    let mut iv: Vec<(f64, f64, usize)> = Vec::new();
    let mut radius = 0.0;
    for &i in group {
        if let Some(EntityKind::Arc(a)) = cands.get(i).map(|c| &c.e.kind) {
            let s = norm_angle(a.start);
            iv.push((s, s + ccw_sweep(a.start, a.end), i));
            radius = a.radius;
        }
    }
    if iv.len() < 2 || radius.is_nan() || radius <= 0.0 {
        return;
    }
    let atol = (o.q() / radius).min(1.0);
    iv.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.2.cmp(&y.2)));
    // Merged runs: (start, end, keeper, grown).
    let mut runs: Vec<(f64, f64, usize, bool)> = Vec::new();
    let kill = |i: usize, gone: &mut [bool], rep: &mut Report| {
        if let Some(g) = gone.get_mut(i) {
            *g = true;
            rep.overlapping += 1;
        }
    };
    let absorbs = |end: f64, s: f64, e: f64| -> Option<bool> {
        // Some(grows) when the run ending at `end` takes in [s, e].
        if e <= end + atol {
            Some(false)
        } else if s < end - atol {
            o.partial.then_some(true)
        } else if s <= end + atol {
            o.end_to_end.then_some(true)
        } else {
            None
        }
    };
    for (s, e, i) in iv {
        match runs.last_mut() {
            Some(run) => match absorbs(run.1, s, e) {
                Some(grows) => {
                    if grows {
                        run.1 = e;
                        run.3 = true;
                    }
                    kill(i, gone, rep);
                }
                None => runs.push((s, e, i, false)),
            },
            None => runs.push((s, e, i, false)),
        }
    }
    // The last run may wrap past 2π onto the first.
    if runs.len() > 1
        && let (Some(first), Some(last)) = (runs.first().copied(), runs.last().copied())
        && let Some(grows) = absorbs(last.1 - TAU, first.0, first.1)
    {
        kill(first.2, gone, rep);
        if grows && let Some(l) = runs.last_mut() {
            l.1 = first.1 + TAU;
            l.3 = true;
        }
        runs.remove(0);
    }
    for (s, e, keeper, grown) in runs {
        if !grown {
            continue;
        }
        let Some(EntityKind::Arc(a)) = cands.get(keeper).map(|c| &c.e.kind) else { continue };
        if e - s >= TAU - atol {
            changed.insert(keeper, EntityKind::Circle(Circle { center: a.center, radius: a.radius }));
        } else {
            let mut a = a.clone();
            a.start = norm_angle(s);
            a.end = norm_angle(e);
            changed.insert(keeper, EntityKind::Arc(a));
        }
    }
}

/// Drop zero-length segments and join consecutive collinear straight segments of a polyline
/// (constant width only). Returns how many segments went.
fn optimize_pline(pl: &mut cadcraft_doc::LwPolyline, q: f64) -> usize {
    let before = pl.vertices.len();
    let min = if pl.closed { 3 } else { 2 };
    // Zero-length segments: the vertex after one takes over its successor's segment.
    let mut i = 0;
    while pl.vertices.len() > min && i + 1 < pl.vertices.len() {
        let (Some(a), Some(b)) = (pl.vertices.get(i).copied(), pl.vertices.get(i + 1).copied()) else { break };
        if a.p.dist(b.p) <= q {
            if let Some(v) = pl.vertices.get_mut(i) {
                v.bulge = b.bulge;
                v.end_width = b.end_width;
            }
            pl.vertices.remove(i + 1);
        } else {
            i += 1;
        }
    }
    if pl.closed
        && pl.vertices.len() > min
        && let (Some(f), Some(l)) = (pl.vertices.first(), pl.vertices.last())
        && f.p.dist(l.p) <= q
    {
        pl.vertices.pop();
    }
    // Collinear straight segments going the same way.
    let plain = |v: &cadcraft_geom::PolyVertex| v.bulge == 0.0 && v.start_width == 0.0 && v.end_width == 0.0;
    let mut i = 0;
    while pl.vertices.len() > min && i + 2 < pl.vertices.len() {
        let (Some(a), Some(b), Some(c)) = (pl.vertices.get(i).copied(), pl.vertices.get(i + 1).copied(), pl.vertices.get(i + 2).copied()) else {
            break;
        };
        let ac = c.p - a.p;
        let len = ac.len();
        let on_line = len > q && (ac.cross(b.p - a.p) / len).abs() <= q;
        let between = (b.p - a.p).dot(ac) > 0.0 && (c.p - b.p).dot(ac) > 0.0;
        if plain(&a) && plain(&b) && on_line && between {
            pl.vertices.remove(i + 1);
        } else {
            i += 1;
        }
    }
    before - pl.vertices.len()
}

fn report_message(r: &Report) -> String {
    let mut m = format!("{} duplicate(s) deleted\n{} overlapping object(s) or segment(s) deleted", r.duplicates, r.overlapping + r.segments);
    if r.segments > 0 {
        m.push_str(&format!(" ({} polyline segment(s))", r.segments));
    }
    m
}

fn run_overkill(s: &mut Session, p: &Value) -> Result<Value> {
    let o = Opts::from_json(p)?;
    let hs = if p.get("handles").is_some_and(|v| !v.is_null()) { Some(targets(s, p)?) } else { None };
    let r = overkill(s, hs, &o)?;
    Ok(json!({
        "deleted": r.duplicates + r.overlapping,
        "duplicates": r.duplicates,
        "overlapping": r.overlapping,
        "segments": r.segments,
        "message": report_message(&r),
    }))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Select,
    Options,
    Ignore,
    Tolerance,
    Plines,
    Partial,
    EndToEnd,
    Assoc,
}

/// OVERKILL typed: "Select objects", then the options until Done/Enter.
#[derive(Default)]
struct OverkillM {
    stage: Stage,
    sel: SelectPhase,
    objs: Vec<Handle>,
    opts: Opts,
}

fn yes_no(b: bool) -> &'static str {
    if b { "Yes" } else { "No" }
}

impl OverkillM {
    fn run(&mut self, s: &mut Session) -> Result<Step> {
        let objs = std::mem::take(&mut self.objs);
        let r = overkill(s, Some(objs), &self.opts)?;
        for line in report_message(&r).lines() {
            s.echo(line.to_string());
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for OverkillM {
    fn name(&self) -> &'static str {
        "OVERKILL"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.objs = self.sel.picked.clone();
            self.stage = Stage::Options;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        let o = &self.opts;
        match self.stage {
            Stage::Select => self.sel.prompt(),
            Stage::Options => Prompt::new("Enter an option to change", Accept::TEXT)
                .kw(&["Done", "Ignore", "tOlerance", "Plines", "parTial overlap", "Endtoend", "Associativity"])
                .default("Done"),
            Stage::Ignore => {
                let mut kws: Vec<&str> = vec!["None", "All"];
                kws.extend(IGNORABLE.iter().map(|(k, _)| *k));
                Prompt::new(format!("Property to ignore or compare again (ignored: {})", o.ignored_names()), Accept::TEXT).kw(&kws).default("Done")
            }
            Stage::Tolerance => Prompt::new("Specify tolerance", Accept::NUMBER).default(format!("{}", o.tol)),
            Stage::Plines => Prompt::new("Optimize segments within polylines", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(o.plines)),
            Stage::Partial => {
                Prompt::new("Combine collinear objects that partially overlap", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(o.partial))
            }
            Stage::EndToEnd => {
                Prompt::new("Combine collinear objects that meet end to end", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(o.end_to_end))
            }
            Stage::Assoc => Prompt::new("Keep associative objects", Accept::TEXT).kw(&["Yes", "No"]).default(yes_no(o.keep_assoc)),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let word = match &i {
            Input::Keyword(k) | Input::Text(k) => Some(k.trim().to_ascii_lowercase()),
            _ => None,
        };
        match self.stage {
            Stage::Select => match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.objs = hs;
                    self.stage = Stage::Options;
                    Ok(Step::Continue)
                }
            },
            Stage::Options => {
                self.stage = match (&i, word.as_deref()) {
                    (Input::Enter, _) | (_, Some("done")) => return self.run(s),
                    (_, Some("ignore")) => Stage::Ignore,
                    (_, Some("tolerance")) => Stage::Tolerance,
                    (_, Some("plines")) => Stage::Plines,
                    (_, Some("partial overlap")) => Stage::Partial,
                    (_, Some("endtoend")) => Stage::EndToEnd,
                    (_, Some("associativity")) => Stage::Assoc,
                    (_, Some(_)) => {
                        s.echo("Invalid option keyword.");
                        Stage::Options
                    }
                    _ => Stage::Options,
                };
                Ok(Step::Continue)
            }
            Stage::Ignore => {
                match (&i, word.as_deref()) {
                    (Input::Enter, _) | (_, Some("done")) => self.stage = Stage::Options,
                    (_, Some("none")) => self.opts.ignore = [false; 8],
                    (_, Some("all")) => self.opts.ignore = [true; 8],
                    (_, Some(w)) => match IGNORABLE.iter().position(|(k, n)| k.eq_ignore_ascii_case(w) || n.eq_ignore_ascii_case(w)) {
                        Some(ix) => {
                            if let Some(b) = self.opts.ignore.get_mut(ix) {
                                *b = !*b;
                            }
                        }
                        None => s.echo("Invalid option keyword."),
                    },
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Stage::Tolerance => {
                match &i {
                    Input::Enter => self.stage = Stage::Options,
                    Input::Text(t) => match number(t).ok_or_else(|| "Requires a number.".to_string()).and_then(check_tol) {
                        Ok(v) => {
                            self.opts.tol = v;
                            self.stage = Stage::Options;
                        }
                        Err(e) => s.echo(e),
                    },
                    _ => {}
                }
                Ok(Step::Continue)
            }
            Stage::Plines | Stage::Partial | Stage::EndToEnd | Stage::Assoc => {
                let v = match (&i, word.as_deref()) {
                    (Input::Enter, _) => None,
                    (_, Some("yes" | "y")) => Some(true),
                    (_, Some("no" | "n")) => Some(false),
                    _ => {
                        s.echo("Invalid option keyword.");
                        return Ok(Step::Continue);
                    }
                };
                if let Some(v) = v {
                    match self.stage {
                        Stage::Plines => self.opts.plines = v,
                        Stage::Partial => self.opts.partial = v,
                        Stage::EndToEnd => self.opts.end_to_end = v,
                        _ => self.opts.keep_assoc = v,
                    }
                }
                self.stage = Stage::Options;
                Ok(Step::Continue)
            }
        }
    }
}
