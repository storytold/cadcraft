use cadcraft_doc::{EntityKind, MText};
use serde_json::json;

use crate::Session;

/// Three TEXT lines made out of order: "B" lowest, "A" highest. The highest is on layer TOP with
/// height 2.5, the others on layer 0 with height 1, so the first text's properties are told apart.
fn three_lines(s: &mut Session) -> Vec<String> {
    s.execute("layer.new", &json!({"name": "TOP"})).unwrap();
    let h = |r: serde_json::Value| r["handle"].as_str().unwrap().to_string();
    let b = h(s.execute("text", &json!({"at": [0, 0], "text": "B", "height": 1})).unwrap());
    s.execute("layer.current", &json!({"name": "TOP"})).unwrap();
    let a = h(s.execute("text", &json!({"at": [2, 10], "text": "A", "height": 2.5})).unwrap());
    s.execute("layer.current", &json!({"name": "0"})).unwrap();
    let c = h(s.execute("text", &json!({"at": [1, 5], "text": "C{1}", "height": 1})).unwrap());
    vec![b, a, c]
}

fn handles(hs: &[String]) -> Vec<cadcraft_doc::Handle> {
    hs.iter().map(|h| cadcraft_doc::Handle::parse_hex(h).unwrap()).collect()
}

fn mtexts(s: &Session) -> Vec<(String, MText)> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::MText(m) => Some((e.common.layer.clone(), m.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn combines_texts_top_down_into_one_mtext_with_the_first_ones_properties() {
    let mut s = Session::new();
    let hs = three_lines(&mut s);
    let r = s.execute("txt2mtxt", &json!({"handles": hs})).unwrap();
    let doc = s.doc().unwrap();
    assert_eq!(doc.model.len(), 1, "the three texts became one object");
    let ms = mtexts(&s);
    let [(layer, m)] = &ms[..] else { panic!("one MTEXT: {ms:?}") };
    // Highest first whatever the selection order; braces are literal characters; lines of
    // another height than the first keep theirs.
    assert_eq!(m.contents, "A\\P{\\H1;C\\{1\\}}\\P{\\H1;B}");
    assert_eq!((layer.as_str(), m.height, m.attach, m.width), ("TOP", 2.5, 1, 0.0));
    // Top left of the texts: the leftmost start point, the top of the highest text.
    assert_eq!((m.insert.x, m.insert.y), (0.0, 12.5));
    assert_eq!(r["handles"], json!([hs[1]]), "the highest text's object holds the MTEXT");

    s.undo().unwrap();
    let kinds: Vec<_> = s.doc().unwrap().model.iter().map(|e| e.kind.type_name()).collect();
    assert_eq!(kinds, ["Text", "Text", "Text"], "one undo step brings the three texts back");
}

#[test]
fn selection_order_wrap_and_one_mtext_per_text() {
    let mut s = Session::new();
    let hs = three_lines(&mut s);
    s.execute("txt2mtxt", &json!({"handles": hs, "sort": "selection", "wrap": true})).unwrap();
    let ms = mtexts(&s);
    let [(layer, m)] = &ms[..] else { panic!("one MTEXT: {ms:?}") };
    assert_eq!(m.contents, "B {\\H2.5;A} C\\{1\\}", "selection order, one wrapped paragraph");
    assert_eq!((layer.as_str(), m.height), ("0", 1.0), "the first selected text's properties");
    assert!(m.width > 0.0, "word wrap gives the MTEXT a width");
    s.undo().unwrap();

    s.execute("txt2mtxt", &json!({"handles": hs, "combine": false})).unwrap();
    let ms = mtexts(&s);
    assert_eq!(ms.len(), 3, "one MTEXT per TEXT: {ms:?}");
    let a = ms.iter().find(|(_, m)| m.contents == "A").unwrap();
    // A baseline-left TEXT is attached bottom left at its start point, keeping layer and height.
    assert_eq!((a.0.as_str(), a.1.height, a.1.attach, a.1.insert.x, a.1.insert.y), ("TOP", 2.5, 7, 2.0, 10.0));
    assert_eq!(s.doc().unwrap().model.len(), 3);
}

#[test]
fn command_line_settings_and_locked_layers() {
    let mut s = Session::new();
    let hs = three_lines(&mut s);
    let steps = s.state().unwrap().undo.len();
    s.start("txt2mtxt").unwrap();
    assert!(s.prompt_text().contains("Select objects or [SEttings]"), "{}", s.prompt_text());
    s.cmdline("se").unwrap();
    assert!(s.prompt_text().contains("Combine text into a single MTEXT? [Yes/No] <Yes>"), "{}", s.prompt_text());
    s.cmdline("n").unwrap();
    assert!(s.prompt_text().contains("Select objects"), "sorting and wrapping are skipped: {}", s.prompt_text());
    s.input(crate::Input::Pick(handles(&hs))).unwrap();
    s.cmdline("").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    assert_eq!(mtexts(&s).len(), 3);
    assert_eq!(s.state().unwrap().undo.len(), steps + 1, "one undo step");
    s.undo().unwrap();

    // The setting is remembered; a text on a locked layer is left alone.
    s.execute("layer.set", &json!({"name": "TOP", "locked": true})).unwrap();
    s.start("txt2mtxt").unwrap();
    s.cmdline("se").unwrap();
    assert!(s.prompt_text().contains("<No>"), "{}", s.prompt_text());
    s.cmdline("y").unwrap();
    assert!(s.prompt_text().contains("Sort text top-down? [Yes/No] <Yes>"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    assert!(s.prompt_text().contains("Word-wrap text? [Yes/No] <No>"), "{}", s.prompt_text());
    s.cmdline("").unwrap();
    s.input(crate::Input::Pick(handles(&hs))).unwrap();
    s.cmdline("").unwrap();
    let ms = mtexts(&s);
    let [(_, m)] = &ms[..] else { panic!("one MTEXT: {ms:?}") };
    assert_eq!(m.contents, "C\\{1\\}\\PB");
    assert_eq!(s.doc().unwrap().model.len(), 2, "the locked text stays a TEXT");

    let n = s.doc().unwrap().model.len();
    for p in [json!({"handles": []}), json!({"handles": [hs[1]]}), json!({"handles": hs, "sort": "sideways"})] {
        assert!(s.execute("txt2mtxt", &p).is_err(), "{p}");
    }
    assert_eq!(s.doc().unwrap().model.len(), n);
}

/// The vertical extent of each group of strokes that overlap vertically: one per line of
/// capitals, top line first.
fn line_extents(strokes: &[Vec<cadcraft_geom::Vec2>]) -> Vec<f64> {
    let mut spans: Vec<(f64, f64)> = strokes
        .iter()
        .filter(|st| !st.is_empty())
        .map(|st| st.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y))))
        .collect();
    spans.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut lines: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match lines.last_mut() {
            Some(l) if hi >= l.0 => l.0 = l.0.min(lo),
            _ => lines.push((lo, hi)),
        }
    }
    lines.iter().map(|(lo, hi)| hi - lo).collect()
}

#[test]
fn combined_lines_keep_their_own_heights() {
    let mut s = Session::new();
    for (y, t, h) in [(0, "LOW", 3.5), (20, "TOP", 5.0), (10, "MID", 3.5)] {
        s.execute("text", &json!({"at": [0, y], "text": t, "height": h})).unwrap();
    }
    s.execute("txt2mtxt", &json!({"handles": s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect::<Vec<_>>()})).unwrap();
    let ms = mtexts(&s);
    let [(_, m)] = &ms[..] else { panic!("one MTEXT: {ms:?}") };
    assert_eq!((m.height, m.contents.as_str()), (5.0, "TOP\\P{\\H3.5;MID}\\P{\\H3.5;LOW}"));
    // Drawn, each line is as tall as the TEXT it came from.
    let drawn = |c: &str, h: f64| cadcraft_fonts::layout_mtext(c, m.insert.xy(), h, 0.0, 1, 0.0, 1.0).strokes;
    let got = line_extents(&drawn(&m.contents, m.height));
    let want: Vec<f64> = [("TOP", 5.0), ("MID", 3.5), ("LOW", 3.5)].iter().flat_map(|(t, h)| line_extents(&drawn(t, *h))).collect();
    assert_eq!(got.len(), 3, "{got:?}");
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 1e-6, "line heights {got:?}, TEXT heights {want:?}");
    }
    assert!(want[1] < want[0] * 0.8, "the fixture's heights differ: {want:?}");
    s.undo().unwrap();

    // One MTEXT per TEXT: each its own height.
    let hs: Vec<String> = s.doc().unwrap().model.iter().map(|e| e.handle.hex()).collect();
    s.execute("txt2mtxt", &json!({"handles": hs, "combine": false})).unwrap();
    let mut got: Vec<(String, f64)> = mtexts(&s).into_iter().map(|(_, m)| (m.contents, m.height)).collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, [("LOW".to_string(), 3.5), ("MID".to_string(), 3.5), ("TOP".to_string(), 5.0)]);
}
