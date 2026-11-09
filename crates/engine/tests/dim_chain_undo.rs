//! A command-local Undo must keep the original dimension available as the chain's seed.
use cadcraft_doc::{Dimension, EntityKind, Handle};
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use serde_json::json;

fn dimension(s: &Session, h: Handle) -> Dimension {
    match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        other => panic!("expected dimension, got {other:?}"),
    }
}

fn seed(s: &mut Session) -> Handle {
    s.execute("dimlinear", &json!({"p1": [0, 0], "p2": [10, 0], "at": [5, 4]})).unwrap();
    s.last_dim.unwrap()
}

fn check_undo(cmd: &str) {
    let mut s = Session::new();
    let original = seed(&mut s);
    let original_dim = dimension(&s, original);
    s.cmdline(cmd).unwrap();
    // Undo before adding anything must leave the seed alone.
    s.cmdline("u").unwrap();
    assert_eq!(s.last_dim, Some(original));
    s.cmdline("20,0").unwrap();
    let first = s.last_dim.unwrap();
    s.cmdline("30,0").unwrap();
    let second = s.last_dim.unwrap();
    // Removing a later link restores the preceding link; removing the first restores the seed.
    s.cmdline("u").unwrap();
    assert!(s.doc().unwrap().entity(second).is_none());
    assert_eq!(s.last_dim, Some(first));
    s.cmdline("u").unwrap();
    assert!(s.doc().unwrap().entity(first).is_none());
    assert_eq!(s.last_dim, Some(original), "{cmd}: Undo must retain the original dimension");
    s.cmdline("u").unwrap();
    assert_eq!(s.last_dim, Some(original));
    assert_eq!(s.doc().unwrap().model.iter().count(), 1);
    assert!(s.running.is_some());
    assert_eq!(dimension(&s, original), original_dim);
    let origin = if cmd == "dimcontinue" { Vec2::new(10.0, 0.0) } else { Vec2::ZERO };
    assert_eq!(s.current_prompt().unwrap().base, Some(origin));
    let preview = s.preview(Vec2::new(25.0, 0.0));
    let preview_dim = match &preview.first().unwrap().kind {
        EntityKind::Dimension(d) => d.clone(),
        other => panic!("expected dimension preview, got {other:?}"),
    };
    assert_eq!(preview_dim.p13.xy(), origin);
    s.cmdline("25,0").unwrap();
    let replacement = s.last_dim.unwrap();
    let replacement_dim = dimension(&s, replacement);
    assert_eq!(replacement_dim.p13, preview_dim.p13);
    assert_eq!(replacement_dim.p14, preview_dim.p14);
    assert_eq!(replacement_dim.defpt, preview_dim.defpt);
    let expected_y = if cmd == "dimcontinue" { 4.0 } else { 4.38 };
    assert!((replacement_dim.defpt.y - expected_y).abs() < 1e-9, "baseline spacing must restart from seed: {replacement_dim:?}");
    assert_eq!(dimension(&s, original), original_dim);
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.iter().count(), 2);
    // Session-level undo/redo still treats the resulting chain as a single command.
    s.undo().unwrap();
    assert!(s.doc().unwrap().entity(replacement).is_none());
    assert_eq!(dimension(&s, original), original_dim);
    s.redo().unwrap();
    assert_eq!(dimension(&s, replacement), replacement_dim);
}

#[test]
fn dimcontinue_undo_allows_a_replacement_from_the_seed() {
    check_undo("dimcontinue");
}

#[test]
fn dimbaseline_undo_allows_a_replacement_from_the_seed() {
    check_undo("dimbaseline");
}
