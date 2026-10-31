//! `properties.set`: change properties of the selected objects, the way the Properties palette
//! edits them. Keys that no selected object takes come back in `ignored`.

use std::collections::{BTreeSet, HashMap};

use cadcraft_color::Color;
use cadcraft_doc::{
    AnnoScale, Arc, Circle, DimStyle, Dimension, Drawing, Entity, EntityKind, HAlign, Handle, Hatch, Insert, Line, Lineweight, MText, Text,
    Transparency, VAlign,
};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::{parse_color, parse_transparency};
use crate::cmd::{bad, point_value, str_param, targets};
use crate::properties::{LINE_PITCH, is_dim_linetype, join_post, split_post};
use crate::{Result, Session};

fn set_xy(p: &mut Vec3, v: &Value) {
    if let Some(q) = point_value(v) {
        p.x = q.x;
        p.y = q.y;
    }
}

/// Keys `properties.set` handles itself; the rest that name a dimension style variable (a field
/// such as `textHeight` or a DIM* name such as `DIMTXT`) set per-dimension overrides.
const SET_KEYS: &[&str] = &[
    "handles",
    "handle",
    "layer",
    "color",
    "linetype",
    "lineweight",
    "ltscale",
    "thickness",
    "transparency",
    "visible",
    "annotative",
    "dimStyle",
    "textOverride",
    "textRotation",
    "textPositionX",
    "textPositionY",
    "clearOverrides",
    "clear",
    "prefix",
    "suffix",
    "altPrefix",
    "altSuffix",
    // An insert's or hatch's scale; a dimension's overall scale is `dimScale` (or DIMSCALE).
    "scale",
];

/// The dimension style variables of a call, checked once: canonical field names with their
/// values, and the keys they were given as. Unusable values are left out (and so `ignored`).
struct DimVars {
    fields: Vec<(&'static str, Value)>,
    keys: Vec<String>,
}

impl DimVars {
    fn new(d: &Drawing, p: &Value) -> Result<Self> {
        // Text height, arrow size, overall scale (`dimScale`) and precision are refused, not
        // ignored, when unusable, and the last two have names of their own (#1473).
        let checked = super::dim_overrides(p)?;
        let mut given = serde_json::Map::new();
        for (k, v) in p.as_object().into_iter().flatten() {
            if SET_KEYS.contains(&k.as_str()) {
                continue;
            }
            let Some(field) = DimStyle::field_name(k).filter(|f| !matches!(*f, "name" | "annotative")) else { continue };
            let v = match (field, v.as_str()) {
                // A text style or linetype that doesn't exist would fall back without a word.
                ("textStyle", Some(n)) => {
                    json!(d.text_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("text style `{n}` not found")))?)
                }
                (f, Some(n)) if is_dim_linetype(f) => {
                    if n.is_empty() || n.eq_ignore_ascii_case("ByBlock") {
                        json!("")
                    } else {
                        json!(d.linetype(n).map(|t| t.name.clone()).ok_or_else(|| bad("properties.set", format!("linetype `{n}` is not loaded")))?)
                    }
                }
                // Closed filled is `""` among the arrowheads, but an empty per-end arrowhead draws
                // DIMBLK's: chosen for one end, it's named.
                ("arrowBlock1" | "arrowBlock2", Some("")) => json!("_CLOSEDFILLED"),
                _ => v.clone(),
            };
            given.insert(k.clone(), v);
        }
        let rejected = DimStyle::default().apply_fields(&given);
        let mut out = DimVars { fields: Vec::new(), keys: Vec::new() };
        for (k, v) in given {
            if rejected.contains(&k) {
                continue;
            }
            if let Some(f) = DimStyle::field_name(&k) {
                out.fields.push((f, v));
                out.keys.push(k);
            }
        }
        for (key, field) in [("dimScale", "scale"), ("precision", "decimals")] {
            if let Some(v) = checked.get(field) {
                out.fields.push((field, v.clone()));
                out.keys.push(key.to_string());
            }
        }
        Ok(out)
    }
}

/// What some edits need to know about objects before they change.
#[derive(Default)]
struct Prepared {
    /// TEXT made aligned or fit: where its baseline ends (its width along its rotation).
    ends: HashMap<Handle, Vec2>,
    /// Dimensions: where their text is drawn now.
    text_pos: HashMap<Handle, Vec2>,
    /// The DIMPOST and DIMAPOST templates of each dimension style (by lowercase name), when a
    /// prefix or suffix is edited: a dimension's are worked out after its other edits.
    posts: Option<HashMap<String, (String, String)>>,
}

impl Prepared {
    fn new(d: &Drawing, p: &Value, hs: &[Handle]) -> Self {
        let mut out = Prepared::default();
        let two_points = str_param(p, "justify").and_then(crate::cmd::draw::justify).is_some_and(|(h, _)| matches!(h, HAlign::Aligned | HAlign::Fit));
        let moves_text = p.get("textPositionX").is_some() || p.get("textPositionY").is_some();
        let posts = ["prefix", "suffix", "altPrefix", "altSuffix"].iter().any(|k| p.get(*k).is_some());
        if !(two_points || moves_text || posts) {
            return out;
        }
        for h in hs {
            let Some(e) = d.entity(*h) else { continue };
            match &e.kind {
                EntityKind::Text(t) if two_points => {
                    // The text as this call leaves it: its contents, style, height, width factor
                    // and rotation, where the call sets them too.
                    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
                    let style = str_param(p, "style").and_then(|n| d.text_style(n)).map_or(t.style.as_str(), |s| s.name.as_str());
                    let value = str_param(p, "text").unwrap_or(&t.value);
                    let height = num("height").filter(|x| *x > 0.0).unwrap_or(t.height);
                    let width_factor = num("widthFactor").filter(|x| *x > 0.0).unwrap_or(t.width_factor);
                    let rotation = num("rotation").map_or(t.rotation, f64::to_radians);
                    let font = cadcraft_fonts::TextFont::resolve(&d.text_style(style).cloned().unwrap_or_default().font);
                    let w = cadcraft_fonts::text_width(&font, value, height, width_factor);
                    let w = if w.is_finite() && w > 1e-9 { w } else { height.max(1e-9) };
                    out.ends.insert(*h, Vec2::polar(t.justify_point(), w, rotation));
                }
                EntityKind::Dimension(dm) if moves_text => {
                    out.text_pos.insert(*h, cadcraft_render::dimension_in(d, dm).text_pos);
                }
                _ => {}
            }
        }
        if posts {
            out.posts = Some(d.dim_styles.iter().map(|st| (st.name.to_ascii_lowercase(), (st.post.clone(), st.alt_post.clone()))).collect());
        }
        out
    }
}

/// Give TEXT a justification. It keeps the point it is placed by, so the text moves (as
/// AutoCAD's Properties Justify does); aligned and fit text then ends at `end`.
fn set_justify(t: &mut Text, h: HAlign, v: VAlign, end: Option<Vec2>) {
    let at = t.justify_point();
    let z = t.insert.z;
    let was_two_points = matches!(t.halign, HAlign::Aligned | HAlign::Fit) && t.align_pt.is_some();
    let two_points = matches!(h, HAlign::Aligned | HAlign::Fit);
    t.halign = h;
    t.valign = if two_points { VAlign::Baseline } else { v };
    if two_points {
        // Between aligned and fit the two points stay.
        if !was_two_points {
            t.insert = at.to3(z);
            t.align_pt = Some(end.unwrap_or_else(|| Vec2::polar(at, t.height.max(1e-9), t.rotation)).to3(z));
        }
    } else if (h, v) == (HAlign::Left, VAlign::Baseline) {
        t.insert = at.to3(z);
        t.align_pt = None;
    } else {
        t.insert = at.to3(z);
        t.align_pt = Some(at.to3(z));
    }
}

/// Move TEXT by `d` (both of its points).
fn shift(t: &mut Text, d: Vec2) {
    for q in std::iter::once(&mut t.insert).chain(t.align_pt.as_mut()) {
        q.x += d.x;
        q.y += d.y;
    }
}

/// Set a per-dimension override, replacing the same variable under another spelling (a DIM*
/// name read from a file).
fn set_override(dm: &mut cadcraft_doc::Dimension, field: &'static str, v: Value) {
    dm.overrides.retain(|k, _| DimStyle::field_name(k) != Some(field));
    dm.overrides.insert(field.to_string(), v);
}

/// A `properties.set` call, checked before any object changes: the values every object takes,
/// and what the edits need to know beforehand.
struct Call<'a> {
    p: &'a Value,
    color: Option<Color>,
    transparency: Option<Transparency>,
    thickness: Option<f64>,
    lineweight: Option<Lineweight>,
    linetype: Option<String>,
    dim_style: Option<String>,
    text_style: Option<String>,
    dim_vars: DimVars,
    prep: Prepared,
    anno_scale: AnnoScale,
}

/// The keys some object took.
type Used<'c> = BTreeSet<&'c str>;

impl Call<'_> {
    /// A finite number under `k`.
    fn num(&self, k: &str) -> Option<f64> {
        self.p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite())
    }
}

pub(super) fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("properties.set", "nothing selected"));
    }
    let color = p.get("color").map(|c| parse_color(c).ok_or_else(|| bad("properties.set", "bad colour"))).transpose()?;
    let transparency = p
        .get("transparency")
        .map(|v| parse_transparency(v).ok_or_else(|| bad("properties.set", "transparency must be ByLayer, ByBlock or a percentage from 0 to 90")))
        .transpose()?;
    let thickness = p
        .get("thickness")
        .map(|v| v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad("properties.set", "thickness must be a finite number")))
        .transpose()?;
    let lineweight =
        p.get("lineweight").map(|v| crate::cmd::layer::parse_lw(v).ok_or_else(|| bad("properties.set", "bad lineweight"))).transpose()?;
    let d = s.doc_mut()?;
    // An explicit object linetype must be ByLayer, ByBlock or a loaded definition (as `linetype current` requires).
    let linetype = str_param(p, "linetype")
        .map(|n| {
            if n.eq_ignore_ascii_case("ByLayer") || n.eq_ignore_ascii_case("ByBlock") {
                Ok(n.to_string())
            } else {
                d.linetype(n).map(|t| t.name.clone()).ok_or_else(|| bad("properties.set", format!("linetype `{n}` is not loaded")))
            }
        })
        .transpose()?;
    let dim_style = str_param(p, "dimStyle")
        .map(|n| d.dim_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("dimension style `{n}` not found"))))
        .transpose()?;
    let text_style = str_param(p, "style")
        .map(|n| d.text_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("text style `{n}` not found"))))
        .transpose()?;
    let dim_vars = DimVars::new(d, p)?;
    if let Some(h) = hs.iter().find(|h| d.entity(**h).is_none()) {
        return Err(bad("properties.set", format!("no object {}", h.hex())));
    }
    // A pattern must be one each hatch can take, as HATCHEDIT refuses others.
    if let Some(v) = str_param(p, "pattern").filter(|v| !v.trim().is_empty())
        && hs.iter().any(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(x)) if !crate::cmd::hatch::takes_pattern(x, v)))
    {
        return Err(bad("properties.set", format!("unknown pattern {}", v.trim().to_ascii_uppercase())));
    }
    // A failed call changes nothing (a command's error doesn't undo what it did), so the layer
    // comes after every check, and every object is there before any is edited.
    if let Some(l) = str_param(p, "layer") {
        d.ensure_layer(l);
    }
    let prep = Prepared::new(d, p, &hs);
    let anno_scale = d.current_anno_scale();
    let c = Call { p, color, transparency, thickness, lineweight, linetype, dim_style, text_style, dim_vars, prep, anno_scale };
    let mut used = Used::new();
    for h in &hs {
        let h = *h;
        d.modify_entity(h, |e| {
            common(e, &c, &mut used);
            match &mut e.kind {
                EntityKind::Line(l) => line(l, &c, &mut used),
                EntityKind::Circle(ci) => circle(ci, &c, &mut used),
                EntityKind::Arc(a) => arc(a, &c, &mut used),
                EntityKind::Text(t) => {
                    text(t, &c, h, &mut used);
                    text_position(t, &c, &mut used);
                }
                EntityKind::MText(t) => mtext(t, &c, &mut used),
                EntityKind::LwPolyline(pl) => {
                    if let Some(x) = c.num("width").filter(|x| *x >= 0.0) {
                        pl.const_width = x;
                        used.insert("width");
                    }
                    if let Some(closed) = p.get("closed").and_then(Value::as_bool) {
                        pl.closed = closed;
                        used.insert("closed");
                    }
                }
                EntityKind::Insert(i) => insert(i, &c, &mut used),
                EntityKind::Dimension(dm) => dimension(dm, &c, h, &mut used),
                EntityKind::Hatch(ha) => hatch(ha, &c, &mut used),
                _ => {}
            }
        })?;
    }
    let ignored: Vec<&str> = p
        .as_object()
        .map(|o| o.keys().map(String::as_str).filter(|k| !matches!(*k, "handles" | "handle") && !used.contains(k)).collect())
        .unwrap_or_default();
    Ok(json!({ "changed": hs.len(), "ignored": ignored }))
}

/// The properties every object has: annotative, layer, color, linetype, lineweight, linetype
/// scale, transparency, thickness and visibility.
fn common<'c>(e: &mut Entity, c: &'c Call<'_>, used: &mut Used<'c>) {
    let p = c.p;
    // Annotative: Yes supports the current annotation scale; No drops every scale.
    if let Some(a) = p.get("annotative").and_then(Value::as_bool)
        && cadcraft_doc::can_be_annotative(&e.kind)
    {
        cadcraft_doc::set_annotative(e, a, &c.anno_scale);
        if let EntityKind::Dimension(dm) = &mut e.kind {
            dm.block = None;
        }
        used.insert("annotative");
    }
    if let Some(l) = str_param(p, "layer") {
        e.common.layer = l.to_string();
        used.insert("layer");
    }
    if let Some(color) = c.color {
        e.common.color = color;
        used.insert("color");
    }
    if let Some(lt) = &c.linetype {
        e.common.linetype.clone_from(lt);
        used.insert("linetype");
    }
    if let Some(w) = c.lineweight {
        e.common.lineweight = w;
        used.insert("lineweight");
    }
    if let Some(x) = p.get("ltscale").and_then(Value::as_f64).filter(|x| *x > 0.0 && x.is_finite()) {
        e.common.ltscale = x;
        used.insert("ltscale");
    }
    if let Some(t) = c.transparency {
        e.common.transparency = t;
        used.insert("transparency");
    }
    if let Some(th) = c.thickness {
        e.common.thickness = th;
        used.insert("thickness");
    }
    if let Some(v) = p.get("visible").and_then(Value::as_bool) {
        e.common.visible = v;
        used.insert("visible");
    }
}

fn line(l: &mut Line, c: &Call<'_>, used: &mut Used<'_>) {
    if let Some(v) = c.p.get("start") {
        set_xy(&mut l.a, v);
        used.insert("start");
    }
    if let Some(v) = c.p.get("end") {
        set_xy(&mut l.b, v);
        used.insert("end");
    }
    // Length / angle keep the start point and move the end point.
    let (a, b) = (l.a.xy(), l.b.xy());
    let length = c.num("length").filter(|x| *x > 0.0 && *x < 1e15);
    let len = length.unwrap_or_else(|| a.dist(b));
    let ang = c.num("angle").map(f64::to_radians).unwrap_or_else(|| a.angle_to(b));
    if length.is_some() || c.num("angle").is_some() {
        let e = Vec2::polar(a, len, ang);
        if e.is_finite() {
            l.b.x = e.x;
            l.b.y = e.y;
            if length.is_some() {
                used.insert("length");
            }
            if c.num("angle").is_some() {
                used.insert("angle");
            }
        }
    }
}

fn circle(ci: &mut Circle, c: &Call<'_>, used: &mut Used<'_>) {
    if let Some(v) = c.p.get("center") {
        set_xy(&mut ci.center, v);
        used.insert("center");
    }
    if let Some(r) = c.num("radius").filter(|r| *r > 0.0) {
        ci.radius = r;
        used.insert("radius");
    }
    if let Some(dm) = c.num("diameter").filter(|r| *r > 0.0) {
        ci.radius = dm / 2.0;
        used.insert("diameter");
    }
    if let Some(cf) = c.num("circumference").filter(|x| *x > 0.0) {
        ci.radius = cf / cadcraft_geom::TAU;
        used.insert("circumference");
    }
    if let Some(a) = c.num("area").filter(|x| *x > 0.0) {
        ci.radius = (a / cadcraft_geom::PI).sqrt();
        used.insert("area");
    }
}

fn arc(a: &mut Arc, c: &Call<'_>, used: &mut Used<'_>) {
    if let Some(v) = c.p.get("center") {
        set_xy(&mut a.center, v);
        used.insert("center");
    }
    if let Some(r) = c.num("radius").filter(|r| *r > 0.0) {
        a.radius = r;
        used.insert("radius");
    }
    if let Some(x) = c.num("startAngle") {
        a.start = cadcraft_geom::norm_angle(x.to_radians());
        used.insert("startAngle");
    }
    if let Some(x) = c.num("endAngle") {
        a.end = cadcraft_geom::norm_angle(x.to_radians());
        used.insert("endAngle");
    }
}

/// TEXT's contents, style, size, angles and justification.
fn text(t: &mut Text, c: &Call<'_>, h: Handle, used: &mut Used<'_>) {
    let p = c.p;
    if let Some(v) = str_param(p, "text") {
        t.value = v.to_string();
        used.insert("text");
    }
    if let Some(st) = &c.text_style {
        t.style.clone_from(st);
        used.insert("style");
    }
    if let Some(x) = c.num("height").filter(|x| *x > 0.0) {
        t.height = x;
        used.insert("height");
    }
    if let Some(x) = c.num("rotation") {
        t.rotation = x.to_radians();
        // Aligned and fit text run along their baseline's two ends: it turns too.
        if matches!(t.halign, HAlign::Aligned | HAlign::Fit)
            && let Some(end) = t.align_pt
        {
            let e = Vec2::polar(t.insert.xy(), t.insert.xy().dist(end.xy()), t.rotation);
            t.align_pt = Some(Vec3::new(e.x, e.y, end.z));
        }
        used.insert("rotation");
    }
    if let Some(x) = c.num("widthFactor").filter(|x| *x > 0.0) {
        t.width_factor = x;
        used.insert("widthFactor");
    }
    if let Some(x) = c.num("oblique").filter(|x| x.abs() <= 85.0) {
        t.oblique = x.to_radians();
        used.insert("oblique");
    }
    if let Some((ha, va)) = str_param(p, "justify").and_then(crate::cmd::draw::justify) {
        set_justify(t, ha, va, c.prep.ends.get(&h).copied());
        used.insert("justify");
    }
}

/// Where TEXT is: its position (the point it's placed by), and an aligned or fit text's second
/// point.
fn text_position(t: &mut Text, c: &Call<'_>, used: &mut Used<'_>) {
    // The position is the point the text is placed by: moving it moves the text.
    if let Some(to) = c.p.get("position").and_then(point_value) {
        shift(t, to - t.justify_point());
        used.insert("position");
    }
    let (x, y) = (c.num("positionX"), c.num("positionY"));
    if x.is_some() || y.is_some() {
        let at = t.justify_point();
        shift(t, Vec2::new(x.unwrap_or(at.x), y.unwrap_or(at.y)) - at);
        used.extend(["positionX", "positionY"].into_iter().filter(|k| c.num(k).is_some()));
    }
    if let Some(z) = c.num("positionZ") {
        t.insert.z = z;
        if let Some(a) = &mut t.align_pt {
            a.z = z;
        }
        used.insert("positionZ");
    }
    // Aligned and fit text: the second point alone (the text refits between them).
    if matches!(t.halign, HAlign::Aligned | HAlign::Fit)
        && let Some(a) = &mut t.align_pt
    {
        if let Some(x) = c.num("alignmentX") {
            a.x = x;
            used.insert("alignmentX");
        }
        if let Some(y) = c.num("alignmentY") {
            a.y = y;
            used.insert("alignmentY");
        }
    }
}

fn mtext(t: &mut MText, c: &Call<'_>, used: &mut Used<'_>) {
    let p = c.p;
    if let Some(v) = str_param(p, "text") {
        t.contents = v.replace('\n', "\\P");
        used.insert("text");
    }
    if let Some(st) = &c.text_style {
        t.style.clone_from(st);
        used.insert("style");
    }
    if let Some(x) = c.num("height").filter(|x| *x > 0.0) {
        t.height = x;
        used.insert("height");
    }
    if let Some(x) = c.num("width").filter(|x| *x >= 0.0) {
        t.width = x;
        used.insert("width");
    }
    if let Some(x) = c.num("rotation") {
        t.rotation = x.to_radians();
        used.insert("rotation");
    }
    // The attachment point stays where it is; the text moves around it.
    if let Some(n) = str_param(p, "justify").and_then(crate::cmd::mtext_opts::attach_code) {
        t.attach = n;
        used.insert("justify");
    }
    mtext_spacing(t, c, used);
    if let Some(to) = p.get("position").and_then(point_value) {
        t.insert.x = to.x;
        t.insert.y = to.y;
        used.insert("position");
    }
    for (k, slot) in [("positionX", &mut t.insert.x), ("positionY", &mut t.insert.y), ("positionZ", &mut t.insert.z)] {
        if let Some(v) = c.num(k) {
            *slot = v;
            used.insert(k);
        }
    }
}

/// MTEXT's line spacing: a factor, a distance, or its style.
fn mtext_spacing(t: &mut MText, c: &Call<'_>, used: &mut Used<'_>) {
    // MTEXT's Line spacing option takes factors from 0.25 to 4.
    let factor = |f: f64| f.is_finite() && (0.25 - 1e-9..=4.0 + 1e-9).contains(&f);
    if let Some(x) = c.num("lineSpacing").filter(|x| factor(*x)) {
        t.line_spacing = x;
        used.insert("lineSpacing");
    }
    if let Some(f) = c.num("lineSpacingDistance").map(|x| x / (t.height * LINE_PITCH)).filter(|f| factor(*f)) {
        t.line_spacing = f;
        used.insert("lineSpacingDistance");
    }
    // "atLeast", "At least", "exactly"…
    let spacing = str_param(c.p, "lineSpacingStyle").map(|v| v.chars().filter(char::is_ascii_alphabetic).collect::<String>().to_ascii_lowercase());
    if let Some(exact) = match spacing.as_deref() {
        Some("atleast") => Some(false),
        Some("exactly") => Some(true),
        _ => None,
    } {
        t.line_spacing_exact = exact;
        used.insert("lineSpacingStyle");
    }
}

fn insert(i: &mut Insert, c: &Call<'_>, used: &mut Used<'_>) {
    if let Some(x) = c.num("rotation") {
        let turn = x.to_radians() - i.rotation;
        i.rotation = x.to_radians();
        // The attached text turns with the reference, around its insertion point (as ROTATE does).
        let base = i.insert.xy();
        for a in &mut i.attribs {
            let t = &mut a.text;
            for q in std::iter::once(&mut t.insert).chain(t.align_pt.as_mut()) {
                let r = q.xy().rotate_about(base, turn);
                q.x = r.x;
                q.y = r.y;
            }
            t.rotation = cadcraft_geom::norm_angle(t.rotation + turn);
            if let Some(ml) = &mut a.multiline {
                let r = ml.mtext.insert.xy().rotate_about(base, turn);
                ml.mtext.insert.x = r.x;
                ml.mtext.insert.y = r.y;
                ml.mtext.rotation = cadcraft_geom::norm_angle(ml.mtext.rotation + turn);
            }
        }
        used.insert("rotation");
    }
    if let Some(x) = c.num("scale").filter(|x| *x != 0.0) {
        // A new positive scale carries the attached attribute text with it, like SCALE about the insertion point.
        if x > 0.0 && i.scale.x > 0.0 {
            let m = cadcraft_geom::Mat3::scale_about(i.insert.xy(), x / i.scale.x);
            for a in &mut i.attribs {
                let mut k = EntityKind::Text(a.text.clone());
                k.transform(&m);
                if let EntityKind::Text(t) = k {
                    a.text = t;
                }
                a.transform_multiline(&m);
            }
        }
        i.scale = Vec3::new(x, x, x);
        used.insert("scale");
    }
}

fn dimension<'c>(dm: &mut Dimension, c: &'c Call<'_>, h: Handle, used: &mut Used<'c>) {
    let p = c.p;
    let before = dm.clone();
    if let Some(v) = str_param(p, "textOverride") {
        dm.text = v.to_string();
        used.insert("textOverride");
    }
    if let Some(st) = &c.dim_style {
        dm.style.clone_from(st);
        used.insert("dimStyle");
    }
    // Overrides: clear first, then set (as DIMOVERRIDE does).
    if p.get("clearOverrides").and_then(Value::as_bool) == Some(true) {
        dm.overrides.clear();
        used.insert("clearOverrides");
    }
    if let Some(names) = p.get("clear").and_then(Value::as_array) {
        // Prefix and suffix rows reset DIMPOST (DIMAPOST), the variable they're halves of.
        let field = |n: &str| match n {
            "prefix" | "suffix" => Some("post"),
            "altPrefix" | "altSuffix" => Some("altPost"),
            n => DimStyle::field_name(n),
        };
        for f in names.iter().filter_map(Value::as_str).filter_map(field) {
            dm.overrides.retain(|k, _| DimStyle::field_name(k) != Some(f));
        }
        used.insert("clear");
    }
    for (f, v) in &c.dim_vars.fields {
        set_override(dm, f, v.clone());
    }
    used.extend(c.dim_vars.keys.iter().map(String::as_str));
    if let Some(x) = c.num("textRotation") {
        dm.text_rotation = x.to_radians();
        used.insert("textRotation");
    }
    let (x, y) = (c.num("textPositionX"), c.num("textPositionY"));
    if x.is_some() || y.is_some() {
        let at = c.prep.text_pos.get(&h).copied().unwrap_or_else(|| dm.text_mid.xy());
        dm.text_mid.x = x.unwrap_or(at.x);
        dm.text_mid.y = y.unwrap_or(at.y);
        dm.user_text_pos = true;
        used.extend(["textPositionX", "textPositionY"].into_iter().filter(|k| c.num(k).is_some()));
    }
    dimension_posts(dm, c, used);
    // The geometry block read from a file no longer matches: draw and save it anew.
    if *dm != before {
        dm.block = None;
    }
}

/// A dimension's prefix and suffix edits, which go into the DIMPOST and DIMAPOST templates it
/// draws with after the call's other edits (its style, cleared and new overrides).
fn dimension_posts(dm: &mut Dimension, c: &Call<'_>, used: &mut Used<'_>) {
    let Some(styles) = &c.prep.posts else { return };
    let (post, alt) = styles.get(&dm.style.to_ascii_lowercase()).cloned().unwrap_or_default();
    let st = DimStyle { post, alt_post: alt, ..DimStyle::default() }.with_overrides(&dm.overrides);
    for (field, template, keys) in [("post", &st.post, ["prefix", "suffix"]), ("altPost", &st.alt_post, ["altPrefix", "altSuffix"])] {
        let (pre, suf) = (str_param(c.p, keys[0]), str_param(c.p, keys[1]));
        if pre.is_some() || suf.is_some() {
            let (pre0, suf0) = split_post(template);
            set_override(dm, field, json!(join_post(pre.unwrap_or(&pre0), suf.unwrap_or(&suf0))));
            used.extend(keys.into_iter().filter(|k| str_param(c.p, k).is_some()));
        }
    }
}

fn hatch(ha: &mut Hatch, c: &Call<'_>, used: &mut Used<'_>) {
    // HATCHEDIT's pattern edit (a blank name changes nothing, so it's ignored).
    if let Some(v) = str_param(c.p, "pattern")
        && crate::cmd::hatch::set_pattern(ha, v)
    {
        used.insert("pattern");
    }
    if let Some(x) = c.num("scale").filter(|x| *x > 0.0) {
        ha.scale = x;
        used.insert("scale");
    }
    if let Some(x) = c.num("angle") {
        ha.angle = x.to_radians();
        used.insert("angle");
    }
}
