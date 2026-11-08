use cadcraft_doc::{EntityKind, Handle, Space, StoreyView};
use cadcraft_geom::Vec2;
use serde_json::{Value, json};

use crate::Session;
use crate::snap::{self, mode};

fn h(r: &Value) -> Handle {
    let s = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap();
    Handle::parse_hex(s).unwrap()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Handle {
    h(&s.execute("line", &json!({ "points": [a, b] })).unwrap())
}

fn storey_of(s: &Session, h: Handle) -> Option<u32> {
    s.doc().unwrap().entity(h).and_then(|e| e.common.storey())
}

fn id_of(s: &Session, name: &str) -> u32 {
    s.doc().unwrap().storeys.by_name(name).unwrap().id
}

fn view(s: &Session, h: Handle) -> StoreyView {
    let d = s.doc().unwrap();
    d.storey_view(d.entity(h).unwrap())
}

/// EG with a wall line, OG1 above it (current) with its own line.
fn building() -> (Session, Handle, Handle) {
    let mut s = Session::new();
    let eg = line(&mut s, [0.0, 0.0], [10.0, 0.0]);
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.new", &json!({ "name": "OG1" })).unwrap();
    let og = line(&mut s, [0.0, 5.0], [10.0, 5.0]);
    (s, eg, og)
}

fn dump(s: &Session) -> String {
    format!("{:?}", s.doc().unwrap().model.iter().map(|e| (e.handle, e.common.clone(), e.kind.clone())).collect::<Vec<_>>())
}

#[test]
fn the_first_storey_takes_the_drawing_and_new_objects_land_on_the_current_one() {
    let (s, eg, og) = building();
    assert_eq!(storey_of(&s, eg), Some(id_of(&s, "EG")), "drawn before storeys: the first storey");
    assert_eq!(storey_of(&s, og), Some(id_of(&s, "OG1")));
    assert_eq!(view(&s, eg), StoreyView::Hidden);
    assert_eq!(view(&s, og), StoreyView::Current);
    let list = s.doc().unwrap().storeys.list.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    assert_eq!(list, ["EG", "OG1"], "a new storey goes above the current one");
}

#[test]
fn switching_storeys_shows_one_at_a_time_and_clears_the_selection() {
    let (mut s, eg, og) = building();
    s.set_selection(vec![og]);
    s.execute("storey.down", &json!({})).unwrap();
    assert!(s.selection().is_empty());
    assert_eq!(view(&s, eg), StoreyView::Current);
    assert_eq!(view(&s, og), StoreyView::Hidden);
    let r = s.execute("storey.down", &json!({})).unwrap();
    assert!(r["message"].as_str().unwrap().contains("bottom"));
    s.execute("storey.current", &json!({ "name": "og1" })).unwrap();
    assert_eq!(view(&s, og), StoreyView::Current, "names match without case");
}

#[test]
fn layers_switch_on_every_storey() {
    let (mut s, eg, og) = building();
    s.execute("layer.new", &json!({ "name": "Walls" })).unwrap();
    s.execute("laymch", &json!({ "handles": [og.hex()], "layer": "Walls" })).unwrap();
    s.execute("storey.down", &json!({})).unwrap();
    s.execute("laymch", &json!({ "handles": [eg.hex()], "layer": "Walls" })).unwrap();
    s.execute("layer.set", &json!({ "name": "Walls", "on": false })).unwrap();
    let d = s.doc().unwrap();
    assert!(!d.is_visible(d.entity(eg).unwrap()), "walls off on EG");
    s.execute("storey.up", &json!({})).unwrap();
    let d = s.doc().unwrap();
    assert!(!d.is_visible(d.entity(og).unwrap()), "and on OG1, without switching anything per storey");
}

#[test]
fn objects_of_other_storeys_are_out_of_reach_of_commands() {
    let (mut s, eg, _) = building();
    let before = dump(&s);
    for (cmd, p) in [
        ("erase", json!({ "handles": [eg.hex()] })),
        ("move", json!({ "handles": [eg.hex()], "delta": [1, 1] })),
        ("rotate", json!({ "handles": [eg.hex()], "base": [0, 0], "angle": 90 })),
        ("laymch", json!({ "handles": [eg.hex()], "layer": "0" })),
        ("storeymove", json!({ "handles": [eg.hex()], "to": "OG1" })),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} on an object of another storey");
    }
    assert_eq!(dump(&s), before, "nothing changed");
    // From the selection they are dropped (a stale selection, a script's pickfirst).
    s.set_selection(vec![eg]);
    s.execute("erase", &json!({})).unwrap();
    assert_eq!(dump(&s), before);
}

#[test]
fn select_all_and_windows_see_the_current_storey_only() {
    let (mut s, eg, og) = building();
    s.execute("selectall", &json!({})).unwrap();
    assert_eq!(s.selection(), vec![og]);
    let d = s.doc().unwrap();
    let w = crate::select::select_window(d, &Space::Model, cadcraft_geom::Bounds2::new(Vec2::new(-1.0, -1.0), Vec2::new(11.0, 6.0)), false);
    assert_eq!(w, vec![og]);
    assert!(!w.contains(&eg));
}

#[test]
fn drawing_wide_layer_commands_reach_every_storey() {
    let (mut s, eg, og) = building();
    s.execute("layer.new", &json!({ "name": "Old" })).unwrap();
    s.execute("laymch", &json!({ "handles": [og.hex()], "layer": "Old" })).unwrap();
    s.execute("storey.down", &json!({})).unwrap();
    s.execute("laymch", &json!({ "handles": [eg.hex()], "layer": "Old" })).unwrap();
    s.execute("storey.up", &json!({})).unwrap();
    // Renaming a layer renames it on every storey.
    s.execute("layer.set", &json!({ "name": "Old", "newName": "Walls" })).unwrap();
    let layer = |s: &Session, h| s.doc().unwrap().entity(h).unwrap().common.layer.clone();
    assert_eq!(layer(&s, eg), "Walls");
    assert_eq!(layer(&s, og), "Walls");
    // Merging layers too.
    s.execute("laymrg", &json!({ "from": ["Walls"], "to": "0" })).unwrap();
    assert_eq!(layer(&s, eg), "0");
}

#[test]
fn style_renames_reach_every_storey() {
    let (mut s, _, _) = building();
    s.execute("style", &json!({ "name": "Notes", "current": false })).unwrap();
    s.execute("dimstyle", &json!({ "name": "DS", "current": false })).unwrap();
    // Text and dimension on EG, renamed from OG1.
    s.execute("storey.down", &json!({})).unwrap();
    s.execute("style.current", &json!({ "name": "Notes" })).unwrap();
    s.execute("dimstyle.current", &json!({ "name": "DS" })).unwrap();
    let t = h(&s.execute("text", &json!({ "at": [0, 2], "height": 0.25, "text": "X" })).unwrap());
    let d = h(&s.execute("dimlinear", &json!({ "p1": [0, 0], "p2": [10, 0], "at": [5, 3] })).unwrap());
    s.execute("style.current", &json!({ "name": "Standard" })).unwrap();
    s.execute("dimstyle.current", &json!({ "name": "Standard" })).unwrap();
    s.execute("storey.up", &json!({})).unwrap();
    s.execute("style.rename", &json!({ "from": "Notes", "to": "Labels" })).unwrap();
    s.execute("dimstyle.rename", &json!({ "from": "DS", "to": "DS2" })).unwrap();
    let doc = s.doc().unwrap();
    assert!(doc.text_style("Labels").is_some() && doc.dim_style("DS2").is_some(), "the renames stand");
    assert!(matches!(dim(&s, t), EntityKind::Text(x) if x.style == "Labels"), "the text on EG follows");
    assert!(matches!(dim(&s, d), EntityKind::Dimension(x) if x.style == "DS2"), "the dimension on EG follows");
}

#[test]
fn the_guard_takes_back_a_command_that_reaches_another_storey() {
    let (mut s, eg, _) = building();
    let before = dump(&s);
    // Simulate a command with a bug: it moves an object of another storey.
    let doc_before = s.state().unwrap().doc.clone();
    s.doc_mut().unwrap().modify_entity(eg, |e| e.kind.transform(&cadcraft_geom::Mat3::translate(Vec2::new(3.0, 0.0)))).unwrap();
    let m = super::guard(&mut s, &doc_before, None, "move");
    assert!(m.unwrap().contains("current storey"));
    assert_eq!(dump(&s), before, "taken back");
    // A drawing-wide command may.
    s.doc_mut().unwrap().modify_entity(eg, |e| e.common.layer = "0".into()).unwrap();
    let doc_before = s.state().unwrap().doc.clone();
    s.doc_mut().unwrap().modify_entity(eg, |e| e.common.color = cadcraft_color::Color::Index(1)).unwrap();
    assert!(super::guard(&mut s, &doc_before, None, "laymrg").is_none());
}

#[test]
fn ghost_is_drawn_beneath_in_its_colour_snapped_to_and_never_selected() {
    let (mut s, eg, og) = building();
    s.execute("storey.ghost", &json!({ "name": "EG", "color": "#c8b48c" })).unwrap();
    assert_eq!(view(&s, eg), StoreyView::Ghost);
    let d = s.doc().unwrap();
    assert!(d.is_ghost(d.entity(eg).unwrap()));
    assert!(!d.is_visible(d.entity(eg).unwrap()));
    // Snaps to its end point.
    let hit = snap::osnap(d, &Space::Model, Vec2::new(10.05, 0.0), 0.5, mode::END, None, false).unwrap();
    assert!(hit.point.near(Vec2::new(10.0, 0.0), 1e-9));
    // Not picked or selected.
    assert_eq!(crate::select::pick(d, &Space::Model, Vec2::new(5.0, 0.0), 0.5), None);
    // Drawn first, in the ghost colour; the current storey after it in its own.
    let list = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
    let first = list.prims.first().unwrap();
    assert_eq!(first.handle, eg);
    assert_eq!(first.color, cadcraft_color::Rgb(0xc8, 0xb4, 0x8c));
    assert!(list.prims.iter().any(|p| p.handle == og && p.color != first.color));
    // Never plotted.
    let plot = cadcraft_render::build_plot(d, &Space::Model, &cadcraft_render::Options::default());
    assert!(plot.prims.iter().all(|p| p.handle != eg));
    // Off again.
    s.execute("storey.ghost", &json!({ "name": null })).unwrap();
    assert_eq!(view(&s, eg), StoreyView::Hidden);
    assert!(s.execute("storey.ghost", &json!({ "name": "OG1" })).is_err(), "not the current storey");
}

#[test]
fn drawing_on_the_ghost_ends_on_the_current_storey() {
    let (mut s, eg, _) = building();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    // A line snapped to the ghost's end point.
    let d = s.doc().unwrap();
    let p = snap::osnap(d, &Space::Model, Vec2::new(9.9, 0.1), 0.5, mode::END, None, false).unwrap().point;
    let new = line(&mut s, [p.x, p.y], [10.0, 5.0]);
    assert_eq!(storey_of(&s, new), Some(id_of(&s, "OG1")));
    assert_eq!(storey_of(&s, eg), Some(id_of(&s, "EG")));
}

#[test]
fn storeycopy_copies_in_place_and_storeymove_moves() {
    let (mut s, _, og) = building();
    s.execute("storey.new", &json!({ "name": "DG" })).unwrap();
    s.execute("storey.current", &json!({ "name": "OG1" })).unwrap();
    let r = s.execute("storeycopy", &json!({ "handles": [og.hex()], "to": ["EG", "DG"] })).unwrap();
    let copies: Vec<Handle> = r["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    assert_eq!(copies.len(), 2);
    let kind = |s: &Session, h| s.doc().unwrap().entity(h).unwrap().kind.clone();
    for c in &copies {
        assert_eq!(kind(&s, *c), kind(&s, og), "same place");
    }
    assert_eq!(storey_of(&s, copies[0]), Some(id_of(&s, "EG")));
    assert_eq!(storey_of(&s, copies[1]), Some(id_of(&s, "DG")));
    s.execute("storeymove", &json!({ "handles": [og.hex()], "to": "DG" })).unwrap();
    assert_eq!(storey_of(&s, og), Some(id_of(&s, "DG")));
    assert_eq!(view(&s, og), StoreyView::Hidden);
    // Undo brings it back to OG1 and stays on the storey being looked at.
    s.undo().unwrap();
    assert_eq!(storey_of(&s, og), Some(id_of(&s, "OG1")));
    assert_eq!(s.doc().unwrap().storeys.current, id_of(&s, "OG1"));
}

#[test]
fn paste_and_explode_land_on_the_current_storey() {
    let (mut s, eg, og) = building();
    s.execute("copyclip", &json!({ "handles": [og.hex()] })).unwrap();
    s.execute("storey.down", &json!({})).unwrap();
    s.execute("pasteclip", &json!({})).unwrap();
    let pasted = s.doc().unwrap().model.last().unwrap().handle;
    assert_eq!(storey_of(&s, pasted), Some(id_of(&s, "EG")), "pasted where it is pasted, not where it was copied");
    // A block made on EG, exploded on OG1: the parts come out on OG1.
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [eg.hex()], "keep": "delete" })).unwrap();
    s.execute("storey.up", &json!({})).unwrap();
    let ins = h(&s.execute("insert", &json!({ "name": "B", "at": [0, 20] })).unwrap());
    s.execute("explode", &json!({ "handles": [ins.hex()] })).unwrap();
    let part = s.doc().unwrap().model.last().unwrap().clone();
    assert!(matches!(part.kind, EntityKind::Line(_)));
    assert_eq!(part.common.storey(), Some(id_of(&s, "OG1")));
    assert_eq!(view(&s, part.handle), StoreyView::Current);
}

#[test]
fn a_block_shows_whole_wherever_it_was_made() {
    let (mut s, eg, _) = building();
    // The block's contents keep the storey they were drawn on in their xdata; they show with the
    // reference all the same.
    s.execute("storey.down", &json!({})).unwrap();
    s.execute("block", &json!({ "name": "B", "base": [0, 0], "handles": [eg.hex()], "keep": "delete" })).unwrap();
    s.execute("storey.up", &json!({})).unwrap();
    let ins = h(&s.execute("insert", &json!({ "name": "B", "at": [0, 20] })).unwrap());
    let d = s.doc().unwrap();
    let list = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
    assert!(list.prims.iter().any(|p| p.handle == ins), "the block's line is drawn");
}

#[test]
fn rename_order_and_delete() {
    let (mut s, _, og) = building();
    s.execute("storey.rename", &json!({ "name": "OG1", "newName": "1. OG" })).unwrap();
    assert!(s.execute("storey.rename", &json!({ "name": "EG", "newName": "1. og" })).is_err(), "names are unique without case");
    s.execute("storey.new", &json!({ "name": "UG", "below": true, "current": false })).unwrap();
    let names = |s: &Session| s.doc().unwrap().storeys.list.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), ["EG", "UG", "1. OG"], "below the current storey");
    s.execute("storey.order", &json!({ "name": "UG", "index": 0 })).unwrap();
    assert_eq!(names(&s), ["UG", "EG", "1. OG"]);
    assert!(s.execute("storey.delete", &json!({ "name": "1. OG" })).is_err(), "not empty");
    s.execute("storey.delete", &json!({ "name": "UG" })).unwrap();
    assert_eq!(names(&s), ["EG", "1. OG"]);
    assert_eq!(storey_of(&s, og), Some(id_of(&s, "1. OG")), "ids are stable");
    assert!(s.execute("storey.new", &json!({ "name": "  " })).is_err());
    assert!(s.execute("storey.new", &json!({ "name": "EG" })).is_err());
}

#[test]
fn deleting_the_last_storey_turns_storeys_off() {
    let mut s = Session::new();
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    let l = line(&mut s, [0.0, 0.0], [1.0, 0.0]);
    s.execute("erase", &json!({ "handles": [l.hex()] })).unwrap();
    s.execute("storey.delete", &json!({ "name": "EG" })).unwrap();
    assert!(!s.doc().unwrap().storeys.active());
    let l = line(&mut s, [0.0, 0.0], [1.0, 0.0]);
    assert_eq!(storey_of(&s, l), None, "no storeys, no storey xdata");
}

#[test]
fn typed_storey_command() {
    let mut s = Session::new();
    s.cmdline("storey").unwrap();
    s.cmdline("new").unwrap();
    s.cmdline("EG").unwrap();
    s.cmdline("n").unwrap();
    s.cmdline("OG1").unwrap();
    s.cmdline("ghost").unwrap();
    s.cmdline("EG").unwrap();
    let eg = id_of(&s, "EG");
    assert_eq!(s.doc().unwrap().ghost_storey, Some(eg));
    s.cmdline("EG").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none());
    let d = s.doc().unwrap();
    assert_eq!(d.storeys.list.len(), 2);
    assert_eq!(d.storeys.current, eg, "a storey name makes it current");
    assert_eq!(d.ghost_storey, None, "going to the ghost storey ends the ghost");
    // The new storeys are undoable, switching is not.
    s.undo().unwrap();
    assert_eq!(s.doc().unwrap().storeys.list.len(), 1);
}

#[test]
fn typed_storeycopy_asks_for_objects_then_the_storey() {
    let (mut s, _, og) = building();
    s.cmdline("storeycopy").unwrap();
    s.input(crate::Input::Pick(vec![og])).unwrap();
    s.input(crate::Input::Enter).unwrap();
    s.cmdline("EG").unwrap();
    assert!(s.running.is_none());
    let copy = s.doc().unwrap().model.last().unwrap().clone();
    assert_ne!(copy.handle, og);
    assert_eq!(copy.common.storey(), Some(id_of(&s, "EG")));
}

// ---------------- round 1 of the review ----------------

fn dim(s: &Session, h: Handle) -> EntityKind {
    s.doc().unwrap().entity(h).unwrap().kind.clone()
}

#[test]
fn a_dimension_on_another_storey_does_not_follow_its_source() {
    // The associative pass ran after the guard: moving a line on EG changed its dimension on OG1.
    let (mut s, eg, _) = building();
    s.execute("storey.down", &json!({})).unwrap();
    let d = h(&s.execute("dimlinear", &json!({ "object": eg.hex(), "at": [5, -3] })).unwrap());
    assert!(matches!(dim(&s, d), EntityKind::Dimension(ref x) if !x.assoc.is_empty()), "associative");
    s.execute("storeymove", &json!({ "handles": [d.hex()], "to": "OG1" })).unwrap();
    let before = dim(&s, d);
    s.execute("move", &json!({ "handles": [eg.hex()], "delta": [0, 2] })).unwrap();
    assert_eq!(dim(&s, d), before, "the dimension on OG1 waits");
    let EntityKind::Line(l) = dim(&s, eg) else { panic!("a line") };
    assert!((l.a.y - 2.0).abs() < 1e-9, "the line on EG moved");
}

#[test]
fn storeymove_cuts_constraints_between_storeys() {
    let (mut s, _, og) = building();
    let other = line(&mut s, [0.0, 8.0], [10.0, 9.0]);
    s.execute("geomconstraint", &json!({ "type": "parallel", "h1": og.hex(), "h2": other.hex() })).unwrap();
    assert_eq!(s.doc().unwrap().constraints.len(), 1);
    let r = s.execute("storeymove", &json!({ "handles": [other.hex()], "to": "EG" })).unwrap();
    assert!(r["message"].as_str().unwrap().contains("1 constraint"));
    assert!(s.doc().unwrap().constraints.is_empty());
    // Rotating the line left on OG1 can't turn the one on EG now.
    s.execute("rotate", &json!({ "handles": [og.hex()], "base": [0, 5], "angle": 30 })).unwrap();
}

#[test]
fn going_to_the_ghost_storey_ends_the_ghost() {
    let (mut s, _, _) = building();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    assert_eq!(s.doc().unwrap().ghost_storey, None);
    // Undo of a new storey falls back to a current storey that was the ghost.
    s.execute("storey.new", &json!({ "name": "DG" })).unwrap();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    s.undo().unwrap();
    let d = s.doc().unwrap();
    assert_ne!(d.ghost_storey, Some(d.storeys.current));
}

#[test]
fn storeycopy_never_copies_onto_the_current_storey() {
    let (mut s, _, og) = building();
    let n = s.doc().unwrap().model.len();
    assert!(s.execute("storeycopy", &json!({ "handles": [og.hex()], "to": "OG1" })).is_err());
    s.execute("storeycopy", &json!({ "handles": [og.hex()], "to": ["OG1", "EG"] })).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), n + 1, "only the copy on EG");
}

#[test]
fn last_and_the_selection_skip_other_storeys() {
    let (mut s, _, og) = building();
    s.execute("storeycopy", &json!({ "handles": [og.hex()], "to": "EG" })).unwrap();
    // LAST would have picked the copy on EG, the newest object.
    s.cmdline("erase").unwrap();
    s.cmdline("l").unwrap();
    s.cmdline("").unwrap();
    assert!(s.doc().unwrap().entity(og).is_none(), "LAST is the last object of the current storey");
    let hidden = s.doc().unwrap().model.last().unwrap().handle;
    s.set_selection(vec![hidden]);
    assert!(s.selection().is_empty());
}

#[test]
fn purge_leaves_degenerate_objects_of_other_storeys() {
    let (mut s, _, _) = building();
    // Zero-length lines as a damaged file brings them (LINE itself refuses them).
    let zero = |s: &mut Session, storey: &str, x: f64| {
        let id = id_of(s, storey);
        let mut c = cadcraft_doc::Common::default();
        c.set_storey(Some(id));
        let p = cadcraft_geom::Vec3::new(x, x, 0.0);
        s.doc_mut().unwrap().add(&Space::Model, c, EntityKind::Line(cadcraft_doc::Line { a: p, b: p })).unwrap()
    };
    let dot = zero(&mut s, "EG", 3.0);
    let here = zero(&mut s, "OG1", 4.0);
    s.execute("purge", &json!({ "type": "zerolength" })).unwrap();
    assert!(s.doc().unwrap().entity(dot).is_some(), "EG's zero-length line stays");
    assert!(s.doc().unwrap().entity(here).is_none());
}

#[test]
fn a_new_storey_does_not_take_objects_without_one() {
    let (mut s, eg, _) = building();
    s.execute("storey.down", &json!({})).unwrap();
    // An object another program added: no storey.
    s.doc_mut().unwrap().modify_entity(eg, |e| e.common.set_storey(None)).unwrap();
    s.execute("storey.new", &json!({ "name": "Zwischen" })).unwrap();
    assert_eq!(storey_of(&s, eg), Some(id_of(&s, "EG")), "it stays on the storey it was drawn on");
}

#[test]
fn opening_a_drawing_settles_objects_without_a_storey() {
    let (s0, eg, _) = building();
    let mut d = s0.doc().unwrap().clone();
    d.model.modify(eg, |e| e.common.set_storey(None));
    let mut s = Session::new();
    s.open_drawing(d, "x", None);
    assert_eq!(storey_of(&s, eg), Some(id_of(&s, "OG1")), "the storey it was saved on");
    assert!(!s.state().unwrap().is_dirty());
}

#[test]
fn the_clipboard_preview_shows_objects_copied_on_another_storey() {
    let (mut s, eg, _) = building();
    let d = s.doc().unwrap();
    let e = d.entity(eg).unwrap().as_ref().clone();
    // A loose copy of an object of another storey (the clipboard holds such copies).
    let list = cadcraft_render::build_entities(d, [&e], &cadcraft_render::Options::default());
    assert!(!list.prims.is_empty());
    let _ = &mut s;
}

#[test]
fn the_drawing_wide_list_names_real_commands() {
    for id in super::DRAWING_WIDE {
        assert!(crate::cmd::find_command(id).is_some(), "{id}");
    }
}

// ---------------- round 2 of the review ----------------

fn assoc(s: &Session, h: Handle) -> Vec<cadcraft_doc::DimAssoc> {
    match dim(s, h) {
        EntityKind::Dimension(d) => d.assoc,
        k => panic!("not a dimension: {k:?}"),
    }
}

#[test]
fn storeycopy_relinks_copies_to_copies_and_drops_the_rest() {
    let (mut s, _, og) = building();
    let d = h(&s.execute("dimlinear", &json!({ "object": og.hex(), "at": [5, 8] })).unwrap());
    assert!(!assoc(&s, d).is_empty());
    // Line and dimension together: the copied dimension follows the copied line.
    let r = s.execute("storeycopy", &json!({ "handles": [og.hex(), d.hex()], "to": "EG" })).unwrap();
    let copies: Vec<Handle> = r["handles"].as_array().unwrap().iter().map(|v| Handle::parse_hex(v.as_str().unwrap()).unwrap()).collect();
    let (line_copy, dim_copy) = (copies[0], copies[1]);
    assert!(assoc(&s, dim_copy).iter().all(|a| a.handle == line_copy), "points at the copied line");
    // The dimension alone: no link back to OG1.
    let r = s.execute("storeycopy", &json!({ "handles": [d.hex()], "to": "EG" })).unwrap();
    let alone = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
    assert!(assoc(&s, alone).is_empty());
}

#[test]
fn a_dimension_on_the_ghost_is_not_associative_to_it() {
    let (mut s, eg, _) = building();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    let d = h(&s.execute("dimlinear", &json!({ "object": eg.hex(), "at": [5, -3] })).unwrap());
    assert!(assoc(&s, d).is_empty(), "measured from the ghost, not tied to it");
    assert_eq!(storey_of(&s, d), Some(id_of(&s, "OG1")));
}

#[test]
fn storeymove_of_a_dimension_cuts_its_links() {
    let (mut s, _, og) = building();
    let d = h(&s.execute("dimlinear", &json!({ "object": og.hex(), "at": [5, 8] })).unwrap());
    s.execute("storeymove", &json!({ "handles": [d.hex()], "to": "EG" })).unwrap();
    assert!(assoc(&s, d).is_empty());
    // And the other way: the line moves, the dimension stays.
    let d2 = h(&s.execute("dimlinear", &json!({ "object": og.hex(), "at": [5, 9] })).unwrap());
    s.execute("storeymove", &json!({ "handles": [og.hex()], "to": "EG" })).unwrap();
    assert!(assoc(&s, d2).is_empty());
}

#[test]
fn previous_selection_leaves_out_other_storeys() {
    let (mut s, eg, _) = building();
    s.execute("storey.down", &json!({})).unwrap();
    s.set_selection(vec![eg]);
    s.execute("move", &json!({ "delta": [0, 0] })).unwrap();
    s.remember_selection(&[eg]);
    s.execute("storey.up", &json!({})).unwrap();
    let before = dim(&s, eg);
    s.cmdline("move").unwrap();
    s.cmdline("p").unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "nothing selected, the command ends");
    assert_eq!(dim(&s, eg), before);
}

// ---------------- round 3 of the review ----------------

/// A dimension on EG associative to a line on OG1, as a file edited elsewhere can bring it.
fn cross_linked_dimension(s: &mut Session, eg_line: Handle, og_line: Handle) -> Handle {
    s.execute("storey.down", &json!({})).unwrap();
    let d = h(&s.execute("dimlinear", &json!({ "object": eg_line.hex(), "at": [5, -3] })).unwrap());
    s.execute("storey.up", &json!({})).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(d, |e| {
            if let EntityKind::Dimension(dm) = &mut e.kind {
                dm.assoc.iter_mut().for_each(|a| a.handle = og_line);
            }
        })
        .unwrap();
    d
}

#[test]
fn an_untouched_neighbour_on_another_storey_keeps_its_links() {
    let (mut s, eg, og) = building();
    let d = cross_linked_dimension(&mut s, eg, og);
    let before = dim(&s, d);
    // A line drawn on OG1 lands in the chunk the EG dimension is in.
    line(&mut s, [0.0, 20.0], [5.0, 20.0]);
    assert_eq!(dim(&s, d), before, "the guard does not touch objects the command left alone");
}

#[test]
fn opening_a_drawing_cuts_links_between_storeys() {
    let (mut s0, eg, og) = building();
    let d = cross_linked_dimension(&mut s0, eg, og);
    let drawing = s0.doc().unwrap().clone();
    let mut s = Session::new();
    s.open_drawing(drawing, "x", None);
    assert!(assoc(&s, d).is_empty());
}

#[test]
fn typed_layer_rename_reaches_every_storey() {
    let (mut s, eg, og) = building();
    s.execute("layer.new", &json!({ "name": "Old" })).unwrap();
    s.execute("laymch", &json!({ "handles": [og.hex()], "layer": "Old" })).unwrap();
    s.execute("storey.down", &json!({})).unwrap();
    s.execute("laymch", &json!({ "handles": [eg.hex()], "layer": "Old" })).unwrap();
    for line in ["-layer", "rename", "Old", "Walls", ""] {
        s.cmdline(line).unwrap();
    }
    assert!(s.running.is_none());
    let layer = |s: &Session, h| s.doc().unwrap().entity(h).unwrap().common.layer.clone();
    assert_eq!(layer(&s, og), "Walls", "renamed on OG1 too, not taken back");
    assert_eq!(layer(&s, eg), "Walls");
}

#[test]
fn storeycopy_with_a_handle_twice_copies_it_once() {
    let (mut s, _, og) = building();
    let n = s.doc().unwrap().model.len();
    s.execute("storeycopy", &json!({ "handles": [og.hex(), og.hex()], "to": "EG" })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.model.len(), n + 1);
    let mut hs: Vec<Handle> = d.model.iter().map(|e| e.handle).collect();
    hs.sort();
    hs.dedup();
    assert_eq!(hs.len(), d.model.len(), "no handle twice");
}

#[test]
fn the_last_storey_goes_with_its_objects_kept() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [1.0, 0.0]);
    s.execute("storey.new", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.delete", &json!({ "name": "EG" })).unwrap();
    let d = s.doc().unwrap();
    assert!(!d.storeys.active());
    assert!(d.entity(l).is_some_and(|e| e.common.storey().is_none() && d.is_visible(e)));
}

// ---------------- the ghost from the keyboard ----------------

#[test]
fn ghost_toggle_remembers_the_last_ghost_and_starts_below() {
    let (mut s, _, _) = building();
    s.execute("storey.new", &json!({ "name": "DG" })).unwrap();
    let ghost = |s: &Session| s.doc().unwrap().ghost_storey;
    // Nothing chosen yet: the storey below the current one (DG → OG1).
    s.execute("storey.ghost.toggle", &json!({})).unwrap();
    assert_eq!(ghost(&s), Some(id_of(&s, "OG1")));
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.ghost.toggle", &json!({})).unwrap();
    assert_eq!(ghost(&s), None);
    s.execute("storey.ghost.toggle", &json!({})).unwrap();
    assert_eq!(ghost(&s), Some(id_of(&s, "EG")), "the last ghost comes back");
    // From the bottom storey: the one above.
    s.execute("storey.ghost.toggle", &json!({})).unwrap();
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.ghost.toggle", &json!({})).unwrap();
    assert_ne!(ghost(&s), Some(id_of(&s, "EG")), "never the current storey");
    assert!(ghost(&s).is_some());
}

#[test]
fn ghost_steps_past_the_current_storey() {
    let (mut s, _, _) = building();
    s.execute("storey.new", &json!({ "name": "DG" })).unwrap();
    s.execute("storey.current", &json!({ "name": "OG1" })).unwrap();
    let ghost = |s: &Session| s.doc().unwrap().ghost_storey;
    s.execute("storey.ghost.down", &json!({})).unwrap();
    assert_eq!(ghost(&s), Some(id_of(&s, "EG")), "from no ghost: next to the current storey");
    s.execute("storey.ghost.up", &json!({})).unwrap();
    assert_eq!(ghost(&s), Some(id_of(&s, "DG")), "over OG1, the current storey");
    let r = s.execute("storey.ghost.up", &json!({})).unwrap();
    assert!(r["message"].as_str().unwrap().contains("top"));
    assert_eq!(ghost(&s), Some(id_of(&s, "DG")));
}

// ---------------- start setup and numbers ----------------

#[test]
fn new_and_foreign_drawings_get_the_setup_on_the_start_storey() {
    let mut s = Session::new();
    s.storey_template = Some(cadcraft_doc::StoreyTemplate::default());
    s.execute("new", &json!({ "metric": true })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.storeys.list.len(), 6);
    assert_eq!(d.storeys.current_storey().map(|x| x.name.as_str()), Some("EG"));
    assert!(!s.state().unwrap().is_dirty(), "a fresh drawing with its setup is not a change");
    // Another office's drawing, no storeys: everything on EG.
    let mut foreign = cadcraft_doc::Drawing::new_metric();
    let l = foreign
        .add(
            &Space::Model,
            cadcraft_doc::Common::default(),
            EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::ZERO, b: cadcraft_geom::Vec3::new(1.0, 0.0, 0.0) }),
        )
        .unwrap();
    s.open_drawing(foreign, "fremd", None);
    let d = s.doc().unwrap();
    assert_eq!(d.entity(l).and_then(|e| d.storey_of(e)), Some(d.storeys.home));
    assert_eq!(d.storeys.current, d.storeys.home);
}

#[test]
fn own_drawings_open_on_their_start_storey() {
    let mut s0 = Session::new();
    s0.storey_template = Some(cadcraft_doc::StoreyTemplate::default());
    s0.execute("new", &json!({})).unwrap();
    s0.execute("storey.current", &json!({ "name": "2" })).unwrap();
    assert_eq!(s0.doc().unwrap().storeys.current_storey().map(|x| x.name.as_str()), Some("2.OG"), "by number");
    let saved = s0.doc().unwrap().clone();
    let mut s = Session::new();
    s.open_drawing(saved, "eigen", None);
    assert_eq!(s.doc().unwrap().storeys.current_storey().map(|x| x.name.as_str()), Some("EG"));
}

#[test]
fn the_start_storey_is_number_zero() {
    let (mut s, _, _) = building();
    // The list runs bottom to top: EG, OG1.
    let r = s.execute("storey", &json!({})).unwrap();
    assert_eq!(r["storeys"][0]["number"], 0, "the bottom storey is 0 as long as no start is set");
    assert_eq!(r["storeys"][1]["number"], 1);
    s.execute("storey.home", &json!({ "name": "OG1" })).unwrap();
    let r = s.execute("storey", &json!({})).unwrap();
    assert_eq!(r["storeys"][1]["number"], 0);
    assert_eq!(r["storeys"][0]["number"], -1);
    assert_eq!(r["storeys"][1]["start"], true);
    s.execute("storey.current", &json!({ "name": "-1" })).unwrap();
    assert_eq!(s.doc().unwrap().storeys.current_storey().map(|x| x.name.as_str()), Some("EG"));
}

#[test]
fn storeys_from_setup_on_a_drawing_without_them() {
    let mut s = Session::new();
    let l = line(&mut s, [0.0, 0.0], [1.0, 0.0]);
    s.execute("storey.template", &json!({ "names": ["Drawing", "Blueprint 1", "Blueprint 2"], "start": "Drawing" })).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.storeys.list.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Blueprint 2", "Blueprint 1", "Drawing"]);
    assert_eq!(storey_of(&s, l), Some(id_of(&s, "Drawing")));
    assert!(s.execute("storey.template", &json!({})).is_err(), "not twice");
}

#[test]
fn blueprint_without_hatches_shows_and_snaps_hatches_of_the_current_storey_only() {
    let (mut s, _, _) = building();
    // A hatched rectangle on EG (its outline erased: only the hatch is left there).
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    let r = s.execute("rectang", &json!({ "p1": [20, 0], "p2": [30, 5] })).unwrap();
    let r = r["handle"].as_str().or_else(|| r["handles"][0].as_str()).unwrap().to_string();
    s.execute("hatch", &json!({ "handles": [r] })).unwrap();
    s.execute("erase", &json!({ "handles": [r] })).unwrap();
    let hatch = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).map(|e| e.handle).expect("a hatch");
    s.execute("storey.current", &json!({ "name": "OG1" })).unwrap();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    let modes = mode::END | mode::HATCH;
    let snaps = |s: &Session| snap::osnap(s.doc().unwrap(), &Space::Model, Vec2::new(30.05, 5.0), 0.5, modes, None, false).is_some();
    assert_eq!(view(&s, hatch), StoreyView::Ghost);
    assert!(snaps(&s), "with hatches, the blueprint's hatch outline snaps (OSNAPHATCH)");
    std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc).ghost_hatches = false;
    assert_eq!(view(&s, hatch), StoreyView::Hidden);
    assert!(!snaps(&s), "a hatch not shown is not snapped to");
    let list = cadcraft_render::build(s.doc().unwrap(), &Space::Model, &cadcraft_render::Options::default());
    assert!(list.prims.iter().all(|p| p.handle != hatch));
}

#[test]
fn blueprint_without_hatches_does_not_snap_hatches_inside_blocks_or_dimensions() {
    let (mut s, _, _) = building();
    // On EG: a hatch kept only in a block definition, shown through a dimension whose block is
    // that definition.
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    let r = h(&s.execute("rectang", &json!({ "p1": [20, 0], "p2": [30, 5] })).unwrap());
    s.execute("hatch", &json!({ "handles": [r.hex()] })).unwrap();
    s.execute("erase", &json!({ "handles": [r.hex()] })).unwrap();
    let hatch = s.doc().unwrap().model.iter().find(|e| matches!(e.kind, EntityKind::Hatch(_))).map(|e| e.handle).expect("a hatch");
    s.execute("block", &json!({ "name": "HB", "base": [0, 0], "handles": [hatch.hex()], "keep": "delete" })).unwrap();
    let dm = h(&s.execute("dimlinear", &json!({ "p1": [20, 0], "p2": [30, 0], "at": [25, 6] })).unwrap());
    let d = std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc);
    d.model.modify(dm, |e| {
        if let EntityKind::Dimension(x) = &mut e.kind {
            x.block = Some("HB".into());
            x.insertion_point = cadcraft_geom::Vec3::default();
        }
    });
    s.execute("storey.current", &json!({ "name": "OG1" })).unwrap();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    let modes = mode::END | mode::HATCH;
    let snaps = |s: &Session| snap::osnap(s.doc().unwrap(), &Space::Model, Vec2::new(30.05, 5.0), 0.5, modes, None, false).is_some();
    assert!(snaps(&s), "with hatches, the hatch in the dimension's block snaps");
    std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc).ghost_hatches = false;
    assert!(!snaps(&s), "a blueprint without hatches: not the hatch in a dimension's block");
}

#[test]
fn a_raster_image_on_the_blueprint_shows_in_the_ghost_colour() {
    use cadcraft_render::raster::{RasterOptions, View, render};
    let (mut s, _, _) = building();
    let eg = id_of(&s, "EG");
    // A white scan on EG, 10 x 10 units right of the lines.
    let d = std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc);
    let mut c = cadcraft_doc::Common::default();
    c.set_storey(Some(eg));
    let img = cadcraft_doc::Image {
        insert: cadcraft_geom::Vec3::new(20.0, 0.0, 0.0),
        u: cadcraft_geom::Vec3::new(10.0, 0.0, 0.0),
        v: cadcraft_geom::Vec3::new(0.0, 10.0, 0.0),
        size: Vec2::new(1.0, 1.0),
        path: "scan.png".into(),
        ..cadcraft_doc::Image::default()
    };
    d.add(&Space::Model, c, EntityKind::Image(img)).unwrap();
    d.rasters.insert("scan.png", std::sync::Arc::new(cadcraft_doc::Raster::new(1, 1, vec![255, 255, 255, 255]).unwrap()));
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    let d = s.doc().unwrap();
    let list = cadcraft_render::build(d, &Space::Model, &cadcraft_render::Options::default());
    assert_eq!(list.images.len(), 1, "the blueprint's image is drawn");
    assert_eq!(list.images[0].tint, Some(d.ghost_color), "on screen too (the canvas tints by it)");
    let view = View { center: Vec2::new(25.0, 5.0), scale: 10.0, width: 100, height: 100 };
    let pm = render(&list, &view, &RasterOptions { antialias: false, ..Default::default() }).unwrap();
    let p = pm.pixel(50, 50).unwrap();
    let g = d.ghost_color;
    assert!(
        [p.red(), p.green(), p.blue()].iter().zip([g.0, g.1, g.2]).all(|(a, b)| a.abs_diff(b) <= 2),
        "the white scan shows in the ghost colour, not white: {p:?}"
    );
}

#[test]
fn each_storey_brings_back_its_own_blueprint() {
    let mut s = Session::new();
    s.storey_template = Some(cadcraft_doc::StoreyTemplate::default());
    s.execute("new", &json!({})).unwrap();
    let ghost = |s: &Session| s.doc().unwrap().ghost_storey.and_then(|g| s.doc().unwrap().storeys.get(g)).map(|g| g.name.clone());
    s.execute("storey.current", &json!({ "name": "2.OG" })).unwrap();
    s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    assert_eq!(ghost(&s), None, "EG can't be its own blueprint, and it had none");
    s.execute("storey.ghost", &json!({ "name": "1.OG" })).unwrap();
    s.execute("storey.current", &json!({ "name": "2.OG" })).unwrap();
    assert_eq!(ghost(&s).as_deref(), Some("EG"), "2.OG brings back EG");
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    assert_eq!(ghost(&s).as_deref(), Some("1.OG"), "EG brings back 1.OG");
    // A storey not visited yet keeps the blueprint shown.
    s.execute("storey.current", &json!({ "name": "DG" })).unwrap();
    assert_eq!(ghost(&s).as_deref(), Some("1.OG"));
    // Off: the blueprint stays as it is.
    std::sync::Arc::make_mut(&mut s.state_mut().unwrap().doc).ghost_per_storey = false;
    s.execute("storey.current", &json!({ "name": "2.OG" })).unwrap();
    assert_eq!(ghost(&s).as_deref(), Some("1.OG"));
}

#[test]
fn going_to_a_storey_or_a_blueprint_is_no_change() {
    let (mut s, _, _) = building();
    s.state_mut().unwrap().saved = s.state().unwrap().doc.clone();
    assert!(!s.state().unwrap().is_dirty());
    s.execute("storey.current", &json!({ "name": "EG" })).unwrap();
    s.execute("storey.ghost", &json!({ "name": "OG1" })).unwrap();
    s.execute("storey.ghost.toggle", &json!({})).unwrap();
    s.execute("storey.up", &json!({})).unwrap();
    assert!(!s.state().unwrap().is_dirty(), "only the view changed");
    // A real change still counts, and stays counted across a switch.
    line(&mut s, [0.0, 9.0], [1.0, 9.0]);
    s.execute("storey.down", &json!({})).unwrap();
    assert!(s.state().unwrap().is_dirty());
}
