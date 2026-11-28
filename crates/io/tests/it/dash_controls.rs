//! The controls of #1069: one line of 49,999 or of 50,001 DASHED (0.5, -0.25) periods, as the
//! issue's generator writes it. A plot or export keeps the pattern on both: the PDF strokes a
//! subpath per dash and a close-up PNG of the line's start shows its gaps.

use cadcraft_doc::{Drawing, Space};
use cadcraft_geom::{Bounds2, Vec2};
use cadcraft_io::PdfOptions;

/// One line on layer L1 with linetype DASHED, `periods` periods long.
fn control(periods: f64) -> Drawing {
    let len = periods * 0.75;
    let dxf = format!(
        "0
SECTION
2
HEADER
9
$ACADVER
1
AC1032
9
$INSUNITS
70
4
9
$LTSCALE
40
1.0
0
ENDSEC
         0
SECTION
2
TABLES
0
TABLE
2
LTYPE
70
2
         0
LTYPE
2
CONTINUOUS
70
0
3
Solid line
72
65
73
0
40
0.0
         0
LTYPE
2
DASHED
70
0
3
Dashed 0.5/0.25
72
65
73
2
40
0.75
49
0.5
74
0
49
-0.25
74
0
0
ENDTAB
         0
TABLE
2
LAYER
70
1
0
LAYER
2
L1
70
0
62
7
6
DASHED
0
ENDTAB
0
ENDSEC
         0
SECTION
2
ENTITIES
0
LINE
8
L1
6
DASHED
10
0.0
20
0.0
30
0.0
11
{len}
21
0.0
31
0.0
0
ENDSEC
0
EOF
"
    );
    cadcraft_io::read(dxf.as_bytes(), "control.dxf").unwrap()
}

/// Runs of inked columns across the middle rows of `png` (a horizontal line through its centre):
/// one for a continuous line, one per dash for a dashed one.
fn dashes_across_the_middle(png: &[u8]) -> usize {
    let r = cadcraft_io::raster::decode(png).unwrap();
    let (w, h) = (r.width(), r.height());
    let paper = r.pixel(0, 0);
    let inked = |x: u32| (h / 2 - 3..=h / 2 + 3).any(|y| r.pixel(x, y) != paper);
    (0..w).filter(|&x| inked(x) && (x == 0 || !inked(x - 1))).count()
}

#[test]
fn the_49_999_and_50_001_period_controls_keep_their_dashes_in_pdf_and_png() {
    for periods in [49_999.0, 50_001.0] {
        let d = control(periods);
        let pdf = cadcraft_io::pdf(&d, &Space::Model, &PdfOptions { compress: false, ..PdfOptions::default() }).unwrap();
        let doc = crate::pdf_read::read(&pdf);
        let dashes = crate::pdf_read::drawn(doc.content()).iter().filter(|o| o.op == "m").count();
        assert!(dashes > periods as usize, "{periods} periods: {dashes} dashes in the PDF");
        // As `cadcraft-cli convert --window 0,-3,60,3`: 80 periods fill the image.
        let png = cadcraft_io::write_framed(&d, "close.png", Some(Bounds2::new(Vec2::new(0.0, -3.0), Vec2::new(60.0, 3.0)))).unwrap();
        let shown = dashes_across_the_middle(&png);
        assert!(shown > 60, "{periods} periods: {shown} dashes across the close-up");
    }
}
