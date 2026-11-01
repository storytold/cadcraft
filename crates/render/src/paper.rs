//! Paper sizes and the layout sheet (paper rectangle, printable area) for paper space.
//!
//! Paper-space units are millimetres in metric drawings (`MEASUREMENT` = 1) and on layouts whose
//! page setup is in millimetres, and inches otherwise. The sheet's lower-left corner is the paper-space origin.
//! Model-space plots have no stored page setup, so they take their units from the paper itself
//! ([`model_page`], [`model_unit_mm`]).

use cadcraft_doc::{Drawing, PageSetup};
use cadcraft_geom::{Bounds2, Vec2};

/// Maximum model-space plot offset accepted by the PLOT command, in millimetres.
pub const MAX_PLOT_OFFSET_MM: f64 = 100_000.0;

/// A standard paper size (portrait: `width_mm <= height_mm`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperSize {
    /// Canonical media name, as shown in page setup.
    pub name: &'static str,
    /// Short names accepted by commands (`A4`, `Letter`, `ANSI B`…).
    pub aliases: &'static [&'static str],
    pub width_mm: f64,
    pub height_mm: f64,
    /// Inch-based (ANSI/ARCH) rather than ISO.
    pub imperial: bool,
}

const fn iso(name: &'static str, aliases: &'static [&'static str], w: f64, h: f64) -> PaperSize {
    PaperSize { name, aliases, width_mm: w, height_mm: h, imperial: false }
}
const fn inch(name: &'static str, aliases: &'static [&'static str], w: f64, h: f64) -> PaperSize {
    PaperSize { name, aliases, width_mm: w * 25.4, height_mm: h * 25.4, imperial: true }
}

/// Common ISO (A0–A6, B0–B5) and ANSI/ARCH sheet sizes (ISO 216, ANSI/ASME Y14.1).
pub const PAPER_SIZES: &[PaperSize] = &[
    iso("ISO A0 (841.00 x 1189.00 MM)", &["A0", "ISO A0"], 841.0, 1189.0),
    iso("ISO A1 (594.00 x 841.00 MM)", &["A1", "ISO A1"], 594.0, 841.0),
    iso("ISO A2 (420.00 x 594.00 MM)", &["A2", "ISO A2"], 420.0, 594.0),
    iso("ISO A3 (297.00 x 420.00 MM)", &["A3", "ISO A3"], 297.0, 420.0),
    iso("ISO A4 (210.00 x 297.00 MM)", &["A4", "ISO A4"], 210.0, 297.0),
    iso("ISO A5 (148.00 x 210.00 MM)", &["A5", "ISO A5"], 148.0, 210.0),
    iso("ISO A6 (105.00 x 148.00 MM)", &["A6", "ISO A6"], 105.0, 148.0),
    iso("ISO B0 (1000.00 x 1414.00 MM)", &["B0", "ISO B0"], 1000.0, 1414.0),
    iso("ISO B1 (707.00 x 1000.00 MM)", &["B1", "ISO B1"], 707.0, 1000.0),
    iso("ISO B2 (500.00 x 707.00 MM)", &["B2", "ISO B2"], 500.0, 707.0),
    iso("ISO B3 (353.00 x 500.00 MM)", &["B3", "ISO B3"], 353.0, 500.0),
    iso("ISO B4 (250.00 x 353.00 MM)", &["B4", "ISO B4"], 250.0, 353.0),
    iso("ISO B5 (176.00 x 250.00 MM)", &["B5", "ISO B5"], 176.0, 250.0),
    inch("ANSI A (8.50 x 11.00 Inches)", &["ANSI A", "Letter", "A-size"], 8.5, 11.0),
    inch("Legal (8.50 x 14.00 Inches)", &["Legal"], 8.5, 14.0),
    inch("ANSI B (11.00 x 17.00 Inches)", &["ANSI B", "Tabloid", "Ledger", "B-size"], 11.0, 17.0),
    inch("ANSI C (17.00 x 22.00 Inches)", &["ANSI C", "C-size"], 17.0, 22.0),
    inch("ANSI D (22.00 x 34.00 Inches)", &["ANSI D", "D-size"], 22.0, 34.0),
    inch("ANSI E (34.00 x 44.00 Inches)", &["ANSI E", "E-size"], 34.0, 44.0),
    inch("ARCH A (9.00 x 12.00 Inches)", &["ARCH A"], 9.0, 12.0),
    inch("ARCH B (12.00 x 18.00 Inches)", &["ARCH B"], 12.0, 18.0),
    inch("ARCH C (18.00 x 24.00 Inches)", &["ARCH C"], 18.0, 24.0),
    inch("ARCH D (24.00 x 36.00 Inches)", &["ARCH D"], 24.0, 36.0),
    inch("ARCH E1 (30.00 x 42.00 Inches)", &["ARCH E1"], 30.0, 42.0),
    inch("ARCH E (36.00 x 48.00 Inches)", &["ARCH E"], 36.0, 48.0),
];

impl PaperSize {
    /// Margins (left, bottom, right, top; mm) of the printable area the built-in plot devices
    /// give this size: 5 mm on ISO sheets, a quarter inch on inch sheets.
    pub fn margins_mm(&self) -> [f64; 4] {
        if self.imperial { [6.35; 4] } else { [5.0; 4] }
    }
}

/// Look up a paper size by canonical name or alias (case- and space-insensitive).
pub fn paper_size(name: &str) -> Option<&'static PaperSize> {
    let norm = |s: &str| s.chars().filter(|c| !c.is_whitespace() && *c != '_' && *c != '-').collect::<String>().to_ascii_lowercase();
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    PAPER_SIZES.iter().find(|p| norm(p.name) == n || p.aliases.iter().any(|a| norm(a) == n))
}

/// Millimetres per paper-space unit for a drawing (1 for metric, 25.4 for imperial).
pub fn paper_unit_mm(d: &Drawing) -> f64 {
    if d.header.i64("MEASUREMENT", 0) == 1 { 1.0 } else { 25.4 }
}

/// Whether the drawing's length unit is metric: `INSUNITS` millimetres to kilometres and
/// angstroms to parsecs (4..=7, 11..=20) are metric, inches to US survey units (1, 2, 3, 8, 9,
/// 10, 21, 22, 23, 24) imperial. A unitless drawing (`INSUNITS` 0, absent or unknown, as files
/// without `$INSUNITS` are read, #1209) falls back to `MEASUREMENT` = 1.
///
/// `MEASUREMENT` only chooses the imperial or ISO hatch and linetype libraries (#1649, #1826),
/// so it is not what says a drawing is metric: a drawing whose units are millimetres but whose
/// MEASUREMENT is 0 — what UNITS leaves when the user picks millimetres, and what AutoCAD writes
/// for many metric drawings — is metric by this test (#1828).
pub fn metric_length_unit(d: &Drawing) -> bool {
    match d.header.i64("INSUNITS", 0) {
        // Inches, feet, miles, microinches, mils, yards and US survey units.
        1 | 2 | 3 | 8 | 9 | 10 | 21 | 22 | 23 | 24 => false,
        // Millimetres, centimetres, metres, kilometres, angstroms, nanometres, microns,
        // decimetres to gigametres, astronomical units, light years and parsecs.
        4..=7 | 11..=20 => true,
        // Unitless: as MEASUREMENT says.
        _ => d.header.i64("MEASUREMENT", 0) == 1,
    }
}

/// Millimetres per paper unit of the paper named by `page`, from the paper itself: 1.0 for ISO
/// and user-defined millimetre sizes, 25.4 for the ANSI, ARCH and Legal sheets. A name that is
/// no known paper size is treated as a user millimetre size (1.0).
pub fn model_unit_mm(page: &PageSetup) -> f64 {
    match paper_size(&page.paper) {
        Some(p) if p.imperial => 25.4,
        _ => 1.0,
    }
}

/// The page a model-space plot starts from: ISO A4 in a drawing whose length unit is metric,
/// ANSI A otherwise. Model space has no stored page setup, so the plot takes its sheet (and its
/// units) from here unless the caller picks another paper.
pub fn model_page(d: &Drawing) -> PageSetup {
    let mut page = PageSetup::default();
    if let Some(p) = paper_size(if metric_length_unit(d) { "A4" } else { "ANSI A" }) {
        page.paper = p.name.into();
        page.width_mm = p.width_mm;
        page.height_mm = p.height_mm;
    }
    apply_model_paper_units(&mut page);
    page
}

/// Set a model-space plot's paper units once its paper is known: the paper's own units decide
/// (ISO and user millimetre sizes are millimetres, ANSI and ARCH sheets inches), as a model-space
/// page setup's "plot paper units" would. Layouts keep their stored page setup and go through
/// [`page_unit_mm`]; only model-space callers use this.
pub fn apply_model_paper_units(page: &mut PageSetup) {
    page.paper_mm = model_unit_mm(page) == 1.0;
}

/// Millimetres per paper-space unit on a layout with page setup `page`: millimetres when the
/// page says so (plot paper units mm), else as the drawing's MEASUREMENT says. Drawings from
/// AutoCAD often have MEASUREMENT 0 with layouts in millimetres (#1058).
pub fn page_unit_mm(d: &Drawing, page: &PageSetup) -> f64 {
    if page.paper_mm { 1.0 } else { paper_unit_mm(d) }
}

/// Millimetres per paper-space unit on the layout holding the paper-space entity `h` (the
/// drawing's when no layout holds it).
pub fn unit_mm_of(d: &Drawing, h: cadcraft_doc::Handle) -> f64 {
    d.layouts.iter().find(|l| l.entities.contains(h)).map_or_else(|| paper_unit_mm(d), |l| page_unit_mm(d, &l.page))
}

/// The paper sheet of a layout, in paper-space units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    /// Sheet size (already rotated for landscape). The sheet spans `(0,0)..size`.
    pub size: Vec2,
    /// Printable area (inside the margins).
    pub printable: Bounds2,
    /// Millimetres per paper-space unit.
    pub unit_mm: f64,
}

impl Sheet {
    pub fn bounds(&self) -> Bounds2 {
        Bounds2::new(Vec2::ZERO, self.size)
    }
    /// The sheet for a page setup in a drawing whose paper units are `unit_mm` millimetres.
    pub fn from_page(page: &PageSetup, unit_mm: f64) -> Sheet {
        let sane = |v: f64, d: f64| if v.is_finite() && v > 0.0 { v.min(100_000.0) } else { d };
        let w = sane(page.width_mm, 215.9);
        let h = sane(page.height_mm, 279.4);
        let (w, h) = if page.landscape { (w.max(h), w.min(h)) } else { (w.min(h), w.max(h)) };
        let u = if unit_mm.is_finite() && unit_mm > 0.0 { unit_mm } else { 1.0 };
        let size = Vec2::new(w / u, h / u);
        let m = |i: usize| page.margins_mm.get(i).copied().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0) / u;
        // Margins: left, bottom, right, top (DXF group codes 40..43).
        let (l, b, r, t) = (m(0), m(1), m(2), m(3));
        let printable = if l + r < size.x && b + t < size.y {
            Bounds2::new(Vec2::new(l, b), Vec2::new(size.x - r, size.y - t))
        } else {
            Bounds2::new(Vec2::ZERO, size)
        };
        Sheet { size, printable, unit_mm: u }
    }
}

/// The sheet of a named layout (`None` if there is no such layout).
pub fn sheet(d: &Drawing, layout: &str) -> Option<Sheet> {
    let l = d.layouts.iter().find(|l| l.name == layout).or_else(|| d.layout(layout))?;
    Some(Sheet::from_page(&l.page, page_unit_mm(d, &l.page)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_length_unit_reads_the_length_unit() {
        // Metric units, whatever MEASUREMENT says.
        let mut d = Drawing::new_imperial();
        d.header.set_i64("MEASUREMENT", 0);
        for u in [4, 5, 6, 7, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20] {
            d.header.set_i64("INSUNITS", u);
            assert!(metric_length_unit(&d), "INSUNITS {u} is metric");
        }
        // Imperial units, even with MEASUREMENT 1.
        d.header.set_i64("MEASUREMENT", 1);
        for u in [1, 2, 3, 8, 9, 10, 21, 22, 23, 24] {
            d.header.set_i64("INSUNITS", u);
            assert!(!metric_length_unit(&d), "INSUNITS {u} is imperial");
        }
        // Unitless: as MEASUREMENT says.
        for u in [0, 25] {
            d.header.set_i64("INSUNITS", u);
            d.header.set_i64("MEASUREMENT", 1);
            assert!(metric_length_unit(&d), "unitless with MEASUREMENT 1 is metric");
            d.header.set_i64("MEASUREMENT", 0);
            assert!(!metric_length_unit(&d), "unitless with MEASUREMENT 0 is imperial");
        }
        d.header.vars.remove("INSUNITS");
        d.header.vars.remove("MEASUREMENT");
        assert!(!metric_length_unit(&d), "no units at all is imperial, as before");
    }

    #[test]
    fn paper_unit_mm_still_follows_measurement() {
        let mut m = Drawing::new_metric();
        assert_eq!(paper_unit_mm(&m), 1.0);
        m.header.set_i64("MEASUREMENT", 0);
        assert_eq!(paper_unit_mm(&m), 25.4, "layouts stay on MEASUREMENT (page setups in inches fit)");
        let mut i = Drawing::new_imperial();
        assert_eq!(paper_unit_mm(&i), 25.4);
        i.header.set_i64("INSUNITS", 4);
        assert_eq!(paper_unit_mm(&i), 25.4, "a metric length unit does not move layout paper units");
        i.header.set_i64("MEASUREMENT", 1);
        assert_eq!(paper_unit_mm(&i), 1.0);
    }

    #[test]
    fn model_unit_mm_reads_the_paper() {
        let page = |name: &str| PageSetup { paper: name.into(), ..PageSetup::default() };
        assert_eq!(model_unit_mm(&page("ISO A4")), 1.0);
        assert_eq!(model_unit_mm(&page("ISO A0")), 1.0);
        assert_eq!(model_unit_mm(&page("ANSI A")), 25.4);
        assert_eq!(model_unit_mm(&page("ARCH D")), 25.4);
        assert_eq!(model_unit_mm(&page("User (300.00 x 400.00 MM)")), 1.0);
        assert_eq!(model_unit_mm(&page("nope")), 1.0, "an unknown paper is a user millimetre size");
        assert_eq!(model_unit_mm(&PageSetup::default()), 25.4, "the default page is ANSI A");
    }

    #[test]
    fn model_page_is_a4_for_metric_drawings() {
        let mut d = Drawing::new_metric();
        let page = model_page(&d);
        assert_eq!(page.paper, "ISO A4 (210.00 x 297.00 MM)");
        assert_eq!((page.width_mm, page.height_mm), (210.0, 297.0));
        assert!(page.paper_mm, "A4 in a metric drawing is in millimetres");
        // The length unit decides, not MEASUREMENT.
        d.header.set_i64("MEASUREMENT", 0);
        assert_eq!(model_page(&d).paper, "ISO A4 (210.00 x 297.00 MM)");
        // An imperial drawing starts on ANSI A.
        let page = model_page(&Drawing::new_imperial());
        assert_eq!(page.paper, "ANSI A (8.50 x 11.00 Inches)");
        assert!((page.width_mm - 215.9).abs() < 1e-9 && (page.height_mm - 279.4).abs() < 1e-9);
        assert!(!page.paper_mm, "ANSI A in an imperial drawing is in inches");
    }

    #[test]
    fn model_paper_units_follow_the_chosen_paper() {
        // The sheet decides, whatever the drawing's length unit is.
        let mut a4 = PageSetup { paper: "ISO A4 (210.00 x 297.00 MM)".into(), ..PageSetup::default() };
        apply_model_paper_units(&mut a4);
        assert!(a4.paper_mm, "ISO A4 is a millimetre sheet");
        let mut ansi = PageSetup { paper: "ANSI A (8.50 x 11.00 Inches)".into(), ..PageSetup::default() };
        apply_model_paper_units(&mut ansi);
        assert!(!ansi.paper_mm, "ANSI A is an inch sheet");
        // A user size in millimetres is one to one.
        let mut user = PageSetup { paper: "User (300.00 x 400.00 MM)".into(), ..PageSetup::default() };
        apply_model_paper_units(&mut user);
        assert!(user.paper_mm);
    }

    #[test]
    fn lookup_and_sheet() {
        assert_eq!(paper_size("a4").map(|p| p.width_mm), Some(210.0));
        assert_eq!(paper_size("ansi b").map(|p| p.height_mm), Some(17.0 * 25.4));
        assert_eq!(paper_size("Letter").map(|p| p.name), Some("ANSI A (8.50 x 11.00 Inches)"));
        assert!(paper_size("").is_none());
        assert!(paper_size("nope").is_none());
        let page = PageSetup { width_mm: 210.0, height_mm: 297.0, margins_mm: [5.0; 4], landscape: true, ..PageSetup::default() };
        let s = Sheet::from_page(&page, 1.0);
        assert_eq!(s.size, Vec2::new(297.0, 210.0));
        assert_eq!(s.printable, Bounds2::new(Vec2::new(5.0, 5.0), Vec2::new(292.0, 205.0)));
        let hostile = PageSetup { width_mm: f64::NAN, height_mm: -1.0, margins_mm: [1e9; 4], ..PageSetup::default() };
        let s = Sheet::from_page(&hostile, 0.0);
        assert!(s.size.x > 0.0 && s.printable == s.bounds());
    }
}
