//! PEDIT Edit vertex: Break and Straighten between two marked vertices, Tangent directions
//! that Fit honours, and Regen.

use super::super::point_value;
use super::{
    EntityKind, Handle, Input, LwPolyline, PPhase, PeditM, PolyVertex, Result, Session, Value, Vec2, bad, curves, json, other, push_entity, set_kind,
};

fn ordered(pl: &LwPolyline, i: usize, j: usize) -> Result<(usize, usize)> {
    let n = pl.vertices.len();
    if i >= n || j >= n {
        return Err(bad("pedit", "vertex index out of range"));
    }
    Ok((i.min(j), i.max(j)))
}

/// Straighten: drop the vertices between `i` and `j` and make the segment between them
/// straight (the same vertex twice straightens the segment that follows it).
pub(super) fn straighten(pl: &mut LwPolyline, i: usize, j: usize) -> Result<()> {
    let (lo, hi) = ordered(pl, i, j)?;
    if hi > lo + 1 {
        pl.vertices.drain(lo + 1..hi);
    }
    if let Some(v) = pl.vertices.get_mut(lo) {
        v.bulge = 0.0;
    }
    Ok(())
}

/// Break: remove the segments between vertices `i` and `j`. An open polyline splits in two (a
/// piece with fewer than two vertices is dropped); a closed one opens, keeping the part from the
/// later vertex round to the earlier one. Returns the handles left (the first is `h`).
pub(super) fn break_poly(s: &mut Session, h: Handle, pl: &LwPolyline, i: usize, j: usize) -> Result<Vec<Handle>> {
    if curves::is_locked(s, h) {
        return Err(other("The object is on a locked layer."));
    }
    let (lo, hi) = ordered(pl, i, j)?;
    let n = pl.vertices.len();
    let open = |mut vs: Vec<PolyVertex>| {
        if let Some(v) = vs.last_mut() {
            *v = PolyVertex::new(v.p);
        }
        LwPolyline { vertices: vs, closed: false, ..pl.clone() }
    };
    let pieces: Vec<LwPolyline> = if pl.closed {
        let vs = pl.vertices.get(hi..).unwrap_or_default().iter().chain(pl.vertices.get(..=lo).unwrap_or_default()).copied().collect();
        vec![open(vs)]
    } else {
        if lo == hi && (lo == 0 || lo + 1 == n) {
            return Err(other("Cannot break a polyline at its end vertex."));
        }
        [pl.vertices.get(..=lo), pl.vertices.get(hi..)].into_iter().flatten().filter(|v| v.len() >= 2).map(|v| open(v.to_vec())).collect()
    };
    let mut it = pieces.into_iter();
    let first = it.next().ok_or_else(|| other("Breaking there would remove the whole polyline."))?;
    let e = curves::entity(s, h)?;
    let space = s.doc()?.space_of(h).ok_or_else(|| other("No such object."))?;
    set_kind(s, h, EntityKind::LwPolyline(first))?;
    let mut out = vec![h];
    for p in it {
        out.push(push_entity(s, &space, e.common.clone(), EntityKind::LwPolyline(p))?);
    }
    Ok(out)
}

/// Tangent directions for Fit from `tangents: [{index, angle (degrees) | direction: [dx, dy]}]`,
/// one entry per vertex (`None` = the direction Fit works out itself).
pub(super) fn tangent_param(p: &Value, n: usize) -> Result<Vec<Option<Vec2>>> {
    let mut out = vec![None; n];
    let Some(list) = p.get("tangents").filter(|v| !v.is_null()) else { return Ok(out) };
    let list = list.as_array().ok_or_else(|| bad("pedit", "`tangents` must be an array of {index, angle | direction}"))?;
    for t in list {
        let i = t.get("index").and_then(Value::as_u64).ok_or_else(|| bad("pedit", "each tangent needs an `index`"))? as usize;
        let d = match (t.get("direction").and_then(point_value), t.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite())) {
            (Some(d), _) => d,
            (None, Some(a)) => Vec2::from_angle(a.to_radians()),
            _ => return Err(bad("pedit", "each tangent needs an `angle` (degrees) or a `direction`")),
        };
        if d.len() <= 1e-12 {
            return Err(bad("pedit", "a tangent direction can't be zero"));
        }
        *out.get_mut(i).ok_or_else(|| bad("pedit", "tangent vertex index out of range"))? = Some(d.normalized());
    }
    Ok(out)
}

impl PeditM {
    /// Next / Previous / Go / eXit with the second marker of Break and Straighten.
    pub(super) fn mark_input(&mut self, s: &mut Session, k: &str) -> Result<()> {
        let n = self.poly(s).map(|p| p.vertices.len()).unwrap_or(0).max(1);
        match k {
            "next" | "n" => self.mark2 = (self.mark2 + 1) % n,
            "previous" | "p" => self.mark2 = (self.mark2 + n - 1) % n,
            "go" | "g" if self.breaking => {
                let h = self.h.ok_or_else(|| other("No polyline."))?;
                let pl = self.poly(s).ok_or_else(|| other("No polyline."))?;
                self.snapshot(s);
                match break_poly(s, h, &pl, self.vertex, self.mark2) {
                    // Undoing the break erases the piece split off again.
                    Ok(hs) => self.split_off.push((self.history.len(), hs.into_iter().skip(1).collect())),
                    Err(e) => {
                        self.history.pop();
                        return Err(e);
                    }
                }
                let len = self.poly(s).map(|p| p.vertices.len()).unwrap_or(1);
                self.vertex = self.vertex.min(self.mark2).min(len.saturating_sub(1));
                self.phase = PPhase::Vertex;
            }
            "go" | "g" => {
                self.apply(s, "vertex", json!({ "action": "straighten", "index": self.vertex, "index2": self.mark2 }))?;
                let len = self.poly(s).map(|p| p.vertices.len()).unwrap_or(1);
                self.vertex = self.vertex.min(self.mark2).min(len.saturating_sub(1));
                self.phase = PPhase::Vertex;
            }
            "exit" | "x" => self.phase = PPhase::Vertex,
            "" => {}
            _ => return Err(other("Invalid option keyword.")),
        }
        Ok(())
    }

    /// A tangent direction for the marked vertex: a point (direction from the vertex) or an angle.
    pub(super) fn tangent_input(&mut self, s: &mut Session, i: &Input) -> Result<()> {
        let v = self.poly(s).and_then(|p| p.vertices.get(self.vertex).map(|v| v.p)).ok_or_else(|| other("No vertex."))?;
        let d = match i {
            Input::Point(p) => *p - v,
            Input::Text(t) => Vec2::from_angle(s.angle_settings().direction(t).ok_or_else(|| other("Requires a point or an angle."))?),
            Input::Enter => {
                self.phase = PPhase::Vertex;
                return Ok(());
            }
            _ => return Ok(()),
        };
        if d.len() <= 1e-12 {
            return Err(other("Specify a point away from the vertex."));
        }
        self.tangents.retain(|(p, _)| !p.near(v, 1e-9));
        self.tangents.push((v, d.normalized()));
        self.phase = PPhase::Vertex;
        Ok(())
    }

    /// The tangents set in this session for the polyline's current vertices (JSON `tangents`).
    pub(super) fn tangents_json(&self, s: &Session) -> Value {
        let Some(pl) = self.poly(s) else { return json!([]) };
        let list: Vec<Value> = pl
            .vertices
            .iter()
            .enumerate()
            .filter_map(|(i, v)| self.tangents.iter().find(|(p, _)| p.near(v.p, 1e-9)).map(|(_, d)| json!({ "index": i, "direction": [d.x, d.y] })))
            .collect();
        Value::Array(list)
    }

    /// Undo of a Break also erases the pieces it split off.
    pub(super) fn undo_split(&mut self, s: &mut Session) -> Result<()> {
        if let Some((at, hs)) = self.split_off.last()
            && *at == self.history.len()
        {
            let d = s.doc_mut()?;
            for h in hs {
                d.remove_entity(*h);
            }
            self.split_off.pop();
        }
        Ok(())
    }
}
