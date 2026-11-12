//! MTEXT line spacing style (group 73: 1 at least, 2 exactly) round-trips through DXF (#408).

use cadcraft_doc::{Common, Drawing, EntityKind, MText, Space};
use cadcraft_geom::Vec3;

#[test]
fn line_spacing_style_roundtrips() {
    for exact in [false, true] {
        let mut d = Drawing::new_metric();
        let t = MText {
            insert: Vec3::new(1.0, 2.0, 0.0),
            height: 2.5,
            width: 40.0,
            attach: 5,
            rotation: 0.0,
            style: "Standard".into(),
            contents: "a\\Pb".into(),
            line_spacing: 1.5,
            line_spacing_exact: exact,
            columns: None,
        };
        d.add(&Space::Model, Common::default(), EntityKind::MText(t.clone())).unwrap();
        let bytes = cadcraft_io::write(&d, "a.dxf").unwrap();
        let back = cadcraft_io::read(&bytes, "a.dxf").unwrap();
        let Some(EntityKind::MText(r)) = back.model.iter().map(|e| e.kind.clone()).next() else { panic!("no mtext") };
        assert_eq!(r.line_spacing_exact, exact);
        assert_eq!(r.line_spacing, 1.5);
    }
}
