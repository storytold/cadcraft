use super::*;

fn frame(list: &DisplayList, look: Look, pointer: Pos2) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0));
    let input = egui::RawInput { screen_rect: Some(rect), events: vec![egui::Event::PointerMoved(pointer)], ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| {
        draw_highlight(ui.painter(), &Xf { rect, center: Vec2::ZERO, scale: 11.0 }, list, &[0], Color32::LIGHT_BLUE, look);
    });
    let shapes = std::mem::take(&mut output.shapes);
    output.textures_delta.clear();
    shapes
}

// Compare visible dash segments, not their submission order or endpoint direction.
fn dash_positions(shapes: &[egui::epaint::ClippedShape]) -> Vec<((u32, u32), (u32, u32))> {
    let mut segments: Vec<_> = shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::LineSegment { points: [a, b], stroke } if stroke.color == Color32::LIGHT_BLUE => {
                let (a, b) = ((a.x.to_bits(), a.y.to_bits()), (b.x.to_bits(), b.y.to_bits()));
                Some(if a < b { (a, b) } else { (b, a) })
            }
            _ => None,
        })
        .collect();
    segments.sort_unstable();
    segments
}

#[test]
fn moving_the_pointer_does_not_move_cpu_fill_highlight_dashes() {
    let p = |x, y| Vec2::new(x, y);
    let mut list = DisplayList { tris: vec![p(0.0, 0.0), p(7.0, 0.0), p(7.0, 3.0), p(0.0, 0.0), p(7.0, 3.0), p(0.0, 3.0)], ..Default::default() };
    list.prims.push(cadcraft_render::DPrim {
        handle: Handle(256),
        color: Rgb(255, 255, 255),
        aci7: true,
        lw: 0.0,
        transparency: 0,
        kind: Kind::Tris,
        start: 0,
        len: 6,
    });
    // Filled glyphs, hatches, solids and arrowheads share the Tris path; wipeouts use Mask.
    for kind in [Kind::Tris, Kind::Mask] {
        list.prims.first_mut().unwrap().kind = kind;
        for width in [1.0, 1.5, 2.0] {
            let look = Look { width, dashed: true };
            let expected = dash_positions(&frame(&list, look, pos2(320.0, 190.0)));
            assert!(!expected.is_empty());
            for i in 0..32 {
                assert_eq!(
                    dash_positions(&frame(&list, look, pos2(320.0 + i as f32, 190.0))),
                    expected,
                    "pointer movement changes only the cursor, not the dashed contour"
                );
            }
        }
    }
    if let Some(dir) = std::env::var_os("CADCRAFT_HIGHLIGHT_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(vec2(600.0, 400.0)).build_ui(|ui| {
            let xf = Xf { rect: ui.max_rect(), center: Vec2::new(3.5, 1.5), scale: 31.0 };
            draw_highlight(ui.painter(), &xf, &list, &[0], Color32::LIGHT_BLUE, Look { width: 2.0, dashed: true });
        });
        h.run_steps(3);
        h.event(egui::Event::PointerMoved(pos2(250.0, 190.0)));
        h.run_steps(2);
        // Harness::render adds a synthetic cursor; omit it from the pixel comparison.
        h.remove_cursor();
        h.run_steps(1);
        let first = h.render().unwrap();
        first.save(dir.join("highlight-pointer-a.png")).unwrap();
        h.event(egui::Event::PointerMoved(pos2(320.0, 190.0)));
        h.run_steps(2);
        h.remove_cursor();
        h.run_steps(1);
        let second = h.render().unwrap();
        second.save(dir.join("highlight-pointer-b.png")).unwrap();
        let changed = first.pixels().zip(second.pixels()).filter(|(a, b)| a != b).count();
        assert_eq!(changed, 0, "moving the pointer cannot change any highlight pixels");
    }
}
