//! A block reference on a layer the active MSPACE viewport draws, with parts on a layer frozen in
//! that viewport ("VP Freeze"): the viewport does not draw those parts, so a selection made
//! through it does not hit the reference on them, and they take no room in the box a window
//! tests. AutoCAD 2022 was given the same objects and answers the cases asserted on them one by
//! one the same way through a viewport. Where it differs is what is taken through the rectangle
//! around a reference (#2991): the grid test only compares that between a layer frozen in the
//! viewport and one frozen for the whole drawing. In a viewport that does not freeze the layer
//! and on the Model tab everything is as before.

use cadcraft_engine::Session;
use cadcraft_engine::doc::Handle;
use cadcraft_engine::geom::{Bounds2, Vec2};
use cadcraft_engine::select;
use serde_json::json;

/// Draws a line from `a` to `b` on layer 0, or on "Hidden" when `hidden`, and returns its handle.
fn line(s: &mut Session, a: [f64; 2], b: [f64; 2], hidden: bool) -> Handle {
    let r = s.execute("line", &json!({ "points": [a, b] })).unwrap();
    let h = Handle::parse_hex(r["handles"][0].as_str().unwrap()).unwrap();
    if hidden {
        to_hidden(s, h);
    }
    h
}

/// Puts `h` on the layer "Hidden".
fn to_hidden(s: &mut Session, h: Handle) {
    s.execute("properties.set", &json!({ "handles": [h.hex()], "layer": "Hidden" })).unwrap();
    assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "Hidden");
}

/// Makes the block `name` of the objects `parts`, with its base at `base`, and returns the
/// reference to it that takes their place, on layer 0.
fn block(s: &mut Session, name: &str, base: [f64; 2], parts: &[Handle]) -> Handle {
    let hs: Vec<String> = parts.iter().map(|h| h.hex()).collect();
    let r = s.execute("block", &json!({ "name": name, "base": base, "handles": hs })).unwrap();
    let h = Handle::parse_hex(r["insert"].as_str().unwrap()).unwrap();
    assert_eq!(s.doc().unwrap().entity(h).unwrap().common.layer, "0");
    h
}

/// Inserts the block `name` at `at` and returns the reference.
fn insert(s: &mut Session, name: &str, at: [f64; 2]) -> Handle {
    let r = s.execute("insert", &json!({ "name": name, "at": at })).unwrap();
    Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap()
}

/// Makes `viewport` the active MSPACE viewport.
fn enter(s: &mut Session, viewport: &str) {
    s.execute("mspace", &json!({ "handle": viewport })).unwrap();
}

/// `hs` in handle order.
fn sorted(mut hs: Vec<Handle>) -> Vec<Handle> {
    hs.sort_by_key(|h| h.0);
    hs
}

/// Five block references on layer 0 and two viewports, as AutoCAD was given them.
struct Parts {
    /// The session, on the layout that holds the two viewports, in paper space.
    s: Session,
    /// One line on layer 0 from (0, 0) to (10, 0) and one on "Hidden" from (0, 5) to (10, 5).
    r: Handle,
    /// One line, on "Hidden", from (20, 5) to (30, 5): nothing of it is drawn where the layer is
    /// frozen. The last object made.
    h: Handle,
    /// A reference to the block of `r` nested in another block: its lines run from (40, 0) to
    /// (50, 0) on layer 0 and from (40, 5) to (50, 5) on "Hidden".
    n: Handle,
    /// A block of a reference to the block of `r` that is itself on "Hidden" (lines from (60, 0)
    /// to (70, 0) and from (60, 5) to (70, 5), neither drawn where the layer is frozen) and of a
    /// line on layer 0 from (60, 10) to (70, 10).
    z: Handle,
    /// Two lines on layer 0, from (80, 0) to (90, 0) and from (80, 10) to (90, 10), and between
    /// them one on "Hidden" from (80, 5) to (90, 5): a part that is not drawn where the layer is
    /// frozen, inside the box around the parts that are.
    q: Handle,
    /// The viewport that freezes "Hidden".
    a: String,
    /// The viewport that freezes nothing.
    b: String,
}

/// The drawing of [`Parts`], in paper space. With `crowded`, model space also holds enough lines,
/// far from the references, for selections to go through the spatial index instead of scanning
/// the space.
fn parts(crowded: bool) -> Parts {
    let mut s = Session::new();
    s.execute("layer.new", &json!({ "name": "Hidden" })).unwrap();
    let on_0 = line(&mut s, [0.0, 0.0], [10.0, 0.0], false);
    let on_hidden = line(&mut s, [0.0, 5.0], [10.0, 5.0], true);
    let r = block(&mut s, "BP", [0.0, 0.0], &[on_0, on_hidden]);
    let nested = insert(&mut s, "BP", [40.0, 0.0]);
    let n = block(&mut s, "BN", [40.0, 0.0], &[nested]);
    let child = insert(&mut s, "BP", [60.0, 0.0]);
    to_hidden(&mut s, child);
    let own = line(&mut s, [60.0, 10.0], [70.0, 10.0], false);
    let z = block(&mut s, "BZ", [60.0, 0.0], &[child, own]);
    let low = line(&mut s, [80.0, 0.0], [90.0, 0.0], false);
    let high = line(&mut s, [80.0, 10.0], [90.0, 10.0], false);
    let between = line(&mut s, [80.0, 5.0], [90.0, 5.0], true);
    let q = block(&mut s, "BQ", [80.0, 0.0], &[low, high, between]);
    if crowded {
        let far: Vec<[f64; 2]> = (0..=cadcraft_engine::spatial::MIN_INDEXED).map(|i| [1000.0 + i as f64, 1000.0 + (i % 2) as f64]).collect();
        s.execute("line", &json!({ "points": far })).unwrap();
    }
    let only_hidden = line(&mut s, [20.0, 5.0], [30.0, 5.0], true);
    let h = block(&mut s, "BH", [20.0, 5.0], &[only_hidden]);
    assert_eq!(s.doc().unwrap().model.len() >= cadcraft_engine::spatial::MIN_INDEXED, crowded, "whether model space is indexed");
    s.execute("layout.set", &json!({ "name": "Layout1" })).unwrap();
    let v = s.execute("mview", &json!({ "count": 2, "p1": [20, 20], "p2": [220, 120] })).unwrap();
    let (a, b) = (v["viewports"][0].as_str().unwrap().to_string(), v["viewports"][1].as_str().unwrap().to_string());
    s.execute("vplayer", &json!({ "handles": [a], "freeze": ["Hidden"] })).unwrap();
    Parts { s, r, h, n, z, q, a, b }
}

/// What a pick at `p` finds where the session is: through the active viewport, or on its tab.
fn pick(s: &Session, p: [f64; 2]) -> Option<Handle> {
    let st = s.state().unwrap();
    select::pick_in(&st.doc, &st.edit_space(), st.viewport_frozen(), Vec2::new(p[0], p[1]), 0.2)
}

/// Every object a pick at `p` finds where the session is, as selection cycling lists them.
fn pick_all(s: &Session, p: [f64; 2]) -> Vec<Handle> {
    let st = s.state().unwrap();
    sorted(select::pick_all_in(&st.doc, &st.edit_space(), st.viewport_frozen(), Vec2::new(p[0], p[1]), 0.2))
}

/// What a window from `a` to `b` takes where the session is (a crossing window with `crossing`).
fn window(s: &Session, a: [f64; 2], b: [f64; 2], crossing: bool) -> Vec<Handle> {
    let st = s.state().unwrap();
    let bx = Bounds2::new(Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
    sorted(select::select_window_in(&st.doc, &st.edit_space(), st.viewport_frozen(), bx, crossing))
}

/// The objects of the selection set the AutoLISP `code` returns (none for nil), in handle order.
fn lisp_set(s: &mut Session, code: &str) -> Vec<Handle> {
    use cadcraft_engine::lisp::value::Value as Lisp;
    match s.lisp_eval(code, false, true, Vec::new()).unwrap() {
        Some(Ok(Lisp::PickSet(set))) => sorted(set.items.lock().unwrap().clone()),
        Some(Ok(Lisp::Nil)) => Vec::new(),
        other => panic!("`{code}` returned {other:?}"),
    }
}

/// What a fence along `pts` takes where the session is.
fn fence(s: &Session, pts: &[[f64; 2]]) -> Vec<Handle> {
    let st = s.state().unwrap();
    let pts: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    sorted(select::select_fence_in(&st.doc, &st.edit_space(), st.viewport_frozen(), &pts))
}

/// What a window polygon through `pts` takes where the session is (a crossing polygon with
/// `crossing`).
fn polygon(s: &Session, pts: &[[f64; 2]], crossing: bool) -> Vec<Handle> {
    let st = s.state().unwrap();
    let pts: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    sorted(select::select_polygon_in(&st.doc, &st.edit_space(), st.viewport_frozen(), &pts, crossing))
}

/// A pick, selection cycling's list, a crossing window, a fence and a crossing polygon on the part
/// the viewport does not draw find nothing; neither does a pick on the side of the box around both parts, nor a
/// crossing window in the empty middle of that box, where the viewport draws nothing of the
/// reference. The part it draws is hit as before, and the other viewport hits both parts. The
/// same with and without the spatial index.
#[test]
fn a_reference_is_not_hit_on_a_part_the_viewport_does_not_draw() {
    for crowded in [false, true] {
        let Parts { mut s, r, a, b, .. } = parts(crowded);
        let around = [[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]];
        enter(&mut s, &a);
        assert_eq!(pick(&s, [5.0, 5.0]), None, "a pick on the part that is not drawn (indexed: {crowded})");
        assert_eq!(pick_all(&s, [5.0, 5.0]), [], "selection cycling's list there (indexed: {crowded})");
        assert_eq!(window(&s, [4.0, 4.0], [6.0, 6.0], true), [], "a crossing window over it (indexed: {crowded})");
        assert_eq!(fence(&s, &[[5.0, 4.0], [5.0, 6.0]]), [], "a fence across it (indexed: {crowded})");
        assert_eq!(polygon(&s, &around, true), [], "a crossing polygon over it (indexed: {crowded})");
        assert_eq!(pick(&s, [0.0, 2.5]), None, "a pick on the side of the box around both parts (indexed: {crowded})");
        assert_eq!(window(&s, [4.0, 2.0], [6.0, 3.0], true), [], "a crossing window between the parts (indexed: {crowded})");
        assert_eq!(pick(&s, [5.0, 0.0]), Some(r), "the part that is drawn (indexed: {crowded})");
        enter(&mut s, &b);
        assert_eq!(pick(&s, [5.0, 5.0]), Some(r), "this viewport draws the part (indexed: {crowded})");
        assert_eq!(pick_all(&s, [5.0, 5.0]), [r]);
        assert_eq!(window(&s, [4.0, 4.0], [6.0, 6.0], true), [r]);
        assert_eq!(fence(&s, &[[5.0, 4.0], [5.0, 6.0]]), [r]);
        assert_eq!(polygon(&s, &around, true), [r]);
    }
}

/// A window and a window polygon around the drawn part alone take the reference: the part that
/// is not drawn takes no room in its box. A window around that part alone takes nothing. The
/// other viewport needs both parts inside the window, as before.
#[test]
fn a_window_takes_a_reference_by_the_parts_the_viewport_draws() {
    for crowded in [false, true] {
        let Parts { mut s, r, a, b, .. } = parts(crowded);
        let around = [[-1.0, -1.0], [11.0, -1.0], [11.0, 1.0], [-1.0, 1.0]];
        enter(&mut s, &a);
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), [r], "around the drawn part alone (indexed: {crowded})");
        assert_eq!(polygon(&s, &around, false), [r], "a window polygon around the drawn part alone (indexed: {crowded})");
        assert_eq!(window(&s, [-1.0, 4.0], [11.0, 6.0], false), [], "around the part that is not drawn (indexed: {crowded})");
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 6.0], false), [r], "around both (indexed: {crowded})");
        enter(&mut s, &b);
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), [], "this viewport draws both parts (indexed: {crowded})");
        assert_eq!(polygon(&s, &around, false), []);
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 6.0], false), [r]);
    }
}

/// A reference whose every part is on the frozen layer has nothing drawn in the viewport: no
/// pick, window, crossing window or fence finds it there, and a window around it and three others
/// takes those three. It is still on a layer the viewport draws, so Select All takes it, and
/// Last takes such a reference when it is the last object made, as AutoCAD's All and Last do. The
/// other viewport finds it.
#[test]
fn a_reference_with_nothing_drawn_in_the_viewport_is_not_selected_there() {
    for crowded in [false, true] {
        let Parts { mut s, r, h, n, z, a, b, .. } = parts(crowded);
        enter(&mut s, &a);
        assert_eq!(pick(&s, [25.0, 5.0]), None, "a pick on its line (indexed: {crowded})");
        assert_eq!(window(&s, [24.0, 4.0], [26.0, 6.0], true), [], "a crossing window over its line (indexed: {crowded})");
        assert_eq!(fence(&s, &[[25.0, 4.0], [25.0, 6.0]]), [], "a fence across its line (indexed: {crowded})");
        assert_eq!(window(&s, [19.0, 4.0], [31.0, 6.0], false), [], "a window around it (indexed: {crowded})");
        assert_eq!(window(&s, [-2.0, -2.0], [72.0, 12.0], false), sorted(vec![r, n, z]), "a window around the four (indexed: {crowded})");
        assert_eq!(window(&s, [-2.0, -2.0], [72.0, 12.0], true), sorted(vec![r, n, z]), "a crossing window over the four (indexed: {crowded})");
        s.execute("selectall", &json!({})).unwrap();
        assert!(s.selection().contains(&h), "its own layer is drawn: Select All takes it (indexed: {crowded})");
        s.set_selection(Vec::new());
        // Another reference to that block, made through this viewport: the last object made.
        let last = insert(&mut s, "BH", [20.0, 15.0]);
        assert_eq!(pick(&s, [25.0, 15.0]), None, "nothing of it is drawn here either (indexed: {crowded})");
        assert_eq!(lisp_set(&mut s, "(ssget \"L\")"), [last], "its own layer is drawn: Last takes it (indexed: {crowded})");
        enter(&mut s, &b);
        assert_eq!(pick(&s, [25.0, 5.0]), Some(h), "this viewport draws it (indexed: {crowded})");
        assert_eq!(window(&s, [19.0, 4.0], [31.0, 6.0], false), [h]);
        assert_eq!(window(&s, [-2.0, -2.0], [72.0, 12.0], false), sorted(vec![r, h, n, z]));
    }
}

/// The same inside a nested reference: a part on the frozen layer two blocks deep is not hit and
/// takes no room, and a nested reference that is itself on the frozen layer brings none of its
/// lines, whatever their layers, while the rest of the block around it is hit and measured.
#[test]
fn a_nested_reference_follows_the_viewport_too() {
    for crowded in [false, true] {
        let Parts { mut s, n, z, a, b, .. } = parts(crowded);
        enter(&mut s, &a);
        assert_eq!(pick(&s, [45.0, 5.0]), None, "the nested part that is not drawn (indexed: {crowded})");
        assert_eq!(window(&s, [44.0, 4.0], [46.0, 6.0], true), [], "a crossing window over it (indexed: {crowded})");
        assert_eq!(pick(&s, [45.0, 0.0]), Some(n), "the nested part that is drawn (indexed: {crowded})");
        assert_eq!(window(&s, [39.0, -1.0], [51.0, 1.0], false), [n], "a window around the drawn nested part (indexed: {crowded})");
        assert_eq!(pick(&s, [65.0, 0.0]), None, "the layer 0 line of a nested reference on the frozen layer (indexed: {crowded})");
        assert_eq!(pick(&s, [65.0, 5.0]), None, "its other line (indexed: {crowded})");
        assert_eq!(window(&s, [64.0, -1.0], [66.0, 6.0], true), [], "a crossing window over that nested reference (indexed: {crowded})");
        assert_eq!(pick(&s, [65.0, 10.0]), Some(z), "the line beside it in the block (indexed: {crowded})");
        assert_eq!(window(&s, [59.0, 9.0], [71.0, 11.0], false), [z], "a window around that line alone (indexed: {crowded})");
        assert_eq!(window(&s, [59.0, -1.0], [71.0, 11.0], false), [z], "a window around the whole block (indexed: {crowded})");
        enter(&mut s, &b);
        assert_eq!(pick(&s, [45.0, 5.0]), Some(n), "this viewport draws every part (indexed: {crowded})");
        assert_eq!(window(&s, [39.0, -1.0], [51.0, 1.0], false), []);
        assert_eq!(pick(&s, [65.0, 0.0]), Some(z));
        assert_eq!(window(&s, [59.0, 9.0], [71.0, 11.0], false), []);
    }
}

/// A part that is not drawn can lie inside the box around the parts that are. A pick, selection
/// cycling's list and a fence on it find nothing there, although they are within that box; the drawn parts are hit, and a
/// window takes the reference when it holds both of them, not one alone. The hit geometry is what
/// decides here, not the box: without the viewport's layers in it the part is hit.
#[test]
fn a_part_that_is_not_drawn_is_not_hit_inside_the_box_of_the_drawn_parts() {
    for crowded in [false, true] {
        let Parts { mut s, q, a, b, .. } = parts(crowded);
        enter(&mut s, &a);
        assert_eq!(pick(&s, [85.0, 5.0]), None, "a pick on the part that is not drawn (indexed: {crowded})");
        assert_eq!(pick_all(&s, [85.0, 5.0]), [], "selection cycling's list there (indexed: {crowded})");
        assert_eq!(fence(&s, &[[85.0, 4.0], [85.0, 6.0]]), [], "a fence across it (indexed: {crowded})");
        assert_eq!(pick(&s, [85.0, 10.0]), Some(q), "a drawn part (indexed: {crowded})");
        assert_eq!(window(&s, [79.0, -1.0], [91.0, 11.0], false), [q], "a window around both drawn parts (indexed: {crowded})");
        assert_eq!(window(&s, [79.0, -1.0], [91.0, 1.0], false), [], "a window around one of them (indexed: {crowded})");
        enter(&mut s, &b);
        assert_eq!(pick(&s, [85.0, 5.0]), Some(q), "this viewport draws the part (indexed: {crowded})");
        assert_eq!(pick_all(&s, [85.0, 5.0]), [q]);
        assert_eq!(fence(&s, &[[85.0, 4.0], [85.0, 6.0]]), [q]);
    }
}

/// Freezing the layer in the viewport the session is in, and thawing it again, changes what the
/// next pick and the next window find: nothing is kept from before the change. With the spatial
/// index too, which is not rebuilt for a layer frozen in a viewport.
#[test]
fn freezing_and_thawing_the_layer_in_the_viewport_changes_what_is_hit_at_once() {
    for crowded in [false, true] {
        let Parts { mut s, r, b, .. } = parts(crowded);
        enter(&mut s, &b);
        assert_eq!(pick(&s, [5.0, 5.0]), Some(r), "drawn here (indexed: {crowded})");
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), []);
        s.execute("vplayer", &json!({ "handles": [b], "freeze": ["Hidden"] })).unwrap();
        assert_eq!(pick(&s, [5.0, 5.0]), None, "frozen in this viewport now (indexed: {crowded})");
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), [r], "the part takes no room any more (indexed: {crowded})");
        s.execute("vplayer", &json!({ "handles": [b], "thaw": ["Hidden"] })).unwrap();
        assert_eq!(pick(&s, [5.0, 5.0]), Some(r), "thawed again (indexed: {crowded})");
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), [], "and it takes its room again (indexed: {crowded})");
    }
}

/// What a user does goes through the same tests. A click on the part the viewport does not draw
/// is a click on nothing: it starts a selection window. A window dragged around the drawn part
/// alone takes the reference, and so does a click on that part. The `select` command's pick and
/// window answer the same way.
#[test]
fn clicks_and_the_select_command_see_the_parts_the_viewport_draws() {
    for crowded in [false, true] {
        let Parts { mut s, r, a, .. } = parts(crowded);
        enter(&mut s, &a);
        s.idle_click(Vec2::new(5.0, 5.0), false).unwrap();
        assert!(s.pending_window.is_some(), "a click on the part that is not drawn must be a click on nothing (indexed: {crowded})");
        // The other corner of that window, left to right around nothing.
        s.idle_click(Vec2::new(6.0, 6.0), false).unwrap();
        assert_eq!(s.selection(), [], "a click on the part that is not drawn (indexed: {crowded})");
        s.idle_click(Vec2::new(-1.0, -1.0), false).unwrap();
        s.idle_click(Vec2::new(11.0, 1.0), false).unwrap();
        assert_eq!(s.selection(), [r], "a window dragged around the drawn part alone (indexed: {crowded})");
        s.set_selection(Vec::new());
        s.idle_click(Vec2::new(5.0, 0.0), false).unwrap();
        assert_eq!(s.selection(), [r], "a click on the drawn part (indexed: {crowded})");
        s.set_selection(Vec::new());
        s.execute("select", &json!({ "at": [5, 5], "aperture": 0.5 })).unwrap();
        assert_eq!(s.selection(), [], "select at a point on the part that is not drawn (indexed: {crowded})");
        s.execute("select", &json!({ "window": [[-1, -1], [11, 1]] })).unwrap();
        assert_eq!(s.selection(), [r], "select by a window around the drawn part (indexed: {crowded})");
    }
}

/// A clipped reference (XCLIP) with every part on the frozen layer is still selected by its clip
/// boundary, as AutoCAD 2022 selects it: a pick, a crossing window and a fence on the boundary
/// and a window around it take the reference, a pick on the part that is not drawn does not, and
/// a window must hold the whole boundary. Hiding the frames (XCLIPFRAME 0) changes none of that.
/// The viewport draws the frame and nothing else of the reference. The viewport that draws the
/// layer selects it on its part too.
#[test]
fn a_clipped_reference_with_no_part_drawn_is_selected_by_its_boundary() {
    /// [`parts`] with one more reference to the block of `h`, at (20, 20), where its line runs
    /// from (20, 20) to (30, 20), clipped to the rectangle from (18, 18) to (32, 22). The session
    /// is left in the viewport that freezes nothing.
    fn clipped(crowded: bool) -> (Parts, Handle) {
        let mut p = parts(crowded);
        enter(&mut p.s, &p.b);
        let c = insert(&mut p.s, "BH", [20.0, 20.0]);
        p.s.execute("xclip", &json!({ "handle": c.hex(), "rectangle": [[18, 18], [32, 22]] })).unwrap();
        (p, c)
    }
    /// What six selections find: a pick on the reference's line, a pick on its boundary, a
    /// crossing window and a fence over the boundary, a window around all of it, and a window
    /// around the line alone, inside the boundary.
    fn found(s: &Session) -> [Vec<Handle>; 6] {
        [
            pick(s, [25.0, 20.0]).into_iter().collect(),
            pick(s, [18.0, 20.0]).into_iter().collect(),
            window(s, [17.0, 19.0], [19.0, 21.0], true),
            fence(s, &[[17.0, 20.0], [19.0, 20.0]]),
            window(s, [15.0, 15.0], [35.0, 25.0], false),
            window(s, [19.0, 19.0], [31.0, 21.0], false),
        ]
    }
    /// Whether `v` lies on a side of the clip rectangle.
    fn on_the_frame(v: Vec2) -> bool {
        [v.x - 18.0, v.x - 32.0, v.y - 18.0, v.y - 22.0].iter().any(|d| d.abs() < 1e-9)
    }
    for crowded in [false, true] {
        let (Parts { mut s, a, .. }, c) = clipped(crowded);
        assert_eq!(found(&s), [vec![c], vec![c], vec![c], vec![c], vec![c], vec![]], "where the layer is drawn (indexed: {crowded})");
        enter(&mut s, &a);
        let by_its_boundary = [vec![], vec![c], vec![c], vec![c], vec![c], vec![]];
        assert_eq!(found(&s), by_its_boundary, "through the viewport that freezes the layer (indexed: {crowded})");
        // That viewport draws the frame, and only the frame.
        {
            let st = s.state().unwrap();
            let reference = st.doc.entity(c).unwrap();
            let drawn = cadcraft_engine::render::build_entities_in_viewport(
                &st.doc,
                std::iter::once(&**reference),
                &Default::default(),
                st.viewport_frozen(),
            );
            assert!(!drawn.verts.is_empty(), "the frame is drawn (indexed: {crowded})");
            assert!(drawn.verts.iter().all(|v| on_the_frame(Vec2::new(v.x, v.y))), "nothing but the frame is drawn (indexed: {crowded})");
        }
        // With the frames hidden the boundary selects all the same.
        s.execute("xclipframe", &json!({ "value": 0 })).unwrap();
        assert_eq!(found(&s), by_its_boundary, "with the frames hidden (indexed: {crowded})");
    }
}

/// A clipped reference with a part on each layer, the clip showing a piece of both, with the
/// coordinates AutoCAD 2022 was given (here 100 to the right) and its answers. Through the
/// viewport the piece on the frozen layer is not hit; the drawn piece and the boundary are, the
/// part of the block the clip cuts away is not, and a window must hold the whole boundary.
#[test]
fn a_clipped_reference_with_a_part_drawn_is_selected_by_its_boundary_and_that_part() {
    for crowded in [false, true] {
        let Parts { mut s, a, b, .. } = parts(crowded);
        enter(&mut s, &b);
        // A line on layer 0 from (130, 30) to (160, 30) and one on "Hidden" 4 above it, clipped to
        // the rectangle from (138, 27) to (152, 37).
        let on_0 = line(&mut s, [130.0, 30.0], [160.0, 30.0], false);
        let on_hidden = line(&mut s, [130.0, 34.0], [160.0, 34.0], true);
        let c = block(&mut s, "BK", [130.0, 30.0], &[on_0, on_hidden]);
        s.execute("xclip", &json!({ "handle": c.hex(), "rectangle": [[138, 27], [152, 37]] })).unwrap();
        assert_eq!(pick(&s, [145.0, 34.0]), Some(c), "where the layer is drawn (indexed: {crowded})");
        enter(&mut s, &a);
        assert_eq!(pick(&s, [145.0, 34.0]), None, "the piece that is not drawn (indexed: {crowded})");
        assert_eq!(pick(&s, [145.0, 30.0]), Some(c), "the drawn piece (indexed: {crowded})");
        assert_eq!(pick(&s, [133.0, 30.0]), None, "the layer 0 part where the clip cuts it away (indexed: {crowded})");
        assert_eq!(pick(&s, [145.0, 27.0]), Some(c), "the bottom side of the boundary (indexed: {crowded})");
        assert_eq!(pick(&s, [145.0, 37.0]), Some(c), "the top side of the boundary (indexed: {crowded})");
        assert_eq!(window(&s, [144.0, 26.0], [146.0, 28.0], true), [c], "a crossing window over the bottom side (indexed: {crowded})");
        assert_eq!(window(&s, [137.0, 26.0], [153.0, 38.0], false), [c], "a window around the boundary (indexed: {crowded})");
        assert_eq!(window(&s, [137.5, 29.0], [152.5, 31.0], false), [], "a window around the drawn piece alone (indexed: {crowded})");
        assert_eq!(window(&s, [137.0, 26.0], [153.0, 33.0], false), [], "a window around the lower half of the boundary (indexed: {crowded})");
        assert_eq!(window(&s, [128.0, 26.0], [162.0, 38.0], false), [c], "a window around the whole block (indexed: {crowded})");
    }
}

/// A reference with an inverted clip keeps the box the drawing gives it as well, so its frame,
/// which the viewport draws, here beside a part it does not draw, is still hit there: by a pick
/// and by a fence, neither of which comes near a side of that box. AutoCAD 2022 selects such a
/// reference by its frame too, and answers the pick on the drawn part and the two windows the
/// same way. The shape is the one AutoCAD was given, 80 to the right. This test passes without
/// the change as well: it holds what the change must leave alone, and fails when a clipped
/// reference is measured through the viewport like any other.
#[test]
fn an_inverted_clip_is_hit_on_its_frame_through_the_viewport() {
    for crowded in [false, true] {
        let Parts { mut s, a, b, .. } = parts(crowded);
        enter(&mut s, &b);
        // The block of `r` at (100, 20): a line on layer 0 from (100, 20) to (110, 20) and one on
        // "Hidden" 5 above it. The clip hides what is inside the rectangle from (103, 23) to
        // (107, 27), so the bottom side of its frame runs between the two lines.
        let c = insert(&mut s, "BP", [100.0, 20.0]);
        s.execute("xclip", &json!({ "handle": c.hex(), "rectangle": [[103, 23], [107, 27]], "invert": true })).unwrap();
        assert_eq!(pick(&s, [105.0, 23.0]), Some(c), "where the layer is drawn (indexed: {crowded})");
        enter(&mut s, &a);
        assert_eq!(pick(&s, [105.0, 23.0]), Some(c), "the bottom side of the frame (indexed: {crowded})");
        assert_eq!(fence(&s, &[[105.0, 22.5], [105.0, 23.5]]), [c], "a fence across it (indexed: {crowded})");
        assert_eq!(pick(&s, [101.0, 20.0]), Some(c), "the layer 0 part outside the boundary (indexed: {crowded})");
        assert_eq!(window(&s, [99.0, 19.0], [111.0, 21.0], false), [], "a window around the drawn part alone (indexed: {crowded})");
        assert_eq!(window(&s, [99.0, 19.0], [111.0, 28.5], false), [c], "a window around both parts and the frame (indexed: {crowded})");
    }
}

/// What a pick at `p` finds in the model space of `d` through a viewport that freezes the layers
/// in `frozen`, or on the Model tab with no layer.
fn picked(d: &cadcraft_engine::doc::Drawing, frozen: &[String], p: [f64; 2]) -> Option<Handle> {
    select::pick_in(d, &cadcraft_engine::doc::Space::Model, frozen, Vec2::new(p[0], p[1]), 0.2)
}

/// What a window from `a` to `b` takes there ([`picked`]).
fn windowed(d: &cadcraft_engine::doc::Drawing, frozen: &[String], a: [f64; 2], b: [f64; 2]) -> Vec<Handle> {
    let area = Bounds2::new(Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
    select::select_window_in(d, &cadcraft_engine::doc::Space::Model, frozen, area, false)
}

/// What a crossing window from `a` to `b` takes there ([`picked`]).
fn crossed(d: &cadcraft_engine::doc::Drawing, frozen: &[String], a: [f64; 2], b: [f64; 2]) -> Vec<Handle> {
    let area = Bounds2::new(Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1]));
    select::select_window_in(d, &cadcraft_engine::doc::Space::Model, frozen, area, true)
}

/// What a fence from `a` to `b` takes there ([`picked`]).
fn fenced(d: &cadcraft_engine::doc::Drawing, frozen: &[String], a: [f64; 2], b: [f64; 2]) -> Vec<Handle> {
    select::select_fence_in(d, &cadcraft_engine::doc::Space::Model, frozen, &[Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1])])
}

/// Other objects that draw a block, built by hand in model space with the coordinates AutoCAD
/// 2022 was given.
struct BlockDrawers {
    /// The drawing that holds them.
    d: cadcraft_engine::doc::Drawing,
    /// A 2 x 2 array of references (MINSERT) from (0, -30), its rows 12 and its columns 15 apart.
    array: Handle,
    /// A reference at (70, 30) with an attribute on "Hidden" 4 above its line.
    tagged: Handle,
    /// An annotative reference at the origin, at the drawing's annotation scale.
    scaled: Handle,
    /// A multileader from (110, -20) to (130, 0) whose content is a block (here 100 to the
    /// right of where AutoCAD had it).
    leader: Handle,
    /// A dimension along (150, -10) to (160, -10) whose block nests a reference, as the block of
    /// a dimension with an arrowhead block does in AutoCAD.
    dim: Handle,
}

/// The drawing of [`BlockDrawers`]: every block has a line on layer 0 and, above it, one on
/// "Hidden", but for the one with the attribute, where the attribute is on "Hidden".
fn block_drawers() -> BlockDrawers {
    use cadcraft_engine::doc::{
        AnnoScale, Attrib, AttribProps, Block, Common, DimKind, Dimension, Drawing, Entity, EntityKind, HAlign, Insert, Line, MLeader, Space, Text,
        VAlign,
    };
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Hidden");
    let line = |handle: u64, a: [f64; 2], b: [f64; 2], layer: &str| {
        let mut part = Entity::new(Handle(handle), EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) }));
        part.common.layer = layer.into();
        part
    };
    let reference = |block: &str, at: [f64; 2]| Insert { block: block.into(), insert: Vec3::new(at[0], at[1], 0.0), ..Insert::default() };
    let mut define = |name: &str, parts: Vec<Entity>| {
        let mut block = Block::new(name);
        for part in parts {
            block.entities.push(part);
        }
        d.blocks.insert(name.into(), std::sync::Arc::new(block));
    };
    // BP: a line on layer 0 and, 5 above it, one on "Hidden". BM: the same, 8 apart. BA: one line.
    define("BP", vec![line(1, [0.0, 0.0], [10.0, 0.0], "0"), line(2, [0.0, 5.0], [10.0, 5.0], "Hidden")]);
    define("BM", vec![line(1, [0.0, 0.0], [6.0, 0.0], "0"), line(2, [0.0, 8.0], [6.0, 8.0], "Hidden")]);
    define("BA", vec![line(1, [0.0, 0.0], [10.0, 0.0], "0")]);
    // *D9: what a dimension draws, here a reference to BP at (150, 0) and a line of its own.
    let mut nested = Entity::new(Handle(1), EntityKind::Insert(reference("BP", [150.0, 0.0])));
    nested.common.layer = "0".into();
    define("*D9", vec![nested, line(2, [150.0, -10.0], [160.0, -10.0], "0")]);
    let model = Space::Model;
    // A 2 x 2 array of BP from (0, -30), its rows 12 and its columns 15 apart.
    let grid = Insert { cols: 2, rows: 2, col_spacing: 15.0, row_spacing: 12.0, ..reference("BP", [0.0, -30.0]) };
    let array = d.add(&model, Common::default(), EntityKind::Insert(grid)).unwrap();
    // BA at (70, 30) with an attribute on "Hidden" 4 above its line.
    let value = Text {
        insert: Vec3::new(70.0, 34.0, 0.0),
        align_pt: None,
        height: 1.5,
        value: "HHHH".into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    };
    let attribute = Attrib {
        tag: "TAG".into(),
        text: value,
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: AttribProps { layer: Some("Hidden".into()), ..Default::default() },
        multiline: None,
        modes: Default::default(),
    };
    let tagged = d.add(&model, Common::default(), EntityKind::Insert(Insert { attribs: vec![attribute], ..reference("BA", [70.0, 30.0]) })).unwrap();
    // An annotative reference to BP at the origin, at the drawing's annotation scale.
    let annotative = Common { anno_scales: vec![AnnoScale::unit()], ..Common::default() };
    let scaled = d.add(&model, annotative, EntityKind::Insert(reference("BP", [0.0, 0.0]))).unwrap();
    // A multileader from (110, -20) to (130, 0) with BM at (127.36, -4), where AutoCAD put it.
    let leader = MLeader {
        leaders: vec![vec![Vec3::new(110.0, -20.0, 0.0)]],
        landing: Vec3::new(130.0, 0.0, 0.0),
        dogleg: 0.36,
        text: None,
        style: "Standard".into(),
        arrow_size: 0.18,
        spline: false,
        block: Some(reference("BM", [127.36, -4.0])),
    };
    let leader = d.add(&model, Common::default(), EntityKind::MLeader(leader)).unwrap();
    // A dimension along (150, -10) to (160, -10) that draws *D9.
    let dimension = Dimension {
        kind: DimKind::Aligned,
        defpt: Vec3::new(160.0, -10.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::new(150.0, -10.0, 0.0),
        p14: Vec3::new(160.0, -10.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: Some("*D9".into()),
        insertion_point: Vec3::ZERO,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let dim = d.add(&model, Common::default(), EntityKind::Dimension(dimension)).unwrap();
    BlockDrawers { d, array, tagged, scaled, leader, dim }
}

/// An array of references (MINSERT) and a reference with an attribute on the frozen layer follow
/// the viewport as a plain reference does: the parts and the attribute on the frozen layer are
/// not hit and take no room in a window. They have the coordinates AutoCAD 2022 was given and its
/// answers. The frozen layers are given to the selection as a viewport gives them.
#[test]
fn arrays_and_attributes_follow_the_viewport() {
    let BlockDrawers { d, array, tagged, .. } = block_drawers();
    let through = ["Hidden".to_string()];
    // The array: the "Hidden" part of no copy is hit, and a window needs the layer 0 parts only.
    assert_eq!(picked(&d, &through, [5.0, -30.0]), Some(array), "the layer 0 part of the first copy");
    assert_eq!(picked(&d, &through, [5.0, -25.0]), None, "the part of the first copy that is not drawn");
    assert_eq!(picked(&d, &through, [20.0, -13.0]), None, "the part of the last copy that is not drawn");
    assert_eq!(picked(&d, &through, [20.0, -18.0]), Some(array), "the layer 0 part of the last copy");
    assert_eq!(crossed(&d, &through, [19.0, -14.0], [21.0, -12.0]), [], "a crossing window over the last copy's part that is not drawn");
    assert_eq!(windowed(&d, &through, [-1.0, -31.0], [26.0, -17.0]), [array], "a window around all but the upper row's parts that are not drawn");
    assert_eq!(picked(&d, &[], [20.0, -13.0]), Some(array), "where the layer is drawn");
    assert_eq!(windowed(&d, &[], [-1.0, -31.0], [26.0, -17.0]), []);
    assert_eq!(windowed(&d, &[], [-1.0, -31.0], [26.0, -12.0]), [array]);

    // The attribute: not hit, and no room taken.
    assert_eq!(picked(&d, &through, [75.0, 30.0]), Some(tagged));
    assert_eq!(crossed(&d, &through, [70.0, 33.5], [76.0, 36.0]), [], "a crossing window over the attribute that is not drawn");
    assert_eq!(fenced(&d, &through, [71.0, 33.0], [71.0, 37.0]), [], "a fence across it");
    assert_eq!(windowed(&d, &through, [69.0, 29.0], [81.0, 31.0]), [tagged], "a window around the line alone");
    assert_eq!(crossed(&d, &[], [70.0, 33.5], [76.0, 36.0]), [tagged], "where the layer is drawn");
    assert_eq!(windowed(&d, &[], [69.0, 29.0], [81.0, 31.0]), []);
    assert_eq!(windowed(&d, &[], [69.0, 29.0], [81.0, 37.0]), [tagged]);
}

/// An annotative reference, a multileader whose content is a block and a dimension whose block
/// nests a reference follow the viewport too: their parts on the frozen layer are not hit and
/// take no room in a window. The first two have the coordinates AutoCAD 2022 was given and its
/// answers. The frozen layers are given to the selection as a viewport gives them.
#[test]
fn annotative_references_multileaders_and_dimensions_follow_the_viewport() {
    let BlockDrawers { d, scaled, leader, dim, .. } = block_drawers();
    let through = ["Hidden".to_string()];
    // The annotative reference.
    assert_eq!(picked(&d, &through, [5.0, 5.0]), None, "the part of the annotative reference that is not drawn");
    assert_eq!(picked(&d, &through, [5.0, 0.0]), Some(scaled));
    assert_eq!(crossed(&d, &through, [4.0, 4.0], [6.0, 6.0]), []);
    assert_eq!(fenced(&d, &through, [5.0, 4.0], [5.0, 6.0]), []);
    assert_eq!(windowed(&d, &through, [-1.0, -1.0], [11.0, 1.0]), [scaled], "a window around its layer 0 part alone");
    assert_eq!(picked(&d, &[], [5.0, 5.0]), Some(scaled), "where the layer is drawn");
    assert_eq!(windowed(&d, &[], [-1.0, -1.0], [11.0, 1.0]), []);

    // The multileader: its block's part on "Hidden" lies from (127.36, 4) to (133.36, 4).
    assert_eq!(picked(&d, &through, [130.36, 4.0]), None, "the part of the multileader's block that is not drawn");
    assert_eq!(picked(&d, &through, [130.36, -4.0]), Some(leader), "the layer 0 part of its block");
    assert_eq!(picked(&d, &through, [120.0, -10.0]), Some(leader), "its leader line");
    assert_eq!(crossed(&d, &through, [129.65, 3.29], [131.07, 4.71]), []);
    assert_eq!(fenced(&d, &through, [130.36, 3.0], [130.36, 5.0]), []);
    assert_eq!(windowed(&d, &through, [108.0, -22.0], [135.36, 2.0]), [leader], "a window around its leader and the layer 0 part");
    assert_eq!(picked(&d, &[], [130.36, 4.0]), Some(leader), "where the layer is drawn");
    assert_eq!(windowed(&d, &[], [108.0, -22.0], [135.36, 2.0]), []);
    assert_eq!(windowed(&d, &[], [108.0, -22.0], [135.36, 6.0]), [leader]);

    // The dimension: the nested reference's part on "Hidden" lies from (150, 5) to (160, 5).
    assert_eq!(picked(&d, &through, [155.0, 5.0]), None, "the part of the reference nested in the dimension's block that is not drawn");
    assert_eq!(crossed(&d, &through, [154.0, 4.0], [156.0, 6.0]), []);
    assert_eq!(picked(&d, &through, [155.0, 0.0]), Some(dim), "the layer 0 part of that reference");
    assert_eq!(picked(&d, &[], [155.0, 5.0]), Some(dim), "where the layer is drawn");
}

/// A drawing, built by hand with the coordinates AutoCAD 2022 was given, whose model space holds
/// one reference, and that reference: to a block with a line on layer 0 from (60, 36) to (70, 36)
/// and a reference to a block of one line on "Hidden", from (60, 30) to (70, 30), clipped (XCLIP)
/// to the rectangle from (58, 27) to (72, 33).
fn framed_reference() -> (cadcraft_engine::doc::Drawing, Handle) {
    use cadcraft_engine::doc::{Block, ClipBoundary, Common, Drawing, Entity, EntityKind, Insert, Line, Space};
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Hidden");
    let line = |a: [f64; 2], b: [f64; 2]| EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) });
    let reference = |block: &str, at: [f64; 2]| Insert { block: block.into(), insert: Vec3::new(at[0], at[1], 0.0), ..Insert::default() };
    let mut define = |name: &str, parts: Vec<(EntityKind, &str)>| {
        let mut block = Block::new(name);
        for (i, (kind, layer)) in parts.into_iter().enumerate() {
            let mut part = Entity::new(Handle(i as u64 + 1), kind);
            part.common.layer = layer.into();
            block.entities.push(part);
        }
        d.blocks.insert(name.into(), std::sync::Arc::new(block));
    };
    // BC2: one line, on "Hidden". BNC: a reference to it, clipped to a rectangle given in the
    // coordinates of BC2, and a line on layer 0.
    define("BC2", vec![(line([0.0, 0.0], [10.0, 0.0]), "Hidden")]);
    let boundary = ClipBoundary::new(vec![Vec2::new(-2.0, -3.0), Vec2::new(12.0, 3.0)]);
    let inner = Insert { clip: Some(boundary), ..reference("BC2", [60.0, 30.0]) };
    define("BNC", vec![(EntityKind::Insert(inner), "0"), (line([60.0, 36.0], [70.0, 36.0]), "0")]);
    let framed = d.add(&Space::Model, Common::default(), EntityKind::Insert(reference("BNC", [0.0, 0.0]))).unwrap();
    (d, framed)
}

/// A clipped reference (XCLIP) that is a part of a block keeps its boundary as well: AutoCAD 2022
/// selects the outer reference by a pick or a crossing window on the boundary of the clipped one
/// with every part inside it on a layer frozen in the viewport, and a window must hold that
/// boundary. A pick on the part that is not drawn takes nothing. The pick on the top side of the
/// boundary holds the hit geometry: it lies inside the outer reference's box, away from its
/// sides. The shape is the one AutoCAD was given. The frozen layers are given to the selection
/// as a viewport gives them.
#[test]
fn a_clipped_reference_inside_a_block_is_selected_by_its_boundary() {
    let (d, framed) = framed_reference();
    let through = ["Hidden".to_string()];
    assert_eq!(picked(&d, &through, [58.0, 30.0]), Some(framed), "the left side of the nested clip boundary");
    assert_eq!(picked(&d, &through, [65.0, 33.0]), Some(framed), "its top side");
    assert_eq!(crossed(&d, &through, [57.0, 29.0], [59.0, 31.0]), [framed], "a crossing window over its left side");
    assert_eq!(picked(&d, &through, [65.0, 30.0]), None, "the clipped reference's line, which is not drawn");
    assert_eq!(picked(&d, &through, [65.0, 36.0]), Some(framed), "the outer reference's own line");
    assert_eq!(windowed(&d, &through, [59.0, 35.0], [71.0, 37.0]), [], "a window around its own line alone");
    assert_eq!(windowed(&d, &through, [56.0, 25.0], [74.0, 38.0]), [framed], "a window around its own line and the boundary");
    assert_eq!(picked(&d, &[], [65.0, 30.0]), Some(framed), "where the layer is drawn");
}

/// A dimension is drawn from its block, and a crossing window inside the rectangle around a
/// dimension does not take it as one inside a block reference's does (#2991), so a dimension
/// shows what a crossing window itself tests. Over a part of that block the viewport does not
/// draw, between two parts it draws, the window takes nothing, and neither do a pick and a
/// fence; over a drawn part they take the dimension. The frozen layers are given to the
/// selection as a viewport gives them.
#[test]
fn a_crossing_window_tests_the_parts_the_viewport_draws() {
    use cadcraft_engine::doc::{Block, Common, DimKind, Dimension, Drawing, Entity, EntityKind, Line, Space};
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Hidden");
    let mut block = Block::new("*D1");
    for (handle, y, layer) in [(1, 0.0, "0"), (2, 10.0, "0"), (3, 5.0, "Hidden")] {
        let mut part = Entity::new(Handle(handle), EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) }));
        part.common.layer = layer.into();
        block.entities.push(part);
    }
    d.blocks.insert("*D1".into(), std::sync::Arc::new(block));
    let dimension = Dimension {
        kind: DimKind::Aligned,
        defpt: Vec3::new(5.0, 1.0, 0.0),
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
        block: Some("*D1".into()),
        insertion_point: Vec3::ZERO,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let dim = d.add(&Space::Model, Common::default(), EntityKind::Dimension(dimension)).unwrap();
    let frozen = ["Hidden".to_string()];
    let (model, hidden_part, drawn_part) = (Space::Model, Vec2::new(5.0, 5.0), Vec2::new(5.0, 10.0));
    let over = |p: Vec2| Bounds2::new(Vec2::new(p.x - 1.0, p.y - 1.0), Vec2::new(p.x + 1.0, p.y + 1.0));
    let across = |p: Vec2| [Vec2::new(p.x, p.y - 1.0), Vec2::new(p.x, p.y + 1.0)];
    assert_eq!(select::select_window_in(&d, &model, &frozen, over(hidden_part), true), [], "a crossing window over the part that is not drawn");
    assert_eq!(select::pick_in(&d, &model, &frozen, hidden_part, 0.2), None, "a pick on it");
    assert_eq!(select::select_fence_in(&d, &model, &frozen, &across(hidden_part)), [], "a fence across it");
    assert_eq!(select::select_window_in(&d, &model, &frozen, over(drawn_part), true), [dim], "a crossing window over a drawn part");
    assert_eq!(select::pick_in(&d, &model, &frozen, drawn_part, 0.2), Some(dim));
    assert_eq!(select::select_window_in(&d, &model, &[], over(hidden_part), true), [dim], "where the layer is drawn");
    assert_eq!(select::pick_in(&d, &model, &[], hidden_part, 0.2), Some(dim));
    assert_eq!(select::select_fence_in(&d, &model, &[], &across(hidden_part)), [dim]);
}

/// The selections of a grid across the five references, with what each finds: at every point a
/// pick, selection cycling's list, a crossing window, a fence, two windows and the two polygons.
fn probes(s: &Session) -> Vec<(String, Vec<Handle>)> {
    let mut out = Vec::new();
    for xi in -2..=92 {
        for yi in -2..=12 {
            let (x, y) = (f64::from(xi), f64::from(yi));
            out.push((format!("pick at ({x}, {y})"), pick(s, [x, y]).into_iter().collect()));
            out.push((format!("cycling list at ({x}, {y})"), pick_all(s, [x, y])));
            out.push((format!("crossing window from ({x}, {y})"), window(s, [x, y], [x + 2.0, y + 2.0], true)));
            out.push((format!("fence through ({x}, {y})"), fence(s, &[[x, y - 1.0], [x, y + 1.0]])));
            out.push((format!("low window from ({x}, {y})"), window(s, [x, y], [x + 13.0, y + 2.0], false)));
            out.push((format!("tall window from ({x}, {y})"), window(s, [x, y], [x + 13.0, y + 12.0], false)));
            out.push((format!("window polygon from ({x}, {y})"), polygon(s, &[[x, y], [x + 13.0, y], [x + 6.0, y + 3.0]], false)));
            out.push((format!("crossing polygon from ({x}, {y})"), polygon(s, &[[x, y], [x + 2.0, y], [x + 1.0, y + 2.0]], true)));
        }
    }
    out
}

/// Through a viewport that freezes a layer every selection answers as it does on the Model tab
/// with that layer frozen for the whole drawing: each selection of a grid across the references,
/// with and without the spatial index. That holds for what is taken through the rectangle around
/// a reference too (#2991): it is the rectangle around the drawn parts in both, and a part that is
/// not drawn changes nothing inside it.
#[test]
fn a_layer_frozen_in_the_viewport_selects_as_one_frozen_for_the_drawing_does() {
    for crowded in [false, true] {
        let Parts { mut s, r, h, n, z, q, a, .. } = parts(crowded);
        enter(&mut s, &a);
        let through_the_viewport = probes(&s);
        let Parts { s: mut model, r: r2, h: h2, n: n2, z: z2, q: q2, .. } = parts(crowded);
        assert_eq!([r, h, n, z, q], [r2, h2, n2, z2, q2], "the two drawings must be made alike");
        model.execute("layout.set", &json!({ "name": "Model" })).unwrap();
        model.execute("layer.set", &json!({ "name": "Hidden", "frozen": true })).unwrap();
        let frozen_for_the_drawing = probes(&model);
        assert_eq!(through_the_viewport.len(), frozen_for_the_drawing.len());
        for (through, frozen) in through_the_viewport.iter().zip(&frozen_for_the_drawing) {
            assert_eq!(through, frozen, "through the viewport, then with the layer frozen for the drawing (indexed: {crowded})");
        }
        // The grid does select: each reference with a drawn part many times, the one without never.
        let times = |x: Handle| through_the_viewport.iter().filter(|(_, hs)| hs.contains(&x)).count();
        assert!([r, n, z, q].iter().all(|x| times(*x) > 50), "taken {:?} times (indexed: {crowded})", [r, n, z, q].map(times));
        assert_eq!(times(h), 0, "a reference with nothing drawn was selected (indexed: {crowded})");
    }
}

/// Nothing changes where no viewport freezes the layer: on the Model tab both parts are hit and
/// both take room, and with the layer frozen for the whole drawing the part is left out there
/// as it always was.
#[test]
fn the_model_tab_and_a_layer_frozen_for_the_drawing_are_as_before() {
    for crowded in [false, true] {
        let Parts { mut s, r, h, .. } = parts(crowded);
        s.execute("layout.set", &json!({ "name": "Model" })).unwrap();
        assert_eq!(pick(&s, [5.0, 5.0]), Some(r), "the Model tab draws both parts (indexed: {crowded})");
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), []);
        assert_eq!(pick(&s, [25.0, 5.0]), Some(h));
        s.execute("layer.set", &json!({ "name": "Hidden", "frozen": true })).unwrap();
        assert_eq!(pick(&s, [5.0, 5.0]), None, "frozen for the whole drawing (indexed: {crowded})");
        assert_eq!(window(&s, [4.0, 4.0], [6.0, 6.0], true), []);
        assert_eq!(window(&s, [-1.0, -1.0], [11.0, 1.0], false), [r]);
        assert_eq!(pick(&s, [25.0, 5.0]), None);
        assert_eq!(pick(&s, [5.0, 0.0]), Some(r));
    }
}
