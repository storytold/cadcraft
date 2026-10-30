use super::common::T;
use super::dimassoc::dimassoc;
use super::document::sortents_table;
use super::{Drawing, EntityKind, Group, Handle, HashMap, MLeaderStyle, PageSetup, Rx, Tag, Vec2};

/// Non-graphical objects applied once every entity is placed.
#[derive(Default)]
pub(super) struct Objects {
    /// Dictionary entry handle (upper case) → entry name.
    names: HashMap<String, String>,
    /// SCALE objects (the annotation scale list), in file order.
    scales: Vec<cadcraft_doc::AnnoScale>,
    /// XRECORD handle (upper case) → its groups.
    xrecords: HashMap<String, Vec<Tag>>,
    /// TABLESTYLE objects: (handle, groups).
    table_styles: Vec<(String, Vec<Tag>)>,
    /// DIMASSOC objects.
    dimassocs: Vec<Vec<Tag>>,
    /// MLEADERSTYLE objects: (handle, groups).
    mleader_styles: Vec<(String, Vec<Tag>)>,
    /// GROUP objects: (handle, groups).
    groups: Vec<(String, Vec<Tag>)>,
    /// Saved paper-space views of layouts: (layout name, view).
    layout_views: Vec<(String, (Vec2, f64))>,
    /// SORTENTSTABLE objects: (owning block record handle, entity handle → sort handle).
    pub(super) sortents: Vec<(String, HashMap<Handle, u64>)>,
    /// WIPEOUTVARIABLES: the drawing's WIPEOUTFRAME (0 = frames hidden, 1 = shown).
    wipeout_frame: Option<i64>,
}

pub(super) const MAX_OBJECTS: usize = 1_000_000;

impl Objects {
    pub(super) fn add(&mut self, kind: &str, tags: &[Tag]) {
        let h = T(tags).s(5).map(|h| h.trim().to_ascii_uppercase()).unwrap_or_default();
        match kind {
            "SCALE" if self.scales.len() < cadcraft_doc::MAX_SCALES => {
                if let Some(s) = crate::dxf_ext::read_scale(tags)
                    && !self.scales.iter().any(|x| x.name.eq_ignore_ascii_case(&s.name))
                {
                    self.scales.push(s);
                }
            }
            "DICTIONARY" => {
                let mut name: Option<String> = None;
                for t in tags {
                    match t.code {
                        3 => name = Some(t.str()),
                        350 | 360 => {
                            if let Some(n) = name.take()
                                && self.names.len() < MAX_OBJECTS
                            {
                                self.names.insert(t.str().trim().to_ascii_uppercase(), n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "XRECORD" if self.xrecords.len() < MAX_OBJECTS => {
                self.xrecords.insert(h, tags.to_vec());
            }
            "TABLESTYLE" if self.table_styles.len() < MAX_OBJECTS => self.table_styles.push((h, tags.to_vec())),
            "DIMASSOC" if self.dimassocs.len() < MAX_OBJECTS => self.dimassocs.push(tags.to_vec()),
            "MLEADERSTYLE" if self.mleader_styles.len() < MAX_OBJECTS => self.mleader_styles.push((h, tags.to_vec())),
            "GROUP" if self.groups.len() < MAX_OBJECTS => self.groups.push((h, tags.to_vec())),
            "WIPEOUTVARIABLES" => self.wipeout_frame = T(tags).i(70).filter(|v| (0..=2).contains(v)),
            "SORTENTSTABLE" if self.sortents.len() < MAX_OBJECTS => {
                if let Some(t) = sortents_table(tags) {
                    self.sortents.push(t);
                }
            }
            "LAYOUT" if self.layout_views.len() < MAX_OBJECTS => {
                let start = tags.iter().position(|x| x.code == 100 && x.str() == "AcDbLayout").unwrap_or(0);
                if let (Some(name), Some(view)) = (T(tags.get(start..).unwrap_or(&[])).s(1), layout_view(tags)) {
                    self.layout_views.push((name, view));
                }
            }
            _ => {}
        }
    }

    /// The text of the CADCraft XRECORD stored under `key` in a dictionary (its 1/3 strings).
    pub(super) fn xrecord_text(&self, key: &str) -> Option<String> {
        let h = self.names.iter().find(|(_, n)| n.as_str() == key).map(|(h, _)| h)?;
        let tags = self.xrecords.get(h)?;
        let mut text = String::new();
        for t in tags.iter().filter(|t| t.code == 1 || t.code == 3) {
            if text.len() > crate::dxf_ext::MAX_PAYLOAD {
                break;
            }
            text.push_str(&t.str());
        }
        Some(text)
    }

    fn apply_table_styles(&self, d: &mut Drawing) {
        // Table styles (named by their ACAD_TABLESTYLE dictionary entries).
        for (h, tags) in &self.table_styles {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let st = crate::dxf_table::read_table_style(name, tags);
            match d.table_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = st,
                None => d.table_styles.push(st),
            }
        }
    }

    fn apply_table_style_fixes(&self, d: &mut Drawing, rx: &Rx) {
        for (h, sh) in &rx.table_style_fix {
            let name = self.names.get(sh).cloned();
            // A table from another writer has no scale of its own: its text height is its
            // style's, so every cell style height applies as it is.
            let height = {
                let style = name.as_deref().unwrap_or("Standard");
                d.table_styles
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(style))
                    .map_or(cadcraft_doc::TableStyle::default().text_height, |s| s.text_height)
            };
            let _ = d.modify_entity(*h, |e| {
                if let EntityKind::Table(t) = &mut e.kind {
                    if let Some(name) = name {
                        t.style = name;
                    }
                    t.text_height = height;
                }
            });
        }
    }

    fn apply_mleader_styles(&self, d: &mut Drawing, rx: &Rx) {
        // Multileader styles (named by their ACAD_MLEADERSTYLE dictionary entries).
        for (h, tags) in &self.mleader_styles {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let t = T(tags);
            let def = MLeaderStyle::default();
            let num = |code: i32, d: f64, min: f64| t.f(code).filter(|v| v.is_finite() && *v >= min).unwrap_or(d);
            let text_style = t.s(342).and_then(|sh| rx.styles.get(&sh.trim().to_ascii_uppercase()).cloned()).filter(|n| !n.is_empty());
            let st = MLeaderStyle {
                name: name.clone(),
                arrow_size: num(44, def.arrow_size, 0.0),
                text_height: t.f(45).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(def.text_height),
                landing_gap: num(42, def.landing_gap, 0.0),
                dogleg: num(43, def.dogleg, 0.0),
                text_style: text_style.unwrap_or(def.text_style),
                annotative: t.i(296).unwrap_or(0) != 0,
            };
            match d.mleader_styles.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = st,
                None => d.mleader_styles.push(st),
            }
        }
    }

    fn apply_groups(&self, d: &mut Drawing) {
        // Groups (named by their ACAD_GROUP dictionary entries); members that exist.
        for (h, tags) in &self.groups {
            let Some(name) = self.names.get(h).filter(|n| !n.is_empty()) else { continue };
            let t = T(tags);
            let members: Vec<Handle> = tags
                .iter()
                .filter(|x| x.code == 340)
                .filter_map(|x| Handle::parse_hex(&x.str()))
                .filter(|m| d.entity(*m).is_some())
                .take(MAX_OBJECTS)
                .collect();
            let g = Group { name: name.clone(), description: t.s(300).unwrap_or_default(), selectable: t.i(71).unwrap_or(1) != 0, members };
            match d.groups.iter_mut().find(|x| x.name.eq_ignore_ascii_case(name)) {
                Some(x) => *x = g,
                None => d.groups.push(g),
            }
        }
    }

    pub(super) fn apply(self, d: &mut Drawing, rx: &Rx) {
        if let Some(v) = self.wipeout_frame {
            d.header.set_i64("WIPEOUTFRAME", v);
        }
        if let Some(style) = self.xrecord_text(crate::dxf_ext::MLEADER_STYLE_KEY).filter(|s| !s.trim().is_empty()) {
            d.header.set_str("CMLEADERSTYLE", &style);
        }
        self.apply_table_styles(d);
        self.apply_table_style_fixes(d, rx);
        // Parametric constraints and parameters.
        if let Some((c, p)) = self.xrecord_text(crate::dxf_ext::CONSTRAINTS_KEY).and_then(|t| crate::dxf_ext::parse_constraints(&t)) {
            d.constraints = c;
            d.parametric = p;
        }
        // The annotation scale list (a metric drawing without one gets the metric defaults),
        // then CADCraft's annotative scaling record.
        if !self.scales.is_empty() {
            d.scales = self.scales.clone();
        } else if d.header.i64("MEASUREMENT", 0) == 1 {
            d.scales = cadcraft_doc::metric_scales();
        }
        if let Some(p) = self.xrecord_text(crate::dxf_ext::ANNOTATION_KEY).and_then(|t| crate::dxf_ext::parse_anno(&t)) {
            crate::dxf_ext::apply_anno(d, p);
        }
        // Saved layer states.
        if let Some(states) = self.xrecord_text(crate::dxf_ext::LAYER_STATES_KEY).and_then(|t| crate::dxf_ext::parse_layer_states(&t)) {
            d.layer_states = states;
        }
        // The drawing's plot style tables.
        if let Some(tables) = self.xrecord_text(crate::dxf_ext::PLOT_STYLES_KEY).and_then(|t| crate::dxf_ext::parse_plot_styles(&t)) {
            d.plot_style_tables = tables;
        }
        self.apply_mleader_styles(d, rx);
        crate::dxf_mleader::apply(d, &rx.mleader_fix, &self.names);
        for (name, view) in &self.layout_views {
            if let Some(l) = d.layouts.iter_mut().find(|l| l.name == *name) {
                l.view = Some(*view);
            }
        }
        self.apply_groups(d);
        // Standard associativity, for dimensions without CADCraft's exact links.
        for tags in &self.dimassocs {
            dimassoc(d, tags);
        }
    }
}

/// Page setup from a LAYOUT object's AcDbPlotSettings fields (sizes and margins in mm).
pub(super) fn page_setup(t: &T) -> PageSetup {
    let mut p = PageSetup::default();
    let mm = |code: i32, d: f64| {
        let v = t.fd(code, d);
        if v.is_finite() && (0.0..=100_000.0).contains(&v) { v } else { d }
    };
    let (w, h) = (mm(44, 0.0), mm(45, 0.0));
    if w > 0.0 && h > 0.0 {
        p.width_mm = w.min(h);
        p.height_mm = w.max(h);
    }
    p.margins_mm = [mm(40, p.margins_mm[0]), mm(41, p.margins_mm[1]), mm(42, p.margins_mm[2]), mm(43, p.margins_mm[3])];
    // Plot paper units 1: millimetres. 0 (inches) is left to MEASUREMENT: earlier CADCraft
    // versions wrote 0 for every layout.
    p.paper_mm = t.i(72) == Some(1);
    // Plot rotation 1 or 3 turns the media a quarter turn; media defined wider than tall is
    // already landscape.
    let rotated = matches!(t.i(73), Some(1 | 3));
    if w > 0.0 && h > 0.0 {
        p.landscape = rotated != (w > h);
    } else if t.i(73).is_some() {
        p.landscape = rotated;
    }
    if let Some(name) = t.s(4).filter(|n| !n.is_empty()) {
        p.paper = name.replace('_', " ");
    }
    if let Some(dev) = t.s(2).map(|s| s.trim().to_string()) {
        p.device = if dev.is_empty() || dev.eq_ignore_ascii_case("none_device") { "None".into() } else { dev };
    }
    // Plot layout flags: 2 display plot styles, 4 centred, 16 standard scale (75 = 0: scaled
    // to fit), 128 lineweights.
    if let Some(flags) = t.i(70) {
        p.show_plot_styles = flags & 2 != 0;
        p.center = flags & 4 != 0;
        p.lineweights = flags & 128 != 0;
        p.scale_to_fit = flags & 16 != 0 && t.i(75) == Some(0);
    }
    // Custom scale: 142 paper units per 143 drawing units; 147 is the scale factor.
    let pos = |v: Option<f64>| v.filter(|v| v.is_finite() && *v > 0.0);
    let ratio = pos(t.f(142)).zip(pos(t.f(143))).map(|(n, d)| n / d);
    if let Some(s) = pos(ratio).or_else(|| pos(t.f(147))) {
        p.scale = s;
    }
    p.plot_area = match t.i(74) {
        Some(0) => "display",
        Some(1) => "extents",
        Some(2) => "limits",
        Some(4) => "window",
        _ => "layout",
    }
    .into();
    p.plot_window = [t.fd(48, 0.0), t.fd(49, 0.0), t.fd(140, 0.0), t.fd(141, 0.0)].map(|v| if v.is_finite() { v } else { 0.0 });
    p.plot_style_table = t.s(7).unwrap_or_default();
    p
}

/// A layout's saved paper-space view (centre, height) from CADCraft xdata.
fn layout_view(tags: &[Tag]) -> Option<(Vec2, f64)> {
    let list = crate::dxf_ext::xdata_list(crate::dxf_ext::xdata(tags, crate::dxf_ext::APP), "PSVIEW");
    let g = |c: i32| list.iter().find(|t| t.code == c).map(Tag::f64).filter(|v| v.is_finite());
    Some((Vec2::new(g(1010)?, g(1020)?), g(1040)?))
}
