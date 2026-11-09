//! One-line commands for a shared session: text into an operation with explicit addresses.
//!
//! A shared session has no interactive prompt: every participant has a different window, so a
//! value a prompt would take from the window (a pick, the selection) would resolve differently on
//! the server and on each client. The command line therefore takes a complete line whose
//! parameters name everything the command needs — points by coordinates, objects by handle — and
//! sends it as an ordinary [`Op`] for the server to run (ADR-0001, ADR-0004).
//!
//! Only the plain drawing commands of the pilot are here. A line the parser refuses never becomes
//! an operation, so the drawing cannot change; the message says what to type instead.
//!
//! An object goes on the layer the author is drawing on: the operation names it in its context,
//! so a participant's line never lands on the server drawing's own current layer (ADR-0019).
//!
//! ```text
//! line 0,0 100,0 100,50
//! circle 50,25 12.5
//! polyline 0,0 100,0 100,50
//! ellipse 50,25 20,10 30
//! rect 0,0 100,50
//! text 10,20 2.5 0 "Shared note"
//! dimlinear 1A2,1A3 offset 5
//! dimlinear 1A2,1A3 point 0,20
//! mleader 1A2 20,10 "Shared note"
//! spline 0,0 10,5 20,0 30,5
//! spline 0,0 10,5 20,0 closed
//! block DOOR base 0,0 1A2,1A3
//! insert DOOR at 10,5 scale 2 angle 90
//! move 1A2,1A3 delta 10,0
//! move 1A2 from 0,0 to 10,0
//! copy 1A2,1A3 delta 10,0
//! rotate 1A2 base 0,0 45
//! mirror 1A2,1A3 0,0 0,100
//! scale 1A2,1A3 base 0,0 2
//! offset 1A2 distance 5 side 0,10
//! trim 1A2 edge 1A3 at 2,0
//! array 1A2 rows 2 cols 3 step 20,10
//! hatch 1A2,1A3 pattern ANSI31 scale 2 angle 45
//! wipeout 0,0 10,0 10,10
//! wipeout 1A2
//! imageattach map.png data iVBORw0KGgo… at 10,5 width 100 rotation 15 name "Site map"
//! erase 1A2,1A3
//! ```
//!
//! `imageattach` carries the raster bytes in `data`: a shared session never reads a file by the
//! client's path, so `path` (the first word) is only the label saved in the drawing (ADR-0021).

use cadcraft_doc::geom::{Arc, Line, Segment, Vec2};
use cadcraft_doc::{Drawing, Entity, Handle, Prim, Space, entity_bounds};
use serde_json::{Value, json};

use crate::{Op, OpContext, OpKind};

/// The command names a one-line shared-session command accepts (aliases included below).
pub const COMMANDS: [&str; 23] = [
    "line",
    "circle",
    "polyline",
    "ellipse",
    "rect",
    "text",
    "dimlinear",
    "mleader",
    "spline",
    "block",
    "insert",
    "move",
    "copy",
    "rotate",
    "mirror",
    "scale",
    "offset",
    "trim",
    "array",
    "hatch",
    "wipeout",
    "imageattach",
    "erase",
];

/// One line of text, parsed into a command and its explicit parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// The engine command id (`"line"`, `"circle"`, `"pline"`, `"move"`, `"erase"`).
    pub id: &'static str,
    /// The command's JSON parameters, with every address spelled out.
    pub params: Value,
}

/// Why a line cannot become a shared-session operation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CmdLineError {
    /// Nothing was typed.
    #[error(
        "type a command: line, circle, polyline, ellipse, rect, text, dimlinear, mleader, spline, block, insert, move, copy, rotate, mirror, scale, offset, trim, array, hatch, wipeout, imageattach or erase"
    )]
    Empty,
    /// The command is not one of [`COMMANDS`].
    #[error(
        "`{0}` is not available one line at a time in a shared session; use line, circle, polyline, ellipse, rect, text, dimlinear, mleader, spline, block, insert, move, copy, rotate, mirror, scale, offset, trim, array, hatch, wipeout, imageattach or erase"
    )]
    UnknownCommand(String),
    /// The command is known but its arguments do not parse.
    #[error("{command}: {detail}")]
    BadArguments { command: &'static str, detail: String },
    /// A handle does not name an object in the space the command acts on.
    #[error("{command}: there is no object {handle} in {space}; address objects by their handles (1A2, 1A3, …)")]
    UnknownHandle { command: &'static str, handle: String, space: String },
    /// The drawing's current layer does not name a layer in the drawing.
    #[error("the current layer `{layer}` is not in this drawing; choose one with layer.current")]
    UnknownLayer { layer: String },
}

/// Parse `text` into an operation to queue for a shared session.
///
/// `drawing` and `space` are the author's current view: a command that addresses existing objects
/// must name objects in the space the operation names, so a typo is refused here instead of moving
/// or erasing nothing on the server. The drawing is only read.
pub fn parse(text: &str, drawing: &Drawing, space: &Space) -> Result<Command, CmdLineError> {
    let mut words = tokenize(text).into_iter();
    let Some(head) = words.next() else {
        return Err(CmdLineError::Empty);
    };
    let head = head.to_ascii_lowercase();
    match head.as_str() {
        "line" | "l" => line(words.collect()),
        "circle" | "c" => circle(words.collect()),
        "polyline" | "pline" | "pl" => polyline(words.collect()),
        "ellipse" | "el" => ellipse(words.collect()),
        "rect" | "rectangle" | "rec" => rect(words.collect()),
        "text" | "dt" | "dtext" => text_command(words.collect()),
        "dimlinear" | "dli" | "dimlin" => dimlinear(drawing, space, words.collect()),
        "mleader" | "mld" => mleader(drawing, space, words.collect()),
        "spline" | "spl" => spline(words.collect()),
        "block" | "b" => block_command(drawing, space, words.collect()),
        "insert" | "i" => insert_command(drawing, space, words.collect()),
        "move" | "m" => move_(drawing, space, words.collect()),
        "copy" | "co" | "cp" => copy(drawing, space, words.collect()),
        "rotate" | "ro" => rotate(drawing, space, words.collect()),
        "mirror" | "mi" => mirror(drawing, space, words.collect()),
        "scale" | "sc" => scale(drawing, space, words.collect()),
        "offset" | "o" => offset(drawing, space, words.collect()),
        "trim" | "tr" => trim(drawing, space, words.collect()),
        "array" | "ar" | "arr" => array(drawing, space, words.collect()),
        "hatch" | "h" | "bh" => hatch_command(drawing, space, words.collect()),
        "wipeout" | "wi" => wipeout(drawing, space, words.collect()),
        "imageattach" | "iat" => imageattach(words.collect()),
        "erase" | "e" | "delete" => erase(drawing, space, words.collect()),
        _ => Err(CmdLineError::UnknownCommand(brief(&head))),
    }
}

/// Build the [`Op`] for `text`; `op_id` and `base_version` are the client's, as for any operation.
///
/// The context carries the space, the author's layer and no selection: the parameters address
/// everything they act on, so the server never reads a selection nobody can see. The layer is
/// named by the commands that create objects — the server has no window and would otherwise put
/// them on the server drawing's own current layer (ADR-0019). A drawing whose current layer is
/// missing is refused here, before an operation exists.
pub fn op_for(text: &str, drawing: &Drawing, space: &Space, op_id: &str, base_version: u64) -> Result<Op, CmdLineError> {
    let command = parse(text, drawing, space)?;
    let clay = if creates_objects(command.id) { Some(current_layer(drawing)?) } else { None };
    Ok(Op {
        op_id: op_id.to_string(),
        actor: String::new(),
        base_version,
        kind: OpKind::Command,
        command: command.id.to_string(),
        params: command.params,
        context: OpContext { space: space.clone(), selection: None, clay },
    })
}

/// Whether `id` creates objects, and so needs the author's layer to put them on.
///
/// A copy or a mirror inherits the source objects' layers, and a scale changes objects in place,
/// so only these commands draw new geometry on a layer of their own (ADR-0019).
fn creates_objects(id: &str) -> bool {
    matches!(
        id,
        "line"
            | "circle"
            | "pline"
            | "ellipse"
            | "rectang"
            | "text"
            | "dimlinear"
            | "mleader"
            | "spline"
            | "block"
            | "insert"
            | "offset"
            | "array"
            | "hatch"
            | "wipeout"
            | "imageattach"
    )
}

/// The layer an object drawn now belongs to: the drawing's `CLAYER`, in the drawing's spelling.
fn current_layer(drawing: &Drawing) -> Result<String, CmdLineError> {
    let layer = drawing.header.str("CLAYER", "0");
    match drawing.layer(&layer) {
        Some(found) => Ok(found.name.clone()),
        None => Err(CmdLineError::UnknownLayer { layer }),
    }
}

fn line(words: Vec<&str>) -> Result<Command, CmdLineError> {
    if words.len() < 2 {
        return Err(bad("line", "needs at least two points: line x,y x,y [x,y …]"));
    }
    let points = words.iter().map(|word| point(word, "line")).collect::<Result<Vec<[f64; 2]>, CmdLineError>>()?;
    Ok(Command { id: "line", params: json!({ "points": points }) })
}

fn circle(words: Vec<&str>) -> Result<Command, CmdLineError> {
    let [centre, radius] = words.as_slice() else {
        return Err(bad("circle", "takes a centre and a radius: circle x,y r"));
    };
    let [x, y] = point(centre, "circle")?;
    let radius = number(radius).filter(|r| *r > 0.0).ok_or_else(|| bad("circle", "the radius must be a positive number"))?;
    Ok(Command { id: "circle", params: json!({ "center": [x, y], "radius": radius }) })
}

fn polyline(words: Vec<&str>) -> Result<Command, CmdLineError> {
    if words.len() < 2 {
        return Err(bad("polyline", "needs at least two points: polyline x,y x,y [x,y …]"));
    }
    let points = words.iter().map(|word| point(word, "polyline")).collect::<Result<Vec<[f64; 2]>, CmdLineError>>()?;
    // The engine command is `pline`; the line spells the familiar name.
    Ok(Command { id: "pline", params: json!({ "vertices": points }) })
}

/// `ellipse cx,cy rx[,ry] [angle]` — the major radius, the minor radius and the major axis's
/// direction in degrees, spelled as the engine's `{center, major, ratio}`.
fn ellipse(words: Vec<&str>) -> Result<Command, CmdLineError> {
    let (centre, sizes, angle) = match words.as_slice() {
        [centre, sizes] => (*centre, *sizes, None),
        [centre, sizes, angle] => (*centre, *sizes, Some(*angle)),
        _ => return Err(bad("ellipse", "takes a centre, its radii and an optional angle: ellipse x,y rx[,ry] [angle]")),
    };
    let [cx, cy] = point(centre, "ellipse")?;
    let [rx, ry] = radii(sizes)?;
    let angle = match angle {
        None => 0.0,
        Some(text) => number(text).ok_or_else(|| bad("ellipse", "the angle must be a finite number of degrees"))?,
    };
    let radians = angle.to_radians();
    if !radians.is_finite() {
        return Err(bad("ellipse", "the angle must be a finite number of degrees"));
    }
    let (sin, cos) = radians.sin_cos();
    // The engine stores the major axis as a vector from the centre and the minor/major ratio.
    Ok(Command { id: "ellipse", params: json!({ "center": [cx, cy], "major": [rx * cos, rx * sin], "ratio": ry / rx }) })
}

/// `rx` or `rx,ry`: two positive radii with the major one first, or one radius for a circle.
fn radii(word: &str) -> Result<[f64; 2], CmdLineError> {
    let mut parts = word.split(',');
    let Some(first) = parts.next() else {
        return Err(bad("ellipse", "give the radii as rx[,ry], both positive"));
    };
    let rx = number(first).filter(|r| *r > 0.0).ok_or_else(|| bad("ellipse", format!("`{}` is not a positive radius", brief(first))))?;
    let ry = match parts.next() {
        None => rx,
        Some(text) => number(text).filter(|r| *r > 0.0).ok_or_else(|| bad("ellipse", format!("`{}` is not a positive radius", brief(text))))?,
    };
    if parts.next().is_some() {
        return Err(bad("ellipse", "give at most two radii: rx[,ry]"));
    }
    if ry > rx {
        return Err(bad("ellipse", "the first radius is the major axis: rx must be at least ry"));
    }
    Ok([rx, ry])
}

fn rect(words: Vec<&str>) -> Result<Command, CmdLineError> {
    let [first, second] = words.as_slice() else {
        return Err(bad("rect", "takes two opposite corners: rect x,y x,y"));
    };
    let [x1, y1] = point(first, "rect")?;
    let [x2, y2] = point(second, "rect")?;
    if (x2 - x1).abs() < 1e-12 || (y2 - y1).abs() < 1e-12 {
        return Err(bad("rect", "the corners form a rectangle with no area; give two different corners"));
    }
    // The engine command is `rectang`; the line spells the familiar name.
    Ok(Command { id: "rectang", params: json!({ "p1": [x1, y1], "p2": [x2, y2] }) })
}

/// `text x,y height angle "content"` — one line of text.
///
/// The content is quoted because it may hold spaces; `\"` and `\\` stand for a quote and a
/// backslash. Height is in drawing units, the angle in degrees, as the engine's `rotation` is.
/// Empty text, a non-positive height and an unterminated (or unquoted) string are refused before an
/// operation exists, so a participant's line never draws nothing.
fn text_command(words: Vec<&str>) -> Result<Command, CmdLineError> {
    let [at, height, angle, content] = words.as_slice() else {
        return Err(bad("text", "give a point, a height, an angle and the text in double quotes: text x,y height angle \"text\""));
    };
    let [x, y] = point(at, "text")?;
    let height = number(height).filter(|h| *h > 0.0).ok_or_else(|| bad("text", "the height must be a positive number"))?;
    let angle = number(angle).ok_or_else(|| bad("text", "the angle must be a finite number of degrees"))?;
    let value = quoted(content).ok_or_else(|| bad("text", "the text must be in double quotes: text x,y height angle \"text\""))?;
    if value.trim().is_empty() {
        return Err(bad("text", "the text is empty"));
    }
    if value.chars().count() > MAX_TEXT_CHARS {
        return Err(bad("text", format!("the text is longer than {MAX_TEXT_CHARS} characters")));
    }
    Ok(Command { id: "text", params: json!({ "at": [x, y], "height": height, "rotation": angle, "text": value }) })
}

/// The longest text one line may carry: a shared session's operation has a size limit, and a line
/// of text is never a drawing's bulk.
const MAX_TEXT_CHARS: usize = 10_000;

/// The drawing's coordinate limit, as the DXF reader applies it: a point at or beyond it cannot be
/// written and read back, so a line that names one is refused before an operation exists.
const MAX_COORDINATE: f64 = 1e12;

/// `dimlinear H1,H2 offset d` or `dimlinear H1,H2 point x,y` — a linear dimension between two
/// objects, with the dimension line's placement spelled out.
///
/// A shared session has no pick: the measured points are the two objects' characteristic points
/// (the same points a dimension attaches to, `crates/engine/src/assoc.rs::candidates`), and the
/// nearest pair of them is measured. `offset d` places the dimension line parallel to the measured
/// one, signed towards the left of `H1 → H2`; `point x,y` places it at that point. Two identical
/// handles, a handle that names nothing here, an object with no measurable point and a zero length
/// are refused before an operation exists.
fn dimlinear(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("dimlinear", "address the two objects to dimension: dimlinear 1A2,1A3 offset d | point x,y"));
    };
    // `dimlinear offset 5` has no address: the keyword is not a handle.
    if addresses.eq_ignore_ascii_case("offset") || addresses.eq_ignore_ascii_case("point") {
        return Err(bad("dimlinear", "address the two objects to dimension: dimlinear 1A2,1A3 offset d | point x,y"));
    }
    let handles = handles("dimlinear", addresses)?;
    let [first, second] = handles.as_slice() else {
        return Err(bad("dimlinear", "takes two objects: dimlinear 1A2,1A3 offset d | point x,y"));
    };
    if first == second {
        return Err(bad("dimlinear", "the two handles name the same object; give two different objects"));
    }
    check_in_space(drawing, space, "dimlinear", &handles)?;
    let (p1, p2) = measured(drawing, space, *first, *second)?;
    if zero_length(p1, p2) {
        return Err(bad("dimlinear", "the two points coincide; the dimension has no length"));
    }
    let at = match rest {
        [keyword, offset] if keyword.eq_ignore_ascii_case("offset") => {
            let offset = number(offset)
                .filter(|d| d.abs() < MAX_COORDINATE)
                .ok_or_else(|| bad("dimlinear", "the offset must be a finite number within the drawing's coordinate limit"))?;
            // The dimension line runs parallel to the measured one, `offset` to its left.
            let normal = (p2 - p1).perp().normalized();
            p1.mid(p2) + normal * offset
        }
        [keyword, location] if keyword.eq_ignore_ascii_case("point") => drawing_point(location, "dimlinear")?,
        _ => {
            return Err(bad("dimlinear", "place the dimension line: dimlinear 1A2,1A3 offset d | point x,y"));
        }
    };
    Ok(Command { id: "dimlinear", params: json!({ "p1": [p1.x, p1.y], "p2": [p2.x, p2.y], "at": [at.x, at.y] }) })
}

/// The nearest pair of the two objects' characteristic points, the points a dimension attaches to.
fn measured(drawing: &Drawing, space: &Space, first: Handle, second: Handle) -> Result<(Vec2, Vec2), CmdLineError> {
    let store = drawing.space(space).ok_or_else(|| bad("dimlinear", "the active layout is not in this drawing"))?;
    let a = store.get(first).map(|entity| definition_points(entity)).unwrap_or_default();
    let b = store.get(second).map(|entity| definition_points(entity)).unwrap_or_default();
    let empty = || bad("dimlinear", "both objects need a point to measure from; dimension a line, arc, circle, polyline or point");
    let mut best: Option<(f64, Vec2, Vec2)> = None;
    for p in &a {
        for q in &b {
            let distance = p.dist(*q);
            if best.as_ref().is_none_or(|found| distance < found.0) {
                best = Some((distance, *p, *q));
            }
        }
    }
    let (_, p1, p2) = best.ok_or_else(empty)?;
    if !p1.is_finite() || !p2.is_finite() {
        return Err(empty());
    }
    Ok((p1, p2))
}

/// The points an object can be dimensioned from, as `assoc::candidates` names them: endpoints and
/// the midpoint of a line, an arc's ends, centre and midpoint, a circle's centre, a polyline's
/// vertices and a point. Anything else has no definition point to attach a dimension to.
fn definition_points(entity: &Entity) -> Vec<Vec2> {
    use cadcraft_doc::EntityKind;
    match &entity.kind {
        EntityKind::Point(p) => vec![p.p.xy()],
        EntityKind::Line(l) => vec![l.a.xy(), l.b.xy(), l.a.xy().mid(l.b.xy())],
        EntityKind::Arc(a) => {
            let arc = cadcraft_doc::geom::Arc::new(a.center.xy(), a.radius, a.start, a.end);
            vec![arc.start_point(), arc.end_point(), a.center.xy(), arc.mid_point()]
        }
        EntityKind::Circle(c) => vec![c.center.xy()],
        EntityKind::LwPolyline(p) => p.vertices.iter().take(MAX_MEASURE_POINTS).map(|vertex| vertex.p).collect(),
        _ => Vec::new(),
    }
}

/// The most vertices of one polyline the measured pair searches: a hostile drawing cannot make the
/// search quadratic without bound.
const MAX_MEASURE_POINTS: usize = 1000;

/// Whether two points are close enough to call the dimension's length zero.
fn zero_length(a: Vec2, b: Vec2) -> bool {
    let scale = 1.0 + a.x.abs().max(a.y.abs()).max(b.x.abs()).max(b.y.abs());
    a.dist(b) <= 1e-9 * scale
}

/// `mleader H[,H…] x,y "text"` — a multileader that points at the named objects and lands on the
/// explicit point, with its text in double quotes.
///
/// A shared session has no pick for the arrowhead: each handle contributes the object's
/// characteristic point nearest the landing (the same points a dimension attaches to, see
/// [`definition_points`]), in the order typed, and the last point of the line is the landing.
/// One handle is the familiar arrow-to-landing leader; more than one makes a multiline leader
/// through those objects. Empty text, a handle that names nothing here, an object with no point to
/// point at, a landing at the drawing's coordinate limit and a landing on the last arrow point are
/// refused before an operation exists.
fn mleader(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let [addresses, landing, content] = words.as_slice() else {
        return Err(bad("mleader", "give the objects to point at, the landing point and the text in double quotes: mleader 1A2[,1A3] x,y \"text\""));
    };
    let handles = handles("mleader", addresses)?;
    check_in_space(drawing, space, "mleader", &handles)?;
    let landing = drawing_point(landing, "mleader")?;
    let value = quoted(content).ok_or_else(|| bad("mleader", "the text must be in double quotes: mleader 1A2[,1A3] x,y \"text\""))?;
    if value.trim().is_empty() {
        return Err(bad("mleader", "the text is empty"));
    }
    if value.chars().count() > MAX_TEXT_CHARS {
        return Err(bad("mleader", format!("the text is longer than {MAX_TEXT_CHARS} characters")));
    }
    let mut points = Vec::with_capacity(handles.len() + 1);
    let mut last = landing;
    for handle in &handles {
        let arrow = arrow_point(drawing, space, *handle, landing, "mleader")?;
        last = arrow;
        points.push([arrow.x, arrow.y]);
    }
    if zero_length(last, landing) {
        return Err(bad("mleader", "the landing point coincides with the last object's point; there is no leader line"));
    }
    points.push([landing.x, landing.y]);
    // The engine's MLEADER takes the leader's points with the landing last and its text.
    Ok(Command { id: "mleader", params: json!({ "points": points, "text": value }) })
}

/// The point of `handle` a leader points at: the object's characteristic point nearest `landing`.
fn arrow_point(drawing: &Drawing, space: &Space, handle: Handle, landing: Vec2, command: &'static str) -> Result<Vec2, CmdLineError> {
    let store = drawing.space(space).ok_or_else(|| bad(command, "the active layout is not in this drawing"))?;
    let entity = store.get(handle).ok_or_else(|| CmdLineError::UnknownHandle { command, handle: handle.hex(), space: space_name(space) })?;
    let points = definition_points(entity);
    let nearest = points.into_iter().filter(|point| point.is_finite()).min_by(|a, b| a.dist(landing).total_cmp(&b.dist(landing)));
    nearest.ok_or_else(|| bad(command, format!("object {} has no point to point at; address a line, arc, circle, polyline or point", handle.hex())))
}

/// `block NAME base x,y H[,H…]` — a new block named `NAME` from the named objects, with `base` as
/// its base point; the objects become one reference to it (BLOCK's Convert default).
///
/// A name that is empty, longer than [`MAX_BLOCK_NAME`], anonymous (`*…`) or already in the drawing
/// is refused before an operation exists: a shared session makes new blocks, and redefining one
/// would change every reference at once (the drawing's blocks travel whole, ADR-0020).
fn block_command(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let [name, base_kw, base, addresses] = words.as_slice() else {
        return Err(bad("block", "give the name, the base point and the objects: block NAME base x,y 1A2[,1A3]"));
    };
    if !base_kw.eq_ignore_ascii_case("base") {
        return Err(bad("block", "give the name, the base point and the objects: block NAME base x,y 1A2[,1A3]"));
    }
    let name = block_name(name, "block")?;
    if drawing.block(&name).is_some() {
        return Err(bad("block", format!("a block named `{}` is already in this drawing; the shared command line makes new blocks", brief(&name))));
    }
    let base = drawing_point(base, "block")?;
    let handles = handles("block", addresses)?;
    check_in_space(drawing, space, "block", &handles)?;
    Ok(Command { id: "block", params: json!({ "name": name, "base": [base.x, base.y], "handles": hex(&handles), "keep": "convert" }) })
}

/// `insert NAME at x,y [scale s] [angle a]` — place a reference to the block named `NAME` at the
/// point, with a positive scale factor and an angle in degrees (both optional).
///
/// The block must already be in the drawing (a shared session's block definitions travel with the
/// change set, ADR-0020); a zero or negative scale and a name that names nothing are refused
/// before an operation exists. The reference's handle arrives with the change set, like any other
/// created object's.
fn insert_command(drawing: &Drawing, _space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((name, rest)) = words.split_first() else {
        return Err(bad("insert", "give a block and an insertion point: insert NAME at x,y [scale s] [angle a]"));
    };
    let name = block_name(name, "insert")?;
    let Some(block) = drawing.block(&name) else {
        return Err(bad("insert", format!("there is no block `{}` in this drawing; define it with block first", brief(&name))));
    };
    if block.name.starts_with('*') {
        return Err(bad("insert", "anonymous blocks cannot be inserted by name"));
    }
    let [at_kw, at, tail @ ..] = rest else {
        return Err(bad("insert", "give the insertion point: insert NAME at x,y [scale s] [angle a]"));
    };
    if !at_kw.eq_ignore_ascii_case("at") {
        return Err(bad("insert", "give the insertion point: insert NAME at x,y [scale s] [angle a]"));
    }
    let at = drawing_point(at, "insert")?;
    let mut scale = 1.0;
    let mut angle = 0.0;
    match tail {
        [] => {}
        [keyword, value] if keyword.eq_ignore_ascii_case("scale") => scale = insert_scale(value)?,
        [keyword, value] if keyword.eq_ignore_ascii_case("angle") => angle = insert_angle(value)?,
        [scale_kw, scale_value, angle_kw, angle_value] if scale_kw.eq_ignore_ascii_case("scale") && angle_kw.eq_ignore_ascii_case("angle") => {
            scale = insert_scale(scale_value)?;
            angle = insert_angle(angle_value)?;
        }
        _ => return Err(bad("insert", "give the insertion point and optionally a scale and an angle: insert NAME at x,y [scale s] [angle a]")),
    }
    Ok(Command { id: "insert", params: json!({ "name": block.name.clone(), "at": [at.x, at.y], "scale": scale, "rotation": angle }) })
}

/// A positive scale factor for INSERT.
fn insert_scale(word: &str) -> Result<f64, CmdLineError> {
    number(word).filter(|s| *s > 0.0).ok_or_else(|| bad("insert", "the scale must be a positive number"))
}

/// An angle in degrees for INSERT.
fn insert_angle(word: &str) -> Result<f64, CmdLineError> {
    number(word).ok_or_else(|| bad("insert", "the angle must be a finite number of degrees"))
}

/// The longest block name a line may carry (the engine's limit).
const MAX_BLOCK_NAME: usize = 255;

/// A block name as typed: a bare word, or one in double quotes when it holds spaces.
///
/// The name is refused when it is empty, too long, anonymous (`*…`) or holds a character the file
/// format cannot write, so the engine never has to refuse the operation after it exists.
fn block_name(word: &str, command: &'static str) -> Result<String, CmdLineError> {
    let name = match quoted(word) {
        Some(name) => name,
        None if word.starts_with('"') => return Err(bad(command, "the block name is missing its closing quote")),
        None => word.to_string(),
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(bad(command, "the block name is empty; give it a name"));
    }
    if name.len() > MAX_BLOCK_NAME {
        return Err(bad(command, format!("the block name is longer than {MAX_BLOCK_NAME} characters")));
    }
    if name.starts_with('*') {
        return Err(bad(command, "a block name cannot start with `*`; those names are anonymous blocks"));
    }
    if name.chars().any(|c| "<>/\\\":;?*|,=`".contains(c)) {
        return Err(bad(command, "the block name holds a character a name cannot: <>/\\\":;?*|,=`"));
    }
    Ok(name.to_string())
}

/// `spline x,y x,y x,y [closed]` — a curve through 3 to 64 fit points.
///
/// The optional `closed` keyword closes the curve. Fewer than three points, more than
/// [`MAX_SPLINE_POINTS`], a coordinate that is not a finite number or one at or beyond the
/// drawing's coordinate limit are refused before an operation exists, as is a set with fewer than
/// two different points (the engine fits a curve through at least two).
fn spline(words: Vec<&str>) -> Result<Command, CmdLineError> {
    let mut words = words;
    let closed = words.last().is_some_and(|word| word.eq_ignore_ascii_case("closed"));
    if closed {
        words.pop();
    }
    if words.len() < 3 {
        return Err(bad("spline", "needs at least three points: spline x,y x,y x,y [closed]"));
    }
    if words.len() > MAX_SPLINE_POINTS {
        return Err(bad("spline", format!("takes at most {MAX_SPLINE_POINTS} points; got {}", words.len())));
    }
    let points = words.iter().map(|word| drawing_point(word, "spline")).collect::<Result<Vec<Vec2>, CmdLineError>>()?;
    let mut distinct: Vec<Vec2> = Vec::new();
    for point in &points {
        if !distinct.iter().any(|seen| zero_length(*seen, *point)) {
            distinct.push(*point);
        }
    }
    if distinct.len() < 2 {
        return Err(bad("spline", "the points are all the same; give at least two different points"));
    }
    let fit = points.iter().map(|point| [point.x, point.y]).collect::<Vec<_>>();
    let params = if closed { json!({ "fit": fit, "closed": true }) } else { json!({ "fit": fit }) };
    Ok(Command { id: "spline", params })
}

/// The most fit points one `spline` line may carry: enough for a drafting curve, small enough to
/// keep an operation small.
const MAX_SPLINE_POINTS: usize = 64;

/// `x,y` within the drawing's coordinate limit.
fn drawing_point(word: &str, command: &'static str) -> Result<Vec2, CmdLineError> {
    let [x, y] = point(word, command)?;
    if x.abs() >= MAX_COORDINATE || y.abs() >= MAX_COORDINATE {
        return Err(bad(command, format!("`{}` is outside the drawing's coordinate limit", brief(word))));
    }
    Ok(Vec2::new(x, y))
}

fn erase(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("erase", "address the objects to erase: erase 1A2[,1A3]"));
    };
    if !rest.is_empty() {
        return Err(bad("erase", "takes only handles: erase 1A2[,1A3]"));
    }
    let handles = handles("erase", addresses)?;
    if handles.is_empty() {
        return Err(bad("erase", "address the objects to erase: erase 1A2[,1A3]"));
    }
    check_in_space(drawing, space, "erase", &handles)?;
    Ok(Command { id: "erase", params: json!({ "handles": hex(&handles) }) })
}

fn move_(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("move", "address the objects to move: move 1A2[,1A3] delta dx,dy"));
    };
    // `move delta 10,0` has no address: the keyword is not a handle, so say what is missing
    // instead of complaining about the keyword.
    if addresses.eq_ignore_ascii_case("delta") || addresses.eq_ignore_ascii_case("from") {
        return Err(bad("move", "address the objects to move: move 1A2[,1A3] delta dx,dy"));
    }
    let handles = handles("move", addresses)?;
    if handles.is_empty() {
        return Err(bad("move", "address the objects to move: move 1A2[,1A3] delta dx,dy"));
    }
    check_in_space(drawing, space, "move", &handles)?;
    let params = match rest {
        [keyword, delta] if keyword.eq_ignore_ascii_case("delta") => {
            let [dx, dy] = point(delta, "move")?;
            json!({ "handles": hex(&handles), "delta": [dx, dy] })
        }
        [from_kw, from, to_kw, to] if from_kw.eq_ignore_ascii_case("from") && to_kw.eq_ignore_ascii_case("to") => {
            let [x1, y1] = point(from, "move")?;
            let [x2, y2] = point(to, "move")?;
            json!({ "handles": hex(&handles), "from": [x1, y1], "to": [x2, y2] })
        }
        _ => {
            return Err(bad("move", "give a displacement: move 1A2[,1A3] delta dx,dy — or move 1A2[,1A3] from x,y to x,y"));
        }
    };
    Ok(Command { id: "move", params })
}

fn copy(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("copy", "address the objects to copy: copy 1A2[,1A3] delta dx,dy"));
    };
    // `copy delta 10,0` has no address: the keyword is not a handle, so say what is missing.
    if addresses.eq_ignore_ascii_case("delta") || addresses.eq_ignore_ascii_case("from") {
        return Err(bad("copy", "address the objects to copy: copy 1A2[,1A3] delta dx,dy"));
    }
    let handles = handles("copy", addresses)?;
    if handles.is_empty() {
        return Err(bad("copy", "address the objects to copy: copy 1A2[,1A3] delta dx,dy"));
    }
    check_in_space(drawing, space, "copy", &handles)?;
    let params = match rest {
        [delta] => {
            let [dx, dy] = point(delta, "copy")?;
            json!({ "handles": hex(&handles), "delta": [dx, dy] })
        }
        [keyword, delta] if keyword.eq_ignore_ascii_case("delta") => {
            let [dx, dy] = point(delta, "copy")?;
            json!({ "handles": hex(&handles), "delta": [dx, dy] })
        }
        _ => {
            return Err(bad("copy", "give a displacement: copy 1A2[,1A3] delta dx,dy"));
        }
    };
    Ok(Command { id: "copy", params })
}

fn rotate(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("rotate", "address the objects to rotate: rotate 1A2[,1A3] base x,y angle"));
    };
    // `rotate base 0,0 45` has no address: the keyword is not a handle, so say what is missing.
    if addresses.eq_ignore_ascii_case("base") {
        return Err(bad("rotate", "address the objects to rotate: rotate 1A2[,1A3] base x,y angle"));
    }
    let handles = handles("rotate", addresses)?;
    if handles.is_empty() {
        return Err(bad("rotate", "address the objects to rotate: rotate 1A2[,1A3] base x,y angle"));
    }
    check_in_space(drawing, space, "rotate", &handles)?;
    let params = match rest {
        [keyword, base, angle] if keyword.eq_ignore_ascii_case("base") => {
            let [x, y] = point(base, "rotate")?;
            let angle = number(angle).ok_or_else(|| bad("rotate", "the angle must be a finite number of degrees"))?;
            json!({ "handles": hex(&handles), "base": [x, y], "angle": angle })
        }
        _ => {
            return Err(bad("rotate", "give the base point and the angle in degrees: rotate 1A2[,1A3] base x,y angle"));
        }
    };
    Ok(Command { id: "rotate", params })
}

fn mirror(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("mirror", "address the objects to mirror: mirror 1A2[,1A3] x1,y1 x2,y2"));
    };
    let handles = handles("mirror", addresses)?;
    if handles.is_empty() {
        return Err(bad("mirror", "address the objects to mirror: mirror 1A2[,1A3] x1,y1 x2,y2"));
    }
    check_in_space(drawing, space, "mirror", &handles)?;
    let [first, second] = rest else {
        return Err(bad("mirror", "give the two points of the mirror line: mirror 1A2[,1A3] x1,y1 x2,y2"));
    };
    let [x1, y1] = point(first, "mirror")?;
    let [x2, y2] = point(second, "mirror")?;
    // A line whose points coincide has no direction; the engine refuses it too, but say so here,
    // before an operation exists.
    if (x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9 {
        return Err(bad("mirror", "the two points of the mirror line coincide; give two different points"));
    }
    // The source objects stay: the engine's `erase` is left out, as AutoCAD's default answer.
    Ok(Command { id: "mirror", params: json!({ "handles": hex(&handles), "p1": [x1, y1], "p2": [x2, y2] }) })
}

fn scale(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("scale", "address the objects to scale: scale 1A2[,1A3] base x,y factor"));
    };
    // `scale base 0,0 2` has no address: the keyword is not a handle, so say what is missing.
    if addresses.eq_ignore_ascii_case("base") {
        return Err(bad("scale", "address the objects to scale: scale 1A2[,1A3] base x,y factor"));
    }
    let handles = handles("scale", addresses)?;
    if handles.is_empty() {
        return Err(bad("scale", "address the objects to scale: scale 1A2[,1A3] base x,y factor"));
    }
    check_in_space(drawing, space, "scale", &handles)?;
    let [keyword, base, factor] = rest else {
        return Err(bad("scale", "give the base point and the factor: scale 1A2[,1A3] base x,y factor"));
    };
    if !keyword.eq_ignore_ascii_case("base") {
        return Err(bad("scale", "give the base point and the factor: scale 1A2[,1A3] base x,y factor"));
    }
    let [x, y] = point(base, "scale")?;
    let factor = number(factor).filter(|f| *f > 0.0).ok_or_else(|| bad("scale", "the factor must be a positive number"))?;
    Ok(Command { id: "scale", params: json!({ "handles": hex(&handles), "base": [x, y], "factor": factor }) })
}

/// `offset H distance d side x,y` — copy the named object `d` away, to the side of the point.
///
/// The engine's OFFSET asks for a distance and a point on the side to offset to; both are spelled
/// out here. The copy goes on the author's layer (`context.clay`), as the shared session's other
/// object-creating commands do (ADR-0019); `layer: "current"` makes the engine read the layer the
/// operation named.
fn offset(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((address, rest)) = words.split_first() else {
        return Err(bad("offset", "address the object to offset: offset 1A2 distance d side x,y"));
    };
    // `offset distance 5 side 0,1` has no address: the keyword is not a handle.
    if address.eq_ignore_ascii_case("distance") || address.eq_ignore_ascii_case("side") {
        return Err(bad("offset", "address the object to offset: offset 1A2 distance d side x,y"));
    }
    let handles = handles("offset", address)?;
    let [handle] = handles.as_slice() else {
        return Err(bad("offset", "takes one object: offset 1A2 distance d side x,y"));
    };
    check_in_space(drawing, space, "offset", &handles)?;
    let [distance_kw, distance, side_kw, side] = rest else {
        return Err(bad("offset", "give the distance and the side: offset 1A2 distance d side x,y"));
    };
    if !distance_kw.eq_ignore_ascii_case("distance") || !side_kw.eq_ignore_ascii_case("side") {
        return Err(bad("offset", "give the distance and the side: offset 1A2 distance d side x,y"));
    }
    // AutoCAD's OFFSET wants a positive distance; a zero or negative one is not an offset.
    let distance = number(distance).filter(|d| *d > 0.0).ok_or_else(|| bad("offset", "the distance must be a positive number"))?;
    let [x, y] = point(side, "offset")?;
    Ok(Command { id: "offset", params: json!({ "handle": handle.hex(), "distance": distance, "side": [x, y], "layer": "current" }) })
}

/// `trim H edge H2[,H3] at x,y` — cut `H` at the crossings with the named edges, removing the
/// piece that holds the pick point `x,y`.
///
/// A trim without a crossing has nothing to cut, so the crossing is checked here, before an
/// operation exists; when the geometry cannot be measured exactly the operation is left to the
/// engine, which refuses it without changing the drawing.
fn trim(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((address, rest)) = words.split_first() else {
        return Err(bad("trim", "address the object to trim: trim 1A2 edge 1A3 at x,y"));
    };
    // `trim edge 1A3 at 0,0` has no address: the keyword is not a handle.
    if address.eq_ignore_ascii_case("edge") || address.eq_ignore_ascii_case("at") {
        return Err(bad("trim", "address the object to trim: trim 1A2 edge 1A3 at x,y"));
    }
    let targets = handles("trim", address)?;
    let [target] = targets.as_slice() else {
        return Err(bad("trim", "takes one object: trim 1A2 edge 1A3 at x,y"));
    };
    check_in_space(drawing, space, "trim", &targets)?;
    let [edge_kw, edges, at_kw, pick] = rest else {
        return Err(bad("trim", "give the cutting edges and the pick: trim 1A2 edge 1A3 at x,y"));
    };
    if !edge_kw.eq_ignore_ascii_case("edge") || !at_kw.eq_ignore_ascii_case("at") {
        return Err(bad("trim", "give the cutting edges and the pick: trim 1A2 edge 1A3 at x,y"));
    }
    let edges = handles("trim", edges)?;
    if edges.is_empty() {
        return Err(bad("trim", "give at least one cutting edge: trim 1A2 edge 1A3 at x,y"));
    }
    if edges.contains(target) {
        return Err(bad("trim", "an object cannot be its own cutting edge"));
    }
    check_in_space(drawing, space, "trim", &edges)?;
    let [x, y] = point(pick, "trim")?;
    if meets(drawing, space, *target, &edges) == Some(false) {
        return Err(bad("trim", "the object does not meet the cutting edge; there is nothing to trim"));
    }
    Ok(Command { id: "trim", params: json!({ "handle": target.hex(), "edges": hex(&edges), "pick": [x, y] }) })
}

/// `array H[,H…] rows r cols c step dx,dy` — a rectangular array of separate copies.
///
/// `dx` is the column step (along x) and `dy` the row step (along y), as the classic -ARRAY lays
/// them out: the source objects stay in row 0, column 0, every other item is a copy. Row and
/// column counts of at least one, at least two items in all, and a non-zero step in each growing
/// direction are required before an operation exists. Copies go on the author's layer
/// (`context.clay`, ADR-0019).
fn array(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("array", "address the objects to array: array 1A2[,1A3] rows r cols c step dx,dy"));
    };
    // `array rows 2 cols 3 step 10,10` has no address: the keyword is not a handle.
    if ["rows", "cols", "step"].iter().any(|keyword| addresses.eq_ignore_ascii_case(keyword)) {
        return Err(bad("array", "address the objects to array: array 1A2[,1A3] rows r cols c step dx,dy"));
    }
    let handles = handles("array", addresses)?;
    if handles.is_empty() {
        return Err(bad("array", "address the objects to array: array 1A2[,1A3] rows r cols c step dx,dy"));
    }
    check_in_space(drawing, space, "array", &handles)?;
    let [rows_kw, rows, cols_kw, cols, step_kw, step] = rest else {
        return Err(bad("array", "give the rows, the columns and the step: array 1A2 rows r cols c step dx,dy"));
    };
    if !rows_kw.eq_ignore_ascii_case("rows") || !cols_kw.eq_ignore_ascii_case("cols") || !step_kw.eq_ignore_ascii_case("step") {
        return Err(bad("array", "give the rows, the columns and the step: array 1A2 rows r cols c step dx,dy"));
    }
    let rows = whole(rows, "array", "rows")?;
    let cols = whole(cols, "array", "cols")?;
    let count = rows.checked_mul(cols).filter(|n| *n >= 2).ok_or_else(|| {
        bad("array", "the rows and columns must be at least 1 each and make at least two items: array 1A2 rows 2 cols 3 step dx,dy")
    })?;
    // Keep a hostile line from reaching the engine with a count no drawing could hold.
    if count > MAX_ARRAY_ITEMS {
        return Err(bad("array", format!("the array would make {count} items; the most one line may name is {MAX_ARRAY_ITEMS}")));
    }
    let [dx, dy] = point(step, "array")?;
    if cols > 1 && dx.abs() < 1e-12 {
        return Err(bad("array", "the column step must be non-zero when there is more than one column"));
    }
    if rows > 1 && dy.abs() < 1e-12 {
        return Err(bad("array", "the row step must be non-zero when there is more than one row"));
    }
    Ok(Command {
        id: "array",
        params: json!({
            "handles": hex(&handles),
            "type": "rectangular",
            "rows": rows,
            "cols": cols,
            "rowSpacing": dy,
            "colSpacing": dx,
            "associative": false
        }),
    })
}

/// The most items one `array` line may make; the engine's MAXARRAY (at most ten million) is the
/// number a drawing could hold, this keeps the operation itself small.
const MAX_ARRAY_ITEMS: u64 = 100_000;

/// `hatch H[,H…] pattern ANSI31 scale s angle a` — fill the closed objects named by handle.
///
/// The pattern, its scale and its angle are named because the server's header may hold another
/// set; `associative` is false and `separate` false, so the fill is one static hatch over the
/// boundary as it stands, the same bytes on every replica. A handle that is not a closed boundary
/// (a circle, a closed polyline, a full ellipse or a closed spline — what the engine's HATCH
/// accepts) and a zero or negative scale are refused here, before an operation exists.
fn hatch_command(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    let Some((addresses, rest)) = words.split_first() else {
        return Err(bad("hatch", "address the closed objects to hatch: hatch 1A2[,1A3] pattern ANSI31 scale s angle a"));
    };
    // `hatch pattern ANSI31 scale 1 angle 0` has no address: the keyword is not a handle.
    if ["pattern", "scale", "angle"].iter().any(|keyword| addresses.eq_ignore_ascii_case(keyword)) {
        return Err(bad("hatch", "address the closed objects to hatch: hatch 1A2[,1A3] pattern ANSI31 scale s angle a"));
    }
    let handles = handles("hatch", addresses)?;
    if handles.is_empty() {
        return Err(bad("hatch", "address the closed objects to hatch: hatch 1A2[,1A3] pattern ANSI31 scale s angle a"));
    }
    check_in_space(drawing, space, "hatch", &handles)?;
    let [pattern_kw, pattern, scale_kw, scale, angle_kw, angle] = rest else {
        return Err(bad("hatch", "give the pattern, the scale and the angle: hatch 1A2[,1A3] pattern ANSI31 scale s angle a"));
    };
    if !pattern_kw.eq_ignore_ascii_case("pattern") || !scale_kw.eq_ignore_ascii_case("scale") || !angle_kw.eq_ignore_ascii_case("angle") {
        return Err(bad("hatch", "give the pattern, the scale and the angle: hatch 1A2[,1A3] pattern ANSI31 scale s angle a"));
    }
    // The engine stores the pattern upper case; a built-in name is what a shared line may rely on,
    // a pattern loaded from a .pat file exists only in the drawing that loaded it.
    let pattern = pattern.trim().to_ascii_uppercase();
    if cadcraft_doc::library::pattern(&pattern).is_none() {
        return Err(bad("hatch", format!("unknown pattern `{}`; the shared command line knows the built-in patterns only", brief(&pattern))));
    }
    let scale = number(scale).filter(|s| *s > 0.0).ok_or_else(|| bad("hatch", "the scale must be a positive number"))?;
    let angle = number(angle).ok_or_else(|| bad("hatch", "the angle must be a finite number of degrees"))?;
    let store = drawing.space(space).ok_or_else(|| bad("hatch", "the active layout is not in this drawing"))?;
    for handle in &handles {
        let Some(entity) = store.get(*handle) else {
            return Err(CmdLineError::UnknownHandle { command: "hatch", handle: handle.hex(), space: space_name(space) });
        };
        if !closed_contour(&entity.kind) {
            return Err(bad(
                "hatch",
                format!("object {} is not a closed boundary; hatch a circle, a closed polyline, a full ellipse or a closed spline", handle.hex()),
            ));
        }
    }
    Ok(Command {
        id: "hatch",
        params: json!({
            "handles": hex(&handles),
            "pattern": pattern,
            "scale": scale,
            "angle": angle,
            "associative": false,
            "separate": false,
            "style": "normal"
        }),
    })
}

/// `wipeout x,y x,y x,y [x,y…]` or `wipeout H` — a masking polygon over explicit boundary points
/// or over the closed polyline named by handle.
///
/// A shared session has no pick for the polyline, so the line names it; either form travels as
/// boundary points, so both replicas draw the same mask even if the polyline is edited later. The
/// handle's polyline must be closed (by its flag or by repeating the first point) and must have no
/// arc segments (a bulge), as the engine's WIPEOUT requires. Fewer than three points, a boundary
/// that encloses no area, a vertex outside the drawing's coordinate limit, a handle that names
/// nothing here and an object that is not such a polyline are refused before an operation exists.
/// `frames` (WIPEOUTFRAME) is a drawing variable shared by everyone, not one participant's object,
/// and is not one-lined.
fn wipeout(drawing: &Drawing, space: &Space, words: Vec<&str>) -> Result<Command, CmdLineError> {
    const WIPEOUT: &str = "wipeout";
    let points = match words.as_slice() {
        [] => return Err(bad(WIPEOUT, "give three or more boundary points or one closed polyline: wipeout x,y x,y x,y [x,y …] | wipeout 1A2")),
        [address] if !address.contains(',') => wipeout_boundary(drawing, space, address)?,
        words if words.iter().all(|word| word.contains(',')) => {
            words.iter().map(|word| drawing_point(word, WIPEOUT)).collect::<Result<Vec<Vec2>, CmdLineError>>()?
        }
        _ => return Err(bad(WIPEOUT, "give boundary points or one closed polyline: wipeout x,y x,y x,y [x,y …] | wipeout 1A2")),
    };
    if points.len() < 3 {
        return Err(bad(WIPEOUT, "a wipeout needs three or more boundary points"));
    }
    // The engine's own ceiling on generated geometry (`cmd::curves::MAX_GEN`); a line is never a
    // drawing's bulk.
    if points.len() > MAX_WIPEOUT_POINTS {
        return Err(bad(WIPEOUT, format!("the boundary has more than {MAX_WIPEOUT_POINTS} points")));
    }
    if points.iter().any(|point| !point.is_finite() || point.x.abs() >= MAX_COORDINATE || point.y.abs() >= MAX_COORDINATE) {
        return Err(bad(WIPEOUT, "a boundary point is outside the drawing's coordinate limit"));
    }
    if cadcraft_doc::geom::shoelace(&points).abs() < 1e-12 {
        return Err(bad(WIPEOUT, "the boundary encloses no area"));
    }
    let points = points.iter().map(|point| [point.x, point.y]).collect::<Vec<_>>();
    // The engine's WIPEOUT takes the boundary; unlike its `handle` form it never removes the
    // source polyline, which stays where the author drew it.
    Ok(Command { id: "wipeout", params: json!({ "points": points }) })
}

/// The most boundary points one `wipeout` line may carry: the engine's ceiling on generated
/// geometry (`cmd::curves::MAX_GEN`), so the parser cannot send more than the command accepts.
const MAX_WIPEOUT_POINTS: usize = 20_000;

/// The boundary of the closed polyline named by `address`, as points, checked the way the engine's
/// WIPEOUT checks a picked polyline (`crates/engine/src/cmd/draw2/wipeout.rs`).
fn wipeout_boundary(drawing: &Drawing, space: &Space, address: &str) -> Result<Vec<Vec2>, CmdLineError> {
    let handles = handles("wipeout", address)?;
    let [handle] = handles.as_slice() else {
        return Err(bad("wipeout", "takes one closed polyline: wipeout 1A2"));
    };
    check_in_space(drawing, space, "wipeout", &handles)?;
    let store = drawing.space(space).ok_or_else(|| bad("wipeout", "the active layout is not in this drawing"))?;
    let entity =
        store.get(*handle).ok_or_else(|| CmdLineError::UnknownHandle { command: "wipeout", handle: handle.hex(), space: space_name(space) })?;
    let cadcraft_doc::EntityKind::LwPolyline(polyline) = &entity.kind else {
        return Err(bad("wipeout", format!("object {} is not a closed polyline; give boundary points instead", handle.hex())));
    };
    let closed = polyline.closed || polyline.vertices.first().zip(polyline.vertices.last()).is_some_and(|(first, last)| first.p.near(last.p, 1e-9));
    if !closed {
        return Err(bad("wipeout", format!("polyline {} is not closed; close it or give boundary points instead", handle.hex())));
    }
    if polyline.vertices.iter().any(|vertex| vertex.bulge.abs() > 1e-12) {
        return Err(bad("wipeout", "a polyline with arc segments cannot bound a wipeout; give boundary points instead"));
    }
    Ok(polyline.vertices.iter().map(|vertex| vertex.p).collect())
}

/// `imageattach PATH data BASE64 [at x,y] [width w | scale s] [rotation deg] [name "Title"]` — put
/// a raster image into the drawing from the bytes the line carries.
///
/// A shared session never opens a file by a path the client chose: the server would read it from
/// its own disk, and the other participants would see another image or a crossed-out frame
/// (ADR-0021). The first word is therefore only the label saved in the drawing and must name a
/// raster type (PNG, JPEG, BMP or TIFF); the pixels travel in `data`, base64, within the
/// operation's size limit. `at` defaults to the origin, `width` is the image's width in drawing
/// units (`scale` is that width at scale 1 = one drawing unit), `rotation` is in degrees and `name`
/// overrides the definition name. A non-numeric coordinate, missing or broken base64, a non-raster
/// extension, data over the limit, an empty name and a rotation without a size are refused before
/// an operation exists.
fn imageattach(words: Vec<&str>) -> Result<Command, CmdLineError> {
    const IMAGE: &str = "imageattach";
    let syntax = "imageattach map.png data <base64> [at x,y] [width w | scale s] [rotation deg] [name \"Title\"]";
    let Some((label, rest)) = words.split_first() else {
        return Err(bad(IMAGE, format!("give the file name and its bytes: {syntax}")));
    };
    let path = unquote(label);
    if path.is_empty() {
        return Err(bad(IMAGE, "the file name is empty"));
    }
    if path.chars().count() > MAX_IMAGE_PATH {
        return Err(bad(IMAGE, format!("the file name is longer than {MAX_IMAGE_PATH} characters")));
    }
    if !raster_extension(&path) {
        return Err(bad(
            IMAGE,
            "the file name must end in png, jpg, jpeg, bmp, tif or tiff; a shared session takes the raster bytes in `data`, not a file by that name",
        ));
    }
    let Some((keyword, rest)) = rest.split_first() else {
        return Err(bad(IMAGE, format!("give the image bytes: {syntax}")));
    };
    if !keyword.eq_ignore_ascii_case("data") {
        return Err(bad(IMAGE, format!("an image in a shared session carries its bytes, not a file to read: {syntax}")));
    }
    let Some((word, rest)) = rest.split_first() else {
        return Err(bad(IMAGE, "`data` is empty; give the image file's bytes as base64"));
    };
    let data = unquote(word);
    if data.is_empty() {
        return Err(bad(IMAGE, "`data` is empty; give the image file's bytes as base64"));
    }
    if data.len() > MAX_IMAGE_DATA {
        return Err(bad(IMAGE, format!("the image data is larger than the operation's 1 MiB limit ({MAX_IMAGE_DATA} base64 characters)")));
    }
    let bytes = crate::message::decode_bytes(&data).map_err(|_| bad(IMAGE, "`data` is not valid base64"))?;
    if bytes.is_empty() {
        return Err(bad(IMAGE, "`data` decodes to no image bytes"));
    }
    if bytes.len() > MAX_IMAGE_DATA {
        return Err(bad(IMAGE, format!("the image bytes are larger than the operation's 1 MiB limit ({MAX_IMAGE_DATA} bytes)")));
    }
    let mut at = [0.0, 0.0];
    let mut width = None;
    let mut scale = None;
    let mut rotation = 0.0;
    let mut name: Option<String> = None;
    let mut index = 0;
    while index < rest.len() {
        let keyword = rest[index];
        let Some(value) = rest.get(index + 1).copied() else {
            return Err(bad(IMAGE, format!("`{}` needs a value", brief(keyword))));
        };
        index += 2;
        match keyword.to_ascii_lowercase().as_str() {
            "at" => {
                let corner = drawing_point(value, IMAGE)?;
                at = [corner.x, corner.y];
            }
            "width" => {
                let w = number(value).filter(|w| *w > 0.0).ok_or_else(|| bad(IMAGE, "the width must be a positive number"))?;
                width = Some(w);
            }
            "scale" => {
                let s = number(value).filter(|s| *s > 0.0).ok_or_else(|| bad(IMAGE, "the scale must be a positive number"))?;
                scale = Some(s);
            }
            "rotation" => {
                rotation = number(value).ok_or_else(|| bad(IMAGE, "the rotation must be a finite number of degrees"))?;
            }
            "name" => {
                let text = unquote(value);
                if text.trim().is_empty() {
                    return Err(bad(IMAGE, "the name is empty"));
                }
                name = Some(text);
            }
            _ => {
                return Err(bad(
                    IMAGE,
                    format!("`{}` is not a parameter; use at x,y, width w, scale s, rotation deg and name \"…\"", brief(keyword)),
                ));
            }
        }
    }
    if width.is_some() && scale.is_some() {
        return Err(bad(IMAGE, "give the size once: width w or scale s"));
    }
    if rotation != 0.0 && width.is_none() && scale.is_none() {
        return Err(bad(IMAGE, "a rotation needs a size: give width w or scale s, or leave the rotation out"));
    }
    let mut params = json!({ "path": path, "data": data, "at": at });
    if let Some(width) = width {
        params["width"] = json!(width);
    } else if let Some(scale) = scale {
        params["scale"] = json!(scale);
    }
    if rotation != 0.0 {
        params["rotation"] = json!(rotation);
    }
    if let Some(name) = name {
        params["name"] = json!(name);
    }
    Ok(Command { id: "imageattach", params })
}

/// The longest file label one line may carry: the engine's own limit on an image path.
const MAX_IMAGE_PATH: usize = 4096;

/// The most base64 text one line may carry. A shared session's operation is at most 1 MiB of JSON
/// parameters (`cadcraft-collab-server::policy::MAX_OP_BYTES`); base64 inflates the bytes by a third
/// and the rest of the parameters need room, so the image text is bounded below that limit.
const MAX_IMAGE_DATA: usize = (1 << 20) - 4096;

/// Whether `path` names a raster type IMAGEATTACH decodes: PNG, JPEG, BMP or TIFF.
fn raster_extension(path: &str) -> bool {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let Some((stem, extension)) = file.rsplit_once('.') else { return false };
    !stem.is_empty() && matches!(extension.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff")
}

/// A word as text: the inside of a `"…"` token, else the word itself.
fn unquote(word: &str) -> String {
    quoted(word).unwrap_or_else(|| word.to_string())
}

/// Whether the object encloses an area: the boundary kinds the engine's HATCH accepts when it is
/// given handles (`crates/engine/src/cmd/hatch.rs::closed_loop`), checked before an operation
/// exists so a line that hatches nothing is refused on the author's side.
fn closed_contour(kind: &cadcraft_doc::EntityKind) -> bool {
    use cadcraft_doc::EntityKind;
    match kind {
        EntityKind::Circle(_) => true,
        EntityKind::LwPolyline(polyline) => {
            polyline.closed || polyline.vertices.first().zip(polyline.vertices.last()).is_some_and(|(first, last)| first.p.near(last.p, 1e-9))
        }
        EntityKind::Ellipse(ellipse) => cadcraft_doc::geom::Ellipse {
            center: ellipse.center.xy(),
            major: ellipse.major.xy(),
            ratio: ellipse.ratio,
            start: ellipse.start,
            end: ellipse.end,
        }
        .is_full(),
        EntityKind::Spline(spline) => spline.closed,
        _ => false,
    }
}

/// A whole, positive count.
fn whole(text: &str, command: &'static str, label: &str) -> Result<u64, CmdLineError> {
    text.parse::<u64>()
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| bad(command, format!("`{}` is not a whole number of at least 1 for {label}", brief(text))))
}

/// Whether the target meets any of the edges: `Some(false)` means it certainly does not, `None`
/// that the geometry could not be measured and the engine has to decide.
fn meets(drawing: &Drawing, space: &Space, target: Handle, edges: &[Handle]) -> Option<bool> {
    let store = drawing.space(space)?;
    let entity = store.get(target)?;
    let target_segments = segments(&entity.kind)?;
    let bounds = entity_bounds(drawing, entity, 0);
    let mut measured = true;
    for edge in edges {
        let Some(other) = store.get(*edge) else { return Some(false) };
        let Some(edge_segments) = segments(&other.kind) else {
            measured = false;
            continue;
        };
        if !bounds.intersects(&entity_bounds(drawing, other, 0)) {
            continue;
        }
        if target_segments.iter().any(|a| edge_segments.iter().any(|b| !cadcraft_doc::geom::intersect_segments(a, b).is_empty())) {
            return Some(true);
        }
    }
    measured.then_some(false)
}

/// `kind` as line and arc segments, for the trim crossing check; `None` when it has no measurable
/// geometry (a block reference or an object the engine handles some other way).
fn segments(kind: &cadcraft_doc::EntityKind) -> Option<Vec<Segment>> {
    let prims = kind.prims();
    if prims.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for prim in prims {
        match prim {
            Prim::Seg(segment) => out.push(segment),
            Prim::Circle(circle) => {
                out.push(Segment::Arc { arc: Arc::new(circle.center, circle.radius, 0.0, std::f64::consts::TAU - 1e-12), ccw: true })
            }
            Prim::Infinite { base, dir, .. } => {
                let far = dir * 1e8;
                out.push(Segment::Line(Line::new(base - far, base + far)));
            }
            Prim::Ellipse(ellipse) => {
                let mut points = Vec::new();
                ellipse.tessellate(ellipse.major.len() * 1e-4, &mut points);
                push_polyline(&mut out, &points);
            }
            Prim::Spline(spline) => push_polyline(&mut out, &spline.tessellate(1e-4)),
            Prim::Fill(points) => push_polyline(&mut out, &points),
            Prim::Point(_) => {}
        }
    }
    Some(out)
}

fn push_polyline(out: &mut Vec<Segment>, points: &[Vec2]) {
    for pair in points.windows(2) {
        if let [a, b] = pair {
            out.push(Segment::Line(Line::new(*a, *b)));
        }
    }
}

fn handles(command: &'static str, addresses: &str) -> Result<Vec<Handle>, CmdLineError> {
    addresses.split(',').map(|address| Handle::parse_hex(address).ok_or_else(|| bad(command, "address objects by handle, e.g. 1A2,1A3"))).collect()
}

/// Refuse handles that name nothing in `space` before an operation is built (ADR-0018).
fn check_in_space(drawing: &Drawing, space: &Space, command: &'static str, handles: &[Handle]) -> Result<(), CmdLineError> {
    let store = drawing.space(space).ok_or_else(|| bad(command, "the active layout is not in this drawing"))?;
    for handle in handles {
        if store.get(*handle).is_none() {
            return Err(CmdLineError::UnknownHandle { command, handle: handle.hex(), space: space_name(space) });
        }
    }
    Ok(())
}

fn hex(handles: &[Handle]) -> Vec<String> {
    handles.iter().map(|handle| handle.hex()).collect()
}

/// Split a line into words; a double-quoted word may hold spaces and `\"`/`\\` escapes.
///
/// The quotes stay in the token, so only the command that wants a quoted string strips them
/// ([`quoted`]); an unterminated quote simply runs to the end of the line and is refused there.
fn tokenize(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quoted = false;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if quoted {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        if c.is_whitespace() {
            if let Some(from) = start.take() {
                out.push(&text[from..index]);
            }
        } else if start.is_none() {
            start = Some(index);
            quoted = c == '"';
        } else if c == '"' {
            quoted = true;
        }
    }
    if let Some(from) = start {
        out.push(&text[from..]);
    }
    out
}

/// The inside of a `"…"` token, with `\"` and `\\` unescaped; `None` when it is not quoted.
fn quoted(token: &str) -> Option<String> {
    let inner = token.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(chars.next()?);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// `x,y` with finite coordinates.
fn point(word: &str, command: &'static str) -> Result<[f64; 2], CmdLineError> {
    let mut parts = word.split(',');
    let (Some(x), Some(y), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad(command, format!("`{}` is not a point; give x,y", brief(word))));
    };
    let x = number(x).ok_or_else(|| bad(command, format!("`{}` is not a finite number", brief(x))))?;
    let y = number(y).ok_or_else(|| bad(command, format!("`{}` is not a finite number", brief(y))))?;
    Ok([x, y])
}

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn bad(command: &'static str, detail: impl Into<String>) -> CmdLineError {
    CmdLineError::BadArguments { command, detail: detail.into() }
}

fn space_name(space: &Space) -> String {
    match space {
        Space::Model => "model space".to_string(),
        Space::Paper(name) => format!("layout `{}`", brief(name)),
    }
}

/// The text as it can be echoed back: user input is hostile and must not make a message unbounded.
fn brief(text: &str) -> String {
    text.chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{Common, EntityKind, Point};

    fn drawing_with_line() -> (Drawing, Handle) {
        let mut drawing = Drawing::new_metric();
        let handle = drawing.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 2.0, 0.0), angle: 0.0 })).unwrap();
        (drawing, handle)
    }

    #[test]
    fn a_line_is_two_or_more_points() {
        let (drawing, _) = drawing_with_line();
        let command = parse("line 0,0 100,0 100,50", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "line");
        assert_eq!(command.params, json!({ "points": [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]] }));
        // The AutoCAD alias works too.
        assert_eq!(parse("l -1.5,2e1 3,4", &drawing, &Space::Model).unwrap().id, "line");
    }

    #[test]
    fn a_circle_is_a_centre_and_a_radius() {
        let (drawing, _) = drawing_with_line();
        let command = parse("circle 10,20 2.5", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "circle");
        assert_eq!(command.params, json!({ "center": [10.0, 20.0], "radius": 2.5 }));
        assert!(matches!(parse("circle 10,20 0", &drawing, &Space::Model), Err(CmdLineError::BadArguments { .. })));
        assert!(matches!(parse("circle 10,20 -1", &drawing, &Space::Model), Err(CmdLineError::BadArguments { .. })));
    }

    #[test]
    fn a_move_addresses_its_objects_by_handle() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("move {address} delta 10,-2.5"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "move");
        assert_eq!(command.params, json!({ "handles": [address.clone()], "delta": [10.0, -2.5] }));

        let command = parse(&format!("move {address} from 0,0 to 10,0"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params, json!({ "handles": [address], "from": [0.0, 0.0], "to": [10.0, 0.0] }));
    }

    #[test]
    fn an_unknown_command_is_refused_and_changes_nothing() {
        let (drawing, _) = drawing_with_line();
        let before = drawing.clone();
        let error = parse("explode 1A2", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownCommand(_)), "{error}");
        assert!(
            error.to_string().contains(
                "line, circle, polyline, ellipse, rect, text, dimlinear, mleader, spline, block, insert, move, copy, rotate, mirror, scale, offset, trim, array, hatch, wipeout, imageattach or erase"
            ),
            "{error}"
        );
        assert_eq!(drawing, before, "parsing must not change the drawing");
    }

    #[test]
    fn every_command_is_named_in_the_refusals() {
        let empty = CmdLineError::Empty.to_string();
        let unknown = CmdLineError::UnknownCommand("explode".into()).to_string();
        for name in COMMANDS {
            assert!(empty.contains(name), "`{name}` is missing from `{empty}`");
            assert!(unknown.contains(name), "`{name}` is missing from `{unknown}`");
        }
    }

    #[test]
    fn an_erase_addresses_its_objects_by_handle() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("erase {address}"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "erase");
        assert_eq!(command.params, json!({ "handles": [address.clone()] }));
        // The AutoCAD aliases work too.
        assert_eq!(parse(&format!("e {address}"), &drawing, &Space::Model).unwrap().id, "erase");
        assert_eq!(parse(&format!("delete {address}"), &drawing, &Space::Model).unwrap().id, "erase");
        // Erasing needs an address: there is no selection to fall back on in a shared session.
        let error = parse("erase", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects to erase"), "{error}");
        // And no trailing arguments.
        let error = parse(&format!("erase {address} now"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("takes only handles"), "{error}");
    }

    #[test]
    fn an_unknown_erase_handle_is_refused_before_an_operation_exists() {
        let (drawing, _) = drawing_with_line();
        let before = drawing.clone();
        let error = parse("erase 1FF", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("erase: there is no object 1FF"), "{error}");
        assert_eq!(drawing, before);
    }

    #[test]
    fn a_polyline_is_two_or_more_points() {
        let (drawing, _) = drawing_with_line();
        let command = parse("polyline 0,0 100,0 100,50", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "pline", "the engine command id");
        assert_eq!(command.params, json!({ "vertices": [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]] }));
        assert_eq!(parse("pline 0,0 1,1", &drawing, &Space::Model).unwrap().id, "pline");
        assert_eq!(parse("pl 0,0 1,1", &drawing, &Space::Model).unwrap().id, "pline");
        // One point is not a polyline.
        let error = parse("polyline 0,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("at least two points"), "{error}");
        let error = parse("polyline 0,0 nowhere", &drawing, &Space::Model).unwrap_err();
        assert_eq!(error.to_string(), "polyline: `nowhere` is not a point; give x,y");
    }

    #[test]
    fn a_rect_is_two_opposite_corners() {
        let (drawing, _) = drawing_with_line();
        let command = parse("rect 0,0 100,50", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "rectang", "the engine command id");
        assert_eq!(command.params, json!({ "p1": [0.0, 0.0], "p2": [100.0, 50.0] }));
        assert_eq!(parse("rectangle -5,-5 5,5", &drawing, &Space::Model).unwrap().id, "rectang");
        assert_eq!(parse("rec 0,0 1,1", &drawing, &Space::Model).unwrap().id, "rectang");
        // Two corners, no more.
        let error = parse("rect 0,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("takes two opposite corners"), "{error}");
        let error = parse("rect 0,0 1,1 2,2", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("takes two opposite corners"), "{error}");
        // A rectangle with no area is refused here, not on the server.
        let error = parse("rect 0,0 0,50", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("rectangle with no area"), "{error}");
        let error = parse("rect 0,0 100,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("rectangle with no area"), "{error}");
    }

    #[test]
    fn a_copy_addresses_its_objects_and_a_displacement() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("copy {address} 10,-2.5"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "copy");
        assert_eq!(command.params, json!({ "handles": [address.clone()], "delta": [10.0, -2.5] }));
        // The `delta` keyword and the AutoCAD aliases work too.
        assert_eq!(parse(&format!("co {address} delta 1,1"), &drawing, &Space::Model).unwrap().id, "copy");
        assert_eq!(parse(&format!("cp {address} 1,1"), &drawing, &Space::Model).unwrap().id, "copy");
        // No displacement at all.
        let error = parse(&format!("copy {address}"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("give a displacement"), "{error}");
        // The keyword without an address says what is missing instead of complaining about `delta`.
        let error = parse("copy delta 10,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects to copy"), "{error}");
        // A handle that names nothing in this drawing.
        let error = parse("copy FF 1,1", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("copy: there is no object FF"), "{error}");
    }

    #[test]
    fn a_rotate_names_its_base_point_and_the_angle_in_degrees() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("rotate {address} base 10,20 45"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "rotate");
        assert_eq!(command.params, json!({ "handles": [address.clone()], "base": [10.0, 20.0], "angle": 45.0 }));
        assert_eq!(parse(&format!("ro {address} base 0,0 -90.5"), &drawing, &Space::Model).unwrap().id, "rotate");
        // No angle.
        let error = parse(&format!("rotate {address} base 0,0"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("give the base point and the angle"), "{error}");
        // The keyword without an address.
        let error = parse("rotate base 0,0 45", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects to rotate"), "{error}");
        // A handle that names nothing in this drawing.
        let error = parse("rotate FF base 0,0 45", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("rotate: there is no object FF"), "{error}");
        // An angle that is not a number.
        let error = parse(&format!("rotate {address} base 0,0 later"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("finite number of degrees"), "{error}");
    }

    #[test]
    fn an_ellipse_is_a_centre_radii_and_an_angle() {
        let (drawing, _) = drawing_with_line();
        let command = parse("ellipse 10,20 5,3", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "ellipse");
        assert_eq!(command.params, json!({ "center": [10.0, 20.0], "major": [5.0, 0.0], "ratio": 0.6 }));
        // One radius makes an ellipse with equal axes; the AutoCAD alias works too.
        let round = parse("el 0,0 5", &drawing, &Space::Model).unwrap();
        assert_eq!(round.id, "ellipse");
        assert_eq!(round.params, json!({ "center": [0.0, 0.0], "major": [5.0, 0.0], "ratio": 1.0 }));
        // An angle in degrees turns the major axis.
        let turned = parse("ellipse 0,0 4,2 90", &drawing, &Space::Model).unwrap();
        let major = turned.params["major"].as_array().expect("a major vector");
        assert!(major[0].as_f64().expect("x").abs() < 1e-12, "{major:?}");
        assert!((major[1].as_f64().expect("y") - 4.0).abs() < 1e-12, "{major:?}");
        assert!((turned.params["ratio"].as_f64().expect("ratio") - 0.5).abs() < 1e-12);
    }

    #[test]
    fn an_unaddressable_ellipse_is_refused_before_an_operation_exists() {
        let (drawing, _) = drawing_with_line();
        let before = drawing.clone();
        for text in [
            "ellipse 10,20",
            "ellipse 10,20 0",
            "ellipse 10,20 0,2",
            "ellipse 10,20 5,0",
            "ellipse 10,20 5,-1",
            "ellipse 10,20 3,5",
            "ellipse 10,20 5,3,1",
            "ellipse 10,20 5,3 later",
            "ellipse 10,20 5 5,3",
        ] {
            let error = parse(text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("ellipse:"), "{text}: {error}");
        }
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_mirror_addresses_its_objects_and_the_two_points_of_its_line() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("mirror {address} 0,0 0,10"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "mirror");
        assert_eq!(command.params, json!({ "handles": [address.clone()], "p1": [0.0, 0.0], "p2": [0.0, 10.0] }));
        assert_eq!(parse(&format!("mi {address} -1,-1 1,1"), &drawing, &Space::Model).unwrap().id, "mirror");
        // No mirror line.
        let error = parse(&format!("mirror {address}"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("give the two points of the mirror line"), "{error}");
        // A line whose points coincide.
        let error = parse(&format!("mirror {address} 5,5 5,5"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("coincide"), "{error}");
        // No address at all: the first word is not a handle.
        let error = parse("mirror GHI 0,0 1,1", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address objects by handle"), "{error}");
        // A handle that names nothing in this drawing.
        let error = parse("mirror FF 0,0 1,1", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("mirror: there is no object FF"), "{error}");
    }

    #[test]
    fn a_scale_names_its_base_point_and_the_factor() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("scale {address} base 10,20 2.5"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "scale");
        assert_eq!(command.params, json!({ "handles": [address.clone()], "base": [10.0, 20.0], "factor": 2.5 }));
        assert_eq!(parse(&format!("sc {address} base 0,0 0.5"), &drawing, &Space::Model).unwrap().id, "scale");
        // No factor, or the keyword without a base point.
        let error = parse(&format!("scale {address} base 0,0"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("give the base point and the factor"), "{error}");
        let error = parse(&format!("scale {address} 0,0 2"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("give the base point and the factor"), "{error}");
        // The keyword without an address.
        let error = parse("scale base 0,0 2", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects to scale"), "{error}");
        // A factor that is not positive.
        for text in [format!("scale {address} base 0,0 0"), format!("scale {address} base 0,0 -2")] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().contains("the factor must be a positive number"), "{error}");
        }
        // A handle that names nothing in this drawing.
        let error = parse("scale FF base 0,0 2", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("scale: there is no object FF"), "{error}");
    }

    #[test]
    fn an_offset_addresses_its_object_distance_and_side() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("offset {address} distance 2.5 side 0,1"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "offset");
        assert_eq!(command.params, json!({ "handle": address.clone(), "distance": 2.5, "side": [0.0, 1.0], "layer": "current" }));
        assert_eq!(parse(&format!("o {address} distance 1 side -1,0"), &drawing, &Space::Model).unwrap().id, "offset");
        // A zero or negative distance is not an offset.
        for text in [format!("offset {address} distance 0 side 0,1"), format!("offset {address} distance -2 side 0,1")] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().contains("the distance must be a positive number"), "{error}");
        }
        // The keyword without an address says what is missing.
        let error = parse("offset distance 2 side 0,1", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the object to offset"), "{error}");
        // Offset takes one object.
        let error = parse(&format!("offset {address},{address} distance 2 side 0,1"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("takes one object"), "{error}");
        // A handle that names nothing in this drawing.
        let error = parse("offset FF distance 2 side 0,1", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("offset: there is no object FF"), "{error}");
    }

    #[test]
    fn a_trim_addresses_its_object_edge_and_pick() {
        let mut drawing = Drawing::new_metric();
        let target = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        let edge = add_line(&mut drawing, [5.0, -5.0], [5.0, 5.0]);
        let far = add_line(&mut drawing, [0.0, 100.0], [10.0, 100.0]);

        let command = parse(&format!("trim {} edge {} at 8,0", target.hex(), edge.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "trim");
        assert_eq!(command.params, json!({ "handle": target.hex(), "edges": [edge.hex()], "pick": [8.0, 0.0] }));
        assert_eq!(parse(&format!("tr {} edge {} at 8,0", target.hex(), edge.hex()), &drawing, &Space::Model).unwrap().id, "trim");
        // A target that the cutting edge never meets has nothing to trim, so no operation is built.
        let error = parse(&format!("trim {} edge {} at 8,0", target.hex(), far.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("does not meet the cutting edge"), "{error}");
        // The keywords are missing.
        let error = parse(&format!("trim {} {} at 8,0", target.hex(), edge.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("give the cutting edges and the pick"), "{error}");
        // An object cannot be its own cutting edge.
        let error = parse(&format!("trim {} edge {} at 8,0", target.hex(), target.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("cannot be its own cutting edge"), "{error}");
        // The keyword without an address says what is missing.
        let error = parse("trim edge 1A3 at 0,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the object to trim"), "{error}");
        // Handles that name nothing in this drawing.
        let error = parse("trim FF edge 1A3 at 0,0", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("trim: there is no object FF"), "{error}");
        let error = parse(&format!("trim {} edge FF at 0,0", target.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("trim: there is no object FF"), "{error}");
    }

    #[test]
    fn an_array_addresses_its_objects_rows_columns_and_step() {
        let (drawing, handle) = drawing_with_line();
        let address = handle.hex();
        let command = parse(&format!("array {address} rows 2 cols 3 step 20,10"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "array");
        assert_eq!(
            command.params,
            json!({
                "handles": [address.clone()],
                "type": "rectangular",
                "rows": 2,
                "cols": 3,
                "rowSpacing": 10.0,
                "colSpacing": 20.0,
                "associative": false
            })
        );
        assert_eq!(parse(&format!("ar {address} rows 1 cols 4 step 5,0"), &drawing, &Space::Model).unwrap().id, "array");
        assert_eq!(parse(&format!("arr {address} rows 4 cols 1 step 0,5"), &drawing, &Space::Model).unwrap().id, "array");
        // Counts of zero, a one-item array, and a zero step where the array grows are all refused.
        for text in [
            format!("array {address} rows 0 cols 3 step 10,10"),
            format!("array {address} rows 2 cols 0 step 10,10"),
            format!("array {address} rows -1 cols 3 step 10,10"),
            format!("array {address} rows 1 cols 1 step 10,10"),
            format!("array {address} rows 2 cols 3 step 0,10"),
            format!("array {address} rows 2 cols 3 step 10,0"),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("array:"), "{text}: {error}");
        }
        // The keyword without an address says what is missing.
        let error = parse("array rows 2 cols 3 step 10,10", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects to array"), "{error}");
        // A handle that names nothing in this drawing.
        let error = parse("array FF rows 2 cols 3 step 10,10", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("array: there is no object FF"), "{error}");
    }

    #[test]
    fn a_text_names_its_point_height_angle_and_quoted_content() {
        let (drawing, _) = drawing_with_line();
        let command = parse("text 10,20 2.5 30 \"Shared note\"", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "text");
        assert_eq!(command.params, json!({ "at": [10.0, 20.0], "height": 2.5, "rotation": 30.0, "text": "Shared note" }));
        // The content may hold spaces, punctuation and an escaped quote.
        let command = parse("dt 0,0 1 0 \"say \\\"hi\\\", then leave\"", &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["text"], "say \"hi\", then leave");
        // The AutoCAD aliases work too; a negative angle is a rotation the other way.
        let command = parse("dtext 0,0 1 -45.5 \"x\"", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "text");
        assert_eq!(command.params["rotation"], -45.5);
        // An empty quoted string is text with nothing to draw.
        let error = parse("text 0,0 1 0 \"\"", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("the text is empty"), "{error}");
        let error = parse("text 0,0 1 0 \"   \"", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("the text is empty"), "{error}");
    }

    #[test]
    fn an_unaddressable_text_is_refused_before_an_operation_exists() {
        let (drawing, _) = drawing_with_line();
        let before = drawing.clone();
        for text in [
            "text 10,20 2.5",
            "text 10,20 2.5 0",
            "text 10,20 2.5 0 shared",
            "text 10,20 2.5 0 \"unterminated",
            "text 10,20 0 0 \"note\"",
            "text 10,20 -1 0 \"note\"",
            "text 10,20 2.5 later \"note\"",
            "text nowhere 2.5 0 \"note\"",
            "text 10,20 2.5 0 \"note\" trailing",
        ] {
            let error = parse(text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("text:"), "{text}: {error}");
        }
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_hatch_addresses_its_closed_objects_pattern_scale_and_angle() {
        let mut drawing = Drawing::new_metric();
        let rect = add_rect(&mut drawing, [0.0, 0.0], [10.0, 5.0]);
        let command = parse(&format!("hatch {} pattern ansi31 scale 2 angle 45", rect.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "hatch");
        assert_eq!(
            command.params,
            json!({
                "handles": [rect.hex()],
                "pattern": "ANSI31",
                "scale": 2.0,
                "angle": 45.0,
                "associative": false,
                "separate": false,
                "style": "normal"
            })
        );
        // A solid fill and the AutoCAD aliases work too.
        assert_eq!(parse(&format!("h {} pattern SOLID scale 1 angle 0", rect.hex()), &drawing, &Space::Model).unwrap().id, "hatch");
        assert_eq!(parse(&format!("bh {} pattern ANSI31 scale 1 angle 0", rect.hex()), &drawing, &Space::Model).unwrap().id, "hatch");
    }

    #[test]
    fn an_unaddressable_hatch_is_refused_before_an_operation_exists() {
        let mut drawing = Drawing::new_metric();
        let rect = add_rect(&mut drawing, [0.0, 0.0], [10.0, 5.0]);
        let line = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        let before = drawing.clone();
        for text in [
            "hatch pattern ANSI31 scale 1 angle 0".to_string(),
            format!("hatch {} pattern ANSI31 scale 1", rect.hex()),
            format!("hatch {} pattern ANSI31 scale 0 angle 0", rect.hex()),
            format!("hatch {} pattern ANSI31 scale -2 angle 0", rect.hex()),
            format!("hatch {} pattern ANSI31 scale 1 angle later", rect.hex()),
            format!("hatch {} pattern NOPE scale 1 angle 0", rect.hex()),
            // An open line has no inside to fill.
            format!("hatch {} pattern ANSI31 scale 1 angle 0", line.hex()),
            // One closed object does not make an open one hatching.
            format!("hatch {},{} pattern ANSI31 scale 1 angle 0", rect.hex(), line.hex()),
            "hatch FF pattern ANSI31 scale 1 angle 0".to_string(),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("hatch:"), "{text}: {error}");
        }
        let error = parse(&format!("hatch {} pattern ANSI31 scale 1 angle 0", line.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("is not a closed boundary"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_wipeout_takes_boundary_points_or_a_closed_polyline() {
        let mut drawing = Drawing::new_metric();
        let command = parse("wipeout 0,0 10,0 10,10", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "wipeout");
        assert_eq!(command.params, json!({ "points": [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]] }));
        // The alias works too.
        assert_eq!(parse("wi 0,0 10,0 10,10", &drawing, &Space::Model).unwrap().id, "wipeout");

        // A closed polyline handle travels as its boundary points, so both replicas draw the same
        // mask and editing the polyline later does not move it.
        let rect = add_rect(&mut drawing, [20.0, 0.0], [30.0, 5.0]);
        let command = parse(&format!("wipeout {}", rect.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "wipeout");
        assert_eq!(command.params, json!({ "points": [[20.0, 0.0], [30.0, 0.0], [30.0, 5.0], [20.0, 5.0]] }));
    }

    #[test]
    fn an_unaddressable_wipeout_is_refused_before_an_operation_exists() {
        let mut drawing = Drawing::new_metric();
        let rect = add_rect(&mut drawing, [0.0, 0.0], [10.0, 5.0]);
        let line = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        let open = drawing
            .add(
                &Space::Model,
                Common::default(),
                EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                    vertices: vec![
                        cadcraft_doc::geom::PolyVertex::new(Vec2::new(0.0, 0.0)),
                        cadcraft_doc::geom::PolyVertex::new(Vec2::new(10.0, 0.0)),
                        cadcraft_doc::geom::PolyVertex::new(Vec2::new(10.0, 5.0)),
                    ],
                    closed: false,
                    const_width: 0.0,
                    elevation: 0.0,
                    plinegen: false,
                }),
            )
            .unwrap();
        let curved = drawing
            .add(
                &Space::Model,
                Common::default(),
                EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                    vertices: vec![
                        cadcraft_doc::geom::PolyVertex::with_bulge(Vec2::new(40.0, 0.0), 0.5),
                        cadcraft_doc::geom::PolyVertex::new(Vec2::new(50.0, 0.0)),
                        cadcraft_doc::geom::PolyVertex::new(Vec2::new(50.0, 5.0)),
                    ],
                    closed: true,
                    const_width: 0.0,
                    elevation: 0.0,
                    plinegen: false,
                }),
            )
            .unwrap();
        let before = drawing.clone();
        for text in [
            // Nothing to bound, too few points, a straight line of points.
            "wipeout".to_string(),
            "wipeout 0,0 10,0".to_string(),
            "wipeout 0,0 10,0 20,0".to_string(),
            // A coordinate that is not a number.
            "wipeout 0,0 10,0 ten,10".to_string(),
            // A point and a handle do not mix.
            format!("wipeout 0,0 10,0 {}", rect.hex()).to_string(),
            // A handle that names nothing, an object that is not a polyline, an open polyline and
            // one with an arc segment.
            "wipeout FF".to_string(),
            format!("wipeout {}", line.hex()),
            format!("wipeout {}", open.hex()),
            format!("wipeout {}", curved.hex()),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("wipeout:"), "{text}: {error}");
        }
        let error = parse(&format!("wipeout {}", open.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("is not closed"), "{error}");
        let error = parse(&format!("wipeout {}", curved.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("arc segments"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    /// A 1×1 red PNG: a raster IMAGEATTACH decodes.
    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

    #[test]
    fn an_imageattach_carries_the_bytes_and_its_placement() {
        let drawing = Drawing::new_metric();
        let command = parse(&format!("imageattach map.png data {PNG_1X1}"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "imageattach");
        assert_eq!(command.params["path"], "map.png", "the first word is the label saved in the drawing");
        assert_eq!(command.params["data"], PNG_1X1, "the pixels travel in the operation");
        assert_eq!(command.params["at"], json!([0.0, 0.0]), "no `at` means the origin");

        // The alias works; the placement, the width, the rotation and the definition name are
        // explicit parameters, and a quoted label may hold spaces.
        let text = format!("iat \"site plan.png\" data {PNG_1X1} at 10,5 width 100 rotation 15 name \"Site map\"");
        let command = parse(&text, &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["path"], "site plan.png");
        assert_eq!(command.params["at"], json!([10.0, 5.0]), "the lower-left corner");
        assert_eq!(command.params["width"], 100.0);
        assert_eq!(command.params["rotation"], 15.0);
        assert_eq!(command.params["name"], "Site map");
        // `scale` is the same width at scale 1; the extension is not case-sensitive.
        let command = parse(&format!("imageattach logo.JPG data {PNG_1X1} scale 2"), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["scale"], 2.0);
        assert!(command.params.get("width").is_none());
    }

    #[test]
    fn an_unaddressable_imageattach_is_refused_before_an_operation_exists() {
        let drawing = Drawing::new_metric();
        let before = drawing.clone();
        let over_limit = "A".repeat(MAX_IMAGE_DATA + 1);
        let refused = [
            // No label, no bytes, no `data` keyword.
            "imageattach".to_string(),
            "imageattach map.png".to_string(),
            format!("imageattach map.png {PNG_1X1}"),
            format!("imageattach map.png path {PNG_1X1}"),
            "imageattach map.png data".to_string(),
            // Empty and broken base64.
            "imageattach map.png data \"\"".to_string(),
            "imageattach map.png data not-base64!".to_string(),
            // A label that is not a raster type, or has no extension at all.
            format!("imageattach notes.txt data {PNG_1X1}"),
            format!("imageattach map data {PNG_1X1}"),
            // Data over the operation's limit.
            format!("imageattach map.png data {over_limit}"),
            // Non-numeric coordinates, an empty name, a rotation without a size.
            format!("imageattach map.png data {PNG_1X1} at ten,5"),
            format!("imageattach map.png data {PNG_1X1} name \"\""),
            format!("imageattach map.png data {PNG_1X1} rotation 15"),
            // A size given twice, a zero size and an unknown parameter.
            format!("imageattach map.png data {PNG_1X1} width 10 scale 2"),
            format!("imageattach map.png data {PNG_1X1} width 0"),
            format!("imageattach map.png data {PNG_1X1} colour red"),
        ];
        for text in &refused {
            let error = parse(text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("imageattach:"), "{}: {error}", &text[..text.len().min(48)]);
        }
        let error = parse(&format!("imageattach notes.txt data {PNG_1X1}"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("png, jpg, jpeg, bmp, tif or tiff"), "{error}");
        let error = parse("imageattach map.png data not-base64!", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("not valid base64"), "{error}");
        let error = parse(&format!("imageattach map.png data {over_limit}"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("1 MiB limit"), "{error}");
        let error = parse(&format!("imageattach map.png data {PNG_1X1} rotation 15"), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("rotation needs a size"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_dimlinear_measures_between_two_objects() {
        let mut drawing = Drawing::new_metric();
        let point = |drawing: &mut Drawing, x: f64, y: f64| {
            drawing.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(x, y, 0.0), angle: 0.0 })).unwrap()
        };
        let a = point(&mut drawing, 0.0, 0.0);
        let b = point(&mut drawing, 10.0, 0.0);

        // `offset` puts the dimension line parallel to the measured one, to its left.
        let command = parse(&format!("dimlinear {},{} offset 5", a.hex(), b.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "dimlinear");
        assert_eq!(command.params, json!({ "p1": [0.0, 0.0], "p2": [10.0, 0.0], "at": [5.0, 5.0] }));
        assert_eq!(parse(&format!("dimlin {},{} offset -2", a.hex(), b.hex()), &drawing, &Space::Model).unwrap().params["at"], json!([5.0, -2.0]));

        // `point` places the dimension line directly; the AutoCAD alias works too.
        let command = parse(&format!("dli {},{} point 0,20", a.hex(), b.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params, json!({ "p1": [0.0, 0.0], "p2": [10.0, 0.0], "at": [0.0, 20.0] }));

        // A line is measured from its nearer characteristic point.
        let line = add_line(&mut drawing, [3.0, 10.0], [13.0, 10.0]);
        let command = parse(&format!("dimlinear {},{} offset 1", a.hex(), line.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["p2"], json!([3.0, 10.0]), "the line's end nearest the point");
    }

    #[test]
    fn an_unaddressable_dimlinear_is_refused_before_an_operation_exists() {
        let (mut drawing, point) = drawing_with_line();
        // A second point at the same place makes a dimension with no length.
        let coincident = drawing.add(&Space::Model, Common::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 2.0, 0.0), angle: 0.0 })).unwrap();
        let far = add_line(&mut drawing, [50.0, 50.0], [60.0, 50.0]);
        let before = drawing.clone();
        for text in [
            format!("dimlinear {} offset 5", point.hex()),
            format!("dimlinear {},{} offset 5", point.hex(), point.hex()),
            "dimlinear 1FF,1FE offset 5".to_string(),
            format!("dimlinear {},{} offset 5", point.hex(), coincident.hex()),
            format!("dimlinear {},{} offset later", point.hex(), far.hex()),
            format!("dimlinear {},{} offset 1e13", point.hex(), far.hex()),
            format!("dimlinear {},{}", point.hex(), far.hex()),
            "dimlinear offset 5".to_string(),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("dimlinear:") || matches!(error, CmdLineError::UnknownHandle { .. }), "{text}: {error}");
        }
        let error = parse(&format!("dimlinear {},{} offset 5", point.hex(), coincident.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("the two points coincide"), "{error}");
        let error = parse(&format!("dimlinear {},{} offset later", point.hex(), far.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("the offset must be a finite number"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_spline_is_three_to_sixty_four_points() {
        let (drawing, _) = drawing_with_line();
        let command = parse("spline 0,0 10,5 20,0", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "spline");
        assert_eq!(command.params, json!({ "fit": [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]] }));
        // The optional trailing keyword closes the curve; the AutoCAD alias works too.
        let closed = parse("spl 0,0 10,5 20,0 closed", &drawing, &Space::Model).unwrap();
        assert_eq!(closed.id, "spline");
        assert_eq!(closed.params["closed"], true);
        // Sixty-four points is the most one line may carry.
        let points = |n: usize| (0..n).map(|i| format!("{i},{}", i % 7)).collect::<Vec<_>>().join(" ");
        assert_eq!(parse(&format!("spline {}", points(64)), &drawing, &Space::Model).unwrap().id, "spline");
        let error = parse(&format!("spline {}", points(65)), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("at most 64 points"), "{error}");
    }

    #[test]
    fn an_unaddressable_spline_is_refused_before_an_operation_exists() {
        let (drawing, _) = drawing_with_line();
        let before = drawing.clone();
        let points = |n: usize| (0..n).map(|i| format!("{i},{}", i % 7)).collect::<Vec<_>>().join(" ");
        for text in [
            "spline 0,0 1,1".to_string(),
            format!("spline {}", points(65)),
            "spline 0,0 1,1 later".to_string(),
            "spline 0,0 1,1 1e13,0".to_string(),
            "spline 0,0 0,0 0,0".to_string(),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(error.to_string().starts_with("spline:"), "{text}: {error}");
        }
        let error = parse("spline 0,0 1,1", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("at least three points"), "{error}");
        let error = parse("spline 0,0 1,1 1e13,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("outside the drawing's coordinate limit"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_mleader_points_at_its_objects_and_lands_on_the_point() {
        let mut drawing = Drawing::new_metric();
        let a = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        let command = parse(&format!("mleader {} 20,10 \"Shared note\"", a.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "mleader");
        assert_eq!(command.params, json!({ "points": [[10.0, 0.0], [20.0, 10.0]], "text": "Shared note" }));
        // The AutoCAD alias works, and the content may hold spaces and escaped quotes.
        let command = parse(&format!("mld {} 20,10 \"say \\\"hi\\\"\"", a.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["text"], "say \"hi\"");
        // Several objects make one leader through their nearest points, with the landing last.
        let b = add_line(&mut drawing, [0.0, -5.0], [10.0, -5.0]);
        let command = parse(&format!("mleader {},{} 20,-10 \"two\"", a.hex(), b.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["points"], json!([[10.0, 0.0], [10.0, -5.0], [20.0, -10.0]]));
    }

    #[test]
    fn an_unaddressable_mleader_is_refused_before_an_operation_exists() {
        let mut drawing = Drawing::new_metric();
        let line = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        // Text has no characteristic point a leader could attach to.
        let no_point = drawing
            .add(
                &Space::Model,
                Common::default(),
                EntityKind::Text(cadcraft_doc::Text {
                    insert: Vec3::new(0.0, 0.0, 0.0),
                    align_pt: None,
                    height: 1.0,
                    value: "x".into(),
                    rotation: 0.0,
                    width_factor: 1.0,
                    oblique: 0.0,
                    style: "Standard".into(),
                    halign: cadcraft_doc::HAlign::Left,
                    valign: cadcraft_doc::VAlign::Baseline,
                }),
            )
            .unwrap();
        let before = drawing.clone();
        for text in [
            // No address: the first word is the landing point, not a handle.
            "mleader 20,10 \"note\"".to_string(),
            format!("mleader {} 20,10 \"\"", line.hex()),
            format!("mleader {} 20,10 note", line.hex()),
            "mleader FF 20,10 \"note\"".to_string(),
            format!("mleader {} 10,0 \"note\"", line.hex()),
            format!("mleader {} nowhere \"note\"", line.hex()),
            format!("mleader {} 20,10 \"note\"", no_point.hex()),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(!error.to_string().is_empty(), "{text}");
        }
        let error = parse(&format!("mleader {} 20,10 \"note\"", no_point.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("has no point to point at"), "{error}");
        let error = parse(&format!("mleader {} 10,0 \"note\"", line.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("the landing point coincides"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn a_block_names_its_objects_and_base_point() {
        let mut drawing = Drawing::new_metric();
        let a = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        let command = parse(&format!("block DOOR base 0,0 {}", a.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "block");
        assert_eq!(command.params, json!({ "name": "DOOR", "base": [0.0, 0.0], "handles": [a.hex()], "keep": "convert" }));
        // The AutoCAD alias works, and a name with spaces is quoted like the text.
        let command = parse(&format!("b \"Front Door\" base 1,2 {}", a.hex()), &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["name"], "Front Door");
        assert_eq!(command.params["base"], json!([1.0, 2.0]));
    }

    #[test]
    fn an_unaddressable_block_is_refused_before_an_operation_exists() {
        let mut drawing = Drawing::new_metric();
        let line = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        drawing.blocks.insert("DOOR".into(), std::sync::Arc::new(cadcraft_doc::Block::new("DOOR")));
        let before = drawing.clone();
        for text in [
            // The base point is named; the name comes first.
            format!("block base 0,0 {}", line.hex()),
            format!("block DOOR 0,0 {}", line.hex()),
            // An empty, anonymous, taken or unwritable name.
            "block \"\" base 0,0 1A2".to_string(),
            format!("block \"*X\" base 0,0 {}", line.hex()),
            format!("block door base 0,0 {}", line.hex()),
            format!("block \"bad,name\" base 0,0 {}", line.hex()),
            format!("block \"unterminated base 0,0 {}", line.hex()),
            // Handles that name nothing, and no handle at all.
            "block PART base 0,0 FF".to_string(),
            "block PART base 0,0".to_string(),
            format!("block PART base nowhere {}", line.hex()),
        ] {
            let error = parse(&text, &drawing, &Space::Model).unwrap_err();
            assert!(!error.to_string().is_empty(), "{text}");
        }
        let error = parse(&format!("block door base 0,0 {}", line.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("already in this drawing"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    #[test]
    fn an_insert_names_its_block_point_scale_and_angle() {
        let mut drawing = Drawing::new_metric();
        drawing.blocks.insert("DOOR".into(), std::sync::Arc::new(cadcraft_doc::Block::new("DOOR")));
        let command = parse("insert DOOR at 10,5", &drawing, &Space::Model).unwrap();
        assert_eq!(command.id, "insert");
        assert_eq!(command.params, json!({ "name": "DOOR", "at": [10.0, 5.0], "scale": 1.0, "rotation": 0.0 }));
        // The alias and the optional scale and angle work; the name is not case-sensitive.
        assert_eq!(parse("i DOOR at 10,5 scale 2", &drawing, &Space::Model).unwrap().params["scale"], 2.0);
        let command = parse("insert door at 10,5 scale 2 angle 90", &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["name"], "DOOR", "the drawing's spelling travels");
        assert_eq!(command.params["rotation"], 90.0);
        let command = parse("insert DOOR at 10,5 angle -45", &drawing, &Space::Model).unwrap();
        assert_eq!(command.params["rotation"], -45.0);
    }

    #[test]
    fn an_unaddressable_insert_is_refused_before_an_operation_exists() {
        let mut drawing = Drawing::new_metric();
        drawing.blocks.insert("DOOR".into(), std::sync::Arc::new(cadcraft_doc::Block::new("DOOR")));
        let before = drawing.clone();
        for text in [
            "insert NOPE at 0,0",
            "insert DOOR 0,0",
            "insert DOOR at 0,0 scale 0",
            "insert DOOR at 0,0 scale -1",
            "insert DOOR at 0,0 angle later",
            "insert DOOR at nowhere",
            "insert DOOR at 0,0 scale 2 angle",
            "insert DOOR at 0,0 later 2",
            "insert at 0,0",
        ] {
            let error = parse(text, &drawing, &Space::Model).unwrap_err();
            assert!(!error.to_string().is_empty(), "{text}");
        }
        let error = parse("insert NOPE at 0,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("there is no block `NOPE`"), "{error}");
        let error = parse("insert DOOR at 0,0 scale 0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("scale must be a positive number"), "{error}");
        assert_eq!(drawing, before, "a refused line never changes the drawing");
    }

    fn add_rect(drawing: &mut Drawing, a: [f64; 2], b: [f64; 2]) -> Handle {
        let vertices = vec![
            cadcraft_doc::geom::PolyVertex::new(Vec2::new(a[0], a[1])),
            cadcraft_doc::geom::PolyVertex::new(Vec2::new(b[0], a[1])),
            cadcraft_doc::geom::PolyVertex::new(Vec2::new(b[0], b[1])),
            cadcraft_doc::geom::PolyVertex::new(Vec2::new(a[0], b[1])),
        ];
        let polyline = cadcraft_doc::LwPolyline { vertices, closed: true, const_width: 0.0, elevation: 0.0, plinegen: false };
        drawing.add(&Space::Model, Common::default(), EntityKind::LwPolyline(polyline)).unwrap()
    }

    fn add_line(drawing: &mut Drawing, a: [f64; 2], b: [f64; 2]) -> Handle {
        drawing
            .add(
                &Space::Model,
                Common::default(),
                EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) }),
            )
            .unwrap()
    }

    #[test]
    fn an_unaddressable_move_says_what_is_wrong() {
        let (drawing, _) = drawing_with_line();
        // No object named at all.
        let error = parse("move delta 10,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects"), "{error}");
        // A handle that names nothing in this drawing.
        let error = parse("move FF delta 10,0", &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
        assert!(error.to_string().contains("there is no object FF"), "{error}");
        // A command that needs a selection cannot be one-lined without one.
        let error = parse("move", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("address the objects"), "{error}");
    }

    #[test]
    fn malformed_arguments_name_the_command_and_the_word() {
        let (drawing, handle) = drawing_with_line();
        let error = parse("line 0,0 nowhere", &drawing, &Space::Model).unwrap_err();
        assert_eq!(error.to_string(), "line: `nowhere` is not a point; give x,y");
        let error = parse("circle 0,0", &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().starts_with("circle: takes a centre and a radius"), "{error}");
        let error = parse(&format!("move {} delta 0,0,0", handle.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(error.to_string().contains("is not a point"), "{error}");
    }

    #[test]
    fn an_empty_line_is_not_a_command() {
        let (drawing, _) = drawing_with_line();
        assert_eq!(parse("   \n\t", &drawing, &Space::Model), Err(CmdLineError::Empty));
    }

    #[test]
    fn an_operation_carries_the_space_and_no_selection() {
        let (drawing, handle) = drawing_with_line();
        let op = op_for(&format!("move {} delta 1,1", handle.hex()), &drawing, &Space::Model, "local-0", 7).unwrap();
        assert_eq!(op.command, "move");
        assert_eq!(op.op_id, "local-0");
        assert_eq!(op.base_version, 7);
        assert_eq!(op.kind, OpKind::Command);
        assert_eq!(op.context.space, Space::Model);
        assert_eq!(op.context.selection, None, "explicit addresses mean no selection is sent");
        assert!(op.actor.is_empty(), "the server fills the actor from the JWT");
    }

    #[test]
    fn a_command_that_creates_objects_names_the_authors_layer() {
        let (mut drawing, handle) = drawing_with_line();
        drawing.layers.push(cadcraft_doc::Layer::new("WALLS"));
        // The author draws on `walls`; the operation names the drawing's own spelling.
        drawing.header.set_str("CLAYER", "walls");
        let op = op_for("line 0,0 10,0", &drawing, &Space::Model, "local-0", 0).unwrap();
        assert_eq!(op.context.clay.as_deref(), Some("WALLS"));
        assert_eq!(op_for("circle 0,0 5", &drawing, &Space::Model, "local-1", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        assert_eq!(op_for("polyline 0,0 1,1", &drawing, &Space::Model, "local-2", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        assert_eq!(op_for("rect 0,0 10,5", &drawing, &Space::Model, "local-5", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        assert_eq!(op_for("ellipse 0,0 5,2", &drawing, &Space::Model, "local-8", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        assert_eq!(op_for("text 0,0 1 0 \"note\"", &drawing, &Space::Model, "local-14", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        // A dimension and a spline are new objects too.
        let other = add_line(&mut drawing, [20.0, 0.0], [20.0, 10.0]);
        assert_eq!(
            op_for(&format!("dimlinear {},{} offset 1", handle.hex(), other.hex()), &drawing, &Space::Model, "local-16", 0)
                .unwrap()
                .context
                .clay
                .as_deref(),
            Some("WALLS")
        );
        assert_eq!(op_for("spline 0,0 10,5 20,0", &drawing, &Space::Model, "local-17", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        // A multileader, a new block and a block reference are new objects too.
        assert_eq!(
            op_for(&format!("mleader {} 20,10 \"note\"", handle.hex()), &drawing, &Space::Model, "local-18", 0).unwrap().context.clay.as_deref(),
            Some("WALLS")
        );
        assert_eq!(
            op_for(&format!("block PART base 0,0 {}", handle.hex()), &drawing, &Space::Model, "local-19", 0).unwrap().context.clay.as_deref(),
            Some("WALLS")
        );
        drawing.blocks.insert("PART".into(), std::sync::Arc::new(cadcraft_doc::Block::new("PART")));
        assert_eq!(op_for("insert PART at 5,5", &drawing, &Space::Model, "local-20", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        // Commands that only address existing objects do not name a layer.
        assert_eq!(op_for(&format!("move {} delta 1,1", handle.hex()), &drawing, &Space::Model, "local-3", 0).unwrap().context.clay, None);
        assert_eq!(op_for(&format!("erase {}", handle.hex()), &drawing, &Space::Model, "local-4", 0).unwrap().context.clay, None);
        // A copy inherits the source objects' layers; a rotate changes them in place.
        assert_eq!(op_for(&format!("copy {} 1,1", handle.hex()), &drawing, &Space::Model, "local-6", 0).unwrap().context.clay, None);
        assert_eq!(op_for(&format!("rotate {} base 0,0 45", handle.hex()), &drawing, &Space::Model, "local-7", 0).unwrap().context.clay, None);
        // A mirror copies the named objects and they keep their own layers; a scale changes them
        // in place. Neither draws on the author's current layer.
        assert_eq!(op_for(&format!("mirror {} 0,0 0,5", handle.hex()), &drawing, &Space::Model, "local-9", 0).unwrap().context.clay, None);
        assert_eq!(op_for(&format!("scale {} base 0,0 2", handle.hex()), &drawing, &Space::Model, "local-10", 0).unwrap().context.clay, None);
        // An offset and an array make new objects; they go on the author's layer too.
        assert_eq!(
            op_for(&format!("offset {} distance 1 side 0,1", handle.hex()), &drawing, &Space::Model, "local-11", 0).unwrap().context.clay.as_deref(),
            Some("WALLS")
        );
        assert_eq!(
            op_for(&format!("array {} rows 2 cols 2 step 10,10", handle.hex()), &drawing, &Space::Model, "local-12", 0)
                .unwrap()
                .context
                .clay
                .as_deref(),
            Some("WALLS")
        );
        // A trim changes the object in place, so it does not name a layer.
        let target = add_line(&mut drawing, [0.0, 0.0], [10.0, 0.0]);
        let edge = add_line(&mut drawing, [5.0, -5.0], [5.0, 5.0]);
        assert_eq!(
            op_for(&format!("trim {} edge {} at 8,0", target.hex(), edge.hex()), &drawing, &Space::Model, "local-13", 0).unwrap().context.clay,
            None
        );
        // A hatch creates a new object and goes on the author's layer too.
        let rect = add_rect(&mut drawing, [20.0, 0.0], [30.0, 5.0]);
        assert_eq!(
            op_for(&format!("hatch {} pattern ANSI31 scale 1 angle 0", rect.hex()), &drawing, &Space::Model, "local-15", 0)
                .unwrap()
                .context
                .clay
                .as_deref(),
            Some("WALLS")
        );
        // A wipeout masks the drawing, so it is a new object on the author's layer too.
        assert_eq!(
            op_for(&format!("wipeout {}", rect.hex()), &drawing, &Space::Model, "local-21", 0).unwrap().context.clay.as_deref(),
            Some("WALLS")
        );
        assert_eq!(op_for("wipeout 0,60 10,60 10,70", &drawing, &Space::Model, "local-22", 0).unwrap().context.clay.as_deref(), Some("WALLS"));
        // An image is a new object too, and its layer travels with the operation.
        assert_eq!(
            op_for(&format!("imageattach map.png data {PNG_1X1}"), &drawing, &Space::Model, "local-23", 0).unwrap().context.clay.as_deref(),
            Some("WALLS")
        );
    }

    #[test]
    fn a_missing_current_layer_is_refused_before_an_operation_exists() {
        let (mut drawing, _) = drawing_with_line();
        drawing.header.set_str("CLAYER", "GHOST");
        let before = drawing.clone();
        let error = op_for("line 0,0 10,0", &drawing, &Space::Model, "local-0", 0).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownLayer { .. }), "{error}");
        assert!(error.to_string().contains("the current layer `GHOST` is not in this drawing"), "{error}");
        assert_eq!(drawing, before, "building an operation never changes the drawing");
    }

    #[test]
    fn a_handle_from_another_space_is_unaddressable() {
        let (mut drawing, _) = drawing_with_line();
        let sheet = Space::Paper("Sheet1".into());
        drawing.layouts.push(cadcraft_doc::Layout::new("Sheet1", 1));
        let in_sheet = drawing.add(&sheet, Common::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 2.0, 0.0), angle: 0.0 })).unwrap();
        let error = parse(&format!("move {} delta 1,1", in_sheet.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
    }
}
