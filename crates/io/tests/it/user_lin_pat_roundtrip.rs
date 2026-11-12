//! Linetypes and hatch patterns read from the user's own `.lin` / `.pat` files (#552) are kept by
//! a DXF save and reopen, so a drawing that uses them no longer needs the file. The files are tiny
//! synthetic ones with our own names.

use cadcraft_doc::geom::{PolyVertex, Vec2};
use cadcraft_doc::library::files::{parse_lin, parse_pat};
use cadcraft_doc::{Common, Drawing, EntityKind, Hatch, HatchLoop, Space};
use cadcraft_io::{read_dxf, write_dxf};

#[test]
fn user_linetypes_and_patterns_survive_dxf() {
    let lin = parse_lin("*MYDASH,My dash\nA,.5,-.25,0,-.25\n*MYTEXT,My text\nA,.5,-.2,[\"MY\",Standard,S=.1,R=15,X=-.1,Y=-.05],-.2\n");
    let pat = parse_pat("*MYSTEPS,My steps\n0,0,0,.25,.25,.25,-.25\n90,.25,0,.25,.25,.25,-.25\n");
    assert!(lin.warnings.is_empty() && pat.warnings.is_empty());
    let mut d = Drawing::new_imperial();
    d.linetypes.extend(lin.items.clone());
    let steps = pat.items.first().unwrap().clone();
    let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].into_iter().map(|(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
    let h = Hatch {
        pattern: steps.name.clone(),
        solid: false,
        loops: vec![HatchLoop { vertices: square, outer: true, sources: Vec::new() }],
        scale: 1.0,
        angle: 0.0,
        associative: false,
        style: 0,
        elevation: 0.0,
        gradient: None,
        origin: Vec2::ZERO,
        background: None,
        pattern_lines: steps.lines.clone(),
    };
    d.add(&Space::Model, Common::default(), EntityKind::Hatch(h)).unwrap();

    let back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    for lt in &lin.items {
        let got = back.linetype(&lt.name).unwrap();
        assert_eq!(got.description, lt.description);
        assert_eq!(got.pattern.len(), lt.pattern.len(), "{}", lt.name);
        for (a, b) in got.pattern.iter().zip(&lt.pattern) {
            assert!((a.length - b.length).abs() < 1e-9 && a.text == b.text && (a.rotation - b.rotation).abs() < 1e-9, "{a:?} vs {b:?}");
        }
    }
    let h = back.model.iter().find_map(|e| if let EntityKind::Hatch(h) = &e.kind { Some(h.clone()) } else { None }).unwrap();
    assert_eq!(h.pattern, "MYSTEPS");
    assert_eq!(h.pattern_lines.len(), 2);
    for (a, b) in h.pattern_lines.iter().zip(&steps.lines) {
        assert!((a.angle - b.angle).abs() < 1e-9 && a.dashes.len() == b.dashes.len(), "{a:?} vs {b:?}");
    }
}
