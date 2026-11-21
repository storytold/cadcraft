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
        paper_mm: Some(true),
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
    assert_eq!(page.paper_mm, Some(true));
    assert_eq!(cadcraft_render::sheet(&back, "SHEET").unwrap().unit_mm, 1.0);
    // A metric drawing's layouts stay in millimetres even from files that say 72 = 0 and carry
    // no PSUNITS xdata (what earlier CADCraft versions wrote for every layout).
    let mut metric = Drawing::new_metric();
    metric.header.set_i64("MEASUREMENT", 1);
    let written = write_dxf(&metric);
    let text = written
        .replace("1001\r\nCADCRAFT\r\n1000\r\nPSUNITS\r\n1002\r\n{\r\n1070\r\n2\r\n1002\r\n}\r\n", "")
        .replace("\r\n 72\r\n1\r\n", "\r\n 72\r\n0\r\n");
    assert_ne!(text, written, "the layouts' 72 = 1 became 0");
    let back = read(text.as_bytes(), "old.dxf").unwrap();
    let name = back.layouts.first().unwrap().name.clone();
    assert_eq!(cadcraft_render::sheet(&back, &name).unwrap().unit_mm, 1.0);
}

#[test]
fn an_explicit_inch_layout_round_trips_on_a_metric_drawing() {
    // #3249: group 72 = 0 is also what an unset layout writes, so the explicit inch choice
    // travels in CADCraft xdata and must win over the group 72 fallback.
    let mut d = Drawing::new_metric();
    d.header.set_i64("MEASUREMENT", 1);
    let mut l = Layout::new("INCHES", 3);
    l.page = PageSetup {
        paper: "ARCH D".into(),
        width_mm: 609.6,
        height_mm: 914.4,
        margins_mm: [0.0; 4],
        landscape: true,
        paper_mm: Some(false),
        ..PageSetup::default()
    };
    d.layouts.push(l);
    let back = read(write_dxf(&d).as_bytes(), "x.dxf").unwrap();
    let page = &back.layouts.iter().find(|l| l.name == "INCHES").unwrap().page;
    assert_eq!(page.paper_mm, Some(false));
    let sheet = cadcraft_render::sheet(&back, "INCHES").unwrap();
    assert_eq!(sheet.unit_mm, 25.4);
    assert!((sheet.size.x - 36.0).abs() < 1e-9 && (sheet.size.y - 24.0).abs() < 1e-9, "{:?}", sheet.size);
}

#[test]
fn an_inherited_layout_keeps_following_measurement_after_a_dxf_round_trip() {
    // #3249: a layout that never chose its paper units reads back as inherited, not fixed to the
    // MEASUREMENT it had when saved. An ARCH D sheet on a metric drawing is 914.4 x 609.6; after
    // MEASUREMENT changes to imperial it must be 36 x 24. The explicit choices do not move.
    let mut d = Drawing::new_metric();
    d.header.set_i64("MEASUREMENT", 1);
    for (name, paper_mm) in [("INHERITED", None), ("INCHES", Some(false)), ("MILLIMETRES", Some(true))] {
        let mut l = Layout::new(name, 3);
        l.page = PageSetup {
            paper: "ARCH D".into(),
            width_mm: 609.6,
            height_mm: 914.4,
            margins_mm: [0.0; 4],
            landscape: true,
            paper_mm,
            ..PageSetup::default()
        };
        d.layouts.push(l);
    }

    let mut back = read(write_dxf(&d).as_bytes(), "x.dxf").unwrap();
    let units = |d: &Drawing, name: &str| d.layouts.iter().find(|l| l.name == name).unwrap().page.paper_mm;
    assert_eq!(units(&back, "INHERITED"), None);
    assert_eq!(units(&back, "INCHES"), Some(false));
    assert_eq!(units(&back, "MILLIMETRES"), Some(true));

    back.header.set_i64("MEASUREMENT", 0);
    let sheet = |name: &str| cadcraft_render::sheet(&back, name).unwrap();
    let inherited = sheet("INHERITED");
    assert_eq!(inherited.unit_mm, 25.4);
    assert!((inherited.size.x - 36.0).abs() < 1e-9 && (inherited.size.y - 24.0).abs() < 1e-9, "{:?}", inherited.size);
    assert_eq!(sheet("INCHES").unit_mm, 25.4);
    let metric = sheet("MILLIMETRES");
    assert_eq!(metric.unit_mm, 1.0);
    assert!((metric.size.x - 914.4).abs() < 1e-9 && (metric.size.y - 609.6).abs() < 1e-9, "{:?}", metric.size);
}
