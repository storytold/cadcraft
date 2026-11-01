//! The AutoLISP conformance corpus (#648): each case's routine runs in a new drawing and must
//! leave exactly the expected entities, layers and printed text. The corpus lives in its own
//! repository; point `CADCRAFT_LISP_CORPUS` at its `cases` directory (or the repository) to run
//! it. Without it the test only says so.

use std::path::{Path, PathBuf};

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{EntityCodec, set_entity_codec};
use cadcraft_engine::doc::Entity;
use serde_json::{Value, json};

fn cases_dir() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("CADCRAFT_LISP_CORPUS")?);
    let cases = root.join("cases");
    Some(if cases.is_dir() { cases } else { root })
}

/// An entity's DXF groups as (code, value) pairs.
fn groups(d: &cadcraft_engine::doc::Drawing, e: &Entity) -> (String, Vec<(i32, Value)>) {
    let (kind, tags) = cadcraft_io::entity_groups(d, e);
    let v = tags
        .into_iter()
        .map(|t| {
            let v = match t.value {
                cadcraft_dxf::Value::Str(s) | cadcraft_dxf::Value::Hex(s) => json!(s),
                cadcraft_dxf::Value::Real(r) => json!(r),
                cadcraft_dxf::Value::Int(i) => json!(i),
                cadcraft_dxf::Value::Bool(b) => json!(i32::from(b)),
            };
            (t.code, v)
        })
        .collect();
    (kind, v)
}

fn num(g: &[(i32, Value)], code: i32) -> Option<f64> {
    g.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.as_f64())
}

fn point(g: &[(i32, Value)], code: i32) -> Value {
    json!([num(g, code).unwrap_or(0.0), num(g, code + 10).unwrap_or(0.0), num(g, code + 20).unwrap_or(0.0)])
}

/// The fields the corpus checks, read from an entity's groups.
fn actual(d: &cadcraft_engine::doc::Drawing, e: &Entity) -> Value {
    let (kind, g) = groups(d, e);
    let s = |code: i32| g.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.as_str().map(str::to_string)).unwrap_or_default();
    let mut o = json!({ "type": kind, "layer": s(8) });
    let m = o.as_object_mut().unwrap();
    match kind.as_str() {
        "LINE" => {
            m.insert("start".into(), point(&g, 10));
            m.insert("end".into(), point(&g, 11));
        }
        "CIRCLE" | "ARC" => {
            m.insert("center".into(), point(&g, 10));
            m.insert("radius".into(), json!(num(&g, 40).unwrap_or(0.0)));
            if kind == "ARC" {
                m.insert("start_angle".into(), json!(num(&g, 50).unwrap_or(0.0)));
                m.insert("end_angle".into(), json!(num(&g, 51).unwrap_or(0.0)));
            }
        }
        "POINT" => {
            m.insert("location".into(), point(&g, 10));
        }
        "LWPOLYLINE" => {
            let xs: Vec<f64> = g.iter().filter(|(c, _)| *c == 10).filter_map(|(_, v)| v.as_f64()).collect();
            let ys: Vec<f64> = g.iter().filter(|(c, _)| *c == 20).filter_map(|(_, v)| v.as_f64()).collect();
            let verts: Vec<Value> = xs.iter().zip(&ys).map(|(x, y)| json!([x, y])).collect();
            m.insert("closed".into(), json!(num(&g, 70).unwrap_or(0.0) as i64 & 1 == 1));
            m.insert("vertices".into(), json!(verts));
        }
        "TEXT" => {
            m.insert("insert".into(), point(&g, 10));
            m.insert("height".into(), json!(num(&g, 40).unwrap_or(0.0)));
            m.insert("rotation".into(), json!(num(&g, 50).unwrap_or(0.0)));
            m.insert("text".into(), json!(s(1)));
        }
        "DIMENSION" => {
            let k = num(&g, 70).unwrap_or(0.0) as i64 & 7;
            m.insert(
                "kind".into(),
                json!(if k == 0 {
                    "linear"
                } else if k == 1 {
                    "aligned"
                } else {
                    "other"
                }),
            );
            m.insert("xline1".into(), point(&g, 13));
            m.insert("xline2".into(), point(&g, 14));
            m.insert("rotation".into(), json!(num(&g, 50).unwrap_or(0.0)));
            m.insert("measurement".into(), json!(num(&g, 42).unwrap_or(0.0)));
        }
        _ => {}
    }
    o
}

fn close(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x.as_f64().unwrap_or(f64::NAN) - y.as_f64().unwrap_or(f64::NAN)).abs() <= tol,
        (Value::Array(x), Value::Array(y)) => {
            // A 2D point compares with z = 0.
            let pad = |v: &Vec<Value>| -> Vec<Value> {
                let mut v = v.clone();
                if v.len() == 2 && v.iter().all(Value::is_number) {
                    v.push(json!(0.0));
                }
                v
            };
            let (x, y) = (pad(x), pad(y));
            x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| close(p, q, tol))
        }
        _ => a == b,
    }
}

/// Closed polylines match as the same cyclic sequence in either direction.
fn same_ring(have: &[Value], want: &[Value], tol: f64) -> bool {
    let n = want.len();
    if have.len() != n {
        return false;
    }
    if n == 0 {
        return true;
    }
    for start in 0..n {
        for dir in [1isize, -1] {
            let ok = (0..n).all(|i| {
                let j = (start as isize + dir * i as isize).rem_euclid(n as isize) as usize;
                close(&have[j], &want[i], tol)
            });
            if ok {
                return true;
            }
        }
    }
    false
}

fn matches(have: &Value, want: &Value, tol: f64) -> bool {
    let Some(w) = want.as_object() else { return false };
    w.iter().all(|(k, v)| {
        let h = have.get(k).unwrap_or(&Value::Null);
        if k == "vertices" && have.get("closed") == Some(&json!(true)) {
            return match (h, v) {
                (Value::Array(h), Value::Array(v)) => same_ring(h, v, tol),
                _ => false,
            };
        }
        close(h, v, tol)
    })
}

fn run_case(dir: &Path) -> Result<(), String> {
    let routine = std::fs::read_to_string(dir.join("routine.lsp")).map_err(|e| e.to_string())?;
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut s = Session::new();
    if let Some(vars) = expected["preconditions"]["sysvars"].as_object() {
        for (k, v) in vars {
            cadcraft_engine::sysvars::set(&mut s, k, v).map_err(|e| format!("setvar {k}: {e}"))?;
        }
    }
    let from = s.log.len();
    match s.lisp_load("routine.lsp", &routine, true) {
        Ok(Some(Err(e))) => return Err(format!("LISP error: {e}\n{:#?}", &s.log[from..])),
        Err(e) => return Err(e.to_string()),
        _ => {}
    }
    let exp = &expected["expect"];
    let tol = exp["tolerance"].as_f64().unwrap_or(1e-9);
    let d = s.doc().map_err(|e| e.to_string())?;
    let mut have: Vec<Value> = d.model.iter().map(|e| actual(d, e)).collect();
    let want = exp["entities"].as_array().cloned().unwrap_or_default();
    let mut missing = Vec::new();
    for w in &want {
        match have.iter().position(|h| matches(h, w, tol)) {
            Some(i) => {
                have.remove(i);
            }
            None => missing.push(w.clone()),
        }
    }
    if !missing.is_empty() || !have.is_empty() {
        return Err(format!(
            "missing {}:\n{}\nunexpected {}:\n{}\nlog:\n{}",
            missing.len(),
            serde_json::to_string_pretty(&missing).unwrap_or_default(),
            have.len(),
            serde_json::to_string_pretty(&have).unwrap_or_default(),
            s.log[from..].join("\n")
        ));
    }
    for l in exp["layers"].as_array().cloned().unwrap_or_default() {
        let name = l["name"].as_str().unwrap_or("");
        let layer = d.layer(name).ok_or_else(|| format!("layer {name} missing"))?;
        if let Some(c) = l["color"].as_i64() {
            let have = match layer.color {
                cadcraft_color::Color::Index(i) => i64::from(i),
                _ => -1,
            };
            if have != c {
                return Err(format!("layer {name}: colour {have}, expected {c}"));
            }
        }
    }
    if let Some(p) = exp["printed"].as_str() {
        let out = s.log[from..].join("\n");
        if !out.contains(p.trim_start_matches('\n')) {
            return Err(format!("printed {p:?} not found in:\n{out}"));
        }
    }
    Ok(())
}

#[test]
fn autolisp_corpus() {
    set_entity_codec(EntityCodec { groups: cadcraft_io::entity_groups, entity: cadcraft_io::entity_from_groups });
    let Some(dir) = cases_dir() else {
        eprintln!("CADCRAFT_LISP_CORPUS is not set: the AutoLISP corpus is skipped");
        return;
    };
    let mut cases: Vec<PathBuf> =
        std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("routine.lsp").is_file()).collect();
    cases.sort();
    assert!(!cases.is_empty(), "no cases in {}", dir.display());
    let mut failed = Vec::new();
    for c in &cases {
        if let Err(e) = run_case(c) {
            failed.push(format!("== {}\n{e}", c.file_name().and_then(|n| n.to_str()).unwrap_or("?")));
        }
    }
    assert!(failed.is_empty(), "{} of {} cases failed:\n{}", failed.len(), cases.len(), failed.join("\n\n"));
}
