//! Header (drawing-scoped system) variables.

use std::collections::BTreeMap;

use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// A header variable value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HVal {
    Int(i64),
    Real(f64),
    Str(String),
    Point(Vec3),
}

impl HVal {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            HVal::Int(i) => Some(*i as f64),
            HVal::Real(r) => Some(*r),
            HVal::Str(s) => s.trim().parse().ok(),
            HVal::Point(_) => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            HVal::Int(i) => Some(*i),
            HVal::Real(r) if r.is_finite() => Some(r.round() as i64),
            HVal::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            HVal::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn display(&self) -> String {
        match self {
            HVal::Int(i) => i.to_string(),
            HVal::Real(r) => format!("{r:.4}"),
            HVal::Str(s) => format!("\"{s}\""),
            HVal::Point(p) => format!("{:.4},{:.4},{:.4}", p.x, p.y, p.z),
        }
    }
}

/// Header variables keyed by name without the `$` (e.g. `LTSCALE`), uppercase.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub vars: BTreeMap<String, HVal>,
}

impl Header {
    pub fn get(&self, name: &str) -> Option<&HVal> {
        self.vars.get(&name.trim_start_matches('$').to_ascii_uppercase())
    }
    pub fn set(&mut self, name: &str, v: HVal) {
        self.vars.insert(name.trim_start_matches('$').to_ascii_uppercase(), v);
    }
    pub fn f64(&self, name: &str, default: f64) -> f64 {
        self.get(name).and_then(HVal::as_f64).filter(|v| v.is_finite()).unwrap_or(default)
    }
    pub fn i64(&self, name: &str, default: i64) -> i64 {
        self.get(name).and_then(HVal::as_i64).unwrap_or(default)
    }
    pub fn str(&self, name: &str, default: &str) -> String {
        self.get(name).and_then(HVal::as_str).map(str::to_string).unwrap_or_else(|| default.to_string())
    }
    pub fn point(&self, name: &str) -> Option<Vec3> {
        match self.get(name)? {
            HVal::Point(p) => Some(*p),
            _ => None,
        }
    }
    /// Drawing snap spacing; malformed or missing values use the drafting default.
    pub fn snap_unit(&self) -> Vec2 {
        self.spacing("SNAPUNIT", false)
    }
    /// Drawing grid spacing. A zero component follows the corresponding snap spacing.
    pub fn grid_unit(&self) -> Vec2 {
        self.spacing("GRIDUNIT", true)
    }
    fn spacing(&self, name: &str, allow_zero: bool) -> Vec2 {
        self.point(name)
            .map(|p| p.xy())
            .filter(|p| p.is_finite() && if allow_zero { p.x >= 0.0 && p.y >= 0.0 } else { p.x > 0.0 && p.y > 0.0 })
            .unwrap_or(Vec2::new(0.5, 0.5))
    }
    /// The model view the drawing was saved at: `$VIEWCTR` (centre) and `$VIEWSIZE` (height, in
    /// drawing units), when both are present and usable. A DXF file keeps the pair in HEADER and
    /// repeats it in the `*ACTIVE` VPORT record (12/22 and 40); a reader opens the drawing on it
    /// rather than on the extents, so a drawing saved on one detail of a large sheet opens there.
    pub fn saved_view(&self) -> Option<(Vec2, f64)> {
        let center = self.point("VIEWCTR")?.xy();
        let height = self.get("VIEWSIZE").and_then(HVal::as_f64)?;
        (center.is_finite() && height.is_finite() && height > 1e-9).then_some((center, height))
    }
    /// Record the view the drawing is saved at (see [`Header::saved_view`]). A view that cannot
    /// be shown is not recorded, so a usable stored view is never replaced by an unusable one.
    pub fn set_saved_view(&mut self, center: Vec2, height: f64) {
        if center.is_finite() && height.is_finite() && height > 1e-9 {
            self.set("VIEWCTR", HVal::Point(center.to3(0.0)));
            self.set_f64("VIEWSIZE", height);
        }
    }
    pub fn set_f64(&mut self, name: &str, v: f64) {
        self.set(name, HVal::Real(v));
    }
    pub fn set_i64(&mut self, name: &str, v: i64) {
        self.set(name, HVal::Int(v));
    }
    pub fn set_str(&mut self, name: &str, v: &str) {
        self.set(name, HVal::Str(v.into()));
    }

    fn common() -> Header {
        let mut h = Header::default();
        h.set_str("ACADVER", "AC1032");
        h.set_str("CLAYER", "0");
        h.set_i64("CECOLOR", 256);
        h.set_str("CELTYPE", "ByLayer");
        h.set_i64("CELWEIGHT", -1);
        h.set_f64("CELTSCALE", 1.0);
        h.set_f64("LTSCALE", 1.0);
        h.set_i64("PSLTSCALE", 1);
        h.set_str("TEXTSTYLE", "Standard");
        h.set_str("DIMSTYLE", "Standard");
        h.set_str("CMLEADERSTYLE", "Standard");
        h.set_str("CTABLESTYLE", "Standard");
        h.set_i64("LUNITS", 2);
        h.set_i64("LUPREC", 4);
        h.set_i64("AUNITS", 0);
        h.set_i64("AUPREC", 0);
        h.set_f64("ANGBASE", 0.0);
        h.set_i64("ANGDIR", 0);
        h.set_i64("PDMODE", 0);
        h.set_f64("PDSIZE", 0.0);
        h.set_i64("FILLMODE", 1);
        h.set_i64("MIRRTEXT", 0);
        h.set_i64("ATTMODE", 1);
        h.set_f64("THICKNESS", 0.0);
        h.set_f64("ELEVATION", 0.0);
        h.set_i64("LWDISPLAY", 0);
        h.set("INSBASE", HVal::Point(Vec3::ZERO));
        // Snap grid origin and rotation (SNAPANG in degrees).
        h.set("SNAPBASE", HVal::Point(Vec3::ZERO));
        h.set_f64("SNAPANG", 0.0);
        h.set("SNAPUNIT", HVal::Point(Vec3::new(0.5, 0.5, 0.0)));
        h.set("GRIDUNIT", HVal::Point(Vec3::new(0.5, 0.5, 0.0)));
        // Annotation scale of model space; annotative objects of other scales show (1) or not.
        h.set_str("CANNOSCALE", "1:1");
        h.set_i64("ANNOALLVISIBLE", 1);
        h
    }

    pub fn imperial() -> Header {
        let mut h = Header::common();
        h.set_i64("MEASUREMENT", 0);
        h.set_i64("INSUNITS", 1);
        h.set_f64("TEXTSIZE", 0.2);
        h.set_f64("DIMSCALE", 1.0);
        h.set_f64("FILLETRAD", 0.0);
        h.set_f64("CHAMFERA", 0.0);
        h.set_f64("CHAMFERB", 0.0);
        h.set("LIMMIN", HVal::Point(Vec3::ZERO));
        h.set("LIMMAX", HVal::Point(Vec3::new(12.0, 9.0, 0.0)));
        h
    }

    pub fn metric() -> Header {
        let mut h = Header::common();
        h.set_i64("MEASUREMENT", 1);
        h.set_i64("INSUNITS", 4);
        h.set_f64("TEXTSIZE", 2.5);
        h.set_f64("DIMSCALE", 1.0);
        h.set_f64("FILLETRAD", 0.0);
        h.set("LIMMIN", HVal::Point(Vec3::ZERO));
        h.set("LIMMAX", HVal::Point(Vec3::new(420.0, 297.0, 0.0)));
        h
    }
}
