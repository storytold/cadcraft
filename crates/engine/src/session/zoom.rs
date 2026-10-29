use super::{Bounds2, Result, Session, Space, Vec2, View};

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

    /// Extents of the space being edited, projected into the 2D view plane when `edit_orbit` is
    /// not Top.
    pub(crate) fn space_extents(&self) -> Result<Bounds2> {
        let space = self.space();
        let orbit = self.edit_orbit();
        let d = self.doc()?;
        if orbit.is_top() {
            Ok(d.extents(&space))
        } else {
            Ok(cadcraft_render::build(
                d,
                &space,
                &cadcraft_render::Options { orbit, text: true, fill: true, ..Default::default() },
            )
            .bounds)
        }
    }

    pub fn zoom_extents(&mut self) -> Result<()> {
        let orbit = self.edit_orbit();
        let ext = self.space_extents()?;
        let (w, h) = self.viewport_px;
        let ext = if ext.is_empty() {
            let lim = self.zoom_limits()?;
            if orbit.is_top() {
                lim
            } else {
                Bounds2::from_points([
                    orbit.project(Vec2::new(lim.min.x, lim.min.y).to3(0.0)),
                    orbit.project(Vec2::new(lim.max.x, lim.min.y).to3(0.0)),
                    orbit.project(Vec2::new(lim.max.x, lim.max.y).to3(0.0)),
                    orbit.project(Vec2::new(lim.min.x, lim.max.y).to3(0.0)),
                ])
            }
        } else {
            ext
        };
        let st = self.state_mut()?;
        let aspect = st.zoom_aspect(w / h.max(1.0));
        let height = ext.height().max(ext.width() / aspect.max(1e-6)).max(1e-6) * 1.05;
        st.set_zoom_frame(View { center: ext.center(), height });
        Ok(())
    }

    /// Zoom by `factor` (> 1 zooms in) keeping `about` fixed on screen.
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
