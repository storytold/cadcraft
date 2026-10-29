use super::common::T;
use super::objects::MAX_OBJECTS;
use super::{
    Color, DashElement, DimStyle, Drawing, HVal, HashMap, Layer, Linetype, Lineweight, NamedView, Rgb, Rx, Tag, TextStyle, Ucs, Vec2, Vec3,
    VportConfig, VportTile, records,
};

pub(super) fn header(tags: &[Tag], d: &mut Drawing) {
    let mut i = 0;
    while let Some(t) = tags.get(i) {
        if t.code == 9 {
            let name = t.str().trim_start_matches('$').to_ascii_uppercase();
            i += 1;
            let mut vals: Vec<&Tag> = Vec::new();
            while let Some(v) = tags.get(i) {
                if v.code == 9 {
                    break;
                }
                vals.push(v);
                i += 1;
            }
            // Legacy files keep spacing in HEADER; modern *ACTIVE VPORT entries override it.
            if matches!(name.as_str(), "SNAPUNIT" | "GRIDUNIT") {
                let (Some(x), Some(y)) = (vals.iter().find(|t| t.code == 10), vals.iter().find(|t| t.code == 20)) else { continue };
                let p = Vec2::new(x.f64(), y.f64());
                if p.is_finite() && if name == "GRIDUNIT" { p.x >= 0.0 && p.y >= 0.0 } else { p.x > 0.0 && p.y > 0.0 } {
                    d.header.set(&name, HVal::Point(p.to3(0.0)));
                }
                continue;
            }
            let hv = match vals.as_slice() {
                [x, y, z] if x.code == 10 => HVal::Point(Vec3::new(x.f64(), y.f64(), z.f64())),
                [x, y] if x.code == 10 => HVal::Point(Vec3::new(x.f64(), y.f64(), 0.0)),
                [v] => match &v.value {
                    cadcraft_dxf::Value::Real(r) => HVal::Real(*r),
                    cadcraft_dxf::Value::Int(n) => HVal::Int(*n),
                    cadcraft_dxf::Value::Bool(b) => HVal::Int(i64::from(*b)),
                    other => HVal::Str(other.as_str()),
                },
                _ => continue,
            };
            // $DIMDSEP holds a character code; the dimension style holds the character.
            let hv = match hv {
                HVal::Int(n) if name == "DIMDSEP" => match u32::try_from(n).ok().and_then(char::from_u32).filter(|c| !c.is_control()) {
                    Some(c) => HVal::Str(c.to_string()),
                    None => continue,
                },
                // $DIMJOGANG is in radians; the variable (like the style) holds degrees.
                HVal::Real(r) if name == "DIMJOGANG" => HVal::Real((r.to_degrees() * 1e9).round() / 1e9),
                // $ANGBASE is in degrees (a drawing measured from north has 90.0); the variable
                // holds radians.
                HVal::Real(r) if name == "ANGBASE" => HVal::Real(r.to_radians()),
                hv => hv,
            };
            d.header.set(&name, hv);
        } else {
            i += 1;
        }
    }
}

/// True when a style record carries `AcadAnnotative` xdata with the annotative flag set.
fn annotative(tags: &[Tag]) -> bool {
    let list = crate::dxf_ext::xdata_list(crate::dxf_ext::xdata(tags, "AcadAnnotative"), "AnnotativeData");
    list.iter().filter(|t| t.code == 1070).nth(1).is_some_and(|t| t.i64() != 0)
}

/// A text style's font: CADCraft's own font name from `CADCRAFT` xdata (`1000 FONT`, name)
/// while the font file is still the `txt` written for other readers; the TrueType family of the
/// style's `ACAD` xdata (`1000` typeface) when it has no font file (how AutoCAD stores TrueType
/// styles); otherwise the file.
fn style_font(tags: &[Tag], file: String) -> String {
    if file.trim().is_empty()
        && let Some(face) =
            crate::dxf_ext::xdata(tags, "ACAD").iter().find(|t| t.code == 1000).map(|t| t.str().trim().to_string()).filter(|f| !f.is_empty())
    {
        return face;
    }
    let x = crate::dxf_ext::xdata(tags, crate::dxf_ext::APP);
    let own = x.windows(2).find_map(|w| match w {
        [m, n] if m.code == 1000 && m.str() == "FONT" && n.code == 1000 => Some(n.str()),
        _ => None,
    });
    let stem = file.trim().to_ascii_lowercase();
    match own {
        Some(name) if !name.trim().is_empty() && (stem == "txt" || stem == "txt.shx") => name,
        _ => file,
    }
}

/// Load symbol tables and resolve handle references, indexing layer names for case-insensitive updates.
pub(super) fn tables(tags: &[Tag], d: &mut Drawing, rx: &mut Rx) {
    let recs = records(tags);
    // STYLE handle → name, or the shape file of an unnamed shape style (linetype elements).
    let ltype_styles: HashMap<String, String> = recs
        .iter()
        .filter(|(k, _)| k == "STYLE")
        .filter_map(|(_, tg)| {
            let t = T(tg);
            let name = t.s(2).filter(|n| !n.trim().is_empty());
            Some((t.s(5)?.trim().to_ascii_uppercase(), name.or_else(|| t.s(3))?))
        })
        .collect();
    // Handle maps first: DIMSTYLE records refer to text styles and arrow blocks by handle.
    index_table_handles(&recs, rx);
    let mut active_vport = false;
    // Index of layers already appended, by lower-cased name, so each LAYER record updates or
    // appends in O(1) instead of scanning the growing layer list — otherwise many distinct layer
    // names make import quadratic (CWE-407).
    let mut layer_index: HashMap<String, usize> = d.layers.iter().enumerate().map(|(i, l)| (l.name.to_ascii_lowercase(), i)).collect();
    for (kind, tg) in recs {
        let t = T(&tg);
        match kind.as_str() {
            "LAYER" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.layers.len() >= MAX_OBJECTS {
                    continue;
                }
                let aci = t.i(62).unwrap_or(7);
                let flags = t.i(70).unwrap_or(0);
                let mut l = Layer {
                    name: name.clone(),
                    color: Color::Index(aci.unsigned_abs().clamp(1, 255) as u8),
                    linetype: t.s(6).unwrap_or_else(|| "Continuous".into()),
                    lineweight: t.i(370).map(|v| Lineweight::from_dxf(v as i16)).unwrap_or(Lineweight::Default),
                    on: aci >= 0,
                    frozen: flags & 1 != 0,
                    locked: flags & 4 != 0,
                    vp_freeze_new: flags & 2 != 0,
                    plot: t.i(290).unwrap_or(1) != 0,
                    ..Layer::default()
                };
                if let Some(tc) = t.i(420) {
                    l.color = Color::True(Rgb::from_u32(tc as u32));
                }
                if let Some(n) = t.s(390).and_then(|h| rx.pstyles.get(&h.trim().to_ascii_uppercase())) {
                    l.plot_style = n.clone();
                }
                let alpha = crate::dxf_ext::xdata(&tg, crate::dxf_ext::LAYER_TRANSPARENCY_APP).iter().find(|x| x.code == 1071);
                if let Some(pct) = alpha.and_then(|x| crate::dxf_ext::transparency_from_dxf(x.i64())) {
                    l.transparency = pct;
                }
                if let Some(desc) = crate::dxf_ext::xdata(&tg, crate::dxf_ext::LAYER_DESCRIPTION_APP).iter().filter(|x| x.code == 1000).nth(1) {
                    l.description = desc.str();
                }
                match layer_index.get(&name.to_ascii_lowercase()) {
                    Some(&i) => {
                        if let Some(x) = d.layers.get_mut(i) {
                            *x = l;
                        }
                    }
                    None => {
                        layer_index.insert(name.to_ascii_lowercase(), d.layers.len());
                        d.layers.push(l);
                    }
                }
            }
            "LTYPE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.linetype(&name).is_some() || d.linetypes.len() >= MAX_OBJECTS {
                    continue;
                }
                let pattern = ltype_pattern(&tg, &ltype_styles);
                d.linetypes.push(Linetype { name, description: t.s(3).unwrap_or_default(), pattern });
            }
            "STYLE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.text_styles.len() >= MAX_OBJECTS {
                    continue;
                }
                // 70 bit 4: vertical; 71 bits 2/4: backwards / upside down.
                let gen_flags = t.i(71).unwrap_or(0);
                let st = TextStyle {
                    name: name.clone(),
                    font: style_font(&tg, t.s(3).unwrap_or_default()),
                    big_font: t.s(4).unwrap_or_default(),
                    height: t.fd(40, 0.0),
                    width_factor: t.fd(41, 1.0),
                    oblique: t.fd(50, 0.0).to_radians(),
                    backwards: gen_flags & 2 != 0,
                    upside_down: gen_flags & 4 != 0,
                    vertical: t.i(70).unwrap_or(0) & 4 != 0,
                    annotative: annotative(&tg),
                };
                match d.text_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = st,
                    None => d.text_styles.push(st),
                }
            }
            // The first *ACTIVE viewport holds the current snap/grid settings.
            "VPORT" if !active_vport && t.s(2).is_some_and(|n| n.trim().eq_ignore_ascii_case("*ACTIVE")) => {
                active_vport = true;
                vport_record(d, &t);
                for (name, code) in [("SNAPUNIT", 14), ("GRIDUNIT", 15)] {
                    if let (Some(x), Some(y)) = (t.f(code), t.f(code + 10)) {
                        let p = Vec2::new(x, y);
                        if p.is_finite() && if name == "GRIDUNIT" { p.x >= 0.0 && p.y >= 0.0 } else { p.x > 0.0 && p.y > 0.0 } {
                            d.header.set(name, HVal::Point(p.to3(0.0)));
                        }
                    }
                }
                if let (Some(x), Some(y)) = (t.f(13), t.f(23))
                    && x.is_finite()
                    && y.is_finite()
                {
                    d.header.set("SNAPBASE", HVal::Point(Vec3::new(x, y, 0.0)));
                }
                if let Some(a) = t.f(50).filter(|a| a.is_finite()) {
                    d.header.set_f64("SNAPANG", a);
                }
            }
            "VIEW" => {
                let name = t.s(2).unwrap_or_default();
                let (h, w) = (t.fd(40, 0.0), t.fd(41, 0.0));
                if name.is_empty() || !(h.is_finite() && w.is_finite() && h > 0.0 && w > 0.0) || d.views.len() >= MAX_OBJECTS {
                    continue;
                }
                let center = t.p(10).xy();
                if !(center.x.is_finite() && center.y.is_finite()) {
                    continue;
                }
                let x = crate::dxf_ext::xdata(&tg, crate::dxf_ext::APP);
                let field = |key: &str| {
                    x.iter().position(|t| t.code == 1000 && t.str() == key).and_then(|i| x.get(i + 1)).filter(|t| t.code == 1000).map(Tag::str)
                };
                // 70 bit 1: a paper space view; its layout is CADCraft xdata.
                let layout = (t.i(70).unwrap_or(0) & 1 != 0).then(|| field("LAYOUT").unwrap_or_default());
                let twist = t.f(50).filter(|a| a.is_finite()).map_or(0.0, |a| a.rem_euclid(360.0).to_radians());
                let v = NamedView { name: name.clone(), center, height: h, width: w, layer_state: field("LAYERSTATE"), twist, layout };
                match d.views.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = v,
                    None => d.views.push(v),
                }
            }
            "UCS" => {
                let name = t.s(2).unwrap_or_default();
                let (origin, x_axis, y_axis) = (t.p(10), t.p(11), t.p(12));
                let finite = [origin, x_axis, y_axis].iter().all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
                if name.is_empty() || !finite || d.ucss.len() >= MAX_OBJECTS {
                    continue;
                }
                let u = Ucs { name: name.clone(), origin, x_axis, y_axis };
                match d.ucss.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = u,
                    None => d.ucss.push(u),
                }
            }
            "DIMSTYLE" => {
                let name = t.s(2).unwrap_or_default();
                if name.is_empty() || d.dim_styles.len() >= MAX_OBJECTS {
                    continue;
                }
                // Every variable with a DIMSTYLE group code; values the style can't take are
                // left at their defaults.
                let mut fields = serde_json::Map::new();
                // The first 70 is the record's flags; a later one is DIMTFILLCLR.
                let flags = tg.iter().position(|x| x.code == 70);
                for (i, tag) in tg.iter().enumerate().take_while(|(_, x)| x.code < 1000) {
                    if Some(i) == flags {
                        continue;
                    }
                    let Some((field, kind)) = crate::dxf_ext::dim_field(tag.code) else { continue };
                    if let Some(v) = crate::dxf_ext::decode(kind, tag, &rx.styles, &rx.brs, &rx.ltypes) {
                        fields.insert(field.to_string(), v);
                    }
                }
                fields.extend(crate::dxf_ext::read_newer_dimvars(&tg, &rx.styles, &rx.brs, &rx.ltypes));
                let mut st = DimStyle { name: name.clone(), annotative: annotative(&tg), ..DimStyle::default() };
                st.apply_fields(&fields);
                match d.dim_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&name)) {
                    Some(x) => *x = st,
                    None => d.dim_styles.push(st),
                }
            }
            "VPORT" => vport_record(d, &t),
            _ => {}
        }
    }
    // One *Active viewport is no tiling: the view is the editor's (it zooms to the extents).
    d.vports.retain(|c| c.tiles.len() > usize::from(c.is_active()));
}

fn index_table_handles(recs: &[(String, Vec<Tag>)], rx: &mut Rx) {
    for (kind, tags) in recs {
        let table = T(tags);
        let (Some(handle), Some(name)) = (table.s(5), table.s(2)) else { continue };
        match kind.as_str() {
            "STYLE" => {
                rx.styles.insert(handle.trim().to_ascii_uppercase(), name);
            }
            "BLOCK_RECORD" => {
                let units = table.i(70).and_then(|units| u8::try_from(units).ok()).unwrap_or(0);
                rx.block_props.insert(name.to_ascii_uppercase(), (units, table.i(280).unwrap_or(1) != 0));
                if let Some(array) = crate::dxf_ext::read_array(tags) {
                    rx.block_arrays.insert(name.to_ascii_uppercase(), array);
                }
                rx.brs.insert(handle.trim().to_ascii_uppercase(), name);
            }
            "LTYPE" => {
                rx.ltypes.insert(handle.trim().to_ascii_uppercase(), name);
            }
            "LAYER" => {
                rx.layers.insert(handle.trim().to_ascii_uppercase(), name);
            }
            _ => {}
        }
    }
}

/// Most tiles a viewport configuration keeps.
const MAX_VPORT_TILES: usize = 64;

/// A VPORT record: one tile of the configuration it names (`*ACTIVE` the one shown). Corners
/// are fractions of the drawing area; unusable tiles are skipped.
fn vport_record(d: &mut Drawing, t: &T) {
    let name = t.s(2).map(|n| n.trim().to_string()).unwrap_or_default();
    let (min, max, center) = (t.p(10).xy(), t.p(11).xy(), t.p(12).xy());
    let height = t.fd(40, 0.0);
    let unit = |v: f64| v.is_finite() && (-1e-9..=1.0 + 1e-9).contains(&v);
    let ok = [min.x, min.y, max.x, max.y].into_iter().all(unit)
        && max.x > min.x + 1e-6
        && max.y > min.y + 1e-6
        && center.is_finite()
        && height.is_finite()
        && height > 0.0;
    if name.is_empty() || !ok {
        return;
    }
    let aspect = t.f(41).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(0.0);
    let clamp = |p: Vec2| Vec2::new(p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0));
    let tile = VportTile { min: clamp(min), max: clamp(max), center, height, aspect };
    let name = if name.eq_ignore_ascii_case(VportConfig::ACTIVE) { VportConfig::ACTIVE.to_string() } else { name };
    let room = d.vports.len() < MAX_OBJECTS;
    match d.vports.iter_mut().find(|c| c.name.eq_ignore_ascii_case(&name)) {
        Some(c) if c.tiles.len() < MAX_VPORT_TILES => c.tiles.push(tile),
        Some(_) => {}
        None if room => d.vports.push(VportConfig { name, tiles: vec![tile] }),
        None => {}
    }
}

/// The dash elements of an LTYPE record with their embedded text and shapes (DXF Reference,
/// LTYPE): each `49` length starts an element; `74` type flags (1 absolute rotation, 2 text,
/// 4 shape), `75` shape number, `340` STYLE, `46` scale, `50` rotation (radians), `44`/`45`
/// offset and `9` text follow it.
fn ltype_pattern(tags: &[Tag], styles: &HashMap<String, String>) -> Vec<DashElement> {
    let mut out: Vec<DashElement> = Vec::new();
    for t in tags.iter().take_while(|t| t.code < 1000) {
        if t.code == 49 {
            out.push(DashElement::dash(t.f64()));
            continue;
        }
        let Some(el) = out.last_mut() else { continue };
        let complex = el.text.is_some() || el.shape.is_some();
        let v = t.f64();
        match t.code {
            74 => {
                let flags = t.i64();
                el.absolute = flags & 1 != 0;
                if flags & 2 != 0 {
                    el.text = Some(String::new());
                } else if flags & 4 != 0 {
                    el.shape = Some(0);
                }
            }
            75 if el.shape.is_some() => el.shape = Some(u16::try_from(t.i64()).unwrap_or(0)),
            340 if complex => el.style = styles.get(&t.str().trim().to_ascii_uppercase()).filter(|n| !n.trim().is_empty()).cloned(),
            9 if el.text.is_some() => el.text = Some(t.str()),
            46 if complex && v.is_finite() => el.scale = v,
            50 if complex && v.is_finite() => el.rotation = v,
            44 if complex && v.is_finite() => el.offset.x = v,
            45 if complex && v.is_finite() => el.offset.y = v,
            _ => {}
        }
    }
    out
}
