//! Plot style resolution: a plot style table applied to the display list while it is built.
//!
//! Plots (PDF) and layouts shown with "Display plot styles" (screen, PNG and SVG) draw each
//! object with the plot style that applies to it. In a colour-dependent table that is the style
//! of the object's index colour; a true colour uses the style of its nearest index colour. In a
//! named table it is the object's PlotStyle property: a style name, `ByLayer` (the layer's
//! style; layer 0 inside a block takes the reference's layer) or `ByBlock` (the block
//! reference's); names the table lacks print as `Normal`. The
//! style replaces or keeps the colour, turns it grey and screens it (toward the white paper),
//! and replaces the linetype and lineweight. Colour 7 prints black, as on paper. Its end, join
//! and fill styles travel with each primitive ([`crate::Pen`]).

use cadcraft_color::{Rgb, nearest_aci};
use cadcraft_doc::{Common, Drawing, Linetype, PlotStyle, PlotStyleTable};

use crate::{Ctx, Ink, Pen};

/// A table ready for the builder: styles plus their resolved linetype overrides.
pub(crate) struct Styler {
    table: std::sync::Arc<PlotStyleTable>,
    /// Per style: `None` keeps the object's linetype; `Some(None)` draws continuous;
    /// `Some(Some(lt))` uses `lt`.
    linetypes: Vec<Option<Option<Linetype>>>,
}

impl Styler {
    pub(crate) fn new(d: &Drawing, table: std::sync::Arc<PlotStyleTable>) -> Styler {
        let mut library: Option<Vec<Linetype>> = None;
        let linetypes = table
            .styles
            .iter()
            .map(|s| {
                let name = s.linetype.as_deref()?;
                if name.eq_ignore_ascii_case("continuous") || name.eq_ignore_ascii_case("solid") {
                    return Some(None);
                }
                let found = d.linetype(name).cloned().or_else(|| {
                    let lib = library.get_or_insert_with(|| cadcraft_doc::library::standard_linetypes_for(d));
                    lib.iter().find(|l| l.name.eq_ignore_ascii_case(name)).cloned()
                })?;
                Some((!found.pattern.is_empty()).then_some(found))
            })
            .collect();
        Styler { table, linetypes }
    }

    /// Whether styles are picked by name.
    pub(crate) fn is_named(&self) -> bool {
        !self.table.is_color_dependent()
    }

    /// The named style an object with properties `c` prints with (index into the table).
    pub(crate) fn named_index(&self, ctx: &Ctx, c: &Common) -> Option<usize> {
        if !self.is_named() {
            return None;
        }
        let layer_name = if c.layer == "0" { ctx.block_layer.as_deref().unwrap_or("0") } else { c.layer.as_str() };
        let by_name = |n: &str| self.table.styles.iter().position(|s| s.name.eq_ignore_ascii_case(n.trim()));
        let normal = || by_name(cadcraft_doc::NORMAL_STYLE);
        let p = c.plot_style.trim();
        if p.is_empty() || p.eq_ignore_ascii_case("bylayer") {
            ctx.d.layer(layer_name).and_then(|l| by_name(&l.plot_style)).or_else(normal)
        } else if p.eq_ignore_ascii_case("byblock") {
            ctx.block_pstyle.or_else(normal)
        } else {
            by_name(p).or_else(normal)
        }
    }

    /// Index of the style that applies to `ink` (colour-dependent tables) or the object's
    /// named style `named`.
    fn index(&self, ink: Ink, named: Option<usize>) -> Option<usize> {
        if self.is_named() {
            return named.filter(|i| *i < self.table.styles.len());
        }
        let aci = if ink.aci == 0 { nearest_aci(ink.rgb) } else { ink.aci };
        let i = usize::from(aci).checked_sub(1)?;
        (i < self.table.styles.len()).then_some(i)
    }

    fn style(&self, ink: Ink, named: Option<usize>) -> Option<&PlotStyle> {
        self.table.styles.get(self.index(ink, named)?)
    }

    /// `ink` and lineweight `lw` (mm) as the plot style prints them. `lineweights`: lineweights
    /// are drawn at all (a style's lineweight replaces the object's only then).
    pub(crate) fn apply(&self, ink: Ink, named: Option<usize>, lw: f32, lineweights: bool) -> (Ink, f32) {
        let Some(s) = self.style(ink, named) else { return (ink, lw) };
        let lw = match s.lineweight {
            Some(w) if lineweights && lw >= 0.0 => f32::from(w.min(211)) / 100.0,
            _ => lw,
        };
        if s.keeps_color() {
            return (ink, lw);
        }
        // Colour 7 is black on the paper the style prints on.
        let base = if ink.aci7 { Rgb(0, 0, 0) } else { ink.rgb };
        (Ink { rgb: s.apply_color(base), aci7: false, ..ink }, lw)
    }

    /// The end, join and fill style of the style that applies to `ink`.
    pub(crate) fn pen(&self, ink: Ink, named: Option<usize>) -> Pen {
        self.style(ink, named).map(Pen::of).unwrap_or_default()
    }

    /// The linetype override of the style that applies to `ink`: `None` keeps the object's,
    /// `Some(None)` draws continuous.
    pub(crate) fn linetype(&self, ink: Ink, named: Option<usize>) -> Option<Option<Linetype>> {
        self.linetypes.get(self.index(ink, named)?)?.clone()
    }
}
