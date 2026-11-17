//! A raster image decoder that panics on a damaged file fails that image only (#956): the
//! drawing it belongs to still opens, and IMAGEATTACH reports an error.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_encode, set_image_decoder, set_io};
use cadcraft_engine::doc::geom::{Vec2, Vec3};
use cadcraft_engine::doc::{Common, Drawing, EntityKind, Image, Line, Space};
use serde_json::json;

fn hooks() {
    set_io(IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
    set_image_decoder(|_| panic!("decoder bug"));
}

#[test]
fn a_panicking_decoder_fails_the_image_not_the_drawing() {
    hooks();
    let dir = std::env::temp_dir().join(format!("cadcraft-image-panic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("map.png"), b"anything").unwrap();
    let mut d = Drawing::new_metric();
    d.add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(5.0, 5.0, 0.0) })).unwrap();
    let i = Image {
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 1.0, 0.0),
        size: Vec2::new(4.0, 2.0),
        path: "map.png".into(),
        name: "map".into(),
        ..Image::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Image(i)).unwrap();
    let host = dir.join("host.dxf");
    std::fs::write(&host, cadcraft_io::write(&d, "host.dxf").unwrap()).unwrap();

    let mut s = Session::new();
    s.execute("open", &json!({ "path": host.to_string_lossy() })).unwrap();
    let doc = s.doc().unwrap();
    assert_eq!(doc.model.len(), 2, "the drawing opened with everything in it");
    assert!(doc.rasters.is_empty());
    assert!(s.log.iter().any(|l| l.contains("map.png") && l.contains("damaged")), "{:?}", s.log);

    let before = s.doc().unwrap().model.len();
    let e = s.execute("imageattach", &json!({ "path": "other.png", "data": base64_encode(b"bytes") })).unwrap_err().to_string();
    assert!(e.contains("damaged"), "{e}");
    assert_eq!(s.doc().unwrap().model.len(), before);
    // The session still works.
    s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), before + 1);
    let _ = std::fs::remove_dir_all(&dir);
}
