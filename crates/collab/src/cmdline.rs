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
//! move 1A2,1A3 delta 10,0
//! move 1A2 from 0,0 to 10,0
//! copy 1A2,1A3 delta 10,0
//! rotate 1A2 base 0,0 45
//! mirror 1A2,1A3 0,0 0,100
//! scale 1A2,1A3 base 0,0 2
//! erase 1A2,1A3
//! ```

use cadcraft_doc::{Drawing, Handle, Space};
use serde_json::{Value, json};

use crate::{Op, OpContext, OpKind};

/// The command names a one-line shared-session command accepts (aliases included below).
pub const COMMANDS: [&str; 11] = ["line", "circle", "polyline", "ellipse", "rect", "move", "copy", "rotate", "mirror", "scale", "erase"];

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
    #[error("type a command: line, circle, polyline, ellipse, rect, move, copy, rotate, mirror, scale or erase")]
    Empty,
    /// The command is not one of [`COMMANDS`].
    #[error(
        "`{0}` is not available one line at a time in a shared session; use line, circle, polyline, ellipse, rect, move, copy, rotate, mirror, scale or erase"
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
    let mut words = text.split_whitespace();
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
        "move" | "m" => move_(drawing, space, words.collect()),
        "copy" | "co" | "cp" => copy(drawing, space, words.collect()),
        "rotate" | "ro" => rotate(drawing, space, words.collect()),
        "mirror" | "mi" => mirror(drawing, space, words.collect()),
        "scale" | "sc" => scale(drawing, space, words.collect()),
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
    matches!(id, "line" | "circle" | "pline" | "ellipse" | "rectang")
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
        assert!(error.to_string().contains("line, circle, polyline, ellipse, rect, move, copy, rotate, mirror, scale or erase"), "{error}");
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
