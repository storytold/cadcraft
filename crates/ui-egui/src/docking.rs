//! Application panel identities and persistence around the shared docking renderer.
//! Document contents remain owned by their existing view implementations.

use craft_ui::docking::{
    Action, Column, DockArea, DockCommand, DockContent, DockHeights, DockOperation, DockStyle, FloatSpace, Group, Layout, Location, Node,
    PanelLimits, Permissions, Placement, Side, Sides, Target, UiDock, Zone,
};
use craft_ui::layout::{SplitAxis, SplitSize};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Panel {
    Canvas,
    ToolSets,
    QuickCalc,
    Layers,
    Properties,
    #[serde(rename = "externalReferences")]
    ExternalRefs,
}

impl Panel {
    pub const ALL: &[Self] = &[Self::Canvas, Self::ToolSets, Self::Layers, Self::Properties, Self::ExternalRefs];
    pub fn id(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::ToolSets => "toolSets",
            Self::QuickCalc => "quickCalc",
            Self::Layers => "layers",
            Self::Properties => "properties",
            Self::ExternalRefs => "externalReferences",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Canvas => "Drawing",
            Self::ToolSets => "Tool Sets",
            Self::QuickCalc => "QuickCalc",
            Self::Layers => "Layers",
            Self::Properties => "Properties",
            Self::ExternalRefs => "External References",
        }
    }
    /// The icon a palette shows in a column collapsed to icons.
    pub fn icon(self) -> Option<crate::icons::Icon> {
        use crate::icons::Icon;
        match self {
            Self::Canvas => None,
            Self::ToolSets => Some(Icon::Switcher),
            Self::QuickCalc => Some(Icon::Table),
            Self::Layers => Some(Icon::Layers),
            Self::Properties => Some(Icon::Properties),
            Self::ExternalRefs => Some(Icon::Attach),
        }
    }
    /// Palettes outside the default arrangement, opened on request (QUICKCALC).
    pub const ON_DEMAND: &[Self] = &[Self::QuickCalc];
}

/// The drawing view never moves, floats, closes or takes tabs; palettes dock beside it.
pub fn permissions(panel: &Panel) -> Permissions {
    if *panel == Panel::Canvas { Permissions { accepts_splits: true, ..Permissions::PROTECTED } } else { Permissions::default() }
}

fn tabs(panel: Panel) -> Node<Panel> {
    Node::Tabs { panels: vec![panel], active: 0 }
}

fn split(axis: SplitAxis, size: SplitSize, first: Node<Panel>, second: Node<Panel>) -> Node<Panel> {
    Node::Split { axis, size, first: Box::new(first), second: Box::new(second) }
}

/// The saved panel arrangement: craft-ui's workspace over CADCraft's panels.
pub type Workspace = craft_ui::docking::Workspace<Panel>;

/// The panel layout's shape version, saved beside it. Layouts saved without it, or with an
/// older one, are the split frames from before the dock columns and give way to the default.
pub const LAYOUT_VERSION: u32 = 2;

/// Width of the left dock's Tool Sets column.
pub const TOOL_SETS_WIDTH: f32 = 220.0;
/// Width of the right dock's palettes column.
pub const PALETTES_WIDTH: f32 = 300.0;
/// Layers' body height above Properties in a dock column.
const LAYERS_HEIGHT: f32 = 280.0;

fn dock(side: Side, width: f32, groups: &[Panel]) -> Node<Panel> {
    let groups = groups.iter().map(|panel| Group::new(vec![*panel], 0)).collect();
    Node::Dock { side, columns: vec![Column::new(groups, width)] }
}

/// Tool Sets in the left dock, the drawing, and Layers above Properties in the right dock.
pub fn default_workspace() -> Workspace {
    Workspace::new(Layout {
        root: Some(split(
            SplitAxis::Horizontal,
            SplitSize::FixedFirst(TOOL_SETS_WIDTH),
            dock(Side::Start, TOOL_SETS_WIDTH, &[Panel::ToolSets]),
            split(
                SplitAxis::Horizontal,
                SplitSize::FixedSecond(PALETTES_WIDTH),
                tabs(Panel::Canvas),
                dock(Side::End, PALETTES_WIDTH, &[Panel::Layers, Panel::Properties]),
            ),
        )),
        floating: Vec::new(),
    })
}

/// The dock a panel belongs in: Tool Sets on the left, the palettes on the right.
fn home_side(panel: Panel) -> (Side, f32) {
    if panel == Panel::ToolSets { (Side::Start, TOOL_SETS_WIDTH) } else { (Side::End, PALETTES_WIDTH) }
}

/// The panels of every dock group on `side`: columns from the window edge inwards, groups from
/// top to bottom.
fn dock_groups(layout: &Layout<Panel>, side: Side) -> Vec<Vec<Vec<Panel>>> {
    let mut nodes: Vec<&Node<Panel>> = layout.root.iter().collect();
    let mut seen = 0usize;
    while let Some(node) = nodes.pop() {
        seen = seen.saturating_add(1);
        if seen > 4 * craft_ui::docking::MAX_PANELS {
            break;
        }
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Dock { side: s, columns } if *s == side => {
                return columns.iter().map(|column| column.groups.iter().map(|group| group.panels.clone()).collect()).collect();
            }
            _ => {}
        }
    }
    Vec::new()
}

fn docked(layout: &Layout<Panel>, panel: Panel) -> bool {
    [Side::Start, Side::End].into_iter().any(|side| dock_groups(layout, side).iter().flatten().flatten().any(|p| *p == panel))
}

/// The docked neighbour a panel with no recorded place opens beside, as a new group above
/// (`false`) or below (`true`) it: Layers above Properties, External References and QuickCalc
/// below them, Tool Sets at the top of the left dock.
fn home_anchor(layout: &Layout<Panel>, panel: Panel) -> Option<(Panel, bool)> {
    let neighbours: &[(Panel, bool)] = match panel {
        Panel::Canvas | Panel::ToolSets => &[],
        Panel::Layers => &[(Panel::Properties, false), (Panel::ExternalRefs, false)],
        Panel::Properties => &[(Panel::Layers, true), (Panel::ExternalRefs, false)],
        Panel::ExternalRefs => &[(Panel::Properties, true), (Panel::Layers, true)],
        Panel::QuickCalc => &[(Panel::ExternalRefs, true), (Panel::Properties, true), (Panel::Layers, true)],
    };
    if let Some(found) = neighbours.iter().copied().find(|(neighbour, _)| docked(layout, *neighbour)) {
        return Some(found);
    }
    let (side, _) = home_side(panel);
    let columns = dock_groups(layout, side);
    let column = columns.first()?;
    if panel == Panel::ToolSets {
        column.first().and_then(|group| group.first()).map(|anchor| (*anchor, false))
    } else {
        column.last().and_then(|group| group.first()).map(|anchor| (*anchor, true))
    }
}

/// The actions that open a panel with no recorded place in its dock: a new group beside its
/// docked neighbour, else a new dock on its side at its default width.
fn home(layout: &Layout<Panel>, panel: Panel) -> Vec<Action<Panel>> {
    if let Some((anchor, below)) = home_anchor(layout, panel) {
        return vec![Action::OpenAt { panel, anchor, placement: Placement::Group { below, height: None } }];
    }
    let (side, width) = home_side(panel);
    vec![Action::Open { panel, anchor: None }, Action::MovePanel { panel, to: Target::NewDock { side } }, Action::ResizeColumn { panel, width }]
}

/// craft-ui's checks plus the drawing view's: a saved or hand-edited file cannot hide, float,
/// group, collapse or dock it, or restore a palette over it.
pub fn validate(workspace: &Workspace) -> Result<(), String> {
    workspace.validate().map_err(|error| error.to_string())?;
    if !workspace.layout.contains(&Panel::Canvas) {
        return Err("The panel layout must contain its document view".into());
    }
    if workspace.layout.floating.iter().any(|window| window.contains(&Panel::Canvas)) {
        return Err("The document view cannot float".into());
    }
    let mut nodes: Vec<_> = workspace.layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => {
                nodes.push(first);
                nodes.push(second);
            }
            Node::Tabs { panels, .. } if panels.contains(&Panel::Canvas) && panels.len() != 1 => {
                return Err("The document view cannot share a tab group".into());
            }
            Node::Stack { entries } if entries.iter().any(|entry| entry.panel == Panel::Canvas) => {
                return Err("The document view cannot be collapsed".into());
            }
            Node::Dock { columns, .. } if columns.iter().flat_map(|column| &column.groups).any(|group| group.panels.contains(&Panel::Canvas)) => {
                return Err("The document view cannot be in a dock".into());
            }
            _ => {}
        }
    }
    let over_canvas = |location: &Location<Panel>| {
        location.anchor == Some(Panel::Canvas) && matches!(location.placement, Placement::Tab { .. } | Placement::Split(Zone::Center))
    };
    if workspace.hidden.iter().any(|(_, location)| over_canvas(location)) {
        return Err("A hidden panel cannot restore over the document view".into());
    }
    Ok(())
}

/// Show a panel and select it at its recorded place, keeping the drawing view's rules. A panel
/// with no recorded place opens in its dock ([`home`]); one whose recorded place is gone joins
/// its docked neighbour's group.
pub fn reveal_panel(workspace: &mut Workspace, panel: Panel) -> Result<(), String> {
    let mut next = workspace.clone();
    if next.layout.contains(&panel) || next.hidden.iter().any(|(id, _)| *id == panel) {
        let fallback = home_anchor(&next.layout, panel).map(|(anchor, _)| anchor);
        next.set_visible(panel, true, fallback, permissions).map_err(|error| error.to_string())?;
    } else {
        next.apply_all(home(&next.layout, panel).into_iter().chain([Action::Activate { panel }]), permissions).map_err(|error| error.to_string())?;
    }
    validate(&next)?;
    *workspace = next;
    Ok(())
}

/// Show or hide a panel for the Tool Sets and palette switches. Showing a panel whose recorded
/// anchor is shown restores it there without changing which tab is selected or which accordion
/// entries are open. Otherwise it goes through [`reveal_panel`], which regroups it with hidden
/// panels it was beside, and selects it.
fn set_visible(workspace: &mut Workspace, panel: Panel, visible: bool) -> Result<(), String> {
    if workspace.layout.contains(&panel) == visible {
        return Ok(());
    }
    if !visible {
        return workspace.set_visible(panel, false, None, permissions).map_err(|error| error.to_string());
    }
    let mut next = workspace.clone();
    let location = next.hidden.iter().find(|(id, _)| *id == panel).map(|(_, location)| location.clone());
    // A direct restore against a hidden anchor would open a window of its own.
    let direct = location.filter(|location| location.anchor.is_none_or(|anchor| next.layout.contains(&anchor)));
    if direct.is_some_and(|location| next.layout.restore(panel, &location).is_ok()) {
        next.hidden.retain(|(id, _)| *id != panel);
        validate(&next)?;
        *workspace = next;
        return Ok(());
    }
    reveal_panel(workspace, panel)
}

/// `ui.dock` params: `{action}` or `{operation, …}` in craft-ui's shared grammar.
fn apply_command(workspace: &mut Workspace, params: &Value) -> Result<(), String> {
    validate(workspace)?;
    let operation = if let Some(action) = params.get("action") {
        UiDock::Action { action: serde_json::from_value(action.clone()).map_err(|error| error.to_string())? }
    } else {
        UiDock::Operation(serde_json::from_value(params.clone()).map_err(|error| error.to_string())?)
    };
    let mut next = workspace.clone();
    match operation {
        UiDock::Operation(DockOperation::Open { panel } | DockOperation::Activate { panel }) if !next.layout.contains(&panel) => {
            reveal_panel(&mut next, panel)?;
        }
        operation => {
            // Every ui.dock operation carries its own rect, so the float space is never consulted.
            let space = FloatSpace::new(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)));
            DockCommand::UiDock(operation).apply(&mut next, false, &space, permissions).map_err(|error| error.to_string())?;
        }
    }
    validate(&next)?;
    *workspace = next;
    Ok(())
}

fn sync(workspace: &mut Workspace, app: &crate::CadApp) -> Result<(), String> {
    set_visible(workspace, Panel::ToolSets, app.ui.show_toolsets)?;
    if !app.ui.show_palettes {
        set_visible(workspace, Panel::Layers, false)?;
        set_visible(workspace, Panel::Properties, false)?;
    } else if !workspace.layout.contains(&Panel::Layers) && !workspace.layout.contains(&Panel::Properties) {
        // Reopen the palette that closed last before the one recorded beside it.
        set_visible(workspace, Panel::Properties, true)?;
        set_visible(workspace, Panel::Layers, true)?;
    }
    Ok(())
}

fn publish_visibility(app: &mut crate::CadApp, workspace: &Workspace) {
    app.ui.show_toolsets = workspace.layout.contains(&Panel::ToolSets);
    app.ui.show_palettes = workspace.layout.contains(&Panel::Layers) || workspace.layout.contains(&Panel::Properties);
}

pub fn command(app: &mut crate::CadApp, reset: bool, params: &Value) -> Result<Value, String> {
    let mut workspace = app.ui.docking.clone();
    if reset {
        workspace = default_workspace();
    } else {
        sync(&mut workspace, app)?;
        apply_command(&mut workspace, params)?;
        // An opened panel's floating window comes to the front, in its own OS window too.
        if matches!(params.get("operation").and_then(Value::as_str), Some("open" | "activate")) {
            app.raise_panel = params.get("panel").and_then(|panel| serde_json::from_value(panel.clone()).ok());
        }
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    Ok(json!({"docking": app.ui.docking}))
}

/// Validate and repair the stored workspace, sync it with the app, and render the dock panels; a failed restore falls back to the default layout with a status message.
pub fn show(app: &mut crate::CadApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get();
    let mut workspace = app.ui.docking.clone();
    if let Err(error) = validate(&workspace).and_then(|()| sync(&mut workspace, app)) {
        app.set_status(format!("Panel layout could not be restored: {error}"));
        workspace = default_workspace();
        let _ = sync(&mut workspace, app);
    }
    // Bodies can dispatch commands (including the drawing's typed command line).
    // Keep the canonical workspace accessible while rendering an immutable snapshot.
    app.ui.docking = workspace.clone();
    let mut style = DockStyle::from_ui(ui);
    style.min_pane = 96.0;
    style.background = t.panel;
    style.tab_background = t.chrome;
    style.active_background = t.tab_active;
    style.text = t.text;
    style.inactive_text = t.text_dim;
    style.border = egui::Stroke::new(1.0, t.border);
    style.accent = t.accent;
    style.font = crate::theme::body();
    // AutoCAD's palettes dock only in the left and right dock columns: a window edge without a
    // dock makes one, and neither the drawing nor the top and bottom edges take a drop.
    // Floating title bars only move windows.
    style.side_edge_zones = false;
    style.new_dock_zones = true;
    style.drop_edges = Sides::NONE;
    style.title_bar_docks = false;
    let mut area = DockArea::new(egui::Id::unique("cadcraft-docking")).input_blocked(native_input_blocked(app, ui.ctx()));
    if let Some(panel) = app.raise_panel.take() {
        area = area.raise_panel(panel);
    }
    let mut output = area.show_content(ui, &workspace.layout, &style, &mut Panels { app });
    // Keys typed in a floating panel's own OS window run the main window's shortcuts and
    // command line; files dropped on one open like files dropped on the main window.
    app.forward_native_input(ui.ctx(), std::mem::take(&mut output.forwarded_events), output.forwarded_modifiers);
    #[cfg(not(target_arch = "wasm32"))]
    for files in std::mem::take(&mut output.window_files) {
        for file in files.dropped {
            let path = file.path();
            if !path.as_os_str().is_empty() {
                app.open_path(path);
            }
        }
    }
    // Existing panel close buttons and commands still own their visibility switches.
    let rendered = workspace;
    let mut workspace = app.ui.docking.clone();
    let _ = sync(&mut workspace, app);
    let stale = workspace != rendered;
    if stale {
        craft_ui::docking::cancel_drag::<Panel>(ui.ctx(), egui::Id::unique("cadcraft-docking"));
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    if stale {
        return;
    }
    for action in output.actions {
        if let Err(error) =
            serde_json::to_value(action).map_err(|error| error.to_string()).and_then(|action| command(app, false, &json!({"action": action})))
        {
            app.set_status(format!("Panel move was not applied: {error}"));
        }
    }
}

/// Whether the floating panels' own OS windows ignore input: while a modal dialog is up, since
/// egui's `Modal` blocks only the main window. egui's modal layer is last frame's; the app's
/// state covers the frame a modal opens in.
pub fn native_input_blocked(app: &crate::CadApp, ctx: &egui::Context) -> bool {
    app.shows_modal() || ctx.memory(|memory| memory.top_modal_layer()).is_some()
}

/// The panels' contents, names, size limits and icons for the dock area.
pub struct Panels<'a> {
    pub app: &'a mut crate::CadApp,
}

impl DockContent<Panel> for Panels<'_> {
    fn body(&mut self, ui: &mut egui::Ui, panel: &Panel) {
        let app = &mut *self.app;
        match panel {
            Panel::Canvas => {
                ui.painter().rect_filled(ui.max_rect(), 0.0, crate::theme::Tokens::get().canvas);
                crate::canvas::show(app, ui);
            }
            Panel::ToolSets => crate::palettes::toolsets(app, ui),
            Panel::QuickCalc => crate::palettes::quickcalc::show(app, ui),
            Panel::Layers => {
                egui::ScrollArea::vertical()
                    .id_salt("dock-layers")
                    .auto_shrink([false, false])
                    .show(ui, |ui| crate::palettes::layers_section(app, ui));
            }
            Panel::Properties => {
                egui::ScrollArea::vertical().id_salt("dock-properties").auto_shrink([false, false]).show(ui, |ui| {
                    crate::shortcut_menus::palette(app, ui, crate::palettes::properties_section, crate::shortcut_menus::properties_entries)
                });
            }
            Panel::ExternalRefs => {
                egui::ScrollArea::both().id_salt("dock-xrefs").auto_shrink([false, false]).show(ui, |ui| crate::xrefs::section(app, ui));
            }
        }
    }
    fn logical_label(&self, panel: &Panel) -> String {
        crate::i18n::t(panel.label()).into()
    }
    fn permissions(&self, panel: &Panel) -> Permissions {
        permissions(panel)
    }
    fn limits(&self, panel: &Panel) -> PanelLimits {
        panel_limits(panel)
    }
    fn dock_heights(&self, panel: &Panel) -> Option<DockHeights> {
        // Layers keeps its list's height above Properties, which takes the rest of the column.
        (*panel == Panel::Layers).then(|| DockHeights::new(LAYERS_HEIGHT))
    }
    fn paint_icon(&mut self, painter: &egui::Painter, rect: egui::Rect, panel: &Panel, _color: egui::Color32) -> bool {
        let Some(icon) = panel.icon() else { return false };
        crate::icons::paint(painter, rect, icon, false);
        true
    }
}

fn panel_limits(panel: &Panel) -> PanelLimits {
    let (min, max_width) = match panel {
        Panel::Canvas => (egui::vec2(300.0, 180.0), 1_000_000.0),
        Panel::QuickCalc => (egui::vec2(240.0, 160.0), 600.0),
        Panel::ToolSets => (egui::vec2(220.0, 150.0), 480.0),
        Panel::Layers | Panel::Properties | Panel::ExternalRefs => (egui::vec2(220.0, 100.0), 720.0),
    };
    PanelLimits { min, max: egui::vec2(max_width, 1_000_000.0) }
}
