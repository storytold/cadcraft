//! MLEADER starts with the placement order and Options chosen the last time (#247 follow-up).

use cadcraft_engine::Session;
use cadcraft_engine::doc::{EntityKind, MLeader};
use cadcraft_engine::geom::Vec2;
use serde_json::json;

fn last_mleader(s: &Session) -> MLeader {
    match &s.doc().unwrap().model.last().unwrap().kind {
        EntityKind::MLeader(m) => m.clone(),
        other => panic!("not a multileader: {other:?}"),
    }
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn mleader_remembers_order_and_options() {
    let mut s = Session::new();
    // Landing first, then Options: Maxpoints 3, no landing line.
    s.script("mleader l o m 3 a n x 10,5 5,5 0,0 one\n").unwrap();
    let m = last_mleader(&s);
    assert!(near(m.leaders[0][0].xy(), Vec2::ZERO) && near(m.landing.xy(), Vec2::new(10.0, 5.0)), "{m:?}");
    assert_eq!(m.dogleg, 0.0);

    // The next MLEADER asks for the landing first and keeps both options.
    s.cmdline("mleader").unwrap();
    assert!(s.prompt_text().contains("landing location"), "{}", s.prompt_text());
    s.script("20,5 15,5 10,0 two\n").unwrap();
    let m = last_mleader(&s);
    assert_eq!(m.leaders[0].len(), 2, "three points (Maxpoints 3): {m:?}");
    assert!(near(m.leaders[0][0].xy(), Vec2::new(10.0, 0.0)) && near(m.landing.xy(), Vec2::new(20.0, 5.0)), "{m:?}");
    assert_eq!(m.dogleg, 0.0);
    assert_eq!(m.text.unwrap().contents, "two");

    // Switching back to arrowhead first is remembered too, also when the command is cancelled.
    s.script("mleader h\n").unwrap();
    s.cancel();
    s.cmdline("mleader").unwrap();
    assert!(s.prompt_text().contains("arrowhead location"), "{}", s.prompt_text());
    s.cancel();

    // JSON calls keep their documented defaults (landing line drawn).
    s.execute("mleader", &json!({"points": [[0, 0], [5, 5]], "text": "json"})).unwrap();
    assert!(last_mleader(&s).dogleg > 0.0);
}
