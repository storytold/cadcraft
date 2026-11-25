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

/// The objects of the two tests about the layer Defpoints, built by hand in model space.
struct OnDefpoints {
    /// The drawing that holds them.
    d: cadcraft_engine::doc::Drawing,
    /// A dimension like the one AutoCAD made for DIMLINEAR from (0, 5) to (40, 15), with the
    /// dimension line at y = 40 and the extension lines starting 2 from those points.
    dim: Handle,
    /// A reference, 200 to the right, to a block that holds such a dimension.
    holder: Handle,
    /// A dimension like the one AutoCAD made for DIMORDINATE, here 100 higher: the feature 60
    /// right of the origin it measures from, and a leader 15 long.
    ord: Handle,
    /// A line on Defpoints from (50, 0) to (60, 0).
    plain: Handle,
    /// A reference at (70, -20) to a block with a line on layer 0 and, 5 above it, one on
    /// Defpoints.
    reference: Handle,
    /// A reference at (100, -20) to a block with a line on layer 0 and, 3 above its middle, a
    /// point on Defpoints: the shape AutoCAD was given.
    pointed: Handle,
    /// A reference at (100, -40) to a block with that point between two lines on layer 0, so
    /// inside the box around them.
    between: Handle,
    /// A reference at (130, -40) to a block that holds a reference to that block.
    deep: Handle,
    /// A reference at (160, -40) to a block like that one, with the point on the layer "Hidden".
    other: Handle,
    /// The first dimension 300 to the right, with the first extension line of its block on
    /// Defpoints.
    moved: Handle,
}

/// The drawing of [`OnDefpoints`]. With `crowded`, model space also holds enough lines, far from
/// those objects, for selections to go through the spatial index instead of scanning the space.
fn defpoints_objects(crowded: bool) -> OnDefpoints {
    use cadcraft_engine::doc::{Block, Common, DimKind, Dimension, Drawing, Entity, EntityKind, Insert, Line, Point, Space};
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Defpoints");
    d.ensure_layer("Hidden");
    let line = |a: [f64; 2], b: [f64; 2]| EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) });
    let point = |p: [f64; 2]| EntityKind::Point(Point { p: Vec3::new(p[0], p[1], 0.0), angle: 0.0 });
    let at = |p: [f64; 2]| Vec3::new(p[0], p[1], 0.0);
    let measured = |kind: DimKind, defpt: [f64; 2], p13: [f64; 2], p14: [f64; 2], block: &str| Dimension {
        kind,
        defpt: at(defpt),
        text_mid: Vec3::ZERO,
        p13: at(p13),
        p14: at(p14),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: Some(block.into()),
        insertion_point: Vec3::ZERO,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let mut define = |name: &str, parts: Vec<(EntityKind, &str)>| {
        let mut block = Block::new(name);
        for (i, (kind, layer)) in parts.into_iter().enumerate() {
            let mut part = Entity::new(Handle(i as u64 + 1), kind);
            part.common.layer = layer.into();
            block.entities.push(part);
        }
        d.blocks.insert(name.into(), std::sync::Arc::new(block));
    };
    // *D1: the two extension lines, the dimension line and the three definition points. *D2 is
    // the same for a second dimension, the one the block BDIM holds.
    let made = || {
        vec![
            (line([0.0, 7.0], [0.0, 40.18]), "0"),
            (line([40.0, 17.0], [40.0, 40.18]), "0"),
            (line([0.18, 40.0], [39.82, 40.0]), "0"),
            (point([0.0, 5.0]), "Defpoints"),
            (point([40.0, 15.0]), "Defpoints"),
            (point([40.0, 40.0]), "Defpoints"),
        ]
    };
    define("*D1", made());
    define("*D2", made());
    define("BDIM", vec![(EntityKind::Dimension(measured(DimKind::Aligned, [40.0, 40.0], [0.0, 5.0], [40.0, 15.0], "*D2")), "0")]);
    // *D3: the leader of an ordinate dimension and its three definition points: the feature at
    // (60, 100), the leader's end and, 60 to the left of all that, the origin it measures from.
    define(
        "*D3",
        vec![
            (line([60.0, 98.0], [60.0, 85.0]), "0"),
            (point([60.0, 100.0]), "Defpoints"),
            (point([60.0, 85.0]), "Defpoints"),
            (point([0.0, 100.0]), "Defpoints"),
        ],
    );
    // BD: a line on layer 0 and, 5 above it, one on Defpoints.
    define("BD", vec![(line([0.0, 0.0], [10.0, 0.0]), "0"), (line([0.0, 5.0], [10.0, 5.0]), "Defpoints")]);
    // BQ: a line on layer 0 and, 3 above its middle, a point on Defpoints. BB: that point between
    // two lines on layer 0, 6 apart. BN holds a reference to BB. BH is BB with the point on
    // "Hidden".
    let two_lines =
        |layer: &'static str| vec![(line([0.0, 0.0], [10.0, 0.0]), "0"), (line([0.0, 6.0], [10.0, 6.0]), "0"), (point([5.0, 3.0]), layer)];
    define("BQ", vec![(line([0.0, 0.0], [10.0, 0.0]), "0"), (point([5.0, 3.0]), "Defpoints")]);
    define("BB", two_lines("Defpoints"));
    define("BN", vec![(EntityKind::Insert(Insert { block: "BB".into(), ..Insert::default() }), "0")]);
    define("BH", two_lines("Hidden"));
    // *D4: *D1 300 to the right, with its first extension line on Defpoints.
    define(
        "*D4",
        vec![
            (line([300.0, 7.0], [300.0, 40.18]), "Defpoints"),
            (line([340.0, 17.0], [340.0, 40.18]), "0"),
            (line([300.18, 40.0], [339.82, 40.0]), "0"),
            (point([300.0, 5.0]), "Defpoints"),
            (point([340.0, 15.0]), "Defpoints"),
            (point([340.0, 40.0]), "Defpoints"),
        ],
    );
    let model = Space::Model;
    let dimension = measured(DimKind::Aligned, [40.0, 40.0], [0.0, 5.0], [40.0, 15.0], "*D1");
    let dim = d.add(&model, Common::default(), EntityKind::Dimension(dimension)).unwrap();
    let ordinate = measured(DimKind::Ordinate { x_type: true }, [0.0, 100.0], [60.0, 100.0], [60.0, 85.0], "*D3");
    let ord = d.add(&model, Common::default(), EntityKind::Dimension(ordinate)).unwrap();
    let held = Insert { block: "BDIM".into(), insert: Vec3::new(200.0, 0.0, 0.0), ..Insert::default() };
    let holder = d.add(&model, Common::default(), EntityKind::Insert(held)).unwrap();
    let on_the_layer = Common { layer: "Defpoints".into(), ..Common::default() };
    let plain = d.add(&model, on_the_layer, line([50.0, 0.0], [60.0, 0.0])).unwrap();
    let placed = Insert { block: "BD".into(), insert: Vec3::new(70.0, -20.0, 0.0), ..Insert::default() };
    let reference = d.add(&model, Common::default(), EntityKind::Insert(placed)).unwrap();
    let place = |block: &str, x: f64, y: f64| EntityKind::Insert(Insert { block: block.into(), insert: Vec3::new(x, y, 0.0), ..Insert::default() });
    let pointed = d.add(&model, Common::default(), place("BQ", 100.0, -20.0)).unwrap();
    let between = d.add(&model, Common::default(), place("BB", 100.0, -40.0)).unwrap();
    let deep = d.add(&model, Common::default(), place("BN", 130.0, -40.0)).unwrap();
    let other = d.add(&model, Common::default(), place("BH", 160.0, -40.0)).unwrap();
    let with_moved_line = measured(DimKind::Aligned, [340.0, 40.0], [300.0, 5.0], [340.0, 15.0], "*D4");
    let moved = d.add(&model, Common::default(), EntityKind::Dimension(with_moved_line)).unwrap();
    if crowded {
        for i in 0..=cadcraft_engine::spatial::MIN_INDEXED {
            let x = 1000.0 + i as f64;
            d.add(&model, Common::default(), line([x, 1000.0], [x, 1001.0])).unwrap();
        }
    }
    assert_eq!(d.model.len() >= cadcraft_engine::spatial::MIN_INDEXED, crowded, "whether model space is indexed");
    OnDefpoints { d, dim, holder, ord, plain, reference, pointed, between, deep, other, moved }
}

/// A dimension is selected on its definition points whatever is frozen: AutoCAD 2022 selects one
/// by a pick or a crossing window on such a point, and a window must hold the points, with the
/// layer Defpoints frozen in the viewport too. So the points on that layer in the block of a
/// dimension are not left out of what the dimension is hit on or of the room it takes. A line of
/// its block on Defpoints is left out, as AutoCAD does not select the dimension by it there. The
/// same holds for a dimension that is a part of a block, where the reference is what is selected,
/// and for the first definition point of an ordinate dimension, the origin it measures from,
/// which lies away from all the dimension draws. The
/// picks on the definition point of the dimension and of the one in a block hold the hit
/// geometry: they lie inside the box, away from its sides. The crossing windows on those points
/// reach a corner of the box as well. The origin of the ordinate dimension lies on a side of
/// that dimension's box, so the assertions on it hold the box. The windows that leave a
/// definition point out would take nothing without the points of the block too: a dimension's
/// box always takes in the definition points the dimension itself stores. The frozen layers are
/// given to the selection as a viewport gives them, with and without the spatial index.
#[test]
fn a_dimension_is_hit_on_its_definition_points_whatever_the_viewport_freezes() {
    let through = ["Defpoints".to_string()];
    for crowded in [false, true] {
        let OnDefpoints { d, dim, holder, ord, moved, .. } = defpoints_objects(crowded);
        // The dimension, on its first definition point.
        assert_eq!(picked(&d, &through, [0.0, 5.0]), Some(dim), "a pick on a definition point (indexed: {crowded})");
        assert_eq!(crossed(&d, &through, [-0.5, 4.5], [0.5, 5.5]), [dim], "a crossing window over it (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [-2.0, 6.0], [42.0, 43.0]), [], "a window that leaves it out (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [-2.0, 3.0], [42.0, 43.0]), [dim], "a window around all of the dimension (indexed: {crowded})");
        assert_eq!(picked(&d, &[], [0.0, 5.0]), Some(dim), "where the layer is drawn (indexed: {crowded})");
        assert_eq!(windowed(&d, &[], [-2.0, 6.0], [42.0, 43.0]), [], "indexed: {crowded}");
        // The dimension in a block: the reference, on the dimension's first definition point. The
        // pick is what tells: it is not near a side of the reference's box.
        assert_eq!(picked(&d, &through, [200.0, 5.0]), Some(holder), "a pick on the definition point of a dimension in a block (indexed: {crowded})");
        assert_eq!(crossed(&d, &through, [199.5, 4.5], [200.5, 5.5]), [holder], "a crossing window over it (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [198.0, 6.0], [242.0, 43.0]), [], "a window that leaves it out (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [198.0, 3.0], [242.0, 43.0]), [holder], "a window around all of the reference (indexed: {crowded})");
        assert_eq!(picked(&d, &[], [200.0, 5.0]), Some(holder), "where the layer is drawn (indexed: {crowded})");
        // The ordinate dimension, on the origin it measures from and on its feature point.
        assert_eq!(picked(&d, &through, [0.0, 100.0]), Some(ord), "a pick on the origin point of an ordinate dimension (indexed: {crowded})");
        assert_eq!(crossed(&d, &through, [-0.5, 99.5], [0.5, 100.5]), [ord], "a crossing window over it (indexed: {crowded})");
        assert_eq!(picked(&d, &through, [60.0, 100.0]), Some(ord), "a pick on its feature point (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [30.0, 60.0], [90.0, 130.0]), [], "a window around all of it but the origin point (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [-5.0, 60.0], [90.0, 130.0]), [ord], "a window around all of it (indexed: {crowded})");
        assert_eq!(picked(&d, &[], [0.0, 100.0]), Some(ord), "where the layer is drawn (indexed: {crowded})");
        assert_eq!(windowed(&d, &[], [30.0, 60.0], [90.0, 130.0]), [], "indexed: {crowded}");
        // A dimension with a line of its block on Defpoints: the line follows the viewport, the
        // definition points do not.
        assert_eq!(picked(&d, &through, [300.0, 20.0]), None, "a line of a dimension's block on Defpoints (indexed: {crowded})");
        assert_eq!(picked(&d, &through, [300.0, 5.0]), Some(moved), "the definition point below it (indexed: {crowded})");
        assert_eq!(picked(&d, &[], [300.0, 20.0]), Some(moved), "where the layer is drawn (indexed: {crowded})");
    }
}

/// A point on the layer Defpoints that is a part of a block counts like the definition points of
/// a dimension, which are such points: AutoCAD 2022 selects a reference by a pick or a crossing window on such
/// a point of its block, one block deeper too, and a window must hold the point, with Defpoints
/// frozen in the viewport. A point on another layer the viewport freezes is not hit. The first
/// reference has the shape AutoCAD was given, the point above the line: that point makes the top
/// side of the reference's box, so the assertions on it hold the box. The picks on the point
/// between two lines hold the hit geometry: they lie inside the box, away from its sides. The
/// frozen layers are given to the selection as a viewport gives them, with and without the
/// spatial index.
#[test]
fn a_reference_is_hit_on_a_point_on_defpoints_whatever_the_viewport_freezes() {
    let through = ["Defpoints".to_string(), "Hidden".to_string()];
    for crowded in [false, true] {
        let OnDefpoints { d, pointed, between, deep, other, .. } = defpoints_objects(crowded);
        // The point above the line.
        assert_eq!(picked(&d, &through, [105.0, -17.0]), Some(pointed), "a pick on the point (indexed: {crowded})");
        assert_eq!(crossed(&d, &through, [104.5, -17.5], [105.5, -16.5]), [pointed], "a crossing window over it (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [99.0, -21.0], [111.0, -19.0]), [], "a window around the line alone (indexed: {crowded})");
        assert_eq!(windowed(&d, &through, [99.0, -21.0], [111.0, -16.0]), [pointed], "a window around the line and the point (indexed: {crowded})");
        // The point inside the box of two drawn lines, and the same one block deeper.
        assert_eq!(picked(&d, &through, [105.0, -37.0]), Some(between), "a pick on the point between two lines (indexed: {crowded})");
        assert_eq!(picked(&d, &through, [135.0, -37.0]), Some(deep), "the same one block deeper (indexed: {crowded})");
        // A point on the other layer the viewport freezes.
        assert_eq!(picked(&d, &through, [165.0, -37.0]), None, "a point on another frozen layer (indexed: {crowded})");
        assert_eq!(picked(&d, &[], [165.0, -37.0]), Some(other), "where that layer is drawn (indexed: {crowded})");
    }
}

/// A line on the layer Defpoints and a block's part on it follow the viewport as on any other
/// layer: where the viewport freezes Defpoints they are not hit and take no room, as in AutoCAD
/// 2022. Only a point on Defpoints inside a block is kept. With and without the spatial index.
#[test]
fn a_line_and_a_block_part_on_defpoints_follow_the_viewport() {
    let through = ["Defpoints".to_string()];
    for crowded in [false, true] {
        let OnDefpoints { d, plain, reference, .. } = defpoints_objects(crowded);
        // A line on Defpoints.
        assert_eq!(picked(&d, &through, [55.0, 0.0]), None, "a line on the layer the viewport freezes (indexed: {crowded})");
        assert_eq!(crossed(&d, &through, [54.0, -1.0], [56.0, 1.0]), [], "indexed: {crowded}");
        assert_eq!(picked(&d, &[], [55.0, 0.0]), Some(plain), "indexed: {crowded}");
        // A block's part on Defpoints.
        assert_eq!(picked(&d, &through, [75.0, -15.0]), None, "a block's part on that layer (indexed: {crowded})");
        assert_eq!(crossed(&d, &through, [74.0, -16.0], [76.0, -14.0]), [], "indexed: {crowded}");
        assert_eq!(picked(&d, &through, [75.0, -20.0]), Some(reference), "indexed: {crowded}");
        assert_eq!(windowed(&d, &through, [69.0, -21.0], [81.0, -19.0]), [reference], "a window around its layer 0 part alone (indexed: {crowded})");
        assert_eq!(picked(&d, &[], [75.0, -15.0]), Some(reference), "indexed: {crowded}");
        assert_eq!(windowed(&d, &[], [69.0, -21.0], [81.0, -19.0]), [], "indexed: {crowded}");
        assert_eq!(windowed(&d, &[], [69.0, -21.0], [81.0, -14.0]), [reference], "indexed: {crowded}");
    }
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

/// Other objects whose block holds a point on the layer Defpoints, and a clipped reference inside
/// a block, built by hand in model space with the coordinates AutoCAD 2022 was given.
struct PointHolders {
    /// The drawing that holds them.
    d: cadcraft_engine::doc::Drawing,
    /// A 2 x 2 array of references (MINSERT) from (0, -30), its rows 12 and its columns 15 apart,
    /// to a block with a line on layer 0 and, 3 above its middle, a point on Defpoints.
    array: Handle,
    /// An annotative reference to that block at (40, -30), at the drawing's annotation scale.
    scaled: Handle,
    /// A multileader from (60, -30) to (75, -15) whose content is a block with a line on layer 0
    /// and, 8 above its middle, a point on Defpoints, at (72.36, -19), where AutoCAD put it.
    leader: Handle,
    /// A reference at (0, 10) to a block with a line on layer 0, 20 long, and two points on
    /// Defpoints 3 above it, clipped (XCLIP) to the rectangle from (-2, 7) to (10, 16): the point
    /// at (5, 13) is inside the boundary, the one at (15, 13) outside.
    clipped: Handle,
    /// A reference to a block with a line on layer 0 from (30, 10) to (40, 10) and, at (30, 15), a
    /// reference on "Hidden" to the block of the array.
    nested: Handle,
    /// A reference on "Hidden" at (50, 10) to the block of the array.
    hidden: Handle,
    /// A dimension on "Hidden" like the one AutoCAD made for DIMLINEAR from (70, 5) to (90, 5),
    /// with the dimension line at y = 20 and the extension lines starting 2 from those points.
    dim: Handle,
    /// A reference to a block with a line on layer 0 from (60, 36) to (70, 36) and a reference to
    /// a block of one line on "Hidden", from (60, 30) to (70, 30), clipped to the rectangle from
    /// (58, 27) to (72, 33).
    framed: Handle,
}

/// The drawing of [`PointHolders`].
fn point_holders() -> PointHolders {
    use cadcraft_engine::doc::{
        AnnoScale, Block, ClipBoundary, Common, DimKind, Dimension, Drawing, Entity, EntityKind, Insert, Line, MLeader, Point, Space,
    };
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Defpoints");
    d.ensure_layer("Hidden");
    let line = |a: [f64; 2], b: [f64; 2]| EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) });
    let point = |p: [f64; 2]| EntityKind::Point(Point { p: Vec3::new(p[0], p[1], 0.0), angle: 0.0 });
    let reference = |block: &str, at: [f64; 2]| Insert { block: block.into(), insert: Vec3::new(at[0], at[1], 0.0), ..Insert::default() };
    // A clip rectangle, in the coordinates of the clipped reference's block.
    let rectangle = |a: [f64; 2], b: [f64; 2]| Some(ClipBoundary::new(vec![Vec2::new(a[0], a[1]), Vec2::new(b[0], b[1])]));
    let mut define = |name: &str, parts: Vec<(EntityKind, &str)>| {
        let mut block = Block::new(name);
        for (i, (kind, layer)) in parts.into_iter().enumerate() {
            let mut part = Entity::new(Handle(i as u64 + 1), kind);
            part.common.layer = layer.into();
            block.entities.push(part);
        }
        d.blocks.insert(name.into(), std::sync::Arc::new(block));
    };
    // BQ: a line on layer 0 and, 3 above its middle, a point on Defpoints. BMQ: the same, shorter,
    // with the point 8 above. BCQ: a line 20 long and two such points.
    define("BQ", vec![(line([0.0, 0.0], [10.0, 0.0]), "0"), (point([5.0, 3.0]), "Defpoints")]);
    define("BMQ", vec![(line([0.0, 0.0], [6.0, 0.0]), "0"), (point([3.0, 8.0]), "Defpoints")]);
    define("BCQ", vec![(line([0.0, 0.0], [20.0, 0.0]), "0"), (point([5.0, 3.0]), "Defpoints"), (point([15.0, 3.0]), "Defpoints")]);
    // BNF: a line on layer 0 and a reference on "Hidden" to BQ.
    define("BNF", vec![(line([30.0, 10.0], [40.0, 10.0]), "0"), (EntityKind::Insert(reference("BQ", [30.0, 15.0])), "Hidden")]);
    // *D5: the two extension lines, the dimension line and the three definition points.
    define(
        "*D5",
        vec![
            (line([70.0, 7.0], [70.0, 20.18]), "0"),
            (line([90.0, 7.0], [90.0, 20.18]), "0"),
            (line([70.18, 20.0], [89.82, 20.0]), "0"),
            (point([70.0, 5.0]), "Defpoints"),
            (point([90.0, 5.0]), "Defpoints"),
            (point([90.0, 20.0]), "Defpoints"),
        ],
    );
    // BC2: one line, on "Hidden". BNC: a reference to it, clipped, and a line on layer 0.
    define("BC2", vec![(line([0.0, 0.0], [10.0, 0.0]), "Hidden")]);
    let inner = Insert { clip: rectangle([-2.0, -3.0], [12.0, 3.0]), ..reference("BC2", [60.0, 30.0]) };
    define("BNC", vec![(EntityKind::Insert(inner), "0"), (line([60.0, 36.0], [70.0, 36.0]), "0")]);
    let model = Space::Model;
    let on_hidden = || Common { layer: "Hidden".into(), ..Common::default() };
    let grid = Insert { cols: 2, rows: 2, col_spacing: 15.0, row_spacing: 12.0, ..reference("BQ", [0.0, -30.0]) };
    let array = d.add(&model, Common::default(), EntityKind::Insert(grid)).unwrap();
    let annotative = Common { anno_scales: vec![AnnoScale::unit()], ..Common::default() };
    let scaled = d.add(&model, annotative, EntityKind::Insert(reference("BQ", [40.0, -30.0]))).unwrap();
    let leader = MLeader {
        leaders: vec![vec![Vec3::new(60.0, -30.0, 0.0)]],
        landing: Vec3::new(75.0, -15.0, 0.0),
        dogleg: 0.36,
        text: None,
        style: "Standard".into(),
        arrow_size: 0.18,
        spline: false,
        block: Some(reference("BMQ", [72.36, -19.0])),
    };
    let leader = d.add(&model, Common::default(), EntityKind::MLeader(leader)).unwrap();
    let cut = Insert { clip: rectangle([-2.0, -3.0], [10.0, 6.0]), ..reference("BCQ", [0.0, 10.0]) };
    let clipped = d.add(&model, Common::default(), EntityKind::Insert(cut)).unwrap();
    let nested = d.add(&model, Common::default(), EntityKind::Insert(reference("BNF", [0.0, 0.0]))).unwrap();
    let hidden = d.add(&model, on_hidden(), EntityKind::Insert(reference("BQ", [50.0, 10.0]))).unwrap();
    let dimension = Dimension {
        kind: DimKind::Aligned,
        defpt: Vec3::new(90.0, 20.0, 0.0),
        text_mid: Vec3::ZERO,
        p13: Vec3::new(70.0, 5.0, 0.0),
        p14: Vec3::new(90.0, 5.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: Some("*D5".into()),
        insertion_point: Vec3::ZERO,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let dim = d.add(&model, on_hidden(), EntityKind::Dimension(dimension)).unwrap();
    let framed = d.add(&model, Common::default(), EntityKind::Insert(reference("BNC", [0.0, 0.0]))).unwrap();
    PointHolders { d, array, scaled, leader, clipped, nested, hidden, dim, framed }
}

/// A point on the layer Defpoints counts in every object that draws a block: in the copies of an
/// array of references (MINSERT), in an annotative reference and in the block a multileader
/// shows. AutoCAD 2022 selects each by a pick and a crossing window on such a point with
/// Defpoints frozen in the viewport, and a window must hold the points. They have the
/// coordinates AutoCAD was given and its answers. The frozen layers are given to the selection
/// as a viewport gives them.
#[test]
fn a_point_on_defpoints_counts_in_arrays_annotative_references_and_multileaders() {
    let PointHolders { d, array, scaled, leader, .. } = point_holders();
    let through = ["Defpoints".to_string(), "Hidden".to_string()];
    // The array: the points of the first and of the last copy.
    assert_eq!(picked(&d, &through, [5.0, -27.0]), Some(array), "the point of the first copy");
    assert_eq!(picked(&d, &through, [20.0, -15.0]), Some(array), "the point of the last copy");
    assert_eq!(crossed(&d, &through, [19.5, -15.5], [20.5, -14.5]), [array], "a crossing window over the last copy's point");
    assert_eq!(windowed(&d, &through, [-1.0, -31.0], [26.0, -17.0]), [], "a window that leaves out the upper row's points");
    assert_eq!(windowed(&d, &through, [-1.0, -31.0], [26.0, -14.0]), [array], "a window around all of the array");
    // The annotative reference.
    assert_eq!(picked(&d, &through, [45.0, -27.0]), Some(scaled), "the point of the annotative reference");
    assert_eq!(crossed(&d, &through, [44.5, -27.5], [45.5, -26.5]), [scaled], "a crossing window over it");
    assert_eq!(windowed(&d, &through, [39.0, -31.0], [51.0, -29.0]), [], "a window around its line alone");
    assert_eq!(windowed(&d, &through, [39.0, -31.0], [51.0, -26.0]), [scaled], "a window around its line and its point");
    // The multileader: the point of its block lies at (75.36, -11), its line runs from (72.36, -19).
    assert_eq!(picked(&d, &through, [75.36, -11.0]), Some(leader), "the point of the multileader's block");
    assert_eq!(crossed(&d, &through, [74.86, -11.5], [75.86, -10.5]), [leader], "a crossing window over it");
    assert_eq!(picked(&d, &through, [75.36, -19.0]), Some(leader), "the line of its block");
    assert_eq!(windowed(&d, &through, [58.0, -32.0], [80.36, -13.0]), [], "a window around its leader and the block's line alone");
    assert_eq!(windowed(&d, &through, [58.0, -32.0], [80.36, -9.0]), [leader], "a window around all of it");
}

/// A point on Defpoints is kept only where its block is drawn. AutoCAD 2022 does not select a
/// clipped reference (XCLIP) by such a point outside the clip boundary, a reference by such a
/// point in a nested reference whose own layer the viewport freezes, or a reference or a
/// dimension that is itself on a layer the viewport freezes; inside the boundary of the clipped
/// reference the point is hit. They have the coordinates AutoCAD was given and its answers. The
/// frozen layers are given to the selection as a viewport gives them.
#[test]
fn a_point_on_defpoints_is_not_kept_where_its_block_is_not_drawn() {
    let PointHolders { d, clipped, nested, hidden, dim, .. } = point_holders();
    let through = ["Defpoints".to_string(), "Hidden".to_string()];
    // The clipped reference: the point inside the boundary and the one outside.
    assert_eq!(picked(&d, &through, [5.0, 13.0]), Some(clipped), "the point inside the clip boundary");
    assert_eq!(picked(&d, &through, [15.0, 13.0]), None, "the point outside the clip boundary");
    assert_eq!(crossed(&d, &through, [14.5, 12.5], [15.5, 13.5]), [], "a crossing window over it");
    assert_eq!(windowed(&d, &through, [-4.0, 5.0], [12.0, 18.0]), [clipped], "a window around the boundary");
    // The nested reference on "Hidden": nothing of it is drawn, its point included.
    assert_eq!(picked(&d, &through, [35.0, 18.0]), None, "the point of a nested reference on a frozen layer");
    assert_eq!(crossed(&d, &through, [34.5, 17.5], [35.5, 18.5]), [], "a crossing window over it");
    assert_eq!(picked(&d, &through, [35.0, 15.0]), None, "the line of that nested reference");
    assert_eq!(picked(&d, &through, [35.0, 10.0]), Some(nested), "the reference's own line");
    assert_eq!(windowed(&d, &through, [29.0, 9.0], [41.0, 11.0]), [nested], "a window around its own line alone");
    assert_eq!(picked(&d, &[], [35.0, 18.0]), Some(nested), "where the layers are drawn");
    // A reference and a dimension that are on "Hidden" themselves.
    assert_eq!(picked(&d, &through, [55.0, 13.0]), None, "the point of a reference on a frozen layer");
    assert_eq!(picked(&d, &through, [55.0, 10.0]), None, "its line");
    assert_eq!(picked(&d, &[], [55.0, 13.0]), Some(hidden), "where the layers are drawn");
    assert_eq!(picked(&d, &through, [70.0, 5.0]), None, "a definition point of a dimension on a frozen layer");
    assert_eq!(crossed(&d, &through, [69.5, 4.5], [70.5, 5.5]), [], "a crossing window over it");
    assert_eq!(picked(&d, &through, [75.0, 20.0]), None, "its dimension line");
    assert_eq!(picked(&d, &[], [70.0, 5.0]), Some(dim), "where the layers are drawn");
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
    let PointHolders { d, framed, .. } = point_holders();
    let through = ["Defpoints".to_string(), "Hidden".to_string()];
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

/// A proxy that is a part of a block draws its proxy graphics as the block draws its other parts,
/// so through a viewport the reference is measured and hit by the graphics that viewport draws:
/// with the proxy's line on a layer frozen there, a window around the reference's own line alone
/// takes it and a pick on the proxy's line finds nothing. Where the layer is drawn that window
/// misses the reference and the pick takes it. A proxy is never selected itself (it is kept as it
/// was read), so it counts only this way. The frozen layers are given to the selection as a
/// viewport gives them.
#[test]
fn a_proxy_in_a_block_counts_by_the_graphics_the_viewport_draws() {
    use cadcraft_engine::doc::{Block, Common, Drawing, Entity, EntityKind, Insert, Line, Space, Unknown};
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Hidden");
    let line = |y: f64, layer: &str| {
        let mut g = Entity::new(Handle(0), EntityKind::Line(Line { a: Vec3::new(0.0, y, 0.0), b: Vec3::new(10.0, y, 0.0) }));
        g.common.layer = layer.into();
        g
    };
    let proxy = Entity::new(
        Handle(2),
        EntityKind::Unknown(Unknown { dxf_type: "ACAD_PROXY_ENTITY".into(), tags: Vec::new(), graphics: vec![line(5.0, "Hidden")] }),
    );
    let mut block = Block::new("BX");
    block.entities.push(Entity { handle: Handle(1), ..line(0.0, "0") });
    block.entities.push(proxy);
    d.blocks.insert("BX".into(), std::sync::Arc::new(block));
    let reference = d.add(&Space::Model, Common::default(), EntityKind::Insert(Insert { block: "BX".into(), ..Insert::default() })).unwrap();
    let through = ["Hidden".to_string()];
    assert_eq!(windowed(&d, &through, [-1.0, -1.0], [11.0, 1.0]), [reference], "a window around the reference's own line alone");
    assert_eq!(picked(&d, &through, [5.0, 5.0]), None, "a pick on the proxy's line");
    assert_eq!(picked(&d, &through, [5.0, 0.0]), Some(reference), "a pick on the reference's own line");
    assert_eq!(windowed(&d, &[], [-1.0, -1.0], [11.0, 1.0]), [], "where the layer is drawn, the same window");
    assert_eq!(picked(&d, &[], [5.0, 5.0]), Some(reference), "and the same pick");
}

/// A dimension is picked anywhere inside the box of its text, and that text is a part of its
/// block. Through a viewport that freezes the text's layer the text is not drawn: a pick in its
/// box finds nothing there and selection cycling lists nothing, while the lines of the dimension
/// are picked as before and a window around all of it takes it. Where the text is drawn a pick
/// in its middle takes the dimension. AutoCAD 2022 was given this dimension, with the text of
/// its block moved to "Hidden", and answers every case asserted here the same way. The block is
/// the one it made, without the arrowheads and the definition points. The frozen layers are
/// given to the selection as a viewport gives them.
///
/// A crossing window inside the box of that text takes nothing through the viewport, as it took
/// nothing before a selection counted the viewport's layers and as AutoCAD answers: the rectangle
/// around the dimension, which is hit along its sides as the rectangle around a block reference
/// is (#2991), keeps the room of the text the viewport leaves out, so no side of it runs through
/// that box. Its top side, at the top of that room, is hit by a pick, selection cycling, a
/// crossing window, a fence and a crossing polygon beside the text, as where the text is drawn. A
/// window polygon takes what a window of the same shape takes.
///
/// One answer of AutoCAD is not matched and not asserted: with the layer frozen for the whole
/// drawing or turned off it picks nothing in that box either, where CADCraft picks the dimension
/// as it did before this change, which leaves what a drawing-wide layer state does alone.
#[test]
fn a_dimension_is_not_picked_on_text_that_is_not_drawn() {
    use cadcraft_engine::doc::{Block, Common, DimKind, Dimension, Drawing, Entity, EntityKind, Line, MText, Space};
    use cadcraft_engine::geom::Vec3;
    let mut d = Drawing::new_metric();
    d.ensure_layer("Hidden");
    // A horizontal dimension from (0, 5) and (40, 15) with its line along y = 40 and text 3 high:
    // two extension lines, the two halves of the dimension line, and the text between them.
    let mut block = Block::new("*D5");
    let lines = [([0.0, 5.063], [0.0, 40.18]), ([40.0, 15.063], [40.0, 40.18]), ([2.0, 40.0], [12.462, 40.0]), ([38.0, 40.0], [27.538, 40.0])];
    for (handle, (a, b)) in (1..).zip(lines) {
        let mut part = Entity::new(Handle(handle), EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) }));
        part.common.layer = "0".into();
        block.entities.push(part);
    }
    let words = MText {
        insert: Vec3::new(20.0, 40.0, 0.0),
        height: 3.0,
        width: 0.0,
        attach: 5,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "\\A1;40.0000".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: None,
    };
    let mut text = Entity::new(Handle(5), EntityKind::MText(words));
    text.common.layer = "Hidden".into();
    block.entities.push(text);
    d.blocks.insert("*D5".into(), std::sync::Arc::new(block));
    let dimension = Dimension {
        kind: DimKind::Linear { rotation: 0.0 },
        defpt: Vec3::new(40.0, 40.0, 0.0),
        text_mid: Vec3::new(20.0, 40.0, 0.0),
        p13: Vec3::new(0.0, 5.0, 0.0),
        p14: Vec3::new(40.0, 15.0, 0.0),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        text: String::new(),
        style: "Standard".into(),
        measurement: 0.0,
        text_rotation: 0.0,
        user_text_pos: false,
        block: Some("*D5".into()),
        insertion_point: Vec3::ZERO,
        overrides: Default::default(),
        assoc: Vec::new(),
        marks: Default::default(),
    };
    let dim = d.add(&Space::Model, Common::default(), EntityKind::Dimension(dimension)).unwrap();
    let through = ["Hidden".to_string()];
    let (middle, dimension_line, extension_line) = ([20.0, 40.0], [3.0, 40.0], [0.0, 20.0]);
    let cycled = |frozen: &[String], p: [f64; 2]| select::pick_all_in(&d, &Space::Model, frozen, Vec2::new(p[0], p[1]), 0.2);
    // AutoCAD was picked at a grid of 27 points inside the box of the text (14.896 by 3.063
    // there) and took nothing at any of them: three rows of nine, each point more than 2 away
    // from the lines. Its crossing window, from the third point of the lowest row to the seventh
    // of the top row, took nothing either.
    let grid: Vec<[f64; 2]> = (0..9).flat_map(|i| (0..3).map(move |j| [14.704 + 1.324 * f64::from(i), 39.387 + 0.613 * f64::from(j)])).collect();
    let inside_the_text = ([17.352, 39.387], [22.648, 40.613]);
    assert_eq!(picked(&d, &[], middle), Some(dim), "where the text is drawn");
    assert_eq!(cycled(&[], middle), [dim]);
    assert_eq!(picked(&d, &through, middle), None, "a pick in the middle of the text that is not drawn");
    assert_eq!(cycled(&through, middle), [], "selection cycling there");
    for p in grid {
        assert_eq!(picked(&d, &through, p), None, "a pick inside the box of the text at {p:?}");
    }
    assert_eq!(picked(&d, &through, dimension_line), Some(dim), "the dimension line");
    assert_eq!(picked(&d, &through, extension_line), Some(dim), "an extension line");
    assert_eq!(crossed(&d, &through, inside_the_text.0, inside_the_text.1), [], "a crossing window inside the box of the text that is not drawn");
    assert_eq!(crossed(&d, &through, [2.5, 39.0], [3.5, 41.0]), [dim], "a crossing window over the dimension line");
    assert_eq!(windowed(&d, &through, [-2.0, 3.0], [42.0, 43.0]), [dim], "a window around all of it");
    // The top side of the rectangle the dimension is hit along lies at the top of the text's
    // room, beyond what the viewport draws, where it lay before: a selection that touches that
    // side beside the text takes the dimension as it does where the text is drawn.
    let top_side = [31.0, 42.49];
    let poly = |a: [f64; 2], b: [f64; 2]| [Vec2::new(a[0], a[1]), Vec2::new(b[0], a[1]), Vec2::new(b[0], b[1]), Vec2::new(a[0], b[1])];
    for frozen in [&through[..], &[]] {
        let near = |dx: f64, dy: f64| [top_side[0] + dx, top_side[1] + dy];
        assert_eq!(picked(&d, frozen, top_side), Some(dim), "a pick on the top side, {frozen:?} frozen");
        assert_eq!(cycled(frozen, top_side), [dim], "selection cycling there, {frozen:?} frozen");
        assert_eq!(crossed(&d, frozen, near(-1.0, -0.5), near(1.0, 0.5)), [dim], "a crossing window over it, {frozen:?} frozen");
        assert_eq!(fenced(&d, frozen, near(0.0, -0.5), near(0.0, 0.5)), [dim], "a fence across it, {frozen:?} frozen");
        let crossing_polygon = select::select_polygon_in(&d, &Space::Model, frozen, &poly(near(-1.0, -0.5), near(1.0, 0.5)), true);
        assert_eq!(crossing_polygon, [dim], "a crossing polygon over it, {frozen:?} frozen");
    }
    // A window polygon takes what a window of the same shape takes, through the viewport too.
    for (a, b) in [([-1.0, 4.0], [41.0, 41.0]), ([-2.0, 3.0], [42.0, 43.0])] {
        for frozen in [&through[..], &[]] {
            let window_polygon = select::select_polygon_in(&d, &Space::Model, frozen, &poly(a, b), false);
            assert_eq!(window_polygon, windowed(&d, frozen, a, b), "a window polygon from {a:?} to {b:?}, {frozen:?} frozen");
        }
    }
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
