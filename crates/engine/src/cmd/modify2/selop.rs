use super::{
    EntityKind, Handle, Input, Interactive, LwPolyline, Mat3, PolyVertex, Prompt, Result, SelOutcome, SelectPhase, Session, Space, Step, Value,
    curves, json, lwpoly, other, set_kind, str_param, targets,
};

// =====================================================================================
// REVERSE / CHSPACE / FLATTEN (select, then act)
// =====================================================================================

pub(crate) fn reverse_kind(k: &EntityKind) -> Option<EntityKind> {
    Some(match k {
        EntityKind::Line(l) => EntityKind::Line(cadcraft_doc::Line { a: l.b, b: l.a }),
        EntityKind::LwPolyline(p) => EntityKind::LwPolyline(LwPolyline { vertices: curves::reverse_vertices(&p.vertices, p.closed), ..p.clone() }),
        EntityKind::Polyline3d(p) => {
            EntityKind::Polyline3d(cadcraft_doc::Polyline3d { points: p.points.iter().rev().copied().collect(), closed: p.closed })
        }
        EntityKind::Spline(sp) => EntityKind::Spline(curves::reverse_spline(sp)),
        _ => return None,
    })
}

fn reverse(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let mut n = 0;
    for h in hs {
        if curves::is_locked(s, *h) {
            continue;
        }
        let Ok(e) = curves::entity(s, *h) else { continue };
        if let Some(k) = reverse_kind(&e.kind) {
            set_kind(s, *h, k)?;
            n += 1;
        }
    }
    Ok(n)
}

pub(super) fn run_reverse(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = reverse(s, &hs)?;
    Ok(json!({ "reversed": n }))
}

/// Zero every z coordinate; 3D polylines become 2D polylines.
pub(crate) fn flatten_kind(k: &mut EntityKind) {
    let z = |p: &mut cadcraft_geom::Vec3| p.z = 0.0;
    match k {
        EntityKind::Line(l) => {
            z(&mut l.a);
            z(&mut l.b);
        }
        EntityKind::Point(p) => z(&mut p.p),
        EntityKind::Circle(c) => z(&mut c.center),
        EntityKind::Arc(a) => z(&mut a.center),
        EntityKind::Ellipse(e) => {
            z(&mut e.center);
            z(&mut e.major);
        }
        EntityKind::Helix(h) => *k = EntityKind::Spline(h.spline()),
        EntityKind::LwPolyline(p) => p.elevation = 0.0,
        EntityKind::Polyline3d(p) => {
            *k = lwpoly(p.points.iter().map(|q| PolyVertex::new(q.xy())).collect(), p.closed);
        }
        EntityKind::Ray(r) | EntityKind::XLine(r) => {
            z(&mut r.base);
            r.dir.z = 0.0;
            r.dir = r.dir.normalized();
        }
        EntityKind::Text(t) => {
            z(&mut t.insert);
            t.align_pt.iter_mut().for_each(z);
        }
        EntityKind::AttDef(a) => {
            z(&mut a.text.insert);
            a.text.align_pt.iter_mut().for_each(z);
            if let Some(ml) = &mut a.multiline {
                z(&mut ml.mtext.insert);
            }
        }
        EntityKind::MText(t) => z(&mut t.insert),
        EntityKind::Insert(i) => {
            z(&mut i.insert);
            for a in &mut i.attribs {
                z(&mut a.text.insert);
                a.text.align_pt.iter_mut().for_each(z);
                if let Some(ml) = &mut a.multiline {
                    z(&mut ml.mtext.insert);
                }
            }
        }
        EntityKind::Dimension(d) => {
            for p in [&mut d.defpt, &mut d.text_mid, &mut d.p13, &mut d.p14, &mut d.p15, &mut d.p16] {
                z(p);
            }
            d.block = None;
        }
        EntityKind::Leader(l) => l.vertices.iter_mut().for_each(z),
        EntityKind::MLine(m) => m.vertices.iter_mut().for_each(|v| z(&mut v.p)),
        EntityKind::MLeader(m) => {
            m.leaders.iter_mut().for_each(|l| l.iter_mut().for_each(z));
            z(&mut m.landing);
            m.block.iter_mut().for_each(|b| z(&mut b.insert));
        }
        EntityKind::Hatch(h) => h.elevation = 0.0,
        EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(z),
        EntityKind::Face3d(f) => f.corners.iter_mut().for_each(z),
        EntityKind::Region(_) => {}
        EntityKind::Image(i) => {
            z(&mut i.insert);
            i.u.z = 0.0;
            i.v.z = 0.0;
        }
        EntityKind::Table(t) => z(&mut t.insert),
        EntityKind::Tolerance(t) => {
            z(&mut t.insert);
            t.dir.z = 0.0;
        }
        EntityKind::Spline(_) | EntityKind::Viewport(_) | EntityKind::Wipeout(_) | EntityKind::Unknown(_) => {}
    }
}

fn flatten(s: &mut Session, hs: &[Handle]) -> Result<usize> {
    let mut n = 0;
    for h in hs {
        if curves::is_locked(s, *h) {
            continue;
        }
        if s.doc()?.entity(*h).is_none() {
            continue;
        }
        s.doc_mut()?.modify_entity(*h, |e| {
            flatten_kind(&mut e.kind);
            e.common.thickness = 0.0;
        })?;
        n += 1;
    }
    Ok(n)
}

pub(super) fn run_flatten(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let n = flatten(s, &hs)?;
    Ok(json!({ "flattened": n }))
}

/// Model↔paper transform through a viewport: paper = center + (model − viewCenter) / scale.
fn viewport_of(s: &Session, layout: &str, vp: Option<Handle>) -> Option<cadcraft_doc::Viewport> {
    let d = s.doc().ok()?;
    if let Some(h) = vp
        && let Some(e) = d.entity(h)
        && let EntityKind::Viewport(v) = &e.kind
    {
        return Some(v.clone());
    }
    let st = d.space(&Space::Paper(layout.to_string()))?;
    st.iter().find_map(|e| match &e.kind {
        EntityKind::Viewport(v) if v.height > 1e-12 && v.view_height > 1e-12 && v.id != 1 => Some(v.clone()),
        _ => None,
    })
}

fn chspace(s: &mut Session, hs: &[Handle], to: Option<&str>, vp: Option<Handle>) -> Result<(usize, Space, Space)> {
    let from = s.space();
    let (target, layout) = match (&from, to) {
        (Space::Model, t) => {
            let name = match t {
                Some(n) if !n.eq_ignore_ascii_case("model") => n.to_string(),
                _ => {
                    let d = s.doc()?;
                    let mut ls: Vec<_> = d.layouts.iter().collect();
                    ls.sort_by_key(|l| l.tab_order);
                    ls.first().map(|l| l.name.clone()).ok_or_else(|| other("No layout to move objects to."))?
                }
            };
            if s.doc()?.layout(&name).is_none() {
                return Err(other(format!("Layout \"{name}\" not found.")));
            }
            let real = s.doc()?.layout(&name).map(|l| l.name.clone()).unwrap_or(name);
            (Space::Paper(real.clone()), real)
        }
        (Space::Paper(n), _) => (Space::Model, n.clone()),
    };
    let m = match viewport_of(s, &layout, vp) {
        Some(v) => {
            let k = v.view_height / v.height;
            let to_paper = Mat3::translate(v.center.xy()).then_before(Mat3::scale(1.0 / k, 1.0 / k)).then_before(Mat3::translate(-v.view_center));
            if target == Space::Model { to_paper.inverse().unwrap_or(Mat3::IDENTITY) } else { to_paper }
        }
        None => Mat3::IDENTITY,
    };
    let mut n = 0;
    for h in hs {
        if curves::is_locked(s, *h) {
            continue;
        }
        let d = s.doc_mut()?;
        if d.space_of(*h).as_ref() != Some(&from) {
            continue;
        }
        let Some(e) = d.entity(*h).map(|e| (**e).clone()) else { continue };
        if matches!(e.kind, EntityKind::Viewport(_)) {
            continue;
        }
        let mut e = e;
        e.kind.transform(&m);
        d.remove_entity(*h);
        if let Some(st) = d.space_mut(&target) {
            st.push(e);
            n += 1;
        }
    }
    s.set_selection(Vec::new());
    Ok((n, from, target))
}

pub(super) fn run_chspace(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let (n, _, target) = chspace(s, &hs, str_param(p, "to"), curves::handle_param(p, "viewport"))?;
    Ok(json!({ "moved": n, "to": target.layout().unwrap_or("model") }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelKind {
    Reverse,
    Chspace,
    Flatten,
}

pub(super) struct SelOp {
    kind: SelKind,
    sel: SelectPhase,
}

impl SelOp {
    pub(super) fn new(kind: SelKind) -> Self {
        SelOp { kind, sel: SelectPhase::default() }
    }
    fn act(&self, s: &mut Session, hs: &[Handle]) -> Result<Step> {
        match self.kind {
            SelKind::Reverse => {
                let n = reverse(s, hs)?;
                s.echo(format!("{n} object(s) direction has been reversed."));
            }
            SelKind::Flatten => {
                let n = flatten(s, hs)?;
                s.echo(format!("{n} object(s) flattened."));
            }
            SelKind::Chspace => {
                let (n, from, to) = chspace(s, hs, None, None)?;
                let nm = |sp: &Space| if *sp == Space::Model { "MODEL" } else { "PAPER" };
                s.echo(format!("{n} object(s) changed from {} space to {} space.", nm(&from), nm(&to)));
            }
        }
        s.set_selection(Vec::new());
        Ok(Step::Done)
    }
}

impl Interactive for SelOp {
    fn name(&self) -> &'static str {
        match self.kind {
            SelKind::Reverse => "REVERSE",
            SelKind::Chspace => "CHSPACE",
            SelKind::Flatten => "FLATTEN",
        }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.act(s, &hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        self.sel.prompt()
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.sel.feed(s, &i)? {
            SelOutcome::More => Ok(Step::Continue),
            SelOutcome::Empty => Ok(Step::Cancel),
            SelOutcome::Done(hs) => self.act(s, &hs),
        }
    }
}
