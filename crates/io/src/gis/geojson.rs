//! GeoJSON (RFC 7946): FeatureCollections, Features and the seven geometry types. A `crs`
//! member (from the 2008 specification, before RFC 7946 fixed WGS 84) says whether the
//! coordinates are longitude/latitude.

use cadcraft_geom::Vec3;
use serde_json::{Map, Value};

use super::{Budget, Feature, Polygon, Skipped, Source};
use crate::{IoError, Result};

const MAX_DEPTH: usize = 16;

fn err(msg: impl Into<String>) -> IoError {
    IoError::Format(format!("GeoJSON: {}", msg.into()))
}

/// Read a GeoJSON document.
pub(super) fn read(bytes: &[u8], budget: &mut Budget) -> Result<Source> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let root: Value = serde_json::from_slice(bytes).map_err(|e| err(e.to_string()))?;
    let obj = root.as_object().ok_or_else(|| err("the document is not a GeoJSON object"))?;
    let mut r = Reader { src: Source::default(), skipped: Skipped::default(), budget };
    r.src.geographic = crs_geographic(obj);
    match obj.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            let features = obj.get("features").and_then(Value::as_array).ok_or_else(|| err("a FeatureCollection needs a `features` array"))?;
            for f in features {
                r.feature(f)?;
            }
        }
        Some("Feature") => r.feature(&root)?,
        Some(_) => {
            let mut f = Feature::default();
            r.geometry(&root, &mut f, 0)?;
            r.push(f)?;
        }
        None => return Err(err("the document has no `type`")),
    }
    let mut src = r.src;
    src.notes.extend(r.skipped.note("GeoJSON"));
    Ok(src)
}

/// Whether a `crs` member names longitude/latitude (`CRS84`, EPSG 4326) or another system;
/// `None` without one.
fn crs_geographic(obj: &Map<String, Value>) -> Option<bool> {
    let name = obj.get("crs")?.get("properties")?.get("name")?.as_str()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    Some(name.contains("CRS84") || name.ends_with(":4326") || name.ends_with("::4326"))
}

fn attr_value(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        other => Some(other.to_string()),
    }
}

fn position(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    let n = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
    Some(Vec3::new(n(0)?, n(1)?, n(2).unwrap_or(0.0)))
}

struct Reader<'a> {
    src: Source,
    skipped: Skipped,
    budget: &'a mut Budget,
}

impl Reader<'_> {
    fn push(&mut self, f: Feature) -> Result<()> {
        self.budget.take_feature()?;
        self.src.features.push(f);
        Ok(())
    }

    fn feature(&mut self, v: &Value) -> Result<()> {
        if v.get("type").and_then(Value::as_str) != Some("Feature") {
            self.skipped.add("members that are not Features");
            return Ok(());
        }
        let mut f = Feature::default();
        if let Some(props) = v.get("properties").and_then(Value::as_object) {
            f.attrs = props.iter().filter_map(|(k, v)| Some((k.clone(), attr_value(v)?))).collect();
        }
        match v.get("geometry") {
            Some(g) if !g.is_null() => self.geometry(g, &mut f, 0)?,
            _ => self.skipped.add("features without geometry"),
        }
        self.push(f)
    }

    fn positions(&mut self, v: &Value) -> Result<Vec<Vec3>> {
        let Some(a) = v.as_array() else {
            self.skipped.add("malformed coordinate lists");
            return Ok(Vec::new());
        };
        self.budget.take_verts(a.len())?;
        let mut out: Vec<Vec3> = Vec::with_capacity(a.len());
        for p in a {
            match position(p) {
                Some(p) if out.last() != Some(&p) => out.push(p),
                Some(_) => {}
                None => self.skipped.add("malformed positions"),
            }
        }
        Ok(out)
    }

    fn line(&mut self, v: &Value, f: &mut Feature) -> Result<()> {
        let pts = self.positions(v)?;
        if pts.len() >= 2 {
            f.lines.push(pts);
        } else {
            self.skipped.add("lines with fewer than two positions");
        }
        Ok(())
    }

    fn ring(&mut self, v: &Value) -> Result<Option<Vec<Vec3>>> {
        let mut pts = self.positions(v)?;
        if pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        Ok((pts.len() >= 3).then_some(pts))
    }

    fn polygon(&mut self, v: &Value, f: &mut Feature) -> Result<()> {
        let Some(rings) = v.as_array() else {
            self.skipped.add("malformed polygons");
            return Ok(());
        };
        let mut it = rings.iter();
        let Some(outer) = it.next() else { return Ok(()) };
        let Some(outer) = self.ring(outer)? else {
            self.skipped.add("polygons with fewer than three positions");
            return Ok(());
        };
        let mut holes = Vec::new();
        for h in it {
            match self.ring(h)? {
                Some(h) => holes.push(h),
                None => self.skipped.add("holes with fewer than three positions"),
            }
        }
        f.polygons.push(Polygon { outer, holes });
        Ok(())
    }

    fn each(&mut self, v: &Value, f: &mut Feature, one: fn(&mut Self, &Value, &mut Feature) -> Result<()>) -> Result<()> {
        match v.as_array() {
            Some(items) => items.iter().try_for_each(|i| one(self, i, f)),
            None => {
                self.skipped.add("malformed coordinate lists");
                Ok(())
            }
        }
    }

    fn geometry(&mut self, g: &Value, f: &mut Feature, depth: usize) -> Result<()> {
        let kind = g.get("type").and_then(Value::as_str).unwrap_or_default();
        if kind == "GeometryCollection" {
            if depth >= MAX_DEPTH {
                return Err(err(format!("geometry collections nested deeper than {MAX_DEPTH}")));
            }
            for g in g.get("geometries").and_then(Value::as_array).into_iter().flatten() {
                self.geometry(g, f, depth + 1)?;
            }
            return Ok(());
        }
        let c = g.get("coordinates").unwrap_or(&Value::Null);
        match kind {
            "Point" => match position(c) {
                Some(p) => {
                    self.budget.take_verts(1)?;
                    f.points.push(p);
                }
                None => self.skipped.add("malformed positions"),
            },
            "MultiPoint" => {
                let pts = self.positions(c)?;
                f.points.extend(pts);
            }
            "LineString" => self.line(c, f)?,
            "MultiLineString" => self.each(c, f, Self::line)?,
            "Polygon" => self.polygon(c, f)?,
            "MultiPolygon" => self.each(c, f, Self::polygon)?,
            _ => self.skipped.add("geometries of unknown types"),
        }
        Ok(())
    }
}
