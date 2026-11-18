//! Plane sections and capped slicing for grouped 3D solids.
use std::collections::HashSet;

use cadcraft_doc::{EntityKind, Handle, Polyline3d};
use cadcraft_geom::{Mesh3, Vec3};
use serde_json::{Value, json};

use super::machines::{SelOutcome, SelectPhase};
use super::{CommandSpec, bad, solids, targets};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

const MAX_SOLIDS: usize = 64;
const MAX_INPUT_FACES: usize = 100_000;
const MAX_OUTPUT_FACES: usize = 50_000;
const MAX_SECTION_POLYLINES: usize = 20_000;
const MAX_SECTION_POINTS: usize = 1_000_000;
const SECTION_GROUP: &str = "3D section";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("slice", "Slice Solid", run_slice)
            .menu(&["Modify", "3D Operations", "Slice"])
            .params("{handles? (default selection): grouped 3D solids, origin: [x,y,z], normal: [x,y,z], keep?: positive|negative|both (default both)} → {handles, groups, faces, solids, keep}. Positive means `(point - origin) · normal >= 0`.")
            .interactive(|_| Ok(Box::new(SectionM::new(SectionKind::Slice)))),
        CommandSpec::new("section", "Section Solid", run_section)
            .menu(&["Draw", "3D Operations", "Section"])
            .params("{handles? (default selection): grouped 3D solids, origin: [x,y,z], normal: [x,y,z]} → {handles, groups, polylines, sections}. Adds closed 3D polylines at the current layer; one group per source solid.")
            .interactive(|_| Ok(Box::new(SectionM::new(SectionKind::Section)))),
    ]
}

#[derive(Clone, Copy)]
enum SectionKind {
    Slice,
    Section,
}

struct SectionM {
    kind: SectionKind,
    selection: SelectPhase,
    handles: Vec<Handle>,
    origin: Option<Vec3>,
}

impl SectionM {
    fn new(kind: SectionKind) -> Self {
        Self { kind, selection: SelectPhase::default(), handles: Vec::new(), origin: None }
    }

    fn commit(&self, s: &mut Session, second: Vec3) -> Result<Step> {
        let origin = self.origin.ok_or_else(|| bad("section", "the cutting plane origin is missing"))?;
        let id = match self.kind {
            SectionKind::Slice => "slice",
            SectionKind::Section => "section",
        };
        let params = json!({
            "handles": self.handles.iter().map(|h| h.hex()).collect::<Vec<_>>(),
            "origin": [origin.x, origin.y, origin.z],
            "normal": [second.x-origin.x, second.y-origin.y, second.z-origin.z],
        });
        let spec = super::find_command(id).ok_or_else(|| bad(id, "command is missing"))?;
        (spec.run)(s, &params)?;
        Ok(Step::Done)
    }
}

impl Interactive for SectionM {
    fn name(&self) -> &'static str {
        match self.kind {
            SectionKind::Slice => "Slice",
            SectionKind::Section => "Section",
        }
    }

    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        self.selection = SelectPhase::begin(s);
        if self.selection.done {
            self.handles = self.selection.picked.clone();
        }
        Ok(Step::Continue)
    }

    fn prompt(&self, _s: &Session) -> Prompt {
        if !self.selection.done {
            self.selection.prompt()
        } else if self.origin.is_none() {
            Prompt::new("Specify cutting plane origin", Accept::POINT).point3d()
        } else {
            Prompt::new("Specify second point on positive normal side", Accept::POINT).point3d()
        }
    }

    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        if !self.selection.done {
            return match self.selection.feed(s, &input)? {
                SelOutcome::More => Ok(Step::Continue),
                SelOutcome::Empty => Ok(Step::Done),
                SelOutcome::Done(handles) => {
                    self.handles = handles;
                    Ok(Step::Continue)
                }
            };
        }
        match input {
            Input::Cancel => Ok(Step::Cancel),
            Input::Point3(point) => {
                if self.origin.is_none() {
                    self.origin = Some(point);
                    Ok(Step::Continue)
                } else {
                    self.commit(s, point)
                }
            }
            Input::Point(point) => {
                let point = Vec3::new(point.x, point.y, 0.0);
                if self.origin.is_none() {
                    self.origin = Some(point);
                    Ok(Step::Continue)
                } else {
                    self.commit(s, point)
                }
            }
            _ => Err(bad(self.name(), "specify the cutting plane with two distinct 3D points")),
        }
    }
}

fn point3(cmd: &str, p: &Value, key: &str) -> Result<Vec3> {
    let value = p.get(key).ok_or_else(|| bad(cmd, format!("`{key}` ([x,y,z]) is required")))?;
    let array = value.as_array().ok_or_else(|| bad(cmd, format!("`{key}` must be a three-number array")))?;
    if array.len() != 3 {
        return Err(bad(cmd, format!("`{key}` must be a three-number array")));
    }
    let mut coords = [0.0; 3];
    for (i, item) in array.iter().enumerate() {
        let Some(n) = item.as_f64().filter(|n| n.is_finite()) else {
            return Err(bad(cmd, format!("`{key}` must contain finite numbers")));
        };
        coords[i] = n;
    }
    Ok(Vec3::new(coords[0], coords[1], coords[2]))
}

fn selection_groups(s: &Session, cmd: &str, p: &Value) -> Result<Vec<(String, Vec<Handle>)>> {
    let requested = targets(s, p)?;
    if requested.is_empty() {
        return Err(bad(cmd, "select at least one grouped 3D solid"));
    }
    let d = s.doc()?;
    let groups = solids::solid_groups(d, &requested);
    if groups.is_empty() {
        return Err(bad(cmd, "selection contains no grouped 3D solid"));
    }
    if groups.len() > MAX_SOLIDS {
        return Err(bad(cmd, format!("at most {MAX_SOLIDS} solids at once")));
    }
    let members: HashSet<_> = groups.iter().flat_map(|(_, hs)| hs.iter().copied()).collect();
    if requested.iter().any(|h| !members.contains(h)) {
        return Err(bad(cmd, "select grouped 3D solids only"));
    }
    for (_, hs) in &groups {
        if hs.iter().any(|h| !matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Face3d(_)))) {
            return Err(bad(cmd, "solid group contains a non-face object"));
        }
    }
    Ok(groups)
}

fn locked_sources(s: &Session, cmd: &str, groups: &[(String, Vec<Handle>)]) -> Result<()> {
    let d = s.doc()?;
    for (_, hs) in groups {
        for h in hs {
            if let Some(e) = d.entity(*h)
                && d.layer(&e.common.layer).is_some_and(|layer| layer.locked)
            {
                return Err(bad(cmd, format!("solid contains an object on locked layer `{}`", e.common.layer)));
            }
        }
    }
    Ok(())
}

fn locked_current_layer(s: &Session, cmd: &str) -> Result<()> {
    let common = s.current_common();
    if s.doc()?.layer(&common.layer).is_some_and(|layer| layer.locked) {
        return Err(bad(cmd, format!("current layer `{}` is locked", common.layer)));
    }
    Ok(())
}

fn mesh_inputs(s: &Session, cmd: &str, groups: &[(String, Vec<Handle>)]) -> Result<Vec<(String, Vec<Handle>, Mesh3)>> {
    let input_faces =
        groups.iter().try_fold(0usize, |n, (_, handles)| n.checked_add(handles.len())).ok_or_else(|| bad(cmd, "solid input is too large"))?;
    if input_faces > MAX_INPUT_FACES {
        return Err(bad(cmd, format!("solid input exceeds the {MAX_INPUT_FACES}-face command limit")));
    }
    let d = s.doc()?;
    groups
        .iter()
        .map(|(name, handles)| solids::mesh_of(d, handles).map(|mesh| (name.clone(), handles.clone(), mesh)).map_err(|e| bad(cmd, e.to_string())))
        .collect()
}

fn keep_mode(p: &Value) -> Result<&'static str> {
    let keep = match p.get("keep") {
        None | Some(Value::Null) => "both",
        Some(Value::String(value)) => value.trim(),
        Some(_) => return Err(bad("slice", "`keep` must be positive, negative, or both")),
    };
    match keep.to_ascii_lowercase().as_str() {
        "positive" | "front" => Ok("positive"),
        "negative" | "back" => Ok("negative"),
        "both" => Ok("both"),
        _ => Err(bad("slice", "`keep` must be positive, negative, or both")),
    }
}

fn run_slice(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "slice";
    let origin = point3(cmd, p, "origin")?;
    let normal = point3(cmd, p, "normal")?;
    let keep = keep_mode(p)?;
    let groups = selection_groups(s, cmd, p)?;
    locked_sources(s, cmd, &groups)?;
    locked_current_layer(s, cmd)?;
    let inputs = mesh_inputs(s, cmd, &groups)?;

    // Compute every result before touching the drawing, so one failed solid leaves all inputs.
    let mut outputs = Vec::<Mesh3>::new();
    for (_, _, mesh) in &inputs {
        let (positive, negative) = cadcraft_geom::mesh_slice::slice(mesh, origin, normal).map_err(|e| bad(cmd, e.to_string()))?;
        if keep != "negative" {
            outputs.push(positive);
        }
        if keep != "positive" {
            outputs.push(negative);
        }
    }
    let total_faces: usize = outputs.iter().map(|m| m.triangles.len()).sum();
    if outputs.is_empty() || total_faces == 0 {
        return Err(bad(cmd, "the slice produced no solid"));
    }
    if total_faces > MAX_OUTPUT_FACES {
        return Err(bad(cmd, format!("slice output exceeds the {MAX_OUTPUT_FACES}-face command limit")));
    }

    let source_names: HashSet<_> = inputs.iter().map(|(name, _, _)| name.as_str()).collect();
    let sources: HashSet<_> = inputs.iter().flat_map(|(_, hs, _)| hs.iter().copied()).collect();
    let backup = s.state()?.doc.clone();
    let result = (|| {
        {
            let d = s.doc_mut()?;
            for h in &sources {
                d.remove_entity(*h);
            }
            d.groups.retain(|g| !source_names.contains(g.name.as_str()));
        }
        let mut handles = Vec::new();
        let mut groups = Vec::new();
        for mesh in &outputs {
            let r = solids::emit_mesh(s, cmd, mesh, "slice")?;
            let emitted_handles: Vec<Value> = r.get("handles").and_then(Value::as_array).into_iter().flatten().cloned().collect();
            let emitted_group = r.get("group").and_then(Value::as_str).ok_or_else(|| bad(cmd, "slice output group is missing"))?;
            let first_handle = emitted_handles.first().and_then(Value::as_str).ok_or_else(|| bad(cmd, "slice output has no face handle"))?;
            // `groups::next_unnamed` intentionally picks the first currently free `*A<n>` name.
            // Since slicing removes the source groups before emitting replacements, that would
            // recycle a deleted solid's persistent identity. Give each replacement a fresh,
            // handle-derived name instead. Handles are monotonic in a drawing and make retries
            // and undo/redo unambiguous.
            let stem = format!("SLICE_{first_handle}");
            let mut name = stem.clone();
            let mut suffix = 1usize;
            loop {
                let used = s.doc()?.groups.iter().any(|g| g.name.eq_ignore_ascii_case(&name));
                if !used {
                    break;
                }
                name = format!("{stem}_{suffix}");
                suffix = suffix.checked_add(1).ok_or_else(|| bad(cmd, "slice group name space exhausted"))?;
            }
            let group = s.doc_mut()?.groups.iter_mut().find(|g| g.name == emitted_group).ok_or_else(|| bad(cmd, "slice output group disappeared"))?;
            group.name = name.clone();
            handles.extend(emitted_handles);
            groups.push(Value::String(name));
        }
        s.set_selection(Vec::new());
        Ok(json!({"handles": handles, "groups": groups, "faces": total_faces, "solids": outputs.len(), "keep": keep}))
    })();
    if result.is_err() {
        s.state_mut()?.doc = backup;
    }
    result
}

fn run_section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "section";
    let origin = point3(cmd, p, "origin")?;
    let normal = point3(cmd, p, "normal")?;
    let groups = selection_groups(s, cmd, p)?;
    locked_current_layer(s, cmd)?;
    let inputs = mesh_inputs(s, cmd, &groups)?;
    let mut outputs = Vec::<Vec<Vec3>>::new();
    let mut section_groups = Vec::<Vec<usize>>::new();
    let mut total_points = 0usize;
    for (_, _, mesh) in &inputs {
        let loops = cadcraft_geom::mesh_slice::section(mesh, origin, normal).map_err(|e| bad(cmd, e.to_string()))?;
        if loops.is_empty() {
            return Err(bad(cmd, "the plane does not intersect every selected solid"));
        }
        let mut group_ids = Vec::new();
        for loop_points in loops {
            if loop_points.len() < 3 {
                return Err(bad(cmd, "section returned a degenerate contour"));
            }
            total_points = total_points.checked_add(loop_points.len()).ok_or_else(|| bad(cmd, "section output is too large"))?;
            if total_points > MAX_SECTION_POINTS {
                return Err(bad(cmd, "section output exceeds the point limit"));
            }
            group_ids.push(outputs.len());
            outputs.push(loop_points);
        }
        section_groups.push(group_ids);
    }
    if outputs.len() > MAX_SECTION_POLYLINES {
        return Err(bad(cmd, "section output exceeds the polyline limit"));
    }

    let backup = s.state()?.doc.clone();
    let result = (|| {
        let mut handles = Vec::with_capacity(outputs.len());
        for points in &outputs {
            handles.push(s.add_entity(EntityKind::Polyline3d(Polyline3d { points: points.clone(), closed: true }))?);
        }
        let mut names = Vec::new();
        for group_ids in &section_groups {
            let members: Vec<_> = group_ids.iter().filter_map(|i| handles.get(*i).copied()).collect();
            names.push(super::groups::create(s, None, SECTION_GROUP, &members, true)?);
        }
        Ok(
            json!({"handles": handles.iter().map(|h| h.hex()).collect::<Vec<_>>(), "groups": names, "polylines": handles.len(), "sections": section_groups.len()}),
        )
    })();
    if result.is_err() {
        s.state_mut()?.doc = backup;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineError;

    fn box_with(s: &mut Session, corner: Vec3) -> Result<Value> {
        s.execute("box", &json!({"p1": [corner.x, corner.y, corner.z], "p2": [corner.x + 2.0, corner.y + 2.0, corner.z + 2.0]}))
    }

    fn solids(s: &Session) -> Vec<String> {
        s.doc()
            .map(|d| d.groups.iter().filter(|g| g.description == super::super::solids::SOLID_GROUP).map(|g| g.name.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn slice_replaces_group_once_and_undo_restores_original() -> Result<()> {
        let mut s = Session::new();
        let source = box_with(&mut s, Vec3::ZERO)?;
        let source_group = source["group"].as_str().unwrap().to_string();
        let r = s.execute("slice", &json!({"handles": source["handles"], "origin": [0,0,1], "normal": [0,0,1], "keep": "both"}))?;
        assert_eq!(r["solids"], 2);
        assert_eq!(solids(&s).len(), 2);
        assert!(!solids(&s).contains(&source_group));
        let d = s.doc()?;
        for group_name in r["groups"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let members = d.groups.iter().find(|g| g.name == group_name).unwrap().members.as_slice();
            assert!(cadcraft_doc::brep::read(d, members).unwrap().is_none(), "mesh slice must not retain the source exact-solid record");
        }
        s.undo()?;
        assert_eq!(solids(&s), vec![source_group]);
        Ok(())
    }

    #[test]
    fn section_adds_closed_xyz_contours_and_groups_them() -> Result<()> {
        let mut s = Session::new();
        let source = box_with(&mut s, Vec3::ZERO)?;
        let r = s.execute("section", &json!({"handles": source["handles"], "origin": [0,0,1], "normal": [0,0,1]}))?;
        assert_eq!(r["polylines"], 1);
        let d = s.doc()?;
        let h = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
        let Some(e) = d.entity(h) else {
            return Err(EngineError::Other("section polyline missing".into()));
        };
        let EntityKind::Polyline3d(poly) = &e.kind else {
            return Err(EngineError::Other("section output is not a 3D polyline".into()));
        };
        assert!(poly.closed && poly.points.iter().all(|p| (p.z - 1.0).abs() < 1e-8));
        assert_eq!(d.groups.iter().filter(|g| g.description == SECTION_GROUP).count(), 1);
        Ok(())
    }

    #[test]
    fn locked_source_or_current_layer_is_atomic() -> Result<()> {
        let mut s = Session::new();
        let source = box_with(&mut s, Vec3::ZERO)?;
        let before = s.doc()?.entity_count();
        if let Some(layer) = s.doc_mut()?.layers.iter_mut().find(|l| l.name == "0") {
            layer.locked = true;
        }
        assert!(s.execute("slice", &json!({"handles": source["handles"], "origin": [0,0,1], "normal": [0,0,1]})).is_err());
        assert_eq!(s.doc()?.entity_count(), before);
        Ok(())
    }

    #[test]
    fn later_solid_failure_does_not_partially_replace_earlier_groups() -> Result<()> {
        let mut s = Session::new();
        let lower = box_with(&mut s, Vec3::ZERO)?;
        let upper = box_with(&mut s, Vec3::new(4.0, 0.0, 4.0))?;
        let handles: Vec<_> =
            lower["handles"].as_array().into_iter().flatten().chain(upper["handles"].as_array().into_iter().flatten()).cloned().collect();
        let groups_before = solids(&s);
        let entities_before = s.doc()?.entity_count();
        assert!(s.execute("slice", &json!({"handles": handles, "origin": [0,0,1], "normal": [0,0,1], "keep": "both"})).is_err());
        assert_eq!(solids(&s), groups_before);
        assert_eq!(s.doc()?.entity_count(), entities_before);
        Ok(())
    }
}
