//! Snap queries through the same rectangular layout viewports that the renderer displays.

use cadcraft_doc::{Drawing, EntityKind, Handle, Space, Viewport};
use cadcraft_geom::{Bounds2, Vec2};
use serde::Serialize;

use super::{Deferred, SnapCurve, SnapHit};

/// A model-space snap seen through a layout viewport. Commands still receive paper coordinates;
/// DIST can use these model coordinates when both picks came through the same viewport.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ViewportPoint {
    pub viewport: Handle,
    pub point: Vec2,
    /// Paper units per model unit at the time of the pick.
    pub scale: f64,
}

pub(super) struct Projection<'a> {
    pub handle: Handle,
    pub viewport: &'a Viewport,
    pub scale: f64,
    pub model_clip: Bounds2,
    paper_clip: Bounds2,
}

impl<'a> Projection<'a> {
    fn new(handle: Handle, viewport: &'a Viewport) -> Option<Self> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        let center = viewport.center.xy();
        if viewport.id == 1
            || !(center.is_finite() && viewport.view_center.is_finite())
            || ![viewport.width, viewport.height, viewport.view_height].into_iter().all(positive)
        {
            return None;
        }
        let scale = viewport.height / viewport.view_height;
        if !positive(scale) || !positive(1.0 / scale) {
            return None;
        }
        let half = Vec2::new(viewport.width / 2.0, viewport.height / 2.0);
        let paper_clip = Bounds2::new(center - half, center + half);
        let model_clip = Bounds2::new(viewport.view_center - half / scale, viewport.view_center + half / scale);
        if ![paper_clip.min, paper_clip.max, model_clip.min, model_clip.max].into_iter().all(Vec2::is_finite) {
            return None;
        }
        Some(Self { handle, viewport, scale, model_clip, paper_clip })
    }

    pub fn to_model(&self, point: Vec2) -> Vec2 {
        self.viewport.view_center + (point - self.viewport.center.xy()) / self.scale
    }

    fn to_paper(&self, point: Vec2) -> Vec2 {
        self.viewport.center.xy() + (point - self.viewport.view_center) * self.scale
    }

    pub fn hit(&self, mut hit: SnapHit) -> Option<SnapHit> {
        let model_point = hit.point;
        hit.point = self.to_paper(model_point);
        if !hit.point.is_finite() || !self.paper_clip.contains(hit.point) {
            return None;
        }
        hit.viewport = Some(ViewportPoint { viewport: self.handle, point: model_point, scale: self.scale });
        if let Some(d) = hit.deferred {
            let curve = match d.curve {
                SnapCurve::Circle { center, radius } => {
                    let (center, radius) = (self.to_paper(center), radius * self.scale);
                    if !(center.is_finite() && radius.is_finite() && radius > 0.0) {
                        return None;
                    }
                    SnapCurve::Circle { center, radius }
                }
                SnapCurve::Arc { center, radius, start, end } => {
                    let (center, radius) = (self.to_paper(center), radius * self.scale);
                    if !(center.is_finite() && radius.is_finite() && radius > 0.0) {
                        return None;
                    }
                    SnapCurve::Arc { center, radius, start, end }
                }
                SnapCurve::Line { a, b } => {
                    let (a, b) = (self.to_paper(a), self.to_paper(b));
                    if !(a.is_finite() && b.is_finite()) {
                        return None;
                    }
                    SnapCurve::Line { a, b }
                }
            };
            hit.deferred = Some(Deferred { mode: d.mode, curve, at: hit.point });
        }
        Some(hit)
    }
}

/// Bound layout work like the renderer. An off viewport layer hides its border, while a frozen
/// viewport layer (or an invisible viewport entity) also hides its model contents.
pub(super) fn near<'a>(d: &'a Drawing, space: &Space, cursor: Vec2, aperture: f64) -> Vec<Projection<'a>> {
    if !matches!(space, Space::Paper(_)) {
        return Vec::new();
    }
    let Some(store) = d.space(space) else { return Vec::new() };
    let probe = Bounds2::new(cursor, cursor).expand(aperture);
    store
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Viewport(vp) => Some((e, vp)),
            _ => None,
        })
        .take(256)
        .filter(|(e, _)| e.common.visible && !d.layer(&e.common.layer).is_some_and(|l| l.frozen))
        .filter_map(|(e, vp)| Projection::new(e.handle, vp))
        .filter(|p| p.paper_clip.intersects(&probe))
        .collect()
}
