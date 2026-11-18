//! BURST (Express Tools): a block reference explodes like EXPLODE, its visible attributes become
//! TEXT with their values, its invisible ones and the block's attribute definitions go.

use cadcraft_engine::Session;
use cadcraft_engine::doc::color::Color;
use cadcraft_engine::doc::{Entity, EntityKind, Handle};
use serde_json::{Value, json};

/// Block B: attribute PART (on layer TAGS, height 1), NOTE (on layer 0, height 0.5), SECRET
/// (invisible), a line and a reference to block N (a circle). One reference to B on layer R at
/// `at`, scaled by `scale`, with values for all three.
fn drawing(at: [f64; 2], scale: [f64; 2]) -> (Session, Handle) {
    let mut s = Session::new();
    s.execute("layer.new", &json!({"name": "TAGS", "current": true})).unwrap();
    s.execute("attdef", &json!({"tag": "PART", "default": "p", "at": [0, 2], "height": 1.0})).unwrap();
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    s.execute("attdef", &json!({"tag": "NOTE", "default": "n", "at": [0, 4], "height": 0.5})).unwrap();
    s.execute("attdef", &json!({"tag": "SECRET", "default": "s", "at": [0, 6], "height": 1.0, "invisible": true})).unwrap();
    let c = s.execute("circle", &json!({"center": [0, 0], "radius": 1})).unwrap();
    s.execute("block", &json!({"name": "N", "base": [0, 0], "keep": "delete", "handles": [c["handle"]]})).unwrap();
    s.execute("insert", &json!({"name": "N", "at": [5, 5]})).unwrap();
    s.execute("line", &json!({"points": [[0, 0], [10, 0]]})).unwrap();
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("block", &json!({"name": "B", "base": [0, 0], "keep": "delete", "handles": hs})).unwrap();
    s.execute("layer.new", &json!({"name": "R", "current": true})).unwrap();
    let r = s
        .execute("insert", &json!({"name": "B", "at": at, "scale": scale, "attribs": {"PART": "P-7", "NOTE": "Hello", "SECRET": "hidden"}}))
        .unwrap();
    (s, Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap())
}

fn model(s: &Session) -> Vec<Entity> {
    s.doc().unwrap().model.iter().map(|e| (**e).clone()).collect()
}

/// (value, layer, x, y, height) of every TEXT in model space.
fn texts(s: &Session) -> Vec<(String, String, f64, f64, f64)> {
    model(s)
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Text(t) => Some((t.value.clone(), e.common.layer.clone(), t.insert.x, t.insert.y, t.height)),
            _ => None,
        })
        .collect()
}

fn lines(s: &Session) -> Vec<[f64; 4]> {
    model(s)
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Line(l) => Some([l.a.x, l.a.y, l.b.x, l.b.y]),
            _ => None,
        })
        .collect()
}

fn inserts(s: &Session) -> Vec<String> {
    model(s)
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) => Some(i.block.clone()),
            _ => None,
        })
        .collect()
}

fn burst(s: &mut Session, h: Handle) -> Value {
    s.execute("burst", &json!({"handles": [h.hex()]})).unwrap()
}

#[test]
fn burst_turns_visible_attribute_values_into_text() {
    let (mut s, r) = drawing([100.0, 0.0], [1.0, 1.0]);
    burst(&mut s, r);
    assert_eq!(lines(&s), vec![[100.0, 0.0, 110.0, 0.0]]);
    // The nested block reference stays a block reference, as EXPLODE leaves it (one level).
    assert_eq!(inserts(&s), ["N"]);
    assert!(!model(&s).iter().any(|e| matches!(e.kind, EntityKind::AttDef(_))));
    // The values, not the tags; the invisible attribute is gone. PART keeps its definition's
    // layer; NOTE was defined on layer 0, so it goes on the reference's layer.
    let mut t = texts(&s);
    t.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(t, [("Hello".into(), "R".into(), 100.0, 4.0, 0.5), ("P-7".into(), "TAGS".into(), 100.0, 2.0, 1.0)]);
}

#[test]
fn burst_text_keeps_the_attribute_colour_and_explode_does_not() {
    let (mut s, r) = drawing([0.0, 0.0], [1.0, 1.0]);
    s.doc_mut()
        .unwrap()
        .modify_entity(r, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.attribs.iter_mut().filter(|a| a.tag == "NOTE").for_each(|a| a.props.color = Some(Color::Index(3)));
            }
        })
        .unwrap();
    burst(&mut s, r);
    let note = model(&s).into_iter().find(|e| matches!(&e.kind, EntityKind::Text(t) if t.value == "Hello")).unwrap();
    assert_eq!(note.common.color, Color::Index(3));
    // EXPLODE gives the definitions (their tags) instead of the values.
    let (mut x, r) = drawing([0.0, 0.0], [1.0, 1.0]);
    x.execute("explode", &json!({"handles": [r.hex()]})).unwrap();
    assert!(texts(&x).is_empty());
    assert_eq!(model(&x).iter().filter(|e| matches!(e.kind, EntityKind::AttDef(_))).count(), 3);
}

#[test]
fn burst_places_the_geometry_as_explode_does_for_a_non_uniformly_scaled_reference() {
    let (mut s, r) = drawing([10.0, 20.0], [2.0, 3.0]);
    let (mut x, _) = drawing([10.0, 20.0], [2.0, 3.0]);
    let EntityKind::Insert(ins) = &s.doc().unwrap().entity(r).unwrap().kind else { panic!("not a block reference") };
    let part = ins.attribs.iter().find(|a| a.tag == "PART").unwrap().text.clone();
    burst(&mut s, r);
    x.execute("explode", &json!({"handles": [r.hex()]})).unwrap();
    let geometry = |s: &Session| -> Vec<EntityKind> {
        model(s).into_iter().filter(|e| !matches!(e.kind, EntityKind::Text(_) | EntityKind::AttDef(_))).map(|e| e.kind).collect()
    };
    assert_eq!(geometry(&s), geometry(&x));
    // The text is the attribute as it was drawn: position, height, rotation, style.
    assert_eq!(part.insert.y, 26.0);
    assert!(model(&s).iter().any(|e| e.kind == EntityKind::Text(part.clone())), "{:?}", texts(&s));
}

#[test]
fn burst_is_one_undo_step_and_leaves_other_objects_alone() {
    let (mut s, r) = drawing([0.0, 0.0], [1.0, 1.0]);
    s.execute("line", &json!({"points": [[50, 50], [60, 50]]})).unwrap();
    let l = s.doc().unwrap().model.iter().last().unwrap().handle;
    let undo = s.state().unwrap().undo.len();
    s.execute("burst", &json!({"handles": [r.hex(), l.hex()]})).unwrap();
    assert_eq!(s.state().unwrap().undo.len(), undo + 1);
    assert!(lines(&s).contains(&[50.0, 50.0, 60.0, 50.0]));
    assert_eq!(texts(&s).len(), 2);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(inserts(&s), ["B"]);
    assert!(texts(&s).is_empty());
    assert_eq!(lines(&s), vec![[50.0, 50.0, 60.0, 50.0]]);
}

#[test]
fn burst_at_the_command_line_selects_then_bursts() {
    let (mut s, r) = drawing([0.0, 0.0], [1.0, 1.0]);
    s.cmdline("burst").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "{}", s.prompt_text());
    s.input(cadcraft_engine::Input::Pick(vec![r])).unwrap();
    s.input(cadcraft_engine::Input::Enter).unwrap();
    assert_eq!(inserts(&s), ["N"]);
    assert_eq!(texts(&s).len(), 2);
}
