//! A window plot builds only the objects that reach the page (#1455): objects far from the
//! window leave the written page unchanged, and objects past the window that still land on the
//! paper are plotted as before.

use cadcraft_doc::{Common, Drawing, EntityKind, Line, Space};
use cadcraft_geom::{Bounds2, Vec2, Vec3};
use cadcraft_io::{PdfOptions, pdf};

fn line(d: &mut Drawing, a: (f64, f64), b: (f64, f64)) {
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })).unwrap();
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.linetypes.extend(cadcraft_doc::library::standard_linetypes());
    line(&mut d, (1.0, 1.0), (9.0, 8.0));
    line(&mut d, (2.0, 9.0), (30.0, 9.5));
    d
}

fn window_pdf(d: &Drawing) -> Vec<u8> {
    let window = Bounds2::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
    pdf(d, &Space::Model, &PdfOptions { window: Some(window), ..PdfOptions::default() }).unwrap()
}

#[test]
fn far_objects_leave_a_window_page_unchanged() {
    let a = drawing();
    let mut far = a.clone();
    for i in 0..1000 {
        let y = 5000.0 + i as f64;
        far.add(
            &Space::Model,
            Common { linetype: "DASHED".into(), ..Default::default() },
            EntityKind::Line(Line { a: Vec3::new(5000.0, y, 0.0), b: Vec3::new(5400.0, y + 3.0, 0.0) }),
        )
        .unwrap();
    }
    assert_eq!(window_pdf(&far), window_pdf(&a));
}

#[test]
fn objects_past_the_window_on_the_paper_still_plot() {
    let a = drawing();
    let mut near = a.clone();
    // Just right of and just above the window: the paper is not square, so one lands on it.
    line(&mut near, (11.0, 5.0), (12.0, 5.0));
    line(&mut near, (5.0, 11.0), (5.0, 12.0));
    assert_ne!(window_pdf(&near), window_pdf(&a));
}

#[test]
fn far_block_contents_leave_a_window_page_unchanged() {
    // A block reference over the window whose block also holds geometry far from it.
    let page = |far: bool| {
        let mut d = drawing();
        let mut blk = cadcraft_doc::Block::new("PLAN");
        let mut add = |a: (f64, f64), b: (f64, f64)| {
            let h = cadcraft_doc::Handle(blk.entities.len() as u64 + 1);
            blk.entities.push(cadcraft_doc::Entity::new(h, EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })));
        };
        add((3.0, 3.0), (6.0, 4.0));
        if far {
            for i in 0..200 {
                let y = 3000.0 + f64::from(i);
                add((3000.0, y), (3400.0, y + 3.0));
            }
        }
        d.blocks.insert("PLAN".into(), std::sync::Arc::new(blk));
        let ins = cadcraft_doc::Insert {
            block: "PLAN".into(),
            insert: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        };
        d.add(&Space::Model, Common::default(), EntityKind::Insert(ins)).unwrap();
        window_pdf(&d)
    };
    assert_eq!(page(true), page(false));
    assert_ne!(page(false), window_pdf(&drawing()), "the near block contents plot");
}
