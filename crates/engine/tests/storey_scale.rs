//! What storeys cost per command on a large drawing: the same commands on 100 000 objects,
//! once without storeys and once on four storeys with a blueprint. Run by hand (release):
//!
//!     cargo test --release -p cadcraft-engine --test storey_scale -- --ignored --nocapture

use std::time::{Duration, Instant};

use cadcraft_engine::Session;
use cadcraft_engine::doc::geom::Vec3;
use cadcraft_engine::doc::{Common, EntityKind, Line, Space};
use serde_json::json;

const OBJECTS: usize = 100_000;

fn drawing(storeys: bool) -> Session {
    let mut s = Session::new();
    if storeys {
        for n in ["UG", "EG", "OG1", "OG2"] {
            s.execute("storey.new", &json!({ "name": n })).unwrap();
        }
    }
    let ids: Vec<u32> = s.doc().unwrap().storeys.list.iter().map(|x| x.id).collect();
    let d = s.doc_mut().unwrap();
    for i in 0..OBJECTS {
        let mut c = Common::default();
        if !ids.is_empty() {
            c.set_storey(Some(ids[i % ids.len()]));
        }
        let x = (i % 1000) as f64;
        let y = (i / 1000) as f64;
        d.add(&Space::Model, c, EntityKind::Line(Line { a: Vec3::new(x, y, 0.0), b: Vec3::new(x + 0.5, y, 0.0) })).unwrap();
    }
    if storeys {
        s.execute("storey.ghost", &json!({ "name": "EG" })).unwrap();
    }
    s
}

/// 200 lines drawn, 100 moves of the last one, 50 storey switches (storeys only): mean per command.
fn run(s: &mut Session, storeys: bool) -> (Duration, Duration, Option<Duration>) {
    let t = Instant::now();
    let mut last = String::new();
    for i in 0..200 {
        let r = s.execute("line", &json!({ "points": [[i, -5], [i, -6]] })).unwrap();
        last = r["handles"][0].as_str().unwrap().to_string();
    }
    let draw = t.elapsed() / 200;
    let t = Instant::now();
    for _ in 0..100 {
        s.execute("move", &json!({ "handles": [last], "delta": [0.1, 0] })).unwrap();
    }
    let mv = t.elapsed() / 100;
    let switch = storeys.then(|| {
        let t = Instant::now();
        for i in 0..50 {
            s.execute(if i % 2 == 0 { "storey.up" } else { "storey.down" }, &json!({})).unwrap();
        }
        t.elapsed() / 50
    });
    (draw, mv, switch)
}

#[test]
#[ignore = "a measurement, run by hand in release"]
fn storeys_per_command_on_100k_objects() {
    let mut plain = drawing(false);
    let (d0, m0, _) = run(&mut plain, false);
    let mut with = drawing(true);
    let (d1, m1, sw) = run(&mut with, true);
    println!("{OBJECTS} objects — without storeys: LINE {d0:?}, MOVE {m0:?}");
    println!("{OBJECTS} objects — four storeys + blueprint: LINE {d1:?}, MOVE {m1:?}, switch {sw:?}");
    println!("extra per command: LINE {:?}, MOVE {:?}", d1.saturating_sub(d0), m1.saturating_sub(m0));
}
