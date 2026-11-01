//! #1058: a layout whose page setup is in millimetres (plot paper units 1) has millimetre paper
//! units even when the drawing's MEASUREMENT is 0, as drawings from AutoCAD often are.
//! CADCraft read MEASUREMENT only, took the paper as inches and plotted everything 25.4 times too
//! large, off the page.

use cadcraft_doc::{Common, Drawing, EntityKind, Layout, LwPolyline, PageSetup, Space};
use cadcraft_geom::{PolyVertex, Vec2};
use cadcraft_io::{PdfOptions, pdf, read, write_dxf};

/// An imperial drawing (MEASUREMENT 0) with an A4 landscape layout in millimetres and a frame
/// 10 mm inside the sheet.
fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.header.set_i64("MEASUREMENT", 0);
    let mut l = Layout::new("SHEET", 3);
    l.page = PageSetup {
        paper: "ISO A4".into(),
        width_mm: 210.0,
        height_mm: 297.0,
        margins_mm: [0.0; 4],
        landscape: true,
        paper_mm: true,
        ..PageSetup::default()
    };
    d.layouts.push(l);
    let frame = [(10.0, 10.0), (287.0, 10.0), (287.0, 200.0), (10.0, 200.0)];
    let pl = LwPolyline {
        vertices: frame.iter().map(|(x, y)| PolyVertex::new(Vec2::new(*x, *y))).collect(),
        closed: true,
        const_width: 0.0,
        elevation: 0.0,
        plinegen: false,
    };
    d.add(&Space::Paper("SHEET".into()), Common::default(), EntityKind::LwPolyline(pl)).unwrap();
    d
}

#[test]
fn a_millimetre_layout_plots_in_millimetres_whatever_measurement_says() {
    let d = drawing();
    let sheet = cadcraft_render::sheet(&d, "SHEET").unwrap();
    assert_eq!(sheet.unit_mm, 1.0);
    assert!((sheet.size.x - 297.0).abs() < 1e-9 && (sheet.size.y - 210.0).abs() < 1e-9, "{:?}", sheet.size);

    // The frame lands 10 mm (28.35 pt) inside the page, not 10 inches.
    let bytes = pdf(&d, &Space::Paper("SHEET".into()), &PdfOptions { compress: false, ..PdfOptions::default() }).unwrap();
    let doc = crate::pdf_read::read(&bytes);
    let xs: Vec<f64> =
        crate::pdf_read::drawn(doc.content()).iter().filter(|o| o.op == "m" || o.op == "l").filter_map(|o| o.nums.first().copied()).collect();
    assert!(!xs.is_empty(), "no path in the PDF");
    let (lo, hi) = xs.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!((lo - 28.35).abs() < 0.1 && (hi - 813.54).abs() < 0.1, "frame from {lo} to {hi} pt");
}

#[test]
fn plot_paper_units_round_trip_through_dxf() {
    // Written as group 72 = 1 (millimetres) and read back as such.
    let back = read(write_dxf(&drawing()).as_bytes(), "x.dxf").unwrap();
    let page = &back.layouts.iter().find(|l| l.name == "SHEET").unwrap().page;
    assert!(page.paper_mm);
    assert_eq!(cadcraft_render::sheet(&back, "SHEET").unwrap().unit_mm, 1.0);
    // A metric drawing's layouts stay in millimetres even from files that say 72 = 0 (what
    // earlier CADCraft versions wrote for every layout).
    let mut metric = Drawing::new_metric();
    metric.header.set_i64("MEASUREMENT", 1);
    let written = write_dxf(&metric);
    let text = written.replace("\r\n 72\r\n1\r\n", "\r\n 72\r\n0\r\n");
    assert_ne!(text, written, "the layouts' 72 = 1 became 0");
    let back = read(text.as_bytes(), "old.dxf").unwrap();
    let name = back.layouts.first().unwrap().name.clone();
    assert_eq!(cadcraft_render::sheet(&back, &name).unwrap().unit_mm, 1.0);
}
