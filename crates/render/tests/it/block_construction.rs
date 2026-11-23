//! Construction geometry remains editable in the block's model, but is absent from references.

use std::sync::Arc;

use cadcraft_color::Color;
use cadcraft_doc::{
    Block, BlockBoundsCache, Drawing, Entity, EntityKind, Handle, Insert, Line, RawTag, Space, entity_bounds, entity_bounds_cached, extent_bounds,
    extent_bounds_cached,
};
use cadcraft_geom::Vec3;
use cadcraft_render::{Kind, Options, build, build_plot};

/// An extended data tag.
fn tag(code: i32, value: &str) -> RawTag {
    RawTag { code, value: value.into() }
}

/// The extended data that marks an object as construction geometry.
fn marker() -> Vec<RawTag> {
    vec![tag(1001, "ACAD_BCONSTRUCTION"), tag(1070, "1")]
}

/// A red 100 unit line from (`at`, `at`) carrying `xdata`.
fn line(handle: u64, at: f64, xdata: Vec<RawTag>) -> Entity {
    let mut e = Entity::new(Handle(handle), EntityKind::Line(Line { a: Vec3::new(at, at, 0.0), b: Vec3::new(at + 100.0, at, 0.0) }));
    e.common.color = Color::Index(1);
    e.common.xdata = xdata;
    e
}

/// A reference to `block`, placed, scaled and rotated so that a missed transform shows.
fn insert(handle: u64, block: &str) -> Entity {
    Entity::new(
        Handle(handle),
        EntityKind::Insert(Insert {
            block: block.into(),
            insert: Vec3::new(20.0, 30.0, 0.0),
            scale: Vec3::new(2.0, 3.0, 1.0),
            rotation: std::f64::consts::FRAC_PI_2,
            attribs: Vec::new(),
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
            clip: None,
        }),
    )
}

/// A drawing whose model holds a reference to the block "B" (through "Outer" when `nested`). "B" holds a plain line and a far one carrying `xdata`.
fn reference(xdata: Vec<RawTag>, nested: bool) -> Drawing {
    let mut d = Drawing::new_metric();
    let mut b = Block::new("B");
    b.entities.push(line(1, 0.0, Vec::new()));
    b.entities.push(line(2, 10_000.0, xdata));
    d.blocks.insert("B".into(), Arc::new(b));
    if nested {
        let mut outer = Block::new("Outer");
        outer.entities.push(insert(3, "B"));
        d.blocks.insert("Outer".into(), Arc::new(outer));
    }
    d.model.push(insert(100, if nested { "Outer" } else { "B" }));
    d
}

/// A copy of `d` whose block "B" no longer holds its second line.
fn without_guide(d: &Drawing) -> Drawing {
    let mut plain = d.clone();
    Arc::make_mut(plain.blocks.get_mut("B").unwrap()).entities.remove(Handle(2)).unwrap();
    plain
}

/// Construction is gray and dashed when drawn as editable model geometry.
#[test]
fn construction_is_gray_and_dashed_when_drawn_as_editable_model_geometry() {
    let mut plain = Drawing::new_metric();
    plain.model.push(line(1, 0.0, Vec::new()));
    let mut editor = plain.clone();
    editor.block_editor = true;
    editor.model.replace(line(1, 0.0, marker()));
    let normal = build(&plain, &Space::Model, &Options::default());
    let guide = build(&editor, &Space::Model, &Options::default());
    assert_eq!(normal.prims.len(), 1);
    assert!(guide.prims.len() > 1, "the guide is visibly dashed without requiring a DASHED library entry");
    assert!(guide.prims.iter().all(|p| p.kind == Kind::Polyline && p.color != normal.prims[0].color));
    assert!(guide.prims.iter().all(|p| p.color.0 == p.color.1 && p.color.1 == p.color.2), "construction uses gray ink");
    assert_eq!(editor.model.get(Handle(1)).unwrap().common.xdata, marker(), "rendering preserves the authoring marker");
}

/// An editable construction insert draws its children with construction style.
#[test]
fn an_editable_construction_insert_draws_its_children_with_construction_style() {
    let plain = reference(Vec::new(), false);
    let mut editor = plain.clone();
    editor.block_editor = true;
    assert!(editor.model.modify(Handle(100), |e| e.common.xdata = marker()));
    let normal = build(&plain, &Space::Model, &Options::default());
    let guide = build(&editor, &Space::Model, &Options::default());
    assert_eq!(normal.prims.len(), 2);
    assert!(guide.prims.len() > normal.prims.len(), "the editor's nested linework is dashed, not suppressed");
    assert!(guide.prims.iter().all(|p| p.kind == Kind::Polyline && p.color != normal.prims[0].color));
    assert!(guide.prims.iter().all(|p| p.color.0 == p.color.1 && p.color.1 == p.color.2));
    assert_eq!(editor.blocks.get("B").unwrap().entities, plain.blocks.get("B").unwrap().entities, "styling does not alter child data");
}

/// Outside the block editor a marked object is drawn like any other.
#[test]
fn outside_the_block_editor_a_marked_object_is_drawn_like_any_other() {
    // AutoCAD shows the gray dashed style only inside the Block Editor: a drawing that carries the
    // mark on an ordinary object (a saved <Current Drawing>) shows it as it was drawn.
    let mut plain = Drawing::new_metric();
    plain.model.push(line(1, 0.0, Vec::new()));
    let mut marked = plain.clone();
    marked.model.replace(line(1, 0.0, marker()));
    assert!(!marked.block_editor);
    let normal = build(&plain, &Space::Model, &Options::default());
    let shown = build(&marked, &Space::Model, &Options::default());
    assert_eq!(shown.prims, normal.prims, "same colour, linetype and strokes as the unmarked object");
    let plotted = build_plot(&marked, &Space::Model, &Options::default());
    assert_eq!(plotted.prims, build_plot(&plain, &Space::Model, &Options::default()).prims, "and it plots like any other");
    assert_eq!(marked.model.get(Handle(1)).unwrap().common.xdata, marker(), "the mark stays in the data");
}

/// Outside the block editor a marked reference draws its children normally.
#[test]
fn outside_the_block_editor_a_marked_reference_draws_its_children_normally() {
    let plain = reference(Vec::new(), false);
    let mut marked = plain.clone();
    assert!(marked.model.modify(Handle(100), |e| e.common.xdata = marker()));
    assert_eq!(build(&marked, &Space::Model, &Options::default()).prims, build(&plain, &Space::Model, &Options::default()).prims);
}

/// A marked object inside a reference is still omitted outside the block editor.
#[test]
fn a_marked_object_inside_a_reference_is_still_omitted_outside_the_block_editor() {
    // The mark means "not shown in references" wherever the drawing is: only the editor's own
    // styling and locks depend on the editor.
    let d = reference(marker(), false);
    assert!(!d.block_editor);
    assert_eq!(build(&d, &Space::Model, &Options::default()).prims, build(&without_guide(&d), &Space::Model, &Options::default()).prims);
}

/// Direct and nested references omit construction without changing the definition.
#[test]
fn direct_and_nested_references_omit_construction_without_changing_the_definition() {
    for nested in [false, true] {
        let d = reference(marker(), nested);
        let before = d.blocks.get("B").unwrap().entities.clone();
        let actual = build(&d, &Space::Model, &Options::default());
        let expected = build(&without_guide(&d), &Space::Model, &Options::default());
        assert_eq!(actual.prims, expected.prims, "nested={nested}");
        assert_eq!(actual.verts, expected.verts, "nested={nested}");
        assert_eq!(d.blocks.get("B").unwrap().entities, before, "hidden guides remain available to authoring and actions");
    }
}

/// A construction insert suppresses its whole nested subtree.
#[test]
fn a_construction_insert_suppresses_its_whole_nested_subtree() {
    let mut d = reference(Vec::new(), true);
    assert!(Arc::make_mut(d.blocks.get_mut("Outer").unwrap()).entities.modify(Handle(3), |e| e.common.xdata = marker()));
    assert!(build(&d, &Space::Model, &Options::default()).prims.is_empty());
    assert!(build_plot(&d, &Space::Model, &Options::default()).prims.is_empty());
}

/// Construction does not enlarge reference bounds or extents.
#[test]
fn construction_does_not_enlarge_reference_bounds_or_extents() {
    for nested in [false, true] {
        let d = reference(marker(), nested);
        let plain = without_guide(&d);
        let e = d.model.get(Handle(100)).unwrap();
        let p = plain.model.get(Handle(100)).unwrap();
        let expected = entity_bounds(&plain, p, 0);
        assert!(!expected.is_empty());
        assert_eq!(entity_bounds(&d, e, 0), expected, "drawn bounds, nested={nested}");
        assert_eq!(extent_bounds(&d, e), extent_bounds(&plain, p), "zoom extents, nested={nested}");
        let mut cache = BlockBoundsCache::default();
        assert_eq!(entity_bounds_cached(&d, e, 0, &mut cache), expected);
        assert_eq!(extent_bounds_cached(&d, e, &mut cache), extent_bounds(&plain, p));
    }
}

/// A reference with only construction has no rendered or measured geometry.
#[test]
fn a_reference_with_only_construction_has_no_rendered_or_measured_geometry() {
    let mut d = reference(marker(), true);
    Arc::make_mut(d.blocks.get_mut("B").unwrap()).entities.remove(Handle(1)).unwrap();
    let e = d.model.get(Handle(100)).unwrap();
    assert!(build(&d, &Space::Model, &Options::default()).prims.is_empty());
    assert!(entity_bounds(&d, e, 0).is_empty());
    assert!(extent_bounds(&d, e).is_empty());
}

/// Plotted references omit construction at every depth.
#[test]
fn plotted_references_omit_construction_at_every_depth() {
    for nested in [false, true] {
        let d = reference(marker(), nested);
        let actual = build_plot(&d, &Space::Model, &Options::default());
        let expected = build_plot(&without_guide(&d), &Space::Model, &Options::default());
        assert_eq!(actual.prims, expected.prims, "nested={nested}");
        assert_eq!(actual.verts, expected.verts, "nested={nested}");
    }
}

/// Absent zero malformed and other application payloads remain ordinary geometry.
#[test]
fn absent_zero_malformed_and_other_application_payloads_remain_ordinary_geometry() {
    for xdata in [
        Vec::new(),
        vec![tag(1001, "ACAD_BCONSTRUCTION")],
        vec![tag(1001, "ACAD_BCONSTRUCTION"), tag(1070, "0")],
        vec![tag(1001, "ACAD_BCONSTRUCTION"), tag(1070, "bad")],
        vec![tag(1001, "Other"), tag(1070, "1")],
        vec![tag(1001, "ACAD_BCONSTRUCTION"), tag(1001, "Other"), tag(1070, "1")],
        vec![tag(1001, "Other"), tag(1070, "1"), tag(1001, "ACAD_BCONSTRUCTION"), tag(1070, "0")],
    ] {
        let d = reference(xdata.clone(), false);
        assert_eq!(build(&d, &Space::Model, &Options::default()).prims.len(), 2, "{xdata:?}");
        assert_eq!(build_plot(&d, &Space::Model, &Options::default()).prims.len(), 2, "{xdata:?}");
    }
}

/// The construction marker is application scoped and case insensitive.
#[test]
fn the_construction_marker_is_application_scoped_and_case_insensitive() {
    let mut xdata = vec![tag(1001, "Other"), tag(1070, "0"), tag(1001, " acad_bconstruction "), tag(1070, " 1 ")];
    xdata.extend([tag(1001, "After"), tag(1070, "0")]);
    let d = reference(xdata, false);
    assert_eq!(build(&d, &Space::Model, &Options::default()).prims.len(), 1);
}
