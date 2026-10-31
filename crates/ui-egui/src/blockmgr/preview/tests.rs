use super::*;
use crate::blockmgr::{
    FrameBuilds, MAX_BUILDS_PER_FRAME, MAX_CACHE_BYTES, MAX_CACHE_RESULTS, PreviewCache, State, ViewMode, draw_block_item, grab_block,
    reference_count, retained_bytes,
};
use crate::{CadApp, Services};
use cadcraft_doc::{Block, Handle, Table, TableCell};
use cadcraft_engine::{Input, Session};
use cadcraft_geom::{PolyVertex, Vec3};
use egui::vec2;
use serde_json::json;
use std::collections::HashSet;

fn drawing(kind: EntityKind, count: usize) -> Drawing {
    let mut d = Drawing::default();
    let mut block = Block::new("Synthetic");
    for i in 0..count {
        block.entities.push(Entity::new(Handle(i as u64 + 1), kind.clone()));
    }
    d.blocks.insert(block.name.clone(), Arc::new(block));
    d
}
fn simple() -> Spline {
    Spline::from_control(vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(2.0, 1.0)], 3)
}
fn line() -> EntityKind {
    EntityKind::Line(cadcraft_doc::Line { a: Vec3::ZERO, b: Vec3::new(2.0, 1.0, 0.0) })
}
fn insert(name: &str) -> Insert {
    serde_json::from_value(json!({"block":name,"insert":[0,0,0],"scale":[1,1,1],"attribs":[]})).unwrap()
}
fn assert_failure(d: &Drawing, reason: PreviewFailure) {
    assert!(matches!(build(d, "Synthetic"), PreviewResult::Unavailable(r) if r == reason), "expected {reason:?}");
}

#[test]
fn hundreds_of_simple_splines_preview_but_fewer_expensive_splines_are_bounded() {
    let d = drawing(EntityKind::Spline(simple()), 500);
    assert!(matches!(build(&d, "Synthetic"), PreviewResult::Ready(_)));
    let expensive = Spline::from_control((0..65).map(|i| Vec2::new(i as f64, (i % 2) as f64 * 10.0)).collect(), 32);
    assert_failure(&drawing(EntityKind::Spline(expensive), 10), PreviewFailure::Computation);
}

#[test]
fn curves_and_wide_polylines_use_thumbnail_tolerance() {
    for (count, width) in [(128, 0.0), (16, 2.0)] {
        let p = cadcraft_doc::LwPolyline {
            vertices: (0..count).map(|i| PolyVertex { bulge: 1.0, ..PolyVertex::new(Vec2::new(i as f64, 0.0)) }).collect(),
            closed: false,
            const_width: width,
            elevation: 0.0,
            plinegen: false,
        };
        let d = drawing(EntityKind::LwPolyline(p), 1);
        let PreviewResult::Ready(p) = build(&d, "Synthetic") else {
            panic!("ordinary curved polyline refused");
        };
        assert!(p.list.verts.len() + p.list.tris.len() < MAX_VERTICES);
        if width > 0.0 {
            assert!(!p.list.tris.is_empty());
        }
    }
}

#[test]
fn hostile_text_mtext_attributes_tables_and_metadata_fail_before_bounds() {
    let text: cadcraft_doc::Text = serde_json::from_value(json!({"insert":[0,0,0],"height":1,"value":"x".repeat(MAX_INPUT+1)})).unwrap();
    assert_failure(&drawing(EntityKind::Text(text.clone()), 1), PreviewFailure::Input);
    let mut m: cadcraft_doc::MText = serde_json::from_value(json!({"insert":[0,0,0],"height":1,"contents":text.value})).unwrap();
    assert_failure(&drawing(EntityKind::MText(m.clone()), 1), PreviewFailure::Input);
    m.contents.clear();
    m.columns = Some(cadcraft_doc::MTextColumns { heights: vec![1.0; MAX_INPUT + 1], ..Default::default() });
    assert_failure(&drawing(EntityKind::MText(m), 1), PreviewFailure::Input);
    let mut ins = insert("Missing");
    ins.attribs.push(serde_json::from_value(json!({"tag":"A","text":text})).unwrap());
    assert_failure(&drawing(EntityKind::Insert(ins), 1), PreviewFailure::Input);
    let mut t: Table =
        serde_json::from_value(json!({"insert":[0,0,0],"rowHeights":[1],"colWidths":[1],"cells":[[{"text":"x".repeat(MAX_INPUT+1)}]]})).unwrap();
    assert_failure(&drawing(EntityKind::Table(t.clone()), 1), PreviewFailure::Input);
    t.cells.clear();
    t.row_heights = vec![1.0; 200];
    t.col_widths = vec![1.0; 200];
    assert_failure(&drawing(EntityKind::Table(t), 1), PreviewFailure::Input);
    let mut d = drawing(line(), 1);
    let b = Arc::make_mut(d.blocks.get_mut("Synthetic").unwrap());
    let e = b.entities.iter().next().unwrap().handle;
    // Common data is cloned by bounds and nested references too.
    let mut huge = Entity::new(e, line());
    huge.common.xdata.push(cadcraft_doc::RawTag { code: 1000, value: "x".repeat(MAX_INPUT + 1) });
    b.entities = Default::default();
    b.entities.push(huge);
    assert_failure(&d, PreviewFailure::Input);
}

#[test]
fn normal_table_is_visible_and_merge_work_is_bounded() {
    let mut t: Table = serde_json::from_value(json!({"insert":[0,0,0],"rowHeights":[1],"colWidths":[1],"cells":[[{"text":"OK"}]]})).unwrap();
    assert!(matches!(build(&drawing(EntityKind::Table(t.clone()), 1), "Synthetic"), PreviewResult::Ready(_)));
    t.row_heights = vec![1.0; 100];
    t.col_widths = vec![1.0; 100];
    t.cells = vec![vec![TableCell { merged: Some((100, 100)), ..Default::default() }; 100]; 100];
    assert_failure(&drawing(EntityKind::Table(t), 1), PreviewFailure::Computation);
}

#[test]
fn excessive_geometry_hatches_and_mleaders_remain_bounded() {
    assert_failure(
        &drawing(EntityKind::Polyline3d(cadcraft_doc::Polyline3d { points: vec![Vec3::ZERO; MAX_INPUT + 1], closed: false }), 1),
        PreviewFailure::Input,
    );
    let h: cadcraft_doc::Hatch = serde_json::from_value(json!({"pattern":"SOLID","solid":true,"loops":[]})).unwrap();
    let mut h = h;
    h.loops.push(cadcraft_doc::HatchLoop { vertices: vec![PolyVertex::new(Vec2::ZERO); MAX_INPUT + 1], outer: true, sources: vec![] });
    assert_failure(&drawing(EntityKind::Hatch(h), 1), PreviewFailure::Input);
    let m = cadcraft_doc::MLeader {
        leaders: vec![vec![Vec3::ZERO; MAX_INPUT + 1]],
        landing: Vec3::ZERO,
        dogleg: 0.0,
        text: None,
        style: "STANDARD".into(),
        arrow_size: 1.0,
        spline: false,
        block: None,
    };
    assert_failure(&drawing(EntityKind::MLeader(m), 1), PreviewFailure::Input);
    assert_failure(&drawing(EntityKind::Wipeout(cadcraft_doc::Wipeout { boundary: vec![Vec2::ZERO; MAX_INPUT + 1] }), 1), PreviewFailure::Input);
}

#[test]
fn exhausted_fill_work_is_a_refusal_instead_of_a_partial_preview() {
    let mut h: cadcraft_doc::Hatch = serde_json::from_value(json!({"pattern":"SOLID","solid":true,"loops":[]})).unwrap();
    let vertices =
        (0..400).map(|i| PolyVertex::new(Vec2::new(i as f64, if i % 2 == 0 { i as f64 / 400.0 } else { 1.0 - i as f64 / 400.0 }))).collect();
    h.loops.push(cadcraft_doc::HatchLoop { vertices, outer: true, sources: vec![] });
    assert_failure(&drawing(EntityKind::Hatch(h), 1), PreviewFailure::Computation);
}

#[test]
fn cycles_deep_references_missing_references_and_arrays_are_explicit() {
    assert_failure(&drawing(EntityKind::Insert(insert("Synthetic")), 1), PreviewFailure::Computation);
    assert_failure(&drawing(EntityKind::Insert(insert("Missing")), 1), PreviewFailure::MissingReference);
    let mut d = drawing(line(), 1);
    for i in (0..=cadcraft_doc::MAX_BLOCK_DEPTH).rev() {
        let mut b = Block::new(&format!("Depth{i}"));
        let next = if i == cadcraft_doc::MAX_BLOCK_DEPTH { "Synthetic".into() } else { format!("Depth{}", i + 1) };
        b.entities.push(Entity::new(Handle(1), EntityKind::Insert(insert(&next))));
        d.blocks.insert(b.name.clone(), Arc::new(b));
    }
    assert!(matches!(build(&d, "Depth0"), PreviewResult::Unavailable(PreviewFailure::Computation)));
    let mut i = insert("Synthetic");
    i.rows = 200;
    i.cols = 200;
    let mut b = Block::new("Array");
    b.entities.push(Entity::new(Handle(2), EntityKind::Insert(i)));
    d.blocks.insert("Array".into(), Arc::new(b));
    assert!(matches!(build(&d, "Array"), PreviewResult::Unavailable(PreviewFailure::Computation)));
}

#[test]
fn empty_ready_invalid_and_refused_have_distinct_statuses() {
    assert!(matches!(build(&drawing(line(), 0), "Synthetic"), PreviewResult::Empty));
    assert!(matches!(build(&drawing(line(), 1), "Synthetic"), PreviewResult::Ready(_)));
    let mut s = simple();
    s.knots[0] = f64::NAN;
    assert_failure(&drawing(EntityKind::Spline(s), 1), PreviewFailure::Invalid);
    assert_ne!(
        super::super::preview_status(Some(&PreviewResult::Empty), true),
        super::super::preview_status(Some(&PreviewResult::Unavailable(PreviewFailure::Input)), false)
    );
    assert_ne!(super::super::preview_status(None, false), super::super::preview_status(Some(&PreviewResult::Empty), false));
}

#[test]
fn renderer_refuses_output_before_retaining_excess_geometry() {
    let d = drawing(line(), 2);
    let ents = || d.block("Synthetic").unwrap().entities.iter().map(|e| &**e);
    assert!(cadcraft_render::build_entities_limited(&d, ents(), &Default::default(), 1, 100).is_none());
    assert!(cadcraft_render::build_entities_limited(&d, ents(), &Default::default(), 100, 3).is_none());
    assert_eq!(cadcraft_render::build_entities_limited(&d, ents(), &Default::default(), 2, 4).unwrap().verts.len(), 4);
}

#[test]
fn cache_caps_prefer_offscreen_lru_and_count_empty_results() {
    let mut cache = PreviewCache::default();
    let visible = HashSet::from(["0".to_string()]);
    for i in 0..=MAX_CACHE_RESULTS {
        cache.insert(i.to_string(), PreviewResult::Empty, &visible);
    }
    assert_eq!(cache.len(), MAX_CACHE_RESULTS);
    assert!(cache.entries.contains_key("0"));
    assert!(!cache.entries.contains_key("1"));
    cache.touch("2");
    cache.insert("next".into(), PreviewResult::Unavailable(PreviewFailure::Input), &visible);
    assert!(!cache.entries.contains_key("3"));
    assert!(cache.entries.contains_key("2"));
    for entry in cache.entries.values_mut() {
        entry.bytes = MAX_CACHE_BYTES / 3;
    }
    cache.evict(&visible);
    assert!(cache.len() <= 2);
    assert!(cache.estimated_bytes().unwrap() <= MAX_CACHE_BYTES);
    cache.sync_revision(1, 1);
    assert!(cache.is_empty());
    cache.insert("refused".into(), PreviewResult::Unavailable(PreviewFailure::Input), &HashSet::new());
    cache.sync_revision(2, 1);
    assert!(cache.is_empty());
}

#[test]
fn capacity_is_accounted_even_when_vectors_are_empty() {
    let list = cadcraft_render::DisplayList { verts: Vec::with_capacity(10_000), ..Default::default() };
    let result = PreviewResult::Ready(Preview { list: Arc::new(list), bounds: Bounds2::EMPTY });
    assert!(retained_bytes(&result).unwrap() >= 10_000 * std::mem::size_of::<Vec2>());
    let mut cache = PreviewCache::default();
    cache.insert("Huge".into(), result, &HashSet::new());
    cache.entries.get_mut("Huge").unwrap().bytes = usize::MAX;
    cache.evict(&HashSet::new());
    assert!(cache.is_empty());

    // A result which cannot fit is replaced by a lightweight cached refusal.
    let list = cadcraft_render::DisplayList { verts: Vec::with_capacity(MAX_CACHE_BYTES / std::mem::size_of::<Vec2>()), ..Default::default() };
    cache.insert("Too large".into(), PreviewResult::Ready(Preview { list: Arc::new(list), bounds: Bounds2::EMPTY }), &HashSet::new());
    assert!(matches!(cache.entries.get("Too large").unwrap().result, PreviewResult::Unavailable(PreviewFailure::Output)));
    assert!(cache.estimated_bytes().unwrap() < 4096);
}

#[test]
fn visibility_cannot_override_cache_limits() {
    let visible: HashSet<_> = (0..600).map(|i| i.to_string()).collect();
    let mut cache = PreviewCache::default();
    for name in &visible {
        cache.insert(name.clone(), PreviewResult::Empty, &visible);
    }
    assert_eq!(cache.len(), MAX_CACHE_RESULTS);
    assert!(cache.estimated_bytes().unwrap() <= MAX_CACHE_BYTES);
}

#[test]
fn refused_thumbnail_is_not_rebuilt_each_frame_and_does_not_block_insertion() {
    let text: cadcraft_doc::Text = serde_json::from_value(json!({"insert":[0,0,0],"height":1,"value":"x".repeat(MAX_INPUT+1)})).unwrap();
    let d = drawing(EntityKind::Text(text), 1);
    assert_failure(&d, PreviewFailure::Input);
    let ctx = egui::Context::default();
    let mut warm = ctx.run_ui(Default::default(), |_| {});
    warm.textures_delta.clear();
    let mut cache = PreviewCache::default();
    for pass in 0..4 {
        let mut frame = FrameBuilds { visible: HashSet::new(), builds_left: MAX_BUILDS_PER_FRAME, started: web_time::Instant::now() };
        let mut out = ctx.run_ui(Default::default(), |ui| {
            frame.started = web_time::Instant::now();
            draw_block_item(ui, &d, "Synthetic", false, ViewMode::LargeIcons, vec2(100.0, 112.0), (&mut cache, &mut frame));
        });
        out.textures_delta.clear();
        assert_eq!(frame.builds_left, if pass == 0 { MAX_BUILDS_PER_FRAME - 1 } else { MAX_BUILDS_PER_FRAME });
    }
    let mut s = Session::empty();
    s.open_drawing(d, "Synthetic", None);
    let mut app = CadApp::new(s, Services::default());
    grab_block(&mut app, "Synthetic", &State::default());
    assert!(app.session.current_prompt().unwrap().accept.point);
    app.session.input(Input::Point(Vec2::new(10.0, 20.0))).unwrap();
    assert_eq!(reference_count(app.session.doc().unwrap(), "Synthetic"), 1);
}
