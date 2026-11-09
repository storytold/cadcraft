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
//! ```text
//! line 0,0 100,0 100,50
//! circle 50,25 12.5
//! polyline 0,0 100,0 100,50
//! move 1A2,1A3 delta 10,0
//! move 1A2 from 0,0 to 10,0
//! erase 1A2,1A3
//! ```

use cadcraft_doc::{Drawing, Handle, Space};
use serde_json::{Value, json};

use crate::{Op, OpContext, OpKind};

/// The command names a one-line shared-session command accepts (aliases included below).
pub const COMMANDS: [&str; 5] = ["line", "circle", "polyline", "move", "erase"];

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
    #[error("type a command: line, circle, polyline, move or erase")]
    Empty,
    /// The command is not one of [`COMMANDS`].
    #[error("`{0}` is not available one line at a time in a shared session; use line, circle, polyline, move or erase")]
    UnknownCommand(String),
    /// The command is known but its arguments do not parse.
    #[error("{command}: {detail}")]
    BadArguments { command: &'static str, detail: String },
    /// A handle does not name an object in the space the command acts on.
    #[error("{command}: there is no object {handle} in {space}; address objects by their handles (1A2, 1A3, …)")]
    UnknownHandle { command: &'static str, handle: String, space: String },
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
        "move" | "m" => move_(drawing, space, words.collect()),
        "erase" | "e" | "delete" => erase(drawing, space, words.collect()),
        _ => Err(CmdLineError::UnknownCommand(brief(&head))),
    }
}

/// Build the [`Op`] for `text`; `op_id` and `base_version` are the client's, as for any operation.
///
/// The context carries the space but no selection: the parameters address everything they act on,
/// so the server never reads a selection nobody can see.
pub fn op_for(text: &str, drawing: &Drawing, space: &Space, op_id: &str, base_version: u64) -> Result<Op, CmdLineError> {
    let command = parse(text, drawing, space)?;
    Ok(Op {
        op_id: op_id.to_string(),
        actor: String::new(),
        base_version,
        kind: OpKind::Command,
        command: command.id.to_string(),
        params: command.params,
        context: OpContext { space: space.clone(), selection: None },
    })
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
        assert!(error.to_string().contains("line, circle, polyline, move or erase"), "{error}");
        assert_eq!(drawing, before, "parsing must not change the drawing");
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
    fn a_handle_from_another_space_is_unaddressable() {
        let (mut drawing, _) = drawing_with_line();
        let sheet = Space::Paper("Sheet1".into());
        drawing.layouts.push(cadcraft_doc::Layout::new("Sheet1", 1));
        let in_sheet = drawing.add(&sheet, Common::default(), EntityKind::Point(Point { p: Vec3::new(1.0, 2.0, 0.0), angle: 0.0 })).unwrap();
        let error = parse(&format!("move {} delta 1,1", in_sheet.hex()), &drawing, &Space::Model).unwrap_err();
        assert!(matches!(error, CmdLineError::UnknownHandle { .. }), "{error}");
    }
}
