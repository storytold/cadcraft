//! Annotative scaling.
//!
//! An annotation scale ("1:50") says how many drawing units one paper unit stands for. The
//! drawing keeps a list of them (SCALELISTEDIT) and a current one (`CANNOSCALE`). An annotative
//! object (text, MText, dimension, multileader, hatch, block reference) stores its size as it
//! should measure on paper and the set of scales it supports ([`Common::anno_scales`]); in a view
//! it draws at paper size × that view's annotation scale, and only in views whose scale it
//! supports unless `ANNOALLVISIBLE` is 1. Model space is seen at `CANNOSCALE`, a layout viewport
//! at its viewport scale, and paper space itself at 1:1.
//!
//! An object supporting several scales can sit somewhere else at one of them ([`AnnoMove`]): its
//! grips move it at the current scale only, and ANNORESET puts every scale back where the current
//! one is. What moves is the object's position ([`shift_position`]).

use serde::{Deserialize, Serialize};

use cadcraft_geom::{Mat3, Vec2};

use crate::{Common, Drawing, Entity, EntityKind};

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

/// How far an annotative object sits from its shared position at the scale named `scale`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnnoMove {
    pub scale: String,
    pub by: Vec2,
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
        e.common.anno_moves.clear();
    } else if can_be_annotative(&e.kind) && e.common.anno_scales.is_empty() {
        e.common.anno_scales.push(current.clone());
    }
}

/// How an object draws in a view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnnoDraw {
    /// Not annotative: as stored.
    Plain,
    /// Annotative, at `factor` (drawing units per paper unit), its position moved `by` at that
    /// scale.
    Scaled { factor: f64, by: Vec2 },
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
    /// How far the object sits from its shared position at the scale named `name`.
    pub fn moved_at(&self, name: &str) -> Vec2 {
        self.anno_moves.iter().find(|m| m.scale.eq_ignore_ascii_case(name)).map(|m| m.by).filter(|v| v.is_finite()).unwrap_or(Vec2::ZERO)
    }
    /// How far the object sits from its shared position at its scale with the ratio `factor`.
    pub fn moved_at_factor(&self, factor: f64) -> Vec2 {
        self.anno_scales.iter().find(|s| s.has_factor(factor)).map_or(Vec2::ZERO, |s| self.moved_at(&s.name))
    }
    /// Move the object `by` at the scale named `name` only (an offset that comes to nothing is
    /// dropped).
    pub fn move_at(&mut self, name: &str, by: Vec2) {
        if !by.is_finite() {
            return;
        }
        let to = self.moved_at(name) + by;
        self.anno_moves.retain(|m| !m.scale.eq_ignore_ascii_case(name));
        if to.len() > 1e-12 * (1.0 + by.len()) && self.anno_moves.len() < MAX_SCALES {
            self.anno_moves.push(AnnoMove { scale: name.to_string(), by: to });
        }
    }
    /// Turn the offsets with `m`'s linear part (MOVE leaves them, ROTATE turns them, SCALE sizes
    /// them, MIRROR mirrors them), as the shared position goes through `m`.
    pub fn transform_moves(&mut self, m: &Mat3) {
        for mv in &mut self.anno_moves {
            let v = m.apply_vec(mv.by);
            mv.by = if v.is_finite() { v } else { Vec2::ZERO };
        }
    }
}

/// Move the position of `k` by `by`: a text's, MText's or block reference's whole object, a
/// multileader's landing and content (its arrowheads stay), a dimension's text. Other kinds stay.
pub fn shift_position(k: &mut EntityKind, by: Vec2) {
    if !by.is_finite() || by == Vec2::ZERO {
        return;
    }
    let add = |p: &mut cadcraft_geom::Vec3| {
        p.x += by.x;
        p.y += by.y;
    };
    match k {
        EntityKind::Text(_) | EntityKind::MText(_) | EntityKind::Insert(_) => k.transform(&Mat3::translate(by)),
        EntityKind::MLeader(m) => {
            add(&mut m.landing);
            if let Some(t) = &mut m.text {
                add(&mut t.insert);
            }
            if let Some(b) = &mut m.block {
                add(&mut b.insert);
            }
        }
        EntityKind::Dimension(d) => {
            add(&mut d.text_mid);
            d.user_text_pos = true;
            d.block = None;
        }
        _ => {}
    }
}

/// Whether grip `index` of `k` is its position grip: moving it moves what [`shift_position`]
/// moves.
pub fn is_position_grip(k: &EntityKind, index: usize) -> bool {
    match k {
        EntityKind::Text(_) | EntityKind::MText(_) => true,
        EntityKind::Insert(_) => index == 0,
        EntityKind::Dimension(_) => index == 3,
        EntityKind::MLeader(m) => index == m.leaders.iter().map(Vec::len).sum::<usize>(),
        _ => false,
    }
}

/// How `e` draws in a view whose annotation scale is `view` (drawing units per paper unit). An
/// object that doesn't support the view's scale shows at its first scale when `all_visible`.
pub fn anno_draw(e: &Entity, view: f64, all_visible: bool) -> AnnoDraw {
    if !is_annotative(e) {
        return AnnoDraw::Plain;
    }
    if e.common.supports_factor(view) {
        let factor = if view.is_finite() && view > 0.0 { view } else { 1.0 };
        return AnnoDraw::Scaled { factor, by: e.common.moved_at_factor(view) };
    }
    match e.common.anno_scales.first() {
        Some(s) if all_visible => AnnoDraw::Scaled { factor: s.factor(), by: e.common.moved_at(&s.name) },
        _ => AnnoDraw::Hidden,
    }
}

/// `e` at each of its scales other than the one it shows at in a view of scale `view` (see
/// [`anno_draw`]): at that scale's size and position, no longer annotative (so it draws as it
/// is). AutoCAD shows these faded while the object is selected (SELECTIONANNODISPLAY). Empty when
/// `e` isn't annotative, supports one scale or doesn't show in that view; scales of the same
/// ratio come once.
pub fn anno_others(e: &Entity, view: f64, all_visible: bool) -> Vec<Entity> {
    let AnnoDraw::Scaled { factor, .. } = anno_draw(e, view, all_visible) else {
        return Vec::new();
    };
    let mut seen = vec![factor];
    let mut out = Vec::new();
    for s in e.common.anno_scales.iter().take(MAX_SCALES).filter(|s| s.is_valid()) {
        let f = s.factor();
        if seen.iter().any(|g| same_factor(*g, f)) {
            continue;
        }
        seen.push(f);
        let mut other = anno_placed(e, f, e.common.moved_at(&s.name));
        other.common.anno_scales.clear();
        other.common.anno_moves.clear();
        out.push(other);
    }
    out
}

/// `e` as it draws at annotation scale `f` with its position moved `by` there: [`anno_scaled`],
/// then [`shift_position`].
pub fn anno_placed(e: &Entity, f: f64, by: Vec2) -> Entity {
    let mut out = anno_scaled(e, f);
    shift_position(&mut out.kind, by);
    out
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
    /// Whether annotative objects that don't support the view's scale show (`ANNOALLVISIBLE`).
    pub fn anno_all_visible(&self) -> bool {
        self.header.i64("ANNOALLVISIBLE", 1) != 0
    }
    /// How top-level object `e` draws outside a viewport: in a layout's paper space at 1:1
    /// (paper space shows every annotative object), elsewhere (model space, or an object not in
    /// the drawing yet) as [`anno_draw`] at the current annotation scale.
    pub fn anno_draw(&self, e: &Entity) -> AnnoDraw {
        self.anno_draw_in(e, None)
    }
    /// How top-level object `e` draws where it is edited: through an active viewport whose
    /// annotation scale is `view` (drawing units per paper unit, the viewport's scale, as the
    /// renderer draws model space there), else as [`Drawing::anno_draw`].
    pub fn anno_draw_in(&self, e: &Entity, view: Option<f64>) -> AnnoDraw {
        if !is_annotative(e) {
            return AnnoDraw::Plain;
        }
        if !self.model.contains(e.handle) && self.layouts.iter().any(|l| l.entities.contains(e.handle)) {
            return AnnoDraw::Scaled { factor: 1.0, by: e.common.moved_at_factor(1.0) };
        }
        let view = view.filter(|f| f.is_finite() && *f > 0.0).unwrap_or_else(|| self.current_anno_scale().factor());
        anno_draw(e, view, self.anno_all_visible())
    }
    /// The scale `e` shows at outside a viewport (see [`Drawing::anno_draw`]): its name and
    /// factor, `None` when `e` isn't annotative or doesn't show.
    pub fn anno_shown_scale(&self, e: &Entity) -> Option<AnnoScale> {
        self.anno_shown_scale_in(e, None)
    }
    /// The scale `e` shows at in view `view` (see [`Drawing::anno_draw_in`]).
    pub fn anno_shown_scale_in(&self, e: &Entity, view: Option<f64>) -> Option<AnnoScale> {
        match self.anno_draw_in(e, view) {
            AnnoDraw::Scaled { factor, .. } => e.common.anno_scales.iter().find(|s| s.has_factor(factor)).cloned(),
            _ => None,
        }
    }
    /// `e` at its other scales outside a viewport (see [`anno_others`]); none in a layout's paper
    /// space, which shows annotative objects at 1:1 only.
    pub fn anno_others(&self, e: &Entity) -> Vec<Entity> {
        if !is_annotative(e) || (!self.model.contains(e.handle) && self.layouts.iter().any(|l| l.entities.contains(e.handle))) {
            return Vec::new();
        }
        anno_others(e, self.current_anno_scale().factor(), self.anno_all_visible())
    }
    /// `e` as it draws outside a viewport: an annotative object at its scale and position there,
    /// anything else (and a hidden annotative object) as stored.
    pub fn anno_shown(&self, e: &Entity) -> EntityKind {
        self.anno_shown_in(e, None)
    }
    /// `e` as it draws in view `view` (see [`Drawing::anno_draw_in`]).
    pub fn anno_shown_in(&self, e: &Entity, view: Option<f64>) -> EntityKind {
        match self.anno_draw_in(e, view) {
            AnnoDraw::Scaled { factor, by } => anno_placed(e, factor, by).kind,
            _ => e.kind.clone(),
        }
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
