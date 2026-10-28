use super::{Bounds2, Handle, Pen, Rgb, Sheet, Vec2, outline};

/// Handle carried by model-space geometry drawn inside a paper-space viewport (it belongs to no
/// single paper-space entity, so selection highlighting never matches it).
pub const VIEWPORT_CONTENT: Handle = Handle(0);

/// [`DisplayList::prim_layers`] of a primitive whose layer isn't in the layer table.
pub const NO_LAYER: u32 = u32::MAX;

/// What a display-list primitive draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `verts[start..start+len]` as a connected polyline.
    Polyline,
    /// `tris[start..start+len]`, three vertices per triangle.
    Tris,
    /// A wipeout: triangles laid out like [`Kind::Tris`], filled with the background colour
    /// (paper white when plotting, the canvas colour on screen) so they hide what was drawn
    /// before them.
    Mask,
    /// A single point marker at `verts[start]`.
    Point,
    /// An infinite line or ray: `verts[start]` = base, `verts[start+1]` = unit direction.
    Infinite { ray: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DPrim {
    pub handle: Handle,
    pub color: Rgb,
    /// Colour 7 (directly, by layer or by block): drawn white on a dark background and black on
    /// a light one, see [`DPrim::display_rgb`]. Other colours keep their RGB.
    pub aci7: bool,
    /// Lineweight in mm (0 = thinnest).
    pub lw: f32,
    /// Transparency in percent (0 = opaque, at most 90): the object's own, its layer's when
    /// ByLayer, the block reference's when ByBlock. Always 0 unless [`Options::transparency`].
    pub transparency: u8,
    pub kind: Kind,
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub prims: Vec<DPrim>,
    pub verts: Vec<Vec2>,
    pub tris: Vec<Vec2>,
    /// Plots ([`build_plot`]): for each primitive, the index in [`Drawing::layers`] of the layer
    /// it is drawn on (block contents on layer 0: the block reference's; [`NO_LAYER`] when the
    /// layer isn't in the table), for PDF layers. Empty for other lists.
    pub prim_layers: Vec<u32>,
    /// Plots: outline-font glyphs that a plotter can write as text instead of drawing the
    /// filled outline primitive they stand for ([`PlotText::prim`]), in drawing order.
    pub texts: Vec<PlotText>,
    pub bounds: Bounds2,
    /// The paper sheet when the list was built for a layout (paper space); the UI draws the
    /// white sheet, its shadow and the printable-area outline from it.
    pub sheet: Option<Sheet>,
    /// Block contents expanded while building (see [`MAX_BLOCK_EXPANSION`]).
    pub expanded: usize,
    /// Plot style end, join and fill style per primitive (parallel to `prims`; missing entries
    /// are "use object"), turned into geometry by [`pen::apply`].
    pub pens: Vec<Pen>,
    /// PDF underlays' pages, in drawing order: the space's own and, in a layout, model space's
    /// seen through each viewport (not those in blocks). The UI and plotters draw each page
    /// before primitive [`ListUnderlay::at`], so objects drawn earlier are under it and later
    /// ones over it (DRAWORDER).
    pub underlays: Vec<ListUnderlay>,
}

/// A PDF underlay's page as a display list draws it: the underlay entity, the primitive it is
/// drawn before, the page's lower-left corner and width/height vectors in the list's units
/// (paper units for model space seen through a viewport), and what it is clipped to.
#[derive(Clone, Debug)]
pub struct ListUnderlay {
    pub handle: Handle,
    pub at: u32,
    pub origin: Vec2,
    pub u: Vec2,
    pub v: Vec2,
    pub clip: PageClip,
    /// The underlay's own clip boundary (PDFCLIP), in the list's units; applied before `clip`.
    pub own: Option<std::sync::Arc<crate::clip::PolyClip>>,
    /// `own` is inverted: the page is drawn outside it.
    pub inverted: bool,
}

/// The PDF underlay a display list's page belongs to: a drawing's object, or one in a block
/// definition (a page seen through a block reference).
pub fn pdf_underlay(d: &cadcraft_doc::Drawing, h: Handle) -> Option<&cadcraft_doc::PdfUnderlay> {
    let e = d.entity(h).or_else(|| d.blocks.values().find_map(|b| b.entities.get(h)))?;
    match &e.kind {
        cadcraft_doc::EntityKind::PdfUnderlay(u) => Some(u),
        _ => None,
    }
}

/// What a page is clipped to: nothing, or the viewport it is seen through.
#[derive(Clone, Debug)]
pub enum PageClip {
    None,
    Rect(Bounds2),
    Poly(std::sync::Arc<crate::clip::PolyClip>),
}

impl ListUnderlay {
    /// The part of the page drawn, as triangles (three vertices each) in the list's units.
    pub fn triangles(&self) -> Vec<Vec2> {
        let (o, u, v) = (self.origin, self.u, self.v);
        let tris = [o, o + u, o + u + v, o, o + u + v, o + v];
        let tris = match &self.own {
            Some(c) if self.inverted => {
                // The page less the boundary (even-odd), kept on the page.
                let quad = [o, o + u, o + u + v, o + v];
                let outside = crate::fill::triangulate_evenodd(&[quad.to_vec(), c.polygon().to_vec()]);
                match crate::clip::PolyClip::new(&quad) {
                    Some(page) => page.clip_triangles(&outside),
                    None => Vec::new(),
                }
            }
            Some(c) => c.clip_triangles(&tris),
            None => tris.to_vec(),
        };
        match &self.clip {
            PageClip::None => tris,
            PageClip::Rect(r) => crate::clip::clip_triangles(&tris, r),
            PageClip::Poly(c) => c.clip_triangles(&tris),
        }
    }

    /// Only part of the page is drawn (its own clip boundary or a viewport's).
    pub fn is_clipped(&self) -> bool {
        self.own.is_some() || !matches!(self.clip, PageClip::None)
    }

    /// Where point `p` (list units) is on the page, as fractions of its width and height from
    /// its lower-left corner; `None` for a page of no area.
    pub fn fraction(&self, p: Vec2) -> Option<Vec2> {
        let (u, v) = (self.u, self.v);
        let det = u.x * v.y - u.y * v.x;
        if !det.is_finite() || det.abs() < 1e-300 {
            return None;
        }
        let d = p - self.origin;
        let f = Vec2::new((d.x * v.y - d.y * v.x) / det, (u.x * d.y - u.y * d.x) / det);
        f.is_finite().then_some(f)
    }

    /// The part of the page drawn, as page fractions `[left, bottom, right, top]`; `None` when
    /// none of it is.
    pub fn shown(&self) -> Option<[f64; 4]> {
        let fr: Vec<Vec2> = self.triangles().into_iter().filter_map(|p| self.fraction(p)).collect();
        let b = Bounds2::from_points(fr);
        let r = [b.min.x.max(0.0), b.min.y.max(0.0), b.max.x.min(1.0), b.max.y.min(1.0)];
        (!b.is_empty() && r.iter().all(|x| x.is_finite()) && r[2] > r[0] && r[3] > r[1]).then_some(r)
    }
}

/// A glyph of plotted text: the font glyph and, in world coordinates, where it sits. It is
/// drawn by primitive [`PlotText::prim`] (a filled outline in the primitive's colour); output
/// with real text writes the glyph instead of that primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotText {
    /// The filled outline primitive (index in [`DisplayList::prims`]); [`PlotText::NONE`] when
    /// the glyph has none in the list (clipped by a viewport).
    pub prim: u32,
    /// Triangle vertices of the outline when it was drawn, and their bounding box (a clipped
    /// outline has others).
    pub verts: u32,
    pub bounds: Bounds2,
    /// The glyph; its origin and em axes are in world coordinates.
    pub glyph: cadcraft_fonts::TextGlyph,
}

impl PlotText {
    pub const NONE: u32 = u32::MAX;
    /// [`PlotText::prim`] of a glyph cut by a viewport edge.
    pub const CLIPPED: u32 = u32::MAX - 1;
}

impl DPrim {
    /// The colour to draw with on `background`.
    pub fn display_rgb(&self, background: Rgb) -> Rgb {
        cadcraft_color::display_rgb(self.color, self.aci7, background)
    }
    /// The opacity to draw with: 255 when opaque, down to 26 at 90 % transparency.
    pub fn alpha(&self) -> u8 {
        alpha(self.transparency)
    }
}

/// The opacity (0..=255) of a transparency percentage, which is clamped to 0..=90 first.
pub fn alpha(transparency: u8) -> u8 {
    let t = u32::from(transparency.min(MAX_TRANSPARENCY));
    u8::try_from((255 * (100 - t) + 50) / 100).unwrap_or(u8::MAX)
}

/// The most transparent an object can be, in percent.
pub const MAX_TRANSPARENCY: u8 = 90;

impl DisplayList {
    pub fn segment_count(&self) -> usize {
        self.prims.iter().filter(|p| p.kind == Kind::Polyline).map(|p| p.len.saturating_sub(1) as usize).sum()
    }
    /// The polyline points of a primitive.
    pub fn points(&self, p: &DPrim) -> &[Vec2] {
        match p.kind {
            Kind::Tris | Kind::Mask => self.tris.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
            _ => self.verts.get(p.start as usize..(p.start + p.len) as usize).unwrap_or(&[]),
        }
    }
    /// The boundary of a filled primitive ([`Kind::Tris`], [`Kind::Mask`]) as closed polylines,
    /// for highlighting it; empty for other kinds.
    pub fn fill_outlines(&self, p: &DPrim) -> Vec<Vec<Vec2>> {
        match p.kind {
            Kind::Tris | Kind::Mask => outline::fill_outlines(self.points(p)),
            _ => Vec::new(),
        }
    }
}

/// Append `src` to `dst`: its primitives index into `dst`'s vertex and triangle arrays.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn append(dst: &mut DisplayList, src: DisplayList) {
    let (verts, tris, prims) = (dst.verts.len() as u32, dst.tris.len() as u32, dst.prims.len() as u32);
    // Pens are parallel to the primitives, with missing entries meaning "use object".
    if !src.pens.is_empty() {
        dst.pens.resize(dst.prims.len(), Pen::default());
    }
    dst.prims.extend(src.prims.into_iter().map(|mut p| {
        p.start += if matches!(p.kind, Kind::Tris | Kind::Mask) { tris } else { verts };
        p
    }));
    dst.verts.extend(src.verts);
    dst.tris.extend(src.tris);
    dst.prim_layers.extend(src.prim_layers);
    dst.texts.extend(src.texts.into_iter().map(|mut t| {
        if t.prim < PlotText::CLIPPED {
            t.prim += prims;
        }
        t
    }));
    dst.pens.extend(src.pens);
    dst.underlays.extend(src.underlays.into_iter().map(|u| ListUnderlay { at: u.at + prims, ..u }));
    dst.bounds = dst.bounds.union(&src.bounds);
    dst.expanded = dst.expanded.saturating_add(src.expanded);
}
