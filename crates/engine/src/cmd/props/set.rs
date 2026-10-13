//! `properties.set`: change properties of the selected objects, the way the Properties palette
//! edits them. Keys that no selected object takes come back in `ignored`.

use std::collections::{BTreeSet, HashMap};

use cadcraft_doc::{DimStyle, Drawing, HAlign, Handle, Text, VAlign};
use cadcraft_geom::Vec2;

use super::*;
use crate::properties::{LINE_PITCH, is_dim_linetype, join_post, split_post};

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
    /// Dimensions: the DIMPOST and DIMAPOST templates they draw with.
    posts: HashMap<Handle, (String, String)>,
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
                    let font = cadcraft_fonts::TextFont::resolve(&d.text_style(&t.style).cloned().unwrap_or_default().font);
                    let w = cadcraft_fonts::text_width(&font, &t.value, t.height, t.width_factor);
                    let w = if w.is_finite() && w > 1e-9 { w } else { t.height.max(1e-9) };
                    out.ends.insert(*h, Vec2::polar(t.justify_point(), w, t.rotation));
                }
                EntityKind::Dimension(dm) => {
                    if moves_text {
                        out.text_pos.insert(*h, cadcraft_render::dimension_in(d, dm).text_pos);
                    }
                    if posts {
                        let st = d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides);
                        out.posts.insert(*h, (st.post, st.alt_post));
                    }
                }
                _ => {}
            }
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
    let lw = p.get("lineweight").map(|v| crate::cmd::layer::parse_lw(v).ok_or_else(|| bad("properties.set", "bad lineweight"))).transpose()?;
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
    if let Some(l) = str_param(p, "layer") {
        d.ensure_layer(l);
    }
    let dim_style = str_param(p, "dimStyle")
        .map(|n| d.dim_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("dimension style `{n}` not found"))))
        .transpose()?;
    let text_style = str_param(p, "style")
        .map(|n| d.text_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("text style `{n}` not found"))))
        .transpose()?;
    let dim_vars = DimVars::new(d, p)?;
    let prep = Prepared::new(d, p, &hs);
    let anno_scale = d.current_anno_scale();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for h in &hs {
        let h = *h;
        d.modify_entity(h, |e| {
            // Annotative: Yes supports the current annotation scale; No drops every scale.
            if let Some(a) = p.get("annotative").and_then(Value::as_bool)
                && cadcraft_doc::can_be_annotative(&e.kind)
            {
                cadcraft_doc::set_annotative(e, a, &anno_scale);
                if let EntityKind::Dimension(dm) = &mut e.kind {
                    dm.block = None;
                }
                used.insert("annotative");
            }
            if let Some(l) = str_param(p, "layer") {
                e.common.layer = l.to_string();
                used.insert("layer");
            }
            if let Some(c) = color {
                e.common.color = c;
                used.insert("color");
            }
            if let Some(lt) = &linetype {
                e.common.linetype.clone_from(lt);
                used.insert("linetype");
            }
            if let Some(w) = lw {
                e.common.lineweight = w;
                used.insert("lineweight");
            }
            if let Some(x) = p.get("ltscale").and_then(Value::as_f64).filter(|x| *x > 0.0 && x.is_finite()) {
                e.common.ltscale = x;
                used.insert("ltscale");
            }
            if let Some(t) = transparency {
                e.common.transparency = t;
                used.insert("transparency");
            }
            if let Some(th) = thickness {
                e.common.thickness = th;
                used.insert("thickness");
            }
            if let Some(v) = p.get("visible").and_then(Value::as_bool) {
                e.common.visible = v;
                used.insert("visible");
            }
            let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
            match &mut e.kind {
                EntityKind::Line(l) => {
                    if let Some(v) = p.get("start") {
                        set_xy(&mut l.a, v);
                        used.insert("start");
                    }
                    if let Some(v) = p.get("end") {
                        set_xy(&mut l.b, v);
                        used.insert("end");
                    }
                    // Length / angle keep the start point and move the end point.
                    let (a, b) = (l.a.xy(), l.b.xy());
                    let length = num("length").filter(|x| *x > 0.0 && *x < 1e15);
                    let len = length.unwrap_or_else(|| a.dist(b));
                    let ang = num("angle").map(f64::to_radians).unwrap_or_else(|| a.angle_to(b));
                    if length.is_some() || num("angle").is_some() {
                        let e = Vec2::polar(a, len, ang);
                        if e.is_finite() {
                            l.b.x = e.x;
                            l.b.y = e.y;
                            if length.is_some() {
                                used.insert("length");
                            }
                            if num("angle").is_some() {
                                used.insert("angle");
                            }
                        }
                    }
                }
                EntityKind::Circle(c) => {
                    if let Some(v) = p.get("center") {
                        set_xy(&mut c.center, v);
                        used.insert("center");
                    }
                    if let Some(r) = num("radius").filter(|r| *r > 0.0) {
                        c.radius = r;
                        used.insert("radius");
                    }
                    if let Some(dm) = num("diameter").filter(|r| *r > 0.0) {
                        c.radius = dm / 2.0;
                        used.insert("diameter");
                    }
                    if let Some(cf) = num("circumference").filter(|x| *x > 0.0) {
                        c.radius = cf / cadcraft_geom::TAU;
                        used.insert("circumference");
                    }
                    if let Some(a) = num("area").filter(|x| *x > 0.0) {
                        c.radius = (a / cadcraft_geom::PI).sqrt();
                        used.insert("area");
                    }
                }
                EntityKind::Arc(a) => {
                    if let Some(v) = p.get("center") {
                        set_xy(&mut a.center, v);
                        used.insert("center");
                    }
                    if let Some(r) = num("radius").filter(|r| *r > 0.0) {
                        a.radius = r;
                        used.insert("radius");
                    }
                    if let Some(x) = num("startAngle") {
                        a.start = cadcraft_geom::norm_angle(x.to_radians());
                        used.insert("startAngle");
                    }
                    if let Some(x) = num("endAngle") {
                        a.end = cadcraft_geom::norm_angle(x.to_radians());
                        used.insert("endAngle");
                    }
                }
                EntityKind::Text(t) => {
                    if let Some(v) = str_param(p, "text") {
                        t.value = v.to_string();
                        used.insert("text");
                    }
                    if let Some(st) = &text_style {
                        t.style.clone_from(st);
                        used.insert("style");
                    }
                    if let Some(x) = num("height").filter(|x| *x > 0.0) {
                        t.height = x;
                        used.insert("height");
                    }
                    if let Some(x) = num("rotation") {
                        t.rotation = x.to_radians();
                        used.insert("rotation");
                    }
                    if let Some(x) = num("widthFactor").filter(|x| *x > 0.0) {
                        t.width_factor = x;
                        used.insert("widthFactor");
                    }
                    if let Some(x) = num("oblique").filter(|x| x.abs() <= 85.0) {
                        t.oblique = x.to_radians();
                        used.insert("oblique");
                    }
                    if let Some((ha, va)) = str_param(p, "justify").and_then(crate::cmd::draw::justify) {
                        set_justify(t, ha, va, prep.ends.get(&h).copied());
                        used.insert("justify");
                    }
                    // The position is the point the text is placed by: moving it moves the text.
                    if let Some(to) = p.get("position").and_then(point_value) {
                        shift(t, to - t.justify_point());
                        used.insert("position");
                    }
                    let (x, y) = (num("positionX"), num("positionY"));
                    if x.is_some() || y.is_some() {
                        let at = t.justify_point();
                        shift(t, Vec2::new(x.unwrap_or(at.x), y.unwrap_or(at.y)) - at);
                        used.extend(["positionX", "positionY"].into_iter().filter(|k| num(k).is_some()));
                    }
                    if let Some(z) = num("positionZ") {
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
                        if let Some(x) = num("alignmentX") {
                            a.x = x;
                            used.insert("alignmentX");
                        }
                        if let Some(y) = num("alignmentY") {
                            a.y = y;
                            used.insert("alignmentY");
                        }
                    }
                }
                EntityKind::MText(t) => {
                    if let Some(v) = str_param(p, "text") {
                        t.contents = v.replace('\n', "\\P");
                        used.insert("text");
                    }
                    if let Some(st) = &text_style {
                        t.style.clone_from(st);
                        used.insert("style");
                    }
                    if let Some(x) = num("height").filter(|x| *x > 0.0) {
                        t.height = x;
                        used.insert("height");
                    }
                    if let Some(x) = num("width").filter(|x| *x >= 0.0) {
                        t.width = x;
                        used.insert("width");
                    }
                    if let Some(x) = num("rotation") {
                        t.rotation = x.to_radians();
                        used.insert("rotation");
                    }
                    // The attachment point stays where it is; the text moves around it.
                    if let Some(n) = str_param(p, "justify").and_then(crate::cmd::mtext_opts::attach_code) {
                        t.attach = n;
                        used.insert("justify");
                    }
                    // MTEXT's Line spacing option takes factors from 0.25 to 4.
                    let factor = |f: f64| f.is_finite() && (0.25 - 1e-9..=4.0 + 1e-9).contains(&f);
                    if let Some(x) = num("lineSpacing").filter(|x| factor(*x)) {
                        t.line_spacing = x;
                        used.insert("lineSpacing");
                    }
                    if let Some(f) = num("lineSpacingDistance").map(|x| x / (t.height * LINE_PITCH)).filter(|f| factor(*f)) {
                        t.line_spacing = f;
                        used.insert("lineSpacingDistance");
                    }
                    // "atLeast", "At least", "exactly"…
                    let spacing = str_param(p, "lineSpacingStyle")
                        .map(|v| v.chars().filter(char::is_ascii_alphabetic).collect::<String>().to_ascii_lowercase());
                    if let Some(exact) = match spacing.as_deref() {
                        Some("atleast") => Some(false),
                        Some("exactly") => Some(true),
                        _ => None,
                    } {
                        t.line_spacing_exact = exact;
                        used.insert("lineSpacingStyle");
                    }
                    if let Some(to) = p.get("position").and_then(point_value) {
                        t.insert.x = to.x;
                        t.insert.y = to.y;
                        used.insert("position");
                    }
                    for (k, slot) in [("positionX", &mut t.insert.x), ("positionY", &mut t.insert.y), ("positionZ", &mut t.insert.z)] {
                        if let Some(v) = num(k) {
                            *slot = v;
                            used.insert(k);
                        }
                    }
                }
                EntityKind::LwPolyline(pl) => {
                    if let Some(x) = num("width").filter(|x| *x >= 0.0) {
                        pl.const_width = x;
                        used.insert("width");
                    }
                    if let Some(c) = p.get("closed").and_then(Value::as_bool) {
                        pl.closed = c;
                        used.insert("closed");
                    }
                }
                EntityKind::Insert(i) => {
                    if let Some(x) = num("rotation") {
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
                    if let Some(x) = num("scale").filter(|x| *x != 0.0) {
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
                EntityKind::Dimension(dm) => {
                    let before = dm.clone();
                    if let Some(v) = str_param(p, "textOverride") {
                        dm.text = v.to_string();
                        used.insert("textOverride");
                    }
                    if let Some(st) = &dim_style {
                        dm.style.clone_from(st);
                        used.insert("dimStyle");
                    }
                    // Overrides: clear first, then set (as DIMOVERRIDE does).
                    if p.get("clearOverrides").and_then(Value::as_bool) == Some(true) {
                        dm.overrides.clear();
                        used.insert("clearOverrides");
                    }
                    if let Some(names) = p.get("clear").and_then(Value::as_array) {
                        for f in names.iter().filter_map(Value::as_str).filter_map(DimStyle::field_name) {
                            dm.overrides.retain(|k, _| DimStyle::field_name(k) != Some(f));
                        }
                        used.insert("clear");
                    }
                    for (f, v) in &dim_vars.fields {
                        set_override(dm, f, v.clone());
                    }
                    used.extend(dim_vars.keys.iter().map(String::as_str));
                    if let Some(x) = num("textRotation") {
                        dm.text_rotation = x.to_radians();
                        used.insert("textRotation");
                    }
                    let (x, y) = (num("textPositionX"), num("textPositionY"));
                    if x.is_some() || y.is_some() {
                        let at = prep.text_pos.get(&h).copied().unwrap_or_else(|| dm.text_mid.xy());
                        dm.text_mid.x = x.unwrap_or(at.x);
                        dm.text_mid.y = y.unwrap_or(at.y);
                        dm.user_text_pos = true;
                        used.extend(["textPositionX", "textPositionY"].into_iter().filter(|k| num(k).is_some()));
                    }
                    if let Some((post, alt)) = prep.posts.get(&h) {
                        for (field, template, keys) in [("post", post, ["prefix", "suffix"]), ("altPost", alt, ["altPrefix", "altSuffix"])] {
                            let (pre, suf) = (str_param(p, keys[0]), str_param(p, keys[1]));
                            if pre.is_some() || suf.is_some() {
                                let (pre0, suf0) = split_post(template);
                                set_override(dm, field, json!(join_post(pre.unwrap_or(&pre0), suf.unwrap_or(&suf0))));
                                used.extend(keys.into_iter().filter(|k| str_param(p, k).is_some()));
                            }
                        }
                    }
                    // The geometry block read from a file no longer matches: draw and save it anew.
                    if *dm != before {
                        dm.block = None;
                    }
                }
                EntityKind::Hatch(h) => {
                    if let Some(v) = str_param(p, "pattern") {
                        if !h.pattern.eq_ignore_ascii_case(v) {
                            h.pattern_lines = crate::cmd::hatch::lines_for(v);
                        }
                        h.pattern = v.to_ascii_uppercase();
                        // The old pattern's definition lines no longer apply.
                        h.pattern_lines.clear();
                        h.solid = h.pattern == "SOLID";
                        if !h.solid {
                            // A named pattern replaces the gradient fill; the renderer draws the gradient first when it is set.
                            h.gradient = None;
                        }
                        used.insert("pattern");
                    }
                    if let Some(x) = num("scale").filter(|x| *x > 0.0) {
                        h.scale = x;
                        used.insert("scale");
                    }
                    if let Some(x) = num("angle") {
                        h.angle = x.to_radians();
                        used.insert("angle");
                    }
                }
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
