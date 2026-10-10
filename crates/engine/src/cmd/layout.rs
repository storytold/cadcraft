//! Layouts, paper-space viewports, page setup and plotting: LAYOUT (new/delete/rename/copy),
//! MVIEW / VPORTS, viewport scale/lock/layer settings, PAGESETUP, PLOT and EXPORTPDF.
//!
//! Plotting goes through the host's [`super::file::IoHooks::plot`] hook so the engine stays
//! I/O-agnostic.

use cadcraft_doc::{Drawing, Entity, EntityKind, EntityStore, Handle, Layout, PageSetup, Space, Viewport};
use cadcraft_geom::{Bounds2, Vec2};
use serde_json::{Value, json};

use super::file::{base64_encode, io};
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("layout", "Layout", run_layout)
            .alias(&["lo"])
            .params("{option: new|copy|delete|rename|set|list, name?, to?}"),
        CommandSpec::new("layout.new", "New Layout", run_new)
            .menu(&["Insert", "Layout", "New Layout"])
            .params("{name?, viewport?: bool (default true)} → {name, viewport}")
            .interactive(|_| Ok(Box::new(NewLayoutM))),
        CommandSpec::new("layout.delete", "Delete Layout", run_delete).params("{name}"),
        CommandSpec::new("layout.rename", "Rename Layout", run_rename).params("{from?: current layout, to}"),
        CommandSpec::new("layout.copy", "Copy Layout", run_copy).params("{from, to?}"),
        CommandSpec::new("layout.list", "List Layouts", run_list).params("→ [{name, tabOrder, page, viewports}]").noundo(),
        CommandSpec::new("mview", "Viewports", run_mview)
            .alias(&["mv"])
            .params("{p1?, p2? (default: printable area), count?: 1-4, arrangement?: vertical|horizontal|left|right|above|below, layout?}")
            .interactive(|_| Ok(Box::new(MviewM::new(1, true)))),
        CommandSpec::new("vports", "New Viewports...", run_mview)
            .menu(&["View", "Viewports", "New Viewports..."])
            .params("{count?: 1-4, arrangement?, p1?, p2?, layout?}")
            .interactive(|_| Ok(Box::new(MviewM::new(1, true)))),
        CommandSpec::new("vports.1", "1 Viewport", |s, p| run_mview_n(s, p, 1))
            .menu(&["View", "Viewports", "1 Viewport"])
            .params("{p1?, p2?, layout?}")
            .interactive(|_| Ok(Box::new(MviewM::new(1, false)))),
        CommandSpec::new("vports.2", "2 Viewports", |s, p| run_mview_n(s, p, 2))
            .menu(&["View", "Viewports", "2 Viewports"])
            .params("{p1?, p2?, arrangement?: vertical|horizontal, layout?}")
            .interactive(|_| Ok(Box::new(MviewM::new(2, false)))),
        CommandSpec::new("vports.3", "3 Viewports", |s, p| run_mview_n(s, p, 3))
            .menu(&["View", "Viewports", "3 Viewports"])
            .params("{p1?, p2?, arrangement?: right|left|above|below|vertical|horizontal, layout?}")
            .interactive(|_| Ok(Box::new(MviewM::new(3, false)))),
        CommandSpec::new("vports.4", "4 Viewports", |s, p| run_mview_n(s, p, 4))
            .menu(&["View", "Viewports", "4 Viewports"])
            .params("{p1?, p2?, layout?}")
            .interactive(|_| Ok(Box::new(MviewM::new(4, false)))),
        CommandSpec::new("viewport.set", "Viewport Properties", run_viewport_set).params(
            "{handle? (default: selected viewports), scale?: paper units per model unit | \"1:50\", viewHeight?, center?: [x,y], locked?, freeze?: [layer], thaw?: [layer], colors?: {layer: color | null}}",
        ),
        CommandSpec::new("vplayer", "Viewport Layer Freeze", run_viewport_set)
            .params("{handle?, freeze?: [layer], thaw?: [layer], colors?: {layer: color (\"red\" | 1..255 | \"r,g,b\") | null to clear}}"),
        CommandSpec::new("pagesetup", "Page Setup Manager...", run_pagesetup)
            .menu(&["File", "Page Setup Manager..."])
            .params(
                "{layout?: current, paper?: \"A4\"|\"A3\"|\"Letter\"|\"ANSI B\"|…, width?, height? (mm), landscape?, margins?: [l,b,r,t] mm, lineweights?, plotArea?, scale?, scaleToFit?, center?, plotStyleTable?}",
            ),
        CommandSpec::new("plot", "Print...", |s, p| run_plot(s, p, "plot"))
            .menu(&["File", "Print..."])
            .key("Cmd+P")
            .alias(&["print"])
            .params("{path? (else returns base64 `data`), layout?: current|\"Model\", paper?, landscape?, fit?: bool, scale?: number | \"1:50\", lineweights?: bool, plotArea?: extents|display|limits|window (Model) | layout, window?: [[x1,y1],[x2,y2]]} (typed or from the menu: the Plot dialog)")
            .noundo(),
        CommandSpec::new("exportpdf", "Export to PDF...", |s, p| run_plot(s, p, "exportpdf"))
            .menu(&["File", "Export to PDF..."])
            .params("{path? (else returns base64 `data`), layout?, paper?, landscape?, fit?, scale?, lineweights?, plotArea?, window?} (as PLOT)")
            .noundo(),
        CommandSpec::new("mspace", "Model Space (in viewport)", run_mspace)
            .alias(&["ms"])
            .params("{handle?: viewport, at?: [x,y] paper point inside a viewport} (default: the last active or first viewport)")
            .noundo(),
        CommandSpec::new("pspace", "Paper Space", run_pspace).alias(&["ps"]).noundo(),
    ]
}

// ---------- model space through viewports ----------

fn layout_viewports(s: &Session) -> Result<Vec<(Handle, Viewport)>> {
    let sp = s.layout_space();
    if sp == Space::Model {
        return Err(bad("mspace", "** Command not allowed in Model Tab **"));
    }
    let d = s.doc()?;
    Ok(d.space(&sp)
        .map(|st| {
            st.iter()
                .filter_map(|e| match &e.kind {
                    EntityKind::Viewport(v) if v.id != 1 && v.height > 1e-12 && v.view_height > 1e-12 => Some((e.handle, v.clone())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default())
}

fn run_mspace(s: &mut Session, p: &Value) -> Result<Value> {
    let vps = layout_viewports(s)?;
    let at = p.get("at").and_then(|v| Some(Vec2::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?)));
    let want = p.get("handle").and_then(|v| v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle)));
    let pick = if let Some(h) = want {
        vps.iter().find(|(x, _)| *x == h).map(|(h, _)| *h).ok_or_else(|| bad("mspace", "`handle` is not a viewport in this layout"))?
    } else if let Some(a) = at {
        vps.iter()
            .rev()
            .find(|(_, v)| (a.x - v.center.x).abs() <= v.width / 2.0 && (a.y - v.center.y).abs() <= v.height / 2.0)
            .map(|(h, _)| *h)
            .ok_or_else(|| bad("mspace", "no viewport at that point"))?
    } else {
        let last = s.state()?.mspace;
        match last.filter(|h| vps.iter().any(|(x, _)| x == h)).or_else(|| vps.first().map(|(h, _)| *h)) {
            Some(h) => h,
            None => return Err(bad("mspace", "There are no active model space viewports.")),
        }
    };
    let st = s.state_mut()?;
    st.mspace = Some(pick);
    st.selection.clear();
    s.echo("MSPACE");
    Ok(json!({ "viewport": pick.hex() }))
}

fn run_pspace(s: &mut Session, _p: &Value) -> Result<Value> {
    if s.layout_space() == Space::Model {
        return Err(bad("pspace", "** Command not allowed in Model Tab **"));
    }
    let st = s.state_mut()?;
    st.mspace = None;
    st.selection.clear();
    Ok(json!({ "space": "paper" }))
}

// ---------- layouts ----------

/// Characters AutoCAD does not allow in layout names.
const BAD_CHARS: &[char] = &['<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`'];

fn check_name(cmd: &str, d: &Drawing, name: &str, except: Option<&str>) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(bad(cmd, "layout name is empty"));
    }
    if n.chars().count() > 255 {
        return Err(bad(cmd, "layout name is longer than 255 characters"));
    }
    if n.contains(BAD_CHARS) || n.chars().any(char::is_control) {
        return Err(bad(cmd, format!("invalid layout name `{n}`")));
    }
    if n.eq_ignore_ascii_case("model") {
        return Err(bad(cmd, "`Model` is reserved"));
    }
    if d.layouts.iter().any(|l| l.name.eq_ignore_ascii_case(n) && except.is_none_or(|e| !l.name.eq_ignore_ascii_case(e))) {
        return Err(bad(cmd, format!("layout `{n}` already exists")));
    }
    Ok(n.to_string())
}

fn layout_name(cmd: &str, d: &Drawing, name: &str) -> Result<String> {
    d.layout(name).map(|l| l.name.clone()).ok_or_else(|| bad(cmd, format!("no layout `{name}`")))
}

fn next_layout_name(d: &Drawing) -> String {
    (1..=10_000).map(|i| format!("Layout{i}")).find(|n| d.layout(n).is_none()).unwrap_or_else(|| "Layout".into())
}

/// The page setup new layouts get: ANSI A landscape (imperial) or ISO A4 landscape (metric).
fn default_page(d: &Drawing) -> PageSetup {
    let mut p = PageSetup::default();
    if cadcraft_render::paper::paper_unit_mm(d) == 1.0
        && let Some(a4) = cadcraft_render::paper_size("A4")
    {
        p.paper = a4.name.into();
        p.width_mm = a4.width_mm;
        p.height_mm = a4.height_mm;
    }
    p
}

/// LAYOUT.NEW from the menu or command line: asks for the name (Enter takes the next free one).
struct NewLayoutM;

impl Interactive for NewLayoutM {
    fn name(&self) -> &'static str {
        "LAYOUT"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let next = s.doc().map(next_layout_name).unwrap_or_else(|_| "Layout1".into());
        Prompt::new("Enter name of new layout", Accept::TEXT).default(next)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        let p = match i {
            Input::Text(t) if !t.trim().is_empty() => json!({ "name": t.trim() }),
            Input::Text(_) | Input::Enter => json!({}),
            _ => return Ok(Step::Continue),
        };
        // A taken or invalid name is an error: the session reports it and asks again.
        run_new(s, &p)?;
        Ok(Step::Done)
    }
}

fn run_new(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let name = match str_param(p, "name") {
        Some(n) => check_name("layout.new", d, n, None)?,
        None => next_layout_name(d),
    };
    let page = default_page(d);
    let order = d.layouts.iter().map(|l| l.tab_order).max().unwrap_or(0).saturating_add(1);
    s.doc_mut()?.layouts.push(Layout { page, ..Layout::new(&name, order) });
    let mut vp = Value::Null;
    if bool_or(p, "viewport", true) {
        // AutoCAD behaviour: one viewport over the printable area, zoomed to the model extents.
        let space = Space::Paper(name.clone());
        let rect = printable(s, &name)?;
        if let Some(h) = add_viewports(s, &space, rect, 1, "")?.first() {
            vp = json!(h.hex());
        }
    }
    Ok(json!({ "name": name, "viewport": vp }))
}

fn run_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad("layout.delete", "`name` is required"))?;
    let name = layout_name("layout.delete", s.doc()?, name)?;
    if s.doc()?.layouts.len() <= 1 {
        return Err(bad("layout.delete", "a drawing must keep at least one layout"));
    }
    s.doc_mut()?.layouts.retain(|l| l.name != name);
    let gone = Space::Paper(name.clone());
    let st = s.state_mut()?;
    st.views.retain(|(sp, _)| sp != &gone);
    if st.space == gone {
        st.space = Space::Model;
        st.mspace = None;
        st.selection.clear();
    }
    s.touch();
    Ok(json!({ "deleted": name }))
}

fn run_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let from = match (str_param(p, "from"), s.layout_space()) {
        (Some(f), _) => f.to_string(),
        (None, Space::Paper(n)) => n,
        (None, Space::Model) => return Err(bad("layout.rename", "`from` is required in the Model tab")),
    };
    let from = layout_name("layout.rename", s.doc()?, &from)?;
    let to = str_param(p, "to").ok_or_else(|| bad("layout.rename", "`to` is required"))?;
    let to = check_name("layout.rename", s.doc()?, to, Some(&from))?;
    if let Some(l) = s.doc_mut()?.layouts.iter_mut().find(|l| l.name == from) {
        l.name = to.clone();
    }
    let (old, new) = (Space::Paper(from.clone()), Space::Paper(to.clone()));
    let st = s.state_mut()?;
    for (sp, _) in st.views.iter_mut() {
        if *sp == old {
            *sp = new.clone();
        }
    }
    if st.space == old {
        st.space = new;
    }
    s.touch();
    Ok(json!({ "from": from, "to": to }))
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value> {
    let from = str_param(p, "from").ok_or_else(|| bad("layout.copy", "`from` is required"))?;
    let from = layout_name("layout.copy", s.doc()?, from)?;
    let to = match str_param(p, "to") {
        Some(t) => check_name("layout.copy", s.doc()?, t, None)?,
        None => {
            let d = s.doc()?;
            (2..=10_000).map(|i| format!("{from} ({i})")).find(|n| d.layout(n).is_none()).ok_or_else(|| bad("layout.copy", "no free name"))?
        }
    };
    let d = s.doc_mut()?;
    let Some(src) = d.layouts.iter().find(|l| l.name == from).cloned() else { return Err(bad("layout.copy", "no such layout")) };
    let order = d.layouts.iter().map(|l| l.tab_order).max().unwrap_or(0).saturating_add(1);
    let mut store = EntityStore::new();
    for e in src.entities.iter() {
        let h = d.new_handle();
        store.push(Entity { handle: h, ..Entity::clone(e) });
    }
    d.layouts.push(Layout { name: to.clone(), tab_order: order, page: src.page, entities: store, view: src.view });
    Ok(json!({ "name": to }))
}

fn run_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let mut ls: Vec<&Layout> = d.layouts.iter().collect();
    ls.sort_by_key(|l| l.tab_order);
    let current = s.layout_space();
    Ok(Value::Array(
        ls.iter()
            .map(|l| {
                json!({
                    "name": l.name,
                    "tabOrder": l.tab_order,
                    "current": current == Space::Paper(l.name.clone()),
                    "page": serde_json::to_value(&l.page).unwrap_or(Value::Null),
                    "viewports": l.entities.iter().filter(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.id != 1)).map(|e| e.handle.hex()).collect::<Vec<_>>(),
                })
            })
            .collect(),
    ))
}

/// `-LAYOUT`-style dispatcher.
fn run_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let opt = str_param(p, "option").unwrap_or("list").to_ascii_lowercase();
    match opt.as_str() {
        "new" | "n" => run_new(s, p),
        "copy" | "c" => {
            let mut q = p.clone();
            if q.get("from").is_none()
                && let (Some(n), Some(o)) = (p.get("name").cloned(), q.as_object_mut())
            {
                o.insert("from".into(), n);
            }
            run_copy(s, &q)
        }
        "delete" | "d" => run_delete(s, p),
        "rename" | "r" => {
            let mut q = p.clone();
            if q.get("from").is_none()
                && let (Some(n), Some(o)) = (p.get("name").cloned(), q.as_object_mut())
            {
                o.insert("from".into(), n);
            }
            run_rename(s, &q)
        }
        "set" | "s" => {
            let set = find_command("layout.set").ok_or_else(|| bad("layout", "layout.set is missing"))?;
            (set.run)(s, p)
        }
        "list" | "?" => run_list(s, p),
        _ => Err(bad("layout", "option must be new, copy, delete, rename, set or list")),
    }
}

// ---------- viewports ----------

/// The printable area of a layout, in paper units.
fn printable(s: &Session, layout: &str) -> Result<Bounds2> {
    cadcraft_render::sheet(s.doc()?, layout).map(|sh| sh.printable).ok_or_else(|| bad("mview", format!("no layout `{layout}`")))
}

/// The layout a viewport command acts on: `layout` param or the current layout.
fn target_layout(s: &Session, cmd: &str, p: &Value) -> Result<Space> {
    if let Some(n) = str_param(p, "layout") {
        return Ok(Space::Paper(layout_name(cmd, s.doc()?, n)?));
    }
    match s.layout_space() {
        Space::Model => Err(bad(cmd, "** Command not allowed in Model Tab ** (switch to a layout or pass `layout`)")),
        sp => Ok(sp),
    }
}

/// Split a rectangle into `count` viewport rectangles (AutoCAD's VPORTS arrangements).
fn split(r: Bounds2, count: usize, arrangement: &str) -> Vec<Bounds2> {
    let (x0, y0, x1, y1) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let xm = (x0 + x1) / 2.0;
    let ym = (y0 + y1) / 2.0;
    let b = |a: f64, b: f64, c: f64, d: f64| Bounds2::new(Vec2::new(a, b), Vec2::new(c, d));
    let a = arrangement.to_ascii_lowercase();
    match count {
        2 if a.starts_with('h') => vec![b(x0, ym, x1, y1), b(x0, y0, x1, ym)],
        2 => vec![b(x0, y0, xm, y1), b(xm, y0, x1, y1)],
        3 => {
            let (t1, t2) = (x0 + (x1 - x0) / 3.0, x0 + 2.0 * (x1 - x0) / 3.0);
            let (u1, u2) = (y0 + (y1 - y0) / 3.0, y0 + 2.0 * (y1 - y0) / 3.0);
            match a.as_str() {
                "left" | "l" => vec![b(x0, y0, xm, y1), b(xm, ym, x1, y1), b(xm, y0, x1, ym)],
                "above" | "a" => vec![b(x0, ym, x1, y1), b(x0, y0, xm, ym), b(xm, y0, x1, ym)],
                "below" | "b" => vec![b(x0, y0, x1, ym), b(x0, ym, xm, y1), b(xm, ym, x1, y1)],
                "vertical" | "v" => vec![b(x0, y0, t1, y1), b(t1, y0, t2, y1), b(t2, y0, x1, y1)],
                "horizontal" | "h" => vec![b(x0, u2, x1, y1), b(x0, u1, x1, u2), b(x0, y0, x1, u1)],
                // Right (the default): two stacked on the left, the large one on the right.
                _ => vec![b(x0, ym, xm, y1), b(x0, y0, xm, ym), b(xm, y0, x1, y1)],
            }
        }
        4 => vec![b(x0, ym, xm, y1), b(xm, ym, x1, y1), b(x0, y0, xm, ym), b(xm, y0, x1, ym)],
        _ => vec![r],
    }
}

/// View height that shows `ext` in a viewport of the given size (zoom extents).
fn fit_view(d: &Drawing, w: f64, h: f64) -> (Vec2, f64) {
    let mut ext = d.extents(&Space::Model);
    if ext.is_empty() {
        let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
        let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
        ext = Bounds2::new(lo, hi);
    }
    let aspect = (w / h.max(1e-12)).max(1e-12);
    let vh = (ext.height().max(ext.width() / aspect) * 1.02).max(1e-9);
    let c = ext.center();
    if vh.is_finite() && c.is_finite() { (c, vh) } else { (Vec2::ZERO, 1.0) }
}

/// Add `count` viewports tiling `rect` in a layout, each zoomed to the model extents.
fn add_viewports(s: &mut Session, space: &Space, rect: Bounds2, count: usize, arrangement: &str) -> Result<Vec<Handle>> {
    if !(1..=4).contains(&count) {
        return Err(bad("mview", "count must be 1, 2, 3 or 4"));
    }
    if rect.is_empty() || !rect.min.is_finite() || !rect.max.is_finite() || rect.width() < 1e-6 || rect.height() < 1e-6 {
        return Err(bad("mview", "the viewport has no area"));
    }
    if rect.width() > 1e7 || rect.height() > 1e7 {
        return Err(bad("mview", "the viewport is too large"));
    }
    let common = s.current_common();
    let d = s.doc_mut()?;
    let mut id = d
        .layouts
        .iter()
        .flat_map(|l| l.entities.iter())
        .filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some(v.id) } else { None })
        .max()
        .unwrap_or(1)
        .max(1);
    let mut out = Vec::new();
    // Layers marked "New VP Freeze" start frozen in new viewports.
    let vp_frozen: Vec<String> = d.layers.iter().filter(|l| l.vp_freeze_new).map(|l| l.name.clone()).collect();
    for r in split(rect, count, arrangement) {
        let (view_center, view_height) = fit_view(d, r.width(), r.height());
        id = id.saturating_add(1);
        let vp = Viewport {
            center: r.center().to3(0.0),
            width: r.width(),
            height: r.height(),
            view_center,
            view_height,
            id,
            locked: false,
            frozen_layers: vp_frozen.clone(),
            layer_colors: Vec::new(),
        };
        out.push(d.add(space, common.clone(), EntityKind::Viewport(vp))?);
    }
    Ok(out)
}

fn mview(s: &mut Session, p: &Value, count: usize, cmd: &str) -> Result<Value> {
    let space = target_layout(s, cmd, p)?;
    let Space::Paper(name) = &space else { return Err(bad(cmd, "not a layout")) };
    let rect = match (point_param(p, "p1"), point_param(p, "p2")) {
        (Some(a), Some(b)) => Bounds2::new(a, b),
        _ => printable(s, name)?,
    };
    let arrangement = str_param(p, "arrangement").unwrap_or("");
    let hs = add_viewports(s, &space, rect, count, arrangement)?;
    Ok(json!({ "viewports": hs.iter().map(|h| h.hex()).collect::<Vec<_>>() }))
}

fn run_mview(s: &mut Session, p: &Value) -> Result<Value> {
    let count = match p.get("count") {
        None | Some(Value::Null) => 1,
        Some(v) => v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("mview", "count must be 1, 2, 3 or 4"))?,
    };
    mview(s, p, count, "mview")
}

fn run_mview_n(s: &mut Session, p: &Value, n: usize) -> Result<Value> {
    mview(s, p, n, "vports")
}

/// MVIEW / VPORTS prompts: first corner (or Fit, or a viewport count), opposite corner.
struct MviewM {
    count: usize,
    /// Offer the 2/3/4 keywords (MVIEW) rather than a fixed count (View > Viewports > n).
    choose: bool,
    first: Option<Vec2>,
}

impl MviewM {
    fn new(count: usize, choose: bool) -> Self {
        MviewM { count, choose, first: None }
    }
    fn create(&self, s: &mut Session, rect: Option<Bounds2>) -> Result<Step> {
        let mut p = json!({ "count": self.count });
        if let (Some(r), Some(o)) = (rect, p.as_object_mut()) {
            o.insert("p1".into(), json!([r.min.x, r.min.y]));
            o.insert("p2".into(), json!([r.max.x, r.max.y]));
        }
        let r = mview(s, &p, self.count, "mview")?;
        let n = r.get("viewports").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
        s.echo(format!("{n} viewport(s) created, zoomed to the model extents."));
        Ok(Step::Done)
    }
}

impl Interactive for MviewM {
    fn name(&self) -> &'static str {
        "MVIEW"
    }
    fn begin(&mut self, s: &mut Session) -> Result<Step> {
        if s.space() == Space::Model {
            s.echo("** Command not allowed in Model Tab ** (switch to a layout first)");
            return Ok(Step::Done);
        }
        Ok(Step::Continue)
    }
    fn prompt(&self, _s: &Session) -> Prompt {
        match self.first {
            None if self.choose && self.count == 1 => {
                Prompt::new("Specify corner of viewport", Accept::POINT).kw(&["Fit", "2", "3", "4"]).default("Fit")
            }
            None => Prompt::new("Specify first corner", Accept::POINT).kw(&["Fit"]).default("Fit"),
            Some(a) => Prompt::new("Specify opposite corner", Accept::POINT).base(a),
        }
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match (self.first, i) {
            (None, Input::Enter) => self.create(s, None),
            (None, Input::Keyword(k)) | (None, Input::Text(k)) => match k.trim().to_ascii_lowercase().as_str() {
                "fit" | "f" => self.create(s, None),
                n @ ("2" | "3" | "4") if self.choose => {
                    self.count = n.parse().unwrap_or(1);
                    Ok(Step::Continue)
                }
                _ => Err(crate::EngineError::Other("Requires a point or option keyword.".into())),
            },
            (None, Input::Point(p)) => {
                self.first = Some(p);
                Ok(Step::Continue)
            }
            (Some(a), Input::Point(b)) => self.create(s, Some(Bounds2::new(a, b))),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _s: &Session, c: Vec2) -> Vec<EntityKind> {
        let Some(a) = self.first else { return Vec::new() };
        split(Bounds2::new(a, c), self.count, "")
            .into_iter()
            .map(|r| super::helpers::lwpoly(super::helpers::rect_vertices(r.min, r.max), true))
            .collect()
    }
}

/// A plot scale: a number (paper units per model unit) or `"a:b"` / `"a/b"`.
fn parse_scale(v: &Value) -> Option<f64> {
    let s = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(t) => {
            let t = t.trim();
            match t.split_once([':', '/']) {
                Some((a, b)) => {
                    let a: f64 = a.trim().parse().ok()?;
                    let b: f64 = b.trim().parse().ok()?;
                    Some(a / b)
                }
                None => t.trim_end_matches(['x', 'X']).parse().ok(),
            }
        }
        _ => None,
    }?;
    (s.is_finite() && s > 0.0).then_some(s)
}

fn layer_list(p: &Value, key: &str) -> Vec<String> {
    match p.get(key) {
        Some(Value::String(s)) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).take(10_000).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).take(10_000).collect(),
        _ => Vec::new(),
    }
}

fn viewport_json(h: Handle, v: &Viewport) -> Value {
    json!({
        "handle": h.hex(),
        "id": v.id,
        "center": [v.center.x, v.center.y],
        "width": v.width,
        "height": v.height,
        "viewCenter": [v.view_center.x, v.view_center.y],
        "viewHeight": v.view_height,
        "scale": if v.view_height > 0.0 { v.height / v.view_height } else { 0.0 },
        "locked": v.locked,
        "frozenLayers": v.frozen_layers,
        "layerColors": v.layer_colors.iter().map(|(l, c)| json!({ "layer": l, "color": c.name() })).collect::<Vec<_>>(),
    })
}

fn run_viewport_set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "viewport.set";
    let d = s.doc()?;
    let mut hs: Vec<Handle> =
        targets(s, p)?.into_iter().filter(|h| d.entity(*h).is_some_and(|e| matches!(&e.kind, EntityKind::Viewport(v) if v.id != 1))).collect();
    // Inside a viewport (MSPACE) the active viewport is the default.
    if hs.is_empty()
        && p.get("handle").is_none()
        && p.get("handles").is_none()
        && let Some((h, _)) = s.state()?.active_viewport()
    {
        hs.push(h);
    }
    if hs.is_empty() {
        return Err(bad(cmd, "no viewport given (pass `handle` or select a viewport)"));
    }
    let scale = match p.get("scale") {
        Some(v) => Some(parse_scale(v).ok_or_else(|| bad(cmd, "`scale` must be a positive number or \"a:b\""))?),
        None => None,
    };
    let view_height = match p.get("viewHeight") {
        Some(v) => Some(v.as_f64().filter(|x| x.is_finite() && *x > 0.0).ok_or_else(|| bad(cmd, "`viewHeight` must be positive"))?),
        None => None,
    };
    let center = match p.get("center") {
        Some(v) => Some(point_value(v).ok_or_else(|| bad(cmd, "`center` must be [x, y]"))?),
        None => None,
    };
    let locked = p.get("locked").and_then(Value::as_bool);
    let freeze = layer_list(p, "freeze");
    let thaw = layer_list(p, "thaw");
    // Per-viewport layer colours: {layer: colour} (null clears the override).
    let mut colors: Vec<(String, Option<cadcraft_color::Color>)> = Vec::new();
    if let Some(m) = p.get("colors").and_then(Value::as_object) {
        for (layer, c) in m.iter().take(10_000) {
            let col = match c {
                Value::Null => None,
                Value::String(t) => Some(cadcraft_color::Color::parse(t).ok_or_else(|| bad(cmd, format!("bad colour for `{layer}`")))?),
                Value::Number(n) => Some(
                    n.as_u64()
                        .and_then(|i| u8::try_from(i).ok())
                        .filter(|i| *i > 0)
                        .map(cadcraft_color::Color::Index)
                        .ok_or_else(|| bad(cmd, "colour index 1..255"))?,
                ),
                _ => return Err(bad(cmd, "`colors` values are colours or null")),
            };
            if matches!(col, Some(cadcraft_color::Color::ByLayer | cadcraft_color::Color::ByBlock)) {
                return Err(bad(cmd, "a viewport layer colour must be an index or true colour"));
            }
            colors.push((layer.clone(), col));
        }
    }
    let changes_view = scale.is_some() || view_height.is_some() || center.is_some();
    let mut out = Vec::new();
    for h in hs {
        let Some(EntityKind::Viewport(cur)) = s.doc()?.entity(h).map(|e| e.kind.clone()) else { continue };
        if changes_view && cur.locked && locked != Some(false) {
            return Err(bad(cmd, format!("viewport {} is locked", h.hex())));
        }
        let mut v = cur;
        if let Some(sc) = scale {
            let vh = v.height / sc;
            if vh.is_finite() && vh > 0.0 {
                v.view_height = vh;
            }
        }
        if let Some(vh) = view_height {
            v.view_height = vh;
        }
        if let Some(c) = center {
            v.view_center = c;
        }
        if let Some(l) = locked {
            v.locked = l;
        }
        for f in &freeze {
            if !v.frozen_layers.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                v.frozen_layers.push(f.clone());
            }
        }
        v.frozen_layers.retain(|x| !thaw.iter().any(|t| t.eq_ignore_ascii_case(x)));
        for (layer, c) in &colors {
            v.layer_colors.retain(|(n, _)| !n.eq_ignore_ascii_case(layer));
            if let Some(c) = c {
                v.layer_colors.push((layer.clone(), *c));
            }
        }
        out.push(viewport_json(h, &v));
        s.doc_mut()?.modify_entity(h, |e| e.kind = EntityKind::Viewport(v)).map_err(|e| bad(cmd, e.to_string()))?;
    }
    Ok(Value::Array(out))
}

// ---------- page setup ----------

fn run_pagesetup(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "pagesetup";
    let keys = ["paper", "width", "height", "landscape", "margins", "lineweights", "plotArea", "scale", "scaleToFit", "center", "plotStyleTable"];
    let changes = keys.iter().any(|k| p.get(k).is_some());
    let name = match (str_param(p, "layout"), s.space()) {
        (Some(n), _) => Some(layout_name(cmd, s.doc()?, n)?),
        (None, Space::Paper(n)) => Some(n),
        (None, Space::Model) => None,
    };
    let Some(name) = name else {
        if changes {
            return Err(bad(cmd, "pass `layout` (model-space page setups are not stored yet)"));
        }
        // Report every layout's page setup.
        let d = s.doc()?;
        return Ok(json!({
            "layouts": d.layouts.iter().map(|l| json!({"name": l.name, "page": serde_json::to_value(&l.page).unwrap_or(Value::Null)})).collect::<Vec<_>>(),
            "papers": cadcraft_render::PAPER_SIZES.iter().map(|p| p.name).collect::<Vec<_>>(),
        }));
    };
    let mut page = s.doc()?.layout(&name).map(|l| l.page.clone()).unwrap_or_default();
    if let Some(paper) = p.get("paper") {
        let n = paper.as_str().ok_or_else(|| bad(cmd, "`paper` must be a paper name"))?;
        let ps = cadcraft_render::paper_size(n).ok_or_else(|| {
            let known: Vec<&str> = cadcraft_render::PAPER_SIZES.iter().filter_map(|p| p.aliases.first().copied()).collect();
            bad(cmd, format!("unknown paper size `{n}` (known: {})", known.join(", ")))
        })?;
        page.paper = ps.name.into();
        page.width_mm = ps.width_mm;
        page.height_mm = ps.height_mm;
    }
    let mm = |k: &str| p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0 && *v <= 5080.0);
    match (mm("width"), mm("height")) {
        (Some(w), Some(h)) => {
            page.paper = format!("User ({w:.2} x {h:.2} MM)");
            page.width_mm = w.min(h);
            page.height_mm = w.max(h);
        }
        (None, None) if p.get("width").is_none() && p.get("height").is_none() => {}
        _ => return Err(bad(cmd, "`width` and `height` (mm, 0-5080) are both required for a custom size")),
    }
    if let Some(l) = p.get("landscape").and_then(Value::as_bool) {
        page.landscape = l;
    }
    if let Some(m) = p.get("margins") {
        let a: Vec<f64> = m.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
        match a[..] {
            [l, b, r, t] if a.iter().all(|v| v.is_finite() && (0.0..=1000.0).contains(v)) => page.margins_mm = [l, b, r, t],
            _ => return Err(bad(cmd, "`margins` must be [left, bottom, right, top] in mm")),
        }
    }
    if let Some(v) = p.get("lineweights").and_then(Value::as_bool) {
        page.lineweights = v;
    }
    if let Some(v) = p.get("scaleToFit").and_then(Value::as_bool) {
        page.scale_to_fit = v;
    }
    if let Some(v) = p.get("center").and_then(Value::as_bool) {
        page.center = v;
    }
    if let Some(v) = p.get("scale") {
        page.scale = parse_scale(v).ok_or_else(|| bad(cmd, "`scale` must be positive"))?;
    }
    if let Some(v) = str_param(p, "plotArea") {
        let v = v.to_ascii_lowercase();
        if !["layout", "extents", "display", "window", "limits"].contains(&v.as_str()) {
            return Err(bad(cmd, "`plotArea` must be layout, extents, display, window or limits"));
        }
        page.plot_area = v;
    }
    if let Some(v) = str_param(p, "plotStyleTable") {
        page.plot_style_table = v.chars().take(260).collect();
    }
    if changes {
        if let Some(l) = s.doc_mut()?.layouts.iter_mut().find(|l| l.name == name) {
            l.page = page.clone();
        }
        s.touch();
    }
    Ok(json!({ "layout": name, "page": serde_json::to_value(&page).unwrap_or(Value::Null) }))
}

// ---------- plotting ----------

fn run_plot(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let hook = io().and_then(|h| h.plot).ok_or_else(|| bad(cmd, "plotting is not available in this build"))?;
    let space = match str_param(p, "layout") {
        Some(n) if n.eq_ignore_ascii_case("model") => Space::Model,
        Some(n) => Space::Paper(layout_name(cmd, s.doc()?, n)?),
        None => s.space(),
    };
    let mut opts = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = opts.as_object_mut() {
        if let Some(v) = p.get("scale") {
            let k = parse_scale(v).ok_or_else(|| bad(cmd, "`scale` must be positive: paper units per drawing unit, or a ratio such as \"1:50\""))?;
            o.insert("scale".into(), json!(k));
        }
        if let Some(w) = plot_window(s, p, cmd, &space)? {
            o.insert("window".into(), json!([[w.min.x, w.min.y], [w.max.x, w.max.y]]));
        }
    }
    let bytes = hook(s.doc()?, &space, &opts).map_err(|e| bad(cmd, e))?;
    let layout = match &space {
        Space::Model => "Model".to_string(),
        Space::Paper(n) => n.clone(),
    };
    match str_param(p, "path") {
        Some(_path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let path = _path;
                let tmp = format!("{path}.cadcraft-tmp");
                std::fs::write(&tmp, &bytes).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
                std::fs::rename(&tmp, path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
                Ok(json!({ "path": path, "bytes": bytes.len(), "layout": layout }))
            }
            #[cfg(target_arch = "wasm32")]
            Err(bad(cmd, "paths are not available on the web; omit `path` to get the PDF as base64 `data`"))
        }
        None => Ok(json!({ "data": base64_encode(&bytes), "bytes": bytes.len(), "layout": layout })),
    }
}

/// The model-space rectangle a plot of `plotArea` covers: `extents` (default) plots everything,
/// `display` the current model view, `limits` the drawing limits, `window` the `window` corners.
/// Layouts plot their sheet (`layout`).
fn plot_window(s: &Session, p: &Value, cmd: &str, space: &Space) -> Result<Option<Bounds2>> {
    let area = str_param(p, "plotArea").map(str::to_ascii_lowercase);
    let w = match (space, area.as_deref()) {
        (_, None) | (Space::Model, Some("extents")) | (Space::Paper(_), Some("layout")) => return Ok(None),
        (Space::Paper(_), Some(_)) => return Err(bad(cmd, "a layout plots its whole sheet: `plotArea` must be layout")),
        (Space::Model, Some("display")) => {
            let st = s.state()?;
            let v = st.views.iter().find(|(sp, _)| *sp == Space::Model).map(|(_, v)| *v).unwrap_or_default();
            let (px, py) = s.viewport_px;
            let aspect = if px.is_finite() && py.is_finite() && px > 0.0 && py > 0.0 { px / py } else { 1.0 };
            let half = Vec2::new(v.height * aspect / 2.0, v.height / 2.0);
            Bounds2::new(v.center - half, v.center + half)
        }
        (Space::Model, Some("limits")) => {
            let d = s.doc()?;
            let lo = d.header.point("LIMMIN").map(|p| p.xy()).unwrap_or(Vec2::ZERO);
            let hi = d.header.point("LIMMAX").map(|p| p.xy()).unwrap_or(Vec2::new(12.0, 9.0));
            Bounds2::new(lo, hi)
        }
        (Space::Model, Some("window")) => {
            let corners = p.get("window").and_then(Value::as_array);
            let a = corners.and_then(|c| c.first()).and_then(point_value);
            let b = corners.and_then(|c| c.get(1)).and_then(point_value);
            let (Some(a), Some(b)) = (a, b) else {
                return Err(bad(cmd, "`plotArea` window needs `window`: [[x1, y1], [x2, y2]]"));
            };
            Bounds2::new(a, b)
        }
        (Space::Model, Some(_)) => return Err(bad(cmd, "`plotArea` must be extents, display, limits or window (layouts: layout)")),
    };
    let ok = w.min.is_finite() && w.max.is_finite() && w.width() > 1e-9 && w.height() > 1e-9;
    if ok { Ok(Some(w)) } else { Err(bad(cmd, "the plot area is empty")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_model() -> Session {
        let mut s = Session::new();
        s.cmdline("line 0,0 100,0").unwrap();
        s.cmdline("").unwrap();
        s.cmdline("circle 50,25 20").unwrap();
        s
    }

    fn viewports(s: &Session, layout: &str) -> Vec<(Handle, Viewport)> {
        s.doc()
            .unwrap()
            .layout(layout)
            .unwrap()
            .entities
            .iter()
            .filter_map(|e| if let EntityKind::Viewport(v) = &e.kind { Some((e.handle, v.clone())) } else { None })
            .collect()
    }

    #[test]
    fn new_layout_gets_viewport_zoomed_to_extents() {
        let mut s = session_with_model();
        let r = s.execute("layout.new", &json!({})).unwrap();
        assert_eq!(r["name"], "Layout3");
        let vps = viewports(&s, "Layout3");
        assert_eq!(vps.len(), 1);
        let (_, v) = &vps[0];
        let sheet = cadcraft_render::sheet(s.doc().unwrap(), "Layout3").unwrap();
        assert!((v.width - sheet.printable.width()).abs() < 1e-9 && (v.height - sheet.printable.height()).abs() < 1e-9);
        assert!(v.id >= 2);
        // Model extents (0,0)..(100,45) are centred and fully visible.
        assert!(v.view_center.near(Vec2::new(50.0, 22.5), 1e-6));
        let model_w = v.view_height * v.width / v.height;
        assert!(model_w >= 100.0 && v.view_height >= 45.0);
        // The render shows model geometry inside the viewport only.
        let list = cadcraft_render::build(s.doc().unwrap(), &Space::Paper("Layout3".into()), &cadcraft_render::Options::default());
        let rect = Bounds2::new(v.center.xy() - Vec2::new(v.width, v.height) / 2.0, v.center.xy() + Vec2::new(v.width, v.height) / 2.0).expand(1e-9);
        let content: Vec<_> = list.prims.iter().filter(|p| p.handle == cadcraft_render::VIEWPORT_CONTENT).collect();
        assert!(content.len() >= 2);
        assert!(content.iter().all(|p| list.points(p).iter().all(|q| rect.contains(*q))));
        assert!(list.sheet.is_some());
        // Undo removes the layout.
        s.undo().unwrap();
        assert!(s.doc().unwrap().layout("Layout3").is_none());
    }

    #[test]
    fn rename_copy_delete() {
        let mut s = session_with_model();
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        s.execute("layout.rename", &json!({"to": "Sheet A"})).unwrap();
        assert_eq!(s.space(), Space::Paper("Sheet A".into()));
        assert!(s.execute("layout.rename", &json!({"from": "Sheet A", "to": "Layout2"})).is_err(), "duplicate");
        assert!(s.execute("layout.rename", &json!({"from": "Sheet A", "to": "a/b"})).is_err(), "bad char");
        assert!(s.execute("layout.rename", &json!({"from": "Sheet A", "to": "model"})).is_err(), "reserved");
        s.execute("mview", &json!({"p1": [1, 1], "p2": [5, 4]})).unwrap();
        let r = s.execute("layout.copy", &json!({"from": "Sheet A"})).unwrap();
        assert_eq!(r["name"], "Sheet A (2)");
        let a = viewports(&s, "Sheet A");
        let b = viewports(&s, "Sheet A (2)");
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_ne!(a[0].0, b[0].0, "copied entities get new handles");
        s.execute("layout.delete", &json!({"name": "sheet a"})).unwrap();
        assert_eq!(s.space(), Space::Model, "deleting the current layout returns to Model");
        s.execute("layout", &json!({"option": "delete", "name": "Layout2"})).unwrap();
        assert!(s.execute("layout.delete", &json!({"name": "Sheet A (2)"})).is_err(), "last layout stays");
        let l = s.execute("layout", &json!({"option": "list"})).unwrap();
        assert_eq!(l.as_array().unwrap().len(), 1);
        assert!(s.execute("layout.delete", &json!({"name": "Model"})).is_err());
    }

    #[test]
    fn new_layout_from_the_menu_asks_for_a_name() {
        let mut s = session_with_model();
        s.start("layout.new").unwrap();
        assert_eq!(s.current_prompt().map(|p| p.display()).as_deref(), Some("Enter name of new layout <Layout3>:"));
        s.cmdline("Layout1").unwrap();
        assert!(s.running.is_some(), "a taken name asks again");
        s.cmdline("Sheet A").unwrap();
        assert!(s.running.is_none());
        s.start("layout.new").unwrap();
        s.cmdline("").unwrap();
        let d = s.doc().unwrap();
        assert!(d.layout("Sheet A").is_some() && d.layout("Layout3").is_some(), "Enter takes the default name");
    }

    #[test]
    fn mview_arrangements_and_interactive() {
        let mut s = session_with_model();
        assert!(s.execute("mview", &json!({"p1": [0, 0], "p2": [1, 1]})).is_err(), "not in model space");
        s.execute("mview", &json!({"layout": "Layout2", "count": 4})).unwrap();
        assert_eq!(viewports(&s, "Layout2").len(), 4);
        s.execute("vports.3", &json!({"layout": "Layout2", "p1": [0, 0], "p2": [9, 6]})).unwrap();
        let v = viewports(&s, "Layout2");
        assert_eq!(v.len(), 7);
        let total: f64 = v[4..].iter().map(|(_, v)| v.width * v.height).sum();
        assert!((total - 54.0).abs() < 1e-9, "three viewports tile the window");
        // Interactive MVIEW: two corners.
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        s.cmdline("mview").unwrap();
        s.cmdline("1,1").unwrap();
        s.cmdline("6,4").unwrap();
        assert!(s.running.is_none());
        let v = viewports(&s, "Layout1");
        assert_eq!(v.len(), 1);
        assert!((v[0].1.width - 5.0).abs() < 1e-9 && (v[0].1.height - 3.0).abs() < 1e-9);
        // MVIEW 2 → Fit (Enter).
        s.cmdline("mview").unwrap();
        s.cmdline("2").unwrap();
        s.cmdline("").unwrap();
        assert!(s.running.is_none());
        assert_eq!(viewports(&s, "Layout1").len(), 3);
        // One undo step per MVIEW.
        s.undo().unwrap();
        assert_eq!(viewports(&s, "Layout1").len(), 1);
        // In model space the interactive command ends at once.
        s.execute("layout.set", &json!({"name": "Model"})).unwrap();
        s.cmdline("vports.4").unwrap();
        assert!(s.running.is_none());
    }

    #[test]
    fn viewport_scale_lock_and_layers() {
        let mut s = session_with_model();
        s.execute("layout.set", &json!({"name": "Layout1"})).unwrap();
        let r = s.execute("mview", &json!({"p1": [1, 1], "p2": [5, 4]})).unwrap();
        let h = r["viewports"][0].as_str().unwrap().to_string();
        let out = s.execute("viewport.set", &json!({"handle": h, "scale": "1:20", "center": [50, 20]})).unwrap();
        assert!((out[0]["viewHeight"].as_f64().unwrap() - 60.0).abs() < 1e-9);
        assert!((out[0]["scale"].as_f64().unwrap() - 0.05).abs() < 1e-12);
        s.execute("viewport.set", &json!({"handle": h, "locked": true})).unwrap();
        assert!(s.execute("viewport.set", &json!({"handle": h, "scale": 0.1})).is_err(), "locked");
        s.execute("viewport.set", &json!({"handle": h, "scale": 0.1, "locked": false})).unwrap();
        s.execute("vplayer", &json!({"handle": h, "freeze": ["0", "Walls"]})).unwrap();
        s.execute("vplayer", &json!({"handle": h, "thaw": "walls"})).unwrap();
        let v = viewports(&s, "Layout1");
        assert_eq!(v[0].1.frozen_layers, vec!["0".to_string()]);
        s.execute("vplayer", &json!({"handle": h, "colors": {"Walls": "red"}})).unwrap();
        assert!(s.execute("vplayer", &json!({"handle": h, "colors": {"Walls": "bylayer"}})).is_err());
        assert!(s.execute("vplayer", &json!({"handle": h, "colors": {"Walls": [1]}})).is_err());
        let vv = viewports(&s, "Layout1");
        assert_eq!(vv[0].1.layer_colors, vec![("Walls".to_string(), cadcraft_color::Color::Index(1))]);
        s.execute("vplayer", &json!({"handle": h, "colors": {"walls": null}})).unwrap();
        assert!(viewports(&s, "Layout1")[0].1.layer_colors.is_empty());
        assert!((v[0].1.view_height - 30.0).abs() < 1e-9);
        // Layer 0 frozen in the viewport: nothing of the model shows.
        let list = cadcraft_render::build(s.doc().unwrap(), &Space::Paper("Layout1".into()), &cadcraft_render::Options::default());
        assert!(list.prims.iter().all(|p| p.handle != cadcraft_render::VIEWPORT_CONTENT));
        for bad_p in [
            json!({"handle": h, "scale": "0:0"}),
            json!({"handle": h, "scale": -1}),
            json!({"handle": h, "viewHeight": 0}),
            json!({"handle": "FFFFFF"}),
        ] {
            assert!(s.execute("viewport.set", &bad_p).is_err(), "{bad_p}");
        }
    }

    #[test]
    fn pagesetup_paper_table() {
        let mut s = Session::new();
        let r = s.execute("pagesetup", &json!({"layout": "Layout1", "paper": "A3", "landscape": true, "margins": [5, 5, 5, 5]})).unwrap();
        assert_eq!(r["page"]["widthMm"], 297.0);
        let sheet = cadcraft_render::sheet(s.doc().unwrap(), "Layout1").unwrap();
        // Imperial drawing: paper units are inches.
        assert!((sheet.size.x - 420.0 / 25.4).abs() < 1e-9 && (sheet.size.y - 297.0 / 25.4).abs() < 1e-9);
        assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "paper": "Z12"})).is_err());
        assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "margins": [1, 2]})).is_err());
        assert!(s.execute("pagesetup", &json!({"layout": "Layout1", "width": 1e308, "height": 5})).is_err());
        assert!(s.execute("pagesetup", &json!({"paper": "A4"})).is_err(), "model tab needs a layout");
        let report = s.execute("pagesetup", &Value::Null).unwrap();
        assert!(report["papers"].as_array().unwrap().len() >= 10);
        s.execute("pagesetup", &json!({"layout": "Layout2", "width": 500, "height": 300})).unwrap();
        let p = &s.doc().unwrap().layout("Layout2").unwrap().page;
        assert_eq!((p.width_mm, p.height_mm), (300.0, 500.0));
    }

    fn fake_plot(d: &Drawing, space: &Space, opts: &Value) -> std::result::Result<Vec<u8>, String> {
        let _ = d;
        Ok(format!("%PDF-1.4 fake {space:?} {opts}").into_bytes())
    }

    #[test]
    fn plot_goes_through_the_hook() {
        super::super::file::set_io(super::super::file::IoHooks {
            read: |_, _| Err("no reader in tests".into()),
            write: |_, _| Err("no writer in tests".into()),
            plot: Some(fake_plot),
        });
        let mut s = session_with_model();
        if io().and_then(|h| h.plot).is_none() {
            return; // another test installed hooks without a plotter first
        }
        let r = s.execute("plot", &json!({"layout": "Layout1", "paper": "A4"})).unwrap();
        let data = super::super::file::base64_decode(r["data"].as_str().unwrap()).unwrap();
        let text = String::from_utf8(data).unwrap();
        assert!(text.starts_with("%PDF") && text.contains("Paper(\"Layout1\")") && text.contains("A4"));
        let r = s.execute("exportpdf", &json!({})).unwrap();
        assert_eq!(r["layout"], "Model");
        assert!(s.execute("plot", &json!({"layout": "Nope"})).is_err());
        // Plot area and a ratio scale reach the plotter as a model-space window and a number.
        let opts = |r: Value| String::from_utf8(super::super::file::base64_decode(r["data"].as_str().unwrap()).unwrap()).unwrap();
        let p = json!({"layout": "Model", "plotArea": "window", "window": [[10, 0], [0, 5]], "fit": false, "scale": "1:2"});
        let text = opts(s.execute("plot", &p).unwrap());
        assert!(text.contains(r#""window":[[0.0,0.0],[10.0,5.0]]"#) && text.contains(r#""scale":0.5"#), "{text}");
        assert!(opts(s.execute("plot", &json!({"layout": "Model", "plotArea": "limits"})).unwrap()).contains(r#""window":[[0.0,0.0],"#));
        assert!(s.execute("plot", &json!({"layout": "Model", "plotArea": "window"})).is_err(), "window needs corners");
        assert!(s.execute("plot", &json!({"layout": "Layout1", "plotArea": "display"})).is_err(), "layouts plot their sheet");
        assert!(s.execute("plot", &json!({"layout": "Model", "scale": -1})).is_err());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = std::env::temp_dir().join(format!("cadcraft-plot-test-{}.pdf", std::process::id()));
            let ps = path.to_string_lossy().to_string();
            let r = s.execute("plot", &json!({"path": ps})).unwrap();
            assert!(r["bytes"].as_u64().unwrap() > 0);
            assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn hostile_layout_params() {
        let mut s = session_with_model();
        let long = "x".repeat(300);
        for (c, p) in [
            ("layout.new", json!({"name": ""})),
            ("layout.new", json!({"name": long})),
            ("layout.new", json!({"name": "a\u{0}b"})),
            ("layout.new", json!({"name": "Layout1"})),
            ("mview", json!({"layout": "Layout1", "p1": [1e308, -1e308], "p2": [-1e308, 1e308]})),
            ("mview", json!({"layout": "Layout1", "p1": [1, 1], "p2": [1, 1]})),
            ("mview", json!({"layout": "Layout1", "count": 99})),
            ("mview", json!({"layout": "Layout1", "count": -1})),
            ("layout.rename", json!({"from": "Layout1"})),
            ("layout.copy", json!({"from": "nope"})),
            ("viewport.set", json!({"handles": ["zz"]})),
        ] {
            assert!(s.execute(c, &p).is_err(), "{c} {p}");
        }
    }

    #[test]
    fn mspace_edits_model_through_viewport() {
        let mut s = session_with_model();
        let before = s.doc().unwrap().model.len();
        s.execute("layout.new", &json!({"name": "L"})).unwrap();
        s.execute("layout.set", &json!({"name": "L"})).unwrap();
        assert!(s.execute("mspace", &json!({"at": [-1000, -1000]})).is_err());
        let vps = viewports(&s, "L");
        let (h, vp) = vps.iter().find(|(_, v)| v.id != 1).cloned().unwrap();
        s.execute("mspace", &json!({"at": [vp.center.x, vp.center.y]})).unwrap();
        assert_eq!(s.space(), Space::Model);
        assert_eq!(s.layout_space(), Space::Paper("L".into()));
        // The screen view in MSPACE maps the viewport centre to its model view centre.
        let st = s.state().unwrap();
        let (pv, v) = (st.paper_view(), st.view());
        let k = vp.view_height / vp.height;
        assert!((v.height - pv.height * k).abs() < 1e-9);
        // New geometry goes to model space.
        s.cmdline("circle 10,10 3").unwrap();
        assert_eq!(s.doc().unwrap().model.len(), before + 1);
        // Zooming changes the viewport's model view, not the sheet.
        s.state_mut().unwrap().set_view(crate::View { center: v.center, height: v.height / 2.0 });
        let vp2 = viewports(&s, "L").into_iter().find(|(x, _)| *x == h).unwrap().1;
        assert!((vp2.view_height - vp.view_height / 2.0).abs() < 1e-9);
        assert_eq!(s.state().unwrap().paper_view(), pv);
        s.execute("pspace", &json!({})).unwrap();
        assert_eq!(s.space(), Space::Paper("L".into()));
        s.execute("mspace", &json!({})).unwrap();
        s.execute("layout.set", &json!({"name": "Model"})).unwrap();
        assert!(s.state().unwrap().mspace.is_none());
    }
}
