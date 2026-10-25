//! EXPORT's SVG `text` option: glyph outlines by default, `<text>` only for `text: "text"`
//! (#1876), handed verbatim to the host's exporter.
//!
//! The `<text>` element itself is checked in `cadcraft_io`'s `svg_text` tests, which build a
//! synthetic outline font (no font files live in the repository); here the drawing is in the
//! stroke font, which is always geometry, so the command-level checks are the forwarded options
//! and the byte-identical default.

use std::sync::Mutex;

use cadcraft_engine::Session;
use cadcraft_engine::cmd::file::{IoHooks, base64_decode, set_export_with, set_io};
use serde_json::{Value, json};

/// The exporter's `(name, text option)` for each call, in order.
static SEEN: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());

/// Install the real exporter behind a recorder, as the apps install it.
fn install() {
    set_io(IoHooks { read: |_, _| Err("no reader in this test".into()), write: |_, _| Ok(b"plain".to_vec()), plot: None });
    set_export_with(|d, name, opts| {
        let text = opts.get("text").and_then(Value::as_str).map(str::to_string);
        if let Ok(mut seen) = SEEN.lock() {
            seen.push((name.to_string(), text));
        }
        cadcraft_io::export_with(d, name, opts).map_err(|e| e.to_string())
    });
}

/// Run EXPORT for SVG and return its bytes as a string.
fn export(s: &mut Session, opts: Value) -> String {
    let r = s.execute("export", &opts).expect("export runs");
    let data = r["data"].as_str().expect("base64 data");
    String::from_utf8(base64_decode(data).expect("valid base64")).expect("UTF-8 SVG")
}

/// Guards #1876: EXPORT writes glyph outlines when no `text` option is given (the default and
/// the `"geometry"` output are the same bytes), forwards `text: "text"` (its opt-in) and unknown
/// values to the host's exporter under the generated name `export.svg`, and returns that
/// exporter's bytes rather than the plain writer's.
#[test]
fn export_keeps_glyphs_unless_text_is_asked_for() {
    install();
    let mut s = Session::new();
    s.cmdline("line 0,0 10,0").unwrap();
    s.execute("text", &json!({ "at": [0.0, 0.0], "text": "A  B", "height": 2.5 })).expect("text runs");

    let default = export(&mut s, json!({ "format": "svg" }));
    assert!(!default.contains("<text"), "glyph outlines are the default: {default}");
    assert_eq!(default, export(&mut s, json!({ "format": "svg", "text": "geometry" })), "the default is the geometry export");
    assert_eq!(default, export(&mut s, json!({ "format": "svg", "text": "kludge" })), "an unknown value falls back to geometry");
    let opted_in = export(&mut s, json!({ "format": "svg", "text": "text" }));
    assert!(opted_in.starts_with("<svg") && opted_in.ends_with("</svg>"), "text mode still writes a document: {opted_in}");

    let seen = SEEN.lock().expect("recorder");
    assert_eq!(
        *seen,
        vec![
            ("export.svg".to_string(), None),
            ("export.svg".to_string(), Some("geometry".to_string())),
            ("export.svg".to_string(), Some("kludge".to_string())),
            ("export.svg".to_string(), Some("text".to_string())),
        ],
        "every option reaches the exporter verbatim"
    );
}
