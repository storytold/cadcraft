//! OFFSET keeps a closed (periodic) spline closed (issue #303): the offset copy was built with
//! the open-spline interpolation, so it came out open with a seam where the curve started.

use cadcraft_engine::Session;
use cadcraft_engine::doc::EntityKind;
use cadcraft_engine::geom::Spline;
use serde_json::json;

fn last_spline(s: &Session) -> (String, Spline) {
    let d = s.doc().unwrap();
    let Some(e) = d.model.iter().last() else { panic!("empty drawing") };
    let EntityKind::Spline(sp) = &e.kind else { panic!("not a spline: {:?}", e.kind) };
    (e.handle.hex(), sp.clone())
}

#[test]
fn offset_of_closed_spline_is_closed() {
    let mut s = Session::new();
    s.cmdline("spline 0,0 10,0 10,10 0,10 c").unwrap();
    let (h, src) = last_spline(&s);
    assert!(src.closed && src.is_periodic());
    for (side, outward) in [([5.0, 5.0], false), ([30.0, 5.0], true)] {
        s.execute("offset", &json!({ "handle": h, "distance": 1, "side": side })).unwrap();
        let (_, off) = last_spline(&s);
        assert!(off.closed && off.is_periodic(), "offset copy is open: {off:?}");
        let (lo, hi) = off.domain();
        assert!(off.eval(lo).near(off.eval(hi), 1e-9), "offset copy has a gap at the seam");
        // The copy lies 1 unit inside / outside the original everywhere.
        let (sb, ob) = (src.bounds(), off.bounds());
        let grow = ob.max.x - sb.max.x;
        assert!((grow - if outward { 1.0 } else { -1.0 }).abs() < 0.1, "{grow}");
    }
}
