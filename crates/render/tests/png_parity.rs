//! The shared PNG writer preserves tiny-skia's straight-alpha export pixels.
#[test]
fn shared_png_matches_tiny_skia_for_premultiplied_pixels() {
    let pixmap =
        tiny_skia::Pixmap::from_vec(vec![0, 0, 0, 0, 32, 64, 96, 128, 1, 2, 3, 4, 255, 63, 9, 255], tiny_skia::IntSize::from_wh(4, 1).unwrap())
            .unwrap();
    let expected = tiny_skia::Pixmap::decode_png(&pixmap.encode_png().unwrap()).unwrap();
    let actual = tiny_skia::Pixmap::decode_png(&cadcraft_render::raster::encode_png(&pixmap).unwrap()).unwrap();
    assert_eq!(actual.data(), expected.data());
}
