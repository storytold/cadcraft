//! Properties: per-object property edits, current properties, MATCHPROP, linetypes, units.

use cadcraft_color::Color;
use cadcraft_doc::{EntityKind, Lineweight, Transparency};
use cadcraft_geom::Vec3;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("properties", "Properties", run_get).menu(&["Modify", "Properties"]).alias(&["pr", "props", "ch"]).params("{handles?} → properties of the selection").noundo(),
        CommandSpec::new("properties.set", "Set Properties", run_set).params("{handles?, layer?, color?, linetype?, lineweight?, ltscale?, transparency?: \"ByLayer\" | \"ByBlock\" | 0..90, visible?, <geometry fields: radius, center, start, end, text, height, rotation…>}"),
        CommandSpec::new("matchprop", "Match Properties", run_matchprop).menu(&["Modify", "Match Properties"]).alias(&["ma", "painter"]).params("{source, targets: [hex]}"),
        CommandSpec::new("color", "Color...", run_color).menu(&["Format", "Color..."]).alias(&["col", "colour"]).params("{color: \"ByLayer\" | \"red\" | 1..255 | \"r,g,b\"}"),
        CommandSpec::new("linetype", "Linetype...", run_linetype).menu(&["Format", "Linetype..."]).alias(&["lt", "ltype"]).params("{current?: name, load?: name | \"*\"}"),
        CommandSpec::new("lweight", "Lineweight...", run_lweight).menu(&["Format", "Lineweight..."]).alias(&["lw", "lineweight"]).params("{lineweight: mm | ByLayer}"),
        CommandSpec::new("ltscale", "Linetype Scale", run_ltscale).alias(&["lts"]).params("{scale}"),
        CommandSpec::new("units", "Units...", run_units).menu(&["Format", "Units..."]).alias(&["un"]).params("{lunits?: 1..5, luprec?: 0..8, aunits?: 0..4, auprec?: 0..8, insunits?}"),
        CommandSpec::new("limits", "Drawing Limits", run_limits).menu(&["Format", "Drawing Limits"]).params("{min: [x,y], max: [x,y]}"),
        CommandSpec::new("style", "Text Style...", run_style)
            .menu(&["Format", "Text Style..."])
            .alias(&["st"])
            .params("{name, font?, bigFont?, height?, widthFactor?, oblique? (degrees), backwards?, upsideDown?, vertical?, annotative?, current?} → styles"),
        CommandSpec::new("style.list", "List Text Styles", run_style_list).params("{} → text styles").noundo(),
        CommandSpec::new("style.rename", "Rename Text Style", run_style_rename).params("{from, to}"),
        CommandSpec::new("style.delete", "Delete Text Style", run_style_delete).params("{name} (not Standard, the current style or one in use)"),
        CommandSpec::new("style.current", "Set Current Text Style", run_style_current).params("{name}"),
        CommandSpec::new("dimstyle", "Dimension Style...", run_dimstyle)
            .menu(&["Format", "Dimension Style..."])
            .alias(&["d", "dst", "ddim"])
            .params("{name, current?, <style fields or DIM* variables, e.g. arrowSize / DIMASZ, DIMTSZ, DIMBLK, DIMTAD, DIMLUNIT…>} → styles"),
        CommandSpec::new("dimstyle.dimension", "Dimension Style...", run_dimstyle).menu(&["Dimension", "Dimension Style..."]).params("same as `dimstyle`"),
        CommandSpec::new("dimstyle.list", "List Dimension Styles", run_dimstyle_list).params("{name?} → styles (with all variables for `name`)").noundo(),
        CommandSpec::new("dimstyle.rename", "Rename Dimension Style", run_dimstyle_rename).params("{from, to}"),
        CommandSpec::new("dimstyle.delete", "Delete Dimension Style", run_dimstyle_delete).params("{name} (not Standard, the current style or one in use)"),
        CommandSpec::new("dimstyle.current", "Set Current Dimension Style", run_dimstyle_current).params("{name}"),
        CommandSpec::new("dimstyle.override", "Dimension Style Override", dim_override)
            .params("{handles?, <style fields or DIM* variables: values>, clear?: bool} (per-dimension overrides)"),
        CommandSpec::new("tablestyle", "Table Style...", run_tablestyle)
            .menu(&["Format", "Table Style..."])
            .alias(&["ts"])
            .params("{name, textHeight?, margin?, title?, header?, current?} → styles"),
        CommandSpec::new("mleaderstyle", "Multileader Style...", run_mleaderstyle)
            .menu(&["Format", "Multileader Style..."])
            .alias(&["mls"])
            .params("{name, arrowSize?, textHeight?, landingGap?, dogleg?, textStyle?, current?} → styles"),
        CommandSpec::new("ddptype", "Point Style...", run_ptype).menu(&["Format", "Point Style..."]).params("{pdmode, pdsize}"),
        CommandSpec::new("rename", "Rename...", run_rename).menu(&["Format", "Rename..."]).params("{table: layer|linetype|style|dimstyle|block, from, to}"),
    ]
}

pub(super) fn entity_props(d: &cadcraft_doc::Drawing, e: &cadcraft_doc::Entity) -> Value {
    let mut v = json!({
        "handle": e.handle.hex(),
        "type": e.kind.type_name(),
        "layer": e.common.layer,
        "color": e.common.color.name(),
        "linetype": e.common.linetype,
        "lineweight": e.common.lineweight.name(),
        "ltscale": e.common.ltscale,
        "transparency": transparency_value(e.common.transparency),
        "visible": e.common.visible,
    });
    let geo = serde_json::to_value(&e.kind).unwrap_or(Value::Null);
    if let (Some(o), Some(g)) = (v.as_object_mut(), geo.as_object()) {
        o.insert("geometry".into(), Value::Object(g.clone()));
        let b = cadcraft_doc::entity_bounds(d, e, 0);
        if !b.is_empty() {
            o.insert("bounds".into(), json!([[b.min.x, b.min.y], [b.max.x, b.max.y]]));
        }
        match &e.kind {
            EntityKind::Line(l) => {
                o.insert("length".into(), json!(l.a.xy().dist(l.b.xy())));
                o.insert("angle".into(), json!(l.a.xy().angle_to(l.b.xy()).to_degrees()));
            }
            EntityKind::Circle(c) => {
                o.insert("diameter".into(), json!(c.radius * 2.0));
                o.insert("circumference".into(), json!(c.radius * cadcraft_geom::TAU));
                o.insert("area".into(), json!(c.radius * c.radius * cadcraft_geom::PI));
            }
            EntityKind::Arc(a) => {
                o.insert("arcLength".into(), json!(a.radius * cadcraft_geom::ccw_sweep(a.start, a.end)));
                o.insert("totalAngle".into(), json!(cadcraft_geom::ccw_sweep(a.start, a.end).to_degrees()));
            }
            EntityKind::LwPolyline(p) => {
                let pl = cadcraft_geom::Polyline { vertices: p.vertices.clone(), closed: p.closed };
                o.insert("length".into(), json!(pl.len()));
                if p.closed {
                    o.insert("area".into(), json!(pl.area().abs()));
                }
            }
            _ => {}
        }
    }
    v
}

fn run_get(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let d = s.doc()?;
    if hs.is_empty() {
        let h = &d.header;
        return Ok(json!({
            "selection": "No selection",
            "color": Color::from_aci(h.i64("CECOLOR", 256) as i16).name(),
            "layer": h.str("CLAYER", "0"),
            "linetype": h.str("CELTYPE", "ByLayer"),
            "linetypeScale": h.f64("CELTSCALE", 1.0),
            "lineweight": Lineweight::from_dxf(h.i64("CELWEIGHT", -1) as i16).name(),
            "textStyle": h.str("TEXTSTYLE", "Standard"),
            "dimStyle": h.str("DIMSTYLE", "Standard"),
            "textHeight": h.f64("TEXTSIZE", 0.2),
        }));
    }
    let ents: Vec<Value> = hs.iter().filter_map(|h| d.entity(*h).map(|e| entity_props(d, e))).collect();
    Ok(json!({ "count": ents.len(), "objects": ents }))
}

fn parse_color(v: &Value) -> Option<Color> {
    v.as_str().and_then(Color::parse).or_else(|| v.as_i64().and_then(|i| i16::try_from(i).ok()).map(Color::from_aci))
}

/// A transparency as JSON, in the form `properties.set` takes: "ByLayer", "ByBlock" or a percentage.
pub(super) fn transparency_value(t: Transparency) -> Value {
    match t {
        Transparency::Percent(p) => json!(p),
        other => json!(other.name()),
    }
}

/// "ByLayer", "ByBlock" or a percentage 0..=90 (a number, or a string such as "50" or "50%").
fn parse_transparency(v: &Value) -> Option<Transparency> {
    let pct = match v.as_str() {
        Some(s) if s.trim().eq_ignore_ascii_case("bylayer") => return Some(Transparency::ByLayer),
        Some(s) if s.trim().eq_ignore_ascii_case("byblock") => return Some(Transparency::ByBlock),
        Some(s) => s.trim().trim_end_matches('%').trim_end().parse::<f64>().ok()?,
        None => v.as_f64()?,
    };
    (pct.is_finite() && (0.0..=90.0).contains(&pct)).then(|| Transparency::Percent(pct.round() as u8))
}

fn set_xy(p: &mut Vec3, v: &Value) {
    if let Some(q) = point_value(v) {
        p.x = q.x;
        p.y = q.y;
    }
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(bad("properties.set", "nothing selected"));
    }
    let color = p.get("color").map(|c| parse_color(c).ok_or_else(|| bad("properties.set", "bad colour"))).transpose()?;
    let transparency = p
        .get("transparency")
        .map(|v| parse_transparency(v).ok_or_else(|| bad("properties.set", "transparency must be ByLayer, ByBlock or a percentage from 0 to 90")))
        .transpose()?;
    let lw = p.get("lineweight").map(|v| match v.as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("bylayer") => Lineweight::ByLayer,
        Some("byblock") => Lineweight::ByBlock,
        Some("default") => Lineweight::Default,
        _ => Lineweight::Mm100(
            (v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim_end_matches("mm").trim().parse().ok())).unwrap_or(0.25) * 100.0)
                .round()
                .clamp(0.0, 211.0) as u16,
        ),
    });
    let d = s.doc_mut()?;
    if let Some(l) = str_param(p, "layer") {
        d.ensure_layer(l);
    }
    let dim_style = str_param(p, "dimStyle")
        .map(|n| d.dim_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("dimension style `{n}` not found"))))
        .transpose()?;
    for h in &hs {
        d.modify_entity(*h, |e| {
            if let Some(l) = str_param(p, "layer") {
                e.common.layer = l.to_string();
            }
            if let Some(c) = color {
                e.common.color = c;
            }
            if let Some(lt) = str_param(p, "linetype") {
                e.common.linetype = lt.to_string();
            }
            if let Some(w) = lw {
                e.common.lineweight = w;
            }
            if let Some(x) = p.get("ltscale").and_then(Value::as_f64).filter(|x| *x > 0.0) {
                e.common.ltscale = x;
            }
            if let Some(t) = transparency {
                e.common.transparency = t;
            }
            if let Some(v) = p.get("visible").and_then(Value::as_bool) {
                e.common.visible = v;
            }
            let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
            match &mut e.kind {
                EntityKind::Line(l) => {
                    if let Some(v) = p.get("start") {
                        set_xy(&mut l.a, v);
                    }
                    if let Some(v) = p.get("end") {
                        set_xy(&mut l.b, v);
                    }
                    // Length / angle keep the start point and move the end point.
                    let (a, b) = (l.a.xy(), l.b.xy());
                    let len = num("length").filter(|x| *x > 0.0 && *x < 1e15).unwrap_or_else(|| a.dist(b));
                    let ang = num("angle").map(f64::to_radians).unwrap_or_else(|| a.angle_to(b));
                    if num("length").is_some() || num("angle").is_some() {
                        let e = cadcraft_geom::Vec2::polar(a, len, ang);
                        if e.is_finite() {
                            l.b.x = e.x;
                            l.b.y = e.y;
                        }
                    }
                }
                EntityKind::Circle(c) => {
                    if let Some(v) = p.get("center") {
                        set_xy(&mut c.center, v);
                    }
                    if let Some(r) = num("radius").filter(|r| *r > 0.0) {
                        c.radius = r;
                    }
                    if let Some(dm) = num("diameter").filter(|r| *r > 0.0) {
                        c.radius = dm / 2.0;
                    }
                    if let Some(cf) = num("circumference").filter(|x| *x > 0.0) {
                        c.radius = cf / cadcraft_geom::TAU;
                    }
                    if let Some(a) = num("area").filter(|x| *x > 0.0) {
                        c.radius = (a / cadcraft_geom::PI).sqrt();
                    }
                }
                EntityKind::Arc(a) => {
                    if let Some(v) = p.get("center") {
                        set_xy(&mut a.center, v);
                    }
                    if let Some(r) = num("radius").filter(|r| *r > 0.0) {
                        a.radius = r;
                    }
                    if let Some(x) = num("startAngle") {
                        a.start = cadcraft_geom::norm_angle(x.to_radians());
                    }
                    if let Some(x) = num("endAngle") {
                        a.end = cadcraft_geom::norm_angle(x.to_radians());
                    }
                }
                EntityKind::Text(t) => {
                    if let Some(v) = str_param(p, "text") {
                        t.value = v.to_string();
                    }
                    if let Some(x) = num("height").filter(|x| *x > 0.0) {
                        t.height = x;
                    }
                    if let Some(x) = num("rotation") {
                        t.rotation = x.to_radians();
                    }
                    if let Some(x) = num("widthFactor").filter(|x| *x > 0.0) {
                        t.width_factor = x;
                    }
                    if let Some(v) = p.get("position") {
                        set_xy(&mut t.insert, v);
                    }
                }
                EntityKind::MText(t) => {
                    if let Some(v) = str_param(p, "text") {
                        t.contents = v.replace('\n', "\\P");
                    }
                    if let Some(x) = num("height").filter(|x| *x > 0.0) {
                        t.height = x;
                    }
                    if let Some(x) = num("width").filter(|x| *x >= 0.0) {
                        t.width = x;
                    }
                    if let Some(x) = num("rotation") {
                        t.rotation = x.to_radians();
                    }
                }
                EntityKind::LwPolyline(pl) => {
                    if let Some(x) = num("width").filter(|x| *x >= 0.0) {
                        pl.const_width = x;
                    }
                    if let Some(c) = p.get("closed").and_then(Value::as_bool) {
                        pl.closed = c;
                    }
                }
                EntityKind::Insert(i) => {
                    if let Some(x) = num("rotation") {
                        i.rotation = x.to_radians();
                    }
                    if let Some(x) = num("scale").filter(|x| *x != 0.0) {
                        i.scale = Vec3::new(x, x, x);
                    }
                }
                EntityKind::Dimension(dm) => {
                    if let Some(v) = str_param(p, "textOverride") {
                        dm.text = v.to_string();
                        dm.block = None;
                    }
                    if let Some(st) = &dim_style {
                        dm.style.clone_from(st);
                        dm.block = None;
                    }
                }
                EntityKind::Hatch(h) => {
                    if let Some(v) = str_param(p, "pattern") {
                        h.pattern = v.to_ascii_uppercase();
                        h.solid = h.pattern == "SOLID";
                        if !h.solid {
                            // A named pattern replaces the gradient fill; the renderer draws the gradient first when it is set.
                            h.gradient = None;
                        }
                    }
                    if let Some(x) = num("scale").filter(|x| *x > 0.0) {
                        h.scale = x;
                    }
                    if let Some(x) = num("angle") {
                        h.angle = x.to_radians();
                    }
                }
                _ => {}
            }
        })?;
    }
    Ok(json!({ "changed": hs.len() }))
}

fn run_matchprop(s: &mut Session, p: &Value) -> Result<Value> {
    let src =
        p.get("source").and_then(Value::as_str).and_then(cadcraft_doc::Handle::parse_hex).ok_or_else(|| bad("matchprop", "`source` is required"))?;
    let tg: Vec<cadcraft_doc::Handle> = p
        .get("targets")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().and_then(cadcraft_doc::Handle::parse_hex)).collect())
        .unwrap_or_default();
    let d = s.doc_mut()?;
    let c = d.entity(src).map(|e| e.common.clone()).ok_or_else(|| bad("matchprop", "no such source"))?;
    for h in &tg {
        d.modify_entity(*h, |e| {
            e.common.layer = c.layer.clone();
            e.common.color = c.color;
            e.common.linetype = c.linetype.clone();
            e.common.lineweight = c.lineweight;
            e.common.ltscale = c.ltscale;
            e.common.transparency = c.transparency;
        })?;
    }
    Ok(json!({ "changed": tg.len() }))
}

fn run_color(s: &mut Session, p: &Value) -> Result<Value> {
    let c = p.get("color").and_then(parse_color).ok_or_else(|| bad("color", "`color` is required"))?;
    let d = s.doc_mut()?;
    match c {
        Color::True(rgb) => {
            d.header.set_i64("CECOLOR", i64::from(cadcraft_color::nearest_aci(rgb)));
            d.header.set_i64("CECOLOR_RGB", i64::from(rgb.to_u32()));
        }
        other => {
            d.header.set_i64("CECOLOR", i64::from(other.to_aci()));
            d.header.vars.remove("CECOLOR_RGB");
        }
    }
    ok()
}

fn run_linetype(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    if let Some(load) = str_param(p, "load") {
        let lib = cadcraft_doc::library::standard_linetypes();
        let mut n = 0;
        for lt in lib {
            if (load == "*" || lt.name.eq_ignore_ascii_case(load)) && d.linetype(&lt.name).is_none() {
                d.linetypes.push(lt);
                n += 1;
            }
        }
        if n == 0 && load != "*" && d.linetype(load).is_none() {
            return Err(bad("linetype", format!("linetype `{load}` not found in the library")));
        }
    }
    if let Some(cur) = str_param(p, "current") {
        if !["bylayer", "byblock"].contains(&cur.to_ascii_lowercase().as_str()) && d.linetype(cur).is_none() {
            return Err(bad("linetype", format!("linetype `{cur}` is not loaded")));
        }
        d.header.set_str("CELTYPE", cur);
    }
    Ok(
        json!({ "loaded": d.linetypes.iter().map(|l| json!({"name": l.name, "description": l.description})).collect::<Vec<_>>(), "library": cadcraft_doc::library::standard_linetypes().iter().map(|l| l.name.clone()).collect::<Vec<_>>() }),
    )
}

fn run_lweight(s: &mut Session, p: &Value) -> Result<Value> {
    let v = p.get("lineweight").ok_or_else(|| bad("lweight", "`lineweight` is required"))?;
    let code = match v.as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("bylayer") => -1,
        Some("byblock") => -2,
        Some("default") => -3,
        _ => (v.as_f64().ok_or_else(|| bad("lweight", "bad lineweight"))? * 100.0).round().clamp(0.0, 211.0) as i64,
    };
    s.doc_mut()?.header.set_i64("CELWEIGHT", code);
    ok()
}

fn run_ltscale(s: &mut Session, p: &Value) -> Result<Value> {
    let v = f64_req("ltscale", p, "scale")?;
    if v <= 0.0 {
        return Err(bad("ltscale", "scale must be positive"));
    }
    s.doc_mut()?.header.set_f64("LTSCALE", v);
    Ok(json!({ "message": "Regenerating model." }))
}

fn run_units(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    for (k, lo, hi) in [("lunits", 1, 5), ("luprec", 0, 8), ("aunits", 0, 4), ("auprec", 0, 8), ("insunits", 0, 24)] {
        if let Some(v) = p.get(k).and_then(Value::as_i64) {
            d.header.set_i64(&k.to_ascii_uppercase(), v.clamp(lo, hi));
        }
    }
    let h = &d.header;
    Ok(
        json!({ "lunits": h.i64("LUNITS", 2), "luprec": h.i64("LUPREC", 4), "aunits": h.i64("AUNITS", 0), "auprec": h.i64("AUPREC", 0), "insunits": h.i64("INSUNITS", 1) }),
    )
}

fn run_limits(s: &mut Session, p: &Value) -> Result<Value> {
    let a = point_req("limits", p, "min")?;
    let b = point_req("limits", p, "max")?;
    let d = s.doc_mut()?;
    d.header.set("LIMMIN", cadcraft_doc::HVal::Point(a.to3(0.0)));
    d.header.set("LIMMAX", cadcraft_doc::HVal::Point(b.to3(0.0)));
    ok()
}

fn style_json(st: &cadcraft_doc::TextStyle, current: &str) -> Value {
    json!({
        "name": st.name,
        "font": st.font,
        "bigFont": st.big_font,
        "height": st.height,
        "widthFactor": st.width_factor,
        "oblique": st.oblique.to_degrees(),
        "backwards": st.backwards,
        "upsideDown": st.upside_down,
        "vertical": st.vertical,
        "annotative": st.annotative,
        "current": st.name.eq_ignore_ascii_case(current),
        "trueType": cadcraft_fonts::TextFont::resolve(&st.font).is_outline(),
    })
}

fn styles_json(d: &cadcraft_doc::Drawing) -> Value {
    let cur = d.header.str("TEXTSTYLE", "Standard");
    json!({ "current": cur, "styles": d.text_styles.iter().map(|t| style_json(t, &cur)).collect::<Vec<_>>() })
}

fn run_style(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style", "`name` is required"))?.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(bad("style", "bad style name"));
    }
    let d = s.doc_mut()?;
    let idx = match d.text_styles.iter().position(|t| t.name.eq_ignore_ascii_case(&name)) {
        Some(i) => i,
        None => {
            d.text_styles.push(cadcraft_doc::TextStyle { name: name.clone(), ..Default::default() });
            d.text_styles.len() - 1
        }
    };
    let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(st) = d.text_styles.get_mut(idx) {
        if let Some(f) = str_param(p, "font").map(str::trim).filter(|f| !f.is_empty()) {
            st.font = f.to_string();
        }
        if let Some(f) = str_param(p, "bigFont") {
            st.big_font = f.to_string();
        }
        if let Some(h) = num("height").filter(|h| *h >= 0.0) {
            st.height = h;
        }
        if let Some(w) = num("widthFactor").filter(|w| *w > 0.0 && *w <= 100.0) {
            st.width_factor = w;
        }
        if let Some(o) = num("oblique").filter(|o| o.abs() <= 85.0) {
            st.oblique = o.to_radians();
        }
        for (k, slot) in [
            ("backwards", &mut st.backwards),
            ("upsideDown", &mut st.upside_down),
            ("vertical", &mut st.vertical),
            ("annotative", &mut st.annotative),
        ] {
            if let Some(b) = p.get(k).and_then(Value::as_bool) {
                *slot = b;
            }
        }
    }
    if bool_or(p, "current", true) {
        d.header.set_str("TEXTSTYLE", &name);
    }
    Ok(styles_json(d))
}

fn run_style_list(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(styles_json(s.doc()?))
}

fn run_style_current(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.current", "`name` is required"))?;
    let d = s.doc_mut()?;
    let n = d.text_style(name).map(|t| t.name.clone()).ok_or_else(|| bad("style.current", format!("no text style `{name}`")))?;
    d.header.set_str("TEXTSTYLE", &n);
    ok()
}

/// Text style names used by entities (in model, paper spaces and blocks) and dimension styles.
fn text_styles_in_use(d: &cadcraft_doc::Drawing) -> Vec<String> {
    let mut out: Vec<String> = d.dim_styles.iter().map(|s| s.text_style.clone()).collect();
    out.extend(d.mleader_styles.iter().map(|s| s.text_style.clone()));
    let stores = std::iter::once(&d.model).chain(d.layouts.iter().map(|l| &l.entities)).chain(d.blocks.values().map(|b| &b.entities));
    for st in stores {
        for e in st.iter() {
            match &e.kind {
                EntityKind::Text(t) => out.push(t.style.clone()),
                EntityKind::MText(t) => out.push(t.style.clone()),
                EntityKind::AttDef(a) => out.push(a.text.style.clone()),
                EntityKind::Insert(i) => out.extend(i.attribs.iter().map(|a| a.text.style.clone())),
                EntityKind::MLeader(m) => out.extend(m.text.iter().map(|t| t.style.clone())),
                _ => {}
            }
        }
    }
    out
}

fn run_style_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").ok_or_else(|| bad("style.rename", "`from` is required"))?.to_string();
    let to = str_param(p, "to").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("style.rename", "`to` is required"))?.to_string();
    let d = s.doc_mut()?;
    if from.eq_ignore_ascii_case("Standard") {
        return Err(bad("style.rename", "the Standard style cannot be renamed"));
    }
    if d.text_style(&to).is_some() && !to.eq_ignore_ascii_case(&from) {
        return Err(bad("style.rename", format!("a text style `{to}` already exists")));
    }
    let st = d.text_styles.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&from)).ok_or_else(|| bad("style.rename", "no such style"))?;
    st.name = to.clone();
    // References follow the new name.
    let fix = |n: &mut String| {
        if n.eq_ignore_ascii_case(&from) {
            *n = to.clone();
        }
    };
    if d.header.str("TEXTSTYLE", "Standard").eq_ignore_ascii_case(&from) {
        d.header.set_str("TEXTSTYLE", &to);
    }
    d.dim_styles.iter_mut().for_each(|x| fix(&mut x.text_style));
    d.mleader_styles.iter_mut().for_each(|x| fix(&mut x.text_style));
    for space in
        std::iter::once(cadcraft_doc::Space::Model).chain(d.layouts.iter().map(|l| cadcraft_doc::Space::Paper(l.name.clone())).collect::<Vec<_>>())
    {
        let hs: Vec<cadcraft_doc::Handle> = d.space(&space).map(|st| st.handles()).unwrap_or_default();
        for h in hs {
            let uses = d.entity(h).is_some_and(|e| match &e.kind {
                EntityKind::Text(t) => t.style.eq_ignore_ascii_case(&from),
                EntityKind::MText(t) => t.style.eq_ignore_ascii_case(&from),
                EntityKind::AttDef(a) => a.text.style.eq_ignore_ascii_case(&from),
                EntityKind::Insert(i) => i.attribs.iter().any(|a| a.text.style.eq_ignore_ascii_case(&from)),
                _ => false,
            });
            if uses {
                d.modify_entity(h, |e| match &mut e.kind {
                    EntityKind::Text(t) => fix(&mut t.style),
                    EntityKind::MText(t) => fix(&mut t.style),
                    EntityKind::AttDef(a) => fix(&mut a.text.style),
                    EntityKind::Insert(i) => i.attribs.iter_mut().for_each(|a| fix(&mut a.text.style)),
                    _ => {}
                })?;
            }
        }
    }
    Ok(styles_json(d))
}

fn run_style_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.delete", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    if name.eq_ignore_ascii_case("Standard") {
        return Err(bad("style.delete", "the Standard style cannot be deleted"));
    }
    if d.header.str("TEXTSTYLE", "Standard").eq_ignore_ascii_case(&name) {
        return Err(bad("style.delete", "the current text style cannot be deleted"));
    }
    if text_styles_in_use(d).iter().any(|n| n.eq_ignore_ascii_case(&name)) {
        return Err(bad("style.delete", format!("text style `{name}` is in use")));
    }
    let before = d.text_styles.len();
    d.text_styles.retain(|t| !t.name.eq_ignore_ascii_case(&name));
    if d.text_styles.len() == before {
        return Err(bad("style.delete", format!("no text style `{name}`")));
    }
    Ok(styles_json(d))
}

fn dimstyles_json(d: &cadcraft_doc::Drawing) -> Value {
    json!({ "current": d.header.str("DIMSTYLE", "Standard"), "styles": d.dim_styles.iter().map(|s| s.name.clone()).collect::<Vec<_>>() })
}

fn run_dimstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("dimstyle", "`name` is required"))?.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(bad("dimstyle", "bad style name"));
    }
    let d = s.doc_mut()?;
    let base = d.dim_style(&d.header.str("DIMSTYLE", "Standard")).cloned().unwrap_or_default();
    let idx = match d.dim_styles.iter().position(|t| t.name.eq_ignore_ascii_case(&name)) {
        Some(i) => i,
        None => {
            d.dim_styles.push(cadcraft_doc::DimStyle { name: name.clone(), ..base });
            d.dim_styles.len() - 1
        }
    };
    let mut rejected = Vec::new();
    if let (Some(st), Some(src)) = (d.dim_styles.get_mut(idx), p.as_object()) {
        let fields: serde_json::Map<String, Value> =
            src.iter().filter(|(k, _)| !matches!(k.as_str(), "name" | "current")).map(|(k, v)| (k.clone(), v.clone())).collect();
        rejected = st.apply_fields(&fields);
    }
    if bool_or(p, "current", true) {
        d.header.set_str("DIMSTYLE", &name);
    }
    let mut out = dimstyles_json(d);
    if !rejected.is_empty()
        && let Some(o) = out.as_object_mut()
    {
        o.insert("ignored".into(), json!(rejected));
    }
    Ok(out)
}

fn run_dimstyle_list(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut out = dimstyles_json(d);
    if let Some(n) = str_param(p, "name") {
        let st = d.dim_style(n).ok_or_else(|| bad("dimstyle.list", format!("no dimension style `{n}`")))?;
        let vars: serde_json::Map<String, Value> = {
            let v = serde_json::to_value(st).unwrap_or(Value::Null);
            cadcraft_doc::DIMVARS.iter().filter_map(|(var, field)| v.get(*field).map(|x| ((*var).to_string(), x.clone()))).collect()
        };
        if let Some(o) = out.as_object_mut() {
            o.insert("style".into(), serde_json::to_value(st).unwrap_or(Value::Null));
            o.insert("variables".into(), Value::Object(vars));
        }
    }
    Ok(out)
}

fn run_dimstyle_current(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("dimstyle.current", "`name` is required"))?;
    let d = s.doc_mut()?;
    let n = d.dim_style(name).map(|t| t.name.clone()).ok_or_else(|| bad("dimstyle.current", format!("no dimension style `{name}`")))?;
    d.header.set_str("DIMSTYLE", &n);
    Ok(dimstyles_json(d))
}

fn dims_using(d: &cadcraft_doc::Drawing, name: &str) -> Vec<(cadcraft_doc::Space, cadcraft_doc::Handle)> {
    let mut out = Vec::new();
    for space in std::iter::once(cadcraft_doc::Space::Model).chain(d.layouts.iter().map(|l| cadcraft_doc::Space::Paper(l.name.clone()))) {
        if let Some(st) = d.space(&space) {
            for e in st.iter() {
                let uses = match &e.kind {
                    EntityKind::Dimension(dm) => dm.style.eq_ignore_ascii_case(name),
                    EntityKind::Leader(l) => l.style.eq_ignore_ascii_case(name),
                    _ => false,
                };
                if uses {
                    out.push((space.clone(), e.handle));
                }
            }
        }
    }
    out
}

fn run_dimstyle_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").ok_or_else(|| bad("dimstyle.rename", "`from` is required"))?.to_string();
    let to = str_param(p, "to").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("dimstyle.rename", "`to` is required"))?.to_string();
    let d = s.doc_mut()?;
    if from.eq_ignore_ascii_case("Standard") {
        return Err(bad("dimstyle.rename", "the Standard style cannot be renamed"));
    }
    if d.dim_style(&to).is_some() && !to.eq_ignore_ascii_case(&from) {
        return Err(bad("dimstyle.rename", format!("a dimension style `{to}` already exists")));
    }
    let st = d.dim_styles.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&from)).ok_or_else(|| bad("dimstyle.rename", "no such dimension style"))?;
    st.name = to.clone();
    if d.header.str("DIMSTYLE", "Standard").eq_ignore_ascii_case(&from) {
        d.header.set_str("DIMSTYLE", &to);
    }
    for (_, h) in dims_using(d, &from) {
        d.modify_entity(h, |e| match &mut e.kind {
            EntityKind::Dimension(dm) => dm.style = to.clone(),
            EntityKind::Leader(l) => l.style = to.clone(),
            _ => {}
        })?;
    }
    Ok(dimstyles_json(d))
}

fn run_dimstyle_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("dimstyle.delete", "`name` is required"))?.to_string();
    let d = s.doc_mut()?;
    if name.eq_ignore_ascii_case("Standard") {
        return Err(bad("dimstyle.delete", "the Standard style cannot be deleted"));
    }
    if d.header.str("DIMSTYLE", "Standard").eq_ignore_ascii_case(&name) {
        return Err(bad("dimstyle.delete", "the current dimension style cannot be deleted"));
    }
    if !dims_using(d, &name).is_empty() {
        return Err(bad("dimstyle.delete", format!("dimension style `{name}` is in use")));
    }
    let before = d.dim_styles.len();
    d.dim_styles.retain(|t| !t.name.eq_ignore_ascii_case(&name));
    if d.dim_styles.len() == before {
        return Err(bad("dimstyle.delete", format!("no dimension style `{name}`")));
    }
    Ok(dimstyles_json(d))
}

/// Per-dimension style overrides (DIMOVERRIDE): merge fields, or `clear` them.
pub(super) fn dim_override(s: &mut Session, p: &Value) -> Result<Value> {
    let hs = targets(s, p)?;
    let clear = bool_or(p, "clear", false);
    let fields: serde_json::Map<String, Value> = p
        .as_object()
        .map(|o| o.iter().filter(|(k, _)| !matches!(k.as_str(), "handles" | "handle" | "clear")).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    // Validate against a scratch style; store canonical field names.
    let mut scratch = cadcraft_doc::DimStyle::default();
    let rejected = scratch.apply_fields(&fields);
    let good: serde_json::Map<String, Value> = fields
        .iter()
        .filter(|(k, _)| !rejected.contains(k))
        .filter_map(|(k, v)| cadcraft_doc::DimStyle::field_name(k).map(|f| (f.to_string(), v.clone())))
        .collect();
    if !clear && good.is_empty() {
        return Err(bad("dimoverride", format!("no valid dimension variables given (ignored: {})", rejected.join(", "))));
    }
    let d = s.doc_mut()?;
    let mut n = 0;
    for h in &hs {
        d.modify_entity(*h, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                if clear {
                    dm.overrides.clear();
                }
                for (k, v) in &good {
                    dm.overrides.insert(k.clone(), v.clone());
                }
                dm.block = None;
                n += 1;
            }
        })?;
    }
    Ok(json!({ "changed": n, "ignored": rejected }))
}

fn run_tablestyle(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    if let Some(name) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        let idx = match d.table_styles.iter().position(|t| t.name.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                d.table_styles.push(cadcraft_doc::TableStyle { name: name.to_string(), ..Default::default() });
                d.table_styles.len() - 1
            }
        };
        if let Some(st) = d.table_styles.get_mut(idx) {
            if let Some(h) = p.get("textHeight").and_then(Value::as_f64).filter(|h| h.is_finite() && *h > 0.0) {
                st.text_height = h;
            }
            if let Some(m) = p.get("margin").and_then(Value::as_f64).filter(|h| h.is_finite() && *h >= 0.0) {
                st.margin = m;
            }
            if let Some(b) = p.get("title").and_then(Value::as_bool) {
                st.title = b;
            }
            if let Some(b) = p.get("header").and_then(Value::as_bool) {
                st.header = b;
            }
        }
        if bool_or(p, "current", true) {
            d.header.set_str("CTABLESTYLE", name);
        }
    }
    Ok(json!({ "current": d.header.str("CTABLESTYLE", "Standard"), "styles": serde_json::to_value(&d.table_styles).unwrap_or(Value::Null) }))
}

fn run_mleaderstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    if let Some(name) = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()) {
        let idx = match d.mleader_styles.iter().position(|t| t.name.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                d.mleader_styles.push(cadcraft_doc::MLeaderStyle { name: name.to_string(), ..Default::default() });
                d.mleader_styles.len() - 1
            }
        };
        if let Some(st) = d.mleader_styles.get_mut(idx) {
            let num = |k: &str| p.get(k).and_then(Value::as_f64).filter(|h| h.is_finite() && *h >= 0.0);
            if let Some(v) = num("arrowSize") {
                st.arrow_size = v;
            }
            if let Some(v) = num("textHeight").filter(|v| *v > 0.0) {
                st.text_height = v;
            }
            if let Some(v) = num("landingGap") {
                st.landing_gap = v;
            }
            if let Some(v) = num("dogleg") {
                st.dogleg = v;
            }
            if let Some(t) = str_param(p, "textStyle") {
                st.text_style = t.to_string();
            }
        }
        if bool_or(p, "current", true) {
            d.header.set_str("CMLEADERSTYLE", name);
        }
    }
    Ok(json!({ "current": d.header.str("CMLEADERSTYLE", "Standard"), "styles": serde_json::to_value(&d.mleader_styles).unwrap_or(Value::Null) }))
}

fn run_ptype(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    if let Some(m) = p.get("pdmode").and_then(Value::as_i64) {
        d.header.set_i64("PDMODE", m);
    }
    if let Some(z) = p.get("pdsize").and_then(Value::as_f64) {
        d.header.set_f64("PDSIZE", z);
    }
    ok()
}

fn run_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let table = str_param(p, "table").unwrap_or("layer").to_ascii_lowercase();
    let from = str_param(p, "from").ok_or_else(|| bad("rename", "`from` is required"))?.to_string();
    let to = str_param(p, "to").ok_or_else(|| bad("rename", "`to` is required"))?.to_string();
    match table.as_str() {
        "layer" => {
            s.execute("layer.set", &json!({ "name": from, "newName": to }))?;
        }
        "style" => {
            run_style_rename(s, &json!({ "from": from, "to": to }))?;
        }
        "dimstyle" => {
            run_dimstyle_rename(s, &json!({ "from": from, "to": to }))?;
        }
        "linetype" => {
            let d = s.doc_mut()?;
            let st = d.linetypes.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&from)).ok_or_else(|| bad("rename", "no such linetype"))?;
            st.name = to;
        }
        _ => return Err(bad("rename", "unsupported table")),
    }
    ok()
}
