//! The tool bars: strips of command buttons docked along the top of the window (a row) or the
//! left edge (a column). There are three: the standard Tool Bar (file, output, view), the Tool
//! Sets bar (drawing and editing tools) and the Layers bar (layer tools and the current layer).
//!
//! The buttons and their order are the user's: drag a button along the bar to move it, right-click
//! it to remove it, add any Tool Sets command from its right-click menu or from the Customize Tool
//! Bar dialog (Window ▸ Customize Tool Bar...). Drag the handle at the start of the bar to dock it
//! at the top or on the left. Every button has a tooltip with its name, command, alias and
//! keyboard shortcut.
//!
//! The order is saved as command ids in [`crate::UiState::toolbar_items`] (`"|"` is a group
//! separator); ids the bar does not know are skipped, so a hand-edited or older workspace never
//! breaks it.

use egui::{Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// A group separator in [`crate::UiState::toolbar_items`].
pub const SEPARATOR: &str = "|";
/// Most buttons and separators the bar holds (the saved workspace caps the list too).
pub const MAX_ITEMS: usize = 200;
/// Height of the bar docked at the top, width of the bar docked on the left.
pub const THICKNESS: f32 = 34.0;
/// Button size, the gap after a button and the room a separator takes.
const BUTTON: f32 = 26.0;
const GAP: f32 = 6.0;
const SEP: f32 = 13.0;
/// The drag handle at the start of the bar.
const GRIP: f32 = 12.0;

/// Where the Tool Bar is docked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dock {
    /// Along the top of the window, buttons in a row.
    #[default]
    Top,
    /// Along the left edge of the window, buttons in a column.
    Left,
}

impl Dock {
    pub fn as_str(self) -> &'static str {
        match self {
            Dock::Top => "top",
            Dock::Left => "left",
        }
    }

    pub fn parse(s: &str) -> Option<Dock> {
        match s.trim().to_ascii_lowercase().as_str() {
            "top" | "row" | "horizontal" => Some(Dock::Top),
            "left" | "column" | "vertical" => Some(Dock::Left),
            _ => None,
        }
    }
}

/// One of the tool bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Bar {
    /// The standard Tool Bar: file, undo, output, view and inquiry commands.
    Standard,
    /// Drawing and editing tools (the classic Draw and Modify bars).
    ToolSets,
    /// Layer tools and the current layer.
    Layers,
}

impl Bar {
    pub const ALL: [Bar; 3] = [Bar::Standard, Bar::ToolSets, Bar::Layers];

    pub fn id(self) -> &'static str {
        match self {
            Bar::Standard => "standard",
            Bar::ToolSets => "toolSets",
            Bar::Layers => "layers",
        }
    }

    pub fn parse(s: &str) -> Option<Bar> {
        let s = s.trim();
        Bar::ALL.into_iter().find(|b| b.id().eq_ignore_ascii_case(s)).or(match s.to_ascii_lowercase().as_str() {
            "" | "toolbar" | "tool bar" => Some(Bar::Standard),
            "toolsets" | "tool sets" | "draw" => Some(Bar::ToolSets),
            _ => None,
        })
    }

    /// English name (translated when shown).
    pub fn title(self) -> &'static str {
        match self {
            Bar::Standard => "Standard Tool Bar",
            Bar::ToolSets => "Tool Sets Tool Bar",
            Bar::Layers => "Layers Tool Bar",
        }
    }

    /// Accessible label of the bar's More button.
    fn more_label(self) -> &'static str {
        match self {
            Bar::Standard => "More toolbar commands",
            Bar::ToolSets => "More Tool Sets bar commands",
            Bar::Layers => "More Layers bar commands",
        }
    }

    pub fn default_items(self) -> Vec<String> {
        let ids: &[&str] = match self {
            Bar::Standard => return default_items(),
            Bar::ToolSets => &[
                "line",
                "pline",
                "circle",
                "arc",
                "rectang",
                "polygon",
                "ellipse",
                "spline",
                "hatch",
                "|",
                "insert",
                "block",
                "|",
                "text",
                "mtext",
                "|",
                "move",
                "copy",
                "rotate",
                "mirror",
                "scale",
                "offset",
                "arrayrect",
                "trim",
                "extend",
                "fillet",
                "erase",
            ],
            Bar::Layers => &[
                "ui.dialog.layers",
                LAYER_COMBO,
                "|",
                "laymcur",
                "laymch",
                "layerp",
                "|",
                "layiso",
                "layoff",
                "layfrz",
                "laylck",
                "layulk",
                "|",
                TEXTSTYLE_COMBO,
                DIMSTYLE_COMBO,
                LINETYPE_COMBO,
                LWEIGHT_COMBO,
            ],
        };
        ids.iter().map(|s| s.to_string()).collect()
    }

    pub fn default_state(self) -> BarState {
        BarState { show: true, dock: if self == Bar::ToolSets { Dock::Left } else { Dock::Top }, items: self.default_items() }
    }
}

/// Whether a tool bar shows, where it is docked and its buttons in order (command ids; `"|"`
/// separates groups).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BarState {
    pub show: bool,
    pub dock: Dock,
    pub items: Vec<String>,
}

/// The state fields of `bar`: shown, dock, items.
fn fields(ui: &mut crate::UiState, bar: Bar) -> (&mut bool, &mut Dock, &mut Vec<String>) {
    match bar {
        Bar::Standard => (&mut ui.show_toolbar, &mut ui.toolbar_dock, &mut ui.toolbar_items),
        Bar::ToolSets => (&mut ui.toolsets_bar.show, &mut ui.toolsets_bar.dock, &mut ui.toolsets_bar.items),
        Bar::Layers => (&mut ui.layers_bar.show, &mut ui.layers_bar.dock, &mut ui.layers_bar.items),
    }
}

/// A copy of `bar`'s state.
pub fn state(ui: &crate::UiState, bar: Bar) -> BarState {
    match bar {
        Bar::Standard => BarState { show: ui.show_toolbar, dock: ui.toolbar_dock, items: ui.toolbar_items.clone() },
        Bar::ToolSets => ui.toolsets_bar.clone(),
        Bar::Layers => ui.layers_bar.clone(),
    }
}

/// The current-layer drop-down, an item only the bars hold (not a command).
pub const LAYER_COMBO: &str = "layer.combo";
/// The current text style, dimension style and linetype drop-downs.
pub const TEXTSTYLE_COMBO: &str = "textstyle.combo";
pub const DIMSTYLE_COMBO: &str = "dimstyle.combo";
pub const LINETYPE_COMBO: &str = "linetype.combo";
pub const LWEIGHT_COMBO: &str = "lweight.combo";
/// Width of a drop-down row's preview (a linetype's dashes, a lineweight's line).
const PREVIEW_W: f32 = 64.0;
/// Width of the current-layer drop-down in a row.
const COMBO_W: f32 = 220.0;
/// Width of the style and linetype drop-downs in a row.
const STYLE_COMBO_W: f32 = 160.0;
/// Most height a drop-down's list takes before it scrolls.
const COMBO_LIST_H: f32 = 320.0;

/// Width of the steps arrow beside Undo and Redo.
const HISTORY_W: f32 = 13.0;
/// Most steps the Undo and Redo lists show.
const HISTORY_MAX: usize = 100;

/// Whether the bar button for `command` has a list of steps beside it (Undo, Redo).
fn has_history(command: &str) -> bool {
    matches!(command, "u" | "redo")
}

/// The Undo (or Redo) steps, newest first, at most [`HISTORY_MAX`], in a scrolling list; hovering
/// a step highlights it and every step before it. The number of steps picked.
fn history_list(app: &CadApp, ui: &mut egui::Ui, undo: bool) -> Option<usize> {
    let Ok(st) = app.session.state() else { return None };
    let steps: Vec<&str> = if undo {
        st.undo.iter().rev().map(|s| s.label.as_str()).take(HISTORY_MAX).collect()
    } else {
        st.redo.iter().rev().map(|s| s.label.as_str()).take(HISTORY_MAX).collect()
    };
    let t = Tokens::get();
    ui.set_min_width(220.0);
    if steps.is_empty() {
        ui.label(RichText::new(if undo { crate::tl!("Nothing to undo") } else { crate::tl!("Nothing to redo") }).color(t.text_dim));
        return None;
    }
    let hover_id = egui::Id::new(("cc_history_hover", undo));
    let hovered: Option<usize> = ui.data(|d| d.get_temp(hover_id));
    let mut now_hovered = None;
    let mut picked = None;
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().id_salt(("cc_history", undo)).max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        for (i, label) in steps.iter().enumerate() {
            let name = if label.trim().is_empty() { crate::tl!("Change").to_string() } else { label.to_ascii_uppercase() };
            let on = hovered.is_some_and(|h| i <= h);
            let r = ui.selectable_label(on, format!("{}  {name}", i + 1));
            if r.hovered() {
                now_hovered = Some(i);
            }
            if r.clicked() {
                picked = Some(i + 1);
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(hover_id, now_hovered));
    let n = now_hovered.or(hovered).map_or(0, |h| h + 1);
    let what = if undo { crate::tl!("Undo") } else { crate::tl!("Redo") };
    ui.separator();
    ui.label(RichText::new(format!("{what}: {n}")).small().color(t.text_dim));
    picked
}

/// Whether `command` is one of the bars' drop-downs (not a command).
pub fn is_combo(command: &str) -> bool {
    [LAYER_COMBO, TEXTSTYLE_COMBO, DIMSTYLE_COMBO, LINETYPE_COMBO, LWEIGHT_COMBO].contains(&command)
}

/// A drop-down's width in a row.
fn combo_width(command: &str) -> f32 {
    if command == LAYER_COMBO { COMBO_W } else { STYLE_COMBO_W }
}
/// The category of the layer tools.
const LAYERS_CATEGORY: &str = "Layers";
/// The layer tools (the Layers palette's row) and the current layer.
const LAYER_TOOLS: &[(Icon, &str, &str)] = &[
    (Icon::LayerProps, "ui.dialog.layers", "Layer Properties"),
    (Icon::Layers, LAYER_COMBO, "Current Layer"),
    (Icon::MakeCurrent, "laymcur", "Make Object's Layer Current"),
    (Icon::LayerMatch, "laymch", "Match Layer"),
    (Icon::LayerPrev, "layerp", "Previous Layer"),
    (Icon::LayerIso, "layiso", "Isolate"),
    (Icon::LayerOff, "layoff", "Turn Off Layer"),
    (Icon::LayerFreeze, "layfrz", "Freeze Layer"),
    (Icon::LayerLock, "laylck", "Lock Layer"),
    (Icon::Unlock, "layulk", "Unlock Layer"),
    (Icon::Text, TEXTSTYLE_COMBO, "Current Text Style"),
    (Icon::DimLinear, DIMSTYLE_COMBO, "Current Dimension Style"),
    (Icon::Line, LINETYPE_COMBO, "Current Linetype"),
    (Icon::Lineweight, LWEIGHT_COMBO, "Current Lineweight"),
];

/// The default bar: file, undo, output, exchange, navigation, palettes, inquiry and help groups.
const STANDARD: &[&[(Icon, &str, &str, Option<&str>)]] = &[
    &[
        (Icon::New, "new", "New Drawing", Some("Cmd+N")),
        (Icon::Open, "open", "Open", Some("Cmd+O")),
        (Icon::Save, "qsave", "Save", Some("Cmd+S")),
        (Icon::SaveAs, "saveas", "Save As", None),
    ],
    &[(Icon::Undo, "u", "Undo", Some("Cmd+Z")), (Icon::Redo, "redo", "Redo", Some("Cmd+Shift+Z"))],
    &[
        (Icon::Plot, "plot", "Print", None),
        (Icon::Publish, "publish", "Batch Publish", None),
        (Icon::PageSetup, "pagesetup", "Page Setup Manager", None),
        (Icon::Preview, "preview", "Plot Preview", None),
    ],
    &[
        (Icon::Import, "import", "Import", None),
        (Icon::Export, "export", "Export", None),
        (Icon::Attach, "attach", "Attach", None),
        (Icon::Share, "share", "Share", None),
    ],
    &[(Icon::ZoomWindow, "zoom.window", "Zoom Window", None), (Icon::Pan, "pan", "Pan", None), (Icon::Orbit, "3dorbit", "Orbit", None)],
    &[
        (Icon::ZoomExtents, "zoom.extents", "Zoom Extents", None),
        (Icon::Properties, "ui.toggle.palettes", "Properties", None),
        (Icon::Layers, "ui.dialog.layers", "Layer Properties Manager", None),
        (Icon::Blocks, "ui.dialog.blocks", "Blocks", None),
    ],
    &[(Icon::Measure, "dist", "Measure Distance", None), (Icon::List, "list", "List", None), (Icon::Area, "area", "Area", None)],
    &[(Icon::Help, "ui.dialog.about", "Help", None)],
];

/// Standard tools that are not on the bar at first but can be added to it.
const STANDARD_EXTRA: &[(Icon, &str, &str)] =
    &[(Icon::TextStyle, "style", "Text Style"), (Icon::DimStyle, "dimstyle", "Dimension Style"), (Icon::Gear, "options", "Options")];

/// The category of the [`STANDARD`] tools in the Customize dialog.
const STANDARD_CATEGORY: &str = "Standard";

/// A command the Tool Bar can hold.
#[derive(Clone, Copy, Debug)]
pub struct Tool {
    pub icon: Icon,
    /// Command id, as `app.start` takes it.
    pub command: &'static str,
    /// English label (translated when shown).
    pub label: &'static str,
    /// Tool Sets category, or "Standard".
    pub category: &'static str,
    shortcut: Option<&'static str>,
}

impl Tool {
    /// A tool outside the catalog (a Tool Sets button), for its tooltip.
    pub fn new(icon: Icon, command: &'static str, label: &'static str) -> Tool {
        Tool { icon, command, label, category: "", shortcut: None }
    }

    /// Keyboard shortcut, in the menus' notation (`Cmd+S`).
    pub fn shortcut(&self) -> Option<&'static str> {
        self.shortcut
            .or_else(|| cadcraft_engine::find_command(self.command).and_then(|c| c.shortcut))
            .or_else(|| crate::menus::UI_COMMANDS.iter().find(|c| c.0 == self.command).and_then(|c| c.3))
    }

    /// Whether the command exists yet. The standard tools always run (some are routed by the UI).
    pub fn enabled(&self) -> bool {
        self.category == STANDARD_CATEGORY
            || self.category == LAYERS_CATEGORY
            || cadcraft_engine::find_command(self.command).is_some()
            || crate::menus::is_ui_command(self.command)
    }

    /// Name with its shortcut: the button's accessible label.
    fn accessible_label(&self, mac: bool) -> String {
        match self.shortcut() {
            Some(sc) => format!("{} ({})", crate::i18n::t(self.label), crate::menus::shortcut_label(sc, mac)),
            None => crate::i18n::t(self.label).to_string(),
        }
    }
}

fn push(tools: &mut Vec<Tool>, tool: Tool) {
    if !tools.iter().any(|t| t.command == tool.command) {
        tools.push(tool);
    }
}

/// Every command the bar can hold: the standard tools, then the Tool Sets (Drafting, Modeling).
pub fn catalog() -> &'static [Tool] {
    static CATALOG: std::sync::OnceLock<Vec<Tool>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut tools = Vec::new();
        for group in STANDARD {
            for (icon, command, label, shortcut) in group.iter() {
                push(&mut tools, Tool { icon: *icon, command, label, category: STANDARD_CATEGORY, shortcut: *shortcut });
            }
        }
        for (icon, command, label) in STANDARD_EXTRA {
            push(&mut tools, Tool { icon: *icon, command, label, category: STANDARD_CATEGORY, shortcut: None });
        }
        for (icon, command, label) in LAYER_TOOLS {
            push(&mut tools, Tool { icon: *icon, command, label, category: LAYERS_CATEGORY, shortcut: None });
        }
        for groups in [crate::palettes::DRAFTING, crate::palettes::MODELING] {
            for (name, large, small) in groups.iter() {
                for (icon, command, label) in large.iter().chain(small.iter()) {
                    push(&mut tools, Tool { icon: *icon, command, label, category: name, shortcut: None });
                }
            }
        }
        tools
    })
}

/// The catalog entry for a command id.
pub fn find(command: &str) -> Option<Tool> {
    catalog().iter().find(|t| t.command == command).copied()
}

/// The standard bar CADCraft starts with.
pub fn default_items() -> Vec<String> {
    let mut items = Vec::new();
    for (i, group) in STANDARD.iter().enumerate() {
        if i > 0 {
            items.push(SEPARATOR.to_string());
        }
        items.extend(group.iter().map(|(_, command, _, _)| command.to_string()));
    }
    items
}

/// `items` without unknown or repeated commands, doubled, leading or trailing separators, at
/// most [`MAX_ITEMS`] long.
pub fn sanitize(items: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items.iter().take(MAX_ITEMS * 4) {
        if out.len() >= MAX_ITEMS {
            break;
        }
        if item == SEPARATOR {
            if out.last().is_some_and(|last| last != SEPARATOR) {
                out.push(SEPARATOR.to_string());
            }
        } else if find(item).is_some() && !out.contains(item) {
            out.push(item.clone());
        }
    }
    while out.last().is_some_and(|last| last == SEPARATOR) {
        out.pop();
    }
    out
}

/// Move the item at `from` so it lands before the item now at `slot` (`items.len()` = the end).
pub fn move_item(items: &mut Vec<String>, from: usize, slot: usize) {
    if from >= items.len() || slot > items.len() {
        return;
    }
    let item = items.remove(from);
    let to = if slot > from { slot - 1 } else { slot };
    items.insert(to.min(items.len()), item);
}

/// Add `command` before `index` (or at the end). Unknown and present commands are left out.
pub fn add_item(items: &mut Vec<String>, command: &str, index: Option<usize>) -> bool {
    if find(command).is_none() || items.iter().any(|i| i == command) || items.len() >= MAX_ITEMS {
        return false;
    }
    let at = index.unwrap_or(items.len()).min(items.len());
    items.insert(at, command.to_string());
    true
}

/// The tooltip of a tool button: name, command and alias, shortcut.
/// Short descriptions of the tools, shown in their tooltips: (command, what it does).
pub const DESCRIPTIONS: &[(&str, &str)] = &[
    ("new", "Starts a new drawing from the default template."),
    ("open", "Opens a drawing (DWG, DXF) from a file."),
    ("qsave", "Saves the drawing to its file (asks for a name the first time)."),
    ("saveas", "Saves the drawing under a new name, format or version."),
    ("u", "Undoes the last action. The arrow beside it undoes several at once."),
    ("redo", "Redoes what was undone. The arrow beside it redoes several at once."),
    ("plot", "Prints the current layout or model space to a printer or a PDF file."),
    ("publish", "Prints several sheets at once, to separate files or one PDF."),
    ("pagesetup", "Sets the paper, printer and plot settings of layouts."),
    ("preview", "Shows the page as it will be printed."),
    ("import", "Brings another file's contents (DXF, SVG...) into this drawing."),
    ("export", "Saves the drawing or part of it as PDF, SVG, PNG or another format."),
    ("attach", "Attaches an external reference: a drawing, image or PDF shown in this one."),
    ("share", "Shares the drawing with others."),
    ("zoom.window", "Zooms to a rectangle you draw."),
    ("pan", "Moves the view without changing its scale."),
    ("3dorbit", "Turns the view around the drawing in 3D."),
    ("zoom.extents", "Zooms to show the whole drawing."),
    ("ui.toggle.palettes", "Shows or hides the Properties and Layers palettes."),
    ("ui.dialog.layers", "Opens the Layer Properties Manager: create layers, set their colour, linetype and state."),
    ("ui.dialog.blocks", "Opens the block manager: insert, edit and manage blocks."),
    ("dist", "Measures the distance and angle between two points."),
    ("list", "Lists everything about the selected objects in the command history."),
    ("area", "Measures the area and perimeter of an object or of points you pick."),
    ("ui.dialog.about", "About CADCraft and help."),
    ("style", "Creates and changes text styles: font, height, width factor."),
    ("dimstyle", "Creates and changes dimension styles: arrows, text, lines, units."),
    ("options", "Opens the program's settings."),
    ("layer.combo", "The current layer: new objects are drawn on it. Pick another from the list."),
    ("laymcur", "Makes the layer of the object you pick the current layer."),
    ("laymch", "Moves the selected objects to the layer of another object."),
    ("layerp", "Undoes the last change to layer settings."),
    ("layiso", "Hides or locks every layer except those of the selected objects."),
    ("layoff", "Turns off the layer of the object you pick."),
    ("layfrz", "Freezes the layer of the object you pick: hidden and not regenerated."),
    ("laylck", "Locks the layer of the object you pick: its objects can't be changed."),
    ("layulk", "Unlocks the layer of the object you pick."),
    ("textstyle.combo", "The current text style, used by new text."),
    ("dimstyle.combo", "The current dimension style, used by new dimensions."),
    ("linetype.combo", "The linetype of new objects."),
    ("lweight.combo", "The lineweight of new objects."),
    ("line", "Draws straight line segments, one after another."),
    ("pline", "Draws a polyline: connected straight and arc segments that form one object."),
    ("circle", "Draws a circle by its centre and radius, or by points."),
    ("rectang", "Draws a rectangle by two corners, or by its width and height."),
    ("arc", "Draws an arc through three points, or by centre, start and end."),
    ("spline", "Draws a smooth curve through or near the points you pick."),
    ("xline", "Draws an infinite construction line, for alignment."),
    ("ray", "Draws a line that starts at a point and runs on without end."),
    ("ellipse", "Draws an ellipse or an elliptical arc."),
    ("polygon", "Draws a regular polygon with as many sides as you like."),
    ("point.multiple", "Places points, one after another."),
    ("donut", "Draws a filled ring or a filled circle."),
    ("revcloud", "Draws a revision cloud around something to mark a change."),
    ("region", "Turns closed shapes into regions (areas you can combine and measure)."),
    ("wipeout", "Covers what is behind an area with the background colour."),
    ("hatch", "Fills an enclosed area with a pattern or a solid colour."),
    ("gradient", "Fills an enclosed area with a colour gradient."),
    ("boundary", "Draws a polyline around an enclosed area you click inside."),
    ("block", "Groups selected objects into a block that can be inserted again and again."),
    ("insert", "Inserts a block (opens the block manager)."),
    ("bedit", "Opens a block to change what it contains."),
    ("attdef", "Defines an attribute: text asked for each time the block is inserted."),
    ("move", "Moves objects by a distance, from a base point to another point."),
    ("copy", "Copies objects to other places."),
    ("rotate", "Turns objects around a base point."),
    ("trim", "Cuts off the parts of objects that cross other objects. Hover to see what goes."),
    ("fillet", "Rounds the corner between two objects with an arc."),
    ("arrayrect", "Copies objects in rows and columns."),
    ("mirror", "Makes a mirror copy of objects across a line (and can erase the originals)."),
    ("scale", "Makes objects larger or smaller from a base point."),
    ("extend", "Lengthens objects to meet other objects. Hover to see the extension."),
    ("offset", "Makes a parallel copy of a line, curve or outline at a distance."),
    ("arraypolar", "Copies objects around a centre point."),
    ("stretch", "Stretches objects crossed by a selection window, moving their points inside it."),
    ("chamfer", "Bevels the corner between two lines."),
    ("explode", "Breaks a compound object (block, polyline, dimension) into its parts."),
    ("break", "Breaks an object in two, or removes a part between two points."),
    ("join", "Joins lines, arcs and polylines that touch into one object."),
    ("erase", "Deletes the selected objects."),
    ("lengthen", "Changes the length of lines and arcs."),
    ("matchprop", "Copies the properties of one object (layer, colour, style...) to others."),
    ("text", "Writes single-line text. A text starting with = shows a formula's result."),
    ("mtext", "Writes multiline text in a box, with formatting."),
    ("table", "Inserts a table of rows and columns."),
    ("find", "Finds text in the drawing and replaces it."),
    ("dimlinear", "A horizontal or vertical dimension between two points."),
    ("dimaligned", "A dimension parallel to the line between two points."),
    ("dimradius", "The radius of a circle or arc."),
    ("dimdiameter", "The diameter of a circle or arc."),
    ("dimangular", "The angle between two lines, or of an arc."),
    ("dimarc", "The length along an arc."),
    ("dimordinate", "The X or Y coordinate of a point."),
    ("dimbaseline", "More dimensions from the same base line as the last one."),
    ("dimcontinue", "More dimensions continuing from the end of the last one."),
    ("qdim", "Dimensions several objects at once."),
    ("mleader", "A note with an arrow pointing at something (multileader)."),
    ("mleaderedit", "Adds another leader line to a multileader."),
    ("mleaderedit.remove", "Removes a leader line from a multileader."),
    ("mleaderalign", "Lines up several multileaders."),
    ("autoconstrain", "Adds geometric constraints to the selected objects automatically."),
    ("gccoincident", "Keeps two points together."),
    ("gcparallel", "Keeps two lines parallel."),
    ("gcperpendicular", "Keeps two lines at right angles."),
    ("gchorizontal", "Keeps a line horizontal."),
    ("gcvertical", "Keeps a line vertical."),
    ("gctangent", "Keeps a curve tangent to another object."),
    ("gcconcentric", "Keeps circles and arcs on one centre."),
    ("gcequal", "Keeps two lines the same length, or two circles the same size."),
    ("box", "Draws a 3D box."),
    ("cylinder", "Draws a 3D cylinder."),
    ("pyramid", "Draws a 3D pyramid."),
    ("sphere", "Draws a 3D sphere."),
    ("torus", "Draws a 3D torus (a ring)."),
    ("cone", "Draws a 3D cone."),
    ("extrude", "Pulls a closed shape into a 3D solid."),
    ("revolve", "Turns a shape around an axis into a 3D solid."),
    ("sweep", "Sweeps a shape along a path into a 3D solid."),
    ("loft", "Makes a 3D solid through several cross sections."),
    ("union", "Joins 3D solids or regions into one."),
    ("subtract", "Cuts 3D solids or regions out of others."),
    ("intersect", "Keeps only the part 3D solids or regions share."),
];

/// The short description of a command for its tooltip.
pub fn description(command: &str) -> Option<&'static str> {
    DESCRIPTIONS.iter().find(|(c, _)| *c == command).map(|(_, d)| *d)
}

pub fn tooltip(ui: &mut egui::Ui, tool: &Tool, mac: bool) {
    ui.set_max_width(300.0);
    ui.label(RichText::new(crate::i18n::t(tool.label)).strong());
    if let Some(d) = description(tool.command) {
        ui.label(crate::i18n::t(d));
    }
    if !tool.command.starts_with("ui.") && !is_combo(tool.command) {
        let name = tool.command.split('.').next().unwrap_or(tool.command).to_ascii_uppercase();
        let mut line = format!("{} {name}", crate::tl!("Command:"));
        if let Some(alias) = cadcraft_engine::find_command(tool.command).and_then(|c| c.aliases.first()) {
            line.push_str(&format!("    {} {}", crate::tl!("Alias:"), alias.to_ascii_uppercase()));
        }
        ui.label(RichText::new(line).weak());
    }
    if let Some(sc) = tool.shortcut() {
        ui.label(RichText::new(format!("{} {}", crate::tl!("Shortcut:"), crate::menus::shortcut_label(sc, mac))).weak());
    }
    if !tool.enabled() {
        ui.label(RichText::new(crate::tl!("Not available yet")).weak().italics());
    }
}

/// An icon button with the tool tooltip (Tool Sets palette).
pub fn tool_button(ui: &mut egui::Ui, icon: Icon, size: f32, command: &'static str, label: &'static str) -> egui::Response {
    let tool = Tool::new(icon, command, label);
    let mac = ui.ctx().os().is_mac();
    icons::button_bare(ui, icon, size, label, false).on_hover_ui(|ui| tooltip(ui, &tool, mac)).on_disabled_hover_ui(|ui| tooltip(ui, &tool, mac))
}

/// What the bar was asked to do this frame (applied after drawing it).
enum Action {
    Run(&'static str),
    Move {
        from: usize,
        slot: usize,
    },
    Remove(usize),
    Dock(Dock),
    Customize,
    Show(Bar, bool),
    /// A command of the application menu (as the menus run it).
    AppMenu,
    AppCommand(&'static str),
    SetLayer(String),
    LayerToggle(String, &'static str, bool),
    /// A style or linetype drop-down's choice: the drop-down and the name.
    SetCurrent(&'static str, String),
    /// Undo (true) or redo this many steps.
    History(bool, usize),
}

/// The bar's right-click menu; `item` is the button it was opened on.
fn options_menu(ui: &mut egui::Ui, bar: Bar, shown: [bool; 3], item: Option<usize>, dock: Dock, action: &mut Option<Action>) {
    if let Some(index) = item {
        if ui.button(crate::tl!("Remove from Tool Bar")).clicked() {
            *action = Some(Action::Remove(index));
            ui.close();
        }
        ui.separator();
    }
    if ui.radio(dock == Dock::Top, crate::tl!("Dock at Top")).clicked() {
        *action = Some(Action::Dock(Dock::Top));
        ui.close();
    }
    if ui.radio(dock == Dock::Left, crate::tl!("Dock at Left")).clicked() {
        *action = Some(Action::Dock(Dock::Left));
        ui.close();
    }
    ui.separator();
    if ui.button(crate::tl!("Customize Tool Bar...")).clicked() {
        *action = Some(Action::Customize);
        ui.close();
    }
    if ui.button(crate::tl!("Hide Tool Bar")).clicked() {
        *action = Some(Action::Show(bar, false));
        ui.close();
    }
    ui.separator();
    ui.label(RichText::new(crate::tl!("Tool Bars")).weak());
    for (other, on) in Bar::ALL.into_iter().zip(shown) {
        let mut checked = on;
        if ui.checkbox(&mut checked, crate::i18n::t(other.title())).changed() {
            *action = Some(Action::Show(other, checked));
            ui.close();
        }
    }
}

/// Where dropping the bar's handle at `pointer` docks it: near the top or the left edge.
fn drop_target(screen: Rect, pointer: Pos2) -> Option<Dock> {
    let from_left = pointer.x - screen.left();
    let from_top = pointer.y - screen.top();
    if from_left.min(from_top) > 160.0 {
        return None;
    }
    Some(if from_left < from_top { Dock::Left } else { Dock::Top })
}

/// The standard bar docked at the top, in `row` of the title panel.
pub fn top(app: &mut CadApp, ui: &mut egui::Ui, row: Rect) {
    strip(app, ui, row, false, Bar::Standard);
}

/// The other bars docked at the top: one row each, below the menu bar.
pub fn top_panels(app: &mut CadApp, ui: &mut egui::Ui) {
    // A Layers bar saved before the lineweight drop-down existed gets it after the linetype one.
    if !app.ui.lweight_combo_added {
        app.ui.lweight_combo_added = true;
        let items = &mut app.ui.layers_bar.items;
        if !items.iter().any(|i| i == LWEIGHT_COMBO) {
            let at = items.iter().position(|i| i == LINETYPE_COMBO).map_or(items.len(), |i| i + 1);
            items.insert(at, LWEIGHT_COMBO.to_string());
        }
    }
    let t = Tokens::get();
    for bar in [Bar::ToolSets, Bar::Layers] {
        let st = state(&app.ui, bar);
        if !st.show || st.dock != Dock::Top {
            continue;
        }
        egui::Panel::top(egui::Id::new(("cc_toolbar_top", bar.id())))
            .exact_size(THICKNESS)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(t.chrome))
            .show(ui, |ui| {
                let r = ui.max_rect();
                ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.border));
                strip(app, ui, Rect::from_min_max(pos2(r.left() + 12.0, r.top()), r.max), false, bar);
            });
    }
}

/// The bars docked on the left: a column each along the left edge of the window.
pub fn left_panels(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    for bar in Bar::ALL {
        let st = state(&app.ui, bar);
        if !st.show || st.dock != Dock::Left {
            continue;
        }
        let id = if bar == Bar::Standard { egui::Id::new("cc_toolbar_left") } else { egui::Id::new(("cc_toolbar_left", bar.id())) };
        egui::Panel::left(id).exact_size(THICKNESS).resizable(false).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, t.border));
            strip(app, ui, r, true, bar);
        });
    }
}

/// The current layer and every layer, for the drop-down.
fn layer_rows(app: &CadApp) -> Option<(String, Vec<crate::palettes::LayerRow>)> {
    let d = app.session.doc().ok()?;
    let cur = d.header.str("CLAYER", "0").to_string();
    Some((cur, d.layers.iter().map(|l| (l.name.clone(), l.on, l.frozen, l.locked, l.color)).collect()))
}

/// The current name and every name of a style or linetype drop-down.
fn combo_names(app: &CadApp, combo: &str) -> Option<(String, Vec<String>)> {
    let d = app.session.doc().ok()?;
    Some(match combo {
        TEXTSTYLE_COMBO => (d.header.str("TEXTSTYLE", "Standard").to_string(), d.text_styles.iter().map(|s| s.name.clone()).collect()),
        DIMSTYLE_COMBO => (d.header.str("DIMSTYLE", "Standard").to_string(), d.dim_styles.iter().map(|s| s.name.clone()).collect()),
        LINETYPE_COMBO => {
            let mut names = vec!["ByLayer".to_string(), "ByBlock".to_string()];
            for l in &d.linetypes {
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&l.name)) {
                    names.push(l.name.clone());
                }
            }
            if !names.iter().any(|n| n.eq_ignore_ascii_case("Continuous")) {
                names.insert(2, "Continuous".into());
            }
            (d.header.str("CELTYPE", "ByLayer").to_string(), names)
        }
        LWEIGHT_COMBO => {
            let mut names: Vec<String> = ["ByLayer", "ByBlock", "Default"].iter().map(|s| s.to_string()).collect();
            names.extend(cadcraft_engine::doc::Lineweight::STANDARD.iter().map(|v| lweight_name(i64::from(*v))));
            (lweight_name(d.header.i64("CELWEIGHT", -1)), names)
        }
        _ => return None,
    })
}

/// A CELWEIGHT value as the lineweight drop-down names it (`0.25 mm`, ByLayer...).
fn lweight_name(v: i64) -> String {
    match v {
        -1 => "ByLayer".into(),
        -2 => "ByBlock".into(),
        -3 => "Default".into(),
        v => format!("{:.2} mm", v.clamp(0, 211) as f64 / 100.0),
    }
}

/// What a drop-down row previews beside its name.
enum Preview {
    /// Linetype dash patterns (drawing units: dashes positive, gaps negative, dots 0) by
    /// lower-case name; ByLayer shows the current layer's.
    Linetypes(std::collections::HashMap<String, Vec<f64>>, Vec<f64>),
    /// Lineweights (mm): ByLayer shows the current layer's, Default LWDEFAULT.
    Lineweights { layer: f64, default: f64 },
}

/// The preview for the drop-down `combo`, if it has one.
fn combo_preview(app: &CadApp, combo: &str) -> Option<Preview> {
    let d = app.session.doc().ok()?;
    let clayer = d.header.str("CLAYER", "0").to_string();
    let layer = d.layers.iter().find(|l| l.name.eq_ignore_ascii_case(&clayer));
    match combo {
        LINETYPE_COMBO => {
            let pattern = |name: &str| {
                d.linetypes.iter().find(|l| l.name.eq_ignore_ascii_case(name)).map(|l| l.pattern.iter().map(|e| e.length).collect::<Vec<f64>>())
            };
            let map = d.linetypes.iter().map(|l| (l.name.to_lowercase(), l.pattern.iter().map(|e| e.length).collect())).collect();
            let by_layer = layer.and_then(|l| pattern(&l.linetype)).unwrap_or_default();
            Some(Preview::Linetypes(map, by_layer))
        }
        LWEIGHT_COMBO => {
            let default = d.header.i64("LWDEFAULT", 25).clamp(0, 211) as f64 / 100.0;
            let layer = layer.map_or(default, |l| match l.lineweight.to_dxf() {
                v if v >= 0 => f64::from(v) / 100.0,
                _ => default,
            });
            Some(Preview::Lineweights { layer, default })
        }
        _ => None,
    }
}

/// Paint `name`'s preview in `r`.
fn paint_preview(p: &egui::Painter, r: Rect, preview: &Preview, name: &str, color: egui::Color32) {
    let y = r.center().y;
    match preview {
        Preview::Lineweights { layer, default } => {
            let mm = match name {
                "ByLayer" => *layer,
                "ByBlock" | "Default" => *default,
                n => n.trim_end_matches(" mm").trim().parse().unwrap_or(*default),
            };
            // Thicker than on screen (3.78 px per mm) so neighbouring weights tell apart.
            let w = ((mm * 6.0) as f32).clamp(1.0, 12.0);
            p.line_segment([pos2(r.left() + 2.0, y), pos2(r.right() - 2.0, y)], Stroke::new(w, color));
        }
        Preview::Linetypes(map, by_layer) => {
            let pattern: &[f64] = match name {
                "ByLayer" => by_layer,
                "ByBlock" => &[],
                n => map.get(&n.to_lowercase()).map_or(&[], Vec::as_slice),
            };
            let total: f64 = pattern.iter().map(|v| v.abs()).sum();
            let (x0, x1) = (r.left() + 2.0, r.right() - 2.0);
            let st = Stroke::new(1.2, color);
            if pattern.is_empty() || !(total.is_finite() && total > 1e-9) {
                p.line_segment([pos2(x0, y), pos2(x1, y)], st);
                return;
            }
            // About two and a half repeats of the pattern across the preview.
            let k = f64::from(x1 - x0) / (2.5 * total);
            let mut x = f64::from(x0);
            let mut steps = 0;
            'draw: while x < f64::from(x1) {
                for v in pattern {
                    steps += 1;
                    if steps > 400 || x >= f64::from(x1) {
                        break 'draw;
                    }
                    let len = v.abs() * k;
                    if *v > 0.0 {
                        let end = (x + len).min(f64::from(x1));
                        p.line_segment([pos2(x as f32, y), pos2(end as f32, y)], st);
                    } else if *v == 0.0 {
                        p.circle_filled(pos2(x as f32, y), 1.0, color);
                    }
                    x += len.max(if *v == 0.0 { 0.0 } else { 0.5 });
                }
            }
        }
    }
}

/// A drop-down's list of names: picking one makes it current. It scrolls when long.
fn name_list(ui: &mut egui::Ui, salt: &str, current: &str, names: &[String], preview: Option<&Preview>) -> Option<String> {
    let mut picked = None;
    let t = Tokens::get();
    // A scroll bar that is always there once the list is longer than the drop-down.
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().id_salt(("cc_combo_list", salt)).max_height(COMBO_LIST_H).auto_shrink([false, true]).show(ui, |ui| {
        for n in names {
            let shown = if ["ByLayer", "ByBlock", "Default"].contains(&n.as_str()) { crate::i18n::t(n).to_string() } else { n.clone() };
            let on = n.eq_ignore_ascii_case(current);
            let Some(pv) = preview else {
                if ui.selectable_label(on, shown).clicked() {
                    picked = Some(n.clone());
                }
                continue;
            };
            // Preview | separator | name.
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
            let p = ui.painter();
            if on || resp.hovered() {
                p.rect_filled(r, 2.0, if on { t.accent.gamma_multiply(0.45) } else { t.control_hover });
            }
            let pr = Rect::from_min_size(pos2(r.left() + 4.0, r.top()), vec2(PREVIEW_W, r.height()));
            paint_preview(p, pr, pv, n, t.text);
            let sep = pr.right() + 5.0;
            p.vline(sep, (r.top() + 3.0)..=(r.bottom() - 3.0), Stroke::new(1.0, t.border));
            p.text(pos2(sep + 6.0, r.center().y), egui::Align2::LEFT_CENTER, shown, crate::theme::body(), t.text);
            if resp.clicked() {
                picked = Some(n.clone());
            }
        }
    });
    picked
}

/// The current layer list in a drop-down, scrolling when there are many layers.
fn layer_popup(ui: &mut egui::Ui, salt: &str, cur: &str, rows: &[crate::palettes::LayerRow], action: &mut Option<Action>) -> bool {
    let mut set_current = None;
    let mut toggle = None;
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().id_salt(("cc_layer_list", salt)).max_height(COMBO_LIST_H).auto_shrink([false, true]).show(ui, |ui| {
        crate::palettes::layer_list(ui, rows, cur, &mut set_current, &mut toggle);
    });
    if let Some(name) = set_current {
        *action = Some(Action::SetLayer(name));
        return true;
    }
    if let Some((name, key, on)) = toggle {
        *action = Some(Action::LayerToggle(name, key, on));
    }
    false
}

/// Lay out and run `bar` in `area`: a row, or a column when `vertical`.
fn strip(app: &mut CadApp, ui: &mut egui::Ui, area: Rect, vertical: bool, bar: Bar) {
    let t = Tokens::get();
    let mac = ui.ctx().os().is_mac();
    let st = state(&app.ui, bar);
    let dock = st.dock;
    let shown_bars = Bar::ALL.map(|b| state(&app.ui, b).show);
    let mut items = sanitize(&st.items);
    let mut action: Option<Action> = None;
    let id = if bar == Bar::Standard { ui.id().with("cc_toolbar") } else { egui::Id::new(("cc_toolbar", bar.id())) };
    let layers = if items.iter().any(|i| i == LAYER_COMBO) { layer_rows(app) } else { None };

    // Main-axis start and end, cross-axis centre; `slot` is a rect at a main-axis position.
    let (start, end) = if vertical { (area.top() + 4.0, area.bottom() - 4.0) } else { (area.left(), area.right() - 12.0) };
    let cross = if vertical { area.center().x } else { area.center().y };
    let slot = |pos: f32, len: f32, thick: f32| {
        if vertical {
            Rect::from_min_size(pos2(cross - thick / 2.0, pos), vec2(thick, len))
        } else {
            Rect::from_min_size(pos2(pos, cross - thick / 2.0), vec2(len, thick))
        }
    };
    let main = |p: Pos2| if vertical { p.y } else { p.x };

    // The bar's empty space: right-click for its options (not a keyboard stop).
    let background = ui.interact(area, id.with("background"), Sense::CLICK);
    background.context_menu(|ui| options_menu(ui, bar, shown_bars, None, dock, &mut action));

    // The handle: drag it to the top or the left edge to dock the bar there.
    let grip = slot(start, GRIP, BUTTON);
    let grip_resp = ui.interact(grip, id.with("grip"), Sense::DRAG).on_hover_cursor(egui::CursorIcon::Grab);
    for i in 0..4 {
        for j in [-2.0, 2.0] {
            let along = grip.min
                + if vertical {
                    vec2(grip.width() / 2.0 - 7.5 + i as f32 * 5.0, grip.height() / 2.0 + j)
                } else {
                    vec2(grip.width() / 2.0 + j, grip.height() / 2.0 - 7.5 + i as f32 * 5.0)
                };
            ui.painter().circle_filled(along, 1.0, t.text_dim);
        }
    }
    let grip_resp = if grip_resp.dragged() {
        grip_resp
    } else {
        grip_resp.on_hover_ui(|ui| {
            ui.label(RichText::new(crate::i18n::t(bar.title())).strong());
            ui.label(crate::tl!("Drag to dock the Tool Bar at the top or on the left"));
        })
    };
    let screen = ui.ctx().content_rect();
    if grip_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(target) = ui.ctx().pointer_latest_pos().and_then(|p| drop_target(screen, p)) {
            let preview = match target {
                Dock::Top => Rect::from_min_size(screen.min, vec2(screen.width(), THICKNESS)),
                Dock::Left => Rect::from_min_size(screen.min, vec2(THICKNESS, screen.height())),
            };
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, id.with("dock_preview")));
            painter.rect_filled(preview, 2.0, t.accent.gamma_multiply(0.25));
            painter.rect_stroke(preview, 2.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
        }
    }
    if grip_resp.drag_stopped()
        && let Some(target) = ui.ctx().pointer_latest_pos().and_then(|p| drop_target(screen, p))
    {
        action = Some(Action::Dock(target));
    }

    // How many items fit; the rest go to the More menu at the end.
    let mut first = start + GRIP + 4.0;
    // The standard bar starts with the application button: always there, never customized.
    if bar == Bar::Standard {
        let br = slot(first, BUTTON, BUTTON);
        if app_button(ui, br, id.with("app_menu"), vertical) {
            action = Some(Action::AppMenu);
        }
        let at = first + BUTTON + GAP + (SEP - GAP) / 2.0;
        let half = BUTTON / 2.0 - 3.0;
        if vertical {
            ui.painter().hline((cross - half)..=(cross + half), at, Stroke::new(1.0, t.border));
        } else {
            ui.painter().vline(at, (cross - half)..=(cross + half), Stroke::new(1.0, t.border));
        }
        first += BUTTON + GAP + SEP;
    }
    let extent = |item: &String| {
        if item == SEPARATOR {
            SEP
        } else if is_combo(item) && !vertical {
            combo_width(item) + GAP
        } else if has_history(item) && !vertical {
            BUTTON + HISTORY_W + GAP
        } else {
            BUTTON + GAP
        }
    };
    let room = end - first;
    let total: f32 = items.iter().map(extent).sum();
    let mut shown = items.len();
    if total > room {
        let budget = room - (BUTTON + GAP);
        let mut used = 0.0;
        shown = 0;
        for item in &items {
            let w = extent(item);
            if used + w > budget {
                break;
            }
            used += w;
            shown += 1;
        }
    }

    let dragging = ui.ctx().dragged_id().is_some();
    let mut placed: Vec<(usize, Rect)> = Vec::new();
    let mut drag_from: Option<usize> = None;
    let mut drop_from: Option<usize> = None;
    let mut pos = first;
    for (index, item) in items.iter().enumerate().take(shown) {
        if item == SEPARATOR {
            let at = pos + (SEP - GAP) / 2.0;
            let half = BUTTON / 2.0 - 3.0;
            if vertical {
                ui.painter().hline((cross - half)..=(cross + half), at, Stroke::new(1.0, t.border));
            } else {
                ui.painter().vline(at, (cross - half)..=(cross + half), Stroke::new(1.0, t.border));
            }
            pos += SEP;
            continue;
        }
        let Some(tool) = find(item) else { continue };
        let combo = is_combo(tool.command);
        let names = if combo && tool.command != LAYER_COMBO { combo_names(app, tool.command) } else { None };
        let len = if combo && !vertical { combo_width(tool.command) } else { BUTTON };
        let br = slot(pos, len, if combo && !vertical { 24.0 } else { BUTTON });
        let resp = ui.interact(br, id.with(("tb", tool.command)), Sense::click() | Sense::DRAG);
        if tool.command == LAYER_COMBO {
            paint_combo(ui, br, resp.hovered() || resp.dragged(), vertical, layers.as_ref());
        } else if combo {
            paint_name_combo(ui, br, resp.hovered() || resp.dragged(), vertical, tool.icon, names.as_ref().map(|(c, _)| c.as_str()));
        } else {
            if resp.hovered() || resp.dragged() {
                ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
            }
            icons::paint(ui.painter(), br.shrink(4.0), tool.icon, !tool.enabled());
        }
        icons::describe_control(ui, &resp, &tool.accessible_label(mac), None, false);
        let resp = if dragging {
            resp
        } else if tool.command == LAYER_COMBO {
            let current = layers.as_ref().map(|(cur, _)| cur.clone()).unwrap_or_default();
            resp.on_hover_ui(|ui| {
                ui.label(RichText::new(crate::i18n::t(tool.label)).strong());
                ui.label(RichText::new(format!("{} {current}", crate::tl!("Current layer:"))).weak());
            })
        } else if combo {
            let current = names.as_ref().map(|(c, _)| c.clone()).unwrap_or_default();
            resp.on_hover_ui(|ui| {
                ui.label(RichText::new(crate::i18n::t(tool.label)).strong());
                ui.label(RichText::new(current).weak());
            })
        } else {
            resp.on_hover_ui(|ui| tooltip(ui, &tool, mac))
        };
        resp.context_menu(|ui| options_menu(ui, bar, shown_bars, Some(index), dock, &mut action));
        let align = if vertical { egui::RectAlign::RIGHT_START } else { egui::RectAlign::BOTTOM_START };
        if tool.command == LAYER_COMBO {
            if let Some((cur, rows)) = &layers {
                egui::Popup::from_toggle_button_response(&resp)
                    .align(align)
                    .width(COMBO_W.max(240.0))
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if layer_popup(ui, "bar", cur, rows, &mut action) {
                            ui.close();
                        }
                    });
            }
        } else if combo {
            if let Some((cur, list)) = &names {
                let command = tool.command;
                let preview = combo_preview(app, command);
                egui::Popup::from_toggle_button_response(&resp)
                    .align(align)
                    .width(STYLE_COMBO_W.max(200.0))
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if let Some(n) = name_list(ui, command, cur, list, preview.as_ref()) {
                            action = Some(Action::SetCurrent(command, n));
                            ui.close();
                        }
                    });
            }
        } else if resp.clicked() && tool.enabled() {
            action = Some(Action::Run(tool.command));
        }
        if resp.dragged() {
            drag_from = Some(index);
        }
        if resp.drag_stopped() {
            drop_from = Some(index);
        }
        // Undo and Redo: a narrow arrow beside the button lists the steps to go back (or forward) to.
        let mut extra = 0.0;
        if has_history(tool.command) && !vertical {
            let hr = slot(pos + len, HISTORY_W, BUTTON);
            let hresp = ui.interact(hr, id.with(("tb_history", tool.command)), Sense::click());
            if hresp.hovered() {
                ui.painter().rect_filled(hr, 3.0, t.control_hover.gamma_multiply(0.6));
            }
            icons::paint(ui.painter(), Rect::from_center_size(hr.center(), vec2(10.0, 10.0)), Icon::ChevronDown, false);
            let undo = tool.command == "u";
            let hresp = hresp.on_hover_text(if undo { crate::tl!("Undo several steps") } else { crate::tl!("Redo several steps") });
            egui::Popup::menu(&hresp).align(egui::RectAlign::BOTTOM_START).show(|ui| {
                if let Some(k) = history_list(app, ui, undo) {
                    action = Some(Action::History(undo, k));
                    ui.close();
                }
            });
            extra = HISTORY_W;
        }
        placed.push((index, br));
        pos += len + extra + GAP;
    }

    // Dragging a button: show where it lands; dropping it on the bar moves it there.
    let insertion = |p: Pos2| -> Option<(usize, f32)> {
        if !area.expand(24.0).contains(p) {
            return None;
        }
        let along = main(p);
        match placed.iter().find(|(_, r)| main(r.center()) > along) {
            Some((index, r)) => Some((*index, main(r.min) - GAP / 2.0)),
            None => placed.last().map(|(index, r)| (index + 1, main(r.max) + GAP / 2.0)),
        }
    };
    if let Some(from) = drag_from {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let pointer = ui.ctx().pointer_latest_pos();
        if let Some((_, at)) = pointer.and_then(insertion) {
            let half = BUTTON / 2.0 + 2.0;
            let stroke = Stroke::new(2.0, t.accent);
            if vertical {
                ui.painter().hline((cross - half)..=(cross + half), at, stroke);
            } else {
                ui.painter().vline(at, (cross - half)..=(cross + half), stroke);
            }
        }
        if let (Some(p), Some(tool)) = (pointer, items.get(from).and_then(|c| find(c))) {
            let ghost = Rect::from_center_size(p + vec2(10.0, 10.0), vec2(BUTTON, BUTTON));
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, id.with("ghost")));
            painter.rect_filled(ghost, 3.0, t.control_hover);
            painter.rect_stroke(ghost, 3.0, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
            icons::paint(&painter, ghost.shrink(4.0), tool.icon, false);
        }
    }
    if let Some(from) = drop_from
        && let Some((slot_index, _)) = ui.ctx().pointer_latest_pos().and_then(insertion)
        && slot_index != from
        && slot_index != from + 1
    {
        action = Some(Action::Move { from, slot: slot_index });
    }

    // The items that did not fit.
    if shown < items.len() {
        let br = slot(pos, BUTTON, BUTTON);
        let response = ui.interact(br, id.with("toolbar_more"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(br, 3.0, t.control_hover.gamma_multiply(0.6));
        }
        for offset in [-5.0, 0.0, 5.0] {
            let d = if vertical { vec2(0.0, offset) } else { vec2(offset, 0.0) };
            ui.painter().circle_filled(br.center() + d, 1.5, t.text);
        }
        icons::describe_control(ui, &response, bar.more_label(), None, false);
        let align = if vertical { egui::RectAlign::RIGHT_START } else { egui::RectAlign::BOTTOM_START };
        egui::Popup::menu(&response).align(align).show(|ui| {
            for item in items.iter().skip(shown) {
                if item == SEPARATOR {
                    ui.separator();
                    continue;
                }
                let Some(tool) = find(item) else { continue };
                if tool.command == LAYER_COMBO {
                    // The drop-down does not fit a menu row: its layers are listed in place.
                    if let Some((cur, rows)) = &layers
                        && layer_popup(ui, "more", cur, rows, &mut action)
                    {
                        ui.close();
                    }
                    continue;
                }
                if is_combo(tool.command) {
                    if let Some((cur, list)) = combo_names(app, tool.command) {
                        ui.label(RichText::new(crate::i18n::t(tool.label)).weak());
                        let preview = combo_preview(app, tool.command);
                        if let Some(n) = name_list(ui, tool.command, &cur, &list, preview.as_ref()) {
                            action = Some(Action::SetCurrent(tool.command, n));
                            ui.close();
                        }
                        ui.separator();
                    }
                    continue;
                }
                let response = ui.add_enabled(tool.enabled(), egui::Button::new(crate::i18n::t(tool.label)));
                crate::control::record_widget(ui, &response, tool.label, "button", None);
                let response = response.on_hover_ui(|ui| tooltip(ui, &tool, mac));
                if response.clicked() {
                    action = Some(Action::Run(tool.command));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(crate::tl!("Customize Tool Bar...")).clicked() {
                action = Some(Action::Customize);
                ui.close();
            }
        });
        response.on_hover_text(crate::i18n::t(bar.more_label()));
    }

    if matches!(action, Some(Action::AppMenu)) {
        let picked = ui.ctx().data_mut(|d| d.remove_temp::<&'static str>(id.with("app_menu").with("picked")));
        action = picked.map(Action::AppCommand);
    }
    match action {
        Some(Action::Run(command)) => run_tool(app, command),
        Some(Action::Move { from, slot }) => {
            move_item(&mut items, from, slot);
            *fields(&mut app.ui, bar).2 = sanitize(&items);
        }
        Some(Action::Remove(index)) => {
            if index < items.len() {
                items.remove(index);
            }
            *fields(&mut app.ui, bar).2 = sanitize(&items);
        }
        Some(Action::Dock(target)) => *fields(&mut app.ui, bar).1 = target,
        Some(Action::Customize) => app.ui.dialog = Some(format!("toolbar:{}", bar.id())),
        Some(Action::Show(other, on)) => *fields(&mut app.ui, other).0 = on,
        Some(Action::AppMenu) => {}
        Some(Action::AppCommand(command)) => crate::menus::activate(app, command),
        Some(Action::SetLayer(name)) => {
            let _ = app.run("layer.current", json!({ "name": name }));
        }
        Some(Action::LayerToggle(name, key, on)) => {
            let _ = app.run("layer.set", json!({ "name": name, key: on }));
        }
        Some(Action::History(undo, steps)) => {
            for _ in 0..steps.min(HISTORY_MAX) {
                if let Err(e) = app.run(if undo { "u" } else { "redo" }, json!({})) {
                    app.session.echo(e);
                    break;
                }
            }
        }
        Some(Action::SetCurrent(combo, name)) => {
            let r = match combo {
                TEXTSTYLE_COMBO => app.run("style.current", json!({ "name": name })),
                DIMSTYLE_COMBO => app.run("dimstyle.current", json!({ "name": name })),
                LWEIGHT_COMBO => {
                    let v = match name.trim_end_matches(" mm").trim().parse::<f64>() {
                        Ok(mm) => json!(mm),
                        Err(_) => json!(name),
                    };
                    app.run("lweight", json!({ "lineweight": v }))
                }
                _ => app.run("linetype", json!({ "current": name })),
            };
            if let Err(e) = r {
                app.session.echo(e);
            }
        }
        None => {}
    }
}

/// The application menu: the file commands and Options, under the CADCraft button.
pub const APP_MENU: &[(&str, &str)] = &[
    ("new", "New"),
    ("open", "Open..."),
    ("qsave", "Save"),
    ("saveas", "Save As..."),
    ("|", ""),
    ("import", "Import..."),
    ("export", "Export..."),
    ("|", ""),
    ("plot", "Print..."),
    ("publish", "Batch Print..."),
    ("|", ""),
    ("close", "Close"),
    ("options", "Options..."),
];

/// The CADCraft application icon, decoded once.
fn app_icon(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    const PNG: &[u8] = include_bytes!("../../../assets/app-icon/cadcraft-64.png");
    let id = egui::Id::new("cc_app_icon");
    if let Some(t) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return t;
    }
    let tex = image::load_from_memory(PNG).ok().map(|img| {
        let img = img.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        ctx.load_texture("cc_app_icon", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

/// The application button with its menu. True when a menu command was chosen this frame (the
/// command is remembered until the bar's action runs).
fn app_button(ui: &mut egui::Ui, rect: Rect, id: egui::Id, vertical: bool) -> bool {
    let t = Tokens::get();
    let resp = ui.interact(rect, id, Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    if resp.hovered() || open {
        ui.painter().rect_filled(rect, 3.0, t.control_hover.gamma_multiply(0.8));
    }
    match app_icon(ui.ctx()) {
        Some(tex) => {
            ui.painter().image(tex.id(), rect.shrink(2.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => icons::paint(ui.painter(), rect.shrink(4.0), Icon::New, false),
    }
    icons::describe_control(ui, &resp, "CADCraft", None, false);
    let resp = resp.on_hover_text(crate::tl!("Application menu"));
    let align = if vertical { egui::RectAlign::RIGHT_START } else { egui::RectAlign::BOTTOM_START };
    let mut picked = None;
    egui::Popup::menu(&resp).align(align).show(|ui| {
        ui.set_min_width(260.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for (command, label) in APP_MENU {
            if *command == SEPARATOR {
                ui.separator();
                continue;
            }
            let r = app_menu_row(ui, command, label);
            crate::control::record_widget(ui, &r, label, "button", None);
            if r.clicked() {
                picked = Some(*command);
                ui.close();
            }
        }
    });
    if let Some(c) = picked {
        ui.ctx().data_mut(|d| d.insert_temp(id.with("picked"), c));
        return true;
    }
    false
}

/// The icon beside an application-menu command.
pub fn app_menu_icon(command: &str) -> Icon {
    match command {
        "new" => Icon::New,
        "open" => Icon::Open,
        "qsave" => Icon::Save,
        "saveas" => Icon::SaveAs,
        "import" => Icon::Import,
        "export" => Icon::Export,
        "plot" => Icon::Plot,
        "publish" => Icon::Publish,
        "close" => Icon::Close,
        _ => Icon::Gear,
    }
}

/// One row of the application menu: a large icon, the label in a larger font and the shortcut on
/// the right, 36 points tall.
fn app_menu_row(ui: &mut egui::Ui, command: &str, label: &str) -> egui::Response {
    let t = Tokens::get();
    let width = ui.available_width().max(260.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let text = crate::i18n::t(label);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text.to_string()));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, t.control_hover);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(28.0, 28.0));
    icons::paint(ui.painter(), icon, app_menu_icon(command), false);
    let font = egui::FontId::proportional(crate::theme::body().size + 3.0);
    let text_color = if resp.hovered() { ui.visuals().widgets.hovered.fg_stroke.color } else { t.text };
    ui.painter().text(pos2(rect.left() + 46.0, rect.center().y), egui::Align2::LEFT_CENTER, text, font, text_color);
    if let Some(sc) = cadcraft_engine::find_command(command).and_then(|c| c.shortcut) {
        let sc = crate::menus::shortcut_label(sc, ui.ctx().os().is_mac());
        ui.painter().text(pos2(rect.right() - 8.0, rect.center().y), egui::Align2::RIGHT_CENTER, sc, crate::theme::body(), t.text_dim);
    }
    resp
}

/// Run a bar's tool: layer tools act at once (as in the Layers palette), the others start like a
/// typed command.
fn run_tool(app: &mut CadApp, command: &'static str) {
    let layer_tool = find(command).is_some_and(|t| t.category == LAYERS_CATEGORY) && !command.starts_with("ui.");
    if layer_tool {
        let _ = app.run(command, json!({}));
    } else {
        app.start(command);
    }
}

/// The current-layer drop-down: its state icons, colour and name in a row, a layers icon in a column.
fn paint_combo(ui: &egui::Ui, rect: Rect, hovered: bool, vertical: bool, layers: Option<&(String, Vec<crate::palettes::LayerRow>)>) {
    let t = Tokens::get();
    let p = ui.painter();
    if vertical {
        if hovered {
            p.rect_filled(rect, 3.0, t.control_hover.gamma_multiply(0.6));
        }
        icons::paint(p, rect.shrink(4.0), Icon::Layers, false);
        return;
    }
    p.rect_filled(rect, 3.0, if hovered { t.control_hover } else { t.control });
    let current = layers.and_then(|(cur, rows)| rows.iter().find(|r| r.0.eq_ignore_ascii_case(cur)));
    if let Some((name, on, frozen, locked, color)) = current {
        let mut x = rect.left() + 6.0;
        for icon in [
            if *on { Icon::Bulb } else { Icon::BulbOff },
            if *locked { Icon::Lock } else { Icon::Unlock },
            if *frozen { Icon::Snowflake } else { Icon::Sun },
        ] {
            icons::paint(p, Rect::from_min_size(pos2(x, rect.center().y - 8.0), vec2(16.0, 16.0)), icon, false);
            x += 18.0;
        }
        crate::palettes::swatch(p, Rect::from_min_size(pos2(x + 2.0, rect.center().y - 5.0), vec2(10.0, 10.0)), *color);
        let text_rect = Rect::from_min_max(pos2(x + 18.0, rect.top()), pos2(rect.right() - 22.0, rect.bottom()));
        p.with_clip_rect(text_rect).text(pos2(text_rect.left(), rect.center().y), egui::Align2::LEFT_CENTER, name, crate::theme::body(), t.text);
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)), Icon::ChevronDown, false);
}

/// A style or linetype drop-down: its icon and the current name in a row, the icon in a column.
fn paint_name_combo(ui: &egui::Ui, rect: Rect, hovered: bool, vertical: bool, icon: Icon, current: Option<&str>) {
    let t = Tokens::get();
    let p = ui.painter();
    if vertical {
        if hovered {
            p.rect_filled(rect, 3.0, t.control_hover.gamma_multiply(0.6));
        }
        icons::paint(p, rect.shrink(4.0), icon, false);
        return;
    }
    p.rect_filled(rect, 3.0, if hovered { t.control_hover } else { t.control });
    icons::paint(p, Rect::from_min_size(pos2(rect.left() + 4.0, rect.center().y - 8.0), vec2(16.0, 16.0)), icon, false);
    if let Some(name) = current {
        let shown = if ["ByLayer", "ByBlock"].contains(&name) { crate::i18n::t(name).to_string() } else { name.to_string() };
        let text_rect = Rect::from_min_max(pos2(rect.left() + 24.0, rect.top()), pos2(rect.right() - 22.0, rect.bottom()));
        p.with_clip_rect(text_rect).text(pos2(text_rect.left(), rect.center().y), egui::Align2::LEFT_CENTER, shown, crate::theme::body(), t.text);
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)), Icon::ChevronDown, false);
}

/// Selection and search of the Customize Tool Bar dialog.
#[derive(Clone, Default)]
struct DialogState {
    filter: String,
    available: Option<&'static str>,
    current: Option<usize>,
}

/// A list row: the tool's icon and its name.
fn row(ui: &mut egui::Ui, icon: Icon, name: &str, selected: bool, dim: bool) -> egui::Response {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        icons::paint(ui.painter(), r, icon, dim);
        ui.selectable_label(selected, name)
    })
    .inner
}

/// Window ▸ Customize Tool Bar...: where a bar is docked and which buttons it holds, in order.
/// `which` is the dialog id, `toolbar:<bar>` (`toolbar` alone: the standard bar).
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, which: &str, open: &mut bool) {
    let bar = which.strip_prefix("toolbar:").and_then(Bar::parse).unwrap_or(Bar::Standard);
    let id = egui::Id::new("Customize Tool Bar");
    let state_id = id.with(("state", bar.id()));
    let mut st: DialogState = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let bar_state = state(&app.ui, bar);
    let mut items = sanitize(&bar_state.items);
    let mut dock = bar_state.dock;
    let mut show = bar_state.show;
    let mut changed = false;
    let mut switch_to = None;
    egui::Window::new(crate::tl!("Customize Tool Bar")).id(id).open(open).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(crate::tl!("Tool Bar:"));
            for other in Bar::ALL {
                if ui.selectable_label(other == bar, crate::i18n::t(other.title())).clicked() && other != bar {
                    switch_to = Some(other);
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut show, crate::tl!("Show Tool Bar"));
            ui.add_space(16.0);
            ui.label(crate::tl!("Position:"));
            ui.radio_value(&mut dock, Dock::Top, crate::tl!("Top (row)"));
            ui.radio_value(&mut dock, Dock::Left, crate::tl!("Left (column)"));
        });
        ui.separator();
        let mut add: Option<&'static str> = None;
        let mut add_separator = false;
        let mut shift: Option<bool> = None;
        let mut remove = false;
        let mut reset = false;
        ui.horizontal_top(|ui| {
            // Every command the bar can hold, by category.
            ui.vertical(|ui| {
                ui.set_width(250.0);
                ui.label(RichText::new(crate::tl!("Commands")).strong());
                ui.horizontal(|ui| {
                    ui.label(crate::tl!("Search:"));
                    ui.add(egui::TextEdit::singleline(&mut st.filter).desired_width(170.0));
                });
                let mac = ui.ctx().os().is_mac();
                egui::ScrollArea::vertical().id_salt("tb_available").max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
                    let needle = st.filter.trim().to_lowercase();
                    let mut category = "";
                    for tool in catalog() {
                        let name = crate::i18n::t(tool.label);
                        if !needle.is_empty() && !name.to_lowercase().contains(&needle) && !tool.command.contains(needle.as_str()) {
                            continue;
                        }
                        if tool.category != category {
                            category = tool.category;
                            ui.add_space(4.0);
                            ui.label(RichText::new(crate::i18n::t(category)).weak());
                        }
                        let on_bar = items.iter().any(|i| i == tool.command);
                        let resp = row(ui, tool.icon, name, st.available == Some(tool.command), on_bar);
                        let resp = resp.on_hover_ui(|ui| tooltip(ui, tool, mac));
                        if resp.clicked() {
                            st.available = Some(tool.command);
                        }
                        if resp.double_clicked() && !on_bar {
                            add = Some(tool.command);
                        }
                    }
                });
            });
            // Add a command or a separator to the bar.
            ui.vertical(|ui| {
                ui.add_space(80.0);
                let can_add = st.available.is_some_and(|c| !items.iter().any(|i| i == c));
                if ui.add_enabled(can_add, egui::Button::new(format!("{} →", crate::tl!("Add")))).clicked() {
                    add = st.available;
                }
                ui.add_space(6.0);
                if ui.button(crate::tl!("Add Separator")).clicked() {
                    add_separator = true;
                }
            });
            // The bar's buttons in order.
            ui.vertical(|ui| {
                ui.set_width(250.0);
                ui.label(RichText::new(crate::tl!("Tool Bar buttons")).strong());
                egui::ScrollArea::vertical().id_salt("tb_current").max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (index, item) in items.iter().enumerate() {
                        let selected = st.current == Some(index);
                        let resp = if item == SEPARATOR {
                            ui.selectable_label(selected, RichText::new(format!("──── {} ────", crate::tl!("Separator"))).weak())
                        } else if let Some(tool) = find(item) {
                            row(ui, tool.icon, crate::i18n::t(tool.label), selected, false)
                        } else {
                            continue;
                        };
                        if resp.clicked() {
                            st.current = Some(index);
                        }
                    }
                });
                let picked = st.current.filter(|i| *i < items.len());
                ui.horizontal(|ui| {
                    if ui.add_enabled(picked.is_some_and(|i| i > 0), egui::Button::new(crate::tl!("Move Up"))).clicked() {
                        shift = Some(true);
                    }
                    if ui.add_enabled(picked.is_some_and(|i| i + 1 < items.len()), egui::Button::new(crate::tl!("Move Down"))).clicked() {
                        shift = Some(false);
                    }
                    if ui.add_enabled(picked.is_some(), egui::Button::new(crate::tl!("Remove"))).clicked() {
                        remove = true;
                    }
                });
                if ui.button(crate::tl!("Reset to Default")).clicked() {
                    reset = true;
                }
            });
        });
        ui.separator();
        ui.label(RichText::new(crate::tl!("Drag buttons along the Tool Bar to reorder them, or its handle to dock it.")).weak());

        let picked = st.current.filter(|i| *i < items.len());
        if let Some(command) = add {
            let at = picked.map(|i| i + 1);
            if add_item(&mut items, command, at) {
                st.current = Some(at.unwrap_or(items.len().saturating_sub(1)));
                changed = true;
            }
        }
        if add_separator && items.len() < MAX_ITEMS {
            let at = picked.map(|i| i + 1).unwrap_or(items.len()).min(items.len());
            items.insert(at, SEPARATOR.to_string());
            st.current = Some(at);
            changed = true;
        }
        if let (Some(up), Some(i)) = (shift, picked) {
            if up && i > 0 {
                items.swap(i, i - 1);
                st.current = Some(i - 1);
                changed = true;
            } else if !up && i + 1 < items.len() {
                items.swap(i, i + 1);
                st.current = Some(i + 1);
                changed = true;
            }
        }
        if remove && let Some(i) = picked {
            items.remove(i);
            st.current = if items.is_empty() { None } else { Some(i.min(items.len() - 1)) };
            changed = true;
        }
        if reset {
            items = bar.default_items();
            st.current = None;
            changed = true;
        }
    });
    ctx.data_mut(|d| d.insert_temp(state_id, st));
    let (shown, docked, saved) = fields(&mut app.ui, bar);
    if changed {
        // Separators may sit side by side while editing in the list; the bar shows them once.
        *saved = items.into_iter().take(MAX_ITEMS).collect();
    }
    *docked = dock;
    *shown = show;
    if let Some(other) = switch_to {
        app.ui.dialog = Some(format!("toolbar:{}", other.id()));
    }
}

/// `ui.toolbar.*` commands (menus, command line, control channel). `None` if `id` isn't one.
/// Every command takes `"bar"`: `"standard"` (the default), `"toolSets"` or `"layers"`.
pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    let bar = match params.get("bar").and_then(Value::as_str) {
        None => Bar::Standard,
        Some(name) => match Bar::parse(name) {
            Some(bar) => bar,
            None if id.starts_with("ui.toolbar.") => {
                return Some(Err(format!("unknown tool bar \"{}\" (standard, toolSets or layers)", name.chars().take(40).collect::<String>())));
            }
            None => Bar::Standard,
        },
    };
    let report = |app: &CadApp, bar: Bar| {
        let st = state(&app.ui, bar);
        json!({ "bar": bar.id(), "show": st.show, "position": st.dock.as_str(), "items": sanitize(&st.items) })
    };
    Some(match id {
        "ui.toolbar.top" | "ui.toolbar.left" => {
            let (shown, dock, _) = fields(&mut app.ui, bar);
            *dock = if id == "ui.toolbar.top" { Dock::Top } else { Dock::Left };
            *shown = true;
            Ok(report(&*app, bar))
        }
        "ui.toggle.toolbar.toolsets" | "ui.toggle.toolbar.layers" => {
            let bar = if id.ends_with("toolsets") { Bar::ToolSets } else { Bar::Layers };
            let shown = fields(&mut app.ui, bar).0;
            *shown = params.get("on").and_then(Value::as_bool).unwrap_or(!*shown);
            Ok(report(&*app, bar))
        }
        // `{"position": "top"|"left"}` docks the bar; without it, reports the bar.
        "ui.toolbar.dock" => match params.get("position").and_then(Value::as_str) {
            Some(name) => match Dock::parse(name) {
                Some(dock) => {
                    *fields(&mut app.ui, bar).1 = dock;
                    Ok(report(&*app, bar))
                }
                None => Err(format!("unknown Tool Bar position \"{}\" (top or left)", name.chars().take(40).collect::<String>())),
            },
            None => Ok(report(&*app, bar)),
        },
        // `{"command": "line", "index": 3}` adds a Tool Sets, layer or standard command.
        "ui.toolbar.add" => {
            let Some(command) = params.get("command").and_then(Value::as_str) else { return Some(Err("ui.toolbar.add needs \"command\"".into())) };
            let index = params.get("index").and_then(Value::as_u64).map(|i| usize::try_from(i).unwrap_or(usize::MAX));
            let mut items = sanitize(&state(&app.ui, bar).items);
            if command == SEPARATOR {
                let at = index.unwrap_or(items.len()).min(items.len());
                items.insert(at, SEPARATOR.to_string());
            } else if !add_item(&mut items, command, index) {
                return Some(Err(format!("\"{}\" cannot be added to the Tool Bar", command.chars().take(40).collect::<String>())));
            }
            *fields(&mut app.ui, bar).2 = sanitize(&items);
            Ok(report(&*app, bar))
        }
        // `{"command": "line"}` or `{"index": 3}` removes a button.
        "ui.toolbar.remove" => {
            let mut items = sanitize(&state(&app.ui, bar).items);
            let at = match (params.get("command").and_then(Value::as_str), params.get("index").and_then(Value::as_u64)) {
                (Some(command), _) => items.iter().position(|i| i == command),
                (None, Some(i)) => usize::try_from(i).ok().filter(|i| *i < items.len()),
                (None, None) => None,
            };
            let Some(at) = at else { return Some(Err("No such Tool Bar button".into())) };
            items.remove(at);
            *fields(&mut app.ui, bar).2 = sanitize(&items);
            Ok(report(&*app, bar))
        }
        // `{"from": 0, "to": 5}`: move a button before the one now at `to`.
        "ui.toolbar.move" => {
            let mut items = sanitize(&state(&app.ui, bar).items);
            let get = |key: &str| params.get(key).and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok());
            let (Some(from), Some(to)) = (get("from"), get("to")) else { return Some(Err("ui.toolbar.move needs \"from\" and \"to\"".into())) };
            if from >= items.len() || to > items.len() {
                return Some(Err("Tool Bar position out of range".into()));
            }
            move_item(&mut items, from, to);
            *fields(&mut app.ui, bar).2 = sanitize(&items);
            Ok(report(&*app, bar))
        }
        "ui.toolbar.reset" => {
            *fields(&mut app.ui, bar).2 = bar.default_items();
            Ok(report(&*app, bar))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_bar_is_the_standard_groups() {
        let items = default_items();
        assert_eq!(items.first().map(String::as_str), Some("new"));
        assert_eq!(items.last().map(String::as_str), Some("ui.dialog.about"));
        assert_eq!(items.iter().filter(|i| *i == SEPARATOR).count(), STANDARD.len() - 1);
        assert_eq!(sanitize(&items), items, "the default bar is already clean");
    }

    #[test]
    fn every_tool_has_a_description() {
        let missing: Vec<&str> = catalog().iter().map(|t| t.command).filter(|c| description(c).is_none()).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn sanitize_drops_unknown_repeated_and_stray_separators() {
        let items = strings(&["|", "line", "nonsense", "line", "|", "|", "circle", "|"]);
        assert_eq!(sanitize(&items), strings(&["line", "|", "circle"]));
        let many: Vec<String> = std::iter::repeat_n("line".to_string(), 10_000).collect();
        assert_eq!(sanitize(&many), strings(&["line"]));
    }

    #[test]
    fn move_item_lands_before_the_slot() {
        let mut items = strings(&["a", "b", "c", "d"]);
        move_item(&mut items, 0, 3);
        assert_eq!(items, strings(&["b", "c", "a", "d"]));
        move_item(&mut items, 3, 0);
        assert_eq!(items, strings(&["d", "b", "c", "a"]));
        move_item(&mut items, 1, 4);
        assert_eq!(items, strings(&["d", "c", "a", "b"]));
        move_item(&mut items, 9, 0);
        move_item(&mut items, 0, 9);
        assert_eq!(items, strings(&["d", "c", "a", "b"]), "out of range does nothing");
    }

    #[test]
    fn add_item_takes_catalog_commands_once() {
        let mut items = strings(&["new"]);
        assert!(add_item(&mut items, "line", Some(0)));
        assert!(!add_item(&mut items, "line", None), "already on the bar");
        assert!(!add_item(&mut items, "rm -rf", None), "not a tool");
        assert_eq!(items, strings(&["line", "new"]));
    }

    #[test]
    fn every_drafting_tool_is_in_the_catalog() {
        for groups in [crate::palettes::DRAFTING, crate::palettes::MODELING] {
            for (_, large, small) in groups.iter() {
                for (_, command, _) in large.iter().chain(small.iter()) {
                    assert!(find(command).is_some(), "{command}");
                }
            }
        }
    }

    #[test]
    fn every_bar_default_is_clean() {
        for bar in Bar::ALL {
            let items = bar.default_items();
            assert_eq!(sanitize(&items), items, "{bar:?}");
            assert_eq!(Bar::parse(bar.id()), Some(bar));
        }
        assert!(Bar::Layers.default_items().iter().any(|i| i == LAYER_COMBO));
        assert!(!Bar::ToolSets.default_items().iter().any(|i| i == "revcloud"), "keeps palette-only names unique for tests");
    }

    #[test]
    fn dock_names() {
        assert_eq!(Dock::parse("Left"), Some(Dock::Left));
        assert_eq!(Dock::parse("column"), Some(Dock::Left));
        assert_eq!(Dock::parse("top"), Some(Dock::Top));
        assert_eq!(Dock::parse("bottom"), None);
        assert_eq!(serde_json::to_value(Dock::Left).ok(), Some(json!("left")));
    }

    #[test]
    fn dropping_the_handle_docks_at_the_nearer_edge() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        assert_eq!(drop_target(screen, pos2(20.0, 400.0)), Some(Dock::Left));
        assert_eq!(drop_target(screen, pos2(600.0, 20.0)), Some(Dock::Top));
        assert_eq!(drop_target(screen, pos2(600.0, 400.0)), None);
    }
}
