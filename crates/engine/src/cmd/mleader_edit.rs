//! MLEADEREDIT (add / remove leaders), MLEADERALIGN and MLEADERCOLLECT.
//!
//! The model has one landing per multileader: its leaders are polylines from an arrowhead to that
//! landing, so "add leader" pushes one more arrowhead polyline and "remove leader" drops one.

use cadcraft_doc::{Entity, EntityKind, Handle, Insert, MLeader};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::helpers::v3;
use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, point_req, points_param, str_param, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

/// Most arrowheads added by one call, and most multileaders aligned / collected by one call.
const MAX_ITEMS: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("mleaderedit", "Edit Multileader", run_edit)
            .alias(&["mledit"])
            .params("{handle, mode?: add|remove (default add), points?: [[x,y]...] (arrowheads to add; with remove, points picking the leaders to remove), index?: number|[number] (remove by position)}")
            .interactive(|_| Ok(Box::new(EditM::new(None)))),
        CommandSpec::new("mleaderedit.add", "Add Leader", |s, p| run_edit(s, &with_mode(p, "add")))
            .menu(&["Modify", "Object", "Multileader", "Add Leader"])
            .params("{handle, points: [[x,y]...] (arrowhead locations)}")
            .interactive(|_| Ok(Box::new(EditM::new(Some(false))))),
        CommandSpec::new("mleaderedit.remove", "Remove Leader", |s, p| run_edit(s, &with_mode(p, "remove")))
            .menu(&["Modify", "Object", "Multileader", "Remove Leader"])
            .params("{handle, points?: [[x,y]...] (pick the leaders), index?: number|[number]}")
            .interactive(|_| Ok(Box::new(EditM::new(Some(true))))),
        CommandSpec::new("mleaderalign", "Align Multileaders", run_align)
            .menu(&["Modify", "Object", "Multileader", "Align"])
            .alias(&["mleadera"])
            .params("{handles?, option?: current|distribute|parallel|spacing (default current), spacing?: number, to?: hex (multileader to align to), direction?: [[x,y],[x,y]] | angle?: degrees, points?: [[x,y],[x,y]] (distribute: first and last position)}")
            .interactive(|_| Ok(Box::new(AlignM::default()))),
        CommandSpec::new("mleadercollect", "Collect Multileaders", run_collect)
            .menu(&["Modify", "Object", "Multileader", "Collect"])
            .alias(&["mleaderc"])
            .params("{handles?, at: [x,y] (top left of the collected blocks), mode?: horizontal|vertical|wrap (default horizontal), width?: number (wrap), spacing?: number}")
            .interactive(|_| Ok(Box::new(CollectM::default()))),
    ]
}

fn with_mode(p: &Value, mode: &str) -> Value {
    let mut o = p.as_object().cloned().unwrap_or_default();
    o.insert("mode".into(), json!(mode));
    Value::Object(o)
}

fn handle_of(p: &Value, key: &str) -> Option<Handle> {
    p.get(key).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

fn get_mleader(s: &Session, h: Handle) -> Option<MLeader> {
    match s.doc().ok()?.entity(h).map(|e| &e.kind) {
        Some(EntityKind::MLeader(m)) => Some(m.clone()),
        _ => None,
    }
}

fn locked(s: &Session, h: Handle) -> bool {
    s.doc().ok().is_some_and(|d| d.entity(h).and_then(|e| d.layer(&e.common.layer)).is_some_and(|l| l.locked))
}

/// Keyword abbreviation test: "p" / "par" / "parallel" all match "parallel".
fn kw_is(input: &str, full: &str) -> bool {
    let t = input.trim().to_ascii_lowercase();
    !t.is_empty() && full.to_ascii_lowercase().starts_with(&t)
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).len()
}

/// The leader of `m` nearest `p`.
fn nearest_leader(m: &MLeader, p: Vec2) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, l) in m.leaders.iter().enumerate() {
        let pts: Vec<Vec2> = l.iter().map(|v| v.xy()).chain(std::iter::once(m.landing.xy())).collect();
        let d = pts.windows(2).filter_map(|w| Some(seg_dist(p, *w.first()?, *w.get(1)?))).fold(f64::INFINITY, f64::min);
        if d.is_finite() && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

// ---------------------------------------------------------------- MLEADEREDIT

fn add_leaders(s: &mut Session, h: Handle, pts: &[Vec2]) -> Result<()> {
    get_mleader(s, h).ok_or_else(|| bad("mleaderedit", "`handle` is not a multileader"))?;
    if locked(s, h) {
        return Err(bad("mleaderedit", "the multileader is on a locked layer"));
    }
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::MLeader(m) = &mut e.kind {
            m.leaders.extend(pts.iter().map(|p| vec![v3(*p)]));
        }
    })?;
    Ok(())
}

fn remove_leaders(s: &mut Session, h: Handle, idx: &[usize]) -> Result<()> {
    let m = get_mleader(s, h).ok_or_else(|| bad("mleaderedit", "`handle` is not a multileader"))?;
    if locked(s, h) {
        return Err(bad("mleaderedit", "the multileader is on a locked layer"));
    }
    if let Some(i) = idx.iter().find(|i| **i >= m.leaders.len()) {
        return Err(bad("mleaderedit", format!("no leader {i} (the multileader has {})", m.leaders.len())));
    }
    s.doc_mut()?.modify_entity(h, |e| {
        if let EntityKind::MLeader(m) = &mut e.kind {
            let mut sorted: Vec<usize> = idx.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            for i in sorted.into_iter().rev() {
                if i < m.leaders.len() {
                    m.leaders.remove(i);
                }
            }
        }
    })?;
    Ok(())
}

fn run_edit(s: &mut Session, p: &Value) -> Result<Value> {
    let h = match handle_of(p, "handle") {
        Some(h) => h,
        None => targets(s, p)?.into_iter().next().ok_or_else(|| bad("mleaderedit", "`handle` (a multileader) is required"))?,
    };
    let pts = match p.get("points") {
        None | Some(Value::Null) => Vec::new(),
        Some(_) => points_param(p, "points").ok_or_else(|| bad("mleaderedit", "`points` must be [[x,y]...]"))?,
    };
    if pts.len() > MAX_ITEMS {
        return Err(bad("mleaderedit", "too many points"));
    }
    match str_param(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("add") => {
            if pts.is_empty() {
                return Err(bad("mleaderedit", "`points` (arrowhead locations) is required"));
            }
            add_leaders(s, h, &pts)?;
            Ok(json!({ "added": pts.len() }))
        }
        Some("remove") => {
            let m = get_mleader(s, h).ok_or_else(|| bad("mleaderedit", "`handle` is not a multileader"))?;
            let mut idx: Vec<usize> = Vec::new();
            match p.get("index") {
                None | Some(Value::Null) => {}
                Some(Value::Array(a)) => {
                    for v in a {
                        idx.push(
                            v.as_u64()
                                .and_then(|n| usize::try_from(n).ok())
                                .ok_or_else(|| bad("mleaderedit", "`index` must be non-negative whole numbers"))?,
                        );
                    }
                }
                Some(v) => idx.push(
                    v.as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| bad("mleaderedit", "`index` must be a non-negative whole number"))?,
                ),
            }
            idx.extend(pts.iter().filter_map(|q| nearest_leader(&m, *q)));
            if idx.is_empty() {
                return Err(bad("mleaderedit", "`points` or `index` (the leaders to remove) is required"));
            }
            let n = {
                let mut u = idx.clone();
                u.sort_unstable();
                u.dedup();
                u.len()
            };
            remove_leaders(s, h, &idx)?;
            Ok(json!({ "removed": n }))
        }
        Some(_) => Err(bad("mleaderedit", "`mode` must be \"add\" or \"remove\"")),
    }
}

/// MLEADEREDIT at the command line: select a multileader, then arrowhead locations to add (or
/// leaders to remove by picking them), Enter to finish.
struct EditM {
    remove: bool,
    fixed: bool,
    target: Option<Handle>,
    sel: SelectPhase,
    changed: usize,
}

impl EditM {
    fn new(mode: Option<bool>) -> Self {
        EditM { remove: mode.unwrap_or(false), fixed: mode.is_some(), target: None, sel: SelectPhase::single(), changed: 0 }
    }
}

impl Interactive for EditM {
    fn name(&self) -> &'static str {
        "MLEADEREDIT"
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match (self.target, self.remove) {
            (None, _) => Prompt::new("Select a multileader", Accept::SELECT),
            (Some(_), false) if self.fixed => Prompt::new("Specify leader arrowhead location", Accept::POINT),
            (Some(_), false) => Prompt::new("Specify leader arrowhead location or [Remove leaders]", Accept::POINT).kw(&["Remove leaders"]),
            (Some(_), true) if self.fixed => Prompt::new("Specify leaders to remove", Accept::POINT),
            (Some(_), true) => Prompt::new("Specify leaders to remove or [Add leaders]", Accept::POINT).kw(&["Add leaders"]),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let Some(h) = self.target else {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    s.set_selection(Vec::new());
                    match hs.into_iter().find(|h| get_mleader(s, *h).is_some()) {
                        Some(h) => {
                            self.target = Some(h);
                            Ok(Step::Continue)
                        }
                        None => {
                            s.echo("Select a multileader.");
                            self.sel = SelectPhase::single();
                            Ok(Step::Continue)
                        }
                    }
                }
            };
        };
        match i {
            Input::Enter => {
                s.echo(format!("{} leader(s) {}.", self.changed, if self.remove { "removed" } else { "added" }));
                Ok(Step::Done)
            }
            Input::Keyword(k) | Input::Text(k) if !self.fixed && (kw_is(&k, "Remove leaders") || kw_is(&k, "Add leaders")) => {
                self.remove = kw_is(&k, "Remove leaders");
                Ok(Step::Continue)
            }
            Input::Point(pt) => {
                if self.remove {
                    let Some(m) = get_mleader(s, h) else { return Ok(Step::Cancel) };
                    match nearest_leader(&m, pt) {
                        Some(ix) => {
                            remove_leaders(s, h, &[ix])?;
                            self.changed += 1;
                        }
                        None => s.echo("No leader to remove."),
                    }
                } else {
                    add_leaders(s, h, &[pt])?;
                    self.changed += 1;
                }
                Ok(Step::Continue)
            }
            _ => Ok(Step::Continue),
        }
    }
}

// ---------------------------------------------------------------- shared geometry

/// Move a multileader's content (landing, text, block) by `d`; the arrowheads stay.
fn shift_content(m: &mut MLeader, d: Vec2) {
    let mv = |v: &mut Vec3| *v = Vec3::new(v.x + d.x, v.y + d.y, v.z);
    mv(&mut m.landing);
    if let Some(t) = &mut m.text {
        mv(&mut t.insert);
    }
    if let Some(b) = &mut m.block {
        mv(&mut b.insert);
    }
}

fn move_content(s: &mut Session, moves: &[(Handle, Vec2)]) -> Result<()> {
    let d = s.doc_mut()?;
    for (h, delta) in moves {
        if !delta.is_finite() {
            continue;
        }
        d.modify_entity(*h, |e| {
            if let EntityKind::MLeader(m) = &mut e.kind {
                shift_content(m, *delta);
            }
        })?;
    }
    Ok(())
}

// ---------------------------------------------------------------- MLEADERALIGN

#[derive(Clone, Copy, Debug, PartialEq)]
enum Spacing {
    Current,
    Distribute,
    Parallel,
    Value(f64),
}

/// The direction of the last leader segment (arrowhead side to landing).
fn segment_dir(m: &MLeader) -> Option<Vec2> {
    let last = m.leaders.first()?.last()?;
    let v = m.landing.xy() - last.xy();
    (v.len() > 1e-12).then(|| v.normalized())
}

fn align_parallel(items: &[(Handle, MLeader)], reference: Option<&MLeader>) -> Result<Vec<(Handle, Vec2)>> {
    let rdir = reference.and_then(segment_dir).ok_or_else(|| bad("mleaderalign", "the reference multileader has no leader"))?;
    let mut moves = Vec::new();
    for (h, m) in items {
        let Some(arrow) = m.leaders.first().and_then(|l| l.last()) else { continue };
        let len = (m.landing.xy() - arrow.xy()).len();
        moves.push((*h, arrow.xy() + rdir * len - m.landing.xy()));
    }
    Ok(moves)
}

fn align_distribute(mut items: Vec<(Handle, MLeader)>, line: Option<(Vec2, Vec2)>) -> Result<Vec<(Handle, Vec2)>> {
    let (a, b) = line.ok_or_else(|| bad("mleaderalign", "`points` (first and last position) are required"))?;
    let d = b - a;
    if d.len() < 1e-12 {
        return Err(bad("mleaderalign", "the two points coincide"));
    }
    let u = d.normalized();
    items.sort_by(|x, y| (x.1.landing.xy() - a).dot(u).total_cmp(&(y.1.landing.xy() - a).dot(u)));
    let n = items.len();
    let mut moves = Vec::new();
    for (i, (h, m)) in items.iter().enumerate() {
        let f = if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 };
        moves.push((*h, a + d * f - m.landing.xy()));
    }
    Ok(moves)
}

fn align_directional(
    items: &[(Handle, MLeader)],
    reference: Option<&MLeader>,
    line: Option<(Vec2, Vec2)>,
    spacing: Spacing,
) -> Result<Vec<(Handle, Vec2)>> {
    let (a, b) = line.ok_or_else(|| bad("mleaderalign", "`direction` (two points or an angle) is required"))?;
    let d = b - a;
    if d.len() < 1e-12 {
        return Err(bad("mleaderalign", "the direction is zero"));
    }
    let u = d.normalized();
    let origin = reference.map(|r| r.landing.xy()).unwrap_or(a);
    let t_of = |m: &MLeader| (m.landing.xy() - origin).dot(u);
    let mut moves = Vec::new();
    if let Spacing::Value(sp) = spacing {
        let (mut pos, mut neg): (Vec<_>, Vec<_>) = items.iter().partition(|(_, m)| t_of(m) >= 0.0);
        pos.sort_by(|x, y| t_of(&x.1).total_cmp(&t_of(&y.1)));
        neg.sort_by(|x, y| t_of(&y.1).total_cmp(&t_of(&x.1)));
        for (k, (h, m)) in pos.iter().enumerate() {
            moves.push((*h, origin + u * (sp * (k + 1) as f64) - m.landing.xy()));
        }
        for (k, (h, m)) in neg.iter().enumerate() {
            moves.push((*h, origin - u * (sp * (k + 1) as f64) - m.landing.xy()));
        }
    } else {
        for (h, m) in items {
            moves.push((*h, origin + u * t_of(m) - m.landing.xy()));
        }
    }
    Ok(moves)
}

fn align(s: &mut Session, hs: &[Handle], to: Option<Handle>, spacing: Spacing, line: Option<(Vec2, Vec2)>) -> Result<usize> {
    let mut items: Vec<(Handle, MLeader)> = Vec::new();
    for h in hs {
        if Some(*h) == to || items.iter().any(|(x, _)| x == h) || locked(s, *h) {
            continue;
        }
        if let Some(m) = get_mleader(s, *h) {
            items.push((*h, m));
        }
    }
    if items.is_empty() {
        return Err(bad("mleaderalign", "no multileaders to align"));
    }
    if items.len() > MAX_ITEMS {
        return Err(bad("mleaderalign", "too many multileaders"));
    }
    let reference = match (spacing, to) {
        (Spacing::Distribute, _) => None,
        (_, Some(t)) => Some(get_mleader(s, t).ok_or_else(|| bad("mleaderalign", "`to` is not a multileader"))?),
        (_, None) => return Err(bad("mleaderalign", "`to` (the multileader to align to) is required")),
    };
    let moves = match spacing {
        Spacing::Parallel => align_parallel(&items, reference.as_ref())?,
        Spacing::Distribute => align_distribute(items, line)?,
        Spacing::Current | Spacing::Value(_) => align_directional(&items, reference.as_ref(), line, spacing)?,
    };
    move_content(s, &moves)?;
    Ok(moves.len())
}

fn parse_line(p: &Value, cmd: &str) -> Result<Option<(Vec2, Vec2)>> {
    if let Some(a) = p.get("angle").and_then(Value::as_f64) {
        if !a.is_finite() {
            return Err(bad(cmd, "`angle` must be a finite number"));
        }
        return Ok(Some((Vec2::new(0.0, 0.0), Vec2::from_angle(a.to_radians()))));
    }
    for key in ["direction", "points"] {
        if p.get(key).is_some_and(|v| !v.is_null()) {
            let pts = points_param(p, key).ok_or_else(|| bad(cmd, format!("`{key}` must be [[x,y],[x,y]]")))?;
            if let (Some(a), Some(b)) = (pts.first(), pts.get(1)) {
                return Ok(Some((*a, *b)));
            }
            return Err(bad(cmd, format!("`{key}` needs two points")));
        }
    }
    Ok(None)
}

fn run_align(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let to = match p.get("to") {
        None | Some(Value::Null) => None,
        Some(_) => Some(handle_of(p, "to").ok_or_else(|| bad("mleaderalign", "`to` must be an object handle"))?),
    };
    let spacing = match (str_param(p, "option").map(str::to_ascii_lowercase).as_deref(), p.get("spacing")) {
        (_, Some(v)) if !v.is_null() => match v.as_f64().filter(|x| x.is_finite() && *x >= 0.0) {
            Some(x) => Spacing::Value(x),
            None => return Err(bad("mleaderalign", "`spacing` must be a non-negative number")),
        },
        (None | Some("current"), _) => Spacing::Current,
        (Some("distribute"), _) => Spacing::Distribute,
        (Some("parallel"), _) => Spacing::Parallel,
        (Some("spacing"), _) => return Err(bad("mleaderalign", "`spacing` (a number) is required")),
        (Some(_), _) => return Err(bad("mleaderalign", "`option` must be current, distribute, parallel or spacing")),
    };
    let line = parse_line(p, "mleaderalign")?;
    let n = align(s, &hs, to, spacing, line)?;
    Ok(json!({ "aligned": n }))
}

/// MLEADERALIGN at the command line: select, Options (Distribute / make segments Parallel /
/// Specify spacing / Use current spacing), the multileader to align to, then the direction.
#[derive(Default)]
struct AlignM {
    sel: SelectPhase,
    picked: Vec<Handle>,
    selecting: bool,
    spacing: Option<Spacing>,
    asking_spacing: bool,
    to: Option<Handle>,
    first: Option<Vec2>,
    pick_to: SelectPhase,
}

impl AlignM {
    fn mode(&self) -> Spacing {
        self.spacing.unwrap_or(Spacing::Current)
    }
    fn finish(&self, s: &mut Session, line: Option<(Vec2, Vec2)>) -> Result<Step> {
        let r = align(s, &self.picked, self.to, self.mode(), line)?;
        s.set_selection(Vec::new());
        s.echo(format!("{r} multileader(s) aligned."));
        Ok(Step::Done)
    }
    fn option(&mut self, s: &mut Session, k: &str) -> bool {
        if kw_is(k, "Distribute") {
            self.spacing = Some(Spacing::Distribute);
        } else if kw_is(k, "make segments Parallel") || kw_is(k, "Parallel") {
            self.spacing = Some(Spacing::Parallel);
        } else if kw_is(k, "Specify spacing") {
            self.asking_spacing = true;
        } else if kw_is(k, "Use current spacing") {
            self.spacing = Some(Spacing::Current);
        } else {
            return false;
        }
        let _ = s;
        true
    }
}

impl Interactive for AlignM {
    fn name(&self) -> &'static str {
        "MLEADERALIGN"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        self.pick_to = SelectPhase::single();
        if self.sel.done {
            self.picked = self.sel.picked.clone();
            self.selecting = true;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        const OPTS: &[&str] = &["Options"];
        if !self.selecting {
            return self.sel.prompt();
        }
        if self.asking_spacing {
            return Prompt::new("Specify spacing", Accept::NUMBER);
        }
        match (self.mode(), self.to, self.first) {
            (Spacing::Distribute, _, None) => Prompt::new("Specify first point or [Options]", Accept::POINT).kw(OPTS),
            (Spacing::Distribute, _, Some(a)) => {
                let mut p = Prompt::new("Specify second point", Accept::POINT);
                p.base = Some(a);
                p
            }
            (_, None, _) => Prompt::new("Select multileader to align to or [Options]", Accept::SELECT).kw(OPTS),
            (Spacing::Parallel, Some(_), _) => Prompt::new("Specify direction", Accept::POINT),
            (_, Some(_), None) => Prompt::new("Specify direction (first point)", Accept::POINT),
            (_, Some(_), Some(a)) => {
                let mut p = Prompt::new("Specify direction (second point)", Accept::POINT);
                p.base = Some(a);
                p
            }
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.selecting {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = hs;
                    self.selecting = true;
                    Ok(Step::Continue)
                }
            };
        }
        if self.asking_spacing {
            return match &i {
                Input::Text(t) => match super::machines::number(t).filter(|v| v.is_finite() && *v >= 0.0) {
                    Some(v) => {
                        self.spacing = Some(Spacing::Value(v));
                        self.asking_spacing = false;
                        Ok(Step::Continue)
                    }
                    None => {
                        s.echo("Requires a non-negative number.");
                        Ok(Step::Continue)
                    }
                },
                _ => Ok(Step::Continue),
            };
        }
        if let Input::Keyword(k) | Input::Text(k) = &i {
            if kw_is(k, "Options") {
                s.echo("Enter an option [Distribute/make segments Parallel/Specify spacing/Use current spacing]: type D, P, S or U.");
                return Ok(Step::Continue);
            }
            let k = k.clone();
            if self.option(s, &k) {
                return Ok(Step::Continue);
            }
        }
        let distribute = self.mode() == Spacing::Distribute;
        if !distribute && self.to.is_none() {
            if let SelOutcome::Done(hs) = self.pick_to.feed(s, &i)? {
                s.set_selection(Vec::new());
                match hs.into_iter().find(|h| get_mleader(s, *h).is_some()) {
                    Some(h) => self.to = Some(h),
                    None => {
                        s.echo("Select a multileader.");
                        self.pick_to = SelectPhase::single();
                    }
                }
            }
            return Ok(Step::Continue);
        }
        let Input::Point(pt) = i else { return Ok(Step::Continue) };
        if self.mode() == Spacing::Parallel {
            return self.finish(s, None);
        }
        match self.first.take() {
            None => {
                self.first = Some(pt);
                Ok(Step::Continue)
            }
            Some(a) => self.finish(s, Some((a, pt))),
        }
    }
}

// ---------------------------------------------------------------- MLEADERCOLLECT

#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    Horizontal,
    Vertical,
    Wrap(f64),
}

/// Size and centre of a block reference's extents (the insertion point when it has none).
fn block_box(s: &Session, ins: &Insert) -> Result<(Vec2, Vec2)> {
    let d = s.doc()?;
    let probe = Entity { handle: Handle(0), common: Default::default(), kind: EntityKind::Insert(ins.clone()) };
    let b = cadcraft_doc::entity_bounds(d, &probe, 0);
    if b.is_empty() || !b.center().is_finite() {
        return Ok((Vec2::new(0.0, 0.0), ins.insert.xy()));
    }
    Ok((b.max - b.min, b.center()))
}

fn collect(s: &mut Session, hs: &[Handle], at: Vec2, layout: Layout, spacing: Option<f64>) -> Result<Value> {
    let mut items: Vec<(Handle, MLeader)> = Vec::new();
    for h in hs {
        if items.iter().any(|(x, _)| x == h) || locked(s, *h) {
            continue;
        }
        if let Some(m) = get_mleader(s, *h).filter(|m| m.block.is_some()) {
            items.push((*h, m));
        }
    }
    if items.len() > MAX_ITEMS {
        return Err(bad("mleadercollect", "too many multileaders"));
    }
    if items.len() < 2 {
        return Err(bad("mleadercollect", "need two or more multileaders with block content"));
    }
    let gap = spacing.unwrap_or_else(|| {
        let d = s.doc().ok();
        d.and_then(|d| d.mleader_styles.iter().find(|m| m.name.eq_ignore_ascii_case(&items[0].1.style)).map(|m| m.landing_gap)).unwrap_or(0.09)
    });
    // Lay the blocks out from the top left corner at `at`.
    let (mut x, mut y, mut row_h) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut placed: Vec<Insert> = Vec::new();
    for (_, m) in &items {
        let Some(b) = &m.block else { continue };
        let (size, centre) = block_box(s, b)?;
        match layout {
            Layout::Horizontal => {}
            Layout::Vertical => {
                if x > 0.0 || row_h > 0.0 {
                    y -= row_h + gap;
                    x = 0.0;
                    row_h = 0.0;
                }
            }
            Layout::Wrap(w) => {
                if x > 0.0 && x + size.x > w {
                    y -= row_h + gap;
                    x = 0.0;
                    row_h = 0.0;
                }
            }
        }
        let target = Vec2::new(at.x + x + size.x / 2.0, at.y + y - size.y / 2.0);
        let mut ins = b.clone();
        ins.insert = Vec3::new(b.insert.x + target.x - centre.x, b.insert.y + target.y - centre.y, b.insert.z);
        placed.push(ins);
        x += size.x + gap;
        row_h = row_h.max(size.y);
    }
    let (first_h, first) = items[0].clone();
    let mut merged = first;
    merged.leaders = items.iter().flat_map(|(_, m)| m.leaders.iter().cloned()).collect();
    merged.landing = Vec3::new(at.x, at.y, merged.landing.z);
    merged.dogleg = 0.0;
    merged.text = None;
    let mut rest = placed.split_off(1.min(placed.len()));
    merged.block = placed.pop();
    let common = s.doc()?.entity(first_h).map(|e| e.common.clone());
    {
        let d = s.doc_mut()?;
        d.modify_entity(first_h, |e| e.kind = EntityKind::MLeader(merged))?;
        for (h, _) in items.iter().skip(1) {
            d.remove_entity(*h);
        }
    }
    // The other blocks stay block references next to the first (the model has one block per multileader).
    let mut extra = Vec::new();
    for ins in rest.drain(..) {
        let h = s.add_entity(EntityKind::Insert(ins))?;
        if let Some(c) = &common {
            s.doc_mut()?.modify_entity(h, |e| e.common = c.clone())?;
        }
        extra.push(h.hex());
    }
    Ok(json!({ "handle": first_h.hex(), "blocks": extra, "collected": items.len() }))
}

fn run_collect(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let at = point_req("mleadercollect", p, "at")?;
    let spacing = match p.get("spacing") {
        None | Some(Value::Null) => None,
        Some(v) => {
            Some(v.as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| bad("mleadercollect", "`spacing` must be a non-negative number"))?)
        }
    };
    let layout = match str_param(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("horizontal") => Layout::Horizontal,
        Some("vertical") => Layout::Vertical,
        Some("wrap") => {
            let w = p.get("width").and_then(Value::as_f64).filter(|w| w.is_finite() && *w > 0.0);
            Layout::Wrap(w.ok_or_else(|| bad("mleadercollect", "`width` (a positive number) is required for wrap"))?)
        }
        Some(_) => return Err(bad("mleadercollect", "`mode` must be horizontal, vertical or wrap")),
    };
    collect(s, &hs, at, layout, spacing)
}

/// MLEADERCOLLECT at the command line: select multileaders, then the location (Vertical /
/// Horizontal / Wrap to change the arrangement; Wrap asks for the width).
struct CollectM {
    sel: SelectPhase,
    picked: Vec<Handle>,
    selecting: bool,
    layout: Layout,
    asking_width: bool,
}

impl Default for CollectM {
    fn default() -> Self {
        CollectM { sel: SelectPhase::default(), picked: Vec::new(), selecting: false, layout: Layout::Horizontal, asking_width: false }
    }
}

impl Interactive for CollectM {
    fn name(&self) -> &'static str {
        "MLEADERCOLLECT"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.picked = self.sel.picked.clone();
            self.selecting = true;
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.selecting {
            return self.sel.prompt();
        }
        if self.asking_width {
            return Prompt::new("Specify width", Accept::NUMBER);
        }
        Prompt::new("Specify collected multileader location or [Vertical/Horizontal/Wrap]", Accept::POINT)
            .kw(&["Vertical", "Horizontal", "Wrap"])
            .default("Horizontal")
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.selecting {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.picked = hs;
                    self.selecting = true;
                    Ok(Step::Continue)
                }
            };
        }
        if self.asking_width {
            if let Input::Text(t) = &i {
                match super::machines::number(t).filter(|v| v.is_finite() && *v > 0.0) {
                    Some(w) => {
                        self.layout = Layout::Wrap(w);
                        self.asking_width = false;
                    }
                    None => s.echo("Requires a positive number."),
                }
            }
            return Ok(Step::Continue);
        }
        match i {
            Input::Keyword(k) | Input::Text(k) => {
                if kw_is(&k, "Vertical") {
                    self.layout = Layout::Vertical;
                } else if kw_is(&k, "Horizontal") {
                    self.layout = Layout::Horizontal;
                } else if kw_is(&k, "Wrap") {
                    self.asking_width = true;
                }
                Ok(Step::Continue)
            }
            Input::Point(at) => {
                let r = collect(s, &self.picked, at, self.layout, None)?;
                s.set_selection(Vec::new());
                s.echo(format!("{} multileader(s) collected.", r.get("collected").and_then(Value::as_u64).unwrap_or(0)));
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
}

#[cfg(test)]
#[path = "mleader_edit_tests.rs"]
mod tests;
