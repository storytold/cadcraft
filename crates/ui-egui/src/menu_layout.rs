//! The order, grouping and icons of the menus. Commands keep their own labels and shortcuts
//! (`CommandSpec`, `menus::UI_COMMANDS`); a command with a menu path that is not listed here is
//! appended to the end of its top menu by `menus::tree`, so nothing disappears.

use crate::icons::Icon;

/// A row of a menu.
#[derive(Clone, Copy, Debug)]
pub enum Row {
    /// A command. `label` replaces the command's own label: the rows of a flattened third level
    /// name their heading ("Constraint Bars › Show All"), each part a string the catalogs hold.
    Cmd {
        id: &'static str,
        icon: Option<Icon>,
        label: Option<&'static str>,
    },
    Sub {
        label: &'static str,
        icon: Option<Icon>,
        rows: &'static [Row],
    },
    Sep,
}

/// What joins the heading and the item in a flattened row's label.
pub const FLAT: &str = " › ";

const SEP: Row = Row::Sep;

const fn cmd(id: &'static str) -> Row {
    Row::Cmd { id, icon: None, label: None }
}

const fn ico(id: &'static str, icon: Icon) -> Row {
    Row::Cmd { id, icon: Some(icon), label: None }
}

const fn named(id: &'static str, label: &'static str) -> Row {
    Row::Cmd { id, icon: None, label: Some(label) }
}

const fn sub(label: &'static str, rows: &'static [Row]) -> Row {
    Row::Sub { label, icon: None, rows }
}

const fn sub_ico(label: &'static str, icon: Icon, rows: &'static [Row]) -> Row {
    Row::Sub { label, icon: Some(icon), rows }
}

/// The top menus (the same names and order as `menus::MENUS`) and their rows.
pub const MENU_LAYOUT: &[(&str, &[Row])] = &[
    (
        "File",
        &[
            ico("new", Icon::New),
            ico("ui.newtemplate", Icon::Template),
            ico("open", Icon::Open),
            SEP,
            ico("close", Icon::Close),
            cmd("closeall"),
            SEP,
            ico("qsave", Icon::Save),
            ico("saveas", Icon::SaveAs),
            sub(
                "Default Save Format",
                &[
                    cmd("ui.saveformat.dxf"),
                    cmd("ui.saveformat.dwg"),
                    SEP,
                    cmd("ui.saveversion.2018"),
                    cmd("ui.saveversion.2013"),
                    cmd("ui.saveversion.2010"),
                    cmd("ui.saveversion.2007"),
                    cmd("ui.saveversion.2004"),
                    cmd("ui.saveversion.2000"),
                ],
            ),
            SEP,
            ico("import", Icon::Import),
            ico("export", Icon::Export),
            ico("exportpdf", Icon::Pdf),
            ico("pdfbook", Icon::Pdf),
            SEP,
            ico("plot", Icon::Plot),
            ico("preview", Icon::Preview),
            ico("pagesetup", Icon::PageSetup),
            cmd("stylesmanager"),
            cmd("plottermanager"),
            ico("publish", Icon::Publish),
            SEP,
            sub_ico("Drawing Utilities", Icon::Wrench, &[cmd("audit"), cmd("recover"), cmd("drawingrecovery")]),
            SEP,
            ico("ui.quit", Icon::Quit),
        ],
    ),
    (
        "Edit",
        &[
            ico("u", Icon::Undo),
            ico("redo", Icon::Redo),
            SEP,
            ico("cutclip", Icon::Cut),
            ico("copyclip", Icon::ClipCopy),
            cmd("copybase"),
            ico("pasteclip", Icon::Paste),
            cmd("pasteblock"),
            cmd("pasteorig"),
            SEP,
            ico("erase.selection", Icon::Erase),
            ico("selectall", Icon::SelectAll),
            SEP,
            ico("find", Icon::Search),
            ico("qselect", Icon::QuickSelect),
            SEP,
            ico("ui.dialog.language", Icon::Globe),
        ],
    ),
    (
        "View",
        &[
            ico("redraw", Icon::Regen),
            cmd("regen"),
            cmd("regenall"),
            SEP,
            sub_ico(
                "Zoom",
                Icon::ZoomWindow,
                &[
                    cmd("zoom.realtime"),
                    cmd("zoom.previous"),
                    cmd("zoom.window"),
                    cmd("zoom.dynamic"),
                    SEP,
                    cmd("zoom.all"),
                    cmd("zoom.extents"),
                    cmd("zoom.object"),
                    SEP,
                    cmd("zoom.in"),
                    cmd("zoom.out"),
                ],
            ),
            sub_ico("Pan", Icon::Pan, &[cmd("pan"), SEP, cmd("pan.left"), cmd("pan.right"), cmd("pan.up"), cmd("pan.down")]),
            SEP,
            ico("view", Icon::NamedViews),
            sub_ico(
                "Viewports",
                Icon::Viewports,
                &[cmd("vports.1"), cmd("vports.2"), cmd("vports.3"), cmd("vports.4"), SEP, cmd("vports"), cmd("mview.polygonal")],
            ),
            SEP,
            sub_ico("UCS Icon", Icon::Ucs, &[cmd("ui.toggle.ucsicon")]),
            sub_ico("ViewCube", Icon::Orbit, &[cmd("ui.toggle.viewcube")]),
            sub_ico("Show Annotative Objects", Icon::Annotation, &[cmd("annovisible.all"), cmd("annovisible.current")]),
            SEP,
            sub_ico("Interface Theme", Icon::Theme, &[cmd("ui.theme.dark"), cmd("ui.theme.light"), cmd("ui.theme.system")]),
            sub_ico(
                "Accessibility",
                Icon::Accessibility,
                &[
                    cmd("ui.toggle.systemcursor"),
                    cmd("ui.toggle.accesskeys"),
                    sub("Animation Effects", &[cmd("ui.animations.system"), cmd("ui.animations.on"), cmd("ui.animations.off")]),
                ],
            ),
        ],
    ),
    (
        "Insert",
        &[
            ico("insert", Icon::Insert),
            ico("xattach", Icon::Xref),
            ico("datalink", Icon::DataLink),
            SEP,
            sub_ico("Layout", Icon::Layout, &[cmd("layout.new")]),
        ],
    ),
    (
        "Format",
        &[
            ico("layer", Icon::LayerProps),
            ico("layerstate.save", Icon::LayerState),
            sub(
                "Layer Tools",
                &[
                    cmd("layer.current"),
                    cmd("laymch"),
                    cmd("laycur"),
                    cmd("layerp"),
                    SEP,
                    cmd("layiso"),
                    cmd("layuniso"),
                    cmd("layfrz"),
                    cmd("layoff"),
                    cmd("laylck"),
                    cmd("layulk"),
                    SEP,
                    cmd("laymrg"),
                    cmd("laydel"),
                ],
            ),
            SEP,
            ico("color", Icon::ColorWheel),
            ico("linetype", Icon::Linetype),
            ico("lweight", Icon::Lineweight),
            SEP,
            cmd("scalelistedit"),
            ico("style", Icon::TextStyle),
            ico("dimstyle", Icon::DimStyle),
            ico("tablestyle", Icon::Table),
            ico("mleaderstyle", Icon::MLeader),
            ico("mlstyle", Icon::Multiline),
            cmd("plotstyle"),
            ico("ddptype", Icon::Point),
            SEP,
            ico("units", Icon::Units),
            cmd("limits"),
            SEP,
            ico("rename", Icon::Rename),
        ],
    ),
    (
        "Tools",
        &[
            ico("script", Icon::Script),
            cmd("appload"),
            ico("compare", Icon::Compare),
            ico("dataextraction", Icon::DataLink),
            SEP,
            ico("group", Icon::Group),
            cmd("ungroup"),
            sub_ico(
                "Draw Order",
                Icon::DrawOrder,
                &[
                    cmd("draworder.front"),
                    cmd("draworder.back"),
                    cmd("draworder.above"),
                    cmd("draworder.under"),
                    SEP,
                    named("texttofront.text", "Bring Annotations to Front › Text Objects Only"),
                    named("texttofront.dimensions", "Bring Annotations to Front › Dimension Objects Only"),
                    named("texttofront.leaders", "Bring Annotations to Front › Leaders Only"),
                    named("texttofront.all", "Bring Annotations to Front › All Annotation Objects"),
                    SEP,
                    cmd("hatchtoback"),
                ],
            ),
            sub_ico("Isolate", Icon::LayerIso, &[cmd("isolateobjects"), cmd("hideobjects"), cmd("unisolateobjects")]),
            SEP,
            sub_ico(
                "Inquiry",
                Icon::Measure,
                &[
                    cmd("dist"),
                    cmd("radius.measure"),
                    cmd("angle.measure"),
                    cmd("area"),
                    cmd("cal"),
                    cmd("massprop"),
                    SEP,
                    cmd("list"),
                    cmd("id"),
                    cmd("time"),
                    cmd("status"),
                    cmd("setvar"),
                ],
            ),
            ico("count", Icon::List),
            SEP,
            sub_ico(
                "Parametric",
                Icon::Constraint,
                &[
                    cmd("autoconstrain"),
                    sub(
                        "Geometric Constraints",
                        &[
                            cmd("gccoincident"),
                            cmd("gcperpendicular"),
                            cmd("gcparallel"),
                            cmd("gctangent"),
                            cmd("gchorizontal"),
                            cmd("gcvertical"),
                            cmd("gccollinear"),
                            cmd("gcconcentric"),
                            cmd("gcsmooth"),
                            cmd("gcsymmetric"),
                            cmd("gcequal"),
                            cmd("gcfix"),
                        ],
                    ),
                    sub(
                        "Dimensional Constraints",
                        &[cmd("dcaligned"), cmd("dchorizontal"), cmd("dcvertical"), cmd("dcangular"), cmd("dcradius"), cmd("dcdiameter")],
                    ),
                    SEP,
                    named("constraintbar", "Constraint Bars › Select Objects"),
                    named("constraintbar.showall", "Constraint Bars › Show All"),
                    named("constraintbar.hideall", "Constraint Bars › Hide All"),
                    SEP,
                    named("dcdisplay", "Dynamic Dimensions › Select Objects"),
                    named("dcdisplay.showall", "Dynamic Dimensions › Show All"),
                    named("dcdisplay.hideall", "Dynamic Dimensions › Hide All"),
                    SEP,
                    cmd("delconstraint"),
                    cmd("parameters"),
                    cmd("constraintsettings"),
                ],
            ),
            ico("xopen", Icon::Xref),
            ico("bedit", Icon::BlockEdit),
            sub_ico("Xref and Block In-place Editing", Icon::Xref, &[cmd("refedit"), cmd("refset"), cmd("refclose")]),
            SEP,
            ico("dsettings", Icon::Gear),
            ico("ui.spacemouse", Icon::Mouse3d),
            sub("Express Tools", &[cmd("burst"), cmd("txt2mtxt"), cmd("mocoro"), cmd("breakline")]),
            sub("Customize", &[cmd("aliasedit")]),
            ico("options", Icon::Gear),
        ],
    ),
    (
        "Draw",
        &[
            ico("line", Icon::Line),
            ico("pline", Icon::Polyline),
            ico("3dpoly", Icon::Polyline3d),
            ico("xline", Icon::XLine),
            ico("ray", Icon::Ray),
            ico("mline", Icon::Multiline),
            SEP,
            ico("rectang", Icon::Rectangle),
            ico("polygon", Icon::Polygon),
            sub_ico(
                "Circle",
                Icon::Circle,
                &[cmd("circle"), cmd("circle.cd"), SEP, cmd("circle.2p"), cmd("circle.3p"), SEP, cmd("circle.ttr"), cmd("circle.ttt")],
            ),
            sub_ico(
                "Arc",
                Icon::Arc,
                &[
                    cmd("arc"),
                    SEP,
                    cmd("arc.sce"),
                    cmd("arc.sca"),
                    cmd("arc.scl"),
                    SEP,
                    cmd("arc.sea"),
                    cmd("arc.sed"),
                    cmd("arc.ser"),
                    SEP,
                    cmd("arc.cse"),
                    cmd("arc.csa"),
                    cmd("arc.csl"),
                    SEP,
                    cmd("arc.continue"),
                ],
            ),
            sub_ico("Ellipse", Icon::Ellipse, &[cmd("ellipse"), cmd("ellipse.axis"), cmd("ellipse.arc")]),
            ico("donut", Icon::Donut),
            sub_ico("Spline", Icon::Spline, &[cmd("spline"), cmd("spline.cv")]),
            ico("helix", Icon::Helix),
            sub_ico("Revision Cloud", Icon::RevCloud, &[cmd("revcloud"), cmd("revcloud.polygonal"), cmd("revcloud.freehand")]),
            SEP,
            sub_ico("Point", Icon::Point, &[cmd("point"), cmd("point.multiple"), SEP, cmd("divide"), cmd("measure")]),
            ico("centermark", Icon::CenterMark),
            ico("centerline", Icon::CenterLine),
            SEP,
            sub_ico("Block", Icon::Blocks, &[cmd("block"), cmd("attdef"), cmd("base")]),
            ico("table", Icon::Table),
            sub_ico("Text", Icon::Text, &[cmd("text"), cmd("mtext")]),
            SEP,
            ico("hatch", Icon::Hatch),
            ico("gradient", Icon::Gradient),
            ico("boundary", Icon::Boundary),
            ico("region", Icon::Region),
            ico("wipeout", Icon::Wipeout),
        ],
    ),
    (
        "Dimension",
        &[
            ico("qdim", Icon::QDim),
            ico("dim", Icon::Dimension),
            SEP,
            ico("dimlinear", Icon::DimLinear),
            ico("dimaligned", Icon::DimAligned),
            ico("dimarc", Icon::DimArc),
            ico("dimordinate", Icon::DimOrdinate),
            SEP,
            ico("dimradius", Icon::DimRadius),
            ico("dimjogged", Icon::DimJogged),
            ico("dimdiameter", Icon::DimDiameter),
            ico("dimangular", Icon::DimAngular),
            SEP,
            ico("dimbaseline", Icon::DimBaseline),
            ico("dimcontinue", Icon::DimContinue),
            ico("dimspace", Icon::DimSpace),
            ico("dimbreak", Icon::DimBreak),
            SEP,
            ico("mleader", Icon::MLeader),
            ico("tolerance", Icon::Tolerance),
            ico("diminspect", Icon::Inspection),
            ico("dimjogline", Icon::DimJogged),
            SEP,
            ico("dimedit.oblique", Icon::Oblique),
            sub_ico(
                "Align Text",
                Icon::AlignText,
                &[cmd("dimtedit.home"), cmd("dimtedit.angle"), SEP, cmd("dimtedit.left"), cmd("dimtedit.center"), cmd("dimtedit.right")],
            ),
            SEP,
            ico("dimstyle.dimension", Icon::DimStyle),
            ico("dimoverride", Icon::DimOverride),
            ico("dimstyle.update", Icon::Regen),
            cmd("dimreassociate"),
        ],
    ),
    (
        "Modify",
        &[
            ico("properties", Icon::Properties),
            ico("matchprop", Icon::MatchProp),
            SEP,
            ico("erase", Icon::Erase),
            ico("copy", Icon::Copy),
            ico("mirror", Icon::Mirror),
            ico("offset", Icon::Offset),
            sub_ico("Array", Icon::ArrayRect, &[cmd("arrayrect"), cmd("arraypath"), cmd("arraypolar")]),
            SEP,
            ico("move", Icon::Move),
            ico("rotate", Icon::Rotate),
            ico("scale", Icon::Scale),
            ico("stretch", Icon::Stretch),
            ico("lengthen", Icon::Lengthen),
            SEP,
            ico("trim", Icon::Trim),
            ico("extend", Icon::Extend),
            ico("break", Icon::Break),
            ico("breakatpoint", Icon::Break),
            ico("join", Icon::Join),
            ico("chamfer", Icon::Chamfer),
            ico("fillet", Icon::Fillet),
            cmd("blend"),
            SEP,
            ico("explode", Icon::Explode),
            sub_ico(
                "Object",
                Icon::Object,
                &[
                    cmd("pedit"),
                    cmd("splinedit"),
                    cmd("mledit"),
                    cmd("hatchedit"),
                    cmd("arrayedit"),
                    cmd("textedit"),
                    SEP,
                    named("attedit", "Attribute › Single..."),
                    named("-attedit", "Attribute › Global"),
                    named("attsync", "Attribute › Synchronize Attributes"),
                    named("battman", "Attribute › Block Attribute Manager..."),
                    SEP,
                    named("xbind", "External Reference › Bind..."),
                    named("mleaderedit.add", "Multileader › Add Leader"),
                    named("mleaderedit.remove", "Multileader › Remove Leader"),
                    named("mleaderalign", "Multileader › Align"),
                    named("mleadercollect", "Multileader › Collect"),
                ],
            ),
            sub_ico(
                "Annotative Object Scale",
                Icon::Annotation,
                &[cmd("objectscale"), cmd("aiobjectscaleadd"), cmd("aiobjectscaleremove"), SEP, cmd("annoreset")],
            ),
            SEP,
            cmd("ncopy"),
            cmd("chspace"),
            ico("flatten", Icon::Flatten),
            ico("overkill", Icon::Overkill),
            SEP,
            sub_ico("Clip", Icon::Clip, &[cmd("vpclip")]),
            sub_ico("Solid Editing", Icon::Solid, &[cmd("union"), cmd("subtract"), cmd("intersect")]),
            sub_ico("3D Operations", Icon::Orbit, &[cmd("align")]),
        ],
    ),
    (
        "Window",
        &[
            cmd("ui.toggle.toolsets"),
            cmd("ui.panel.properties"),
            cmd("ui.panel.layers"),
            cmd("ui.panel.quickcalc"),
            cmd("xref"),
            SEP,
            cmd("ui.toggle.toolbar"),
            cmd("ui.toggle.filetabs"),
            cmd("ui.toggle.statusbar"),
            cmd("ui.toggle.cmdline"),
            cmd("ui.toggle.menubar"),
            SEP,
            cmd("ui.hidepalettes"),
            cmd("ui.resetpalettes"),
            cmd("ui.dock.reset"),
            SEP,
            cmd("ui.start"),
            ico("ui.dialog.layers", Icon::LayerProps),
            ico("ui.dialog.blocks", Icon::Blocks),
            ico("ui.dialog.parameters", Icon::Constraint),
            ico("ui.toggle.history", Icon::List),
        ],
    ),
    (
        "Help",
        &[
            ico("ui.dialog.commands", Icon::Help),
            ico("ui.sample", Icon::Sample),
            SEP,
            ico("ui.discord", Icon::Chat),
            SEP,
            ico("ui.dialog.about", Icon::Info),
        ],
    ),
];

/// Access keys. Alt and a menu's letter opens it from the keyboard; in an open menu a row's letter
/// chooses that row, or steps through the rows that share it. A row is named by its command id,
/// a submenu by its heading. Rows without a letter (those of submenus) answer to their initial.
pub const ACCESS_KEYS: &[(&str, char, &[(&str, char)])] = &[
    (
        "File",
        'F',
        &[
            ("new", 'N'),
            ("ui.newtemplate", 'T'),
            ("open", 'O'),
            ("Open Recent", 'R'),
            ("close", 'C'),
            ("closeall", 'E'),
            ("qsave", 'S'),
            ("saveas", 'A'),
            ("Default Save Format", 'D'),
            ("import", 'I'),
            ("export", 'E'),
            ("exportpdf", 'F'),
            ("pdfbook", 'K'),
            ("plot", 'P'),
            ("preview", 'V'),
            ("pagesetup", 'G'),
            ("stylesmanager", 'Y'),
            ("plottermanager", 'M'),
            ("publish", 'B'),
            ("Drawing Utilities", 'U'),
            ("ui.quit", 'Q'),
        ],
    ),
    (
        "Edit",
        'E',
        &[
            ("u", 'U'),
            ("redo", 'R'),
            ("cutclip", 'T'),
            ("copyclip", 'C'),
            ("copybase", 'B'),
            ("pasteclip", 'P'),
            ("pasteblock", 'L'),
            ("pasteorig", 'O'),
            ("erase.selection", 'R'),
            ("selectall", 'A'),
            ("find", 'F'),
            ("qselect", 'Q'),
            ("ui.dialog.language", 'I'),
        ],
    ),
    (
        "View",
        'V',
        &[
            ("redraw", 'R'),
            ("regen", 'G'),
            ("regenall", 'A'),
            ("Zoom", 'Z'),
            ("Pan", 'P'),
            ("view", 'N'),
            ("Viewports", 'V'),
            ("UCS Icon", 'U'),
            ("ViewCube", 'C'),
            ("Show Annotative Objects", 'H'),
            ("Interface Theme", 'T'),
            ("Accessibility", 'Y'),
        ],
    ),
    ("Insert", 'I', &[("insert", 'B'), ("xattach", 'R'), ("datalink", 'L'), ("Layout", 'Y')]),
    (
        "Format",
        'O',
        &[
            ("layer", 'L'),
            ("layerstate.save", 'S'),
            ("Layer Tools", 'T'),
            ("color", 'C'),
            ("linetype", 'N'),
            ("lweight", 'W'),
            ("scalelistedit", 'E'),
            ("style", 'X'),
            ("dimstyle", 'D'),
            ("tablestyle", 'B'),
            ("mleaderstyle", 'M'),
            ("mlstyle", 'U'),
            ("plotstyle", 'P'),
            ("ddptype", 'I'),
            ("units", 'U'),
            ("limits", 'G'),
            ("rename", 'R'),
        ],
    ),
    (
        "Tools",
        'T',
        &[
            ("script", 'R'),
            ("appload", 'L'),
            ("compare", 'C'),
            ("dataextraction", 'X'),
            ("group", 'G'),
            ("ungroup", 'U'),
            ("Draw Order", 'D'),
            ("Isolate", 'I'),
            ("Inquiry", 'Q'),
            ("count", 'N'),
            ("Parametric", 'P'),
            ("xopen", 'X'),
            ("bedit", 'B'),
            ("Xref and Block In-place Editing", 'E'),
            ("dsettings", 'F'),
            ("ui.spacemouse", '3'),
            ("options", 'O'),
        ],
    ),
    (
        "Draw",
        'D',
        &[
            ("line", 'L'),
            ("pline", 'P'),
            ("3dpoly", '3'),
            ("xline", 'T'),
            ("ray", 'R'),
            ("mline", 'U'),
            ("rectang", 'G'),
            ("polygon", 'Y'),
            ("Circle", 'C'),
            ("Arc", 'A'),
            ("Ellipse", 'E'),
            ("donut", 'D'),
            ("Spline", 'S'),
            ("helix", 'H'),
            ("Revision Cloud", 'V'),
            ("Point", 'O'),
            ("centermark", 'M'),
            ("centerline", 'N'),
            ("Block", 'K'),
            ("table", 'B'),
            ("Text", 'X'),
            ("hatch", 'H'),
            ("gradient", 'G'),
            ("boundary", 'B'),
            ("region", 'I'),
            ("wipeout", 'W'),
        ],
    ),
    (
        "Dimension",
        'N',
        &[
            ("qdim", 'Q'),
            ("dim", 'D'),
            ("dimlinear", 'L'),
            ("dimaligned", 'G'),
            ("dimarc", 'H'),
            ("dimordinate", 'O'),
            ("dimradius", 'R'),
            ("dimjogged", 'J'),
            ("dimdiameter", 'M'),
            ("dimangular", 'A'),
            ("dimbaseline", 'B'),
            ("dimcontinue", 'C'),
            ("dimspace", 'P'),
            ("dimbreak", 'K'),
            ("mleader", 'E'),
            ("tolerance", 'T'),
            ("diminspect", 'I'),
            ("dimjogline", 'V'),
            ("dimedit.oblique", 'Q'),
            ("Align Text", 'X'),
            ("dimstyle.dimension", 'S'),
            ("dimoverride", 'V'),
            ("dimstyle.update", 'U'),
            ("dimreassociate", 'N'),
        ],
    ),
    (
        "Modify",
        'M',
        &[
            ("properties", 'P'),
            ("matchprop", 'M'),
            ("erase", 'E'),
            ("copy", 'Y'),
            ("mirror", 'I'),
            ("offset", 'F'),
            ("Array", 'A'),
            ("move", 'V'),
            ("rotate", 'R'),
            ("scale", 'L'),
            ("stretch", 'H'),
            ("lengthen", 'G'),
            ("trim", 'T'),
            ("extend", 'X'),
            ("break", 'K'),
            ("breakatpoint", 'B'),
            ("join", 'J'),
            ("chamfer", 'C'),
            ("fillet", 'F'),
            ("blend", 'D'),
            ("explode", 'O'),
            ("Object", 'O'),
            ("Annotative Object Scale", 'N'),
            ("ncopy", 'N'),
            ("chspace", 'S'),
            ("flatten", 'A'),
            ("overkill", 'D'),
            ("Clip", 'P'),
            ("Solid Editing", 'Z'),
            ("3D Operations", '3'),
        ],
    ),
    (
        "Window",
        'W',
        &[
            ("ui.toggle.toolsets", 'T'),
            ("ui.panel.properties", 'P'),
            ("ui.panel.layers", 'L'),
            ("ui.panel.quickcalc", 'Q'),
            ("xref", 'R'),
            ("ui.toggle.toolbar", 'B'),
            ("ui.toggle.filetabs", 'F'),
            ("ui.toggle.statusbar", 'S'),
            ("ui.toggle.cmdline", 'C'),
            ("ui.toggle.menubar", 'M'),
            ("ui.hidepalettes", 'H'),
            ("ui.resetpalettes", 'E'),
            ("ui.dock.reset", 'O'),
            ("ui.start", 'A'),
            ("ui.dialog.layers", 'Y'),
            ("ui.dialog.blocks", 'K'),
            ("ui.dialog.parameters", 'M'),
            ("ui.toggle.history", 'I'),
        ],
    ),
    ("Help", 'H', &[("ui.dialog.commands", 'H'), ("ui.sample", 'S'), ("ui.discord", 'D'), ("ui.dialog.about", 'A')]),
];

/// The letter that opens `menu` with Alt.
pub fn menu_key(menu: &str) -> Option<char> {
    ACCESS_KEYS.iter().find(|(name, ..)| *name == menu).map(|(_, key, _)| *key)
}

/// The letters of the rows of `menu`.
pub fn row_keys(menu: &str) -> &'static [(&'static str, char)] {
    ACCESS_KEYS.iter().find(|(name, ..)| *name == menu).map_or(&[], |(_, _, rows)| rows)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cadcraft_engine::Session;

    use super::{ACCESS_KEYS, FLAT, MENU_LAYOUT, Row, menu_key};
    use crate::menus::{self, Entry};
    use crate::{CadApp, Services};

    fn layout_rows(rows: &'static [Row], out: &mut Vec<Row>) {
        for row in rows {
            out.push(*row);
            if let Row::Sub { rows, .. } = row {
                layout_rows(rows, out);
            }
        }
    }

    fn all_rows() -> Vec<Row> {
        let mut out = Vec::new();
        for (_, rows) in MENU_LAYOUT {
            layout_rows(rows, &mut out);
        }
        out
    }

    fn item_ids(entries: &[Entry], out: &mut Vec<String>) {
        for e in entries {
            match e {
                Entry::Item { id, .. } => out.push(id.clone()),
                Entry::Sub { children, .. } => item_ids(children, out),
                Entry::Separator => {}
            }
        }
    }

    /// Every command with a menu path: (id, top menu).
    fn menu_commands() -> Vec<(&'static str, &'static str)> {
        let engine = cadcraft_engine::command_specs().iter().map(|c| (c.id, c.menu));
        engine.chain(menus::UI_COMMANDS.iter().map(|c| (c.0, c.2))).filter_map(|(id, menu)| Some((id, *menu.first()?))).collect()
    }

    #[test]
    fn layout_names_the_menus_in_order() {
        assert_eq!(MENU_LAYOUT.iter().map(|(m, _)| *m).collect::<Vec<_>>(), menus::MENUS);
    }

    #[test]
    fn every_layout_id_exists() {
        let mut seen = BTreeSet::new();
        for row in all_rows() {
            if let Row::Cmd { id, .. } = row {
                assert!(
                    cadcraft_engine::find_command(id).is_some_and(|c| c.id == id) || menus::is_ui_command(id),
                    "the layout names {id}, which is not a command"
                );
                assert!(seen.insert(id), "the layout names {id} twice");
            }
        }
    }

    #[test]
    fn every_menu_command_is_placed() {
        let app = CadApp::new(Session::new(), Services::default());
        let tree = menus::tree(&app);
        let mut shown = Vec::new();
        for (_, entries) in &tree {
            item_ids(entries, &mut shown);
        }
        let unique: BTreeSet<&str> = shown.iter().map(String::as_str).collect();
        assert_eq!(unique.len(), shown.len(), "a command is in the menus twice");
        let named: BTreeSet<&str> = all_rows().iter().filter_map(|r| if let Row::Cmd { id, .. } = r { Some(*id) } else { None }).collect();
        for (id, top) in menu_commands() {
            assert!(unique.contains(id), "{id} has a menu path but is in no menu");
            // The fallback of `menus::tree` keeps a new command visible; give it a place here.
            assert!(named.contains(id), "{id} ({top}) is not in MENU_LAYOUT");
            let (_, entries) = tree.iter().find(|(m, _)| m == top).unwrap();
            let mut ids = Vec::new();
            item_ids(entries, &mut ids);
            assert!(ids.iter().any(|i| i == id), "{id} moved out of the {top} menu");
        }
    }

    #[test]
    fn no_stray_separators() {
        fn check(menu: &str, entries: &[Entry]) {
            assert!(!matches!(entries.first(), Some(Entry::Separator)), "{menu} starts with a separator");
            assert!(!matches!(entries.last(), Some(Entry::Separator)), "{menu} ends with a separator");
            assert!(!entries.windows(2).any(|w| matches!(w, [Entry::Separator, Entry::Separator])), "{menu} has two separators in a row");
            assert!(!entries.is_empty(), "{menu} is empty");
            for e in entries {
                if let Entry::Sub { label, children, .. } = e {
                    check(label, children);
                }
            }
        }
        let app = CadApp::new(Session::new(), Services::default());
        for (menu, entries) in menus::tree(&app) {
            check(&menu, &entries);
        }
    }

    #[test]
    fn menu_access_keys_are_distinct_letters_of_the_names() {
        assert_eq!(ACCESS_KEYS.iter().map(|(m, ..)| *m).collect::<Vec<_>>(), menus::MENUS);
        let keys: BTreeSet<char> = ACCESS_KEYS.iter().map(|(_, key, _)| *key).collect();
        assert_eq!(keys.len(), menus::MENUS.len(), "two menus share an access key");
        for menu in menus::MENUS {
            let key = menu_key(menu).unwrap();
            assert!(key.is_ascii_uppercase() && menu.to_ascii_uppercase().contains(key), "{menu}: {key}");
        }
    }

    /// A letter must belong to a row at the top of its menu, or it could never be pressed.
    #[test]
    fn row_access_keys_name_rows_of_their_menu() {
        for (menu, _, keys) in ACCESS_KEYS {
            let (_, rows) = MENU_LAYOUT.iter().find(|(m, _)| m == menu).unwrap();
            let targets: BTreeSet<&str> = rows
                .iter()
                .filter_map(|r| match r {
                    Row::Cmd { id, .. } => Some(*id),
                    Row::Sub { label, .. } => Some(*label),
                    Row::Sep => None,
                })
                // Built from the recent files, not from the layout.
                .chain((*menu == "File").then_some(menus::OPEN_RECENT))
                .collect();
            let mut seen = BTreeSet::new();
            for (target, key) in *keys {
                assert!(targets.contains(target), "{menu}: {target} is not a row of the menu");
                assert!(seen.insert(*target), "{menu}: {target} has two access keys");
                assert!(key.is_ascii_uppercase() || key.is_ascii_digit(), "{menu}: {target} has the key {key:?}");
            }
        }
    }

    /// Painting has a catch-all arm for the menu icons, so a variant without a drawing would
    /// leave its row's slot empty: every icon the layout names must paint something.
    #[test]
    fn every_layout_icon_draws() {
        let ctx = egui::Context::default();
        let mut blank = Vec::new();
        let mut seen = 0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for row in all_rows() {
                let (Row::Cmd { icon: Some(icon), .. } | Row::Sub { icon: Some(icon), .. }) = row else { continue };
                let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new(("layout icon", icon)));
                let slot = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(18.0));
                crate::icons::paint(&ui.ctx().layer_painter(layer), slot, icon, false);
                seen += 1;
                if ui.ctx().graphics(|g| g.get(layer).is_none_or(|shapes| shapes.is_empty())) {
                    blank.push(icon);
                }
            }
        });
        output.textures_delta.clear();
        assert!(seen > 100, "{seen} rows have an icon");
        assert!(blank.is_empty(), "these icons paint nothing: {blank:?}");
    }

    /// A flattened row is translated part by part, so each part must be a string the catalogs
    /// already cover: a segment of that command's own menu path.
    #[test]
    fn flattened_labels_come_from_the_command_menu_path() {
        for row in all_rows() {
            let Row::Cmd { id, label: Some(label), .. } = row else { continue };
            let (heading, item) = label.split_once(FLAT).unwrap_or_else(|| panic!("{id}: only flattened rows are relabelled, got {label}"));
            let path = cadcraft_engine::find_command(id).unwrap().menu;
            assert_eq!(path.get(path.len().wrapping_sub(2)..), Some(&[heading, item][..]), "{id}");
        }
    }
}
