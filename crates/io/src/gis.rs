//! GIS vector data → Drawing, as AutoCAD Map's MAPIMPORT brings it in: ESRI Shapefiles (the
//! `.shp` geometry with its `.dbf` attribute table and `.prj` coordinate system) and GeoJSON
//! (RFC 7946).
//!
//! Points become POINTs, lines open polylines, and polygon rings (outer rings and holes) closed
//! polylines without a hatch. Coordinates with varying heights make 3D polylines; measures (M)
//! are dropped. A feature's attributes travel on each of its entities as `GIS` xdata, one
//! `KEY=VALUE` string (group 1000) per attribute. Entities go on layers by what they are
//! (`GIS_POINTS`, `GIS_LINES`, `GIS_ROADS`, `GIS_POLYGONS`, `GIS_PARCELS`, `GIS_BUILDINGS`) or by
//! one attribute's value; polygons with a `name`, `label`, `number`, `code` or `id` attribute get
//! that as a label at their centroid on `GIS_TEXT`. Longitude/latitude data is projected to Web
//! Mercator (EPSG:3857) metres.

mod dbf;
mod geojson;
mod shp;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use cadcraft_color::Color;
use cadcraft_doc::{Common, Drawing, EntityKind, HAlign, LwPolyline, Point, Polyline3d, RawTag, Space, Text, VAlign};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde_json::Value;

use crate::{IoError, Result};

/// The xdata application name the attributes are kept under.
pub const APP: &str = "GIS";

/// The layers features are sorted onto, and the label layer.
pub const POINTS: &str = "GIS_POINTS";
pub const LINES: &str = "GIS_LINES";
pub const ROADS: &str = "GIS_ROADS";
pub const POLYGONS: &str = "GIS_POLYGONS";
pub const PARCELS: &str = "GIS_PARCELS";
pub const BUILDINGS: &str = "GIS_BUILDINGS";
pub const LABELS: &str = "GIS_TEXT";

const LAYER_COLORS: &[(&str, u8)] = &[(POINTS, 1), (LINES, 5), (ROADS, 8), (POLYGONS, 4), (PARCELS, 3), (BUILDINGS, 30), (LABELS, 7)];

const MAX_FEATURES: usize = 2_000_000;
const MAX_TOTAL_VERTS: usize = 8_000_000;
const MAX_XDATA_STRING: usize = 255;
const MAX_XDATA_BYTES: usize = 16_000;
const MAX_LABEL_CHARS: usize = 64;
const MIN_LABEL_HEIGHT: f64 = 0.5;
const MAX_LABEL_HEIGHT: f64 = 25.0;
const LABEL_FIELDS: &[&str] = &["name", "label", "number", "code", "id"];
const EARTH_RADIUS: f64 = 6_378_137.0;
const MAX_MERCATOR_LAT: f64 = 85.051_128_779_806_59;

/// How longitude/latitude coordinates are handled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Projection {
    /// Project when the `.prj` (or GeoJSON `crs`) names a geographic system, or, without one,
    /// when every coordinate lies within ±180° / ±90°.
    #[default]
    Auto,
    /// Always project from longitude/latitude to Web Mercator.
    WebMercator,
    /// Keep the coordinates as they are.
    None,
}

/// How features become entities.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The attribute whose value names each feature's layer (features without it keep the
    /// layer their kind gives them).
    pub layer_field: Option<String>,
    /// Label polygons with their name or number attribute.
    pub labels: bool,
    pub projection: Projection,
}

impl Default for Options {
    fn default() -> Self {
        Options { layer_field: None, labels: true, projection: Projection::Auto }
    }
}

impl Options {
    /// The options of an IMPORTGIS call: `layerField`, `labels` and `projection`
    /// (`auto`, `mercator` or `none`).
    pub fn from_json(p: &Value) -> std::result::Result<Self, String> {
        let mut o = Options::default();
        if let Some(v) = p.get("layerField").filter(|v| !v.is_null()) {
            let f = v.as_str().ok_or("`layerField` must be an attribute name")?.trim();
            o.layer_field = (!f.is_empty()).then(|| f.to_string());
        }
        if let Some(v) = p.get("labels").filter(|v| !v.is_null()) {
            o.labels = v.as_bool().ok_or("`labels` must be true or false")?;
        }
        if let Some(v) = p.get("projection").filter(|v| !v.is_null()) {
            o.projection = match v.as_str().map(|s| s.trim().to_ascii_lowercase()).as_deref() {
                Some("auto") => Projection::Auto,
                Some("mercator" | "webmercator" | "epsg:3857" | "3857") => Projection::WebMercator,
                Some("none") => Projection::None,
                _ => return Err("`projection` must be auto, mercator or none".into()),
            };
        }
        Ok(o)
    }
}

/// Whether `name` is a GIS file IMPORTGIS reads (`.shp`, `.geojson`).
pub fn is_gis_file(name: impl AsRef<std::path::Path>) -> bool {
    matches!(crate::ext(name).as_str(), "shp" | "geojson")
}

/// IMPORTGIS's reader: a shapefile (by its `.shp` name or content) with its `.dbf` and `.prj`
/// when given, or a GeoJSON file; `params` as [`Options::from_json`].
pub fn import(name: &str, main: &[u8], dbf: Option<&[u8]>, prj: Option<&str>, params: &Value) -> std::result::Result<Drawing, String> {
    let opts = Options::from_json(params)?;
    let r = if crate::ext(name) == "shp" || shp::is_shapefile(main) {
        read_shapefile(main, dbf, prj, name, &opts)
    } else {
        read_geojson(main, name, &opts)
    };
    r.map_err(|e| e.to_string())
}

/// A shapefile's geometry (`shp`), attributes (`dbf`, matched to the shapes by record order) and
/// coordinate system (`prj`, WKT). `name` is the file's name, whose stem helps sort features.
pub fn read_shapefile(shp: &[u8], dbf: Option<&[u8]>, prj: Option<&str>, name: &str, opts: &Options) -> Result<Drawing> {
    let mut budget = Budget::new();
    let mut src = shp::read(shp, &mut budget)?;
    match dbf {
        Some(bytes) => {
            let table = dbf::read(bytes)?;
            if table.rows.len() != src.features.len() {
                src.notes.push(format!(
                    "GIS: the attribute table has {} records for {} shapes; they were matched in order",
                    table.rows.len(),
                    src.features.len()
                ));
            }
            for (f, row) in src.features.iter_mut().zip(table.rows) {
                f.attrs = row;
            }
            src.notes.extend(table.notes);
        }
        None => src.notes.push("GIS: no .dbf attribute table was found beside the shapefile, so no attributes were imported".into()),
    }
    src.geographic = prj.and_then(prj_is_geographic);
    build(src, name, opts)
}

/// A GeoJSON document: a FeatureCollection, a Feature or a bare geometry.
pub fn read_geojson(bytes: &[u8], name: &str, opts: &Options) -> Result<Drawing> {
    let mut budget = Budget::new();
    let src = geojson::read(bytes, &mut budget)?;
    build(src, name, opts)
}

/// Whether a `.prj` WKT names a geographic (longitude/latitude) system: `GEOGCS` without
/// `PROJCS` (WKT 1) or `GEOGCRS` without `PROJCRS` (WKT 2). `None` when it names neither.
fn prj_is_geographic(wkt: &str) -> Option<bool> {
    let head = wkt.trim_start().trim_start_matches('\u{feff}').to_ascii_uppercase();
    if head.starts_with("PROJCS") || head.starts_with("PROJCRS") || head.starts_with("PROJECTEDCRS") {
        Some(false)
    } else if head.starts_with("GEOGCS") || head.starts_with("GEOGCRS") || head.starts_with("GEOGRAPHICCRS") {
        Some(true)
    } else {
        None
    }
}

/// Vertices and features left for one file, so a small file can't expand into unbounded
/// geometry.
pub(super) struct Budget {
    verts: usize,
    features: usize,
}

impl Budget {
    fn new() -> Self {
        Budget { verts: MAX_TOTAL_VERTS, features: MAX_FEATURES }
    }

    pub(super) fn take_verts(&mut self, n: usize) -> Result<()> {
        self.verts = self.verts.checked_sub(n).ok_or_else(|| IoError::Format(format!("GIS: more than {MAX_TOTAL_VERTS} coordinates")))?;
        Ok(())
    }

    pub(super) fn take_feature(&mut self) -> Result<()> {
        self.features = self.features.checked_sub(1).ok_or_else(|| IoError::Format(format!("GIS: more than {MAX_FEATURES} features")))?;
        Ok(())
    }
}

/// A polygon: one outer ring and the holes in it, each without the closing vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Polygon {
    pub outer: Vec<Vec3>,
    pub holes: Vec<Vec<Vec3>>,
}

/// One feature's geometry and attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Feature {
    pub points: Vec<Vec3>,
    pub lines: Vec<Vec<Vec3>>,
    pub polygons: Vec<Polygon>,
    pub attrs: Vec<(String, String)>,
}

/// What a reader found: the features, whether its coordinate system is geographic (when the
/// file says) and what it left out.
#[derive(Debug, Default)]
pub(super) struct Source {
    pub features: Vec<Feature>,
    pub geographic: Option<bool>,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Point,
    Line,
    Polygon,
}

fn coords(src: &Source) -> impl Iterator<Item = &Vec3> {
    src.features.iter().flat_map(|f| {
        f.points.iter().chain(f.lines.iter().flatten()).chain(f.polygons.iter().flat_map(|p| p.outer.iter().chain(p.holes.iter().flatten())))
    })
}

fn looks_geographic(src: &Source) -> bool {
    let mut any = false;
    for c in coords(src) {
        if !((-180.0..=180.0).contains(&c.x) && (-90.0..=90.0).contains(&c.y)) {
            return false;
        }
        any = true;
    }
    any
}

fn mercator(c: Vec3) -> Vec3 {
    let lat = c.y.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT).to_radians();
    Vec3::new(EARTH_RADIUS * c.x.to_radians(), EARTH_RADIUS * (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln(), c.z)
}

fn project(src: &mut Source) {
    for f in &mut src.features {
        for c in f.points.iter_mut().chain(f.lines.iter_mut().flatten()) {
            *c = mercator(*c);
        }
        for p in &mut f.polygons {
            for c in p.outer.iter_mut().chain(p.holes.iter_mut().flatten()) {
                *c = mercator(*c);
            }
        }
    }
}

fn mid_latitude(src: &Source) -> f64 {
    let (lo, hi) = coords(src).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c.y), hi.max(c.y)));
    if lo.is_finite() && hi.is_finite() { (lo + hi) / 2.0 } else { 0.0 }
}

fn build(mut src: Source, name: &str, opts: &Options) -> Result<Drawing> {
    let geographic = match opts.projection {
        Projection::WebMercator => true,
        Projection::None => false,
        Projection::Auto => src.geographic.unwrap_or_else(|| looks_geographic(&src)),
    };
    if geographic {
        let lat = mid_latitude(&src).clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT);
        let k = 1.0 / lat.to_radians().cos();
        project(&mut src);
        src.notes.push(format!(
            "GIS: longitude/latitude projected to Web Mercator (EPSG:3857) metres; at {lat:.1}° lengths and areas read {k:.3} and {:.3} times true",
            k * k
        ));
    }
    let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut d = Drawing::new_metric();
    for f in &src.features {
        add_feature(&mut d, f, &stem, opts)?;
    }
    for l in &mut d.layers {
        if let Some((_, c)) = LAYER_COLORS.iter().find(|(n, _)| l.name.eq_ignore_ascii_case(n)) {
            l.color = Color::Index(*c);
        }
    }
    d.read_notes.extend(src.notes);
    Ok(d)
}

fn add(d: &mut Drawing, layer: &str, xdata: &[RawTag], kind: EntityKind) -> Result<()> {
    let common = Common { layer: layer.to_string(), xdata: xdata.to_vec(), ..Common::default() };
    d.add(&Space::Model, common, kind).map(|_| ()).map_err(|e| IoError::Format(format!("GIS: {e}")))
}

fn add_feature(d: &mut Drawing, f: &Feature, stem: &str, opts: &Options) -> Result<()> {
    let xdata = xdata(&f.attrs);
    let by_field = opts.layer_field.as_deref().and_then(|k| attr(&f.attrs, k)).and_then(layer_name);
    let layer = |kind| by_field.clone().unwrap_or_else(|| classify(kind, &f.attrs, stem).to_string());
    if !f.points.is_empty() {
        let l = layer(Kind::Point);
        for p in &f.points {
            add(d, &l, &xdata, EntityKind::Point(Point { p: *p, angle: 0.0 }))?;
        }
    }
    if !f.lines.is_empty() {
        let l = layer(Kind::Line);
        for line in &f.lines {
            if line.len() >= 2 {
                add(d, &l, &xdata, curve(line, false))?;
            }
        }
    }
    if !f.polygons.is_empty() {
        let l = layer(Kind::Polygon);
        for p in &f.polygons {
            for ring in std::iter::once(&p.outer).chain(&p.holes).filter(|r| r.len() >= 3) {
                add(d, &l, &xdata, curve(ring, true))?;
            }
        }
        if opts.labels
            && let Some(text) = label_text(&f.attrs)
            && let Some(t) = label(&f.polygons, text)
        {
            add(d, LABELS, &[], EntityKind::Text(t))?;
        }
    }
    Ok(())
}

/// An open or closed polyline through `pts`: a 2D polyline at their elevation when they share
/// one, else a 3D polyline.
fn curve(pts: &[Vec3], closed: bool) -> EntityKind {
    let z = pts.first().map_or(0.0, |p| p.z);
    if pts.iter().all(|p| p.z == z) {
        let vertices = pts.iter().map(|p| PolyVertex::new(Vec2::new(p.x, p.y))).collect();
        EntityKind::LwPolyline(LwPolyline { vertices, closed, const_width: 0.0, elevation: z, plinegen: false })
    } else {
        EntityKind::Polyline3d(Polyline3d { points: pts.to_vec(), closed })
    }
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

/// A layer name from an attribute value: characters layer names can't hold become `_`.
fn layer_name(v: &str) -> Option<String> {
    let s: String = v.trim().chars().map(|c| if c.is_control() || "<>/\\\":;?*|,=`".contains(c) { '_' } else { c }).take(MAX_XDATA_STRING).collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// The layer a feature of `kind` goes on, from its attributes and the file's name.
fn classify(kind: Kind, attrs: &[(String, String)], stem: &str) -> &'static str {
    let has = |words: &[&str]| {
        let named = |s: &str| {
            let s = s.to_ascii_lowercase();
            words.iter().any(|w| s.contains(w))
        };
        named(stem)
            || attrs.iter().any(|(k, v)| {
                let k = k.to_ascii_lowercase();
                (named(&k) && !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "no" | "false" | "0"))
                    || (matches!(k.as_str(), "type" | "class" | "fclass" | "kind" | "category" | "feature" | "use" | "landuse") && named(v))
            })
    };
    match kind {
        Kind::Point => POINTS,
        Kind::Line if has(&["highway", "road", "street", "motorway"]) => ROADS,
        Kind::Line => LINES,
        Kind::Polygon if has(&["building", "bldg"]) => BUILDINGS,
        Kind::Polygon if has(&["parcel", "cadast", "lot_", "plot"]) => PARCELS,
        Kind::Polygon => POLYGONS,
    }
}

fn clip(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or_default()
}

/// The `GIS` xdata of a feature: one `KEY=VALUE` string per attribute with a value, each at most
/// 255 bytes, up to 16 000 bytes in all.
fn xdata(attrs: &[(String, String)]) -> Vec<RawTag> {
    let mut out = Vec::new();
    let mut total = 0usize;
    for (k, v) in attrs.iter().filter(|(_, v)| !v.is_empty()) {
        let pair: String = format!("{k}={v}").chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let pair = clip(&pair, MAX_XDATA_STRING);
        total += pair.len();
        if total > MAX_XDATA_BYTES {
            break;
        }
        if out.is_empty() {
            out.push(RawTag { code: 1001, value: APP.into() });
        }
        out.push(RawTag { code: 1000, value: pair.to_string() });
    }
    out
}

/// The attributes a GIS entity carries in its `GIS` xdata, in order.
pub fn attributes(xdata: &[RawTag]) -> Vec<(String, String)> {
    let Some(at) = xdata.iter().position(|t| t.code == 1001 && t.value.trim().eq_ignore_ascii_case(APP)) else { return Vec::new() };
    xdata
        .iter()
        .skip(at + 1)
        .take_while(|t| t.code != 1001)
        .filter(|t| t.code == 1000)
        .filter_map(|t| t.value.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}

fn label_text(attrs: &[(String, String)]) -> Option<String> {
    let v = LABEL_FIELDS.iter().find_map(|k| attr(attrs, k).map(str::trim).filter(|v| !v.is_empty()))?;
    let v: String = v.chars().filter(|c| !c.is_control()).take(MAX_LABEL_CHARS).collect();
    (!v.is_empty()).then_some(v)
}

/// Twice the signed area of a ring (counter-clockwise positive).
pub(super) fn area2(ring: &[Vec3]) -> f64 {
    edges(ring).map(|(a, b)| a.x * b.y - b.x * a.y).sum()
}

fn edges(ring: &[Vec3]) -> impl Iterator<Item = (&Vec3, &Vec3)> {
    ring.iter().zip(ring.iter().cycle().skip(1))
}

/// A ring's area (unsigned) and centroid.
fn centroid(ring: &[Vec3]) -> (f64, Vec2) {
    let n = ring.len();
    let a2 = area2(ring);
    if a2.abs() < 1e-300 || n == 0 {
        let k = n.max(1) as f64;
        let (sx, sy) = ring.iter().fold((0.0, 0.0), |(x, y), p| (x + p.x, y + p.y));
        return (0.0, Vec2::new(sx / k, sy / k));
    }
    let (mut cx, mut cy) = (0.0, 0.0);
    for (a, b) in edges(ring) {
        let cross = a.x * b.y - b.x * a.y;
        cx += (a.x + b.x) * cross;
        cy += (a.y + b.y) * cross;
    }
    (a2.abs() / 2.0, Vec2::new(cx / (3.0 * a2), cy / (3.0 * a2)))
}

/// The label of a (multi)polygon: centred on the centroid of its largest polygon (holes
/// subtracted), sized to fit the polygon's bounding box by the text's length.
fn label(polygons: &[Polygon], text: String) -> Option<Text> {
    let p = polygons.iter().filter(|p| p.outer.len() >= 3).max_by(|a, b| area2(&a.outer).abs().total_cmp(&area2(&b.outer).abs()))?;
    let (a, c) = centroid(&p.outer);
    let (mut area, mut mx, mut my) = (a, a * c.x, a * c.y);
    for h in p.holes.iter().filter(|h| h.len() >= 3) {
        let (ha, hc) = centroid(h);
        area -= ha;
        mx -= ha * hc.x;
        my -= ha * hc.y;
    }
    let at = if area > 1e-12 * a.max(1e-300) && (mx / area).is_finite() && (my / area).is_finite() { Vec2::new(mx / area, my / area) } else { c };
    let (lo, hi) = p.outer.iter().fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
        (Vec2::new(lo.x.min(q.x), lo.y.min(q.y)), Vec2::new(hi.x.max(q.x), hi.y.max(q.y)))
    });
    let chars = text.chars().count().max(1) as f64;
    let fit = (0.8 * (hi.x - lo.x) / (0.7 * chars)).min(0.4 * (hi.y - lo.y));
    let height = if fit.is_finite() { fit.clamp(MIN_LABEL_HEIGHT, MAX_LABEL_HEIGHT) } else { MIN_LABEL_HEIGHT };
    let z = p.outer.first().map_or(0.0, |q| q.z);
    let at = Vec3::new(at.x, at.y, z);
    Some(Text {
        insert: at,
        align_pt: Some(at),
        height,
        value: text,
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Center,
        valign: VAlign::Middle,
    })
}

/// Counts of what a reader skipped, reported as one read note.
#[derive(Default)]
pub(super) struct Skipped(BTreeMap<&'static str, usize>);

impl Skipped {
    pub(super) fn add(&mut self, what: &'static str) {
        *self.0.entry(what).or_default() += 1;
    }

    pub(super) fn note(&self, format: &str) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let list: Vec<String> = self.0.iter().map(|(k, n)| format!("{n} {k}")).collect();
        Some(format!("GIS: {format}: left out {}", list.join(", ")))
    }
}
