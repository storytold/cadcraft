//! Annotative scaling.
//!
//! An annotation scale ("1:50") says how many drawing units one paper unit stands for. The
//! drawing keeps a list of them (SCALELISTEDIT) and a current one (`CANNOSCALE`). An annotative
//! object (text, MText, dimension, multileader, hatch, block reference) stores its size as it
//! should measure on paper and the set of scales it supports ([`Common::anno_scales`]); in a view
//! it draws at paper size × that view's annotation scale, and only in views whose scale it
//! supports unless `ANNOALLVISIBLE` is 1. Model space is seen at `CANNOSCALE`, a layout viewport
//! at its own annotation scale (by default the one matching its viewport scale), and paper space
//! itself at 1:1.

use serde::{Deserialize, Serialize};

use crate::{Common, Drawing, Entity, EntityKind, Viewport};

/// A named annotation scale: `paper_units` on paper show `drawing_units` of the drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnoScale {
    pub name: String,
    pub paper_units: f64,
    pub drawing_units: f64,
}

/// The most scales a drawing's list or one object holds (hostile files and parameters).
pub const MAX_SCALES: usize = 1000;

impl AnnoScale {
    pub fn new(name: &str, paper_units: f64, drawing_units: f64) -> Self {
        AnnoScale { name: name.to_string(), paper_units, drawing_units }
    }
    /// The 1:1 scale.
    pub fn unit() -> Self {
        AnnoScale::new("1:1", 1.0, 1.0)
    }
    /// Both units finite and positive (and their ratio usable).
    pub fn is_valid(&self) -> bool {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        ok(self.paper_units) && ok(self.drawing_units) && ok(self.drawing_units / self.paper_units)
    }
    /// Drawing units per paper unit (1:50 → 50): what an annotative object's paper size is
    /// multiplied by. 1 for an invalid scale.
    pub fn factor(&self) -> f64 {
        if self.is_valid() { self.drawing_units / self.paper_units } else { 1.0 }
    }
    /// Whether this scale has the ratio `factor` (drawing units per paper unit).
    pub fn has_factor(&self, factor: f64) -> bool {
        same_factor(self.factor(), factor)
    }
}

/// A scale named after its ratio (`1:37.5`, `2:1`) with `factor` drawing units per paper unit; 1:1
/// for an unusable factor.
pub fn ratio_scale(factor: f64) -> AnnoScale {
    if !(factor.is_finite() && factor > 0.0 && (1.0 / factor).is_finite()) {
        return AnnoScale::unit();
    }
    let num = |x: f64| {
        let r = (x * 1000.0).round() / 1000.0;
        if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r}") }
    };
    let name = if factor >= 1.0 { format!("1:{}", num(factor)) } else { format!("{}:1", num(1.0 / factor)) };
    AnnoScale::new(&name, 1.0, factor)
}

/// A layout viewport's scale as an annotation factor (model units per paper unit: a 1:50
/// viewport gives 50); `None` for a viewport with no usable size.
pub fn viewport_factor(vp: &Viewport) -> Option<f64> {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    let f = vp.view_height / vp.height;
    (ok(vp.height) && ok(vp.view_height) && ok(f)).then_some(f)
}

/// Two scale factors are the same ratio (relative tolerance).
pub fn same_factor(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1e-12)
}

/// The default metric scale list.
pub fn metric_scales() -> Vec<AnnoScale> {
    let mut v: Vec<AnnoScale> =
        [1.0, 2.0, 4.0, 5.0, 8.0, 10.0, 16.0, 20.0, 30.0, 40.0, 50.0, 100.0].iter().map(|n| AnnoScale::new(&format!("1:{n}"), 1.0, *n)).collect();
    v.extend([2.0, 4.0, 8.0, 10.0, 100.0].iter().map(|n| AnnoScale::new(&format!("{n}:1"), *n, 1.0)));
    v
}

/// The default imperial scale list: the metric ratios, then the architectural inch-to-foot
/// scales.
pub fn imperial_scales() -> Vec<AnnoScale> {
    let mut v = metric_scales();
    let arch: [(&str, f64); 15] = [
        ("1/128\" = 1'-0\"", 1.0 / 128.0),
        ("1/64\" = 1'-0\"", 1.0 / 64.0),
        ("1/32\" = 1'-0\"", 1.0 / 32.0),
        ("1/16\" = 1'-0\"", 1.0 / 16.0),
        ("3/32\" = 1'-0\"", 3.0 / 32.0),
        ("1/8\" = 1'-0\"", 1.0 / 8.0),
        ("3/16\" = 1'-0\"", 3.0 / 16.0),
        ("1/4\" = 1'-0\"", 0.25),
        ("3/8\" = 1'-0\"", 0.375),
        ("1/2\" = 1'-0\"", 0.5),
        ("3/4\" = 1'-0\"", 0.75),
        ("1\" = 1'-0\"", 1.0),
        ("1-1/2\" = 1'-0\"", 1.5),
        ("3\" = 1'-0\"", 3.0),
        ("6\" = 1'-0\"", 6.0),
    ];
    v.extend(arch.iter().map(|(n, p)| AnnoScale::new(n, *p, 12.0)));
    v.push(AnnoScale::new("1'-0\" = 1'-0\"", 12.0, 12.0));
    v
}

/// Kinds that can be annotative.
pub fn can_be_annotative(k: &EntityKind) -> bool {
    matches!(
        k,
        EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Dimension(_) | EntityKind::MLeader(_) | EntityKind::Hatch(_) | EntityKind::Insert(_)
    )
}

/// An annotative object: a kind that can be annotative, with at least one supported scale.
pub fn is_annotative(e: &Entity) -> bool {
    !e.common.anno_scales.is_empty() && can_be_annotative(&e.kind)
}

/// Make `e` annotative (supporting `current` when it supports no scale yet) or not. Kinds that
/// can't be annotative are left alone.
pub fn set_annotative(e: &mut Entity, on: bool, current: &AnnoScale) {
    if !on {
        e.common.anno_scales.clear();
    } else if can_be_annotative(&e.kind) && e.common.anno_scales.is_empty() {
        e.common.anno_scales.push(current.clone());
    }
}

/// How an object draws in a view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnnoDraw {
    /// Not annotative: as stored.
    Plain,
    /// Annotative, at this factor (drawing units per paper unit).
    Scaled(f64),
    /// Annotative and not supporting the view's scale, with `ANNOALLVISIBLE` 0.
    Hidden,
}

impl Common {
    /// Whether these properties list a scale with the ratio `factor`.
    pub fn supports_factor(&self, factor: f64) -> bool {
        self.anno_scales.iter().any(|s| s.has_factor(factor))
    }
    /// Whether these properties list the scale named `name`.
    pub fn supports_scale(&self, name: &str) -> bool {
        self.anno_scales.iter().any(|s| s.name.eq_ignore_ascii_case(name))
    }
}

/// How `e` draws in a view whose annotation scale is `view` (drawing units per paper unit). An
/// object that doesn't support the view's scale shows at its first scale when `all_visible`.
pub fn anno_draw(e: &Entity, view: f64, all_visible: bool) -> AnnoDraw {
    if !is_annotative(e) {
        return AnnoDraw::Plain;
    }
    if e.common.supports_factor(view) {
        return AnnoDraw::Scaled(if view.is_finite() && view > 0.0 { view } else { 1.0 });
    }
    match e.common.anno_scales.first() {
        Some(s) if all_visible => AnnoDraw::Scaled(s.factor()),
        _ => AnnoDraw::Hidden,
    }
}

/// `e` as it draws at annotation scale `f`: its paper sizes (text height and width, dimension
/// overall scale, multileader arrow, landing and content, hatch pattern scale, block scale and
/// attributes) multiplied by `f`. A dimension is drawn from its definition at that scale, not
/// from a stored geometry block. Other kinds come back unchanged.
pub fn anno_scaled(e: &Entity, f: f64) -> Entity {
    let f = if f.is_finite() && f > 0.0 { f } else { 1.0 };
    let mut out = e.clone();
    match &mut out.kind {
        EntityKind::Text(t) => t.height *= f,
        EntityKind::MText(t) => {
            t.height *= f;
            t.width *= f;
            if let Some(c) = &mut t.columns {
                c.scale(f);
            }
        }
        EntityKind::Dimension(dm) => {
            dm.overrides.insert("scale".into(), serde_json::json!(f));
            dm.block = None;
        }
        EntityKind::MLeader(m) => {
            let land = m.landing;
            m.arrow_size *= f;
            m.dogleg *= f;
            if let Some(t) = &mut m.text {
                t.insert = land + (t.insert - land) * f;
                t.height *= f;
                t.width *= f;
                if let Some(c) = &mut t.columns {
                    c.scale(f);
                }
            }
            if let Some(ins) = &mut m.block {
                ins.insert = land + (ins.insert - land) * f;
                ins.scale = ins.scale * f;
            }
        }
        EntityKind::Hatch(h) if !h.solid => h.scale *= f,
        EntityKind::Insert(ins) => {
            let o = ins.insert;
            ins.scale = ins.scale * f;
            ins.col_spacing *= f;
            ins.row_spacing *= f;
            for a in &mut ins.attribs {
                a.text.insert = o + (a.text.insert - o) * f;
                a.text.align_pt = a.text.align_pt.map(|p| o + (p - o) * f);
                a.text.height *= f;
                if let Some(ml) = &mut a.multiline {
                    ml.mtext.insert = o + (ml.mtext.insert - o) * f;
                    ml.mtext.height *= f;
                    ml.mtext.width *= f;
                }
            }
        }
        _ => {}
    }
    out
}

impl Drawing {
    /// The scale named `name` in the drawing's scale list (case-insensitive).
    pub fn anno_scale(&self, name: &str) -> Option<&AnnoScale> {
        let n = name.trim();
        self.scales.iter().find(|s| s.name.eq_ignore_ascii_case(n))
    }
    /// The current annotation scale (`CANNOSCALE`); 1:1 when it names no scale of the list.
    pub fn current_anno_scale(&self) -> AnnoScale {
        let name = self.header.str("CANNOSCALE", "1:1");
        self.anno_scale(&name).filter(|s| s.is_valid()).cloned().unwrap_or_else(AnnoScale::unit)
    }
    /// The scale of the list with the ratio `factor`, else one named after the ratio.
    pub fn scale_with_factor(&self, factor: f64) -> AnnoScale {
        self.scales.iter().find(|s| s.is_valid() && s.has_factor(factor)).cloned().unwrap_or_else(|| ratio_scale(factor))
    }
    /// The annotation scale layout viewport `vp` shows annotative objects at: its own
    /// ([`Viewport::anno_scale`]) when it has a valid one, else the one matching its viewport
    /// scale (a scale of the list with that ratio, or one named after the ratio).
    pub fn viewport_anno_scale(&self, vp: &Viewport) -> AnnoScale {
        if let Some(s) = vp.anno_scale.as_ref().filter(|s| s.is_valid()) {
            return s.clone();
        }
        viewport_factor(vp).map(|f| self.scale_with_factor(f)).unwrap_or_else(AnnoScale::unit)
    }
    /// Whether `vp`'s annotation scale differs from its viewport scale (AutoCAD's status bar then
    /// offers to synchronize them).
    pub fn viewport_scales_differ(&self, vp: &Viewport) -> bool {
        match (vp.anno_scale.as_ref().filter(|s| s.is_valid()), viewport_factor(vp)) {
            (Some(s), Some(f)) => !s.has_factor(f),
            _ => false,
        }
    }
    /// Whether annotative objects that don't support the view's scale show (`ANNOALLVISIBLE`).
    pub fn anno_all_visible(&self) -> bool {
        self.header.i64("ANNOALLVISIBLE", 1) != 0
    }
    /// How top-level object `e` draws outside a viewport: in a layout's paper space at 1:1
    /// (paper space shows every annotative object), elsewhere (model space, or an object not in
    /// the drawing yet) as [`anno_draw`] at the current annotation scale.
    pub fn anno_draw(&self, e: &Entity) -> AnnoDraw {
        if !is_annotative(e) {
            return AnnoDraw::Plain;
        }
        if !self.model.contains(e.handle) && self.layouts.iter().any(|l| l.entities.contains(e.handle)) {
            return AnnoDraw::Scaled(1.0);
        }
        anno_draw(e, self.current_anno_scale().factor(), self.anno_all_visible())
    }
    /// Whether new objects of this kind are annotative: text and MText with an annotative text
    /// style, dimensions with an annotative dimension style, multileaders with an annotative
    /// multileader style, hatches while `HPANNOTATIVE` is 1, references to annotative blocks.
    pub fn annotative_by_style(&self, k: &EntityKind) -> bool {
        match k {
            EntityKind::Text(t) => self.text_style(&t.style).is_some_and(|s| s.annotative),
            EntityKind::MText(t) => self.text_style(&t.style).is_some_and(|s| s.annotative),
            EntityKind::Dimension(dm) => self.dim_style(&dm.style).is_some_and(|s| s.annotative),
            EntityKind::MLeader(m) => self.mleader_styles.iter().find(|s| s.name.eq_ignore_ascii_case(&m.style)).is_some_and(|s| s.annotative),
            EntityKind::Hatch(_) => self.header.i64("HPANNOTATIVE", 0) != 0,
            EntityKind::Insert(ins) => self.block(&ins.block).is_some_and(|b| b.annotative),
            _ => false,
        }
    }
}
