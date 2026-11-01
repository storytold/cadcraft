//! Properties: per-object property edits, current properties, CHPROP, MATCHPROP, linetypes, units,
//! RENAME.

mod change;
mod chprop;
mod dash_units;
mod matchprop;
mod rename;

pub use matchprop::MatchProps;

use cadcraft_color::Color;
use cadcraft_doc::{EntityKind, Lineweight, Transparency};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, bool_or, f64_req, find_command, ok, point_req, point_value, str_param, targets};

use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("properties", "Properties", run_get).menu(&["Modify", "Properties"]).alias(&["pr", "props", "ch"]).params("{handles?} → properties of the selection").noundo(),
        CommandSpec::new("properties.set", "Set Properties", run_set).params("{handles?, layer?, color?, linetype?, lineweight?, ltscale?, thickness?, transparency?: \"ByLayer\" | \"ByBlock\" | 0..90, visible?, <geometry fields: radius, center, start, end, text, height, rotation…>; dimensions: textOverride, dimStyle, textHeight, arrowSize, dimScale, precision (per-dimension overrides); textStyle (text, multiline text, multileaders); text: justify (L C R A M F TL TC TR ML MC MR BL BC BR; the letters stay in place), backwards, upsideDown; multiline text: justify (TL..BR), frame; hatches: pattern, scale, angle, background (colour | \"None\"); multileaders: mleaderStyle, arrow (arrowhead name or block; \"\" = the style's), arrowSize, leaderType: straight|spline|none, dogleg (landing distance; 0 = no landing), textHeight}"),
        CommandSpec::new("chprop", "Change Properties", chprop::run_chprop)
            .params("as properties.set, leaving out objects on locked layers → {changed, locked} (the command line asks Color, LAyer, LType, ltScale, LWeight, Thickness and TRansparency in turn)")
            .interactive(|_| Ok(Box::new(chprop::ChpropM::new()))),
        CommandSpec::new("change", "Change", change::run_change)
            .alias(&["-ch"])
            .params("{handles?, point?: [x, y], elevation?} → {changed, ignored, differingZ, detachedHatches, locked}: the point moves the nearer end of each line to it (level or plumb with the other end under ORTHOMODE) and, when it is the one object, passes a circle through it or places a text, attribute definition or block reference at it (`ignored` otherwise); `elevation` sets the z of objects whose points share one (`differingZ` otherwise); objects on locked layers are left out (the command line asks Specify change point or [Properties], then about each circle, text, attribute definition and block reference in turn)")
            .interactive(|_| Ok(Box::new(change::ChangeM::new()))),
        CommandSpec::new("matchprop", "Match Properties", matchprop::run_matchprop)
            .menu(&["Modify", "Match Properties"])
            .alias(&["ma", "painter"])
            .params("{source, targets?: [hex] (default: selection), properties?: [name] | {name: bool} (default: all; names: color, layer, linetype, ltscale, lineweight, transparency, thickness, plotStyle, dimension, text, hatch, polyline, viewport, table, multileader)} → {changed}")
            .interactive(|_| Ok(Box::new(matchprop::MatchPropM::new()))),
        CommandSpec::new("color", "Color...", run_color).menu(&["Format", "Color..."]).alias(&["col", "colour"]).params("{color: \"ByLayer\" | \"red\" | 1..255 | \"r,g,b\"}"),
        CommandSpec::new("linetype", "Linetype...", run_linetype).menu(&["Format", "Linetype..."]).alias(&["lt", "ltype"]).params("{current?: name, load?: name | \"*\" (with a file: comma-separated names, wildcards), file?: path to a .lin file | text?: its contents, reload?: bool}"),
        CommandSpec::new("hatch.patterns", "Hatch Patterns", super::hatch::run_patterns)
            .params("{file?: path to a .pat file | text?: its contents, clear?: bool} → {standard, user, added, warnings} (HATCH also takes patternFile | patternText)")
            .enabled(always)
            .noundo(),
        CommandSpec::new("-linetype", "Linetype", run_linetype).params("{current?, load?, file? | text?, reload?} (as LINETYPE)").interactive(|_| Ok(Box::new(linfile::DashLinetype::new()))),
        CommandSpec::new("lweight", "Lineweight...", run_lweight).menu(&["Format", "Lineweight..."]).alias(&["lw", "lineweight"]).params("{lineweight: mm | ByLayer}"),
        CommandSpec::new("ltscale", "Linetype Scale", run_ltscale).alias(&["lts"]).params("{scale}").interactive(|_| Ok(Box::new(LtscaleM))),
        CommandSpec::new("units", "Units...", run_units).menu(&["Format", "Units..."]).alias(&["un"]).params("{lunits?: 1..5, luprec?: 0..8, aunits?: 0..4, auprec?: 0..8, insunits?}"),
        CommandSpec::new("-units", "Units", run_units).alias(&["-un"]).params("(as UNITS)").interactive(|_| Ok(Box::new(dash_units::DashUnits::default()))),
        CommandSpec::new("thickness", "Thickness", run_thickness)
            .menu(&["Format", "Thickness"])
            .params("{thickness} (THICKNESS: the 3D thickness given to new lines, arcs, circles, points and polylines; may be negative) → {thickness}")
            .transparent()
            .interactive(|_| Ok(Box::new(ThicknessM))),
        CommandSpec::new("limits", "Drawing Limits", run_limits).menu(&["Format", "Drawing Limits"]).params("{min: [x,y], max: [x,y]}"),
        CommandSpec::new("style", "Text Style...", run_style)
            .menu(&["Format", "Text Style..."])
            .alias(&["st"])
            .params("{name, font?, bigFont?, height?, widthFactor?, oblique? (degrees), backwards?, upsideDown?, vertical?, annotative?, current?} → styles"),
        CommandSpec::new("style.list", "List Text Styles", run_style_list).params("{} → text styles").noundo(),
        CommandSpec::new("fonts.missing", "Missing Fonts", run_fonts_missing)
            .params("{} → [{style, font, substitute}]: text style fonts CADCraft can't find, and what it draws them with")
            .noundo(),
        CommandSpec::new("fonts.rescan", "Look for Fonts Again", run_fonts_rescan)
            .params("{} → fonts.list: look again for fonts added to or removed from the font folders")
            .noundo(),
        CommandSpec::new("fonts.list", "List Fonts", run_fonts_list)
            .params("{} → {folders, fonts: [{name, family?, user}]}: the fonts text styles can use, the user's font folders' first")
            .noundo(),
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
            .params("{name, textHeight?, margin?, title?, header?, current?, cellStyle?: {name, …} (see tablestyle.cellstyle)} → styles"),
        CommandSpec::new("mleaderstyle", "Multileader Style...", run_mleaderstyle)
            .menu(&["Format", "Multileader Style..."])
            .alias(&["mls"])
            .params("{name, arrowSize?, arrowBlock? (\"\" = closed filled, a built-in `_Open`… or a block), textHeight?, landingGap?, dogleg?, textStyle?, annotative?, current?} → styles"),
        CommandSpec::new("ddptype", "Point Style...", run_ptype)
            .menu(&["Format", "Point Style..."])
            .alias(&["ptype"])
            .params("{pdmode, pdsize}"),
        CommandSpec::new("rename", "Rename...", run_rename)
            .menu(&["Format", "Rename..."])
            .alias(&["ren", "-rename"])
            .params("{table: layer|linetype|style|dimstyle|block|mleaderstyle|tablestyle|view|ucs, from, to}")
            .interactive(|_| Ok(Box::new(rename::RenameM::default()))),
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
        "thickness": e.common.thickness,
        "visible": e.common.visible,
    });
    if cadcraft_doc::can_be_annotative(&e.kind)
        && let Some(o) = v.as_object_mut()
    {
        o.insert("annotative".into(), json!(cadcraft_doc::is_annotative(e)));
        o.insert("annotativeScales".into(), json!(e.common.anno_scales.iter().map(|s| s.name.clone()).collect::<Vec<_>>()));
    }
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
    v.as_str()
        .and_then(Color::parse)
        .or_else(|| v.as_i64().filter(|i| (0..=256).contains(i)).and_then(|i| i16::try_from(i).ok()).map(Color::from_aci))
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
    // A hatch background is a colour or none ("None", "" or null), as HATCH and HATCHEDIT take.
    if let Some(v) = p.get("background")
        && !v.is_null()
        && !v.as_str().is_some_and(|t| t.trim().is_empty() || t.trim().eq_ignore_ascii_case("none"))
        && parse_color(v).is_none()
    {
        return Err(bad("properties.set", "background must be a colour or None"));
    }
    let transparency = p
        .get("transparency")
        .map(|v| parse_transparency(v).ok_or_else(|| bad("properties.set", "transparency must be ByLayer, ByBlock or a percentage from 0 to 90")))
        .transpose()?;
    let thickness = p
        .get("thickness")
        .map(|v| v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad("properties.set", "thickness must be a finite number")))
        .transpose()?;
    let lw = p.get("lineweight").map(|v| super::layer::parse_lw(v).ok_or_else(|| bad("properties.set", "bad lineweight"))).transpose()?;
    // A new text is worked out like a typed one: a formula becomes the text's formula, other
    // text drops the one it had.
    let formula_env = crate::textformula::env(s);
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
    let mleader_style = str_param(p, "mleaderStyle")
        .map(|n| {
            d.mleader_styles
                .iter()
                .find(|st| st.name.eq_ignore_ascii_case(n))
                .map(|st| st.name.clone())
                .or_else(|| n.eq_ignore_ascii_case("Standard").then(|| "Standard".to_string()))
                .ok_or_else(|| bad("properties.set", format!("multileader style `{n}` not found")))
        })
        .transpose()?;
    let text_style = str_param(p, "textStyle")
        .map(|n| d.text_style(n).map(|st| st.name.clone()).ok_or_else(|| bad("properties.set", format!("text style `{n}` not found"))))
        .transpose()?;
    // Justification (text: L C R A M F TL..BR; multiline text: TL..BR), and the text styles it
    // lays text out with to keep the letters in place.
    let justify = str_param(p, "justify");
    let text_justify = justify.and_then(super::draw::justify);
    let mtext_justify = justify.and_then(mtext_attach);
    if let Some(j) = justify {
        if text_justify.is_none() && mtext_justify.is_none() {
            return Err(bad("properties.set", format!("unknown justification `{j}`")));
        }
        // Each selected object must have it: multiline text has no baseline justifications (C, A...).
        for e in hs.iter().filter_map(|h| d.entity(*h)) {
            let missing = match &e.kind {
                EntityKind::Text(_) => text_justify.is_none(),
                EntityKind::MText(_) => mtext_justify.is_none(),
                _ => false,
            };
            if missing {
                return Err(bad("properties.set", format!("justification `{j}` does not apply to {}", e.kind.type_name())));
            }
        }
    }
    let style_doc = justify.map(|_| cadcraft_doc::Drawing { text_styles: d.text_styles.clone(), header: d.header.clone(), ..Default::default() });
    let dim_over = dim_overrides(p)?;
    let anno_scale = d.current_anno_scale();
    let kind_props = KindProps {
        dim_style: dim_style.as_ref(),
        dim_overrides: &dim_over,
        mleader_style: mleader_style.as_ref(),
        text_style: text_style.as_ref(),
        text_justify,
        mtext_justify,
        style_doc: style_doc.as_ref(),
    };
    let common = CommonEntityProperties { anno_scale: &anno_scale, color, linetype: linetype.as_deref(), lineweight: lw, transparency, thickness };
    for h in &hs {
        // A dimension's measurement, which `<>` in a new formula stands for.
        let formula_env = match d.entity(*h).map(|e| &e.kind) {
            Some(EntityKind::Dimension(dm)) => crate::textformula::Env { measured: crate::textformula::measured(d, dm), ..formula_env.clone() },
            _ => formula_env.clone(),
        };
        d.modify_entity(*h, |e| {
            set_common_properties(e, p, &common);
            if apply_kind_properties(e, p, &kind_props) {
                crate::textformula::apply(&mut e.kind, &formula_env, true);
            }
        })?;
    }
    Ok(json!({ "changed": hs.len() }))
}

/// The checked kind-specific settings of a `properties.set` call.
struct KindProps<'a> {
    dim_style: Option<&'a String>,
    dim_overrides: &'a serde_json::Map<String, Value>,
    mleader_style: Option<&'a String>,
    text_style: Option<&'a String>,
    text_justify: Option<(cadcraft_doc::HAlign, cadcraft_doc::VAlign)>,
    mtext_justify: Option<u8>,
    /// The text styles and header justification lays text out with.
    style_doc: Option<&'a cadcraft_doc::Drawing>,
}

/// The text style of text, multiline text, a multileader's text and an attribute definition.
fn set_text_style(entity: &mut cadcraft_doc::Entity, st: &str) {
    match &mut entity.kind {
        EntityKind::Text(t) => t.style = st.to_string(),
        EntityKind::MText(t) => t.style = st.to_string(),
        EntityKind::MLeader(m) => {
            if let Some(t) = &mut m.text {
                t.style = st.to_string();
            }
        }
        EntityKind::AttDef(a) => a.text.style = st.to_string(),
        _ => {}
    }
}

/// Set a single-line text's properties; whether a text was given.
fn set_text_properties(t: &mut cadcraft_doc::Text, params: &Value, k: &KindProps<'_>) -> bool {
    let num = |key: &str| params.get(key).and_then(Value::as_f64).filter(|value| value.is_finite());
    let mut changed = false;
    if let Some(v) = str_param(params, "text") {
        t.value = v.to_string();
        changed = true;
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
    if let Some(f) = params.get("backwards").and_then(Value::as_bool) {
        t.backwards = f;
    }
    if let Some(f) = params.get("upsideDown").and_then(Value::as_bool) {
        t.upside_down = f;
    }
    if let (Some((h, v)), Some(sd)) = (k.text_justify, k.style_doc) {
        rejustify_text(sd, t, h, v);
    }
    // The position is the point the text is placed by: moving it moves the text.
    if let Some(to) = params.get("position").and_then(point_value) {
        let d = to - t.justify_point();
        for q in std::iter::once(&mut t.insert).chain(t.align_pt.as_mut()) {
            q.x += d.x;
            q.y += d.y;
        }
    }
    changed
}

/// Set a circle's centre and size (radius, diameter, circumference or area).
fn set_circle_properties(c: &mut cadcraft_doc::Circle, params: &Value) {
    let num = |key: &str| params.get(key).and_then(Value::as_f64).filter(|value| value.is_finite());
    if let Some(v) = params.get("center") {
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

/// Set the kind's own properties; whether a text was given (a new value, worked out as typed).
fn apply_kind_properties(entity: &mut cadcraft_doc::Entity, params: &Value, k: &KindProps<'_>) -> bool {
    let num = |key: &str| params.get(key).and_then(Value::as_f64).filter(|value| value.is_finite());
    let mut new_text = false;
    if let Some(st) = k.text_style {
        set_text_style(entity, st);
    }
    match &mut entity.kind {
        EntityKind::Line(l) => {
            if let Some(v) = params.get("start") {
                set_xy(&mut l.a, v);
            }
            if let Some(v) = params.get("end") {
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
        EntityKind::Circle(c) => set_circle_properties(c, params),
        EntityKind::Arc(a) => {
            if let Some(v) = params.get("center") {
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
        EntityKind::Text(t) => new_text = set_text_properties(t, params, k),
        EntityKind::MText(t) => {
            if let Some(v) = str_param(params, "text") {
                t.contents = v.replace('\n', "\\P");
                new_text = true;
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
            if let Some(f) = params.get("frame").and_then(Value::as_bool) {
                t.frame = f;
            }
            if let (Some(a), Some(sd)) = (k.mtext_justify, k.style_doc) {
                rejustify_mtext(sd, t, a);
            }
        }
        EntityKind::LwPolyline(pl) => {
            if let Some(x) = num("width").filter(|x| *x >= 0.0) {
                pl.const_width = x;
            }
            if let Some(c) = params.get("closed").and_then(Value::as_bool) {
                pl.closed = c;
            }
        }
        EntityKind::Insert(insert) => set_insert_properties(insert, params),
        EntityKind::Dimension(dimension) => new_text = set_dimension_properties(dimension, params, k.dim_style, k.dim_overrides),
        EntityKind::Hatch(hatch) => set_hatch_properties(hatch, params),
        EntityKind::MLeader(m) => {
            if let Some(st) = k.mleader_style {
                m.style.clone_from(st);
            }
            if let Some(v) = str_param(params, "arrow") {
                m.arrow = v.trim().to_string();
            }
            if let Some(x) = num("arrowSize").filter(|x| *x >= 0.0) {
                m.arrow_size = x;
            }
            match str_param(params, "leaderType").map(str::to_ascii_lowercase).as_deref() {
                Some("spline") => (m.spline, m.no_leader) = (true, false),
                Some("straight") => (m.spline, m.no_leader) = (false, false),
                Some("none") => m.no_leader = true,
                _ => {}
            }
            if let Some(x) = num("dogleg").filter(|x| *x >= 0.0) {
                // The content stays at the end of the landing.
                let land = m.landing.x;
                let side = |x: f64| if x >= land { 1.0 } else { -1.0 };
                let shift = x - m.dogleg;
                if let Some(t) = &mut m.text {
                    t.insert.x += shift * side(t.insert.x);
                }
                if let Some(b) = &mut m.block {
                    b.insert.x += shift * side(b.insert.x);
                }
                m.dogleg = x;
            }
            if let (Some(x), Some(t)) = (num("textHeight").filter(|x| *x > 0.0), &mut m.text) {
                t.height = x;
            }
        }
        _ => {}
    }
    new_text
}

fn set_insert_properties(insert: &mut cadcraft_doc::Insert, params: &Value) {
    let number = |key: &str| params.get(key).and_then(Value::as_f64).filter(|value| value.is_finite());

    if let Some(x) = number("rotation") {
        let turn = x.to_radians() - insert.rotation;
        insert.rotation = x.to_radians();
        // The attached text turns with the reference, around its insertion point (as ROTATE does).
        let base = insert.insert.xy();
        for a in &mut insert.attribs {
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
    }
    if let Some(x) = number("scale").filter(|x| *x != 0.0) {
        // A new positive scale carries the attached attribute text with it, like SCALE about the insertion point.
        if x > 0.0 && insert.scale.x > 0.0 {
            let m = cadcraft_geom::Mat3::scale_about(insert.insert.xy(), x / insert.scale.x);
            for a in &mut insert.attribs {
                let mut k = EntityKind::Text(a.text.clone());
                k.transform(&m);
                if let EntityKind::Text(t) = k {
                    a.text = t;
                }
                a.transform_multiline(&m);
            }
        }
        insert.scale = Vec3::new(x, x, x);
    }
}

/// Set a dimension's properties; whether its text was replaced (like typing over it: a formula
/// becomes the formula, other text drops it).
fn set_dimension_properties(
    dimension: &mut cadcraft_doc::Dimension,
    params: &Value,
    style: Option<&String>,
    overrides: &serde_json::Map<String, Value>,
) -> bool {
    let mut new_text = false;
    if let Some(v) = str_param(params, "textOverride") {
        dimension.text = v.to_string();
        dimension.block = None;
        new_text = true;
    }
    if let Some(st) = style {
        dimension.style.clone_from(st);
        dimension.block = None;
    }
    if !overrides.is_empty() {
        dimension.overrides.extend(overrides.clone());
        dimension.block = None;
    }
    new_text
}

fn set_hatch_properties(hatch: &mut cadcraft_doc::Hatch, params: &Value) {
    let number = |key: &str| params.get(key).and_then(Value::as_f64).filter(|value| value.is_finite());

    if let Some(v) = str_param(params, "pattern") {
        if !hatch.pattern.eq_ignore_ascii_case(v) {
            hatch.pattern_lines = super::hatch::lines_for(v);
        }
        hatch.pattern = v.to_ascii_uppercase();
        // The old pattern's definition lines no longer apply.
        hatch.pattern_lines.clear();
        hatch.solid = hatch.pattern == "SOLID";
        if !hatch.solid {
            // A named pattern replaces the gradient fill; the renderer draws the gradient first when it is set.
            hatch.gradient = None;
        }
    }
    if let Some(x) = number("scale").filter(|x| *x > 0.0) {
        hatch.scale = x;
    }
    if let Some(x) = number("angle") {
        hatch.angle = x.to_radians();
    }
    // Background colour: a colour fills the area under the pattern; "None" none.
    match params.get("background") {
        Some(Value::Null) => hatch.background = None,
        Some(v) if v.as_str().is_some_and(|s| s.trim().eq_ignore_ascii_case("none") || s.trim().is_empty()) => hatch.background = None,
        Some(v) => {
            if let Some(c) = parse_color(v) {
                hatch.background = Some(c);
            }
        }
        None => {}
    }
}

struct CommonEntityProperties<'a> {
    anno_scale: &'a cadcraft_doc::AnnoScale,
    color: Option<Color>,
    linetype: Option<&'a str>,
    lineweight: Option<Lineweight>,
    transparency: Option<Transparency>,
    thickness: Option<f64>,
}

fn set_common_properties(entity: &mut cadcraft_doc::Entity, params: &Value, common: &CommonEntityProperties<'_>) {
    if let Some(annotative) = params.get("annotative").and_then(Value::as_bool) {
        cadcraft_doc::set_annotative(entity, annotative, common.anno_scale);
    }
    if let Some(layer) = str_param(params, "layer") {
        entity.common.layer = layer.to_string();
    }
    if let Some(color) = common.color {
        entity.common.color = color;
    }
    if let Some(linetype) = common.linetype {
        entity.common.linetype = linetype.to_string();
    }
    if let Some(lineweight) = common.lineweight {
        entity.common.lineweight = lineweight;
    }
    if let Some(scale) = params.get("ltscale").and_then(Value::as_f64).filter(|scale| *scale > 0.0) {
        entity.common.ltscale = scale;
    }
    if let Some(transparency) = common.transparency {
        entity.common.transparency = transparency;
    }
    if let Some(thickness) = common.thickness {
        entity.common.thickness = thickness;
    }
    if let Some(visible) = params.get("visible").and_then(Value::as_bool) {
        entity.common.visible = visible;
    }
}

fn run_color(s: &mut Session, p: &Value) -> Result<Value> {
    let c = p.get("color").and_then(parse_color).ok_or_else(|| bad("color", "`color` is required"))?;
    set_current_color(s.doc_mut()?, c);
    ok()
}

/// `setvar CECOLOR`: the same input as the `color` command, stored the same way.
pub(crate) fn set_current_color_value(s: &mut Session, v: &Value) -> Option<()> {
    // An out-of-range index is refused here, where `color` folds it to ByLayer.
    let c = match v {
        // "RGB:r,g,b" is how `getvar` reads a true colour back, so a saved value can be restored.
        Value::String(t) => match t.trim().get(..4).filter(|p| p.eq_ignore_ascii_case("rgb:")).and_then(|_| t.trim().get(4..)) {
            Some(triple) => Color::parse(triple).filter(|c| matches!(c, Color::True(_))),
            None => Color::parse(t),
        },
        Value::Number(n) => n.as_i64().and_then(|i| Color::parse(&i.to_string())),
        _ => None,
    }?;
    set_current_color(s.doc_mut().ok()?, c);
    Some(())
}

/// `getvar CECOLOR` as AutoCAD gives it, always text: "BYLAYER", "BYBLOCK", the index ("1" for
/// red) or "RGB:r,g,b".
pub(crate) fn current_color_value(d: &cadcraft_doc::Drawing) -> String {
    if let Some(rgb) = d.header.get("CECOLOR_RGB").and_then(|v| v.as_i64()).and_then(|v| u32::try_from(v).ok()) {
        let cadcraft_color::Rgb(r, g, b) = cadcraft_color::Rgb::from_u32(rgb);
        return format!("RGB:{r},{g},{b}");
    }
    match Color::from_aci(d.header.i64("CECOLOR", 256) as i16) {
        Color::ByLayer => "BYLAYER".into(),
        Color::ByBlock => "BYBLOCK".into(),
        other => other.to_aci().to_string(),
    }
}

/// A true colour keeps its RGB in CECOLOR_RGB (which wins for new objects); any other colour must
/// clear a stale one.
fn set_current_color(d: &mut cadcraft_doc::Drawing, c: Color) {
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
}

mod linfile;

fn run_linetype(s: &mut Session, p: &Value) -> Result<Value> {
    if linfile::has_file(p) {
        return linfile::run(s, p);
    }
    let d = s.doc_mut()?;
    if let Some(load) = str_param(p, "load") {
        let lib = cadcraft_doc::library::standard_linetypes_for(d);
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

/// Typed LTSCALE: "Enter new linetype scale factor <1.0000>:".
struct LtscaleM;

impl Interactive for LtscaleM {
    fn name(&self) -> &'static str {
        "LTSCALE"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = s.doc().map(|d| d.header.f64("LTSCALE", 1.0)).unwrap_or(1.0);
        Prompt::new("Enter new linetype scale factor", Accept::NUMBER).default(format!("{cur:.4}"))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match i {
            Input::Enter | Input::Cancel => return Ok(Step::Done),
            Input::Text(t) => t,
            _ => return Ok(Step::Continue),
        };
        let scale = text.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0);
        match scale.map(|v| run_ltscale(s, &json!({ "scale": v }))) {
            Some(Ok(r)) => {
                if let Some(m) = r.get("message").and_then(Value::as_str) {
                    s.echo(m.to_string());
                }
                Ok(Step::Done)
            }
            _ => {
                s.echo("Requires a positive number.");
                Ok(Step::Continue)
            }
        }
    }
}

/// The largest THICKNESS, either way.
const MAX_THICKNESS: f64 = 1e12;

fn thickness_in_range(t: f64) -> bool {
    t.is_finite() && t.abs() <= MAX_THICKNESS
}

fn run_thickness(s: &mut Session, p: &Value) -> Result<Value> {
    let t = f64_req("thickness", p, "thickness")?;
    if !thickness_in_range(t) {
        return Err(bad("thickness", "thickness is out of range (-1e12 to 1e12)"));
    }
    s.doc_mut()?.header.set_f64("THICKNESS", t);
    Ok(json!({ "thickness": t }))
}

/// Format > Thickness and typed THICKNESS: "Enter new value for THICKNESS <0.0000>:".
struct ThicknessM;

impl Interactive for ThicknessM {
    fn name(&self) -> &'static str {
        "THICKNESS"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let cur = s.doc().map(|d| d.header.f64("THICKNESS", 0.0)).unwrap_or(0.0);
        Prompt::new("Enter new value for THICKNESS", Accept::NUMBER).default(format!("{cur:.4}"))
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let text = match i {
            Input::Enter | Input::Cancel => return Ok(Step::Done),
            Input::Text(t) => t,
            _ => return Ok(Step::Continue),
        };
        match super::machines::number(&text) {
            Some(t) if thickness_in_range(t) => {
                run_thickness(s, &json!({ "thickness": t }))?;
                Ok(Step::Done)
            }
            Some(_) => {
                s.echo("Requires a value between -1e12 and 1e12.");
                Ok(Step::Continue)
            }
            None => {
                s.echo("Requires a number.");
                Ok(Step::Continue)
            }
        }
    }
}

/// The unit variables (lower-case name, lowest, highest value) shared by UNITS and SETVAR.
pub(crate) const UNIT_RANGES: [(&str, i64, i64); 5] = [("lunits", 1, 5), ("luprec", 0, 8), ("aunits", 0, 4), ("auprec", 0, 8), ("insunits", 0, 24)];

fn run_units(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    for (k, lo, hi) in UNIT_RANGES {
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

/// The font a style font that can't be found is drawn with: FONTALT when it is found, else the
/// built-in stroke font.
pub fn substitute_font() -> String {
    let alt = cadcraft_fonts::ttf::font_alt();
    if !alt.is_empty() && cadcraft_fonts::ttf::find(&alt).is_some() { alt } else { cadcraft_fonts::BUILTIN_FONT.to_string() }
}

/// Text style fonts (and big fonts) of `d` that CADCraft can't find, once each.
pub fn missing_fonts(d: &cadcraft_doc::Drawing) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for st in &d.text_styles {
        // The font, and the big font (Asian characters) an SHX style may add.
        let font = st.font.trim();
        let big = st.big_font.trim();
        let missing_big = !big.is_empty() && cadcraft_fonts::shx::find(big).is_none();
        for (f, missing) in [(font, !cadcraft_fonts::ttf::available(font)), (big, missing_big)] {
            if missing && !out.iter().any(|(_, o)| o.eq_ignore_ascii_case(f)) {
                out.push((st.name.clone(), f.to_string()));
            }
        }
    }
    out
}

fn run_fonts_missing(s: &mut Session, _p: &Value) -> Result<Value> {
    let sub = substitute_font();
    Ok(json!(missing_fonts(s.doc()?).into_iter().map(|(style, font)| json!({ "style": style, "font": font, "substitute": sub })).collect::<Vec<_>>()))
}

fn run_fonts_rescan(s: &mut Session, p: &Value) -> Result<Value> {
    cadcraft_fonts::ttf::rescan();
    run_fonts_list(s, p)
}

fn run_fonts_list(_s: &mut Session, _p: &Value) -> Result<Value> {
    let fonts: Vec<Value> =
        cadcraft_fonts::ttf::installed().into_iter().map(|f| json!({ "name": f.name, "family": f.family, "user": f.user })).collect();
    Ok(json!({ "folders": cadcraft_fonts::ttf::font_dirs(), "fonts": fonts }))
}

fn run_style_current(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("style.current", "`name` is required"))?;
    let d = s.doc_mut()?;
    let n = d.text_style(name).map(|t| t.name.clone()).ok_or_else(|| bad("style.current", format!("no text style `{name}`")))?;
    d.header.set_str("TEXTSTYLE", &n);
    ok()
}

/// Text style names used by entities (in model, paper spaces and blocks) and dimension styles.
pub(super) fn text_styles_in_use(d: &cadcraft_doc::Drawing) -> Vec<String> {
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
    let uses = |k: &EntityKind| match k {
        EntityKind::Text(t) => t.style.eq_ignore_ascii_case(&from),
        EntityKind::MText(t) => t.style.eq_ignore_ascii_case(&from),
        EntityKind::AttDef(a) => a.text.style.eq_ignore_ascii_case(&from),
        EntityKind::Insert(i) => i.attribs.iter().any(|a| a.text.style.eq_ignore_ascii_case(&from)),
        EntityKind::MLeader(m) => m.text.iter().any(|t| t.style.eq_ignore_ascii_case(&from)),
        _ => false,
    };
    let repoint = |k: &mut EntityKind| match k {
        EntityKind::Text(t) => fix(&mut t.style),
        EntityKind::MText(t) => fix(&mut t.style),
        EntityKind::AttDef(a) => fix(&mut a.text.style),
        EntityKind::Insert(i) => i.attribs.iter_mut().for_each(|a| fix(&mut a.text.style)),
        EntityKind::MLeader(m) => m.text.iter_mut().for_each(|t| fix(&mut t.style)),
        _ => {}
    };
    let mut hs: Vec<cadcraft_doc::Handle> = d.model.iter().filter(|e| uses(&e.kind)).map(|e| e.handle).collect();
    hs.extend(d.layouts.iter().flat_map(|l| l.entities.iter().filter(|e| uses(&e.kind)).map(|e| e.handle)));
    for h in hs {
        d.modify_entity(h, |e| repoint(&mut e.kind))?;
    }
    // Block definitions too, so their text keeps resolving to the renamed style.
    for block in d.blocks.values_mut() {
        let hs: Vec<cadcraft_doc::Handle> = block.entities.iter().filter(|e| uses(&e.kind)).map(|e| e.handle).collect();
        if !hs.is_empty() {
            let b = std::sync::Arc::make_mut(block);
            for h in hs {
                b.entities.modify(h, |e| repoint(&mut e.kind));
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
    // Editing or making the current style current clears the SETVAR overrides.
    if d.header.str("DIMSTYLE", "Standard").eq_ignore_ascii_case(&name) {
        d.sync_dim_vars();
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
    d.sync_dim_vars();
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
                    EntityKind::Tolerance(t) => t.style.eq_ignore_ascii_case(name),
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

/// Dimensions and leaders inside block definitions that use the style (block name, entity handle).
fn block_dims_using(d: &cadcraft_doc::Drawing, name: &str) -> Vec<(String, cadcraft_doc::Handle)> {
    let mut out = Vec::new();
    for (bname, b) in &d.blocks {
        for e in b.entities.iter() {
            let uses = match &e.kind {
                EntityKind::Dimension(dm) => dm.style.eq_ignore_ascii_case(name),
                EntityKind::Leader(l) => l.style.eq_ignore_ascii_case(name),
                EntityKind::Tolerance(t) => t.style.eq_ignore_ascii_case(name),
                _ => false,
            };
            if uses {
                out.push((bname.clone(), e.handle));
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
            EntityKind::Tolerance(t) => t.style = to.clone(),
            _ => {}
        })?;
    }
    for (bname, h) in block_dims_using(d, &from) {
        if let Some(b) = d.blocks.get_mut(&bname) {
            std::sync::Arc::make_mut(b).entities.modify(h, |e| match &mut e.kind {
                EntityKind::Dimension(dm) => dm.style = to.clone(),
                EntityKind::Leader(l) => l.style = to.clone(),
                _ => {}
            });
        }
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
    if !dims_using(d, &name).is_empty() || !block_dims_using(d, &name).is_empty() {
        return Err(bad("dimstyle.delete", format!("dimension style `{name}` is in use")));
    }
    let before = d.dim_styles.len();
    d.dim_styles.retain(|t| !t.name.eq_ignore_ascii_case(&name));
    if d.dim_styles.len() == before {
        return Err(bad("dimstyle.delete", format!("no dimension style `{name}`")));
    }
    Ok(dimstyles_json(d))
}

/// A dimension's text height, arrow size, overall scale (DIMSCALE) and precision as Properties
/// sets them: per-dimension overrides, as in AutoCAD's Properties palette.
fn dim_overrides(p: &Value) -> Result<serde_json::Map<String, Value>> {
    let mut o = serde_json::Map::new();
    for (key, field, zero_ok) in [("textHeight", "textHeight", false), ("arrowSize", "arrowSize", true), ("dimScale", "scale", false)] {
        if let Some(v) = p.get(key).filter(|v| !v.is_null()) {
            let x = v
                .as_f64()
                .filter(|x| x.is_finite() && (*x > 0.0 || (zero_ok && *x == 0.0)))
                .ok_or_else(|| bad("properties.set", format!("`{key}` must be a positive number")))?;
            o.insert(field.into(), json!(x));
        }
    }
    if let Some(v) = p.get("precision").filter(|v| !v.is_null()) {
        let n = v.as_u64().filter(|n| *n <= 8).ok_or_else(|| bad("properties.set", "`precision` must be 0 to 8 decimal places"))?;
        o.insert("decimals".into(), json!(n));
    }
    Ok(o)
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
        super::table::tablestyle_cell_style(d, name, p)?;
        if bool_or(p, "current", true) {
            d.header.set_str("CTABLESTYLE", name);
        }
    }
    Ok(json!({ "current": d.header.str("CTABLESTYLE", "Standard"), "styles": serde_json::to_value(&d.table_styles).unwrap_or(Value::Null) }))
}

fn run_mleaderstyle(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc_mut()?;
    // The arrowhead: closed filled (empty or `_ClosedFilled`), a built-in one or a block of the
    // drawing.
    let arrow = match str_param(p, "arrowBlock").map(str::trim) {
        None => None,
        Some(a) if a.is_empty() || a.eq_ignore_ascii_case("_ClosedFilled") || a.eq_ignore_ascii_case("ClosedFilled") => Some(String::new()),
        Some(a) if a.starts_with('_') || d.blocks.keys().any(|k| k.eq_ignore_ascii_case(a)) => Some(a.to_string()),
        Some(a) => return Err(bad("mleaderstyle", format!("arrowhead block `{a}` not found"))),
    };
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
            if let Some(a) = &arrow {
                st.arrow_block.clone_from(a);
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
            if let Some(a) = p.get("annotative").and_then(Value::as_bool) {
                st.annotative = a;
            }
        }
        if bool_or(p, "current", true) {
            d.header.set_str("CMLEADERSTYLE", name);
        }
    }
    Ok(json!({ "current": d.header.str("CMLEADERSTYLE", "Standard"), "styles": serde_json::to_value(&d.mleader_styles).unwrap_or(Value::Null) }))
}

fn run_ptype(s: &mut Session, p: &Value) -> Result<Value> {
    // PDMODE is a shape 0..=4 plus 32 (circle) and/or 64 (square).
    let mode = match p.get("pdmode") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_i64().filter(|m| (0..=100).contains(m) && m & !96 <= 4).ok_or_else(|| bad("ddptype", "`pdmode` must be 0..4, plus 32 and/or 64"))?,
        ),
    };
    let d = s.doc_mut()?;
    if let Some(m) = mode {
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
            // The command body, not `execute`: RENAME records the one undo step.
            let set = find_command("layer.set").ok_or_else(|| bad("rename", "no layer.set command"))?;
            (set.run)(s, &json!({ "name": from, "newName": to }))?;
        }
        "block" => rename::block(s, &from, &to)?,
        "mleaderstyle" | "multileaderstyle" => rename::simple(s, "mleaderstyle", &from, &to)?,
        "tablestyle" | "view" | "ucs" => rename::simple(s, &table, &from, &to)?,
        "style" | "textstyle" => {
            run_style_rename(s, &json!({ "from": from, "to": to }))?;
        }
        "dimstyle" => {
            run_dimstyle_rename(s, &json!({ "from": from, "to": to }))?;
        }
        "linetype" | "ltype" => {
            // Stored trimmed, as -RENAME types it: lookups compare names untrimmed.
            let to = to.trim().to_string();
            if !rename::valid_name(&to) {
                return Err(bad("rename", format!("`{to}` is not a valid linetype name")));
            }
            let d = s.doc_mut()?;
            if ["ByBlock", "ByLayer", "Continuous"].iter().any(|n| from.eq_ignore_ascii_case(n)) {
                return Err(bad("rename", format!("the {from} linetype cannot be renamed")));
            }
            if d.linetype(&to).is_some() && !to.eq_ignore_ascii_case(&from) {
                return Err(bad("rename", format!("a linetype `{to}` already exists")));
            }
            let st = d.linetypes.iter_mut().find(|t| t.name.eq_ignore_ascii_case(&from)).ok_or_else(|| bad("rename", "no such linetype"))?;
            st.name = to.clone();
            // References follow the new name.
            let fix = |n: &mut String| {
                if n.eq_ignore_ascii_case(&from) {
                    *n = to.clone();
                }
            };
            d.layers.iter_mut().for_each(|l| fix(&mut l.linetype));
            if d.header.str("CELTYPE", "ByLayer").eq_ignore_ascii_case(&from) {
                d.header.set_str("CELTYPE", &to);
            }
            let users = |st: &cadcraft_doc::EntityStore| -> Vec<cadcraft_doc::Handle> {
                st.iter().filter(|e| e.common.linetype.eq_ignore_ascii_case(&from)).map(|e| e.handle).collect()
            };
            let mut hs = users(&d.model);
            hs.extend(d.layouts.iter().flat_map(|l| users(&l.entities)));
            for h in hs {
                d.modify_entity(h, |e| fix(&mut e.common.linetype))?;
            }
            for block in d.blocks.values_mut() {
                let hs = users(&block.entities);
                if !hs.is_empty() {
                    let b = std::sync::Arc::make_mut(block);
                    for h in hs {
                        b.entities.modify(h, |e| fix(&mut e.common.linetype));
                    }
                }
            }
        }
        _ => return Err(bad("rename", "unsupported table")),
    }
    ok()
}

/// An MTEXT attachment (1..=9) from a justification code: TL TC TR ML MC MR BL BC BR (or 1..9).
fn mtext_attach(j: &str) -> Option<u8> {
    let u = j.trim().to_ascii_uppercase();
    ["TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"]
        .iter()
        .position(|c| *c == u)
        .and_then(|i| u8::try_from(i + 1).ok())
        .or_else(|| u.parse::<u8>().ok().filter(|a| (1..=9).contains(a)))
}

/// Give text another justification without moving it: the point it is placed by moves so the
/// letters stay where they are (aligned and fit text span the text's own width).
fn rejustify_text(d: &cadcraft_doc::Drawing, t: &mut cadcraft_doc::Text, h: cadcraft_doc::HAlign, v: cadcraft_doc::VAlign) {
    use cadcraft_doc::{HAlign, VAlign};
    let before = cadcraft_render::place_text_entity(d, t, &t.value).1;
    // The baseline's start: where left-baseline text at the same spot would begin.
    let mut left = t.clone();
    (left.halign, left.valign, left.align_pt) = (HAlign::Left, VAlign::Baseline, None);
    let at = t.justify_point();
    left.insert = cadcraft_geom::Vec3::new(at.x, at.y, t.insert.z);
    let lb = cadcraft_render::place_text_entity(d, &left, &t.value).1;
    let start = if before.is_empty() || lb.is_empty() { t.insert.xy() } else { at + (before.min - lb.min) };
    let width = match t.halign {
        HAlign::Aligned | HAlign::Fit => t.align_pt.map_or(0.0, |a| a.xy().dist(t.insert.xy())),
        _ => {
            let font = d.text_style(&t.style).map(|st| cadcraft_fonts::TextFont::resolve_style(&st.font, &st.big_font)).unwrap_or_default();
            cadcraft_fonts::text_width(&font, &t.value, t.height, t.width_factor)
        }
    };
    let z = t.insert.z;
    let v3 = |p: Vec2| cadcraft_geom::Vec3::new(p.x, p.y, z);
    t.halign = h;
    t.valign = v;
    t.insert = v3(start);
    match (h, v) {
        (HAlign::Left, VAlign::Baseline) => t.align_pt = None,
        (HAlign::Aligned | HAlign::Fit, _) => {
            t.valign = VAlign::Baseline;
            t.align_pt = Some(v3(start + Vec2::from_angle(t.rotation) * width.max(t.height * 0.1)));
        }
        _ => {
            // Place by the start, then shift by how far the letters moved.
            t.align_pt = Some(v3(start));
            let after = cadcraft_render::place_text_entity(d, t, &t.value).1;
            if !before.is_empty() && !after.is_empty() {
                let shift = before.min - after.min;
                if shift.is_finite() {
                    for q in std::iter::once(&mut t.insert).chain(t.align_pt.as_mut()) {
                        q.x += shift.x;
                        q.y += shift.y;
                    }
                }
            }
        }
    }
}

/// Give multiline text another attachment without moving it: the insertion point moves to the
/// matching point of the text's box.
fn rejustify_mtext(d: &cadcraft_doc::Drawing, t: &mut cadcraft_doc::MText, attach: u8) {
    // Laid out unrotated, at the same insertion point: the letters' shift between the two
    // attachments, turned back by the rotation.
    let mut flat = t.clone();
    flat.rotation = 0.0;
    let before = cadcraft_render::layout_mtext_entity(d, &flat).bounds;
    flat.attach = attach;
    let after = cadcraft_render::layout_mtext_entity(d, &flat).bounds;
    t.attach = attach;
    if before.is_empty() || after.is_empty() {
        return;
    }
    let shift = (before.min - after.min).rotate(t.rotation);
    if shift.is_finite() {
        t.insert.x += shift.x;
        t.insert.y += shift.y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn properties_set_rejects_out_of_range_colour() {
        let mut s = Session::new();
        s.execute("line", &json!({ "points": [[0, 0], [50, 0]] })).unwrap();
        let h = s.doc().unwrap().model.last().unwrap().handle.hex();
        s.execute("properties.set", &json!({ "handles": [h], "color": 1 })).unwrap();
        let col = |s: &Session| s.doc().unwrap().model.last().unwrap().common.color;
        for bad in [300, -5, 257] {
            let e = s.execute("properties.set", &json!({ "handles": [h], "color": bad })).unwrap_err();
            assert!(e.to_string().contains("bad colour"), "{e}");
            assert_eq!(col(&s), Color::Index(1));
        }
        s.execute("properties.set", &json!({ "handles": [h], "color": 256 })).unwrap();
        assert_eq!(col(&s), Color::ByLayer);
        s.execute("properties.set", &json!({ "handles": [h], "color": 0 })).unwrap();
        assert_eq!(col(&s), Color::ByBlock);
    }

    #[test]
    fn rename_linetype_rejects_invalid_names() {
        let mut s = Session::new();
        s.execute("linetype", &json!({ "load": "DASHED" })).unwrap();
        for to in ["", "  ", "A;B", "A|B", "*X"] {
            let r = s.execute("rename", &json!({ "table": "linetype", "from": "DASHED", "to": to }));
            assert!(r.is_err(), "renamed DASHED to {to:?}: {r:?}");
            assert!(s.doc().unwrap().linetype("DASHED").is_some());
        }
        s.execute("rename", &json!({ "table": "linetype", "from": "DASHED", "to": " DASH2 " })).unwrap();
        assert!(s.doc().unwrap().linetype("DASH2").is_some());
    }

    #[test]
    fn properties_set_rejects_unloaded_linetype() {
        let mut s = Session::new();
        s.execute("linetype", &json!({ "load": "DASHED" })).unwrap();
        s.execute("line", &json!({ "points": [[0, 0], [50, 0]] })).unwrap();
        let h = s.doc().unwrap().model.last().unwrap().handle.hex();
        s.execute("properties.set", &json!({ "handles": [h], "linetype": "DASHED" })).unwrap();
        assert!(s.execute("properties.set", &json!({ "handles": [h], "linetype": "NOPE" })).is_err());
        let lt = |s: &Session| s.doc().unwrap().model.last().unwrap().common.linetype.clone();
        assert_eq!(lt(&s), "DASHED");
        for ok in ["ByBlock", "ByLayer"] {
            s.execute("properties.set", &json!({ "handles": [h], "linetype": ok })).unwrap();
            assert_eq!(lt(&s), ok);
        }
    }
}
