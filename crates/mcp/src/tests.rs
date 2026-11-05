//! An agent completes realistic drafting tasks through MCP only, and verifies through MCP only.

use serde_json::{Value, json};

use crate::{Headless, Server};

/// Send one JSON-RPC request to the server and return the parsed reply; fails the test if the
/// server does not answer or answers with invalid JSON.
fn call(s: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let r = s.handle_line(&line).expect("reply");
    serde_json::from_str(&r).unwrap()
}

/// Call an MCP tool and return its decoded result: JSON text is parsed, an image is passed
/// through, and a tool error fails the test with the reported message.
fn tool(s: &mut Server, name: &str, args: Value) -> Value {
    let r = call(s, 99, "tools/call", json!({"name": name, "arguments": args}));
    let c = &r["result"]["content"][0];
    assert_ne!(r["result"]["isError"], json!(true), "{name} failed: {c}");
    if c["type"] == "text" { serde_json::from_str(c["text"].as_str().unwrap()).unwrap_or(Value::Null) } else { c.clone() }
}

/// The MCP handshake works: `initialize` names the server, the `initialized` notification is
/// accepted, `tools/list` offers the command line, and an unknown method is -32601.
#[test]
fn lifecycle_and_tools() {
    let mut s = Server::new(Box::new(Headless::default()));
    let init = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(init["result"]["serverInfo"]["name"], "cadcraft");
    assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    assert!(s.is_initialized());
    let tools = call(&mut s, 2, "tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "command_line"));
    let bad = call(&mut s, 3, "nope", json!({}));
    assert_eq!(bad["error"]["code"], -32601);
}

/// Guards that an agent can complete a drawing (plate, holes, layer, text) using only the MCP
/// tools, and verify it only through MCP: counts, selection queries, undo and a PNG render.
#[test]
fn agent_draws_a_plate_with_holes() {
    let mut s = Server::new(Box::new(Headless::default()));
    // Task 1: an outline via the command line, AutoCAD style.
    tool(&mut s, "command_line", json!({"text": "rectang 0,0 100,60"}));
    // Task 2: four holes with JSON.
    for (x, y) in [(10, 10), (90, 10), (10, 50), (90, 50)] {
        tool(&mut s, "execute", json!({"command": "circle", "params": {"center": [x, y], "radius": 4}}));
    }
    // Task 3: a layer and a centre line on it.
    tool(&mut s, "execute", json!({"command": "layer.new", "params": {"name": "Center", "color": "red", "current": true}}));
    tool(&mut s, "command_line", json!({"text": "line -5,30 105,30"}));
    tool(&mut s, "command_line", json!({"text": ""}));
    // Task 4: a script with text. TEXT keeps asking for further lines until an empty one, as in
    // AutoCAD, so the script ends it with a blank line (= Enter).
    let r = tool(&mut s, "script", json!({"text": "TEXT 0,-10 5 0 PLATE 100x60\n\n"}));
    assert_eq!(r["state"]["running"], Value::Null, "TEXT must be finished: {r}");
    // Verify only through MCP.
    let d = tool(&mut s, "inspect_drawing", json!({}));
    assert_eq!(d["counts"]["Circle"], 4);
    assert_eq!(d["counts"]["Polyline"], 1);
    assert_eq!(d["counts"]["Line"], 1);
    assert_eq!(d["counts"]["Text"], 1);
    let lines = tool(&mut s, "query_entities", json!({"type": "Line", "layer": "Center"}));
    assert_eq!(lines["count"], 1);
    // Task 5: undo the text, check, then render.
    let u = tool(&mut s, "execute", json!({"command": "undo"}));
    assert_eq!(u["undone"], json!(["Single Line Text"]), "{u}");
    let d = tool(&mut s, "inspect_drawing", json!({"entities": false}));
    assert!(d["counts"].get("Text").is_none());
    assert_eq!(d["counts"]["Circle"], 4);
    assert_eq!(d["counts"]["Polyline"], 1);
    assert_eq!(d["counts"]["Line"], 1);
    let lines = tool(&mut s, "query_entities", json!({"type": "Line", "layer": "Center"}));
    assert_eq!(lines["count"], 1, "the centre line must survive undoing the text");
    let img = tool(&mut s, "render", json!({"width": 320, "height": 200}));
    assert_eq!(img["type"], "image");
    assert!(img["data"].as_str().unwrap().len() > 100);
}

/// Guards that bad input is reported, not fatal: a refused command and a screenshot without a
/// UI are `isError` results, and malformed JSON is a parse error (-32700).
#[test]
fn errors_are_tool_errors_not_crashes() {
    let mut s = Server::new(Box::new(Headless::default()));
    let r = call(&mut s, 1, "tools/call", json!({"name": "execute", "arguments": {"command": "circle", "params": {"center": "x"}}}));
    assert_eq!(r["result"]["isError"], true);
    let r = call(&mut s, 2, "tools/call", json!({"name": "screenshot", "arguments": {}}));
    assert_eq!(r["result"]["isError"], true);
    assert!(s.handle_line("{not json").unwrap().contains("-32700"));
}

/// Unknown tools and arguments that don't fit a tool's schema are invalid params (-32602), never
/// guessed at; an empty batch is one Invalid Request.
#[test]
fn bad_tool_calls_are_invalid_params() {
    let mut s = Server::new(Box::new(Headless::default()));
    tool(&mut s, "command_line", json!({"text": "circle 0,0 1"}));
    let bad = [
        json!({"name": "nope", "arguments": {}}),
        json!({"name": "command_line", "arguments": {}}),
        json!({"name": "command_line", "arguments": {"text": 5}}),
        json!({"name": "command_line", "arguments": "circle 1,1 1"}),
        json!({"name": "script", "arguments": {}}),
        json!({"name": "query_entities", "arguments": {"window": [-5, -5, 5, 5]}}),
        json!({"name": "query_entities", "arguments": {"window": [[-5, -5]]}}),
        json!({"name": "query_entities", "arguments": {"window": {"min": [-5, -5], "max": [5, 5]}}}),
        json!({"name": "query_entities", "arguments": {"type": 7}}),
        json!({"name": "query_entities", "arguments": {"layer": ["0"]}}),
        json!({"name": "query_entities", "arguments": {"offset": -1}}),
        json!({"name": "query_entities", "arguments": {"selected": "true"}}),
        json!({"name": "query_entities", "arguments": {"selected": 1}}),
    ];
    for (i, params) in bad.into_iter().enumerate() {
        let r = call(&mut s, i as u64, "tools/call", params.clone());
        assert_eq!(r["error"]["code"], -32602, "{params} -> {r}");
    }
    // Nothing was typed: the circle is still the only object and no command is running.
    let d = tool(&mut s, "inspect_drawing", json!({"entities": false}));
    assert_eq!(d["entityCount"], 1, "{d}");
    assert_eq!(s.handle_line("[]").map(|r| serde_json::from_str::<Value>(&r).unwrap()["error"]["code"].clone()), Some(json!(-32600)));
}

/// `open` reads both DXF and DWG, so `tools/list` must advertise both (#1875).
#[test]
fn open_tool_description_names_dxf_and_dwg() {
    let mut s = Server::new(Box::new(Headless::default()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let open = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "open").unwrap();
    let desc = open["description"].as_str().unwrap();
    assert!(desc.contains("DXF"), "`open` must still name DXF: {desc}");
    assert!(desc.contains("DWG"), "`open` reads DWG too: {desc}");
}

/// Guards that `query_entities` advertises its `selected` filter and honours it (active-space
/// selection only, both ways) without changing the drawing.
#[test]
fn query_entities_selection_filter_is_discoverable_and_forwarded() {
    let mut s = Server::new(Box::new(Headless::default()));
    let tools = call(&mut s, 1, "tools/list", json!({}));
    let query = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "query_entities").unwrap();
    assert_eq!(query["inputSchema"]["properties"]["selected"]["type"], "boolean");
    tool(&mut s, "execute", json!({"command": "circle", "params": {"center": [0, 0], "radius": 2}}));
    tool(&mut s, "execute", json!({"command": "circle", "params": {"center": [10, 0], "radius": 2}}));
    assert_eq!(tool(&mut s, "query_entities", json!({"selected": true}))["count"], 0);
    tool(&mut s, "execute", json!({"command": "select", "params": {"window": [[-3, -3], [3, 3]]}}));
    let before = tool(&mut s, "inspect_drawing", json!({}));
    let selected = tool(&mut s, "query_entities", json!({"selected": true, "type": "Circle"}));
    assert_eq!(selected["count"], 1);
    assert_eq!(selected["entities"][0]["geometry"]["center"]["x"], 0.0);
    assert_eq!(tool(&mut s, "query_entities", json!({"selected": false}))["count"], 2);
    assert_eq!(tool(&mut s, "inspect_drawing", json!({})), before);
}

/// `count` is every match while `returned` is the page; a window matches geometry, not bounding boxes.
#[test]
fn query_entities_pages_and_windows_by_geometry() {
    let mut s = Server::new(Box::new(Headless::default()));
    for c in ["circle 0,0 1", "circle 100,100 1", "circle 200,0 1"] {
        tool(&mut s, "command_line", json!({"text": c}));
    }
    // An arc whose bounding box covers the window but whose curve stays outside it.
    tool(&mut s, "execute", json!({"command": "arc", "params": {"p1": [40, 0], "p2": [50, 10], "p3": [60, 0]}}));
    let r = tool(&mut s, "query_entities", json!({"type": "Circle", "limit": 1, "offset": 1}));
    assert_eq!((r["count"].clone(), r["returned"].clone()), (json!(3), json!(1)), "{r}");
    assert_eq!(r["entities"][0]["geometry"]["center"]["x"], 100.0, "{r}");
    let r = tool(&mut s, "query_entities", json!({"window": [[45, 1], [55, 5]]}));
    assert_eq!(r["count"], 0, "the arc passes above this window: {r}");
    let r = tool(&mut s, "query_entities", json!({"window": [[-5, -5], [55, 5]]}));
    assert_eq!(r["count"], 2, "the circle at 0,0 and the arc crossing the window: {r}");
    let r = tool(&mut s, "query_entities", json!({"window": [[-5, -5], [55, 5]], "crossing": false}));
    assert_eq!(r["count"], 1, "only the circle is entirely inside: {r}");
}

/// Guards that the `list_commands` filter matches ids, labels and aliases but not parameter
/// docs, so searching `circle` does not return QSELECT merely for mentioning circles.
#[test]
fn list_commands_filter_matches_names_not_docs() {
    let mut s = Server::new(Box::new(Headless::default()));
    let r = tool(&mut s, "list_commands", json!({"filter": "circle"}));
    let ids: Vec<&str> = r.as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(ids.contains(&"circle"), "{ids:?}");
    assert!(!ids.contains(&"qselect"), "QSELECT only mentions circles in its docs: {ids:?}");
    let r = tool(&mut s, "list_commands", json!({"filter": "pl"}));
    assert!(r.as_array().unwrap().iter().any(|c| c["id"] == "pline"), "aliases and ids match");
}

/// Guards that `find_text` returns TEXT and MTEXT with their placement, strips MTEXT formatting,
/// and filters by text, layer and window; a windowed render keeps that area's aspect ratio.
#[test]
fn find_text_reads_texts_and_render_shows_one_area() {
    let mut s = Server::new(Box::new(Headless::default()));
    call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    tool(&mut s, "execute", json!({"command": "layer.new", "params": {"name": "Texto"}}));
    tool(&mut s, "execute", json!({"command": "text", "params": {"at": [1, 1], "height": 0.15, "text": "RECAMARA 1"}}));
    tool(&mut s, "execute", json!({"command": "text", "params": {"at": [50, 1], "height": 0.15, "text": "COCINA"}}));
    tool(
        &mut s,
        "execute",
        json!({"command": "mtext", "params": {"at": [1, 5], "height": 0.2, "width": 5, "text": "{\\fArial|b1;PLANTA} BAJA\\PNIVEL 0.00"}}),
    );
    tool(&mut s, "execute", json!({"command": "rectang", "params": {"p1": [0, 0], "p2": [10, 5]}}));

    let all = tool(&mut s, "find_text", json!({}));
    assert_eq!(all["count"], 3);
    let r = tool(&mut s, "find_text", json!({"contains": "recamara"}));
    assert_eq!(r["count"], 1);
    assert_eq!(r["texts"][0]["text"], "RECAMARA 1");
    assert_eq!(r["texts"][0]["at"], json!([1.0, 1.0]));
    assert_eq!(r["texts"][0]["height"], 0.15);
    // Multiline text comes back readable, without its formatting codes.
    let m = tool(&mut s, "find_text", json!({"contains": "planta"}));
    let t = m["texts"][0]["text"].as_str().unwrap();
    assert!(t.contains("PLANTA BAJA") && t.contains("NIVEL 0.00") && !t.contains('\\') && !t.contains('{'), "{t}");
    // By area: only what is inside the window.
    let w = tool(&mut s, "find_text", json!({"window": [[-1, -1], [20, 20]]}));
    assert_eq!(w["count"], 2);
    assert_eq!(tool(&mut s, "find_text", json!({"layer": "nope"}))["count"], 0);

    // A render of one area takes the area's shape.
    let r = call(&mut s, 9, "tools/call", json!({"name": "render", "arguments": {"window": [[0, 0], [10, 5]]}}));
    let c = &r["result"]["content"][0];
    assert_eq!(c["type"], "image", "{r}");
    let png = cadcraft_engine::cmd::file::base64_decode(c["data"].as_str().unwrap()).unwrap();
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    assert_eq!((w, h), (1600, 800));
}

/// Guards a lazy Remote bridge: it serves tools without the app, reports how to start it
/// (`--control`), and reaches the app once that app starts listening on the port.
#[test]
fn a_lazy_bridge_starts_without_the_app_and_says_how_to_open_it() {
    // Nothing listens on a port that was just freed.
    let addr = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    };
    let mut s = Server::new(Box::new(crate::Remote::lazy(&addr, false)));
    let init = call(&mut s, 1, "initialize", json!({"protocolVersion": "2025-06-18"}));
    assert_eq!(init["result"]["serverInfo"]["name"], "cadcraft");
    let tools = call(&mut s, 2, "tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "screenshot"));
    let r = call(&mut s, 3, "tools/call", json!({"name": "command_line", "arguments": {"text": "line 0,0 1,1"}}));
    assert_eq!(r["result"]["isError"], json!(true));
    let msg = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(msg.contains("not answering") && msg.contains("--control"), "{msg}");

    // Once the app listens, the next call reaches it.
    let listener = std::net::TcpListener::bind(&addr).unwrap();
    let app = std::thread::spawn(move || {
        use std::io::{BufRead, Write};
        let (sock, _) = listener.accept().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(sock.try_clone().unwrap()).read_line(&mut line).unwrap();
        let id = serde_json::from_str::<Value>(&line).unwrap()["id"].clone();
        writeln!(&sock, "{}", json!({"id": id, "ok": true, "result": {"prompt": "Command:"}})).unwrap();
    });
    let r = call(&mut s, 4, "tools/call", json!({"name": "command_line", "arguments": {"text": ""}}));
    assert_ne!(r["result"]["isError"], json!(true), "{r}");
    app.join().unwrap();
}

/// The pixels of a `render` tool result.
fn render_pixels(s: &mut Server, args: Value) -> tiny_skia::Pixmap {
    let img = tool(s, "render", args);
    assert_eq!(img["type"], "image", "{img}");
    let png = cadcraft_engine::cmd::file::base64_decode(img["data"].as_str().unwrap()).unwrap();
    tiny_skia::Pixmap::decode_png(&png).unwrap()
}

/// The RGB of pixel (x, y).
fn rgb(pm: &tiny_skia::Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let c = pm.pixel(x, y).unwrap();
    (c.red(), c.green(), c.blue())
}

/// The pixel of the middle column that differs most from the corner: what the line along the
/// middle of the image is drawn with.
fn ink(pm: &tiny_skia::Pixmap) -> (u8, u8, u8) {
    let bg = rgb(pm, 0, 0);
    let far = |c: &(u8, u8, u8)| bg.0.abs_diff(c.0) as u32 + bg.1.abs_diff(c.1) as u32 + bg.2.abs_diff(c.2) as u32;
    (0..pm.height()).map(|y| rgb(pm, pm.width() / 2, y)).max_by_key(far).unwrap()
}

/// A drawing with one line along y = 0: colour 7 (layer 0's colour), or red on a red layer.
fn line_drawing(red: bool) -> Server {
    let mut s = Server::new(Box::new(Headless::default()));
    if red {
        tool(&mut s, "execute", json!({"command": "layer.new", "params": {"name": "Red", "color": "red", "current": true}}));
    }
    tool(&mut s, "execute", json!({"command": "line", "params": {"points": [[0, 0], [10, 0]]}}));
    s
}

/// Around the line, which runs along the middle of pixel row 20 of a 120 x 40 image (not on a
/// pixel edge, so it isn't blended across two rows).
const WINDOW: [[f64; 2]; 2] = [[-1.0, -1.95], [11.0, 2.05]];

/// Guards #803: `render` draws on the background asked for (`white`, `paper`, `#rrggbb`) and a
/// colour-7 line follows it, as on a plot: dark on white, light on the dark canvas, which stays
/// the default.
#[test]
fn render_draws_on_the_background_asked_for() {
    let mut s = line_drawing(false);
    let dark = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40}));
    assert_eq!(rgb(&dark, 0, 0), (33, 40, 48), "the default stays the dark canvas");
    let (r, g, b) = ink(&dark);
    assert!(r > 200 && g > 200 && b > 200, "colour 7 is light on the dark canvas: {:?}", (r, g, b));
    for bg in ["white", "paper", "WHITE"] {
        let pm = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40, "background": bg}));
        let (w, h) = (pm.width(), pm.height());
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert_eq!(rgb(&pm, x, y), (255, 255, 255), "{bg}: corner ({x}, {y})");
        }
        let (r, g, b) = ink(&pm);
        assert!(r < 128 && g < 128 && b < 128, "{bg}: colour 7 is drawn dark on white: {:?}", (r, g, b));
    }
    let pm = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40, "background": "#336699"}));
    assert_eq!(rgb(&pm, 0, 0), (0x33, 0x66, 0x99));
    let pm = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40, "background": "dark"}));
    assert_eq!(rgb(&pm, 0, 0), (33, 40, 48));
}

/// Guards #803: `plot_style` draws with a built-in plot style table: `monochrome.ctb` turns a red
/// line black, which without one stays red.
#[test]
fn render_draws_with_the_plot_style_asked_for() {
    let mut s = line_drawing(true);
    let plain = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40, "background": "white"}));
    let (r, g, b) = ink(&plain);
    assert!(r > 200 && g < 100 && b < 100, "red without a plot style: {:?}", (r, g, b));
    for table in ["monochrome.ctb", "Monochrome", "monochrome.CTB"] {
        let pm = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40, "background": "white", "plot_style": table}));
        let (r, g, b) = ink(&pm);
        assert!(r < 100 && r == g && g == b, "{table}: red plots black: {:?}", (r, g, b));
    }
    let pm = render_pixels(&mut s, json!({"window": WINDOW, "width": 120, "height": 40, "background": "white", "plot_style": "grayscale.ctb"}));
    let (r, g, b) = ink(&pm);
    assert!(r == g && g == b, "grayscale.ctb draws red grey: {:?}", (r, g, b));
}

/// Guards #803: an unknown `background` or `plot_style` is refused with what is accepted, not
/// drawn some other way; a value of the wrong type is invalid params.
#[test]
fn render_refuses_unknown_backgrounds_and_plot_styles() {
    let mut s = line_drawing(false);
    for (args, wants) in [
        (json!({"background": "purple"}), "#rrggbb"),
        (json!({"background": "#12345"}), "#rrggbb"),
        (json!({"background": "336699"}), "#rrggbb"),
        (json!({"background": ""}), "#rrggbb"),
        (json!({"plot_style": "nope.ctb"}), "monochrome.ctb"),
        (json!({"plot_style": "default.ctx"}), "monochrome.ctb"),
    ] {
        let r = call(&mut s, 1, "tools/call", json!({"name": "render", "arguments": args.clone()}));
        assert_eq!(r["result"]["isError"], json!(true), "{args} must be refused, not drawn");
        let msg = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(msg.contains(wants), "{args}: {msg}");
    }
    for args in [json!({"background": 7}), json!({"plot_style": ["monochrome.ctb"]})] {
        let r = call(&mut s, 2, "tools/call", json!({"name": "render", "arguments": args.clone()}));
        assert_eq!(r["error"]["code"], -32602, "{args} must be invalid params");
    }
    // The headless control method refuses them too.
    let mut h = Headless::default();
    assert!(crate::Backend::call(&mut h, "ui.render", json!({"background": "purple"})).is_err());
    assert!(crate::Backend::call(&mut h, "ui.render", json!({"plot_style": 3})).is_err());
}
