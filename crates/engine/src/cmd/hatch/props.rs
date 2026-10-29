//! A hatch's properties as HATCH takes them: from another hatch (Inherit Properties in the Hatch
//! and Gradient dialog, HATCH's Match properties keyword and its `inherit`), and the layer and
//! transparency new hatches get (HPLAYER, HPTRANSPARENCY).

use cadcraft_doc::{Drawing, EntityKind, HVal, Handle, Header, Transparency};
use serde_json::{Map, Value, json};

/// "ByLayer", "ByBlock" or a percentage 0–90 (a number, or text such as "50" or "50%").
pub(crate) fn parse_transparency(v: &Value) -> Option<Transparency> {
    let pct = match v.as_str() {
        Some(s) if s.trim().eq_ignore_ascii_case("bylayer") => return Some(Transparency::ByLayer),
        Some(s) if s.trim().eq_ignore_ascii_case("byblock") => return Some(Transparency::ByBlock),
        Some(s) => s.trim().trim_end_matches('%').trim_end().parse::<f64>().ok()?,
        None => v.as_f64()?,
    };
    (pct.is_finite() && (0.0..=90.0).contains(&pct)).then(|| Transparency::Percent(pct.round() as u8))
}

/// "Use current" (no value of its own): empty, "." or the words.
pub(crate) fn is_use_current(t: &str) -> bool {
    let t = t.trim();
    t.is_empty() || t == "." || t.eq_ignore_ascii_case("use current") || t.eq_ignore_ascii_case("current")
}

/// A hatch setting held as text (HPLAYER, HPTRANSPARENCY); `None` is "use current".
pub(crate) fn hp_text(h: &Header, name: &str) -> Option<String> {
    let v = h.str(name, "");
    (!is_use_current(&v)).then(|| v.trim().to_string())
}

/// The layer for new hatches: `layer` when given, else HPLAYER; `None` (the current layer) when
/// it names no layer of the drawing.
pub(crate) fn hatch_layer(d: &Drawing, given: Option<&str>) -> Option<String> {
    let name = match given {
        Some(t) if is_use_current(t) => return None,
        Some(t) => t.trim().to_string(),
        None => hp_text(&d.header, "HPLAYER")?,
    };
    d.layer(&name).map(|l| l.name.clone())
}

/// The transparency for new hatches: `transparency` when given, else HPTRANSPARENCY; `None` is
/// the current one (CETRANSPARENCY).
pub(crate) fn hatch_transparency(d: &Drawing, given: Option<&Value>) -> Option<Transparency> {
    match given {
        Some(Value::String(t)) if is_use_current(t) => None,
        Some(v) => parse_transparency(v),
        None => parse_transparency(&json!(hp_text(&d.header, "HPTRANSPARENCY")?)),
    }
}

/// The properties of hatch `h` as HATCH parameters: its pattern, scale and angle (or its
/// gradient's name, colours, angle and centring), island style, associativity, layer and
/// transparency, and its origin when `origin` (HPINHERIT 1). `None` when `h` isn't a hatch.
pub fn inherit_params(d: &Drawing, h: Handle, origin: bool) -> Option<Map<String, Value>> {
    let e = d.entity(h)?;
    let EntityKind::Hatch(x) = &e.kind else { return None };
    let mut m = Map::new();
    match &x.gradient {
        Some(g) => {
            m.insert("gradient".into(), json!(true));
            m.insert("name".into(), json!(g.name));
            m.insert("color1".into(), json!(g.color1.name()));
            m.insert("color2".into(), json!(g.color2.name()));
            m.insert("angle".into(), json!(finite(g.angle.to_degrees())));
            m.insert("centered".into(), json!(g.centered));
        }
        None => {
            m.insert("pattern".into(), json!(if x.solid { "SOLID" } else { x.pattern.as_str() }));
            m.insert("angle".into(), json!(finite(x.angle.to_degrees())));
        }
    }
    if x.scale.is_finite() && x.scale > 0.0 {
        m.insert("scale".into(), json!(x.scale));
    }
    m.insert("style".into(), json!(["normal", "outer", "ignore"].get(usize::from(x.style.min(2))).copied().unwrap_or("normal")));
    m.insert("associative".into(), json!(x.associative));
    m.insert("layer".into(), json!(e.common.layer));
    m.insert(
        "transparency".into(),
        match e.common.transparency {
            Transparency::Percent(p) => json!(p),
            t => json!(t.name()),
        },
    );
    if origin && x.origin.is_finite() {
        m.insert("origin".into(), json!([x.origin.x, x.origin.y]));
    }
    Some(m)
}

fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// Make hatch `h`'s properties the ones HATCH uses next (HPNAME, HPSCALE, HPANG,
/// HPISLANDDETECTION, HPASSOC, HPLAYER, HPTRANSPARENCY, and HPORIGIN when `origin`). Returns the
/// source's gradient, which the running command keeps (it has no header variables).
pub(crate) fn inherit_into_header(d: &mut Drawing, h: Handle, origin: bool) -> Option<Option<cadcraft_doc::Gradient>> {
    let m = inherit_params(d, h, origin)?;
    let gradient = match d.entity(h).map(|e| &e.kind) {
        Some(EntityKind::Hatch(x)) => x.gradient.clone(),
        _ => None,
    };
    let hd = &mut d.header;
    if let Some(p) = m.get("pattern").and_then(Value::as_str) {
        hd.set_str("HPNAME", p);
    }
    if let Some(v) = m.get("scale").and_then(Value::as_f64) {
        hd.set_f64("HPSCALE", v);
    }
    if gradient.is_none()
        && let Some(v) = m.get("angle").and_then(Value::as_f64)
    {
        hd.set_f64("HPANG", v.to_radians());
    }
    let style = m.get("style").and_then(Value::as_str).and_then(super::island_style).unwrap_or(0);
    hd.set_i64("HPISLANDDETECTION", i64::from(style));
    hd.set_i64("HPASSOC", i64::from(m.get("associative").and_then(Value::as_bool).unwrap_or(true)));
    if let Some(l) = m.get("layer").and_then(Value::as_str) {
        hd.set_str("HPLAYER", l);
    }
    let t = match m.get("transparency") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    hd.set_str("HPTRANSPARENCY", &t);
    if let Some([x, y]) = m.get("origin").and_then(Value::as_array).map(|a| [a.first().and_then(Value::as_f64), a.get(1).and_then(Value::as_f64)])
        && let (Some(x), Some(y)) = (x, y)
    {
        hd.set("HPORIGIN", HVal::Point(cadcraft_geom::Vec3::new(x, y, 0.0)));
        hd.set_i64("HPORIGINMODE", 0);
    }
    Some(gradient)
}

/// `p` with every property of hatch `src` it doesn't give itself (HATCH's `inherit`), and
/// whether the source is a gradient.
pub(crate) fn merge_inherited(d: &Drawing, p: &Value, src: Handle) -> Option<(Value, bool)> {
    let origin = p.get("inheritOrigin").and_then(Value::as_bool).unwrap_or(d.header.i64("HPINHERIT", 0) == 1);
    let inherited = inherit_params(d, src, origin)?;
    let gradient = inherited.get("gradient").and_then(Value::as_bool).unwrap_or(false);
    let mut out = p.as_object().cloned().unwrap_or_default();
    for (k, v) in inherited {
        // A given origin mode keeps its own origin.
        if k == "origin" && out.contains_key("originMode") {
            continue;
        }
        out.entry(k).or_insert(v);
    }
    Some((Value::Object(out), gradient))
}
