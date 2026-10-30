use super::{Accept, DimAssoc, DimKind, Dimension, EntityKind, Handle, Input, Prompt, Result, Segment, Session, Value, Vec2, Vec3, v3};

pub(super) fn style(s: &Session) -> String {
    s.doc().map(|d| d.header.str("DIMSTYLE", "Standard")).unwrap_or_else(|_| "Standard".into())
}

pub(super) fn dim(s: &Session, kind: DimKind, defpt: Vec2, p13: Vec2, p14: Vec2, p15: Vec2, text: &str) -> EntityKind {
    EntityKind::Dimension(Dimension {
        kind,
        defpt: v3(defpt),
        text_mid: Vec3::ZERO,
        p13: v3(p13),
        p14: v3(p14),
        p15: v3(p15),
        p16: v3(Vec2::ZERO),
        text: text.into(),
        style: style(s),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: s.doc().map(|d| d.dim_overrides()).unwrap_or_default(),
        assoc: Vec::new(),
        marks: Default::default(),
    })
}

/// Horizontal (0) or vertical (90°) from where the dimension line is placed (AutoCAD's rule).
fn auto_rotation(p1: Vec2, p2: Vec2, at: Vec2) -> f64 {
    let (x0, x1) = (p1.x.min(p2.x), p1.x.max(p2.x));
    let (y0, y1) = (p1.y.min(p2.y), p1.y.max(p2.y));
    let outside_y = at.y < y0 || at.y > y1;
    let outside_x = at.x < x0 || at.x > x1;
    if outside_x && !outside_y { std::f64::consts::FRAC_PI_2 } else { 0.0 }
}

pub(super) fn linear_kind(p1: Vec2, p2: Vec2, at: Vec2, rotation: Option<f64>, aligned: bool) -> DimKind {
    if aligned { DimKind::Aligned } else { DimKind::Linear { rotation: rotation.unwrap_or_else(|| auto_rotation(p1, p2, at)) } }
}

/// Add a dimension. With DIMASSOC on, definition points that sit on object endpoints,
/// vertices or centres are attached to them (associative dimensions).
pub(super) fn add_dim(s: &mut Session, k: EntityKind) -> Result<Handle> {
    let mut k = k;
    if let EntityKind::Dimension(dm) = &mut k
        && dm.assoc.is_empty()
        && assoc_enabled(s)
    {
        dm.assoc = crate::assoc::reassociate(s.doc()?, &s.space(), dm);
    }
    // The measurement is what the DXF/DWG writer computes from the geometry (group 42); filling it
    // in now means queries show it before the drawing is saved and reopened (#2421).
    if let EntityKind::Dimension(dm) = &mut k
        && let Ok(d) = s.doc()
    {
        let style = d.dim_style(&dm.style).cloned().unwrap_or_default();
        let g = cadcraft_render::dimension_geometry(dm, &style, d.header.f64("DIMSCALE", 1.0));
        dm.measurement = g.measurement;
        // The text position the writer puts in group 11 when the user has not placed the text.
        if !dm.user_text_pos {
            dm.text_mid = g.text_pos.to3(0.0);
        }
    }
    let h = s.add_entity(k)?;
    s.last_dim = Some(h);
    Ok(h)
}

pub(super) fn assoc_enabled(s: &Session) -> bool {
    s.doc().map(|d| d.header.i64("DIMASSOC", 2) != 0).unwrap_or(false)
}

pub(super) fn with_assoc(k: EntityKind, assoc: Vec<DimAssoc>) -> EntityKind {
    match k {
        EntityKind::Dimension(mut d) => {
            d.assoc = assoc;
            EntityKind::Dimension(d)
        }
        other => other,
    }
}

/// Centre and radius of a circle/arc entity.
pub(super) fn circle_of(s: &Session, h: Handle) -> Option<(Vec2, f64)> {
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Circle(c) => Some((c.center.xy(), c.radius)),
        EntityKind::Arc(a) => Some((a.center.xy(), a.radius)),
        _ => None,
    }
}

/// The dimension line location of a radius/diameter dimension puts the text there, inside or
/// outside the circle, as a user-positioned text (DXF flag 70 bit 128), like AutoCAD.
pub(super) fn text_at(mut k: EntityKind, at: Vec2) -> EntityKind {
    if let EntityKind::Dimension(d) = &mut k {
        d.text_mid = v3(at);
        d.user_text_pos = true;
    }
    k
}

pub(super) fn hex_param(p: &Value, key: &str) -> Option<Handle> {
    p.get(key).and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)))
}

// ---------------- interactive ----------------

pub(super) fn pick_at(s: &Session, p: Vec2) -> Option<Handle> {
    let ap = s.pixel_size() * s.settings.pickbox.max(1.0) * 1.5;
    crate::select::pick(s.doc().ok()?, &s.space(), p, ap)
}

/// The straight or curved piece of `h` picked at `p`: a line, an arc, or the polyline segment nearest `p`.
pub(in crate::cmd) fn segment_at(s: &Session, h: Handle, p: Vec2) -> Option<Segment> {
    match &s.doc().ok()?.entity(h)?.kind {
        EntityKind::Line(l) => Some(Segment::Line(cadcraft_geom::Line::new(l.a.xy(), l.b.xy()))),
        EntityKind::Arc(a) => Some(Segment::Arc { arc: cadcraft_geom::Arc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true }),
        EntityKind::LwPolyline(pl) => cadcraft_geom::Polyline { vertices: pl.vertices.clone(), closed: pl.closed }
            .segments()
            .into_iter()
            .min_by(|x, y| x.closest(p).dist(p).total_cmp(&y.closest(p).dist(p))),
        _ => None,
    }
}

/// What a dimension placement prompt asks after Mtext, Text or Angle.
#[derive(Clone, Copy, PartialEq)]
enum DimAsk {
    Text,
    Angle,
}

/// The Mtext / Text / Angle options of the dimension placement prompts: replace the measured text
/// (`<>` stands for the measurement) and rotate the text.
#[derive(Default)]
pub(super) struct DimText {
    text: String,
    angle: f64,
    asking: Option<DimAsk>,
}

impl DimText {
    /// Opens the sub-prompt for Mtext, Text or Angle; `false` for any other keyword.
    pub(super) fn keyword(&mut self, k: &str) -> bool {
        self.asking = match k {
            "Mtext" | "Text" => Some(DimAsk::Text),
            "Angle" => Some(DimAsk::Angle),
            _ => return false,
        };
        true
    }
    pub(super) fn prompt(&self) -> Option<Prompt> {
        Some(match self.asking? {
            DimAsk::Text => Prompt::new("Enter dimension text (<> = measurement)", Accept::TEXT),
            DimAsk::Angle => Prompt::new("Specify angle of dimension text", Accept::NUMBER),
        })
    }
    /// Takes the answer to the open sub-prompt (Enter keeps the current value); `false` when none is open.
    pub(super) fn answer(&mut self, s: &mut Session, i: &Input) -> bool {
        let Some(a) = self.asking else { return false };
        match (a, i) {
            (DimAsk::Text, Input::Text(t)) => self.text = t.clone(),
            (DimAsk::Angle, Input::Text(t)) => match s.angle_settings().direction(t) {
                Some(r) => self.angle = r,
                None => {
                    s.echo("Requires a valid angle.");
                    return true;
                }
            },
            _ => {}
        }
        self.asking = None;
        true
    }
    /// `k` with this text and text angle.
    pub(super) fn apply(&self, mut k: EntityKind) -> EntityKind {
        if let EntityKind::Dimension(d) = &mut k {
            d.text = self.text.clone();
            d.text_rotation = self.angle;
        }
        k
    }
}
