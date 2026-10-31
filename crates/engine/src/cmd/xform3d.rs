//! MOVE3D and ROTATE3D: move and turn objects in 3D. The 2D MOVE and ROTATE leave Z alone; these
//! carry it. 3D faces, 3D polylines, lines and points move and turn freely. Objects that live on
//! a plane of constant Z (circles, arcs, 2D polylines, text, hatches…) can be moved sideways and
//! turned about a vertical axis, which keeps them in their plane; anything else is refused
//! before the drawing is touched.

use cadcraft_doc::{EntityKind, Handle};
use cadcraft_geom::{Mat3, Mat4, Vec3, brep::Body};
use serde_json::{Value, json};

use super::machines::{self, SelOutcome, SelectPhase};
use super::{CommandSpec, bad, f64_req, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("move3d", "3D Move", run_move3d)
            .menu(&["Modify", "3D Operations", "3D Move"])
            .alias(&["3dmove"])
            .params("{handles?, offset: [dx,dy,dz] | from: [x,y,z], to: [x,y,z]} → {moved, skipped}: moves 3D faces, 3D polylines, lines and points in 3D; other objects only sideways (dz = 0)")
            .interactive(|_| Ok(Box::new(Xform3dM::new(Kind::Move)))),
        CommandSpec::new("rotate3d", "3D Rotate", run_rotate3d)
            .menu(&["Modify", "3D Operations", "3D Rotate"])
            .alias(&["3drotate"])
            .params("{handles?, angle: degrees (counter-clockwise looking against the axis), axis?: \"x\"|\"y\"|\"z\" (default z) through `base`, or axis: [[x,y,z],[x,y,z]], base?: [x,y,z]} → {rotated, skipped}: turns 3D faces, 3D polylines, lines and points about any axis; other objects only about a vertical axis")
            .interactive(|_| Ok(Box::new(Xform3dM::new(Kind::Rotate)))),
    ]
}

fn vec3(v: &Value) -> Option<Vec3> {
    let a = v.as_array().filter(|a| (2..=3).contains(&a.len()))?;
    let n = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
    Some(Vec3::new(n(0)?, n(1)?, if a.len() == 3 { n(2)? } else { 0.0 }))
}

fn point(cmd: &str, p: &Value, key: &str) -> Result<Option<Vec3>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => vec3(v).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be [x, y] or [x, y, z] of finite numbers"))),
    }
}

/// Whether the object has 3D geometry this module can move and turn freely.
fn is_spatial(k: &EntityKind) -> bool {
    matches!(k, EntityKind::Face3d(_) | EntityKind::Polyline3d(_) | EntityKind::Line(_) | EntityKind::Point(_))
}

/// Apply `f` to every point of a spatial object.
fn map_points(k: &mut EntityKind, f: &dyn Fn(Vec3) -> Vec3) {
    match k {
        EntityKind::Face3d(x) => x.corners.iter_mut().for_each(|c| *c = f(*c)),
        EntityKind::Polyline3d(x) => x.points.iter_mut().for_each(|c| *c = f(*c)),
        EntityKind::Line(x) => {
            x.a = f(x.a);
            x.b = f(x.b);
        }
        EntityKind::Point(x) => x.p = f(x.p),
        _ => {}
    }
}

/// How an object is carried along: through its 3D points, or (for objects in a Z plane) through a
/// plane transform that keeps Z.
enum Plan {
    Points,
    Plane(Mat3),
}

/// Apply the transform to the objects `hs`: `point` moves a 3D point; `plane` is the equivalent
/// sideways transform for objects in a plane, when there is one. Locked layers are skipped.
/// Returns (changed, skipped). Nothing is touched when some object cannot be carried.
pub(super) fn affine_of(point: &dyn Fn(Vec3) -> Vec3) -> Result<Mat4> {
    let o = point(Vec3::ZERO);
    let x = point(Vec3::new(1.0, 0.0, 0.0)) - o;
    let y = point(Vec3::new(0.0, 1.0, 0.0)) - o;
    let z = point(Vec3::Z) - o;
    let matrix = Mat4 { m: [[x.x, y.x, z.x, o.x], [x.y, y.y, z.y, o.y], [x.z, y.z, z.z, o.z], [0.0, 0.0, 0.0, 1.0]] };
    if [o, x, y, z].iter().all(|v| v.is_finite()) { Ok(matrix) } else { Err(bad("3d transform", "the transform is not finite")) }
}

fn apply(s: &mut Session, cmd: &str, hs: &[Handle], point: &dyn Fn(Vec3) -> Vec3, plane: Option<Mat3>, affine: Mat4) -> Result<(usize, usize)> {
    if hs.is_empty() {
        return Err(bad(cmd, "no objects selected"));
    }
    let d = s.doc()?;
    let selected: std::collections::HashSet<_> = hs.iter().copied().collect();
    let mut work: Vec<(Handle, Plan)> = Vec::new();
    let mut skipped = 0;
    for h in hs {
        let Some(e) = d.entity(*h) else {
            return Err(bad(cmd, format!("no object {}", h.hex())));
        };
        if d.layer(&e.common.layer).is_some_and(|l| l.locked) {
            skipped += 1;
            continue;
        }
        if is_spatial(&e.kind) {
            work.push((*h, Plan::Points));
        } else if let Some(m) = plane {
            work.push((*h, Plan::Plane(m)));
        } else {
            return Err(bad(
                cmd,
                format!(
                    "{} {} can't be {} in 3D; only 3D faces, 3D polylines, lines and points can",
                    e.kind.type_name(),
                    h.hex(),
                    if cmd == "move3d" { "moved" } else { "turned about this axis" }
                ),
            ));
        }
    }
    if work.is_empty() {
        return Err(bad(cmd, "the objects are on locked layers"));
    }
    // Exact bodies live in a grouped set of mesh proxies. Reject partial selections and locked
    // members before transforming either representation, then update the exact body and proxy
    // fingerprint together. A stale or damaged record (faces changed by a 2D edit) is not
    // exact any more: its faces move as a mesh and the record stays as it is, so exact edits
    // keep refusing it.
    let mut exact = Vec::<(Vec<Handle>, Body)>::new();
    for (_, members) in super::solids::solid_groups(d, hs) {
        if let Ok(Some(body)) = cadcraft_doc::brep::read(d, &members) {
            if members.iter().any(|h| !selected.contains(h)) {
                return Err(bad(cmd, "select every display face of an exact solid before transforming it"));
            }
            if let Some(e) = members.iter().filter_map(|h| d.entity(*h)).find(|e| d.layer(&e.common.layer).is_some_and(|l| l.locked)) {
                return Err(bad(cmd, format!("exact solid contains an object on locked layer `{}`", e.common.layer)));
            }
            let transformed = body.transformed(affine).map_err(|e| bad(cmd, e.to_string()))?;
            exact.push((members, transformed));
        }
    }
    let backup = s.doc()?.clone();
    let result = (|| {
        let d = s.doc_mut()?;
        for (h, plan) in &work {
            d.modify_entity(*h, |e| match plan {
                Plan::Points => map_points(&mut e.kind, point),
                Plan::Plane(m) => e.kind.transform(m),
            })?;
        }
        for (members, body) in &exact {
            cadcraft_doc::brep::attach(d, members, body).map_err(|e| bad(cmd, e))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        s.state_mut()?.doc = backup.into();
        return Err(error);
    }
    Ok((work.len(), skipped))
}

fn run_move3d(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "move3d";
    let offset = match (point(c, p, "offset")?, point(c, p, "from")?, point(c, p, "to")?) {
        (Some(o), None, None) => o,
        (None, Some(a), Some(b)) => b - a,
        (None, None, None) => return Err(bad(c, "give `offset` [dx,dy,dz] or `from` and `to`")),
        _ => return Err(bad(c, "give either `offset` or both `from` and `to`")),
    };
    if !offset.is_finite() {
        return Err(bad(c, "the offset is not a finite distance"));
    }
    let hs = targets(s, p)?;
    // Objects in a plane move sideways only: a lift would take them out of it.
    let plane = (offset.z == 0.0).then(|| Mat3::translate(offset.xy()));
    let point = |q: Vec3| q + offset;
    let (moved, skipped) = apply(s, c, &hs, &point, plane, affine_of(&point)?)?;
    Ok(json!({ "moved": moved, "skipped": skipped, "offset": [offset.x, offset.y, offset.z] }))
}

/// The axis of ROTATE3D as (point on it, unit direction).
fn axis_of(p: &Value) -> Result<(Vec3, Vec3)> {
    let c = "rotate3d";
    let base = point(c, p, "base")?;
    match p.get("axis") {
        None | Some(Value::Null) => Ok((base.unwrap_or(Vec3::ZERO), Vec3::new(0.0, 0.0, 1.0))),
        Some(Value::String(t)) => {
            let dir = match t.trim().to_ascii_lowercase().as_str() {
                "x" => Vec3::new(1.0, 0.0, 0.0),
                "y" => Vec3::new(0.0, 1.0, 0.0),
                "z" => Vec3::new(0.0, 0.0, 1.0),
                other => return Err(bad(c, format!("`axis` must be x, y, z or two points, not `{other}`"))),
            };
            Ok((base.unwrap_or(Vec3::ZERO), dir))
        }
        Some(Value::Array(a)) if a.len() == 2 => {
            let (Some(p0), Some(p1)) = (a.first().and_then(vec3), a.get(1).and_then(vec3)) else {
                return Err(bad(c, "`axis` points must be [x, y, z] of finite numbers"));
            };
            let d = p1 - p0;
            if !(d.len().is_finite() && d.len() > 1e-12) {
                return Err(bad(c, "the two `axis` points coincide"));
            }
            Ok((p0, d.normalized()))
        }
        Some(_) => Err(bad(c, "`axis` must be \"x\", \"y\", \"z\" or two points [[x,y,z],[x,y,z]]")),
    }
}

/// `p` turned by `angle` radians about the axis through `a` along the unit vector `u` (Rodrigues).
fn turn(p: Vec3, a: Vec3, u: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    let v = p - a;
    a + v * cos + u.cross(v) * sin + u * (u.dot(v) * (1.0 - cos))
}

fn run_rotate3d(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "rotate3d";
    let angle = f64_req(c, p, "angle")?.to_radians();
    let (a, u) = axis_of(p)?;
    let hs = targets(s, p)?;
    // A vertical axis turns planar objects within their plane; a tilted one cannot.
    let vertical = u.x.abs() < 1e-12 && u.y.abs() < 1e-12;
    let plane = vertical.then(|| Mat3::rotate_about(a.xy(), if u.z < 0.0 { -angle } else { angle }));
    let point = |q: Vec3| turn(q, a, u, angle);
    let (rotated, skipped) = apply(s, c, &hs, &point, plane, affine_of(&point)?)?;
    Ok(json!({ "rotated": rotated, "skipped": skipped }))
}

// ---------------- prompts ----------------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Move,
    Rotate,
}

/// Select objects, then base point, second point and a height (3D MOVE) or axis, base point and
/// angle (3D ROTATE). Points are world-space 3D coordinates; Vec2 input remains accepted for
/// scripts and callers that use the legacy XY plane.
struct Xform3dM {
    kind: Kind,
    sel: SelectPhase,
    objs: Vec<Handle>,
    axis: char,
    base: Option<Vec3>,
    second: Option<Vec3>,
}

impl Xform3dM {
    fn new(kind: Kind) -> Self {
        Xform3dM { kind, sel: SelectPhase::default(), objs: Vec::new(), axis: 'z', base: None, second: None }
    }
    fn chosen(&mut self, s: &mut Session, hs: Vec<Handle>) -> Result<Step> {
        self.objs = hs;
        s.set_selection(Vec::new());
        Ok(Step::Continue)
    }
    fn finish(&self, s: &mut Session, params: Value) -> Result<Step> {
        let id = if self.kind == Kind::Move { "move3d" } else { "rotate3d" };
        let mut params = params;
        if let Some(o) = params.as_object_mut() {
            o.insert("handles".into(), json!(self.objs.iter().map(|h| h.hex()).collect::<Vec<_>>()));
        }
        let spec = super::find_command(id).ok_or_else(|| crate::EngineError::UnknownCommand(id.into()))?;
        // The running command records the single undo step.
        (spec.run)(s, &params)?;
        Ok(Step::Done)
    }
}

impl Interactive for Xform3dM {
    fn name(&self) -> &'static str {
        if self.kind == Kind::Move { "3D Move" } else { "3D Rotate" }
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            let hs = self.sel.picked.clone();
            return self.chosen(s, hs);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.sel.done {
            return self.sel.prompt();
        }
        match (self.kind, self.base, self.second) {
            (Kind::Move, None, _) => Prompt::new("Specify base point", Accept::POINT).point3d(),
            (Kind::Move, Some(b), None) => Prompt::new("Specify second point", Accept::POINT).point3d().base(b.xy()),
            (Kind::Move, Some(_), Some(_)) => Prompt::new("Specify height (Z) offset", Accept::NUMBER).default("0"),
            (Kind::Rotate, None, _) if self.axis_pending() => {
                Prompt::new("Specify rotation axis", Accept::POINT).point3d().kw(&["X", "Y", "Z"]).default("Z")
            }
            (Kind::Rotate, None, _) => Prompt::new("Specify base point", Accept::POINT).point3d(),
            (Kind::Rotate, Some(b), _) => Prompt::new("Specify rotation angle", Accept::NUMBER).base(b.xy()),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => self.chosen(s, hs),
            };
        }
        match (self.kind, self.base, self.second, i) {
            (Kind::Move, None, _, Input::Point(p)) => {
                self.base = Some(Vec3::new(p.x, p.y, 0.0));
                Ok(Step::Continue)
            }
            (Kind::Move, Some(_), None, Input::Point(p)) => {
                self.second = Some(Vec3::new(p.x, p.y, 0.0));
                Ok(Step::Continue)
            }
            (Kind::Move, None, _, Input::Point3(p)) => {
                self.base = Some(p);
                Ok(Step::Continue)
            }
            (Kind::Move, Some(_), None, Input::Point3(p)) => {
                self.second = Some(p);
                Ok(Step::Continue)
            }
            (Kind::Move, Some(b), Some(q), Input::Enter) => self.finish(s, json!({ "offset": [q.x - b.x, q.y - b.y, q.z - b.z] })),
            (Kind::Move, Some(b), Some(q), Input::Text(t)) => {
                let z = machines::number(&t).filter(|z| z.is_finite()).ok_or_else(|| bad("move3d", "the height must be a number"))?;
                self.finish(s, json!({ "offset": [q.x - b.x, q.y - b.y, q.z - b.z + z] }))
            }
            (Kind::Rotate, None, _, Input::Keyword(k) | Input::Text(k)) if self.axis_pending() => {
                self.axis = match k.trim().to_ascii_lowercase().as_str() {
                    "x" => 'x',
                    "y" => 'y',
                    "z" => 'z',
                    _ => return Ok(Step::Continue),
                };
                self.second = Some(Vec3::ZERO);
                Ok(Step::Continue)
            }
            (Kind::Rotate, None, _, Input::Enter) if self.axis_pending() => {
                self.second = Some(Vec3::ZERO);
                Ok(Step::Continue)
            }
            (Kind::Rotate, None, _, Input::Point(p)) => {
                self.base = Some(Vec3::new(p.x, p.y, 0.0));
                self.second = Some(Vec3::ZERO);
                Ok(Step::Continue)
            }
            (Kind::Rotate, None, _, Input::Point3(p)) => {
                self.base = Some(p);
                self.second = Some(Vec3::ZERO);
                Ok(Step::Continue)
            }
            (Kind::Rotate, Some(b), _, Input::Text(t)) => {
                let a = machines::number(&t).filter(|a| a.is_finite()).ok_or_else(|| bad("rotate3d", "the angle must be a number"))?;
                self.finish(s, json!({ "angle": a, "axis": self.axis.to_string(), "base": [b.x, b.y, b.z] }))
            }
            _ => Ok(Step::Continue),
        }
    }
}

impl Xform3dM {
    /// ROTATE3D still has to hear the axis (a point picked first counts as the base point).
    fn axis_pending(&self) -> bool {
        self.second.is_none()
    }
}

#[cfg(test)]
mod tests {
    use cadcraft_doc::{Common, EntityKind, Face3d};
    use cadcraft_geom::{Vec2, Vec3};
    use serde_json::json;

    use super::*;

    fn face(s: &mut Session, pts: [[f64; 3]; 3]) -> Handle {
        let corners = [0, 1, 2, 2].map(|i| Vec3::new(pts[i][0], pts[i][1], pts[i][2]));
        let space = s.space();
        s.doc_mut().unwrap().add(&space, Common::default(), EntityKind::Face3d(Face3d { corners, hidden_edges: 0 })).unwrap()
    }

    fn corners(s: &Session, h: Handle) -> [Vec3; 4] {
        match &s.doc().unwrap().entity(h).unwrap().kind {
            EntityKind::Face3d(f) => f.corners,
            _ => panic!("not a 3D face"),
        }
    }

    fn near(a: Vec3, b: [f64; 3]) -> bool {
        (a - Vec3::new(b[0], b[1], b[2])).len() < 1e-9
    }

    #[test]
    fn move3d_carries_z_and_undoes_in_one_step() {
        let mut s = Session::new();
        let f = face(&mut s, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 5.0]]);
        s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
        let before = s.doc().unwrap().clone();
        let r = s.execute("move3d", &json!({ "handles": [f.hex()], "offset": [1, 2, 3] })).unwrap();
        assert_eq!(r["moved"], 1);
        assert!(near(corners(&s, f)[2], [11.0, 12.0, 8.0]));
        s.execute("move3d", &json!({ "handles": [f.hex()], "from": [0, 0, 0], "to": [0, 0, -3] })).unwrap();
        assert!(near(corners(&s, f)[0], [1.0, 2.0, 0.0]));
        s.execute("undo", &json!({})).unwrap();
        assert!(near(corners(&s, f)[0], [1.0, 2.0, 3.0]));
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(
            corners(&s, f),
            match &before.entity(f).unwrap().kind {
                EntityKind::Face3d(x) => x.corners,
                _ => panic!("not a 3D face"),
            }
        );
    }

    /// A snap override typed at a 3D prompt out of habit is refused, and a 3D point given while
    /// modifiers are pending clears them: the prompt never locks up (#2544 review).
    #[test]
    fn point_modifiers_do_not_lock_3d_prompts() {
        let mut s = Session::new();
        let h = face(&mut s, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        s.set_selection(vec![h]);
        s.start("move3d").unwrap();
        s.cmdline("end").unwrap();
        assert!(s.point_mods.is_empty(), "a modifier is refused at a 3D prompt");
        s.cmdline("0,0,0").unwrap();
        // A modifier left pending (pushed before the prompt became 3D) is dropped by a 3D point.
        s.point_mods.push(crate::pointmod::Frame::Snap { mode: 0 });
        s.input(Input::Point3(Vec3::new(2.0, 3.0, 4.0))).unwrap();
        assert!(s.point_mods.is_empty());
        s.input(Input::Enter).unwrap();
        assert!(near(corners(&s, h)[0], [2.0, 3.0, 4.0]));
    }

    #[test]
    fn typed_relative_xyz_moves_from_the_previous_wcs_point() {
        let mut s = Session::new();
        let h = face(&mut s, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        s.set_selection(vec![h]);
        s.start("move3d").unwrap();
        s.cmdline("1,2,3").unwrap();
        s.cmdline("@4,5,6").unwrap();
        s.input(Input::Enter).unwrap();
        assert!(near(corners(&s, h)[0], [4.0, 5.0, 6.0]));
    }

    #[test]
    fn exact_box_move_and_rotate_keep_the_source_valid_and_undoable() {
        let mut s = Session::new();
        let created = s.execute("box", &json!({"p1":[0,0,0],"p2":[4,3,2]})).unwrap();
        let handles = created["handles"].as_array().unwrap().clone();
        let first = handles[0].as_str().unwrap();
        let before = s.execute("solidinfo", &json!({"handles":[first]})).unwrap();
        s.execute("move3d", &json!({"handles":handles.clone(),"offset":[5,-2,7]})).unwrap();
        let moved = s.execute("solidinfo", &json!({"handles":[first]})).unwrap();
        assert_eq!(moved["representation"], "analyticBrep");
        let before_vertex = before["edges"][0]["start"].as_array().unwrap();
        let moved_vertex = moved["edges"][0]["start"].as_array().unwrap();
        assert!((moved_vertex[0].as_f64().unwrap() - before_vertex[0].as_f64().unwrap() - 5.0).abs() < 1e-8);
        assert!((moved_vertex[1].as_f64().unwrap() - before_vertex[1].as_f64().unwrap() + 2.0).abs() < 1e-8);
        assert!((moved_vertex[2].as_f64().unwrap() - before_vertex[2].as_f64().unwrap() - 7.0).abs() < 1e-8);
        s.execute("rotate3d", &json!({"handles":handles.clone(),"axis":"z","angle":90,"base":[0,0,0]})).unwrap();
        assert_eq!(s.execute("solidinfo", &json!({"handles":[first]})).unwrap()["representation"], "analyticBrep");
        assert!(cadcraft_io::write(s.doc().unwrap(), "part.step").is_ok());
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.execute("solidinfo", &json!({"handles":[first]})).unwrap()["representation"], "analyticBrep");
        s.execute("undo", &json!({})).unwrap();
        let restored = s.execute("solidinfo", &json!({"handles":[first]})).unwrap();
        assert_eq!(restored["edges"], before["edges"]);
    }

    #[test]
    fn exact_body_partial_and_locked_member_transforms_are_atomic() {
        let mut s = Session::new();
        let created = s.execute("box", &json!({"p1":[0,0,0],"p2":[4,3,2]})).unwrap();
        let handles = created["handles"].as_array().unwrap().clone();
        let before = s.doc().unwrap().clone();
        assert!(s.execute("move3d", &json!({"handles":[handles[0]],"offset":[1,2,3]})).is_err());
        assert_eq!(&before, s.doc().unwrap());

        let locked_handle = cadcraft_doc::Handle::parse_hex(handles[0].as_str().unwrap()).unwrap();
        let layer_name = s.doc().unwrap().entity(locked_handle).unwrap().common.layer.clone();
        s.doc_mut().unwrap().layers.iter_mut().find(|l| l.name == layer_name).unwrap().locked = true;
        let locked_before = s.doc().unwrap().clone();
        assert!(s.execute("rotate3d", &json!({"handles":handles.clone(),"axis":"x","angle":15})).is_err());
        assert_eq!(&locked_before, s.doc().unwrap());
    }

    /// A 2D edit leaves the exact record stale; MOVE3D and ROTATE3D still move the faces as a
    /// mesh, and the stale record keeps exact edits refusing the solid (#2544 review).
    #[test]
    fn stale_exact_record_transforms_as_mesh_only() {
        let mut s = Session::new();
        let created = s.execute("box", &json!({"p1":[0,0,0],"p2":[4,3,2]})).unwrap();
        let handles = created["handles"].as_array().unwrap().clone();
        let first = cadcraft_doc::Handle::parse_hex(handles[0].as_str().unwrap()).unwrap();
        s.execute("move", &json!({"handles":handles.clone(),"from":[0,0],"to":[10,0]})).unwrap();
        let stale = s.execute("solidinfo", &json!({"handles":[handles[0]]})).unwrap_err().to_string();
        assert!(stale.contains("stale"), "{stale}");
        let before = corners(&s, first);
        s.execute("move3d", &json!({"handles":handles.clone(),"offset":[0,0,5]})).unwrap();
        assert!((corners(&s, first)[0].z - before[0].z - 5.0).abs() < 1e-12);
        s.execute("rotate3d", &json!({"handles":handles.clone(),"axis":"z","angle":90})).unwrap();
        assert!(s.execute("solidinfo", &json!({"handles":[handles[0]]})).unwrap_err().to_string().contains("stale"));
        assert!(s.execute("filletedge", &json!({"handles":[handles[0]],"radius":0.2})).is_err());
    }

    #[test]
    fn move3d_refuses_bad_input_and_planar_objects_that_would_leave_their_plane() {
        let mut s = Session::new();
        let f = face(&mut s, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        s.execute("circle", &json!({ "center": [0, 0], "radius": 2 })).unwrap();
        let c = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().handle;
        for bad in [
            json!({}),
            json!({ "offset": [1, 2, 3], "to": [1, 1] }),
            json!({ "offset": "x" }),
            json!({ "offset": [1, 2, 3, 4] }),
            json!({ "from": [0, 0, 0] }),
        ] {
            let mut q = bad.clone();
            q["handles"] = json!([f.hex()]);
            assert!(s.execute("move3d", &q).is_err(), "{bad}");
        }
        assert!(s.execute("move3d", &json!({ "handles": [] , "offset": [1, 1, 1] })).is_err());
        assert!(s.execute("move3d", &json!({ "handles": ["FFFFF"], "offset": [1, 1, 1] })).is_err());
        // A lift would take the circle out of its plane: refused, and the face (listed first) stays.
        let e = s.execute("move3d", &json!({ "handles": [f.hex(), c.hex()], "offset": [0, 0, 5] })).unwrap_err().to_string();
        assert!(e.contains("Circle"), "{e}");
        assert!(near(corners(&s, f)[0], [0.0, 0.0, 0.0]));
        // Sideways it is fine.
        s.execute("move3d", &json!({ "handles": [f.hex(), c.hex()], "offset": [5, 0, 0] })).unwrap();
        assert!(near(corners(&s, f)[0], [5.0, 0.0, 0.0]));
    }

    #[test]
    fn rotate3d_turns_about_any_axis() {
        let mut s = Session::new();
        let f = face(&mut s, [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 0.0, 1.0]]);
        // 90 degrees about the Y axis through the origin: x -> -z, z -> x.
        s.execute("rotate3d", &json!({ "handles": [f.hex()], "axis": "y", "angle": 90 })).unwrap();
        let c = corners(&s, f);
        assert!(near(c[0], [0.0, 0.0, -1.0]) && near(c[1], [0.0, 0.0, -2.0]) && near(c[2], [1.0, 0.0, -1.0]), "{c:?}");
        // About an axis given by two points: the vertical line through (1,1): z stays, xy turns.
        s.execute("rotate3d", &json!({ "handles": [f.hex()], "axis": [[1, 1, 0], [1, 1, 9]], "angle": 180 })).unwrap();
        let c = corners(&s, f);
        assert!(near(c[0], [2.0, 2.0, -1.0]) && near(c[2], [1.0, 2.0, -1.0]), "{c:?}");
        // Default axis is Z through `base`.
        s.execute("rotate3d", &json!({ "handles": [f.hex()], "base": [2, 2, 0], "angle": 90 })).unwrap();
        assert!(near(corners(&s, f)[0], [2.0, 2.0, -1.0]) && near(corners(&s, f)[2], [2.0, 1.0, -1.0]));
        // Reversed vertical axis turns the other way.
        s.execute("rotate3d", &json!({ "handles": [f.hex()], "axis": [[2, 2, 0], [2, 2, -1]], "angle": 90 })).unwrap();
        // Turning back by the same angle about the reversed axis restores the previous corner.
        assert!(near(corners(&s, f)[2], [1.0, 2.0, -1.0]), "{:?}", corners(&s, f)[2]);
        s.execute("undo", &json!({})).unwrap();
        s.execute("undo", &json!({})).unwrap();
        s.execute("undo", &json!({})).unwrap();
        s.execute("undo", &json!({})).unwrap();
        assert!(near(corners(&s, f)[0], [1.0, 0.0, 0.0]));
    }

    #[test]
    fn rotate3d_refuses_hostile_input_and_tilted_turns_of_planar_objects() {
        let mut s = Session::new();
        let f = face(&mut s, [[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 0.0, 1.0]]);
        s.execute("circle", &json!({ "center": [0, 0], "radius": 2 })).unwrap();
        let c = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Circle(_))).unwrap().handle;
        for bad in [
            json!({}),
            json!({ "angle": "ninety" }),
            json!({ "angle": 10, "axis": "w" }),
            json!({ "angle": 10, "axis": [[0, 0, 0], [0, 0, 0]] }),
            json!({ "angle": 10, "axis": [[0, 0, 0]] }),
            json!({ "angle": 10, "axis": 5 }),
            json!({ "angle": 10, "base": [0] }),
        ] {
            let mut q = bad.clone();
            q["handles"] = json!([f.hex()]);
            assert!(s.execute("rotate3d", &q).is_err(), "{bad}");
        }
        assert!(s.execute("rotate3d", &json!({ "handles": [f.hex(), c.hex()], "axis": "x", "angle": 30 })).is_err());
        assert!(near(corners(&s, f)[0], [1.0, 0.0, 0.0]), "a refused turn changes nothing");
        // Turning the circle about a vertical axis is fine.
        s.execute("rotate3d", &json!({ "handles": [c.hex()], "base": [5, 0, 0], "angle": 90 })).unwrap();
    }

    #[test]
    fn the_prompts_run_the_same_commands() {
        let mut s = Session::new();
        let f = face(&mut s, [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        s.set_selection(vec![f]);
        s.start("move3d").unwrap();
        s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
        s.input(Input::Point(Vec2::new(2.0, 0.0))).unwrap();
        s.input(Input::Text("4".into())).unwrap();
        assert!(s.running.is_none());
        assert!(near(corners(&s, f)[0], [2.0, 0.0, 4.0]));
        s.set_selection(vec![f]);
        s.start("rotate3d").unwrap();
        s.input(Input::Keyword("X".into())).unwrap();
        s.input(Input::Point(Vec2::new(0.0, 0.0))).unwrap();
        s.input(Input::Text("90".into())).unwrap();
        assert!(s.running.is_none());
        assert!(near(corners(&s, f)[0], [2.0, -4.0, 0.0]), "{:?}", corners(&s, f)[0]);
    }
}
