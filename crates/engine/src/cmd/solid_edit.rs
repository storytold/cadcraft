//! Transactional edits of persisted exact solids. The triangle proxy is always regenerated.
use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, solids, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};
use cadcraft_doc::Handle;
use cadcraft_geom::{Mat4, Vec3, brep::Body};
use serde_json::{Value, json};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("solidinfo","Inspect Exact Solid",inspect).menu(&["Tools","Inquiry","Exact Solid"])
            .params("{handles? (one face identifies a solid)} → {group, representation, shells, vertices, edges, faces}: exact edge ids `shell:edge` remain stable across rigid transforms and display retessellation")
            .noundo(),
        CommandSpec::new("retessellate","Refine Solid Display",retessellate).menu(&["Modify","3D Operations","Refine Display"])
            .params("{handles?, tolerance: positive drawing units (>=1e-5)}: regenerate display triangles from persisted exact geometry, preserving source topology ids")
            .interactive(|_|Ok(Box::new(EditMachine::new("retessellate","tolerance")))),
        CommandSpec::new("filletedge","Fillet Solid Edges",fillet).menu(&["Modify","3D Operations","Fillet Edges"])
            .params("{handles?, radius, edges?: [\"shell:edge\",...] (default all vertical edges), tolerance?: 0.05}: exact circular fillets of convex vertical edges on straight polygonal prisms; other configurations refused")
            .interactive(|_|Ok(Box::new(EditMachine::new("filletedge","radius")))),
        CommandSpec::new("chamferedge","Chamfer Solid Edges",chamfer).menu(&["Modify","3D Operations","Chamfer Edges"])
            .params("{handles?, distance, edges?: [\"shell:edge\",...] (default all vertical edges), tolerance?: 0.05}: equal-distance chamfers of convex vertical prism edges")
            .interactive(|_|Ok(Box::new(EditMachine::new("chamferedge","distance")))),
        CommandSpec::new("shell","Shell Solid",shell).menu(&["Modify","3D Operations","Shell"])
            .params("{handles?, thickness, openTop?: true, tolerance?: 0.05}: hollow a straight convex polygonal vertical prism with a constant normal wall thickness; opens the upper face by default")
            .interactive(|_|Ok(Box::new(EditMachine::new("shell","thickness")))),
        CommandSpec::new("scale3d","Scale Exact Solid",scale).menu(&["Modify","3D Operations","3D Scale"])
            .params("{handles?, factors: [sx,sy,sz] (positive), base?: [x,y,z], tolerance?:0.05}: affine scaling of exact curves/surfaces; topology indices preserved"),
    ]
}

fn source(s: &Session, p: &Value, c: &str) -> Result<(String, Vec<Handle>, Body)> {
    let requested = targets(s, p)?;
    let groups = solids::solid_groups(s.doc()?, &requested);
    let [(name, hs)] = groups.as_slice() else { return Err(bad(c, "select exactly one complete grouped solid")) };
    if requested.iter().any(|h| !hs.contains(h)) {
        return Err(bad(c, "selection contains an object outside the solid"));
    }
    let body = cadcraft_doc::brep::read(s.doc()?, hs)
        .map_err(|e| bad(c, e))?
        .ok_or_else(|| bad(c, "this object is a mesh; it has no persisted exact source geometry"))?;
    Ok((name.clone(), hs.clone(), body))
}
fn number(p: &Value, key: &str, default: Option<f64>, c: &str) -> Result<f64> {
    let n = match p.get(key) {
        None => default,
        Some(v) => v.as_f64(),
    }
    .filter(|n| n.is_finite() && *n > 0.0);
    n.ok_or_else(|| bad(c, format!("`{key}` must be a finite positive number")))
}
fn edges(body: &Body, p: &Value, c: &str) -> Result<Vec<(usize, usize)>> {
    if let Some(value) = p.get("edges") {
        let values =
            value.as_array().filter(|a| !a.is_empty() && a.len() <= 256).ok_or_else(|| bad(c, "`edges` must contain 1..256 exact edge ids"))?;
        return values
            .iter()
            .map(|v| {
                let (a, b) =
                    v.as_str().and_then(|v| v.split_once(':')).ok_or_else(|| bad(c, "edge ids use `shell:edge`, as returned by SOLIDINFO"))?;
                Ok((a.parse().map_err(|_| bad(c, "invalid shell index"))?, b.parse().map_err(|_| bad(c, "invalid edge index"))?))
            })
            .collect();
    }
    Ok(body
        .geometry
        .boundaries
        .iter()
        .enumerate()
        .flat_map(|(s, b)| {
            b.edges.iter().enumerate().filter_map(move |(i, e)| {
                let a = b.vertices.get(e.vertices.0)?;
                let z = b.vertices.get(e.vertices.1)?;
                ((a.x - z.x).hypot(a.y - z.y) < 1e-8 && (a.z - z.z).abs() > 1e-8).then_some((s, i))
            })
        })
        .collect())
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let (group, _, body) = source(s, p, "solidinfo")?;
    let mut edge_info = Vec::new();
    let mut vertex_count = 0;
    let mut face_count = 0;
    for (i, b) in body.geometry.boundaries.iter().enumerate() {
        vertex_count += b.vertices.len();
        face_count += b.faces.len();
        for (j, e) in b.edges.iter().enumerate() {
            let a = b.vertices.get(e.vertices.0).ok_or_else(|| bad("solidinfo", "invalid vertex"))?;
            let z = b.vertices.get(e.vertices.1).ok_or_else(|| bad("solidinfo", "invalid vertex"))?;
            let curve = match &e.curve {
                cadcraft_geom::brep::kernel::Curve::Line(_) => "line",
                cadcraft_geom::brep::kernel::Curve::BSplineCurve(_) => "bspline",
                cadcraft_geom::brep::kernel::Curve::NurbsCurve(_) => "nurbs",
                _ => "intersection",
            };
            edge_info.push(json!({"id":format!("{i}:{j}"),"curve":curve,"start":[a.x,a.y,a.z],"end":[z.x,z.y,z.z]}));
        }
    }
    Ok(
        json!({"group":group,"representation":"analyticBrep","shells":body.geometry.boundaries.len(),"vertices":vertex_count,"faces":face_count,"edges":edge_info}),
    )
}

fn edit(s: &mut Session, p: &Value, c: &str, f: impl FnOnce(&Body) -> Result<Body>) -> Result<Value> {
    let (name, hs, old) = source(s, p, c)?;
    let tolerance = number(p, "tolerance", Some(0.05), c)?;
    for h in &hs {
        let e = s.doc()?.entity(*h).ok_or_else(|| bad(c, "missing solid face"))?;
        if s.doc()?.layer(&e.common.layer).is_some_and(|l| l.locked) {
            return Err(bad(c, format!("solid contains a face on locked layer `{}`", e.common.layer)));
        }
    }
    let common = hs.first().and_then(|h| s.doc().ok()?.entity(*h)).map(|e| e.common.clone()).ok_or_else(|| bad(c, "missing solid face"))?;
    let mut common = common;
    // Payload is regenerated from the edited body; never copy the old anchor record.
    cadcraft_doc::brep::strip(&mut common);
    let body = f(&old)?;
    let mesh = body.tessellate(tolerance).map_err(|e| bad(c, e.to_string()))?;
    let backup = s.doc()?.clone();
    let result = (|| {
        let d = s.doc_mut()?;
        for h in &hs {
            d.remove_entity(*h);
        }
        d.groups.retain(|g| g.name != name);
        let (group, handles) = cadcraft_doc::brep::insert(d, &body, &mesh, &common).map_err(|e| bad(c, e))?;
        Ok(
            json!({"group":group,"groups":[group],"handles":handles.iter().map(|h|h.hex()).collect::<Vec<_>>(),"faces":handles.len(),"representation":"analyticBrep"}),
        )
    })();
    if result.is_err() {
        *s.doc_mut()? = backup;
    } else {
        s.set_selection(Vec::new());
    }
    result
}
fn retessellate(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "retessellate", |b| Ok(b.clone()))
}
fn fillet(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "filletedge", |b| {
        b.fillet_vertical_edges(&edges(b, p, "filletedge")?, number(p, "radius", None, "filletedge")?).map_err(|e| bad("filletedge", e.to_string()))
    })
}
fn chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "chamferedge", |b| {
        b.chamfer_vertical_edges(&edges(b, p, "chamferedge")?, number(p, "distance", None, "chamferedge")?)
            .map_err(|e| bad("chamferedge", e.to_string()))
    })
}
fn shell(s: &mut Session, p: &Value) -> Result<Value> {
    let open = match p.get("openTop") {
        None => true,
        Some(v) => v.as_bool().ok_or_else(|| bad("shell", "`openTop` must be boolean"))?,
    };
    edit(s, p, "shell", |b| b.shell_prism(number(p, "thickness", None, "shell")?, open).map_err(|e| bad("shell", e.to_string())))
}
fn scale(s: &mut Session, p: &Value) -> Result<Value> {
    let vector = |key: &str, default: Option<Vec3>| -> Result<Vec3> {
        let Some(value) = p.get(key) else { return default.ok_or_else(|| bad("scale3d", format!("`{key}` is required"))) };
        let a = value.as_array().filter(|a| a.len() == 3).ok_or_else(|| bad("scale3d", format!("`{key}` must be [x,y,z]")))?;
        let v = a
            .iter()
            .map(|n| n.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad("scale3d", "coordinates must be finite")))
            .collect::<Result<Vec<_>>>()?;
        Ok(Vec3::new(v[0], v[1], v[2]))
    };
    let factors = vector("factors", None)?;
    let base = vector("base", Some(Vec3::ZERO))?;
    if [factors.x, factors.y, factors.z].iter().any(|n| *n <= 0.0 || *n > 1e6) {
        return Err(bad("scale3d", "scale factors must be positive and at most 1e6"));
    }
    let m = Mat4 {
        m: [
            [factors.x, 0.0, 0.0, base.x * (1.0 - factors.x)],
            [0.0, factors.y, 0.0, base.y * (1.0 - factors.y)],
            [0.0, 0.0, factors.z, base.z * (1.0 - factors.z)],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    edit(s, p, "scale3d", |b| b.transformed(m).map_err(|e| bad("scale3d", e.to_string())))
}

struct EditMachine {
    id: &'static str,
    key: &'static str,
    sel: SelectPhase,
    handles: Vec<Handle>,
}
impl EditMachine {
    fn new(id: &'static str, key: &'static str) -> Self {
        Self { id, key, sel: SelectPhase::default(), handles: Vec::new() }
    }
}
impl Interactive for EditMachine {
    fn name(&self) -> &'static str {
        self.id
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.sel = SelectPhase::begin(s);
        if self.sel.done {
            self.handles = self.sel.picked.clone();
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _: &Session) -> Prompt {
        if !self.sel.done { self.sel.prompt() } else { Prompt::new(format!("Specify {} (all vertical edges)", self.key), Accept::NUMBER) }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        if !self.sel.done {
            return match self.sel.feed(s, &i)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(hs) => {
                    self.handles = hs;
                    Ok(Step::Continue)
                }
            };
        }
        let Input::Text(text) = i else { return Ok(Step::Continue) };
        let value = super::machines::number(&text).ok_or_else(|| bad(self.id, "enter a positive number"))?;
        let mut p = json!({"handles":self.handles.iter().map(|h|h.hex()).collect::<Vec<_>>()});
        p[self.key] = json!(value);
        let spec = super::find_command(self.id).ok_or_else(|| bad(self.id, "command is missing"))?;
        (spec.run)(s, &p)?;
        Ok(Step::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finishing_is_atomic_undoable_and_preserves_exact_source() {
        for (id, key) in [("filletedge", "radius"), ("chamferedge", "distance"), ("shell", "thickness")] {
            let mut s = Session::new();
            let r = s.execute("box", &json!({"p1":[0,0,0],"p2":[4,4,3]})).unwrap();
            let before = s.doc().unwrap().clone();
            let mut p = json!({"handles":[r["handles"][0]]});
            p[key] = json!(10.0);
            assert!(s.execute(id, &p).is_err());
            assert_eq!(&before, s.doc().unwrap());
            p[key] = json!(0.3);
            let out = s.execute(id, &p).unwrap();
            assert_eq!(out["representation"], "analyticBrep");
            let info = s.execute("solidinfo", &json!({"handles":[out["handles"][0]]})).unwrap();
            assert!(info["edges"].as_array().unwrap().len() > 8);
            s.undo().unwrap();
            assert_eq!(&before, s.doc().unwrap());
            s.redo().unwrap();
        }
    }
    #[test]
    fn locked_member_blocks_entire_exact_edit() {
        let mut s = Session::new();
        let r = s.execute("box", &json!({"p1":[0,0,0],"p2":[4,4,3]})).unwrap();
        s.doc_mut().unwrap().layers[0].locked = true;
        let before = s.doc().unwrap().clone();
        assert!(s.execute("filletedge", &json!({"handles":[r["handles"][0]],"radius":0.3})).is_err());
        assert_eq!(&before, s.doc().unwrap());
    }
}
