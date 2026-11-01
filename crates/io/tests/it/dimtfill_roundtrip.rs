//! DIMTFILL (group 69) and DIMTFILLCLR (group 70) survive DXF: as DIMSTYLE groups from R2007,
//! after the record's own flags (also 70) and only when set, and in the style's `CADCRAFT`
//! "DIMVARS" xdata in older files. The `*D` block of a dimension with DIMTFILL 2 holds the background as a SOLID.

use cadcraft_color::Color;
use cadcraft_doc::{Common, DimKind, DimStyle, Dimension, Drawing, EntityKind, Space};
use cadcraft_geom::Vec3;
use cadcraft_io::{SaveOptions, Version};

fn filled() -> DimStyle {
    DimStyle { name: "Fill".into(), text_fill: 2, text_fill_color: Color::Index(3), ..DimStyle::default() }
}

fn drawing() -> Drawing {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(filled());
    d
}

fn dxf(d: &Drawing, v: Version) -> String {
    String::from_utf8(cadcraft_io::write_with(d, "x.dxf", &SaveOptions { version: Some(v), binary: false }).unwrap()).unwrap()
}

/// The DIMSTYLE record named `name`, as trimmed lines.
fn record<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let l: Vec<&str> = text.lines().map(str::trim).collect();
    let at = l.windows(2).position(|w| w == ["2", name]).unwrap();
    let end = l[at..].windows(2).position(|w| w == ["0", "DIMSTYLE"] || w == ["0", "ENDTAB"]).map_or(l.len(), |e| at + e);
    l[at..end].to_vec()
}

#[test]
fn text_fill_roundtrips_in_the_dimstyle_table() {
    let d = drawing();
    for v in [Version::R2007, Version::R2018] {
        let text = dxf(&d, v);
        let rec = record(&text, "Fill");
        let groups: Vec<(&str, &str)> = rec.chunks(2).filter_map(|p| Some((*p.first()?, *p.get(1)?))).collect();
        assert!(groups.contains(&("69", "2")), "{v:?}: DIMTFILL as group 69");
        let seventies: Vec<&str> = groups.iter().filter(|g| g.0 == "70").map(|g| g.1).collect();
        assert_eq!(seventies, ["0", "3"], "{v:?}: the flags, then DIMTFILLCLR");
        assert!(!rec.contains(&"DIMVARS"), "{v:?}: no xdata");
        let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
        assert_eq!(back.dim_style("Fill").unwrap(), &filled(), "{v:?}");
    }
    for v in [Version::R2000, Version::R2004] {
        let text = dxf(&d, v);
        let rec = record(&text, "Fill");
        assert!(!rec.chunks(2).any(|p| p[0] == "69"), "{v:?}: no group 69");
        assert!(rec.windows(4).any(|w| w == ["1070", "69", "1070", "2"]), "{v:?}: DIMTFILL in the xdata");
        assert!(rec.windows(4).any(|w| w == ["1070", "70", "1070", "3"]), "{v:?}: DIMTFILLCLR in the xdata");
        let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
        assert_eq!(back.dim_style("Fill").unwrap(), &filled(), "{v:?}");
    }
}

#[test]
fn unset_text_fill_writes_no_groups() {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(DimStyle { name: "Plain".into(), ..DimStyle::default() });
    d.dim_styles.push(DimStyle { name: "Canvas".into(), text_fill: 1, ..DimStyle::default() });
    let text = dxf(&d, Version::R2018);
    let groups = |name| -> Vec<(String, String)> {
        record(&text, name).chunks(2).filter_map(|p| Some((p.first()?.to_string(), p.get(1)?.to_string()))).collect()
    };
    let plain = groups("Plain");
    // The record's flags are its only 70.
    assert_eq!(plain.iter().filter(|g| g.0 == "70").count(), 1, "{plain:?}");
    assert!(!plain.iter().any(|g| g.0 == "69"), "{plain:?}");
    let canvas = groups("Canvas");
    assert!(canvas.contains(&("69".into(), "1".into())), "{canvas:?}");
    assert_eq!(canvas.iter().filter(|g| g.0 == "70").count(), 1, "the default DIMTFILLCLR is not written: {canvas:?}");
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert_eq!(back.dim_style("Plain").unwrap().text_fill, 0);
    assert_eq!(back.dim_style("Canvas").unwrap().text_fill, 1);
}

#[test]
fn hostile_text_fill_groups_fall_back_to_defaults() {
    let text = dxf(&drawing(), Version::R2018);
    for (fill, color) in [("99", "3"), ("-5", "3"), ("2", "40000"), ("2", "-7")] {
        let edited = text.replacen(" 69\r\n2\r\n 70\r\n3\r\n", &format!(" 69\r\n{fill}\r\n 70\r\n{color}\r\n"), 1);
        assert_ne!(edited, text, "groups replaced");
        let back = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
        let st = back.dim_style("Fill").unwrap();
        assert!(st.text_fill <= 2, "{fill}/{color}: {}", st.text_fill);
    }
}

#[test]
fn record_flags_are_not_read_as_dimtfillclr() {
    let text = dxf(&drawing(), Version::R2018);
    // Flags 64 (referenced) and no DIMTFILLCLR group: the color stays ByBlock.
    let edited =
        text.replacen("  2\r\nFill\r\n 70\r\n0\r\n", "  2\r\nFill\r\n 70\r\n64\r\n", 1).replacen(" 69\r\n2\r\n 70\r\n3\r\n", " 69\r\n2\r\n", 1);
    assert!(edited.contains("  2\r\nFill\r\n 70\r\n64\r\n"), "flags edited");
    assert!(edited.contains(" 69\r\n2\r\n") && !edited.contains(" 69\r\n2\r\n 70\r\n3\r\n"), "DIMTFILLCLR removed");
    let back = cadcraft_io::read_dxf(edited.as_bytes()).unwrap();
    let st = back.dim_style("Fill").unwrap();
    assert_eq!((st.text_fill, st.text_fill_color), (2, Color::ByBlock));
}

#[test]
fn reopened_dimension_keeps_its_background_mask() {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(DimStyle { name: "Fill".into(), text_fill: 1, ..DimStyle::default() });
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { style: "Fill".into(), ..linear() })).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    assert!(back.model.iter().any(|e| matches!(&e.kind, EntityKind::Dimension(dm) if dm.block.is_some())), "read with its block");
    let list = cadcraft_render::build(&back, &Space::Model, &cadcraft_render::Options::default());
    assert_eq!(list.prims.iter().filter(|p| p.kind == cadcraft_render::Kind::Mask).count(), 1);
}

#[test]
fn reopened_color_fill_is_drawn_once() {
    let mut d = drawing();
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { style: "Fill".into(), ..linear() })).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let list = cadcraft_render::build(&back, &Space::Model, &cadcraft_render::Options::default());
    // The block's SOLID in DIMTFILLCLR (color 3, green) is the background; no second one is added.
    let green = list.prims.iter().filter(|p| p.kind == cadcraft_render::Kind::Tris && p.color == cadcraft_color::Rgb(0, 255, 0)).count();
    assert_eq!(green, 1);
}

#[test]
fn reopened_inspection_frame_stays_over_the_background() {
    let mut d = Drawing::new_imperial();
    d.dim_styles.push(DimStyle { name: "Fill".into(), text_fill: 1, ..DimStyle::default() });
    let mut dm = Dimension { style: "Fill".into(), ..linear() };
    dm.marks.inspect = Some(cadcraft_doc::Inspection { shape: 1, label: "A".into(), rate: String::new() });
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let dm = back
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some(dm.clone()),
            _ => None,
        })
        .unwrap();
    assert!(dm.block.is_some(), "read with its block");
    let corners = cadcraft_render::dimension_in(&back, &dm).text_fill;
    let (lo, hi) = corners.iter().fold((corners[0], corners[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let inside = |q: &cadcraft_geom::Vec2| q.x >= lo.x - 1e-6 && q.x <= hi.x + 1e-6 && q.y >= lo.y - 1e-6 && q.y <= hi.y + 1e-6;
    let list = cadcraft_render::build(&back, &Space::Model, &cadcraft_render::Options::default());
    let mask = list.prims.iter().position(|p| p.kind == cadcraft_render::Kind::Mask).unwrap();
    // The separator after the label, the frame's top and bottom: lines inside the background.
    let marks: Vec<usize> = list
        .prims
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == cadcraft_render::Kind::Polyline && list.points(p).len() == 2 && list.points(p).iter().all(inside))
        .map(|(i, _)| i)
        .collect();
    assert!(marks.len() >= 3 && marks.iter().all(|i| *i > mask), "{marks:?} after the mask at {mask}");
}

/// A linear dimension from (0, 0) to (10, 0) with its line at y = 5.
fn linear() -> Dimension {
    Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(0.0, 5.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::ZERO,
        p14: Vec3::new(10.0, 0.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

#[test]
fn dimension_block_holds_the_text_background() {
    let mut d = drawing();
    // An inspection frame (top, bottom and two angular ends: six segments) goes over the background.
    let mut dm = Dimension { style: "Fill".into(), ..linear() };
    dm.marks.inspect = Some(cadcraft_doc::Inspection { shape: 1, label: String::new(), rate: String::new() });
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let name = back
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => dm.block.clone(),
            _ => None,
        })
        .unwrap();
    let ents: Vec<_> = back.block(&name).unwrap().entities.iter().cloned().collect();
    let solid = ents.iter().position(|e| matches!(e.kind, EntityKind::Solid(_)) && e.common.color == Color::Index(3));
    let mtext = ents.iter().position(|e| matches!(e.kind, EntityKind::MText(_)));
    assert!(solid.is_some() && solid < mtext, "the background before the text: {solid:?}, {mtext:?}");
    let lines_after = ents.iter().skip(solid.unwrap_or(0)).filter(|e| matches!(e.kind, EntityKind::Line(_))).count();
    assert_eq!(lines_after, 6, "the frame after the background, the other lines before it");
}

#[test]
fn background_colour_mask_is_native_in_saved_dimension_blocks() {
    let mut d = Drawing::new_imperial();
    d.header.set_i64("WIPEOUTFRAME", 0);
    let mut dm = linear();
    dm.text_rotation = 0.43;
    dm.marks.inspect = Some(cadcraft_doc::Inspection { shape: 1, label: "A".into(), rate: String::new() });
    // Per-object fill and scale override the unfilled Standard style.
    dm.overrides = serde_json::json!({"textFill": 1, "scale": 2.0}).as_object().unwrap().clone();
    let expected = cadcraft_render::dimension_in(&d, &dm).text_fill;
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    for version in [Version::R2000, Version::R2018] {
        for name in ["mask.dxf", "mask.dwg"] {
            let options = SaveOptions { version: Some(version), binary: false };
            let bytes = cadcraft_io::write_with(&d, name, &options).unwrap();
            if name.ends_with(".dxf") {
                let text = std::str::from_utf8(&bytes).unwrap();
                assert!(text.contains("  1\r\nWIPEOUT\r\n"), "generated WIPEOUT needs its CLASS record");
            }
            let mut back = cadcraft_io::read(&bytes, name).unwrap();
            for pass in 0..2 {
                assert_eq!(back.header.i64("WIPEOUTFRAME", 1), 0, "{version:?} {name} save {pass}");
                let block = back
                    .model
                    .iter()
                    .find_map(|e| match &e.kind {
                        EntityKind::Dimension(dm) => dm.block.as_deref().and_then(|name| back.block(name)),
                        _ => None,
                    })
                    .unwrap();
                let masks: Vec<_> = block
                    .entities
                    .iter()
                    .enumerate()
                    .filter_map(|(i, e)| match &e.kind {
                        EntityKind::Wipeout(w) => Some((i, w)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(masks.len(), 1, "{version:?} {name} save {pass}");
                for (actual, expected) in masks[0].1.vertices().iter().zip(&expected) {
                    assert!(actual.dist(*expected) < 1e-8, "{actual:?} != {expected:?}");
                }
                assert_eq!(masks[0].1.vertices().len(), 4);
                let text = block.entities.iter().position(|e| matches!(e.kind, EntityKind::MText(_))).unwrap();
                assert!(masks[0].0 < text);
                assert!(
                    block.entities.iter().skip(masks[0].0 + 1).take(text - masks[0].0 - 1).any(|e| matches!(e.kind, EntityKind::Line(_))),
                    "inspection marks stay above the mask"
                );
                let list = cadcraft_render::build(&back, &Space::Model, &cadcraft_render::Options::default());
                assert_eq!(
                    list.prims.iter().filter(|p| p.kind == cadcraft_render::Kind::Mask).count(),
                    1,
                    "the stored mask must not get another generated background"
                );
                back = cadcraft_io::read(&cadcraft_io::write_with(&back, name, &options).unwrap(), name).unwrap();
            }
        }
    }
}

#[test]
fn dimension_block_alone_masks_underlying_geometry_on_light_and_dark_backgrounds() {
    use cadcraft_render::raster::{RasterOptions, View, render};
    let mut d = Drawing::new_imperial();
    d.header.set_i64("WIPEOUTFRAME", 0);
    d.dim_styles.push(DimStyle { name: "Fill".into(), text_fill: 1, text_gap: 0.5, ..DimStyle::default() });
    // A red rectangle underneath the whole text box makes a missing mask visible.
    d.add(
        &Space::Model,
        Common { color: Color::Index(1), ..Common::default() },
        EntityKind::Solid(cadcraft_doc::Solid { corners: [(0., 3.), (10., 3.), (0., 8.), (10., 8.)].map(|(x, y)| Vec3::new(x, y, 0.)) }),
    )
    .unwrap();
    let dm = Dimension { style: "Fill".into(), text_rotation: 0.3, ..linear() };
    let geometry = cadcraft_render::dimension_in(&d, &dm);
    let center = (geometry.text_fill[0] + geometry.text_fill[2]) * 0.5;
    d.add(&Space::Model, Common::default(), EntityKind::Dimension(dm)).unwrap();
    let mut back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
    let (handle, name) = back
        .model
        .iter()
        .find_map(|e| match &e.kind {
            EntityKind::Dimension(dm) => Some((e.handle, dm.block.clone().unwrap())),
            _ => None,
        })
        .unwrap();
    let block = back.block(&name).unwrap().clone();
    back.remove_entity(handle);
    for part in block.entities.iter() {
        back.add(&Space::Model, part.common.clone(), part.kind.clone()).unwrap();
    }
    // No DIMENSION remains to synthesize a missing background: use only the saved entities.
    let view = View { center, scale: 60., width: 600, height: 240 };
    let options = cadcraft_render::Options { text: false, ..Default::default() };
    for plot in [false, true] {
        let build = if plot { cadcraft_render::build_plot } else { cadcraft_render::build };
        let list = build(&back, &Space::Model, &options);
        assert_eq!(list.prims.iter().filter(|p| p.kind == cadcraft_render::Kind::Mask).count(), 1);
        for background in [cadcraft_color::Rgb(255, 255, 255), cadcraft_color::Rgb(33, 40, 48)] {
            let pixels = render(&list, &view, &RasterOptions { background, antialias: false, ..Default::default() }).unwrap();
            let center_pixel = pixels.pixel(300, 120).unwrap();
            assert_eq!((center_pixel.red(), center_pixel.green(), center_pixel.blue()), (background.0, background.1, background.2));
            let outside = pixels.pixel(60, 20).unwrap();
            assert_eq!((outside.red(), outside.green(), outside.blue()), (255, 0, 0));
            if let Some(dir) = std::env::var_os("CADCRAFT_DIMMASK_SHOTS") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&dir).unwrap();
                let label = if background.0 == 255 { "light" } else { "dark" };
                let preview = build(&back, &Space::Model, &cadcraft_render::Options::default());
                render(&preview, &view, &RasterOptions { background, ..Default::default() })
                    .unwrap()
                    .save_png(dir.join(format!("dimension-mask-{label}-{plot}.png")))
                    .unwrap();
            }
        }
    }
}

#[test]
fn unfilled_or_suppressed_dimension_text_writes_no_mask() {
    for (fill, text) in [(0, ""), (1, " "), (2, " ")] {
        let mut d = Drawing::new_imperial();
        d.dim_styles.push(DimStyle { name: "Fill".into(), text_fill: fill, ..DimStyle::default() });
        d.add(&Space::Model, Common::default(), EntityKind::Dimension(Dimension { style: "Fill".into(), text: text.into(), ..linear() })).unwrap();
        let back = cadcraft_io::read_dxf(cadcraft_io::write_dxf(&d).as_bytes()).unwrap();
        assert!(!back.blocks.values().flat_map(|b| b.entities.iter()).any(|e| matches!(e.kind, EntityKind::Wipeout(_))));
        let list = cadcraft_render::build(&back, &Space::Model, &cadcraft_render::Options::default());
        assert!(!list.prims.iter().any(|p| p.kind == cadcraft_render::Kind::Mask));
    }
}
