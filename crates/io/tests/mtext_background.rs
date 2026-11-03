//! Frame-free text masks are native from R2004; R2000 keeps the property in CADCraft xdata.

use cadcraft_doc::color::{Color, Rgb};
use cadcraft_doc::{Common, Drawing, EntityKind, MText, MTextMask, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version};

fn drawing(background: MTextMask) -> Drawing {
    let mut d = Drawing::new_imperial();
    let text = MText {
        insert: Vec3::new(5., 5., 0.),
        height: 1.,
        width: 0.,
        attach: 5,
        rotation: 0.4,
        style: "Standard".into(),
        contents: "MASK".into(),
        line_spacing: 1.,
        line_spacing_exact: false,
        columns: None,
        background: Some(background),
    };
    d.add(&Space::Model, Common::default(), EntityKind::MText(text)).unwrap();
    d
}

/// Repeated native and legacy saves preserve mask scales, fill colour and frame-free rendering.
#[test]
fn background_and_true_colour_masks_round_trip_in_dxf_and_dwg() {
    for mask in [
        MTextMask { scale: 1.0, ..MTextMask::default() },
        MTextMask { scale: 2.25, ..MTextMask::default() },
        MTextMask { scale: 10.0, ..MTextMask::default() },
        MTextMask { use_background: false, color: Color::True(Rgb(12, 34, 56)), scale: 1.75 },
    ] {
        let d = drawing(mask);
        for version in [Version::R2000, Version::R2004, Version::R2018] {
            for name in ["text.dxf", "text.dwg"] {
                let opts = SaveOptions { version: Some(version), binary: false };
                let mut back = cadcraft_io::read(&cadcraft_io::write_with(&d, name, &opts).unwrap(), name).unwrap();
                for _ in 0..2 {
                    let actual = back
                        .model
                        .iter()
                        .find_map(|e| match &e.kind {
                            EntityKind::MText(t) => t.background,
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(actual, mask, "{version:?}, {name}");
                    let list = cadcraft_render::build(&back, &Space::Model, &cadcraft_render::Options { text: false, ..Default::default() });
                    assert_eq!(list.prims.len(), 1, "a fill without an outline at the default WIPEOUTFRAME=1");
                    assert_eq!(list.prims[0].kind, if mask.use_background { cadcraft_render::Kind::Mask } else { cadcraft_render::Kind::Tris });
                    if !mask.use_background {
                        assert_eq!(list.prims[0].color, Rgb(12, 34, 56));
                    }
                    back = cadcraft_io::read(&cadcraft_io::write_with(&back, name, &opts).unwrap(), name).unwrap();
                }
            }
        }
    }
}

/// Hostile native scales fall back to 1.5; explicit native fill-off overrides stale fallback data.
#[test]
fn invalid_native_mask_values_are_bounded_and_native_off_overrides_legacy_data() {
    let source = drawing(MTextMask::default());
    let opts = SaveOptions { version: Some(Version::R2018), binary: false };
    let bytes = cadcraft_io::write_with(&source, "text.dxf", &opts).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for scale in ["-1", "0.999", "NaN", "inf", "10.001", "1000000", "1e100"] {
        let edited = text.replacen(" 45\r\n1.5\r\n", &format!(" 45\r\n{scale}\r\n"), 1);
        assert_ne!(edited, text);
        let back = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
        let mask = back
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MText(t) => t.background,
                _ => None,
            })
            .unwrap();
        assert!(mask.scale.is_finite() && (1.0..=10.0).contains(&mask.scale));
        assert_eq!(mask.scale, 1.5, "{scale}: invalid scales fall back to the default");
    }
    // A later native editor turning the fill off takes precedence over old fallback xdata.
    let mut legacy = cadcraft_io::write_dxf(&source);
    legacy = legacy.replacen("1001\r\nCADCRAFT\r\n1000\r\nMTEXTMASK", " 90\r\n0\r\n1001\r\nCADCRAFT\r\n1000\r\nMTEXTMASK", 1);
    let back = cadcraft_io::read_dxf(legacy.as_bytes()).unwrap();
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::MText(t) if t.background.is_none())));
}
