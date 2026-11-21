//! MTEXT columns and paragraph formatting from the command line, scripts and the JSON form
//! (#532).

use cadcraft_doc::{EntityKind, MLeader, MText, MTextColumnType, MTextColumns};
use cadcraft_engine::Session;
use serde_json::json;

fn last_mtext(s: &Session) -> MText {
    match s.doc().unwrap().model.iter().last().map(|e| e.kind.clone()) {
        Some(EntityKind::MText(t)) => t,
        k => panic!("no mtext: {k:?}"),
    }
}

#[test]
fn static_columns_from_the_command_line() {
    let mut s = Session::new();
    // Total width 21, three columns, gutter 1.5: each column (21 - 2 × 1.5) / 3 = 6 wide.
    s.script("MTEXT 0,10 h 0.5 c s 21 3 1.5 4\nfirst column\n\n").unwrap();
    assert!(s.running.is_none(), "{}", s.prompt_text());
    let t = last_mtext(&s);
    assert!((t.width - 6.0).abs() < 1e-9, "{}", t.width);
    let c = t.columns.expect("columns");
    assert_eq!((c.kind, c.count), (MTextColumnType::Static, 3));
    assert_eq!((c.gutter, c.height), (1.5, 4.0));
    // Gutters wider than the total width are refused and asked again.
    s.cmdline("mtext 0,0 c s 5 3 3").unwrap();
    assert!(s.log.iter().any(|l| l.contains("don't fit in the total width")), "{:?}", s.log);
    s.cmdline("1 2").unwrap();
    assert!(s.prompt_text().contains("Enter text"), "{}", s.prompt_text());
    s.cancel();
}

#[test]
fn dynamic_columns_and_defaults() {
    let mut s = Session::new();
    s.script("MTEXT 0,10 c d 8 0.5 3\nflowing\n\n").unwrap();
    let t = last_mtext(&s);
    assert_eq!(t.width, 8.0);
    let c = t.columns.expect("columns");
    assert_eq!(c.kind, MTextColumnType::Dynamic);
    assert!(c.auto_height);
    assert_eq!((c.gutter, c.height), (0.5, 3.0));
}

#[test]
fn json_columns_and_lists() {
    let mut s = Session::new();
    s.execute("mtext", &json!({"at": [0, 0], "text": "one\ntwo\nthree", "height": 1, "width": 5, "list": "number", "columns": {"type": "static", "count": 2, "gutter": 1, "height": 2}}))
        .unwrap();
    let t = last_mtext(&s);
    assert_eq!(t.contents, "\\pxi-3,l3,t3;1.\tone\\P2.\ttwo\\P3.\tthree");
    assert_eq!(t.columns.as_ref().map(|c| c.count), Some(2));
    s.execute("mtext", &json!({"at": [0, 0], "text": "a\tb", "indent": 2, "tabs": [6, 4], "align": "right"})).unwrap();
    assert_eq!(last_mtext(&s).contents, "\\pxi0,l2,qr,t4,6;a\tb");
    // Columns need a column width and a height; unknown list styles are refused.
    assert!(s.execute("mtext", &json!({"at": [0, 0], "text": "x", "columns": {"type": "static", "height": 2}})).is_err());
    assert!(s.execute("mtext", &json!({"at": [0, 0], "text": "x", "width": 4, "columns": {"type": "static"}})).is_err());
    assert!(s.execute("mtext", &json!({"at": [0, 0], "text": "x", "list": "roman"})).is_err());
}

#[test]
fn columns_shape_the_drawn_text_and_its_extents() {
    let mut s = Session::new();
    let words = "word ".repeat(60);
    s.execute(
        "mtext",
        &json!({"at": [0, 0], "text": words, "height": 1, "width": 10, "columns": {"type": "static", "count": 3, "gutter": 2, "height": 6}}),
    )
    .unwrap();
    let t = last_mtext(&s);
    let d = s.doc().unwrap();
    let l = cadcraft_engine::render::layout_mtext_entity(d, &t);
    // Three columns of 10 with two gutters of 2; text in every column, none below the columns.
    assert!((l.bounds.width() - 34.0).abs() < 1e-9, "{:?}", l.bounds);
    let xs: Vec<f64> = l.strokes.iter().flatten().map(|p| p.x).collect();
    assert!(xs.iter().any(|x| *x > 24.0), "text reaches the third column");
    let e = d.model.iter().last().unwrap();
    let b = cadcraft_doc::entity_bounds(d, e, 0);
    assert!((b.width() - 34.0).abs() < 1e-9 && (b.height() - 6.0).abs() < 1e-9, "{b:?}");
}

#[test]
fn scale_scales_multileader_text_width_and_columns() {
    // #709: SCALE 2 on a multileader's text doubles its width and column lengths like it does
    // for a standalone MTEXT, not only its height.
    let text = MText {
        insert: Default::default(),
        height: 2.0,
        width: 10.0,
        attach: 1,
        rotation: 0.0,
        style: "Standard".into(),
        contents: "hello world".into(),
        line_spacing: 1.0,
        line_spacing_exact: false,
        columns: Some(MTextColumns {
            kind: MTextColumnType::Static,
            count: 2,
            gutter: 1.0,
            height: 5.0,
            auto_height: true,
            heights: vec![3.0, 4.0],
            flow_reversed: false,
        }),
    };
    let mut s = Session::new();
    let ml = MLeader {
        leaders: Vec::new(),
        landing: Default::default(),
        dogleg: 1.0,
        text: Some(text),
        style: "Standard".into(),
        arrow_size: 1.0,
        spline: false,
        block: None,
    };
    let h = s.add_entity(EntityKind::MLeader(ml)).unwrap();
    s.execute("scale", &json!({ "handles": [h.hex()], "base": [0, 0], "factor": 2 })).unwrap();
    let t = match &s.doc().unwrap().entity(h).unwrap().kind {
        EntityKind::MLeader(m) => m.text.clone().unwrap(),
        k => panic!("{k:?}"),
    };
    let c = t.columns.unwrap();
    assert_eq!((t.height, t.width), (4.0, 20.0));
    assert_eq!((c.kind, c.count, c.gutter, c.height, c.heights), (MTextColumnType::Static, 2, 2.0, 10.0, vec![6.0, 8.0]));
}
