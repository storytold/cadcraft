use super::{Bounds2, Result, Session, Space, Vec2, View};
use crate::render;

impl Session {
    // ---------------- views ----------------

    /// The limits of the space being edited: the sheet in paper space, else LIMMIN/LIMMAX.
    pub fn zoom_limits(&self) -> Result<Bounds2> {
        let d = self.doc()?;
        if let Space::Paper(name) = self.space()
            && let Some(sheet) = cadcraft_render::sheet(d, &name)
        {
            return Ok(sheet.bounds());
        }
        let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
        let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
        Ok(Bounds2::new(lo, hi))
    }

    /// [`Session::zoom_limits`] as the view shows them: in a 3D view, the limits (a rectangle on
    /// the XY plane) in view coordinates.
    pub fn view_limits(&self) -> Result<Bounds2> {
        let lim = self.zoom_limits()?;
        Ok(match self.state()?.projection() {
            Some(m) => {
                let p = m.plane(0.0);
                let corners = [lim.min, Vec2::new(lim.max.x, lim.min.y), lim.max, Vec2::new(lim.min.x, lim.max.y)];
                Bounds2::from_points(corners.map(|c| p.apply(c)))
            }
            None => lim,
        })
    }

    /// The current space's extents as the view shows them: the drawing's extents in plan view,
    /// the bounds of what is drawn in a 3D view (in view coordinates).
    pub fn view_extents(&self) -> Result<Bounds2> {
        let space = self.space();
        let d = self.doc()?;
        let ext = d.extents(&space);
        match self.state()?.projection() {
            Some(projection) if !ext.is_empty() => {
                let tolerance = (ext.width().max(ext.height()) / 1000.0).max(1e-9);
                let opts = render::Options { projection: Some(projection), tolerance, ..render::Options::default() };
                Ok(render::build(d, &space, &opts).bounds)
            }
            _ => Ok(ext),
        }
    }

    pub fn zoom_extents(&mut self) -> Result<()> {
        let ext = self.view_extents()?;
        let (w, h) = self.viewport_px;
        let ext = if ext.is_empty() { self.view_limits()? } else { ext };
        let st = self.state_mut()?;
        let aspect = st.zoom_aspect(w / h.max(1.0));
        let height = ext.height().max(ext.width() / aspect.max(1e-6)).max(1e-6) * 1.05;
        st.set_zoom_frame(View { center: ext.center(), height });
        Ok(())
    }

    /// Zoom by `factor` (> 1 zooms in) keeping `about` (in view coordinates, where the view's centre
    /// is) fixed on screen.
    pub fn zoom_about(&mut self, factor: f64, about: Vec2) -> Result<()> {
        if !(factor.is_finite() && factor > 0.0) {
            return Ok(());
        }
        let st = self.state_mut()?;
        let v = st.view();
        let h = (v.height / factor).clamp(1e-9, 1e12);
        let k = h / v.height;
        let c = about + (v.center - about) * k;
        st.set_view_quiet(View { center: c, height: h });
        Ok(())
    }
}
