//! The Hatch Editor and Hatch Creation contextual Ribbon tabs: the Editor opens by itself when the
//! selection is only hatches, as Windows CAD ribbons do, and closes when they are deselected;
//! Hatch Creation shows while HATCH or GRADIENT runs, its choices setting the HP* system variables
//! the hatch being made follows. Panels, in the usual order:
//! Boundaries, Pattern (gallery), Properties, Origin, Options, Close. Each panel title's ▾ slides
//! out the panel's second part (pinnable). Every change goes through HATCHEDIT, PROPERTIES.SET or
//! the `hatchedit.*` commands (observed black-box; icons are CADCraft's own).

use cadcraft_doc::{EntityKind, Handle, Hatch, Transparency};
use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::palettes::patterns;
use crate::ribbon::{Action, BIG_ICON, BODY, Cmd, PAD, ROW, TITLE, big, font, hover_fill, small, text_w};
use crate::theme::Tokens;

/// The tabs' ids (as `UiState::ribbon_tab` holds them while shown) and English titles.
pub const TAB_ID: &str = "hatchedit";
pub const TAB_TITLE: &str = "Hatch Editor";
pub const CREATE_ID: &str = "hatchcreate";
pub const CREATE_TITLE: &str = "Hatch Creation";

/// The contextual tab's title for its id.
pub fn title(id: &str) -> &'static str {
    if id == CREATE_ID { CREATE_TITLE } else { TAB_TITLE }
}

/// A Hatch Editor label: its own wording where the tab words it differently from the other tabs
/// (`hatchribbon`), else the Ribbon's.
fn hl(source: &str) -> &str {
    crate::i18n::in_context("hatchribbon", source).unwrap_or_else(|| crate::i18n::ribbon(source))
}

const fn c(id: &'static str, label: &'static str, icon: Icon) -> Cmd {
    Cmd { id, label, icon: Some(icon) }
}

const PICK: Cmd = c("hatchedit.points", "Pick Points", Icon::HatchPick);
const SELECT: Cmd = c("hatchedit.select", "Select", Icon::HatchSelect);
const REMOVE: Cmd = c("hatchedit.remove", "Remove", Icon::HatchRemove);
const RECREATE: Cmd = c("hatchedit.recreate", "Recreate", Icon::HatchRecreate);
const SET_ORIGIN: Cmd = c("hatchedit.origin", "Set Origin", Icon::HatchOrigin);
const CLOSE: Cmd = c("ui.hatcheditor.close", "Close Hatch Editor", Icon::CloseEditor);
// Hatch Creation: the same buttons act on the running HATCH (keywords) or are unavailable.
const C_PICK: Cmd = c("ui.hatchcreate.points", "Pick Points", Icon::HatchPick);
const C_SELECT: Cmd = c("ui.hatchcreate.select", "Select", Icon::HatchSelect);
const C_REMOVE: Cmd = c("ui.hatchcreate.remove", "Remove", Icon::HatchRemove);
const C_RECREATE: Cmd = c("", "Recreate", Icon::HatchRecreate);
const C_SET_ORIGIN: Cmd = c("ui.hatchcreate.origin", "Set Origin", Icon::HatchOrigin);
const C_CLOSE: Cmd = c("ui.hatchcreate.close", "Close Hatch Creation", Icon::CloseEditor);
const NONE: &[Cmd] = &[];

/// Gallery tiles in a row (and rows of the panel's gallery).
const TILES: usize = 4;
const TILE_W: f32 = 54.0;

/// Whether the Hatch Editor shows: the selection is hatches only (in the drawing, not in the
/// Block Editor). Keeps `UiState::ribbon_tab` on the tab while it appears and goes back to the
/// tab shown before once the hatches are deselected.
pub fn context(app: &mut CadApp, ctx: &egui::Context) -> Option<&'static str> {
    let id = if creating(app) {
        Some(CREATE_ID)
    } else if !selected(app).is_empty() {
        Some(TAB_ID)
    } else {
        None
    };
    let shown = id.is_some();
    // Switching between the two contextual tabs (HATCH ended with the new hatch selected).
    if let Some(id) = id
        && (app.ui.ribbon_tab == TAB_ID || app.ui.ribbon_tab == CREATE_ID)
    {
        app.ui.ribbon_tab = id.to_string();
    }
    let was_id = egui::Id::new("rb_hatch_context");
    let was: bool = ctx.data(|d| d.get_temp(was_id)).unwrap_or(false);
    if shown && !was {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("rb_hatch_before"), app.ui.ribbon_tab.clone()));
        // A Ribbon the user reduced to its tabs stays so: the contextual tab opens in the tab row.
        app.ui.ribbon_tab = id.unwrap_or(TAB_ID).to_string();
    } else if !shown && was {
        let before: String = ctx.data(|d| d.get_temp(egui::Id::new("rb_hatch_before"))).unwrap_or_else(|| "home".to_string());
        if app.ui.ribbon_tab == TAB_ID || app.ui.ribbon_tab == CREATE_ID {
            app.ui.ribbon_tab = before;
        }
        ctx.data_mut(|d| d.remove::<String>(egui::Id::new("rb_open_panel")));
    }
    if shown != was {
        ctx.data_mut(|d| d.insert_temp(was_id, shown));
    }
    id
}

/// HATCH or GRADIENT is running (the Hatch Creation tab shows).
pub fn creating(app: &CadApp) -> bool {
    app.session.running.as_ref().is_some_and(|r| matches!(r.id.as_str(), "hatch" | "gradient"))
}

/// The selected hatches (empty when anything else is selected too).
pub fn selected(app: &CadApp) -> Vec<Handle> {
    let Ok(st) = app.session.state() else { return Vec::new() };
    if st.block_edit.is_some() {
        return Vec::new();
    }
    let Ok(d) = app.session.doc() else { return Vec::new() };
    let sel = app.session.selection();
    let all = !sel.is_empty() && sel.iter().all(|h| matches!(d.entity(*h).map(|e| &e.kind), Some(EntityKind::Hatch(_))));
    if all { sel } else { Vec::new() }
}

/// What the panels show: the first selected hatch and its object properties.
struct Shown {
    /// Hatch Creation: the HP* settings of the hatch being made, not a selected hatch.
    create: bool,
    ids: Vec<String>,
    hatch: Hatch,
    color: cadcraft_color::Color,
    layer: String,
    layer_color: Color32,
    transparency: Transparency,
    annotative: bool,
    layers: Vec<String>,
    gap: f64,
    store_origin: bool,
    inherit_origin: bool,
    paper: bool,
    /// Hatch Creation only: HPSEPARATE, HPDRAWORDER, HPBOUNDRETAIN, and whether a boundary set
    /// replaces the current viewport.
    separate: bool,
    draw_order: usize,
    retain: usize,
    custom_set: bool,
}

fn shown(app: &CadApp) -> Option<Shown> {
    if creating(app) {
        return shown_create(app);
    }
    let hs = selected(app);
    let d = app.session.doc().ok()?;
    let e = d.entity(*hs.first()?)?;
    let EntityKind::Hatch(h) = &e.kind else { return None };
    Some(Shown {
        create: false,
        ids: hs.iter().map(|h| h.hex()).collect(),
        hatch: h.clone(),
        color: e.common.color,
        layer: e.common.layer.clone(),
        layer_color: d.layer(&e.common.layer).map_or(Color32::WHITE, |l| swatch(&l.color)),
        transparency: e.common.transparency,
        annotative: cadcraft_doc::is_annotative(e),
        layers: d.layers.iter().map(|l| l.name.clone()).collect(),
        gap: d.header.f64("HPGAPTOL", 0.0),
        store_origin: app.session.hatch_store_origin,
        inherit_origin: d.header.i64("HPINHERIT", 0) == 1,
        paper: app.session.space() != cadcraft_doc::Space::Model,
        separate: false,
        draw_order: 0,
        retain: 0,
        custom_set: false,
    })
}

/// Hatch Creation: what the tab shows, from the HP* system variables (HPNAME, HPSCALE, HPANG,
/// HPCOLOR…; "." or empty is "use current").
fn shown_create(app: &CadApp) -> Option<Shown> {
    let d = app.session.doc().ok()?;
    let hd = &d.header;
    let text = |n: &str| Some(hd.str(n, "")).filter(|v| !v.trim().is_empty() && v.trim() != ".");
    let name = hd.str("HPNAME", "ANSI31").to_ascii_uppercase();
    let gradient_cmd = app.session.running.as_ref().is_some_and(|r| r.id == "gradient");
    let gradient = patterns::gradient_of(&name).or(gradient_cmd.then_some("LINEAR")).map(|g| cadcraft_doc::Gradient {
        name: g.into(),
        color1: text("GFCLR1").and_then(|t| Color::parse(&t)).unwrap_or(Color::Index(5)),
        color2: text("GFCLR2").and_then(|t| Color::parse(&t)).unwrap_or(Color::Index(7)),
        angle: hd.f64("GFANG", 0.0).to_radians(),
        centered: true,
    });
    let user = name == cadcraft_doc::USER_PATTERN;
    let line = |angle: f64| cadcraft_doc::library::PatternLine { angle, origin: (0.0, 0.0), delta: (0.0, 1.0), dashes: Vec::new() };
    let hatch = Hatch {
        pattern: if gradient.is_some() { "SOLID".into() } else { name.clone() },
        solid: gradient.is_some() || name == "SOLID",
        loops: Vec::new(),
        scale: hd.f64("HPSCALE", 1.0),
        angle: hd.f64("HPANG", 0.0),
        associative: hd.i64("HPASSOC", 1) != 0,
        style: hd.i64("HPISLANDDETECTION", 0).clamp(0, 2) as u8,
        elevation: 0.0,
        gradient,
        origin: hd.point("HPORIGIN").map(|p| p.xy()).unwrap_or_default(),
        background: text("HPBACKGROUNDCOLOR").and_then(|t| Color::parse(&t)),
        pattern_lines: match (user, hd.i64("HPDOUBLE", 0) != 0) {
            (true, true) => vec![line(0.0), line(90.0)],
            (true, false) => vec![line(0.0)],
            _ => Vec::new(),
        },
    };
    let layer = text("HPLAYER").unwrap_or_else(|| hd.str("CLAYER", "0"));
    Some(Shown {
        create: true,
        ids: Vec::new(),
        hatch,
        color: text("HPCOLOR").and_then(|t| Color::parse(&t)).unwrap_or_else(|| Color::from_aci(hd.i64("CECOLOR", 256) as i16)),
        layer_color: d.layer(&layer).map_or(Color32::WHITE, |l| swatch(&l.color)),
        layer,
        transparency: match text("HPTRANSPARENCY").as_deref().map(str::trim) {
            Some(t) if t.eq_ignore_ascii_case("byblock") => Transparency::ByBlock,
            Some(t) => t.parse::<u8>().map_or(Transparency::ByLayer, |v| Transparency::Percent(v.min(90))),
            None => Transparency::ByLayer,
        },
        annotative: hd.i64("HPANNOTATIVE", 0) != 0,
        layers: d.layers.iter().map(|l| l.name.clone()).collect(),
        gap: hd.f64("HPGAPTOL", 0.0),
        store_origin: app.session.hatch_store_origin,
        inherit_origin: hd.i64("HPINHERIT", 0) == 1,
        paper: app.session.space() != cadcraft_doc::Space::Model,
        separate: hd.i64("HPSEPARATE", 0) != 0,
        draw_order: hd.i64("HPDRAWORDER", 1).clamp(0, 4) as usize,
        retain: hd.i64("HPBOUNDRETAIN", 0).clamp(0, 2) as usize,
        custom_set: app.session.hatch_boundary_set().is_some(),
    })
}

impl Shown {
    /// HATCHEDIT on the selected hatches with `p`; in Hatch Creation, the HP* variables.
    fn edit(&self, mut p: Value) -> Action {
        if self.create {
            return create_vars(&p);
        }
        if let Some(o) = p.as_object_mut() {
            o.insert("handles".into(), json!(self.ids));
        }
        Action::Do(Box::new(move |app: &mut CadApp| {
            if let Err(e) = app.run("hatchedit", p) {
                app.session.echo(e);
            }
        }))
    }
    /// PROPERTIES.SET on the selected hatches with `p`; in Hatch Creation, the HP* variables.
    fn set(&self, mut p: Value) -> Action {
        if self.create {
            return create_vars(&p);
        }
        if let Some(o) = p.as_object_mut() {
            o.insert("handles".into(), json!(self.ids));
        }
        Action::Do(Box::new(move |app: &mut CadApp| {
            if let Err(e) = app.run("properties.set", p) {
                app.session.echo(e);
            }
        }))
    }
}

/// Hatch Creation: an edit of the tab (HATCHEDIT / PROPERTIES.SET parameters) as the HP*
/// system variables it sets.
fn create_vars(p: &Value) -> Action {
    let mut vars: Vec<(&'static str, Value)> = Vec::new();
    let s = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    if let Some(t) = p.get("type").and_then(Value::as_str) {
        vars.push((
            "HPNAME",
            json!(match t {
                "solid" => "SOLID",
                "gradient" => "GR_LINEAR",
                "user" => cadcraft_doc::USER_PATTERN,
                _ => "ANSI31",
            }),
        ));
    }
    if let Some(v) = p.get("pattern") {
        vars.push(("HPNAME", json!(s(v))));
    }
    if let Some(g) = p.get("gradient") {
        if let Some(n) = g.get("name").and_then(Value::as_str) {
            vars.push(("HPNAME", json!(patterns::tile_of_gradient(n).unwrap_or("GR_LINEAR"))));
        }
        for (k, var) in [("color1", "GFCLR1"), ("color2", "GFCLR2")] {
            if let Some(v) = g.get(k) {
                vars.push((var, json!(s(v))));
            }
        }
        if let Some(a) = g.get("angle") {
            vars.push(("GFANG", a.clone()));
        }
    }
    for (k, var) in [("angle", "HPANG"), ("scale", "HPSCALE"), ("double", "HPDOUBLE"), ("associative", "HPASSOC"), ("annotative", "HPANNOTATIVE")] {
        if let Some(v) = p.get(k) {
            vars.push((var, if v.is_boolean() { json!(i64::from(v.as_bool().unwrap_or(false))) } else { v.clone() }));
        }
    }
    for (k, var) in [("color", "HPCOLOR"), ("layer", "HPLAYER"), ("transparency", "HPTRANSPARENCY")] {
        if let Some(v) = p.get(k) {
            vars.push((var, json!(s(v))));
        }
    }
    if let Some(v) = p.get("background") {
        let t = s(v);
        vars.push(("HPBACKGROUNDCOLOR", json!(if t == "none" { ".".to_string() } else { t })));
    }
    if let Some(m) = p.get("originMode").and_then(Value::as_str) {
        let i = ["current", "bottomLeft", "bottomRight", "topRight", "topLeft", "center"].iter().position(|x| *x == m).unwrap_or(0);
        vars.push(("HPORIGINMODE", json!(i)));
    }
    if let Some(st) = p.get("style").and_then(Value::as_str) {
        vars.push((
            "HPISLANDDETECTION",
            json!(match st {
                "outer" => 1,
                "ignore" | "none" => 2,
                _ => 0,
            }),
        ));
    }
    if let Some(o) = p.get("drawOrder").and_then(Value::as_str) {
        let i = ORDERS.iter().position(|(_, v)| *v == o).unwrap_or(0);
        vars.push(("HPDRAWORDER", json!(i)));
    }
    if let Some(v) = p.get("separate").and_then(Value::as_bool) {
        vars.push(("HPSEPARATE", json!(i64::from(v))));
    }
    Action::Do(Box::new(move |app: &mut CadApp| {
        for (name, value) in vars {
            if let Err(e) = app.run("setvar", json!({ "name": name, "value": value })) {
                app.session.echo(e);
            }
        }
    }))
}

fn setvar(name: &'static str, value: Value) -> Action {
    Action::Do(Box::new(move |app: &mut CadApp| {
        if let Err(e) = app.run("setvar", json!({ "name": name, "value": value })) {
            app.session.echo(e);
        }
    }))
}

/// The tab's panels.
pub(crate) fn body(app: &mut CadApp, ui: &mut egui::Ui, body: Rect, out: &mut Option<Action>) {
    let Some(sh) = shown(app) else { return };
    let t = Tokens::get();
    let (pick, select, remove, recreate, set_origin, close) =
        if sh.create { (C_PICK, C_SELECT, C_REMOVE, C_RECREATE, C_SET_ORIGIN, C_CLOSE) } else { (PICK, SELECT, REMOVE, RECREATE, SET_ORIGIN, CLOSE) };
    let widths = [
        big_w(ui, &pick) + small_w(ui, &[select, remove, recreate]) + 2.0 * PAD,
        TILES as f32 * TILE_W + 18.0 + 2.0 * PAD,
        460.0,
        big_w(ui, &set_origin).max(text_w(ui, "Origin") + 24.0) + 2.0 * PAD,
        big_w(ui, &c("", "Associative", Icon::Associative)) + big_w(ui, &c("", "Annotative", Icon::Annotative)) + match_w(ui) + 10.0 + 2.0 * PAD,
        big_w(ui, &close) + 2.0 * PAD,
    ];
    let titles = ["Boundaries", "Pattern", "Properties", "Origin", "Options", "Close"];
    let mut x = body.left() + 6.0;
    let open_id = egui::Id::new("rb_open_panel");
    let open: Option<String> = ui.ctx().data(|d| d.get_temp(open_id));
    for (i, (w, title)) in widths.iter().zip(titles).enumerate() {
        let pr = Rect::from_min_size(pos2(x, body.top() + 2.0), vec2(*w, BODY + TITLE));
        let cells = Rect::from_min_size(pr.min, vec2(*w, BODY));
        match i {
            0 => boundaries(ui, cells, [pick, select, remove, recreate], out),
            1 => gallery(ui, cells, &sh, out),
            2 => properties(ui, cells, &sh, out),
            3 => big(ui, cells.shrink2(vec2(PAD, 0.0)), &set_origin, NONE, out),
            4 => options(ui, cells, &sh, out),
            _ => big(ui, cells.shrink2(vec2(PAD, 0.0)), &close, NONE, out),
        }
        // Title row: panels with a second part slide it out under the title (▾).
        let title_r = Rect::from_min_max(pos2(pr.left(), pr.bottom() - TITLE), pr.max);
        let expandable = matches!(title, "Boundaries" | "Properties" | "Origin" | "Options");
        let label = hl(title);
        if expandable {
            let resp = ui.interact(title_r, ui.id().with(("rbh_title", title)), Sense::click());
            hover_fill(ui, title_r, &resp);
            let tw = text_w(ui, title);
            ui.painter().text(pos2(title_r.center().x - 6.0, title_r.center().y), Align2::CENTER_CENTER, label, font(), t.text_dim);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(title_r.center().x + tw / 2.0 + 2.0, title_r.center().y), vec2(8.0, 8.0)),
                Icon::ChevronDown,
                false,
            );
            icons::describe_control(ui, &resp, title, Some(open.as_deref() == Some(title)), false);
            if resp.clicked() {
                let next = if open.as_deref() == Some(title) { None } else { Some(title.to_string()) };
                ui.ctx().data_mut(|d| match next {
                    Some(n) => {
                        d.insert_temp(open_id, n);
                    }
                    None => {
                        d.remove::<String>(open_id);
                    }
                });
            }
            if title == "Options" {
                // The dialog launcher (↘) opens the Hatch Edit dialog (Hatch and Gradient while
                // creating): the only way to the dialog in the Ribbon layout.
                let lr = Rect::from_center_size(pos2(title_r.right() - 8.0, title_r.center().y), vec2(12.0, 12.0));
                let l = ui.interact(lr, ui.id().with("rbh_launcher"), Sense::click());
                hover_fill(ui, lr, &l);
                let p = ui.painter();
                p.line_segment([lr.left_top() + vec2(3.0, 3.0), lr.right_bottom() - vec2(3.0, 3.0)], Stroke::new(1.0, t.text_dim));
                p.line_segment([lr.right_bottom() - vec2(3.0, 3.0), lr.right_bottom() - vec2(3.0, 7.0)], Stroke::new(1.0, t.text_dim));
                p.line_segment([lr.right_bottom() - vec2(3.0, 3.0), lr.right_bottom() - vec2(7.0, 3.0)], Stroke::new(1.0, t.text_dim));
                if l.on_hover_text(hl(if sh.create { "Hatch and Gradient" } else { "Hatch Edit" })).clicked() {
                    let create = sh.create;
                    *out = Some(Action::Do(Box::new(move |app: &mut CadApp| {
                        if create {
                            let gradient = app.session.running.as_ref().is_some_and(|r| r.id == "gradient");
                            app.session.cancel();
                            crate::hatchdlg::open_create(app, gradient);
                        } else {
                            crate::hatchdlg::open_edit(app);
                        }
                    })));
                }
            }
        } else {
            let resp = ui.interact(title_r, ui.id().with(("rbh_title", title)), Sense::hover());
            ui.painter().text(title_r.center(), Align2::CENTER_CENTER, label, font(), t.text_dim);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
        }
        if open.as_deref() == Some(title) {
            slide_out(ui, title, pr, &sh, out);
        }
        x += w;
        ui.painter().vline(x, (pr.top() + 4.0)..=(pr.bottom() - 4.0), Stroke::new(1.0, t.border));
        x += 1.0;
    }
}

fn big_w(ui: &egui::Ui, cmd: &Cmd) -> f32 {
    crate::ribbon::cell_width(ui, &crate::ribbon::Cell::Big(*cmd, NONE))
}

fn small_w(ui: &egui::Ui, cmds: &[Cmd]) -> f32 {
    cmds.iter().map(|c| crate::ribbon::SMALL_ICON + 8.0 + text_w(ui, c.label)).fold(0.0, f32::max)
}

// ------------------------------------------------------------------------------------------
// Boundaries
// ------------------------------------------------------------------------------------------

fn boundaries(ui: &mut egui::Ui, r: Rect, cmds: [Cmd; 4], out: &mut Option<Action>) {
    let [pick, rest @ ..] = cmds;
    let bw = big_w(ui, &pick);
    big(ui, Rect::from_min_size(r.min + vec2(PAD / 2.0, 0.0), vec2(bw - PAD, r.height())), &pick, NONE, out);
    for (i, cmd) in rest.iter().enumerate() {
        let rr = Rect::from_min_size(pos2(r.left() + bw + PAD, r.top() + i as f32 * ROW + 1.0), vec2(r.width() - bw - 2.0 * PAD, ROW - 2.0));
        small(ui, rr, cmd, NONE, out);
    }
}

// ------------------------------------------------------------------------------------------
// Pattern gallery
// ------------------------------------------------------------------------------------------

/// The gallery tile the hatch shows as current.
fn current_tile(h: &Hatch) -> String {
    match &h.gradient {
        Some(g) => patterns::tile_of_gradient(&g.name).unwrap_or("GR_LINEAR").to_string(),
        None if h.solid => "SOLID".into(),
        None => h.pattern.clone(),
    }
}

/// HATCHEDIT for a clicked tile: a gradient shape or a pattern.
fn tile_edit(sh: &Shown, tile: &str) -> Action {
    match patterns::gradient_of(tile) {
        Some(g) => sh.edit(json!({ "type": "gradient", "gradient": { "name": g } })),
        None => sh.edit(json!({ "pattern": tile })),
    }
}

fn gallery(ui: &mut egui::Ui, r: Rect, sh: &Shown, out: &mut Option<Action>) {
    let t = Tokens::get();
    let tiles = patterns::gallery();
    let rows = tiles.len().div_ceil(TILES).max(1);
    let row_id = egui::Id::new("rbh_gallery_row");
    let current = current_tile(&sh.hatch);
    // The row with the current pattern, until the arrows scroll elsewhere.
    let cur_row = tiles.iter().position(|n| n.eq_ignore_ascii_case(&current)).map_or(0, |i| i / TILES);
    let row: usize = ui.ctx().data(|d| d.get_temp(row_id)).unwrap_or(cur_row).min(rows - 1);
    let area = Rect::from_min_size(r.min + vec2(PAD, 2.0), vec2(TILES as f32 * TILE_W, r.height() - 4.0));
    ui.painter().rect_filled(area, 2.0, t.field);
    for (k, name) in tiles.iter().skip(row * TILES).take(TILES).enumerate() {
        let tr = Rect::from_min_size(pos2(area.left() + k as f32 * TILE_W, area.top()), vec2(TILE_W, area.height()));
        let resp = ui.interact(tr, ui.id().with(("rbh_tile", name)), Sense::click());
        patterns::paint_tile(ui.painter(), tr.shrink(1.0), name, name.eq_ignore_ascii_case(&current), resp.hovered());
        icons::describe_control(ui, &resp, name, Some(name.eq_ignore_ascii_case(&current)), false);
        if resp.on_hover_text(hl(name)).clicked() {
            *out = Some(tile_edit(sh, name));
        }
    }
    // ▲ ▼ and the full gallery (▾ with a bar).
    let col = Rect::from_min_size(pos2(area.right() + 2.0, area.top()), vec2(14.0, area.height()));
    let third = col.height() / 3.0;
    let arrow = |k: usize, icon: Icon, enabled: bool, tip: &str| -> bool {
        let ar = Rect::from_min_size(pos2(col.left(), col.top() + k as f32 * third), vec2(col.width(), third));
        let resp = ui.interact(ar, ui.id().with(("rbh_garrow", k)), Sense::click());
        hover_fill(ui, ar, &resp);
        icons::paint(ui.painter(), Rect::from_center_size(ar.center(), vec2(9.0, 9.0)), icon, !enabled);
        if k == 2 {
            ui.painter().hline((ar.center().x - 4.0)..=(ar.center().x + 4.0), ar.center().y - 5.0, Stroke::new(1.0, t.icon));
        }
        icons::describe_control(ui, &resp, tip, None, false);
        resp.on_hover_text(hl(tip)).clicked() && enabled
    };
    if arrow(0, Icon::ChevronUp, row > 0, "Previous") {
        ui.ctx().data_mut(|d| d.insert_temp(row_id, row.saturating_sub(1)));
    }
    if arrow(1, Icon::ChevronDown, row + 1 < rows, "Next") {
        ui.ctx().data_mut(|d| d.insert_temp(row_id, row + 1));
    }
    let full_id = egui::Id::new("rbh_gallery_full");
    let opened = arrow(2, Icon::ChevronDown, true, "More");
    if opened {
        ui.ctx().data_mut(|d| d.insert_temp(full_id, true));
    }
    if ui.ctx().data(|d| d.get_temp::<bool>(full_id)).unwrap_or(false) {
        let mut keep = true;
        let at = pos2(area.left(), area.top());
        let resp = egui::Area::new(egui::Id::new("rbh_gallery_area")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            // As wide as the gallery in the ribbon, with the same tiles (and the arrows' column).
            egui::Frame::NONE.fill(t.field).stroke(Stroke::new(1.0, t.border)).show(ui, |ui| {
                ui.set_width(area.width() + 16.0);
                egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.8).show(ui, |ui| {
                    egui::Grid::new("rbh_gallery_grid").spacing(vec2(0.0, 0.0)).show(ui, |ui| {
                        for (i, name) in tiles.iter().enumerate() {
                            let (tr, resp) = ui.allocate_exact_size(vec2(TILE_W, area.height()), Sense::click());
                            patterns::paint_tile(ui.painter(), tr, name, name.eq_ignore_ascii_case(&current), resp.hovered());
                            if resp.on_hover_text(hl(name)).clicked() {
                                *out = Some(tile_edit(sh, name));
                                keep = false;
                            }
                            if (i + 1) % TILES == 0 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
        });
        // The click that opened it is not a click elsewhere.
        if !opened && (resp.response.clicked_elsewhere() || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            keep = false;
        }
        if !keep {
            ui.ctx().data_mut(|d| d.remove::<bool>(full_id));
        }
    }
}

// ------------------------------------------------------------------------------------------
// Properties
// ------------------------------------------------------------------------------------------

/// The hatch type list's entries (English sources) and HATCHEDIT's `type`.
const TYPES: [(&str, &str); 4] = [("Solid", "solid"), ("Gradient", "gradient"), ("Pattern", "pattern"), ("User defined", "user")];

fn type_of(h: &Hatch) -> usize {
    if h.gradient.is_some() {
        1
    } else if h.solid {
        0
    } else if h.is_user() {
        3
    } else {
        2
    }
}

use cadcraft_color::Color;

fn color_label(c: &Color) -> String {
    match c {
        Color::True(rgb) => format!("{},{},{}", rgb.0, rgb.1, rgb.2),
        other => crate::ribbon::color_name(i64::from(other.to_aci())),
    }
}

/// A colour as HATCHEDIT and PROPERTIES.SET take it.
fn color_param(c: Color) -> String {
    match c {
        Color::True(rgb) => format!("{},{},{}", rgb.0, rgb.1, rgb.2),
        other => other.to_aci().to_string(),
    }
}

fn swatch(c: &Color) -> Color32 {
    let rgb = c.resolve(Color::Index(7), Color::Index(7));
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// The colour drop-down under a list box.
fn color_menu(resp: &egui::Response, id: &str, current: Color, by: bool) -> Option<Color> {
    match crate::color_list::popup(resp, egui::Id::new(("rbh_color", id)), Some(current), by, false) {
        Some(crate::color_list::Pick::Color(c)) => Some(c),
        _ => None,
    }
}

/// A list box: `icon`, an optional colour swatch, the value, ▾. Returns its response.
fn combo(ui: &mut egui::Ui, r: Rect, id: &str, icon: Option<Icon>, sw: Option<Color32>, value: &str, enabled: bool) -> egui::Response {
    let t = Tokens::get();
    let mut x = r.left();
    if let Some(icon) = icon {
        icons::paint(ui.painter(), Rect::from_min_size(pos2(x + 1.0, r.center().y - 8.0), vec2(16.0, 16.0)), icon, !enabled);
        x += 20.0;
    }
    let field = Rect::from_min_max(pos2(x, r.top() + 1.0), pos2(r.right(), r.bottom() - 1.0));
    let resp = ui.interact(field, ui.id().with(("rbh_combo", id)), if enabled { Sense::click() } else { Sense::hover() });
    ui.painter().rect_filled(field, 2.0, if resp.hovered() && enabled { t.control_hover } else { t.control });
    let mut tx = field.left() + 5.0;
    if let Some(c) = sw {
        let s = Rect::from_min_size(pos2(tx, field.center().y - 6.0), vec2(16.0, 12.0));
        if c == Color32::TRANSPARENT {
            // "None": an empty swatch crossed by a diagonal.
            ui.painter().line_segment([s.left_bottom(), s.right_top()], Stroke::new(1.0, col_or(t.text, enabled)));
            ui.painter().rect_stroke(s, 1.0, Stroke::new(1.0, col_or(t.text, enabled)), egui::StrokeKind::Inside);
        } else {
            ui.painter().rect_filled(s, 1.0, if enabled { c } else { c.gamma_multiply(0.4) });
            ui.painter().rect_stroke(s, 1.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
        tx += 22.0;
    }
    let col = if enabled { t.text } else { t.text_faint };
    let clip = Rect::from_min_max(pos2(tx, field.top()), pos2(field.right() - 14.0, field.bottom()));
    ui.painter().with_clip_rect(clip).text(pos2(tx, field.center().y), Align2::LEFT_CENTER, value, font(), col);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(field.right() - 7.0, field.center().y), vec2(9.0, 9.0)), Icon::ChevronDown, !enabled);
    icons::describe_control(ui, &resp, id, None, false);
    resp
}

fn col_or(c: Color32, enabled: bool) -> Color32 {
    if enabled { c } else { Tokens::get().text_faint }
}

/// A list's menu: the entries' labels; returns the index clicked.
fn menu(resp: &egui::Response, width: f32, items: &[String], current: Option<usize>) -> Option<usize> {
    let mut picked = None;
    egui::Popup::from_toggle_button_response(resp).width(width).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                if ui.selectable_label(current == Some(i), it).clicked() {
                    picked = Some(i);
                }
            }
        });
    });
    picked
}

/// Width of the value box at the right of a slider.
const VALUE_W: f32 = 56.0;

/// A value box: a click selects the whole value, so what is typed replaces it; Enter or a click
/// elsewhere commits, Escape cancels. Returns the new value.
fn value_box(ui: &mut egui::Ui, r: Rect, id: &str, value: f64, decimals: usize, range: (f64, f64), enabled: bool, left: bool) -> Option<f64> {
    let t = Tokens::get();
    let eid = ui.id().with(("rbh_value", id));
    let edit_id = eid.with("edit");
    let col = if enabled { t.text } else { t.text_faint };
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(edit_id)) {
        let first = ui.data(|d| d.get_temp::<bool>(edit_id.with("first"))).unwrap_or(false);
        let te_id = edit_id.with("te");
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::TextEdit::singleline(&mut text)
                    .id(te_id)
                    .font(font())
                    .desired_width(r.width())
                    .horizontal_align(if left { egui::Align::LEFT } else { egui::Align::RIGHT })
                    .show(ui)
            })
            .inner;
        if first {
            out.response.request_focus();
            let mut st = out.state.clone();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::select_all(&out.galley)));
            st.store(ui.ctx(), te_id);
            ui.data_mut(|d| d.insert_temp(edit_id.with("first"), false));
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.data_mut(|d| d.remove::<String>(edit_id));
            return None;
        }
        if !first && (out.response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            ui.data_mut(|d| d.remove::<String>(edit_id));
            return text
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(range.0, range.1))
                .filter(|v| *v != value);
        }
        ui.data_mut(|d| d.insert_temp(edit_id, text));
        return None;
    }
    let resp = ui.interact(r, eid, if enabled { Sense::click() } else { Sense::hover() });
    ui.painter().rect_filled(r, 2.0, if resp.hovered() && enabled { t.control_hover } else { t.field });
    let text = format_value(value, decimals);
    if left {
        ui.painter().text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, text, font(), col);
    } else {
        ui.painter().text(pos2(r.right() - 4.0, r.center().y), Align2::RIGHT_CENTER, text, font(), col);
    }
    icons::describe_control(ui, &resp, id, None, false);
    if resp.clicked() && enabled {
        ui.data_mut(|d| {
            d.insert_temp(edit_id, format_value(value, decimals));
            d.insert_temp(edit_id.with("first"), true);
        });
    }
    None
}

/// A value as the ribbon shows it: no trailing zeros.
fn format_value(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// A slider (as on Windows CAD ribbons): the label and the filled part showing where the value
/// sits in `range`, dragged to change it; the value in its own box at the right (click to type).
fn slider(ui: &mut egui::Ui, r: Rect, id: &str, label: &str, value: f64, range: (f64, f64), decimals: usize, enabled: bool) -> Option<f64> {
    let t = Tokens::get();
    let field = Rect::from_min_max(r.min + vec2(0.0, 1.0), pos2(r.right() - VALUE_W - 2.0, r.bottom() - 1.0));
    let vbox = Rect::from_min_max(pos2(field.right() + 2.0, field.top()), pos2(r.right(), field.bottom()));
    let eid = ui.id().with(("rbh_slider", id));
    let resp = ui.interact(field, eid, if enabled { Sense::click_and_drag() } else { Sense::hover() });
    let drag_id = eid.with("drag");
    let mut shown_v: f64 = ui.data(|d| d.get_temp(drag_id)).unwrap_or(value);
    if (resp.dragged() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let u = f64::from(((p.x - field.left()) / field.width()).clamp(0.0, 1.0));
        let k = 10f64.powi(decimals as i32);
        shown_v = ((range.0 + u * (range.1 - range.0)) * k).round() / k;
        ui.data_mut(|d| d.insert_temp(drag_id, shown_v));
    }
    ui.painter().rect_filled(field, 2.0, if resp.hovered() && enabled { t.control_hover } else { t.control });
    if enabled && range.1 > range.0 {
        let u = ((shown_v - range.0) / (range.1 - range.0)).clamp(0.0, 1.0) as f32;
        let fill = Rect::from_min_max(field.min, pos2(field.left() + u * field.width(), field.bottom()));
        ui.painter().rect_filled(fill, 2.0, t.control_hover.gamma_multiply(1.6));
    }
    let col = if enabled { t.text } else { t.text_faint };
    ui.painter().with_clip_rect(field).text(pos2(field.left() + 4.0, field.center().y), Align2::LEFT_CENTER, hl(label), font(), col);
    icons::describe_control(ui, &resp, label, None, false);
    let resp = resp.on_hover_text(hl(label));
    let typed = value_box(ui, vbox, id, shown_v, decimals, range, enabled, false);
    if resp.drag_stopped() || resp.clicked() {
        ui.data_mut(|d| d.remove::<f64>(drag_id));
        return (shown_v != value).then_some(shown_v);
    }
    typed
}

/// A numeric box with ▲▼ (the Scale / Spacing spinner): click to type, × or ÷ 2 per step.
fn spinner(ui: &mut egui::Ui, r: Rect, id: &str, value: f64, enabled: bool) -> Option<f64> {
    let t = Tokens::get();
    let arrows = Rect::from_min_max(pos2(r.right() - 12.0, r.top() + 1.0), pos2(r.right(), r.bottom() - 1.0));
    let field = Rect::from_min_max(r.min + vec2(0.0, 1.0), pos2(arrows.left() - 1.0, r.max.y - 1.0));
    let v = value_box(ui, field, id, value, 4, (1e-6, 1e9), enabled, true);
    let up = Rect::from_min_max(arrows.min, pos2(arrows.right(), arrows.center().y));
    let down = Rect::from_min_max(pos2(arrows.left(), arrows.center().y), arrows.max);
    let mut step = None;
    for (rr, icon, k) in [(up, Icon::ChevronUp, 2.0), (down, Icon::ChevronDown, 0.5)] {
        let resp = ui.interact(rr, ui.id().with(("rbh_spin", id, icon as u8)), if enabled { Sense::click() } else { Sense::hover() });
        ui.painter().rect_filled(rr, 1.0, if resp.hovered() && enabled { t.control_hover } else { t.control });
        icons::paint(ui.painter(), rr.shrink(1.5), icon, !enabled);
        if resp.clicked() {
            step = Some(value * k);
        }
    }
    v.or(step)
}

fn properties(ui: &mut egui::Ui, r: Rect, sh: &Shown, out: &mut Option<Action>) {
    let h = &sh.hatch;
    let kind = type_of(h);
    let (left_w, gap) = (220.0, 10.0);
    let row = |i: usize, x: f32, w: f32| Rect::from_min_size(pos2(r.left() + PAD + x, r.top() + i as f32 * ROW + 1.0), vec2(w, ROW - 2.0));
    let right_x = left_w + gap;
    let right_w = r.width() - right_x - 2.0 * PAD;

    // Row 1: hatch type, hatch transparency.
    let types: Vec<String> = TYPES.iter().map(|(l, _)| hl(l).to_string()).collect();
    let resp = combo(ui, row(0, 0.0, left_w), "Hatch Type", Some(Icon::HatchType), None, &types[kind], true);
    if let Some(i) = menu(&resp, left_w - 20.0, &types, Some(kind))
        && i != kind
        && let Some((_, ty)) = TYPES.get(i)
    {
        *out = Some(sh.edit(json!({ "type": ty })));
    }
    transparency(ui, row(0, right_x, right_w), sh, out);

    // Row 2: hatch colour (gradient: colour 1), angle.
    let (c1, c1_label) = match &h.gradient {
        Some(g) => (swatch(&g.color1), color_label(&g.color1)),
        None => (swatch(&sh.color), color_label(&sh.color)),
    };
    let resp = combo(ui, row(1, 0.0, left_w), "Hatch Color", Some(Icon::HatchColor), Some(c1), crate::i18n::t(&c1_label), true);
    let current = h.gradient.as_ref().map_or(sh.color, |g| g.color1);
    if let Some(c) = color_menu(&resp, "Hatch Color", current, h.gradient.is_none()) {
        *out = Some(if h.gradient.is_some() {
            sh.edit(json!({ "gradient": { "color1": color_param(c) } }))
        } else {
            sh.set(json!({ "color": color_param(c) }))
        });
    }
    let angle = match &h.gradient {
        Some(g) => g.angle.to_degrees(),
        None => h.angle.to_degrees(),
    }
    .rem_euclid(360.0);
    if let Some(a) = slider(ui, row(1, right_x, right_w), "Angle", "Angle", angle, (0.0, 359.0), 0, kind != 0) {
        *out = Some(if h.gradient.is_some() { sh.edit(json!({ "gradient": { "angle": a } })) } else { sh.edit(json!({ "angle": a })) });
    }

    // Row 3: background colour (gradient: colour 2), scale or spacing.
    let none = hl("None").to_string();
    match &h.gradient {
        Some(g) => {
            let resp = combo(
                ui,
                row(2, 0.0, left_w),
                "Gradient Color 2",
                Some(Icon::HatchBackground),
                Some(swatch(&g.color2)),
                &color_label(&g.color2),
                true,
            );
            if let Some(c) = color_menu(&resp, "Gradient Color 2", g.color2, false) {
                *out = Some(sh.edit(json!({ "gradient": { "color2": color_param(c) } })));
            }
        }
        None => {
            let pattern = kind >= 2;
            let label = h.background.as_ref().map_or_else(|| if pattern { none.clone() } else { String::new() }, color_label);
            let sw = match &h.background {
                Some(c) => Some(swatch(c)),
                None if pattern => Some(Color32::TRANSPARENT),
                None => None,
            };
            let resp = combo(ui, row(2, 0.0, left_w), "Background Color", Some(Icon::HatchBackground), sw, &label, pattern);
            match crate::color_list::popup(&resp, egui::Id::new("rbh_background"), h.background, false, true) {
                Some(crate::color_list::Pick::None) => *out = Some(sh.edit(json!({ "background": "none" }))),
                Some(crate::color_list::Pick::Color(c)) => *out = Some(sh.edit(json!({ "background": color_param(c) }))),
                None => {}
            }
        }
    }
    let user = h.is_user();
    let label = if user { "Spacing" } else { "Scale" };
    if let Some(v) = spinner(ui, row(2, right_x + 20.0, right_w - 20.0), label, h.scale, kind >= 2) {
        *out = Some(sh.edit(json!({ "scale": v })));
    }
    icons::paint(ui.painter(), Rect::from_min_size(row(2, right_x, 18.0).min + vec2(1.0, 3.0), vec2(16.0, 16.0)), Icon::HatchScale, kind < 2);
}

/// Hatch Transparency: the ▾ list (current value, ByLayer, ByBlock, a value) and the slider.
fn transparency(ui: &mut egui::Ui, r: Rect, sh: &Shown, out: &mut Option<Action>) {
    let ir = Rect::from_min_size(r.min, vec2(26.0, r.height()));
    let resp = ui.interact(ir, ui.id().with("rbh_tr_mode"), Sense::click());
    hover_fill(ui, ir, &resp);
    icons::paint(ui.painter(), Rect::from_min_size(pos2(ir.left() + 1.0, ir.center().y - 8.0), vec2(16.0, 16.0)), Icon::HatchTransparency, false);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(ir.right() - 4.0, ir.center().y), vec2(7.0, 7.0)), Icon::ChevronDown, false);
    let items: Vec<String> = ["ByLayer", "ByBlock", "Transparency Value"].iter().map(|s| crate::i18n::t(s).to_string()).collect();
    if let Some(i) = menu(&resp, 200.0, &items, None) {
        *out = Some(match i {
            0 => sh.set(json!({ "transparency": "ByLayer" })),
            1 => sh.set(json!({ "transparency": "ByBlock" })),
            _ => sh.set(json!({ "transparency": 0 })),
        });
    }
    let value = sh.transparency.name().parse::<f64>().unwrap_or(0.0);
    let field = Rect::from_min_max(pos2(ir.right() + 2.0, r.top()), r.max);
    if let Some(v) = slider(ui, field, "Hatch Transparency", "Hatch Transparency", value, (0.0, 90.0), 0, true) {
        *out = Some(sh.set(json!({ "transparency": v.round() as i64 })));
    }
}

// ------------------------------------------------------------------------------------------
// Options
// ------------------------------------------------------------------------------------------

const MATCH_VARIANTS: [&str; 2] = ["Use Current Origin", "Use Source Hatch Origin"];

/// A big toggle button (Associative, Annotative): drawn pressed while on.
/// A toggle that is on, drawn pressed as Windows CAD ribbons do: a light blue fill and a blue
/// border, clearly visible on the dark theme.
fn pressed(p: &egui::Painter, r: Rect) {
    let t = Tokens::get();
    p.rect_filled(r, 2.0, t.accent.gamma_multiply(0.45));
    p.rect_stroke(r, 2.0, Stroke::new(1.0, t.accent.linear_multiply(1.3)), egui::StrokeKind::Inside);
}

fn big_toggle(ui: &mut egui::Ui, r: Rect, label: &'static str, icon: Icon, on: bool) -> bool {
    let t = Tokens::get();
    if on {
        pressed(ui.painter(), r.shrink(1.0));
    }
    let cmd = c("", label, icon);
    let resp = ui.interact(r, ui.id().with(("rbh_toggle", label)), Sense::click());
    hover_fill(ui, r, &resp);
    crate::ribbon::paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.center().x, r.top() + 4.0 + BIG_ICON / 2.0), vec2(BIG_ICON, BIG_ICON)),
        &cmd,
        false,
    );
    ui.painter().text(pos2(r.center().x, r.top() + BIG_ICON + 10.0), Align2::CENTER_TOP, hl(label), font(), t.text);
    icons::describe_control(ui, &resp, label, Some(on), false);
    resp.on_hover_text(hl(label)).clicked()
}

fn options(ui: &mut egui::Ui, r: Rect, sh: &Shown, out: &mut Option<Action>) {
    let mut x = r.left() + PAD;
    let w = big_w(ui, &c("", "Associative", Icon::Associative));
    if big_toggle(ui, Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height())), "Associative", Icon::Associative, sh.hatch.associative) {
        *out = Some(sh.edit(json!({ "associative": !sh.hatch.associative })));
    }
    x += w;
    let w = big_w(ui, &c("", "Annotative", Icon::Annotative));
    if big_toggle(ui, Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height())), "Annotative", Icon::Annotative, sh.annotative) {
        *out = Some(sh.set(json!({ "annotative": !sh.annotative })));
    }
    x += w;
    // Match Properties, with the origin choice under its ▾ (HPINHERIT).
    let w = match_w(ui) + 10.0;
    let mr = Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()));
    let t = Tokens::get();
    let top = Rect::from_min_max(mr.min, pos2(mr.right() - 12.0, mr.bottom()));
    let resp = ui.interact(top, ui.id().with("rbh_match"), Sense::click());
    hover_fill(ui, top, &resp);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(top.center().x, top.top() + 4.0 + BIG_ICON / 2.0), vec2(BIG_ICON, BIG_ICON)),
        Icon::HatchMatch,
        false,
    );
    let (a, b) = two_lines(hl("Match Properties"));
    let y = top.top() + BIG_ICON + 10.0;
    ui.painter().text(pos2(top.center().x, y), Align2::CENTER_TOP, &a, font(), t.text);
    if let Some(b) = b {
        ui.painter().text(pos2(top.center().x, y + 13.0), Align2::CENTER_TOP, b, font(), t.text);
    }
    icons::describe_control(ui, &resp, "Match Properties", None, false);
    if resp.on_hover_text(format!("{} (HATCHEDIT)", hl("Match Properties"))).clicked() {
        *out = Some(Action::Start(if sh.create { "ui.hatchcreate.match" } else { "hatchedit.match" }));
    }
    let vr = Rect::from_min_max(pos2(top.right(), mr.top() + BIG_ICON + 8.0), mr.max);
    let v = ui.interact(vr, ui.id().with("rbh_match_v"), Sense::click());
    hover_fill(ui, vr, &v);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(vr.center().x, y + 7.0), vec2(8.0, 8.0)), Icon::ChevronDown, false);
    let items: Vec<String> = MATCH_VARIANTS.iter().map(|s| hl(s).to_string()).collect();
    if let Some(i) = menu(&v, 260.0, &items, Some(usize::from(sh.inherit_origin))) {
        let create = sh.create;
        *out = Some(Action::Do(Box::new(move |app: &mut CadApp| {
            let _ = app.run("setvar", json!({ "name": "HPINHERIT", "value": i }));
            app.start(if create { "ui.hatchcreate.match" } else { "hatchedit.match" });
        })));
    }
}

/// Match Properties' width, from its Hatch Editor wording on two lines.
fn match_w(ui: &egui::Ui) -> f32 {
    let (a, b) = two_lines(hl("Match Properties"));
    let w = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font(), Color32::WHITE).size().x;
    w(&a).max(b.as_deref().map_or(0.0, w)).max(BIG_ICON + 4.0) + 2.0 * PAD
}

fn two_lines(s: &str) -> (String, Option<String>) {
    if s.chars().count() <= 10 || !s.contains(' ') {
        return (s.to_string(), None);
    }
    let mid = s.len() / 2;
    let split = s.char_indices().filter(|(_, ch)| *ch == ' ').map(|(i, _)| i).min_by_key(|i| i.abs_diff(mid)).unwrap_or(mid);
    let (a, b) = s.split_at(split);
    (a.to_string(), Some(b.trim_start().to_string()))
}

// ------------------------------------------------------------------------------------------
// Slide-out panels (the panel title's ▾)
// ------------------------------------------------------------------------------------------

/// The island detection list (HATCHEDIT `style`).
const ISLANDS: [(&str, &str, Icon); 4] = [
    ("Normal Island Detection", "normal", Icon::IslandNormal),
    ("Outer Island Detection", "outer", Icon::IslandOuter),
    ("Ignore Island Detection", "ignore", Icon::IslandIgnore),
    ("No Island Detection", "none", Icon::IslandNone),
];

/// The draw order list (HATCHEDIT `drawOrder`).
const ORDERS: [(&str, &str); 5] = [
    ("Do Not Change", "none"),
    ("Send to Back", "back"),
    ("Bring to Front", "front"),
    ("Send Behind Boundary", "behindBoundary"),
    ("Bring in Front of Boundary", "inFrontOfBoundary"),
];

/// The origin buttons (HATCHEDIT `originMode`).
const ORIGINS: [(&str, &str, Icon); 6] = [
    ("Top Left", "topLeft", Icon::OriginTopLeft),
    ("Top Right", "topRight", Icon::OriginTopRight),
    ("Bottom Left", "bottomLeft", Icon::OriginBottomLeft),
    ("Bottom Right", "bottomRight", Icon::OriginBottomRight),
    ("Center", "center", Icon::OriginCenter),
    ("Use Current Origin", "current", Icon::OriginCurrent),
];

/// ISO pen widths (millimetres) for the ACAD_ISO patterns: the pattern's scale.
const ISO_PENS: [f64; 9] = [0.13, 0.18, 0.25, 0.35, 0.5, 0.7, 1.0, 1.4, 2.0];

fn slide_out(ui: &mut egui::Ui, title: &'static str, pr: Rect, sh: &Shown, out: &mut Option<Action>) {
    let t = Tokens::get();
    let pin_id = egui::Id::new("rb_pinned_panel");
    let pinned = ui.ctx().data(|d| d.get_temp::<String>(pin_id)).as_deref() == Some(title);
    let width = match title {
        "Properties" => pr.width(),
        "Origin" => 290.0,
        "Options" => 280.0,
        _ => 240.0,
    };
    let at = pos2(pr.left(), pr.bottom());
    let mut close = false;
    let area = egui::Area::new(egui::Id::new(("rbh_slide", title))).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::NONE.fill(t.panel).stroke(Stroke::new(1.0, t.border)).inner_margin(egui::Margin::same(3)).show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
            match title {
                "Boundaries" => slide_boundaries(ui, sh, out),
                "Properties" => slide_properties(ui, sh, out),
                "Origin" => slide_origin(ui, sh, out),
                _ => slide_options(ui, sh, out),
            }
            // The title bar with the pin.
            let (bar, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
            ui.painter().text(bar.center(), Align2::CENTER_CENTER, hl(title), font(), t.text_dim);
            let pin = Rect::from_min_size(pos2(bar.left() + 2.0, bar.top() + 2.0), vec2(14.0, 14.0));
            let resp = ui.interact(pin, ui.id().with(("rbh_pin", title)), Sense::click());
            if pinned {
                ui.painter().rect_filled(pin, 2.0, t.accent.gamma_multiply(0.4));
            }
            icons::paint(ui.painter(), pin, Icon::Pin, false);
            if resp.on_hover_text(crate::tl!("Pin")).clicked() {
                ui.ctx().data_mut(|d| {
                    if pinned {
                        d.remove::<String>(pin_id);
                    } else {
                        d.insert_temp(pin_id, title.to_string());
                    }
                });
            }
        });
    });
    // An unpinned slide-out closes once a click lands elsewhere or a command is chosen in it.
    if !pinned && (area.response.clicked_elsewhere() || out.is_some()) && !ui.rect_contains_pointer(pr) {
        close = true;
    }
    if close {
        ui.ctx().data_mut(|d| d.remove::<String>(egui::Id::new("rb_open_panel")));
    }
}

/// One full-width row of a slide-out: icon and label; returns its response.
fn slide_button(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool, on: bool) -> egui::Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.0), if enabled { Sense::click() } else { Sense::hover() });
    if on {
        pressed(ui.painter(), r);
    }
    if enabled {
        hover_fill(ui, r, &resp);
    }
    icons::paint(ui.painter(), Rect::from_min_size(pos2(r.left() + 2.0, r.center().y - 8.0), vec2(16.0, 16.0)), icon, !enabled);
    ui.painter().text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, hl(label), font(), if enabled { t.text } else { t.text_faint });
    icons::describe_control(ui, &resp, label, on.then_some(true), false);
    resp
}

/// A full-width list row of a slide-out (icon, value, ▾).
fn slide_combo(ui: &mut egui::Ui, id: &str, icon: Icon, value: &str, enabled: bool) -> egui::Response {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.0), Sense::hover());
    combo(ui, r, id, Some(icon), None, value, enabled)
}

/// A split-button row of a slide-out (icon, the current choice, ▾ right after it), as Windows
/// CAD ribbons draw Island Detection and Draw Order.
fn slide_choice(ui: &mut egui::Ui, id: &str, icon: Icon, value: &str) -> egui::Response {
    let t = Tokens::get();
    let w = ui.painter().layout_no_wrap(value.to_string(), font(), t.text).size().x;
    let (r, resp) = ui.allocate_exact_size(vec2((w + 40.0).min(ui.available_width()), ROW - 2.0), Sense::click());
    hover_fill(ui, r, &resp);
    icons::paint(ui.painter(), Rect::from_min_size(pos2(r.left() + 2.0, r.center().y - 8.0), vec2(16.0, 16.0)), icon, false);
    ui.painter().text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, value, font(), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 30.0 + w, r.center().y), vec2(8.0, 8.0)), Icon::ChevronDown, false);
    icons::describe_control(ui, &resp, id, None, false);
    resp
}

/// The Retain Boundaries list (HPBOUNDRETAIN).
const RETAIN: [&str; 3] = ["Don't Retain Boundaries", "Retain Boundaries - Polyline", "Retain Boundaries - Region"];
/// The boundary set list.
const BOUNDARY_SETS: [&str; 2] = ["Use Current Viewport", "Select New Boundary Set"];

fn slide_boundaries(ui: &mut egui::Ui, sh: &Shown, out: &mut Option<Action>) {
    // Display Boundary Objects shows an existing hatch's boundary; the boundary-creation
    // choices apply to new hatches: each tab offers its own, as Windows CAD ribbons do.
    if slide_button(ui, Icon::DisplayBoundary, "Display Boundary Objects", !sh.create, false).clicked() && !sh.create {
        *out = Some(Action::Start("hatchedit.showboundary"));
    }
    let resp = slide_combo(ui, "Retain Boundaries", Icon::HatchRecreate, hl(RETAIN[sh.retain.min(2)]), sh.create);
    let items: Vec<String> = RETAIN.iter().map(|l| hl(l).to_string()).collect();
    if sh.create
        && let Some(i) = menu(&resp, 260.0, &items, Some(sh.retain))
    {
        *out = Some(setvar("HPBOUNDRETAIN", json!(i)));
    }
    let shown_set = usize::from(sh.custom_set);
    let resp = slide_combo(ui, "Boundary Set", Icon::HatchSelect, hl(BOUNDARY_SETS[shown_set]), sh.create);
    let items: Vec<String> = BOUNDARY_SETS.iter().map(|l| hl(l).to_string()).collect();
    if sh.create
        && let Some(i) = menu(&resp, 260.0, &items, Some(shown_set))
    {
        *out = Some(Action::Do(Box::new(move |app: &mut CadApp| {
            if i == 0 {
                app.session.set_hatch_boundary_set(None);
            } else {
                keyword(app, "new boundary Set");
            }
        })));
    }
}

/// A keyword for the running HATCH (the Hatch Creation tab's buttons).
pub fn keyword(app: &mut CadApp, k: &str) {
    if let Err(e) = app.session.input(cadcraft_engine::Input::Keyword(k.to_string())) {
        app.session.echo(e.to_string());
    }
}

fn slide_properties(ui: &mut egui::Ui, sh: &Shown, out: &mut Option<Action>) {
    let h = &sh.hatch;
    let use_current = hl("Use Current").to_string();
    let mut layers = vec![use_current];
    layers.extend(sh.layers.iter().cloned());
    let (r, _) = ui.allocate_exact_size(vec2(240.0_f32.min(ui.available_width()), ROW - 2.0), Sense::hover());
    let resp = combo(ui, r, "Hatch Layer Override", Some(Icon::HatchLayer), Some(sh.layer_color), &sh.layer, true);
    if let Some(i) = menu(&resp, 240.0, &layers, sh.layers.iter().position(|l| *l == sh.layer).map(|i| i + 1)) {
        match sh.layers.get(i.wrapping_sub(1)) {
            Some(l) => *out = Some(sh.set(json!({ "layer": l }))),
            // Use Current: new hatches go on the current layer (HPLAYER ".").
            None if sh.create => *out = Some(sh.set(json!({ "layer": "." }))),
            None => {}
        }
    }
    slide_button(ui, Icon::PaperSpace, "Relative to Paper Space", false, false)
        .on_hover_text(crate::tl!("not available yet"))
        .on_disabled_hover_text(if sh.paper { "" } else { "Model space" });
    let user = h.is_user();
    if slide_button(ui, Icon::HatchDouble, "Double", user, h.is_double()).clicked() && user {
        *out = Some(sh.edit(json!({ "double": !h.is_double() })));
    }
    let iso = !h.solid && h.pattern.to_ascii_uppercase().starts_with("ACAD_ISO");
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.0), Sense::hover());
    let t = Tokens::get();
    let lw = text_w(ui, "ISO Pen Width:") + 8.0;
    ui.painter().text(pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, hl("ISO Pen Width:"), font(), if iso { t.text } else { t.text_faint });
    let field = Rect::from_min_max(pos2(r.left() + lw, r.top()), pos2((r.left() + lw + 90.0).min(r.right()), r.bottom()));
    let value = if iso { format!("{:.2} mm", h.scale) } else { String::new() };
    let resp = combo(ui, field, "ISO Pen Width", None, None, &value, iso);
    let items: Vec<String> = ISO_PENS.iter().map(|w| format!("{w:.2} mm")).collect();
    if let Some(i) = menu(&resp, 100.0, &items, ISO_PENS.iter().position(|w| (w - h.scale).abs() < 1e-9))
        && let Some(w) = ISO_PENS.get(i)
    {
        *out = Some(sh.edit(json!({ "scale": w })));
    }
}

fn slide_origin(ui: &mut egui::Ui, sh: &Shown, out: &mut Option<Action>) {
    ui.horizontal(|ui| {
        for (label, mode, icon) in ORIGINS {
            let resp = icons::button(ui, icon, 20.0, hl(label), false);
            icons::describe_control(ui, &resp, label, None, false);
            if resp.clicked() {
                *out = Some(sh.edit(json!({ "originMode": mode, "storeOrigin": sh.store_origin })));
            }
        }
    });
    if slide_button(ui, Icon::StoreOrigin, "Store as Default Origin", true, sh.store_origin).clicked() {
        let on = !sh.store_origin;
        *out = Some(Action::Do(Box::new(move |app: &mut CadApp| app.session.hatch_store_origin = on)));
    }
}

fn slide_options(ui: &mut egui::Ui, sh: &Shown, out: &mut Option<Action>) {
    let h = &sh.hatch;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW - 2.0), Sense::hover());
    let lw = text_w(ui, "Gap Tolerance") + 14.0;
    let t = Tokens::get();
    ui.painter().text(pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, hl("Gap Tolerance"), font(), t.text);
    let field = Rect::from_min_max(pos2(r.left() + lw, r.top()), pos2((r.left() + lw + 70.0).min(r.right()), r.bottom()));
    if let Some(v) = value_box(ui, field, "Gap Tolerance", sh.gap, 4, (0.0, 5000.0), true, false) {
        *out = Some(setvar("HPGAPTOL", json!(v)));
    }
    let areas = h.loops.iter().filter(|l| l.outer).count();
    if sh.create {
        // A toggle for the hatches being made (HPSEPARATE).
        if slide_button(ui, Icon::SeparateHatches, "Separate Hatches", true, sh.separate).clicked() {
            *out = Some(sh.edit(json!({ "separate": !sh.separate })));
        }
    } else if slide_button(ui, Icon::SeparateHatches, "Separate Hatches", areas > 1, false).clicked() && areas > 1 {
        *out = Some(sh.edit(json!({ "separate": true })));
    }
    let style = usize::from(h.style).min(2);
    let (label, _, icon) = ISLANDS[style];
    let resp = slide_choice(ui, "Island Detection", icon, hl(label));
    let items: Vec<String> = ISLANDS.iter().map(|(l, _, _)| hl(l).to_string()).collect();
    if let Some(i) = menu(&resp, 260.0, &items, Some(style))
        && let Some((_, v, _)) = ISLANDS.get(i)
    {
        *out = Some(sh.edit(json!({ "style": v })));
    }
    let order = if sh.create { sh.draw_order.min(ORDERS.len() - 1) } else { 0 };
    let resp = slide_choice(ui, "Draw Order", Icon::HatchDrawOrder, hl(ORDERS[order].0));
    let items: Vec<String> = ORDERS.iter().map(|(l, _)| hl(l).to_string()).collect();
    if let Some(i) = menu(&resp, 260.0, &items, Some(order))
        && (i > 0 || sh.create)
        && let Some((_, v)) = ORDERS.get(i)
    {
        *out = Some(sh.edit(json!({ "drawOrder": v })));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gallery_starts_with_solid_and_places_the_gradients_after_the_g_patterns() {
        let g = patterns::gallery();
        assert_eq!(g.first().map(String::as_str), Some("SOLID"));
        let lin = g.iter().position(|n| n == "GR_LINEAR").unwrap();
        assert_eq!(&g[lin..lin + 9], patterns::GRADIENTS.map(|(t, _)| t.to_string()).as_slice());
        assert!(g.iter().position(|n| n == "GRASS").unwrap() > lin, "GRASS sorts after GR_");
        assert!(g.iter().position(|n| n == "EARTH").unwrap() < lin);
    }

    #[test]
    fn type_of_a_hatch() {
        let mut h = Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops: Vec::new(),
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: cadcraft_engine::geom::Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        };
        assert_eq!(type_of(&h), 0);
        h.solid = false;
        h.pattern = "ANSI31".into();
        assert_eq!(type_of(&h), 2);
        h.pattern = "_USER".into();
        assert_eq!(type_of(&h), 3);
    }
}
