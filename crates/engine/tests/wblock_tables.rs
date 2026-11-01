//! WBLOCK writes only the named objects the written geometry uses: the layers, linetypes, text
//! and dimension styles and nested blocks it references (plus the defaults every drawing has),
//! not the source drawing's whole tables and every block definition.

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{DashElement, DimStyle, Drawing, Layer, Linetype, TextStyle};
use serde_json::{Value, json};

/// Writes the written drawing's table names: "layers:a,b|linetypes:…|textstyles:…|dimstyles:…|blocks:…".
fn write(d: &Drawing, _path: &str) -> Result<Vec<u8>, String> {
    let join = |it: &mut dyn Iterator<Item = String>| it.collect::<Vec<_>>().join(",");
    let out = format!(
        "layers:{}|linetypes:{}|textstyles:{}|dimstyles:{}|blocks:{}",
        join(&mut d.layers.iter().map(|l| l.name.clone())),
        join(&mut d.linetypes.iter().map(|l| l.name.clone())),
        join(&mut d.text_styles.iter().map(|l| l.name.clone())),
        join(&mut d.dim_styles.iter().map(|l| l.name.clone())),
        join(&mut d.blocks.values().map(|b| b.name.clone())),
    );
    Ok(out.into_bytes())
}

fn written(s: &mut Session, p: Value) -> Vec<(String, Vec<String>)> {
    let path = std::env::temp_dir().join(format!("cadcraft-wblock-tables-{}.dxf", std::process::id()));
    let mut p = p;
    p["path"] = json!(path.to_string_lossy());
    s.execute("wblock", &p).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    text.split('|')
        .map(|part| {
            let (k, v) = part.split_once(':').unwrap();
            (k.to_string(), v.split(',').filter(|n| !n.is_empty()).map(|n| n.to_ascii_uppercase()).collect())
        })
        .collect()
}

fn names<'a>(t: &'a [(String, Vec<String>)], key: &str) -> &'a [String] {
    &t.iter().find(|(k, _)| k == key).unwrap().1
}

fn handle(v: &Value) -> String {
    v.get("handle").or_else(|| v["handles"].get(0)).and_then(Value::as_str).unwrap().to_string()
}

fn set(s: &mut Session, var: &str, v: &str) {
    s.doc_mut().unwrap().header.set_str(var, v);
}

/// SYM (on SYM-LAYER, with a NOTES text and an insert of NEST) and NEST (on NEST-LAYER, with the
/// complex linetype SHAPED, whose text comes from text style LTFONT) next to unused layers,
/// linetypes, styles and an unused block OTHER.
fn drawing() -> Session {
    let mut s = Session::new();
    {
        let d = s.doc_mut().unwrap();
        d.linetypes.push(Linetype::simple("DASHED2", "dashed", &[0.5, -0.25]));
        let mut shaped = Linetype::simple("SHAPED", "with text", &[0.5, -0.25]);
        shaped.pattern.push(DashElement { text: Some("X".into()), style: Some("LTFONT".into()), ..DashElement::dash(0.0) });
        d.linetypes.push(shaped);
        d.linetypes.push(Linetype::simple("UNUSEDLT", "unused", &[0.1, -0.1]));
        d.layers.push(Layer { name: "SYM-LAYER".into(), linetype: "DASHED2".into(), ..Layer::default() });
        d.layers.push(Layer { name: "NEST-LAYER".into(), ..Layer::default() });
        d.layers.push(Layer { name: "UNUSED1".into(), ..Layer::default() });
        d.layers.push(Layer { name: "UNUSED2".into(), ..Layer::default() });
        for n in ["NOTES", "LTFONT", "UNUSEDTS"] {
            d.text_styles.push(TextStyle { name: n.into(), ..TextStyle::default() });
        }
        d.dim_styles.push(DimStyle { name: "UNUSEDDS".into(), ..DimStyle::default() });
    }

    set(&mut s, "CLAYER", "NEST-LAYER");
    set(&mut s, "CELTYPE", "SHAPED");
    let h = handle(&s.execute("line", &json!({ "points": [[0, 0], [1, 1]] })).unwrap());
    s.execute("block", &json!({ "name": "NEST", "base": [0, 0], "handles": [h], "keep": "delete" })).unwrap();
    set(&mut s, "CELTYPE", "ByLayer");

    set(&mut s, "CLAYER", "SYM-LAYER");
    set(&mut s, "TEXTSTYLE", "NOTES");
    let a = handle(&s.execute("line", &json!({ "points": [[0, 0], [5, 0]] })).unwrap());
    let b = handle(&s.execute("text", &json!({ "at": [0, 1], "text": "SYM", "height": 0.5 })).unwrap());
    let c = handle(&s.execute("insert", &json!({ "name": "NEST", "at": [2, 2] })).unwrap());
    s.execute("block", &json!({ "name": "SYM", "base": [0, 0], "handles": [a, b, c], "keep": "delete" })).unwrap();

    set(&mut s, "CLAYER", "UNUSED1");
    let h = handle(&s.execute("line", &json!({ "points": [[0, 0], [3, 0]] })).unwrap());
    s.execute("block", &json!({ "name": "OTHER", "base": [0, 0], "handles": [h], "keep": "delete" })).unwrap();

    set(&mut s, "CLAYER", "0");
    set(&mut s, "TEXTSTYLE", "Standard");
    s
}

fn check(t: &[(String, Vec<String>)], blocks: &[&str]) {
    let has = |key: &str, n: &str| names(t, key).iter().any(|x| x == n);
    for n in ["0", "SYM-LAYER", "NEST-LAYER"] {
        assert!(has("layers", n), "layer {n} missing: {t:?}");
    }
    for n in ["UNUSED1", "UNUSED2"] {
        assert!(!has("layers", n), "unused layer {n} written: {t:?}");
    }
    for n in ["CONTINUOUS", "DASHED2", "SHAPED"] {
        assert!(has("linetypes", n), "linetype {n} missing: {t:?}");
    }
    assert!(!has("linetypes", "UNUSEDLT"), "unused linetype written: {t:?}");
    for n in ["STANDARD", "NOTES", "LTFONT"] {
        assert!(has("textstyles", n), "text style {n} missing: {t:?}");
    }
    assert!(!has("textstyles", "UNUSEDTS"), "unused text style written: {t:?}");
    assert!(has("dimstyles", "STANDARD"), "dimension style Standard missing: {t:?}");
    assert!(!has("dimstyles", "UNUSEDDS"), "unused dimension style written: {t:?}");
    let mut got: Vec<&str> = names(t, "blocks").iter().map(String::as_str).collect();
    got.sort_unstable();
    assert_eq!(got, blocks, "{t:?}");
}

#[test]
fn wblock_of_a_block_writes_only_what_it_uses() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write, plot: None });
    let mut s = drawing();
    let t = written(&mut s, json!({ "name": "SYM" }));
    check(&t, &["NEST"]);

    // Objects: an insert of SYM keeps SYM and its nested block, not OTHER.
    let h = handle(&s.execute("insert", &json!({ "name": "SYM", "at": [10, 10] })).unwrap());
    let t = written(&mut s, json!({ "handles": [h] }));
    check(&t, &["NEST", "SYM"]);
}
