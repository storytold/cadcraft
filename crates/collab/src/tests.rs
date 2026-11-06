use cadcraft_doc::color::Color;
use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Handle, Layout, Point, Space};

use crate::{Anchor, Change, CollabError, apply, diff};

fn point(x: f64) -> EntityKind {
    EntityKind::Point(Point { p: Vec3::new(x, 0.0, 0.0), angle: 0.0 })
}

fn add(drawing: &mut Drawing, space: &Space, x: f64) -> Handle {
    drawing.add(space, Common::default(), point(x)).unwrap()
}

/// `before` with three points, `after` with one edited, one created between the others and one
/// deleted.
fn sample() -> (Drawing, Drawing) {
    let mut before = Drawing::new_metric();
    let a = add(&mut before, &Space::Model, 1.0);
    let b = add(&mut before, &Space::Model, 2.0);
    let _c = add(&mut before, &Space::Model, 3.0);
    let mut after = before.clone();
    after.modify_entity(a, |e| e.common.color = Color::Index(4)).unwrap();
    let d = add(&mut after, &Space::Model, 4.0);
    assert!(after.model.move_next_to(d, b, true));
    after.remove_entity(b).unwrap();
    (before, after)
}

#[test]
fn diff_and_apply_round_trip() {
    let (before, after) = sample();
    let changes = diff(&before, &after, &Space::Model).unwrap();
    assert!(!changes.is_empty());
    let mut applied = before.clone();
    apply(&mut applied, &changes).unwrap();
    assert_eq!(applied.space(&Space::Model), after.space(&Space::Model));
    assert_eq!(applied.entity_count(), after.entity_count());
}

#[test]
fn identical_spaces_have_no_changes() {
    let (before, _) = sample();
    assert!(diff(&before, &before.clone(), &Space::Model).unwrap().is_empty());
}

#[test]
fn a_change_set_round_trips_through_json() {
    let (before, after) = sample();
    let changes = diff(&before, &after, &Space::Model).unwrap();
    let json = serde_json::to_string(&changes).unwrap();
    let decoded: Vec<Change> = serde_json::from_str(&json).unwrap();
    assert_eq!(changes, decoded);
    let mut applied = before.clone();
    apply(&mut applied, &decoded).unwrap();
    assert_eq!(applied.space(&Space::Model), after.space(&Space::Model));
}

#[test]
fn a_new_draw_order_is_a_move_not_a_rebuild() {
    let mut before = Drawing::new_metric();
    let a = add(&mut before, &Space::Model, 1.0);
    let b = add(&mut before, &Space::Model, 2.0);
    let _c = add(&mut before, &Space::Model, 3.0);
    let mut after = before.clone();
    assert!(after.model.bring_to_front(a));
    let changes = diff(&before, &after, &Space::Model).unwrap();
    assert!(!changes.is_empty());
    assert!(changes.iter().all(|c| matches!(c, Change::EntityOrder { .. })), "a move must not be a rebuild: {changes:?}");
    let mut applied = before.clone();
    apply(&mut applied, &changes).unwrap();
    assert_eq!(applied.space(&Space::Model), after.space(&Space::Model));
    assert_eq!(applied.model.last().map(|e| e.handle), Some(a));
    assert_eq!(applied.model.iter().next().map(|e| e.handle), Some(b));
}

#[test]
fn deleting_an_entity_does_not_move_the_others() {
    let mut before = Drawing::new_metric();
    let _a = add(&mut before, &Space::Model, 1.0);
    let b = add(&mut before, &Space::Model, 2.0);
    let _c = add(&mut before, &Space::Model, 3.0);
    let mut after = before.clone();
    after.remove_entity(b).unwrap();
    let changes = diff(&before, &after, &Space::Model).unwrap();
    assert_eq!(changes, vec![Change::EntityDelete { handle: b }]);
}

#[test]
fn paper_space_changes_are_separate_from_model_space() {
    let mut before = Drawing::new_metric();
    before.layouts.push(Layout::new("Sheet1", 1));
    let sheet = Space::Paper("Sheet1".into());
    let a = add(&mut before, &sheet, 1.0);
    let mut after = before.clone();
    after.modify_entity(a, |e| e.common.color = Color::Index(5)).unwrap();
    let b = add(&mut after, &sheet, 2.0);

    assert!(diff(&before, &after, &Space::Model).unwrap().is_empty());
    let changes = diff(&before, &after, &sheet).unwrap();
    assert_eq!(changes.len(), 2);
    match changes.iter().find(|c| matches!(c, Change::EntityCreate { .. })) {
        Some(Change::EntityCreate { space, handle, .. }) => {
            assert_eq!(space, &sheet);
            assert_eq!(*handle, b);
        }
        other => panic!("expected a creation in the sheet, got {other:?}"),
    }
    let mut applied = before.clone();
    apply(&mut applied, &changes).unwrap();
    assert_eq!(applied.space(&sheet), after.space(&sheet));
}

#[test]
fn an_edit_across_a_chunk_boundary_is_one_replace() {
    let mut before = Drawing::new_metric();
    let handles: Vec<Handle> = (0..3000).map(|i| add(&mut before, &Space::Model, f64::from(i))).collect();
    let middle = handles[1500];
    let mut after = before.clone();
    after.modify_entity(middle, |e| e.common.color = Color::Index(2)).unwrap();
    let changes = diff(&before, &after, &Space::Model).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(matches!(changes[0], Change::EntityReplace { handle, .. } if handle == middle));
    let mut applied = before.clone();
    apply(&mut applied, &changes).unwrap();
    assert_eq!(applied.space(&Space::Model), after.space(&Space::Model));
}

#[test]
fn applying_a_creation_never_reuses_its_handle() {
    let (before, after) = sample();
    let changes = diff(&before, &after, &Space::Model).unwrap();
    let mut applied = before.clone();
    apply(&mut applied, &changes).unwrap();
    let fresh = add(&mut applied, &Space::Model, 9.0);
    assert!(after.entity(fresh).is_none());
    assert!(applied.entity(fresh).is_some());
}

#[test]
fn a_duplicate_handle_is_refused() {
    let mut drawing = Drawing::new_metric();
    let h = add(&mut drawing, &Space::Model, 1.0);
    let entity = drawing.entity(h).unwrap().as_ref().clone();
    let change = Change::EntityCreate { space: Space::Model, handle: h, order: Anchor::Top, entity };
    assert_eq!(apply(&mut drawing, &[change]), Err(CollabError::Duplicate(h)));
}

#[test]
fn a_missing_anchor_is_refused() {
    let mut drawing = Drawing::new_metric();
    let h = add(&mut drawing, &Space::Model, 1.0);
    let change = Change::EntityOrder { handle: h, order: Anchor::After { handle: Handle(999) } };
    assert_eq!(apply(&mut drawing, &[change]), Err(CollabError::NoAnchor(Handle(999))));
}

#[test]
fn an_unknown_layer_is_refused() {
    let mut drawing = Drawing::new_metric();
    let h = Handle(drawing.handseed);
    let mut entity = Entity::new(h, point(1.0));
    entity.common.layer = "NOPE".into();
    let change = Change::EntityCreate { space: Space::Model, handle: h, order: Anchor::Top, entity };
    assert_eq!(apply(&mut drawing, &[change]), Err(CollabError::NoLayer("NOPE".into())));
}

#[test]
fn an_unknown_space_is_refused() {
    let mut drawing = Drawing::new_metric();
    let sheet = Space::Paper("Missing".into());
    assert_eq!(diff(&drawing, &drawing.clone(), &sheet), Err(CollabError::NoSpace(sheet.clone())));
    assert_eq!(apply(&mut drawing, &[]), Ok(()));
}
