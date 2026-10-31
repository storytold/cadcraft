//! Opt-in measurement, never a required local fixture.
use super::*;
#[test]
#[ignore = "set CADCRAFT_LIBRARY and CADCRAFT_AUDIT_DIR for a local release audit"]
fn local_library_audit() {
    let path = std::env::var("CADCRAFT_LIBRARY").expect("CADCRAFT_LIBRARY");
    let out = std::path::PathBuf::from(std::env::var("CADCRAFT_AUDIT_DIR").expect("CADCRAFT_AUDIT_DIR"));
    std::fs::create_dir_all(&out).unwrap();
    let d = cadcraft_io::read(&std::fs::read(&path).unwrap(), &path).unwrap();
    let mut cache = PreviewCache::default();
    let mut csv = String::from("name;entities;status;build_us;primitives;vertices\n");
    for name in names(&d) {
        let start = std::time::Instant::now();
        let result = build(&d, &name);
        let elapsed = start.elapsed().as_micros();
        let (list, bounds) = match &result {
            PreviewResult::Ready(p) => ((*p.list).clone(), p.bounds),
            _ => (cadcraft_render::DisplayList::default(), Bounds2::EMPTY),
        };
        let status = if d.block(&name).unwrap().entities.is_empty() {
            "empty_definition"
        } else {
            match &result {
                PreviewResult::Ready(_) => "ready",
                PreviewResult::Empty => "no_visible_content",
                PreviewResult::Unavailable(reason) => reason.message(),
            }
        };
        csv.push_str(&format!(
            "{name};{};{status};{elapsed};{};{}\n",
            d.block(&name).unwrap().entities.len(),
            list.prims.len(),
            list.verts.len() + list.tris.len()
        ));
        if ["GMAT_PAC_VIESSMANN_PURPLE", "DIV_01_LEGENDE_ACCESSOIRES", "TUY_DEPART_DIRECT_POMPE_DOUBLE_ENERGIE"].contains(&name.as_str()) {
            let view = cadcraft_render::raster::View::fit(&bounds, 512, 512, 0.1);
            cadcraft_render::raster::render(&list, &view, &Default::default()).unwrap().save_png(out.join(format!("{name}.png"))).unwrap();
        }
        cache.insert(name, result, &HashSet::new());
    }
    std::fs::write(out.join("library.csv"), csv).unwrap();
    let initial_bytes = cache.estimated_bytes().unwrap();
    // Exercise the real tile path in both scroll directions, twelve visible tiles per frame.
    let ctx = egui::Context::default();
    let all = names(&d);
    let mut rebuilds = 0;
    let mut paint_times = Vec::new();
    for page in all.chunks(12).chain(all.chunks(12).rev()) {
        let mut frame = FrameBuilds { visible: HashSet::new(), builds_left: MAX_BUILDS_PER_FRAME, started: web_time::Instant::now() };
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))), ..Default::default() };
        let start = std::time::Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            for row in page.chunks(4) {
                ui.horizontal(|ui| {
                    for name in row {
                        draw_block_item(ui, &d, name, false, ViewMode::LargeIcons, vec2(150.0, 112.0), (&mut cache, &mut frame));
                    }
                });
            }
        });
        let _meshes = ctx.tessellate(output.shapes, output.pixels_per_point);
        paint_times.push(start.elapsed().as_micros());
        output.textures_delta.clear();
        rebuilds += MAX_BUILDS_PER_FRAME - frame.builds_left;
        cache.evict(&frame.visible);
    }
    paint_times.sort_unstable();
    std::fs::write(
        out.join("cache.txt"),
        format!(
            "initial_results={}\nestimated_bytes={}\nscroll_rebuilds={}\ncached_frame_p50_us={}\ncached_frame_max_us={}\n",
            cache.len(),
            initial_bytes,
            rebuilds,
            paint_times[paint_times.len() / 2],
            paint_times.last().unwrap()
        ),
    )
    .unwrap();
    let mut warm = String::from("name;build_us\n");
    for name in &all {
        let start = std::time::Instant::now();
        let _result = build(&d, name);
        warm.push_str(&format!("{name};{}\n", start.elapsed().as_micros()));
    }
    std::fs::write(out.join("warm.csv"), warm).unwrap();

    // Render actual palette widgets, including names, clipping, theme and status text.
    let mut session = cadcraft_engine::Session::empty();
    session.open_drawing(d.clone(), "Local audit", None);
    let app = CadApp::new(session, crate::Services::default());
    let mut state = State { filter: "PAC".into(), view_mode: ViewMode::LargeIcons, ..Default::default() };
    let v = view(&app, &mut state, &mut cache).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 760.0)).wgpu().build_ui_state(
        |ui, data: &mut (State, PreviewCache)| {
            crate::theme::apply(ui.ctx(), egui::Theme::Dark, false);
            let mut action = None;
            body(ui, &d, &v, &mut data.0, &mut data.1, false, &mut action);
        },
        (state, cache),
    );
    harness.run_steps(8);
    harness.render().unwrap().save(out.join("palette.png")).unwrap();

    let mut status_drawing = Drawing::default();
    let mut status_cache = PreviewCache::default();
    for name in ["Ready", "Empty", "Invisible", "Refused", "Pending"] {
        let mut b = cadcraft_doc::Block::new(name);
        if name != "Empty" {
            let mut e = cadcraft_doc::Entity::new(
                cadcraft_doc::Handle(1),
                EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::new(2.0, 1.0, 0.0) }),
            );
            e.common.visible = name != "Invisible";
            b.entities.push(e);
        }
        status_drawing.blocks.insert(name.into(), Arc::new(b));
        if name != "Pending" {
            let result = if name == "Refused" { PreviewResult::Unavailable(PreviewFailure::Input) } else { build(&status_drawing, name) };
            status_cache.insert(name.into(), result, &HashSet::new());
        }
    }
    let mut statuses = egui_kittest::Harness::builder().with_size(vec2(1000.0, 200.0)).wgpu().build_ui_state(
        |ui, cache: &mut PreviewCache| {
            crate::i18n::with_language("fr", || {
                crate::theme::apply(ui.ctx(), egui::Theme::Dark, false);
                let mut frame = FrameBuilds { visible: HashSet::new(), builds_left: 0, started: web_time::Instant::now() };
                ui.horizontal(|ui| {
                    for name in ["Ready", "Empty", "Invisible", "Refused", "Pending"] {
                        draw_block_item(ui, &status_drawing, name, false, ViewMode::LargeIcons, vec2(180.0, 112.0), (cache, &mut frame));
                    }
                });
            })
        },
        status_cache,
    );
    statuses.run_steps(2);
    statuses.render().unwrap().save(out.join("statuses.png")).unwrap();
}
