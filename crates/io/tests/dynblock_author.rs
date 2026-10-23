//! Authoring dynamic blocks: parameters and actions added in CADCraft are written as the
//! evaluation graph objects the reader reads, survive a save, and drive references.
//!
//! The fixture is synthetic, written for this test: a BOX block (a 10 × 5 rectangle and a
//! circle on its right edge) with one reference.

use cadcraft_doc::dynblock::{
    ActionKind, ActionSpec, ParamPoint, ParameterKind, ParameterSpec, add_action, add_parameter, dynamic_block, set_property, tag_elements,
};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::{Vec2, Vec3};
use cadcraft_io::{read_dxf, write_dxf};

/// DXF text from lines of `code value` (the value may be empty).
fn dxf(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

const BOX: &str = "
0 SECTION
2 BLOCKS
0 BLOCK
2 BOX
70 0
10 0
20 0
30 0
0 LWPOLYLINE
5 30
8 0
90 4
70 1
10 0
20 0
10 10
20 0
10 10
20 5
10 0
20 5
0 CIRCLE
5 31
8 0
10 10
20 2.5
30 0
40 1
0 ENDBLK
0 ENDSEC
0 SECTION
2 ENTITIES
0 INSERT
5 40
8 0
2 BOX
10 100
20 0
30 0
0 ENDSEC
0 EOF
";

fn h(hex: &str) -> Handle {
    Handle::parse_hex(hex).unwrap()
}

fn box_drawing() -> Drawing {
    read_dxf(dxf(BOX).as_bytes()).unwrap()
}

fn v(x: f64, y: f64) -> Vec3 {
    Vec3::new(x, y, 0.0)
}

fn linear(grips: u8) -> ParameterSpec {
    ParameterSpec::Linear { base: v(0.0, 0.0), end: v(10.0, 0.0), label: None, label_offset: -2.0, grips }
}

/// BOX with a linear parameter (2 grips) stretching the rectangle's right edge.
fn stretchy() -> (Drawing, i32, i32) {
    let mut d = box_drawing();
    let p = add_parameter(&mut d, "box", &linear(2)).unwrap();
    let frame = vec![Vec2::new(8.0, -1.0), Vec2::new(12.0, 6.0)];
    let spec = ActionSpec::Stretch {
        parameter: p,
        point: ParamPoint::End,
        frame,
        selection: vec![h("30")],
        vertices: vec![(h("30"), vec![1, 2])],
        at: v(12.0, -2.0),
    };
    let a = add_action(&mut d, "BOX", &spec).unwrap();
    (d, p, a)
}

/// The graph's node and edge lists as written: per node (index, id, links), per edge (kind,
/// from, to, links).
fn graph(d: &Drawing) -> (Vec<(i32, i32, Vec<i32>)>, Vec<(i32, i32, i32, Vec<i32>)>) {
    let g = d.foreign.objects.iter().find(|o| o.kind == "ACAD_EVALUATION_GRAPH").unwrap();
    let at = g.tags.iter().position(|t| t.value == "AcDbEvalGraph").unwrap();
    let ints: Vec<(i32, i32)> = g.tags[at + 1..].iter().filter(|t| t.code != 360).map(|t| (t.code, t.value.parse().unwrap())).collect();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut i = 2;
    while ints.get(i).is_some_and(|t| t.0 == 91) {
        nodes.push((ints[i].1, ints[i + 2].1, ints[i + 3..i + 7].iter().map(|t| t.1).collect()));
        i += 7;
    }
    while i < ints.len() {
        edges.push((ints[i + 2].1, ints[i + 3].1, ints[i + 4].1, ints[i + 5..i + 10].iter().map(|t| t.1).collect()));
        i += 10;
    }
    (nodes, edges)
}

#[test]
fn an_authored_linear_stretch_reads_back() {
    let (d, p, a) = stretchy();
    assert_eq!((p, a), (1, 8), "parameter, 2 grips × (grip + 2 components), then the action");
    let m = dynamic_block(&d, "BOX").expect("BOX is dynamic now");
    let param = &m.parameters[0];
    assert_eq!((param.node, param.element.as_str(), param.show_properties), (1, "Linear", true));
    assert_eq!(param.grips, [5, 2], "base grip, end grip");
    let ParameterKind::Linear { base, end, label, .. } = &param.kind else { panic!("{param:?}") };
    assert_eq!((*base, *end, label.as_str()), (v(0.0, 0.0), v(10.0, 0.0), "Distance1"));
    let act = &m.actions[0];
    assert_eq!((act.node, act.name.as_str(), act.at), (8, "Stretch", v(12.0, -2.0)));
    assert_eq!(act.selection, [h("30")]);
    assert_eq!(act.connections, [(1, "EndXDelta".to_string()), (1, "EndYDelta".to_string())]);
    let ActionKind::Stretch { frame, vertices, multiplier, angle_offset } = &act.kind else { panic!("{act:?}") };
    assert_eq!(frame, &[Vec2::new(8.0, -1.0), Vec2::new(12.0, 6.0)]);
    assert_eq!(vertices, &[(h("30"), vec![1, 2])]);
    assert_eq!((*multiplier, *angle_offset), (1.0, 0.0));
    assert_eq!(m.other.len(), 6, "{:?}", m.other);
    assert_eq!(m.other.iter().filter(|o| *o == "BLOCKLINEARGRIP").count(), 2);
}

/// The graph is laid out as AutoCAD lays out the same block (nodes in creation order, each
/// edge in its from-node's outgoing list and its to-node's incoming list).
#[test]
fn the_graph_links_nodes_as_autocad_does() {
    let (d, _, _) = stretchy();
    let (nodes, edges) = graph(&d);
    let want_nodes = vec![
        (0, 1, vec![2, 5, 0, 6]),
        (1, 2, vec![-1, -1, 2, 2]),
        (2, 3, vec![0, 0, -1, -1]),
        (3, 4, vec![1, 1, -1, -1]),
        (4, 5, vec![-1, -1, 5, 5]),
        (5, 6, vec![3, 3, -1, -1]),
        (6, 7, vec![4, 4, -1, -1]),
        (7, 8, vec![6, 6, -1, -1]),
    ];
    assert_eq!(nodes, want_nodes);
    let want_edges = vec![
        (1, 0, 2, vec![-1, -1, -1, 1, -1]),
        (1, 0, 3, vec![-1, -1, 0, 3, -1]),
        (2, 1, 0, vec![-1, 5, -1, -1, -1]),
        (1, 0, 5, vec![-1, -1, 1, 4, -1]),
        (1, 0, 6, vec![-1, -1, 3, 6, -1]),
        (2, 4, 0, vec![2, -1, -1, -1, -1]),
        (1, 0, 7, vec![-1, -1, 4, -1, -1]),
    ];
    assert_eq!(edges, want_edges);
    // The record's dictionary holds the graph and the purge preventer.
    let x = d.foreign.records.get(&("BLOCK_RECORD".to_string(), "BOX".to_string())).unwrap();
    let dict = d.foreign.objects.iter().find(|o| Some(o.handle) == x.xdict).unwrap();
    let keys: Vec<&str> = dict.tags.iter().filter(|t| t.code == 3).map(|t| t.value.as_str()).collect();
    assert_eq!(keys, ["ACAD_ENHANCEDBLOCK", "AcDbDynamicBlockRoundTripPurgePreventer"]);
    assert!(x.xdata.iter().any(|t| t.value == "AcDbDynamicBlockTrueName"));
    for c in ["ACAD_EVALUATION_GRAPH", "BLOCKLINEARPARAMETER", "BLOCKLINEARGRIP", "BLOCKGRIPLOCATIONCOMPONENT", "BLOCKSTRETCHACTION"] {
        assert!(d.foreign.classes.iter().any(|k| k.iter().any(|t| t.code == 1 && t.value == c)), "class {c}");
    }
}

#[test]
fn an_authored_block_survives_a_save() {
    let (mut d, _, _) = stretchy();
    let mut store = d.block("BOX").unwrap().entities.clone();
    tag_elements(&mut d, "BOX", &mut store);
    let mut b = (**d.block("BOX").unwrap()).clone();
    b.entities = store;
    d.blocks.insert("BOX".into(), std::sync::Arc::new(b));
    let before = dynamic_block(&d, "BOX").unwrap();
    let text = write_dxf(&d);
    for t in ["ACAD_EVALUATION_GRAPH", "BLOCKSTRETCHACTION", "AcDbDynamicBlockRoundTripPurgePreventer", "AcDbBlockRepETag"] {
        assert!(text.contains(t), "{t} written");
    }
    let again = read_dxf(text.as_bytes()).unwrap();
    let after = dynamic_block(&again, "BOX").expect("still dynamic after a save");
    assert_eq!(after.parameters.len(), 1);
    assert_eq!(after.parameters[0].kind, before.parameters[0].kind);
    assert_eq!(after.actions[0].kind, before.actions[0].kind);
    assert_eq!(after.actions[0].selection, before.actions[0].selection, "the block's objects keep their handles");
}

/// The rectangle's corners in reference `insert`'s block.
fn rect(d: &Drawing, insert: Handle) -> Vec<(f64, f64)> {
    let EntityKind::Insert(i) = &d.entity(insert).unwrap().kind else { panic!() };
    let b = d.block(&i.block).unwrap();
    b.entities
        .iter()
        .find_map(|e| if let EntityKind::LwPolyline(pl) = &e.kind { Some(pl.vertices.iter().map(|v| (v.p.x, v.p.y)).collect()) } else { None })
        .unwrap()
}

#[test]
fn a_reference_to_an_authored_block_stretches() {
    let (mut d, _, _) = stretchy();
    set_property(&mut d, h("40"), "Distance1", "15").unwrap();
    assert_eq!(rect(&d, h("40")), [(0.0, 0.0), (15.0, 0.0), (15.0, 5.0), (0.0, 5.0)]);
}

#[test]
fn a_point_parameter_moves_the_circle() {
    let mut d = box_drawing();
    let p = add_parameter(&mut d, "BOX", &ParameterSpec::Point { at: v(10.0, 2.5), label: None, label_at: v(11.0, 3.5), grips: 1 }).unwrap();
    let a =
        add_action(&mut d, "BOX", &ActionSpec::Move { parameter: p, point: ParamPoint::End, selection: vec![h("31")], at: v(12.0, 1.0) }).unwrap();
    assert_eq!((p, a), (1, 5));
    let m = dynamic_block(&d, "BOX").unwrap();
    let ParameterKind::Point { at, label, .. } = &m.parameters[0].kind else { panic!() };
    assert_eq!((*at, label.as_str()), (v(10.0, 2.5), "Position1"));
    assert_eq!(m.parameters[0].grips, [2]);
    assert_eq!(m.actions[0].connections, [(1, "XDelta".to_string()), (1, "YDelta".to_string())]);
    assert!(matches!(m.actions[0].kind, ActionKind::Move { multiplier: 1.0, .. }));
    set_property(&mut d, h("40"), "Position1 X", "13").unwrap();
    let EntityKind::Insert(i) = &d.entity(h("40")).unwrap().kind else { panic!() };
    let c = d.block(&i.block).unwrap().entities.iter().find_map(|e| if let EntityKind::Circle(c) = &e.kind { Some(c.center) } else { None }).unwrap();
    assert_eq!((c.x, c.y), (13.0, 2.5));
}

#[test]
fn names_and_labels_count_up_and_grips_follow_the_count() {
    let mut d = box_drawing();
    let first = add_parameter(&mut d, "BOX", &linear(0)).unwrap();
    let second = add_parameter(&mut d, "BOX", &linear(1)).unwrap();
    let third = add_parameter(&mut d, "BOX", &ParameterSpec::Point { at: v(1.0, 1.0), label: Some("Hinge".into()), label_at: v(2.0, 2.0), grips: 0 })
        .unwrap();
    assert_eq!((first, second, third), (1, 2, 6), "no grips, then one grip with two components");
    let m = dynamic_block(&d, "BOX").unwrap();
    let names: Vec<_> = m.parameters.iter().map(|p| (p.element.as_str(), p.kind.label().unwrap_or("-"), p.grips.clone())).collect();
    assert_eq!(names, [("Linear", "Distance1", vec![0, 0]), ("Linear1", "Distance2", vec![0, 3]), ("Point", "Hinge", vec![0])]);
    let p = second;
    let a =
        add_action(&mut d, "BOX", &ActionSpec::Move { parameter: p, point: ParamPoint::Base, selection: vec![h("31")], at: v(0.0, 0.0) }).unwrap();
    let b = add_action(&mut d, "BOX", &ActionSpec::Move { parameter: p, point: ParamPoint::End, selection: vec![h("31")], at: v(0.0, 0.0) }).unwrap();
    let m = dynamic_block(&d, "BOX").unwrap();
    let acts: Vec<_> = m.actions.iter().map(|x| (x.node, x.name.as_str(), x.connections[0].1.as_str())).collect();
    assert_eq!(acts, [(a, "Move", "BaseXDelta"), (b, "Move1", "EndXDelta")]);
}

#[test]
fn bad_authoring_is_refused_and_changes_nothing() {
    let (d0, p, _) = stretchy();
    let bad_params = [
        ParameterSpec::Linear { base: v(1.0, 1.0), end: v(1.0, 1.0), label: None, label_offset: 0.0, grips: 2 },
        ParameterSpec::Linear { base: v(f64::NAN, 0.0), end: v(1.0, 1.0), label: None, label_offset: 0.0, grips: 2 },
        ParameterSpec::Linear { base: v(0.0, 0.0), end: v(1.0, 1.0), label: None, label_offset: f64::INFINITY, grips: 2 },
        ParameterSpec::Linear { base: v(1e308, 0.0), end: v(-1e308, 0.0), label: None, label_offset: 0.0, grips: 2 },
        ParameterSpec::Linear { base: v(0.0, 0.0), end: v(1.0, 1.0), label: None, label_offset: 0.0, grips: 3 },
        ParameterSpec::Point { at: v(0.0, 0.0), label: None, label_at: v(0.0, 0.0), grips: 2 },
    ];
    for spec in &bad_params {
        let mut d = d0.clone();
        assert!(add_parameter(&mut d, "BOX", spec).is_err(), "{spec:?}");
        assert_eq!(d.foreign, d0.foreign, "{spec:?} changed the drawing");
    }
    let mv = |parameter, selection: Vec<Handle>| ActionSpec::Move { parameter, point: ParamPoint::End, selection, at: v(0.0, 0.0) };
    let st = |frame: Vec<Vec2>| ActionSpec::Stretch {
        parameter: p,
        point: ParamPoint::End,
        frame,
        selection: vec![h("30")],
        vertices: vec![],
        at: v(0.0, 0.0),
    };
    let bad_actions = [
        mv(99, vec![h("30")]),
        mv(p, vec![]),
        mv(p, vec![h("30"); 20_000]),
        st(vec![Vec2::new(0.0, 0.0)]),
        st(vec![Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0)]),
    ];
    for spec in &bad_actions {
        let mut d = d0.clone();
        assert!(add_action(&mut d, "BOX", spec).is_err(), "{spec:?}");
        assert_eq!(d.foreign, d0.foreign, "{spec:?} changed the drawing");
    }
    let mut d = box_drawing();
    assert!(add_action(&mut d, "BOX", &mv(1, vec![h("30")])).is_err(), "no parameters yet");
    assert!(add_parameter(&mut d, "*Model_Space", &linear(2)).is_err());
    assert!(add_parameter(&mut d, "", &linear(2)).is_err());
}

#[test]
fn element_tags_are_unique_and_the_record_keeps_the_next() {
    let (mut d, _, _) = stretchy();
    let mut store = d.block("BOX").unwrap().entities.clone();
    // A copy carrying the rectangle's tag gets one of its own.
    let mut copy = (**store.get(h("30")).unwrap()).clone();
    tag_elements(&mut d, "BOX", &mut store);
    copy.handle = h("32");
    copy.common.xdata = store.get(h("30")).unwrap().common.xdata.clone();
    store.push(copy);
    tag_elements(&mut d, "BOX", &mut store);
    let tag_of = |e: &cadcraft_doc::Entity| e.common.xdata.iter().find(|t| t.code == 1071).map(|t| t.value.clone()).unwrap();
    let self_of = |e: &cadcraft_doc::Entity| e.common.xdata.iter().find(|t| t.code == 1005).map(|t| t.value.clone()).unwrap();
    let tags: Vec<_> = store.iter().map(|e| (tag_of(e), self_of(e))).collect();
    assert_eq!(tags, [("0".into(), "30".into()), ("1".into(), "31".into()), ("2".into(), "32".into())]);
    let x = d.foreign.records.get(&("BLOCK_RECORD".to_string(), "BOX".to_string())).unwrap();
    assert_eq!(x.xdata.iter().rev().find(|t| t.code == 1071).map(|t| t.value.as_str()), Some("3"));
}
