//! A HATCH edge path whose edges are listed out of order, or stored end to start, is read as the
//! outline its edges make. Read in file order, its vertex list jumps between edges, and the
//! even-odd fill fills the polygon of those jumps instead.

use cadcraft_doc::{EntityKind, HatchLoop};
use cadcraft_geom::{PolyVertex, Vec2};

/// A boundary edge as a DXF HATCH stores it (DXF Reference, HATCH boundary path data).
#[derive(Clone)]
enum Edge {
    Line([f64; 2], [f64; 2]),
    /// Centre, radius, start and end angle in degrees, counter-clockwise.
    Arc([f64; 2], f64, f64, f64, bool),
    /// Centre, major axis end relative to the centre, minor-to-major ratio, start and end angle
    /// in degrees, counter-clockwise.
    Ellipse([f64; 2], [f64; 2], f64, f64, f64, bool),
    /// Degree, knots, control points.
    Spline(u32, Vec<f64>, Vec<[f64; 2]>),
}

fn line(a: [f64; 2], b: [f64; 2]) -> Edge {
    Edge::Line(a, b)
}

fn groups(edge: &Edge) -> String {
    match edge {
        Edge::Line(a, b) => format!("72\n1\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n", a[0], a[1], b[0], b[1]),
        Edge::Arc(c, r, start, end, ccw) => format!("72\n2\n10\n{}\n20\n{}\n40\n{r}\n50\n{start}\n51\n{end}\n73\n{}\n", c[0], c[1], i32::from(*ccw)),
        Edge::Ellipse(c, major, ratio, start, end, ccw) => format!(
            "72\n3\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n40\n{ratio}\n50\n{start}\n51\n{end}\n73\n{}\n",
            c[0],
            c[1],
            major[0],
            major[1],
            i32::from(*ccw)
        ),
        Edge::Spline(degree, knots, control) => {
            let mut s = format!("72\n4\n94\n{degree}\n73\n0\n74\n0\n95\n{}\n96\n{}\n", knots.len(), control.len());
            for k in knots {
                s.push_str(&format!("40\n{k}\n"));
            }
            for p in control {
                s.push_str(&format!("10\n{}\n20\n{}\n", p[0], p[1]));
            }
            s
        }
    }
}

/// A DXF whose one solid HATCH has one edge path per entry of `paths`.
fn dxf(paths: &[Vec<Edge>]) -> String {
    let mut s = String::from(
        "0\nSECTION\n2\nENTITIES\n0\nHATCH\n5\n100\n100\nAcDbEntity\n8\n0\n100\nAcDbHatch\n10\n0\n20\n0\n30\n0\n210\n0\n220\n0\n230\n1\n2\nSOLID\n70\n1\n71\n0\n",
    );
    s.push_str(&format!("91\n{}\n", paths.len()));
    for (i, edges) in paths.iter().enumerate() {
        s.push_str(&format!("92\n{}\n93\n{}\n", i32::from(i == 0), edges.len()));
        for edge in edges {
            s.push_str(&groups(edge));
        }
        s.push_str("97\n0\n");
    }
    s.push_str("75\n0\n76\n1\n98\n0\n0\nENDSEC\n0\nEOF\n");
    s
}

fn loops_of(d: &cadcraft_doc::Drawing) -> Vec<HatchLoop> {
    let hatch = d.model.iter().find_map(|e| match &e.kind {
        EntityKind::Hatch(h) => Some(h.loops.clone()),
        _ => None,
    });
    hatch.expect("a hatch")
}

fn read_loops(paths: &[Vec<Edge>]) -> Vec<HatchLoop> {
    loops_of(&cadcraft_io::read(dxf(paths).as_bytes(), "edges.dxf").expect("read DXF"))
}

/// The vertices of the one loop of a hatch with one edge path.
fn vertices(edges: &[Edge]) -> Vec<PolyVertex> {
    let loops = read_loops(&[edges.to_vec()]);
    assert_eq!(loops.len(), 1);
    loops.into_iter().next().map(|l| l.vertices).unwrap_or_default()
}

/// The same vertices, up to the rounding of points and bulges computed from other stored values.
fn assert_same(got: &[PolyVertex], expected: &[PolyVertex]) {
    let same = got.len() == expected.len() && got.iter().zip(expected).all(|(a, b)| a.p.near(b.p, 1e-9) && (a.bulge - b.bulge).abs() < 1e-12);
    assert!(same, "got {got:?}\nexpected {expected:?}");
}

/// Whether the even-odd fill of loops of straight segments covers `p`.
fn filled(loops: &[HatchLoop], p: Vec2) -> bool {
    let mut inside = false;
    for l in loops {
        assert!(l.vertices.iter().all(|v| v.bulge == 0.0), "straight segments only");
        for (a, b) in l.vertices.iter().zip(l.vertices.iter().cycle().skip(1)) {
            let (a, b) = (a.p, b.p);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
    }
    inside
}

const A: [f64; 2] = [0.0, 0.0];
const B: [f64; 2] = [10.0, 0.0];
const C: [f64; 2] = [10.0, 10.0];
const D: [f64; 2] = [0.0, 10.0];

#[test]
fn edges_listed_out_of_order_read_as_the_outline() {
    let in_order = vertices(&[line(A, B), line(B, C), line(C, D), line(D, A)]);
    // Read in file order, this is A, B, D, C, B, C, A, D: its even-odd fill is an hourglass.
    let shuffled = vertices(&[line(A, B), line(D, C), line(B, C), line(A, D)]);
    assert_same(&shuffled, &in_order);
    let loops = read_loops(&[vec![line(A, B), line(D, C), line(B, C), line(A, D)]]);
    for p in [[5.0, 1.0], [5.0, 9.0], [1.0, 5.0], [9.0, 5.0]] {
        assert!(filled(&loops, Vec2::new(p[0], p[1])), "{p:?}");
    }
}

#[test]
fn edges_stored_end_to_start_are_turned_around() {
    let p = [[0.0, 0.0], [10.0, 0.0], [15.0, 8.0], [10.0, 16.0], [0.0, 16.0], [-5.0, 8.0]];
    let in_order = vertices(&[line(p[0], p[1]), line(p[1], p[2]), line(p[2], p[3]), line(p[3], p[4]), line(p[4], p[5]), line(p[5], p[0])]);
    // The second and third edges are both stored end to start, and the last one too.
    let edges = [line(p[0], p[1]), line(p[2], p[1]), line(p[3], p[2]), line(p[3], p[4]), line(p[4], p[5]), line(p[0], p[5])];
    assert_same(&vertices(&edges), &in_order);
    // Around the two reversed edges in a row, a vertex list in file order (..., P1, P2, P1, P3,
    // P2, P3, ...) leaves the triangle P1 P2 P3 unfilled.
    let loops = read_loops(&[edges.to_vec()]);
    assert!(filled(&loops, Vec2::new(11.0, 7.0)));
}

#[test]
fn arcs_ellipse_arcs_and_splines_stored_end_to_start_are_turned_around() {
    // A loop round counter-clockwise: the bottom, a half circle out to the right, the top, and
    // half an ellipse out to the left.
    let arc = Edge::Arc([10.0, 5.0], 5.0, -90.0, 90.0, true);
    let ellipse = Edge::Ellipse([0.0, 5.0], [0.0, 5.0], 0.4, 0.0, 180.0, true);
    let in_order = vertices(&[line(A, B), arc, line(C, D), ellipse]);
    // The same half circle from (10, 10) to (10, 0) clockwise: a clockwise arc stores its angles
    // negated. The same half ellipse from (0, 0) to (0, 10) clockwise.
    let arc_back = Edge::Arc([10.0, 5.0], 5.0, 270.0, 90.0, false);
    let ellipse_back = Edge::Ellipse([0.0, 5.0], [0.0, 5.0], 0.4, 180.0, 360.0, false);
    let got = vertices(&[line(A, B), arc_back, line(D, C), ellipse_back]);
    assert_same(&got, &in_order);
    assert!(in_order.iter().any(|v| v.bulge > 0.5), "the half circle is a bulge: {in_order:?}");
    // The same half circle found before the run's first edge: an open path listed from its
    // last edge.
    let arc = Edge::Arc([10.0, 5.0], 5.0, -90.0, 90.0, true);
    let in_order = vertices(&[arc, line(C, D)]);
    assert_same(&vertices(&[line(C, D), Edge::Arc([10.0, 5.0], 5.0, 270.0, 90.0, false)]), &in_order);

    // A Bezier spline and the same curve with its control points reversed.
    let control = vec![[10.0, 0.0], [10.0, 8.0], [0.0, 8.0], [0.0, 0.0]];
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let spline = Edge::Spline(3, knots.clone(), control.clone());
    let spline_back = Edge::Spline(3, knots, control.into_iter().rev().collect());
    let in_order = vertices(&[line(A, B), spline]);
    assert!(in_order.len() > 8, "{in_order:?}");
    assert_same(&vertices(&[line(A, B), spline_back]), &in_order);
}

#[test]
fn ends_that_differ_by_rounding_still_meet() {
    // A 1,000 by 10 rectangle whose corners are 5e-4 apart: half a millionth of its length,
    // far more than the 1e-9 within which two points are one vertex, so each joint keeps both
    // points.
    let (x, y, w, h, e) = (5e5, -3e5, 1e3, 10.0, 5e-4);
    let p = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    let q = |i: usize| [p[i][0] + e, p[i][1] - e];
    let edges = [line(p[0], p[1]), line(q(1), p[2]), line(q(2), p[3]), line(q(3), q(0))];
    let in_order = vertices(&edges);
    assert_eq!(in_order.len(), 8, "{in_order:?}");
    let shuffled = vertices(&[edges[0].clone(), line(p[3], q(2)), edges[3].clone(), line(p[2], q(1))]);
    assert_same(&shuffled, &in_order);
}

#[test]
fn an_end_just_past_the_tolerance_does_not_meet() {
    // A 1,000-wide path: ends meet within 1e-3. The triangle A T C is stored with a gap of 5e-4
    // at T; a second triangle has a corner X 1.7e-3 from T, in the next lookup cell. Listed
    // before the edge that does meet T, X's edge must still not be joined there.
    let (a, t, t2, c) = ([0.0, 0.0], [500.0002, 0.0], [500.0007, 0.0], [250.0, 10.0]);
    let (x, r, q) = ([500.0019, 0.0], [1000.0, 0.0], [750.0, 10.0]);
    let after = vertices(&[line(a, t), line(t2, c), line(c, a), line(q, x), line(x, r), line(r, q)]);
    let before = vertices(&[line(a, t), line(q, x), line(t2, c), line(c, a), line(x, r), line(r, q)]);
    assert_same(&before, &after);
}

/// Line edges joined in file order: each edge's start unless the list already ends there, then
/// its end, and the closing point dropped.
fn in_file_order(edges: &[Edge]) -> Vec<PolyVertex> {
    let mut out: Vec<PolyVertex> = Vec::new();
    for edge in edges {
        let Edge::Line(a, b) = edge else { panic!("lines only") };
        for p in [a, b] {
            let p = Vec2::new(p[0], p[1]);
            if out.last().is_none_or(|last| !last.p.near(p, 1e-9)) {
                out.push(PolyVertex::new(p));
            }
        }
    }
    if out.len() > 1 && out[0].p.near(out[out.len() - 1].p, 1e-9) {
        out.pop();
    }
    out
}

#[test]
fn a_loop_whose_lookups_take_too_long_is_read_in_file_order() {
    // Petals from a point Q out and back, and many edges from two points 1.4 tolerances either
    // side of Q, listed between the first petal and the rest: every return to Q looks at those
    // on one side (the other is two cells away). Past the lookup budget the loop is read in file
    // order, as before chaining.
    let at = |r: f64, k: usize, n: usize| {
        let a = std::f64::consts::TAU * k as f64 / n as f64;
        [r * a.cos(), r * a.sin()]
    };
    let (q, side) = ([0.0, 0.0], 2.8e-3);
    let mut edges = vec![line(q, at(1000.0, 0, 1000))];
    for i in 0..2000 {
        edges.push(line([if i % 2 == 0 { side } else { -side }, 0.0], at(500.0, i, 2000)));
    }
    edges.push(line(at(1000.0, 0, 1000), q));
    for k in 1..1000 {
        edges.push(line(q, at(1000.0, k, 1000)));
        edges.push(line(at(1000.0, k, 1000), q));
    }
    assert_eq!(vertices(&edges), in_file_order(&edges));
}

#[test]
fn an_open_path_is_chained_from_both_ends() {
    // Three sides of the square, A to D, listed from the middle: the edges before the first
    // listed one go in front, as stored or turned around.
    let in_order = vertices(&[line(A, B), line(B, C), line(C, D)]);
    assert_same(&vertices(&[line(B, C), line(C, D), line(B, A)]), &in_order);
    assert_same(&vertices(&[line(C, D), line(B, C), line(A, B)]), &in_order);
}

#[test]
fn a_path_whose_edges_run_on_keeps_its_order_and_its_points() {
    // Clockwise, starting at C, with a gap of 1e-6 at D: within a millionth of the outline
    // (1e-5), so the edges run on, but more than 1e-9, so both points stay.
    let d_off = [D[0], D[1] + 1e-6];
    let edges = [line(C, B), line(B, A), line(A, D), line(d_off, C)];
    let got = vertices(&edges);
    let expected: Vec<PolyVertex> = [C, B, A, D, d_off].iter().map(|p| PolyVertex::new(Vec2::new(p[0], p[1]))).collect();
    assert_eq!(got, expected);
}

#[test]
fn separate_outlines_in_one_path_fill_separately() {
    let square = |x: f64, y: f64| {
        let p = [[x, y], [x + 10.0, y], [x + 10.0, y + 10.0], [x, y + 10.0]];
        [line(p[0], p[1]), line(p[1], p[2]), line(p[2], p[3]), line(p[3], p[0])]
    };
    let [a, b, c] = [square(0.0, 0.0), square(100.0, 0.0), square(50.0, 100.0)];
    // In file order, the jumps from one square to the next and back to the first make a
    // triangle between them, which the even-odd fill filled.
    let in_order: Vec<Edge> = a.iter().chain(&b).chain(&c).cloned().collect();
    // The same squares, their edges interleaved and the third square's turned around.
    let mixed = vec![
        a[0].clone(),
        b[2].clone(),
        line([60.0, 110.0], [60.0, 100.0]),
        a[2].clone(),
        b[0].clone(),
        line([50.0, 110.0], [60.0, 110.0]),
        a[1].clone(),
        b[3].clone(),
        line([50.0, 100.0], [50.0, 110.0]),
        a[3].clone(),
        b[1].clone(),
        line([60.0, 100.0], [50.0, 100.0]),
    ];
    for edges in [in_order, mixed] {
        let loops = read_loops(&[edges]);
        for p in [[5.0, 5.0], [105.0, 5.0], [55.0, 105.0]] {
            assert!(filled(&loops, Vec2::new(p[0], p[1])), "{p:?}");
        }
        for p in [[50.0, 30.0], [30.0, 5.0], [55.0, 50.0]] {
            assert!(!filled(&loops, Vec2::new(p[0], p[1])), "{p:?}");
        }
    }
}

#[test]
fn an_outline_left_open_is_closed_on_itself() {
    // A square, then three sides of another listed apart from it: the second is closed by its
    // missing side, not through the first square's corner.
    let b = [[100.0, 0.0], [110.0, 0.0], [110.0, 10.0], [100.0, 10.0]];
    let edges = vec![line(A, B), line(B, C), line(C, D), line(D, A), line(b[1], b[2]), line(b[0], b[1]), line(b[2], b[3])];
    let loops = read_loops(&[edges]);
    assert!(filled(&loops, Vec2::new(5.0, 5.0)));
    assert!(filled(&loops, Vec2::new(105.0, 5.0)));
    assert!(!filled(&loops, Vec2::new(90.0, 5.0)));
}

#[test]
fn outlines_with_missing_edges_are_joined_in_file_order() {
    // The bottom from right to left, then the top and a cut corner from left to right, of an
    // outline whose sides are missing: the two open runs are joined straight in the order they
    // are listed, so the outline still fills.
    let loops = read_loops(&[vec![line(B, A), line(D, [8.0, 10.0]), line([8.0, 10.0], [10.0, 8.0])]]);
    for p in [[5.0, 5.0], [9.0, 2.0], [1.0, 8.0]] {
        assert!(filled(&loops, Vec2::new(p[0], p[1])), "{p:?}");
    }
    assert!(!filled(&loops, Vec2::new(9.8, 9.8)));
}

#[test]
fn a_later_outline_closes_at_its_exact_start() {
    // A square, then a second one 100 to the right whose last edge ends 5e-5 above its first
    // corner: within the tolerance (1.1e-4), so the outline is closed. The jump back to the first
    // square must start from that corner, or the jumps out and back leave a sliver between them.
    let s = [100.0, 0.0];
    let edges = vec![
        line(A, B),
        line(B, C),
        line(C, D),
        line(D, A),
        line(s, [110.0, 0.0]),
        line([110.0, 0.0], [110.0, 10.0]),
        line([110.0, 10.0], [100.0, 10.0]),
        line([100.0, 10.0], [100.0, 5e-5]),
    ];
    let loops = read_loops(&[edges]);
    assert!(filled(&loops, Vec2::new(5.0, 5.0)));
    assert!(filled(&loops, Vec2::new(105.0, 5.0)));
    assert!(!filled(&loops, Vec2::new(50.0, 1.2e-5)));
}

#[test]
fn a_chained_loop_is_saved_as_it_was_read() {
    let in_order = vertices(&[line(A, B), line(B, C), line(C, D), line(D, A)]);
    let shuffled = cadcraft_io::read(dxf(&[vec![line(A, B), line(D, C), line(B, C), line(A, D)]]).as_bytes(), "edges.dxf").expect("read");
    let saved = cadcraft_io::write_dxf(&shuffled);
    let back = loops_of(&cadcraft_io::read(saved.as_bytes(), "saved.dxf").expect("reopen"));
    assert_eq!(back.len(), 1);
    assert_same(&back[0].vertices, &in_order);
    assert_same(&back[0].vertices, &loops_of(&shuffled)[0].vertices);
}

#[test]
#[cfg(not(any(target_arch = "wasm32", all(target_os = "freebsd", target_arch = "aarch64"))))]
fn a_dwg_edge_path_out_of_order_reads_as_the_outline() {
    cadcraft_io::dwg_worker::allow_in_process();
    let in_order = vertices(&[line(A, B), line(B, C), line(C, D), line(D, A)]);
    let text = dxf(&[vec![line(A, B), line(D, C), line(B, C), line(A, D)]]);
    let doc = acadrust::DxfReader::from_reader(std::io::Cursor::new(text.into_bytes())).expect("acadrust reads the DXF").read().expect("parse");
    let dwg = acadrust::DwgWriter::write_to_vec(&doc).expect("acadrust writes a DWG");
    let loops = loops_of(&cadcraft_io::read(&dwg, "edges.dwg").expect("read DWG"));
    assert_eq!(loops.len(), 1);
    assert_same(&loops[0].vertices, &in_order);
}
