//! Application panel identities and persistence around craft-ui's `DockArea`. Document contents remain owned by
//! their existing view implementations.
//!
//! `UiState::docking` is the saved workspace and the source of truth. The dock shows an `Arc` of it
//! ([`LiveDock`]), kept while the saved workspace is unchanged; every change the dock reports goes through the
//! same checks as a `ui.dock` command.

use std::sync::Arc;

use craft_behavior::DockSide;
use craft_behavior::dock::{Action, Column, DockEdge, DockError, DockPlacement, Group, Layout, Location, Node, Permissions, SplitSize, Target};
use craft_behavior::flow::Axis;
use craft_ui::contract::{Bind, BodyCx, MemoKey, MemoProps, PlainData, SizeLimits, Tracked};
use craft_ui::docking::{DockConfig, DockHeights, DockPanels, DockSides, GroupAt, PanelSpec, PanelSpecs};
use craft_ui::patterns::dock::DockArea;
use craft_ui::primitives::{Glyph, IconSpec};
use craft_ui::style::UiTokens;
use egui::{Rect, pos2, vec2};
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
    Blocks,
}

impl Panel {
    pub const ALL: &[Self] = &[Self::Canvas, Self::ToolSets, Self::Layers, Self::Properties, Self::ExternalRefs];
    /// Every panel, the ones opened on request included.
    const EVERY: [Self; 7] = [Self::Canvas, Self::ToolSets, Self::QuickCalc, Self::Layers, Self::Properties, Self::ExternalRefs, Self::Blocks];
    pub fn id(self) -> &'static str {
        match self {
            Self::Canvas => "canvas",
            Self::ToolSets => "toolSets",
            Self::QuickCalc => "quickCalc",
            Self::Layers => "layers",
            Self::Properties => "properties",
            Self::ExternalRefs => "externalReferences",
            Self::Blocks => "blocks",
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
            Self::Blocks => "Blocks",
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
            Self::Blocks => Some(Icon::Blocks),
        }
    }
    /// Palettes outside the default arrangement, opened on request (QUICKCALC).
    pub const ON_DEMAND: &[Self] = &[Self::QuickCalc, Self::Blocks];
}

/// The drawing view never moves, floats, closes or takes tabs; palettes dock beside it.
pub fn permissions(panel: &Panel) -> Permissions {
    if *panel == Panel::Canvas { Permissions::PROTECTED.accepts_splits(true) } else { Permissions::default() }
}

fn tabs(panel: Panel) -> Node<Panel> {
    Node::tabs(vec![panel], 0)
}

/// The saved panel arrangement: craft-ui's workspace over CADCraft's panels.
pub type Workspace = craft_behavior::dock::Workspace<Panel>;

/// The panel layout's shape version, saved beside it. Layouts saved without it, or with an
/// older one, are the split frames from before the dock columns and give way to the default.
pub const LAYOUT_VERSION: u32 = 2;

/// Width of the left dock's Tool Sets column.
pub const TOOL_SETS_WIDTH: f32 = 220.0;
/// Width of the right dock's palettes column.
pub const PALETTES_WIDTH: f32 = 300.0;
/// Layers' body height above Properties in a dock column.
const LAYERS_HEIGHT: f32 = 280.0;

fn dock(side: DockEdge, width: f32, groups: &[Panel]) -> Node<Panel> {
    let groups = groups.iter().map(|panel| Group::new(vec![*panel], 0)).collect();
    Node::dock(side, vec![Column::new(groups).with_width(width)])
}

/// Tool Sets in the left dock, the drawing, and Layers above Properties in the right dock.
pub fn default_workspace() -> Workspace {
    Workspace::new(Layout::new(
        Some(Node::split(
            Axis::X,
            SplitSize::FixedFirst(TOOL_SETS_WIDTH),
            dock(DockEdge::Start, TOOL_SETS_WIDTH, &[Panel::ToolSets]),
            Node::split(
                Axis::X,
                SplitSize::FixedSecond(PALETTES_WIDTH),
                tabs(Panel::Canvas),
                dock(DockEdge::End, PALETTES_WIDTH, &[Panel::Layers, Panel::Properties]),
            ),
        )),
        Vec::new(),
    ))
}

/// The dock a panel belongs in: Tool Sets on the left, the palettes on the right.
fn home_side(panel: Panel) -> (DockEdge, f32) {
    if panel == Panel::ToolSets { (DockEdge::Start, TOOL_SETS_WIDTH) } else { (DockEdge::End, PALETTES_WIDTH) }
}

/// The panels of every dock group on `side`: columns from the window edge inwards, groups from
/// top to bottom.
fn dock_groups(layout: &Layout<Panel>, side: DockEdge) -> Vec<Vec<Vec<Panel>>> {
    let mut nodes: Vec<&Node<Panel>> = layout.root.iter().collect();
    let mut seen = 0usize;
    while let Some(node) = nodes.pop() {
        seen = seen.saturating_add(1);
        if seen > 4 * craft_behavior::dock::MAX_PANELS {
            break;
        }
        match node {
            Node::Split { first, second, .. } => nodes.extend([&**first, &**second]),
            Node::Dock { side: s, columns, .. } if *s == side => {
                return columns.iter().map(|column| column.groups.iter().map(|group| group.panels.clone()).collect()).collect();
            }
            _ => {}
        }
    }
    Vec::new()
}

fn docked(layout: &Layout<Panel>, panel: Panel) -> bool {
    [DockEdge::Start, DockEdge::End].into_iter().any(|side| dock_groups(layout, side).iter().flatten().flatten().any(|p| *p == panel))
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
        Panel::Blocks => &[(Panel::QuickCalc, true), (Panel::ExternalRefs, true), (Panel::Properties, true), (Panel::Layers, true)],
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
        return vec![Action::open_at(panel, anchor, DockPlacement::group(below, None))];
    }
    let (side, width) = home_side(panel);
    vec![Action::open(panel, None), Action::move_panel(panel, Target::new_dock(side)), Action::resize_column(panel, width)]
}

/// craft-ui's checks plus the drawing view's: a saved or hand-edited file cannot hide, float,
/// group, collapse or dock it, or restore a palette over it.
pub fn validate(workspace: &Workspace) -> Result<(), String> {
    workspace.validate().map_err(|error| error.to_string())?;
    let layout = workspace.layout();
    if !layout.contains(&Panel::Canvas) {
        return Err("The panel layout must contain its document view".into());
    }
    if layout.floating.iter().any(|window| window.contains(&Panel::Canvas)) {
        return Err("The document view cannot float".into());
    }
    let mut nodes: Vec<_> = layout.root.iter().collect();
    while let Some(node) = nodes.pop() {
        match node {
            Node::Split { first, second, .. } => {
                nodes.push(first);
                nodes.push(second);
            }
            Node::Tabs { panels, .. } if panels.contains(&Panel::Canvas) && panels.len() != 1 => {
                return Err("The document view cannot share a tab group".into());
            }
            Node::Stack { entries, .. } if entries.iter().any(|entry| entry.panel == Panel::Canvas) => {
                return Err("The document view cannot be collapsed".into());
            }
            Node::Dock { columns, .. } if columns.iter().flat_map(|column| &column.groups).any(|group| group.panels.contains(&Panel::Canvas)) => {
                return Err("The document view cannot be in a dock".into());
            }
            _ => {}
        }
    }
    let over_canvas = |location: &Location<Panel>| {
        location.anchor == Some(Panel::Canvas) && matches!(location.placement, DockPlacement::Tab { .. } | DockPlacement::Split(DockSide::Center))
    };
    if workspace.hidden().iter().any(|(_, location)| over_canvas(location)) {
        return Err("A hidden panel cannot restore over the document view".into());
    }
    Ok(())
}

/// Show a panel and select it at its recorded place, keeping the drawing view's rules. A panel
/// with no recorded place opens in its dock ([`home`]); one whose recorded place is gone joins
/// its docked neighbour's group.
pub fn reveal_panel(workspace: &mut Workspace, panel: Panel) -> Result<(), String> {
    let mut next = workspace.clone();
    if next.layout().contains(&panel) || next.hidden().iter().any(|(id, _)| *id == panel) {
        let fallback = home_anchor(next.layout(), panel).map(|(anchor, _)| anchor);
        next.set_visible(panel, true, fallback, false, permissions).map_err(|error| error.to_string())?;
    } else {
        let actions: Vec<_> = home(next.layout(), panel).into_iter().chain([Action::activate(panel)]).collect();
        next.apply_all(actions, false, permissions).map_err(|error| error.to_string())?;
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
    if workspace.layout().contains(&panel) == visible {
        return Ok(());
    }
    if !visible {
        return workspace.set_visible(panel, false, None, false, permissions).map_err(|error| error.to_string());
    }
    let location = workspace.hidden().iter().find(|(id, _)| *id == panel).map(|(_, location)| location.clone());
    // A direct restore against a hidden anchor would open a window of its own.
    let direct = location.filter(|location| location.anchor.is_none_or(|anchor| workspace.layout().contains(&anchor)));
    let mut layout = workspace.layout().clone();
    if direct.is_some_and(|location| layout.restore(panel, &location).is_ok()) {
        let hidden = workspace.hidden().iter().filter(|(id, _)| *id != panel).cloned().collect();
        let next = Workspace::from_parts(layout, hidden, workspace.floated().to_vec());
        validate(&next)?;
        *workspace = next;
        return Ok(());
    }
    reveal_panel(workspace, panel)
}

/// `ui.dock` params: `{action}`, a layout action, or `{operation, …}` ([`DockOperation`]).
fn apply_command(workspace: &mut Workspace, params: &Value) -> Result<(), String> {
    validate(workspace)?;
    let action = if let Some(action) = params.get("action") {
        serde_json::from_value(action.clone()).map_err(|error| error.to_string())?
    } else {
        let operation: DockOperation = serde_json::from_value(params.clone()).map_err(|error| error.to_string())?;
        if let DockOperation::Open { panel } | DockOperation::Activate { panel } = &operation
            && !workspace.layout().contains(panel)
        {
            let mut next = workspace.clone();
            reveal_panel(&mut next, *panel)?;
            validate(&next)?;
            *workspace = next;
            return Ok(());
        }
        operation.action(workspace.layout()).map_err(|error| error.to_string())?
    };
    apply_action(workspace, action)
}

/// Apply one layout action to a copy of `workspace` and keep it only when it passes [`validate`].
fn apply_action(workspace: &mut Workspace, action: Action<Panel>) -> Result<(), String> {
    let mut next = workspace.clone();
    next.apply(action, false, permissions).map_err(|error| error.to_string())?;
    validate(&next)?;
    *workspace = next;
    Ok(())
}

/// A drop zone by name, as `ui.dock` writes it. PascalCase names also parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ZoneName {
    Tab,
    #[serde(alias = "Center")]
    Center,
    #[serde(alias = "Left")]
    Left,
    #[serde(alias = "Right")]
    Right,
    #[serde(alias = "Top")]
    Top,
    #[serde(alias = "Bottom")]
    Bottom,
    Above,
    Below,
}

/// A present field, `null` included: `Some(None)` for `null`, `None` when absent.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

/// A required field that may be `null`.
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    T::deserialize(deserializer)
}

/// A `ui.dock` operation, tagged by `operation`. `move` takes `target` (the anchor), `zone`
/// (default `center`) and `before`, which only applies to a center move. Rects are `[x, y, width, height]`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase")]
enum DockOperation {
    /// Show a panel at its last location and select it.
    Open {
        panel: Panel,
    },
    Close {
        panel: Panel,
    },
    Activate {
        panel: Panel,
    },
    Float {
        panel: Panel,
        rect: [f32; 4],
    },
    MoveFloating {
        panel: Panel,
        rect: [f32; 4],
    },
    Move {
        panel: Panel,
        target: Panel,
        #[serde(default)]
        zone: Option<ZoneName>,
        #[serde(default, deserialize_with = "present")]
        before: Option<Option<Panel>>,
    },
    SetStackOpen {
        panel: Panel,
        open: bool,
    },
    /// `height` is required; `null` clears it.
    ResizeStack {
        panel: Panel,
        #[serde(deserialize_with = "required")]
        height: Option<f32>,
    },
    ResizeSplit {
        path: Vec<bool>,
        size: SplitSize,
    },
}

/// `[x, y, width, height]` as a rect.
fn rect_of([x, y, width, height]: [f32; 4]) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(width, height))
}

/// Where a `move` puts the panel relative to `anchor`: `zone` (default `center`) and `before`,
/// with side zones mapped onto the anchor's container by [`DockPlacement::for_zone`].
fn placement(
    layout: &Layout<Panel>,
    anchor: &Panel,
    zone: Option<ZoneName>,
    before: Option<Option<Panel>>,
) -> Result<DockPlacement<Panel>, DockError> {
    let side = |side| DockPlacement::for_zone(layout, anchor, side);
    Ok(match (zone.unwrap_or(ZoneName::Center), before) {
        (ZoneName::Tab | ZoneName::Center, Some(before)) => DockPlacement::tab(before),
        (ZoneName::Tab, None) => DockPlacement::tab(None),
        (ZoneName::Center, None) => DockPlacement::Split(DockSide::Center),
        (_, Some(_)) => return Err(DockError::InvalidPlacement),
        (ZoneName::Left, None) => side(DockSide::Start)?,
        (ZoneName::Right, None) => side(DockSide::End)?,
        (ZoneName::Top, None) => side(DockSide::Top)?,
        (ZoneName::Bottom, None) => side(DockSide::Bottom)?,
        (ZoneName::Above, None) => DockPlacement::group(false, None),
        (ZoneName::Below, None) => DockPlacement::group(true, None),
    })
}

impl DockOperation {
    /// The layout action this operation makes on `layout`. A panel that is not shown is
    /// [`DockError::MissingPanel`] for Open and Activate, which show it first ([`apply_command`]).
    fn action(&self, layout: &Layout<Panel>) -> Result<Action<Panel>, DockError> {
        Ok(match self {
            Self::Open { panel } | Self::Activate { panel } => {
                if !layout.contains(panel) {
                    return Err(DockError::MissingPanel);
                }
                Action::activate(*panel)
            }
            Self::Close { panel } => Action::close(*panel),
            Self::Float { panel, rect } => Action::float(*panel, rect_of(*rect)),
            Self::MoveFloating { panel, rect } => Action::move_floating(*panel, rect_of(*rect)),
            Self::Move { panel, target, zone, before } => Action::move_beside(*panel, *target, placement(layout, target, *zone, *before)?),
            Self::SetStackOpen { panel, open } => Action::set_stack_open(*panel, *open),
            Self::ResizeStack { panel, height } => Action::resize_stack(*panel, *height),
            Self::ResizeSplit { path, size } => Action::resize_split(path.clone(), *size),
        })
    }
}

fn sync(workspace: &mut Workspace, app: &crate::CadApp) -> Result<(), String> {
    set_visible(workspace, Panel::ToolSets, app.ui.show_toolsets)?;
    if !app.ui.show_palettes {
        set_visible(workspace, Panel::Layers, false)?;
        set_visible(workspace, Panel::Properties, false)?;
    } else if !workspace.layout().contains(&Panel::Layers) && !workspace.layout().contains(&Panel::Properties) {
        // Reopen the palette that closed last before the one recorded beside it.
        set_visible(workspace, Panel::Properties, true)?;
        set_visible(workspace, Panel::Layers, true)?;
    }
    Ok(())
}

fn publish_visibility(app: &mut crate::CadApp, workspace: &Workspace) {
    app.ui.show_toolsets = workspace.layout().contains(&Panel::ToolSets);
    app.ui.show_palettes = workspace.layout().contains(&Panel::Layers) || workspace.layout().contains(&Panel::Properties);
}

pub fn command(app: &mut crate::CadApp, reset: bool, params: &Value) -> Result<Value, String> {
    let mut workspace = app.ui.docking.clone();
    if reset {
        workspace.replace(default_workspace());
    } else {
        sync(&mut workspace, app)?;
        apply_command(&mut workspace, params)?;
        // An opened panel's floating window comes to the front, in its own OS window too.
        if matches!(params.get("operation").and_then(Value::as_str), Some("open" | "activate")) {
            app.dock.raise = params.get("panel").and_then(|panel| serde_json::from_value(panel.clone()).ok());
        }
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    Ok(json!({"docking": app.ui.docking}))
}

/// Apply a change the dock reported, with the app's visibility switches synced first.
fn apply_reported(app: &mut crate::CadApp, action: Action<Panel>) -> Result<(), String> {
    let mut workspace = app.ui.docking.clone();
    sync(&mut workspace, app)?;
    validate(&workspace)?;
    apply_action(&mut workspace, action)?;
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    Ok(())
}

/// The workspace the dock shows, the panels' specs and the panel to raise.
#[derive(Default)]
pub struct LiveDock {
    shown: Option<Arc<Workspace>>,
    specs: Tracked<DockSpecs>,
    /// A panel opened from a menu or command, whose floating window comes to the front on the next frame.
    raise: Option<Panel>,
}

impl LiveDock {
    /// The `Arc` the dock shows for `workspace`: the last one while its arrangement is the same, so an idle pass keeps
    /// the workspace's revision and the dock reads no spec.
    fn shown(&mut self, workspace: &Workspace) -> Arc<Workspace> {
        if let Some(shown) = self.shown.as_ref().filter(|shown| ***shown == *workspace) {
            return Arc::clone(shown);
        }
        let shown = Arc::new(workspace.clone());
        self.shown = Some(Arc::clone(&shown));
        shown
    }
}

/// What the dock reads of the panels: their names in the interface language.
#[derive(Clone, Debug, Default, PartialEq, PlainData)]
struct DockSpecs {
    /// The tab names, in [`Panel::EVERY`] order.
    labels: Vec<String>,
}

impl PanelSpecs<Panel> for DockSpecs {
    fn spec(&self, panel: &Panel) -> PanelSpec {
        let label = Panel::EVERY.iter().position(|p| p == panel).and_then(|index| self.labels.get(index)).cloned().unwrap_or_default();
        // Every panel is CADCraft's own egui code: the dock runs it with the window's style.
        let mut spec = PanelSpec::new(label).limits(panel_limits(panel)).host_body();
        if let Some(glyph) = PanelGlyph::of(*panel) {
            spec = spec.icon(IconSpec::glyph(glyph));
        }
        match panel {
            Panel::Canvas => spec.canvas(),
            // Layers keeps its list's height above Properties, which takes the rest of the column.
            Panel::Layers => spec.heights(DockHeights::new(LAYERS_HEIGHT)),
            _ => spec,
        }
    }

    fn permissions(&self, panel: &Panel) -> Permissions {
        permissions(panel)
    }
}

/// Reads the panel names in the interface language, moving the specs' revision only when one changed.
fn refresh_specs(app: &mut crate::CadApp) {
    let labels: Vec<String> = Panel::EVERY.iter().map(|panel| crate::i18n::t(panel.label()).to_string()).collect();
    if app.dock.specs.labels != labels {
        app.dock.specs.labels = labels;
    }
}

/// A palette's icon in a column collapsed to icons and in its flyout cell, painted by CADCraft's own icon painter.
#[derive(Debug, PartialEq)]
struct PanelGlyph(crate::icons::Icon);

impl PanelGlyph {
    fn of(panel: Panel) -> Option<&'static PanelGlyph> {
        use crate::icons::Icon;
        static TOOL_SETS: PanelGlyph = PanelGlyph(Icon::Switcher);
        static QUICK_CALC: PanelGlyph = PanelGlyph(Icon::Table);
        static LAYERS: PanelGlyph = PanelGlyph(Icon::Layers);
        static PROPERTIES: PanelGlyph = PanelGlyph(Icon::Properties);
        static EXTERNAL_REFS: PanelGlyph = PanelGlyph(Icon::Attach);
        static BLOCKS: PanelGlyph = PanelGlyph(Icon::Blocks);
        match panel {
            Panel::Canvas => None,
            Panel::ToolSets => Some(&TOOL_SETS),
            Panel::QuickCalc => Some(&QUICK_CALC),
            Panel::Layers => Some(&LAYERS),
            Panel::Properties => Some(&PROPERTIES),
            Panel::ExternalRefs => Some(&EXTERNAL_REFS),
            Panel::Blocks => Some(&BLOCKS),
        }
    }
}

impl MemoProps for PanelGlyph {
    fn key(&self, key: &mut MemoKey) {
        key.feed_hash(&self.0);
    }
}

impl Glyph for PanelGlyph {
    /// CADCraft's icons keep their own colours (blue for created geometry, orange for picked points), so the dock's
    /// ink is not applied.
    fn paint(&self, painter: &egui::Painter, _tokens: &UiTokens, rect: Rect, _color: egui::Color32, _mirrored: bool) {
        crate::icons::paint(painter, rect, self.0, false);
    }
}

/// Validate and repair the stored workspace, sync it with the app, and draw the dock: restore or fall back from the
/// persisted layout (a failed restore falls back to the default layout with a status message), draw every panel body
/// and the drawing, forward the keys typed in floating panels' own windows, and apply the pass's layout actions.
/// Bodies may dispatch commands, so the canonical workspace is published before drawing while the shown `Arc` stays
/// unchanged.
pub fn show(app: &mut crate::CadApp, ui: &mut egui::Ui) {
    let mut workspace = app.ui.docking.clone();
    if let Err(error) = validate(&workspace).and_then(|()| sync(&mut workspace, app)) {
        app.set_status(format!("Panel layout could not be restored: {error}"));
        workspace = default_workspace();
        let _ = sync(&mut workspace, app);
    }
    app.ui.docking = workspace.clone();
    refresh_specs(app);
    let shown = app.dock.shown(&workspace);
    // The specs leave the app while its panels borrow it, and come back with their revision.
    let specs = std::mem::take(&mut app.dock.specs);
    let raise = app.dock.raise.take();
    // AutoCAD's palettes dock only in the left and right dock columns: a window edge without a dock makes one, and
    // neither the drawing nor the top and bottom edges take a drop. Floating title bars only move windows. Each
    // closable tab has its close button.
    let config =
        DockConfig::default().side_edge_zones(false).new_dock_zones(true).drop_edges(DockSides::NONE).title_bar_docks(false).tab_close_buttons(true);
    let (output, focused_windows) = {
        let mut panels = Panels { app: &mut *app, focused_windows: Vec::new() };
        let mut area = DockArea::new("Panels", Bind::given(shown), &specs, &mut panels, config);
        if let Some(panel) = raise {
            area = area.raise(panel);
        }
        let output = area.show(ui);
        (output, panels.focused_windows)
    };
    app.dock.specs = specs;
    // Keys typed in a floating panel's own OS window run the main window's shortcuts and command line; while one of that
    // window's widgets has the keyboard, only the keyboard zoom reads them. Files dropped on one open like files dropped
    // on the main window.
    let ctx = ui.ctx().clone();
    for (_, viewport) in &output.windows {
        if *viewport == egui::ViewportId::ROOT {
            continue;
        }
        if focused_windows.contains(viewport) {
            app.zoom_keys(&ctx, *viewport);
            continue;
        }
        let (events, modifiers) =
            ctx.input_for(*viewport, |input| (input.events.iter().filter(|e| forwarded(e)).cloned().collect::<Vec<_>>(), input.modifiers));
        // A Copy, Cut or Paste chord is the runtime's command: it reaches the app through the next frame's `FrameOutput`.
        if modifiers.command {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        let modifiers = (!events.is_empty()).then_some(modifiers);
        app.forward_native_input(&ctx, events, modifiers);
    }
    #[cfg(not(target_arch = "wasm32"))]
    for dropped in &output.window_files {
        for path in dropped.files.iter().filter_map(|file| file.path.as_ref()) {
            app.open_path(path);
        }
    }
    // Existing panel switches and commands still own their visibility switches.
    let rendered = workspace;
    let mut workspace = app.ui.docking.clone();
    let _ = sync(&mut workspace, app);
    let stale = workspace != rendered;
    if stale {
        craft_ui::runtime::drag::cancel(ui.ctx(), craft_behavior::CancelReason::HostChanged);
    }
    publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    if stale {
        return;
    }
    for action in output.actions.into_iter().chain(output.requested) {
        if let Err(error) = apply_reported(app, action) {
            app.set_status(format!("Panel move was not applied: {error}"));
        }
    }
}

/// The events a floating panel's own window hands to the main window: keys, text and the clipboard.
fn forwarded(event: &egui::Event) -> bool {
    matches!(event, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_))
}

/// Whether the floating panels' own OS windows ignore input: while a modal dialog is up, since
/// egui's `Modal` blocks only the main window. egui's modal layer is last frame's; the app's
/// state covers the frame a modal opens in.
pub fn native_input_blocked(app: &crate::CadApp, ctx: &egui::Context) -> bool {
    app.shows_modal() || ctx.memory(|memory| memory.top_modal_layer()).is_some()
}

/// The dock's panels over the app in one pass.
struct Panels<'a> {
    app: &'a mut crate::CadApp,
    /// The floating panels' own OS windows in which one of CADCraft's widgets had the keyboard after its body ran.
    focused_windows: Vec<egui::ViewportId>,
}

impl DockPanels<Panel> for Panels<'_> {
    /// Render `panel`'s existing view; Layers, Properties and External References scroll independently.
    fn body(&mut self, panel: &Panel, ui: &mut egui::Ui, _: &mut BodyCx<'_>) {
        let app = &mut *self.app;
        match panel {
            // The drawing is its group's `group_view`.
            Panel::Canvas => {}
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
            Panel::Blocks => {
                egui::ScrollArea::vertical()
                    .id_salt("dock-blocks")
                    .auto_shrink([false, false])
                    .show(ui, |ui| crate::blocks_palette::section(app, ui));
            }
        }
        let viewport = ui.ctx().viewport_id();
        if viewport != egui::ViewportId::ROOT {
            self.focused_windows.retain(|window| *window != viewport);
            if ui.memory(|memory| memory.focused().is_some()) {
                self.focused_windows.push(viewport);
            }
        }
    }

    /// The drawing: it paints its own background, and is the window's document canvas.
    fn group_view(&mut self, panels: &[Panel], _: usize, ui: &mut egui::Ui, at: GroupAt, _: &mut BodyCx<'_>) -> Option<f32> {
        let GroupAt::Show(rect) = at else { return None };
        if !panels.contains(&Panel::Canvas) {
            return None;
        }
        // Declared before the widgets drawn over it, for its place in Tab order, and again after this pass's input has
        // changed what Tab has to do.
        crate::framework::host_canvas(ui.ctx(), crate::cmdline::tab_has_work(self.app));
        ui.painter().rect_filled(rect, 0.0, crate::theme::Tokens::get().canvas);
        crate::canvas::show(self.app, ui);
        crate::canvas::focus_opened(self.app, ui.ctx());
        crate::framework::host_canvas(ui.ctx(), crate::cmdline::tab_has_work(self.app));
        Some(rect.height())
    }
}

/// The panels' body limits: a minimum size and a maximum width.
fn panel_limits(panel: &Panel) -> SizeLimits {
    let (min, max_width) = match panel {
        Panel::Canvas => (vec2(300.0, 180.0), SizeLimits::BOUND),
        Panel::QuickCalc => (vec2(240.0, 160.0), 600.0),
        Panel::ToolSets => (vec2(220.0, 150.0), 480.0),
        Panel::Layers | Panel::Properties | Panel::ExternalRefs => (vec2(220.0, 100.0), 720.0),
        Panel::Blocks => (vec2(240.0, 260.0), 720.0),
    };
    SizeLimits::new(min, vec2(max_width, SizeLimits::BOUND))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_panel_is_raised_for_one_frame() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        let ctx = egui::Context::default();
        crate::framework::install_headless(&ctx);
        command(&mut app, false, &json!({"operation": "open", "panel": Panel::Properties.id()})).unwrap();
        assert_eq!(app.dock.raise, Some(Panel::Properties));

        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, 1000.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();

        assert_eq!(app.dock.raise, None);
    }

    #[test]
    fn a_reset_replaces_the_workspace() {
        let mut app = crate::CadApp::new(cadcraft_engine::Session::default(), crate::Services::default());
        let start = app.ui.docking.generation();
        command(&mut app, false, &json!({"operation": "open", "panel": Panel::Blocks.id()})).unwrap();
        assert_eq!(app.ui.docking.generation(), start);
        command(&mut app, true, &Value::Null).unwrap();
        assert_ne!(app.ui.docking.generation(), start);
        assert_eq!(app.ui.docking, default_workspace());
    }
}
