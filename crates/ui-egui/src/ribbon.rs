//! The Ribbon: a Windows-style tabbed command bar, with tabs, panels and buttons where 2D
//! drafting users on Windows expect them (observed black-box). Every button runs a command (`app.start`); a command
//! CADCraft doesn't have yet shows greyed out in its place, so positions stay where users expect
//! them. Icons are CADCraft's own (`icons.rs`); a command without one shows its initial.
//!
//! Panels that don't fit the window collapse from the right into a single button that opens the
//! panel's commands as a menu, as Windows CAD ribbons do.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::CadApp;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// One ribbon command: id (as `app.start` takes it), label (English source, translated by the
/// interface catalog) and icon.
#[derive(Clone, Copy)]
pub struct Cmd {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: Option<Icon>,
}

const fn c(id: &'static str, label: &'static str, icon: Icon) -> Cmd {
    Cmd { id, label, icon: Some(icon) }
}
const fn n(id: &'static str, label: &'static str) -> Cmd {
    Cmd { id, label, icon: None }
}

/// A current-value list in a panel (layer, colour, styles…).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Widget {
    Layer,
    Color,
    Lineweight,
    Linetype,
    TextStyle,
    DimStyle,
    MleaderStyle,
    TableStyle,
    NamedView,
}

/// A column of a panel.
pub enum Cell {
    /// Large button (icon above, label below); `variants` open from its lower half (▾).
    Big(Cmd, &'static [Cmd]),
    /// Up to three small buttons (icon, label to the right), each with optional variants.
    Small(&'static [(Cmd, &'static [Cmd])]),
    /// Icon-only buttons, three per column.
    Icons(&'static [Cmd]),
    /// Current-value lists stacked in one column of the given width.
    Lists(&'static [Widget], f32),
    /// A list on the first row and icon buttons on the two rows under it (the Layers panel).
    ListIcons(Widget, f32, &'static [Cmd]),
}

pub struct PanelDef {
    pub title: &'static str,
    pub cells: &'static [Cell],
    /// Commands under the panel title's ▾ (the expanded panel).
    pub more: &'static [Cmd],
}

pub struct TabDef {
    pub id: &'static str,
    pub title: &'static str,
    pub panels: &'static [PanelDef],
}

// ------------------------------------------------------------------------------------------
// Tabs (the Windows 2D drafting layout; 3D tabs are left out on purpose)
// ------------------------------------------------------------------------------------------

const CIRCLES: &[Cmd] = &[
    c("circle", "Center, Radius", Icon::Circle),
    n("circle.cd", "Center, Diameter"),
    n("circle.2p", "2-Point"),
    n("circle.3p", "3-Point"),
    n("circle.ttr", "Tan, Tan, Radius"),
    n("circle.ttt", "Tan, Tan, Tan"),
];
const ARCS: &[Cmd] = &[
    c("arc", "3-Point", Icon::Arc),
    n("arc.sce", "Start, Center, End"),
    n("arc.sca", "Start, Center, Angle"),
    n("arc.scl", "Start, Center, Length"),
    n("arc.sea", "Start, End, Angle"),
    n("arc.sed", "Start, End, Direction"),
    n("arc.ser", "Start, End, Radius"),
    n("arc.cse", "Center, Start, End"),
    n("arc.csa", "Center, Start, Angle"),
    n("arc.csl", "Center, Start, Length"),
    n("arc.continue", "Continue"),
];
const RECTS: &[Cmd] = &[c("rectang", "Rectangle", Icon::Rectangle), c("polygon", "Polygon", Icon::Polygon)];
const ELLIPSES: &[Cmd] = &[c("ellipse", "Center", Icon::Ellipse), n("ellipse.axis", "Axis, End"), n("ellipse.arc", "Elliptical Arc")];
const HATCHES: &[Cmd] = &[c("hatch", "Hatch", Icon::Hatch), c("gradient", "Gradient", Icon::Gradient), c("boundary", "Boundary", Icon::Boundary)];
const TRIMS: &[Cmd] = &[c("trim", "Trim", Icon::Trim), c("extend", "Extend", Icon::Extend)];
const FILLETS: &[Cmd] = &[c("fillet", "Fillet", Icon::Fillet), c("chamfer", "Chamfer", Icon::Chamfer), n("blend", "Blend Curves")];
const ARRAYS: &[Cmd] =
    &[c("arrayrect", "Rectangular Array", Icon::ArrayRect), n("arraypath", "Path Array"), c("arraypolar", "Polar Array", Icon::ArrayPolar)];
const TEXTS: &[Cmd] = &[c("mtext", "Multiline Text", Icon::MText), c("text", "Single Line", Icon::Text)];
const DIMS: &[Cmd] = &[
    c("dimlinear", "Linear", Icon::DimLinear),
    c("dimaligned", "Aligned", Icon::DimAligned),
    c("dimangular", "Angular", Icon::DimAngular),
    c("dimarc", "Arc Length", Icon::DimArc),
    c("dimradius", "Radius", Icon::DimRadius),
    c("dimdiameter", "Diameter", Icon::DimDiameter),
    n("dimjogged", "Jogged"),
    c("dimordinate", "Ordinate", Icon::DimOrdinate),
];
const LEADERS: &[Cmd] = &[
    c("mleader", "Multileader", Icon::MLeader),
    c("mleaderedit.add", "Add Leader", Icon::AddLeader),
    c("mleaderedit.remove", "Remove Leader", Icon::RemoveLeader),
];
const INSERTS: &[Cmd] = &[c("insert", "Insert", Icon::Insert), c("ui.dialog.blocks", "Blocks Palette", Icon::Blocks)];
const ATT_EDITS: &[Cmd] = &[n("eattedit", "Single"), n("-attedit", "Multiple"), n("attsync", "Synchronize")];
const MEASURES: &[Cmd] = &[
    c("measuregeom", "Distance", Icon::Distance),
    n("measuregeom.radius", "Radius"),
    n("measuregeom.angle", "Angle"),
    c("area", "Area", Icon::Area),
];
const PASTES: &[Cmd] =
    &[n("pasteclip", "Paste"), n("pasteblock", "Paste as Block"), n("pasteorig", "Paste to Original Coordinates"), n("pastespec", "Paste Special")];
const ATT_DISPLAY: &[Cmd] =
    &[n("attdisp.normal", "Retain Attribute Display"), n("attdisp.on", "Display All Attributes"), n("attdisp.off", "Hide All Attributes")];
const BLOCK_CREATE: &[Cmd] = &[c("block", "Create Block", Icon::BlockCreate), n("wblock", "Write Block")];
const PDF_IMPORTS: &[Cmd] = &[n("pdfimport", "PDF Import"), n("import", "Import")];
const REVCLOUDS: &[Cmd] = &[
    c("revcloud", "Rectangular", Icon::RevCloud),
    n("revcloud.polygonal", "Polygonal"),
    n("revcloud.freehand", "Freehand"),
    n("revcloud.object", "Object"),
];
const VPORT_CONFIGS: &[Cmd] =
    &[n("vports.1", "Single"), n("vports.2", "Two"), n("vports.3", "Three"), n("vports.4", "Four"), n("vports", "Named...")];
const EXPORTS: &[Cmd] = &[n("exportpdf", "PDF"), n("exportdwf", "DWF"), n("exportdwfx", "DWFx"), n("export", "Other Formats")];
const CONTINUES: &[Cmd] = &[c("dimcontinue", "Continue", Icon::DimContinue), c("dimbaseline", "Baseline", Icon::DimBaseline)];

const NONE: &[Cmd] = &[];

pub const TABS: &[TabDef] = &[
    TabDef {
        id: "home",
        title: "Home",
        panels: &[
            PanelDef {
                title: "Draw",
                cells: &[
                    Cell::Big(c("line", "Line", Icon::Line), NONE),
                    Cell::Big(c("pline", "Polyline", Icon::Polyline), NONE),
                    Cell::Big(c("circle", "Circle", Icon::Circle), CIRCLES),
                    Cell::Big(c("arc", "Arc", Icon::Arc), ARCS),
                    Cell::Small(&[
                        (c("rectang", "Rectangle", Icon::Rectangle), RECTS),
                        (c("ellipse", "Ellipse", Icon::Ellipse), ELLIPSES),
                        (c("hatch", "Hatch", Icon::Hatch), HATCHES),
                    ]),
                ],
                more: &[
                    c("spline", "Spline Fit", Icon::Spline),
                    c("xline", "Construction Line", Icon::XLine),
                    c("ray", "Ray", Icon::Ray),
                    c("point", "Multiple Points", Icon::Point),
                    n("divide", "Divide"),
                    n("measure", "Measure"),
                    c("polygon", "Polygon", Icon::Polygon),
                    c("donut", "Donut", Icon::Donut),
                    c("region", "Region", Icon::Region),
                    c("wipeout", "Wipeout", Icon::Wipeout),
                    c("revcloud", "Revision Cloud", Icon::RevCloud),
                    n("3dpoly", "3D Polyline"),
                ],
            },
            PanelDef {
                title: "Modify",
                cells: &[
                    Cell::Small(&[
                        (c("move", "Move", Icon::Move), NONE),
                        (c("copy", "Copy", Icon::Copy), NONE),
                        (c("stretch", "Stretch", Icon::Stretch), NONE),
                    ]),
                    Cell::Small(&[
                        (c("rotate", "Rotate", Icon::Rotate), NONE),
                        (c("mirror", "Mirror", Icon::Mirror), NONE),
                        (c("scale", "Scale", Icon::Scale), NONE),
                    ]),
                    Cell::Small(&[
                        (c("trim", "Trim", Icon::Trim), TRIMS),
                        (c("fillet", "Fillet", Icon::Fillet), FILLETS),
                        (c("arrayrect", "Array", Icon::ArrayRect), ARRAYS),
                    ]),
                    Cell::Icons(&[c("erase", "Erase", Icon::Erase), c("explode", "Explode", Icon::Explode), c("offset", "Offset", Icon::Offset)]),
                ],
                more: &[
                    c("break", "Break", Icon::Break),
                    n("breakatpoint", "Break at Point"),
                    c("join", "Join", Icon::Join),
                    c("lengthen", "Lengthen", Icon::Lengthen),
                    c("chamfer", "Chamfer", Icon::Chamfer),
                    n("align", "Align"),
                    n("reverse", "Reverse"),
                    n("pedit", "Edit Polyline"),
                    n("splinedit", "Edit Spline"),
                    n("hatchedit", "Edit Hatch"),
                    n("setbylayer", "Set to ByLayer"),
                    n("overkill", "Delete Duplicate Objects"),
                    n("draworder", "Draw Order"),
                    n("flatten", "Flatten"),
                ],
            },
            PanelDef {
                title: "Annotation",
                cells: &[
                    Cell::Big(c("mtext", "Text", Icon::MText), TEXTS),
                    Cell::Big(c("dim", "Dimension", Icon::DimLinear), NONE),
                    Cell::Small(&[
                        (c("dimlinear", "Linear", Icon::DimLinear), DIMS),
                        (c("mleader", "Leader", Icon::MLeader), LEADERS),
                        (c("table", "Table", Icon::Table), NONE),
                    ]),
                ],
                more: &[
                    n("style", "Text Style"),
                    n("dimstyle", "Dimension Style"),
                    n("mleaderstyle", "Multileader Style"),
                    n("tablestyle", "Table Style"),
                ],
            },
            PanelDef {
                title: "Layers",
                cells: &[
                    Cell::Big(c("ui.dialog.layers", "Layer Properties", Icon::LayerProps), NONE),
                    Cell::ListIcons(
                        Widget::Layer,
                        168.0,
                        &[
                            c("layoff", "Off", Icon::LayerOff),
                            c("layiso", "Isolate", Icon::LayerIso),
                            c("layfrz", "Freeze", Icon::LayerFreeze),
                            c("laylck", "Lock", Icon::LayerLock),
                            n("layon", "Turn All Layers On"),
                            n("layuniso", "Unisolate"),
                            n("laythw", "Thaw All Layers"),
                            c("layulk", "Unlock", Icon::Unlock),
                            c("layerp", "Previous", Icon::LayerPrev),
                        ],
                    ),
                    Cell::Small(&[
                        (c("laymcur", "Make Current", Icon::MakeCurrent), NONE),
                        (c("laymch", "Match Layer", Icon::LayerMatch), NONE),
                        (n("copytolayer", "Copy to Layer"), NONE),
                    ]),
                ],
                more: &[
                    n("layon", "Turn All Layers On"),
                    n("laythw", "Thaw All Layers"),
                    n("layuniso", "Unisolate"),
                    n("laywalk", "Layer Walk"),
                    n("laymrg", "Merge"),
                    n("laydel", "Delete"),
                    n("layvpi", "Isolate to Current Viewport"),
                    n("layerstate", "Layer States"),
                ],
            },
            PanelDef {
                title: "Block",
                cells: &[
                    Cell::Big(c("insert", "Insert", Icon::Insert), INSERTS),
                    Cell::Small(&[
                        (c("block", "Create", Icon::BlockCreate), NONE),
                        (c("bedit", "Edit", Icon::BlockEdit), NONE),
                        (n("eattedit", "Edit Attributes"), ATT_EDITS),
                    ]),
                ],
                more: &[c("attdef", "Define Attributes", Icon::AttDef), n("battman", "Manage Attributes"), n("attsync", "Synchronize Attributes")],
            },
            PanelDef {
                title: "Properties",
                cells: &[
                    Cell::Big(c("matchprop", "Match Properties", Icon::MatchProp), NONE),
                    Cell::Lists(&[Widget::Color, Widget::Lineweight, Widget::Linetype], 140.0),
                ],
                more: &[n("transparency", "Transparency"), c("list", "List", Icon::List), n("setbylayer", "Set to ByLayer")],
            },
            PanelDef {
                title: "Groups",
                cells: &[
                    Cell::Big(n("group", "Group"), NONE),
                    Cell::Icons(&[n("ungroup", "Ungroup"), n("groupedit", "Group Edit"), n("pickstyle", "Group Selection On/Off")]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Utilities",
                cells: &[
                    Cell::Big(c("measuregeom", "Measure", Icon::Measure), MEASURES),
                    Cell::Icons(&[
                        c("ui.dialog.qselect", "Quick Select", Icon::QuickSelect),
                        n("ui.panel.quickcalc", "QuickCalc"),
                        n("id", "ID Point"),
                    ]),
                ],
                more: &[n("selectall", "Select All"), n("count", "Count")],
            },
            PanelDef {
                title: "Clipboard",
                cells: &[
                    Cell::Big(n("pasteclip", "Paste"), PASTES),
                    Cell::Icons(&[n("copyclip", "Copy Clip"), n("cutclip", "Cut"), n("copybase", "Copy with Base Point")]),
                ],
                more: NONE,
            },
        ],
    },
    TabDef {
        id: "insert",
        title: "Insert",
        panels: &[
            PanelDef {
                title: "Block",
                cells: &[
                    Cell::Big(c("insert", "Insert", Icon::Insert), INSERTS),
                    Cell::Small(&[(n("eattedit", "Edit Attribute"), ATT_EDITS), (n("attdisp", "Retain Attribute Display"), ATT_DISPLAY)]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Block Definition",
                cells: &[
                    Cell::Big(c("block", "Create Block", Icon::BlockCreate), BLOCK_CREATE),
                    Cell::Big(c("attdef", "Define Attributes", Icon::AttDef), NONE),
                    Cell::Big(n("battman", "Manage Attributes"), NONE),
                    Cell::Big(c("bedit", "Block Editor", Icon::BlockEdit), NONE),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Reference",
                cells: &[
                    Cell::Big(c("attach", "Attach", Icon::Attach), NONE),
                    Cell::Big(n("clip", "Clip"), NONE),
                    Cell::Big(n("adjust", "Adjust"), NONE),
                    Cell::Small(&[
                        (n("xref", "External References"), NONE),
                        (n("frame", "Frames vary"), NONE),
                        (n("uosnap", "Snap to Underlays"), NONE),
                    ]),
                ],
                more: &[n("xclip", "Clip Xref"), n("xbind", "Bind"), n("imageattach", "Attach Image"), n("pdfattach", "Attach PDF")],
            },
            PanelDef {
                title: "Import",
                cells: &[
                    Cell::Big(n("pdfimport", "PDF Import"), PDF_IMPORTS),
                    Cell::Small(&[(n("pdfshxtext", "Recognize SHX Text"), NONE), (n("pdfshxtextoptions", "Recognition Settings"), NONE)]),
                    Cell::Big(n("txt2mtxt", "Combine Text"), NONE),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Data",
                cells: &[
                    Cell::Big(n("field", "Field"), NONE),
                    Cell::Small(&[
                        (n("updatefield", "Update Fields"), NONE),
                        (n("insertobj", "OLE Object"), NONE),
                        (n("hyperlink", "Hyperlink"), NONE),
                    ]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Linking & Extraction",
                cells: &[
                    Cell::Big(n("datalink", "Data Link"), NONE),
                    Cell::Icons(&[n("dataextraction", "Extract Data"), n("datalinkupdate", "Update Data Links")]),
                ],
                more: NONE,
            },
            PanelDef { title: "Location", cells: &[Cell::Big(n("geographiclocation", "Set Location"), NONE)], more: NONE },
        ],
    },
    TabDef {
        id: "annotate",
        title: "Annotate",
        panels: &[
            PanelDef {
                title: "Text",
                cells: &[
                    Cell::Big(c("mtext", "Multiline Text", Icon::MText), TEXTS),
                    Cell::Lists(&[Widget::TextStyle], 170.0),
                    Cell::Icons(&[n("spell", "Check Spelling"), c("find", "Find Text", Icon::Search), n("scaletext", "Scale Text")]),
                ],
                more: &[n("justifytext", "Justify"), n("textalign", "Text Align"), n("style", "Text Style")],
            },
            PanelDef {
                title: "Dimensions",
                cells: &[
                    Cell::Big(c("dim", "Dimension", Icon::DimLinear), NONE),
                    Cell::Lists(&[Widget::DimStyle], 170.0),
                    Cell::Small(&[
                        (c("dimlinear", "Linear", Icon::DimLinear), DIMS),
                        (c("qdim", "Quick", Icon::QDim), NONE),
                        (c("dimcontinue", "Continue", Icon::DimContinue), CONTINUES),
                    ]),
                    Cell::Icons(&[
                        n("dimbreak", "Break"),
                        n("dimspace", "Adjust Space"),
                        n("diminspect", "Inspect"),
                        n("dimjogline", "Jog Line"),
                        n("dimreassociate", "Reassociate"),
                        n("dimtedit", "Text Angle"),
                    ]),
                ],
                more: &[n("dimstyle", "Dimension Style"), n("dimoverride", "Override"), n("dimedit.oblique", "Oblique"), n("tolerance", "Tolerance")],
            },
            PanelDef {
                title: "Centerlines",
                cells: &[Cell::Big(n("centermark", "Center Mark"), NONE), Cell::Big(n("centerline", "Centerline"), NONE)],
                more: NONE,
            },
            PanelDef {
                title: "Leaders",
                cells: &[
                    Cell::Big(c("mleader", "Multileader", Icon::MLeader), NONE),
                    Cell::Lists(&[Widget::MleaderStyle], 160.0),
                    Cell::Small(&[
                        (c("mleaderedit.add", "Add Leader", Icon::AddLeader), NONE),
                        (c("mleaderedit.remove", "Remove Leader", Icon::RemoveLeader), NONE),
                    ]),
                    Cell::Icons(&[c("mleaderalign", "Align", Icon::AlignLeaders), n("mleadercollect", "Collect")]),
                ],
                more: &[n("mleaderstyle", "Multileader Style")],
            },
            PanelDef {
                title: "Tables",
                cells: &[
                    Cell::Big(c("table", "Table", Icon::Table), NONE),
                    Cell::Lists(&[Widget::TableStyle], 150.0),
                    Cell::Small(&[(n("datalink", "Link Data"), NONE), (n("dataextraction", "Extract Data"), NONE)]),
                ],
                more: &[n("tablestyle", "Table Style"), n("tableexport", "Export")],
            },
            PanelDef {
                title: "Markup",
                cells: &[
                    Cell::Big(c("wipeout", "Wipeout", Icon::Wipeout), NONE),
                    Cell::Big(c("revcloud", "Revision Cloud", Icon::RevCloud), REVCLOUDS),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Annotation Scaling",
                cells: &[
                    Cell::Big(c("objectscale", "Add Current Scale", Icon::Annotation), NONE),
                    Cell::Small(&[(n("scalelistedit", "Scale List"), NONE), (n("annoupdate", "Sync Scale Positions"), NONE)]),
                ],
                more: NONE,
            },
        ],
    },
    TabDef {
        id: "parametric",
        title: "Parametric",
        panels: &[
            PanelDef {
                title: "Geometric",
                cells: &[
                    Cell::Big(c("autoconstrain", "Auto Constrain", Icon::Constraint), NONE),
                    Cell::Icons(&[
                        c("gccoincident", "Coincident", Icon::Coincident),
                        c("gcparallel", "Parallel", Icon::Parallel),
                        c("gcperpendicular", "Perpendicular", Icon::Perpendicular),
                    ]),
                    Cell::Icons(&[
                        c("gchorizontal", "Horizontal", Icon::Horizontal),
                        c("gcvertical", "Vertical", Icon::Vertical),
                        c("gctangent", "Tangent", Icon::Tangent),
                    ]),
                    Cell::Icons(&[
                        c("gcconcentric", "Concentric", Icon::Concentric),
                        c("gcequal", "Equal", Icon::Equal),
                        c("gcfix", "Fix", Icon::Fix),
                    ]),
                    Cell::Small(&[
                        (n("constraintbar", "Show/Hide"), NONE),
                        (n("constraintbar.showall", "Show All"), NONE),
                        (n("constraintbar.hideall", "Hide All"), NONE),
                    ]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Dimensional",
                cells: &[
                    Cell::Small(&[(n("dclinear", "Linear"), NONE), (n("dcaligned", "Aligned"), NONE), (n("dcangular", "Angular"), NONE)]),
                    Cell::Small(&[(n("dcradius", "Radius"), NONE), (n("dcdiameter", "Diameter"), NONE), (n("dcconvert", "Convert"), NONE)]),
                    Cell::Small(&[
                        (n("dcdisplay", "Show/Hide"), NONE),
                        (n("dcdisplay.showall", "Show All"), NONE),
                        (n("dcdisplay.hideall", "Hide All"), NONE),
                    ]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Manage",
                cells: &[Cell::Big(n("delconstraint", "Delete Constraints"), NONE), Cell::Big(n("ui.dialog.parameters", "Parameters Manager"), NONE)],
                more: NONE,
            },
        ],
    },
    TabDef {
        id: "view",
        title: "View",
        panels: &[
            PanelDef {
                title: "Viewport Tools",
                cells: &[
                    Cell::Big(n("ui.toggle.ucsicon", "UCS Icon"), NONE),
                    Cell::Big(n("ui.toggle.viewcube", "Navigation Cube"), NONE),
                    Cell::Big(n("navbar", "Navigation Bar"), NONE),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Named Views",
                cells: &[Cell::Lists(&[Widget::NamedView], 170.0), Cell::Small(&[(n("view", "New View"), NONE), (n("view", "View Manager"), NONE)])],
                more: NONE,
            },
            PanelDef {
                title: "Model Viewports",
                cells: &[
                    Cell::Big(n("vports", "Viewport Configuration"), VPORT_CONFIGS),
                    Cell::Small(&[(n("vports", "Named"), NONE), (n("-vports.join", "Join"), NONE), (n("vports.1", "Restore"), NONE)]),
                ],
                more: NONE,
            },
            PanelDef { title: "Review", cells: &[Cell::Big(n("compare", "DWG Compare"), NONE)], more: NONE },
            PanelDef { title: "History", cells: &[Cell::Big(n("activityinsights", "Activity Insights"), NONE)], more: NONE },
            PanelDef {
                title: "Palettes",
                cells: &[
                    Cell::Big(n("toolpalettes", "Tool Palettes"), NONE),
                    Cell::Big(c("ui.panel.properties", "Properties", Icon::Properties), NONE),
                    Cell::Big(c("ui.dialog.blocks", "Blocks", Icon::Blocks), NONE),
                    Cell::Big(n("count", "Count"), NONE),
                    Cell::Big(n("sheetset", "Sheet Set Manager"), NONE),
                    Cell::Icons(&[
                        n("ui.toggle.cmdline", "Command Line"),
                        c("ui.panel.layers", "Layers", Icon::Layers),
                        n("ui.panel.quickcalc", "QuickCalc"),
                    ]),
                    Cell::Icons(&[n("xref", "External References"), n("ui.toggle.history", "Text Window"), n("markupimport", "Markup Import")]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Interface",
                cells: &[
                    Cell::Big(c("ui.switcher", "Switch Windows", Icon::Switcher), NONE),
                    Cell::Big(n("ui.toggle.filetabs", "File Tabs"), NONE),
                    Cell::Big(n("ui.toggle.layouttabs", "Layout Tabs"), NONE),
                    Cell::Small(&[
                        (n("ui.tile.horizontal", "Tile Horizontally"), NONE),
                        (n("ui.tile.vertical", "Tile Vertically"), NONE),
                        (n("ui.cascade", "Cascade"), NONE),
                    ]),
                ],
                more: NONE,
            },
        ],
    },
    TabDef {
        id: "manage",
        title: "Manage",
        panels: &[
            PanelDef {
                title: "Customization",
                cells: &[
                    Cell::Big(n("cui", "User Interface"), NONE),
                    Cell::Big(n("toolpalettes", "Tool Palettes"), NONE),
                    Cell::Small(&[(n("cuiimport", "Import"), NONE), (n("cuiexport", "Export"), NONE), (n("aliasedit", "Edit Aliases"), NONE)]),
                ],
                more: NONE,
            },
            PanelDef {
                title: "Applications",
                cells: &[Cell::Big(n("appload", "Load Application"), NONE), Cell::Big(n("script", "Run Script"), NONE)],
                more: NONE,
            },
            PanelDef {
                title: "Cleanup",
                cells: &[
                    Cell::Big(n("purge.find", "Find Non-Purgeable Items"), NONE),
                    Cell::Small(&[(n("purge", "Purge"), NONE), (n("overkill", "Overkill"), NONE), (n("audit", "Audit"), NONE)]),
                ],
                more: NONE,
            },
        ],
    },
    TabDef {
        id: "output",
        title: "Output",
        panels: &[
            PanelDef {
                title: "Plot",
                cells: &[
                    Cell::Big(c("plot", "Plot", Icon::Plot), NONE),
                    Cell::Big(c("publish", "Batch Plot", Icon::Publish), NONE),
                    Cell::Big(c("preview", "Preview", Icon::Preview), NONE),
                    Cell::Small(&[
                        (c("pagesetup", "Page Setup Manager", Icon::PageSetup), NONE),
                        (n("viewplotdetails", "View Details"), NONE),
                        (n("plottermanager", "Plotter Manager"), NONE),
                    ]),
                ],
                more: NONE,
            },
            PanelDef { title: "Export to DWF/PDF", cells: &[Cell::Big(c("exportpdf", "Export", Icon::Export), EXPORTS)], more: NONE },
        ],
    },
    TabDef {
        id: "collaborate",
        title: "Collaborate",
        panels: &[
            PanelDef {
                title: "Share",
                cells: &[Cell::Big(c("share", "Share Drawing", Icon::Share), NONE), Cell::Big(n("sharedviews", "Shared Views"), NONE)],
                more: NONE,
            },
            PanelDef { title: "Compare", cells: &[Cell::Big(n("compare", "DWG Compare"), NONE)], more: NONE },
        ],
    },
];

// ------------------------------------------------------------------------------------------
// Layout
// ------------------------------------------------------------------------------------------

/// Height of the tab row.
pub const TAB_ROW: f32 = 24.0;
/// Height of the panel body (three small-button rows) and of the panel title row under it.
pub(crate) const BODY: f32 = 72.0;
pub(crate) const TITLE: f32 = 18.0;
/// Whole ribbon height when expanded.
pub const HEIGHT: f32 = TAB_ROW + BODY + TITLE + 4.0;

pub(crate) const ROW: f32 = BODY / 3.0;
pub(crate) const BIG_ICON: f32 = 30.0;
pub(crate) const SMALL_ICON: f32 = 16.0;
pub(crate) const PAD: f32 = 3.0;

/// Ribbon text is a little smaller than the palettes' body text.
pub(crate) fn font() -> FontId {
    FontId::proportional(11.0)
}

/// A tab's title: the catalog's `Tab: <title>` row when the tab is named differently from a
/// button with the same English word (Insert tab, Insert button), else the plain label.
fn tab_label(title: &str) -> String {
    let key = format!("Tab: {title}");
    let tab = crate::i18n::ribbon(&key);
    if tab == key { crate::i18n::ribbon(title).to_string() } else { tab.to_string() }
}

/// Whether the command exists (else its button is greyed out).
pub fn known(id: &str) -> bool {
    cadcraft_engine::find_command(id).is_some() || crate::menus::is_ui_command(id)
}

pub(crate) fn text_w(ui: &egui::Ui, s: &str) -> f32 {
    ui.painter().layout_no_wrap(crate::i18n::ribbon(s).to_string(), font(), Color32::WHITE).size().x
}

/// A big button's label in up to two lines (split at the space nearest the middle).
fn two_lines(label: &str) -> (String, Option<String>) {
    let s = crate::i18n::ribbon(label);
    if s.chars().count() <= 10 || !s.contains(' ') {
        return (s.to_string(), None);
    }
    let mid = s.len() / 2;
    let split = s.char_indices().filter(|(_, ch)| *ch == ' ').map(|(i, _)| i).min_by_key(|i| i.abs_diff(mid)).unwrap_or(mid);
    let (a, b) = s.split_at(split);
    (a.to_string(), Some(b.trim_start().to_string()))
}

pub(crate) fn cell_width(ui: &egui::Ui, cell: &Cell) -> f32 {
    match cell {
        Cell::Big(cmd, variants) => {
            let (a, b) = two_lines(cmd.label);
            let w = |s: &str| ui.painter().layout_no_wrap(s.to_string(), font(), Color32::WHITE).size().x;
            let lw = w(&a).max(b.as_deref().map_or(0.0, w)) + if variants.is_empty() { 0.0 } else { 10.0 };
            lw.max(BIG_ICON + 4.0) + 2.0 * PAD
        }
        Cell::Small(rows) => {
            rows.iter().map(|(cmd, v)| SMALL_ICON + 5.0 + text_w(ui, cmd.label) + if v.is_empty() { 2.0 } else { 13.0 }).fold(0.0, f32::max)
                + 2.0 * PAD
        }
        Cell::Icons(cmds) => (cmds.len().div_ceil(3) as f32) * (SMALL_ICON + 8.0) + PAD,
        Cell::Lists(_, w) | Cell::ListIcons(_, w, _) => *w + 2.0 * PAD,
    }
}

fn panel_width(ui: &egui::Ui, panel: &PanelDef) -> f32 {
    let cells: f32 = panel.cells.iter().map(|c| cell_width(ui, c)).sum();
    cells.max(text_w(ui, panel.title) + if panel.more.is_empty() { 12.0 } else { 24.0 }) + PAD
}

/// Width of a collapsed panel (a single button with the panel's title).
fn collapsed_width(ui: &egui::Ui, panel: &PanelDef) -> f32 {
    text_w(ui, panel.title).max(BIG_ICON) + 4.0 * PAD + 10.0
}

/// Which panels are shown in full: all of them while they fit, then the rightmost ones collapse
/// first, as Windows CAD ribbons do when the window narrows.
pub fn fit(full: &[f32], collapsed: &[f32], room: f32) -> Vec<bool> {
    let mut expanded = vec![true; full.len()];
    let total = |e: &[bool]| -> f32 {
        e.iter().enumerate().map(|(i, x)| if *x { full.get(i).copied().unwrap_or(0.0) } else { collapsed.get(i).copied().unwrap_or(0.0) }).sum()
    };
    let mut i = full.len();
    while total(&expanded) > room && i > 0 {
        i -= 1;
        if let Some(e) = expanded.get_mut(i) {
            *e = false;
        }
    }
    expanded
}

// ------------------------------------------------------------------------------------------
// Drawing
// ------------------------------------------------------------------------------------------

/// What a click in the ribbon asks for.
pub(crate) enum Action {
    Start(&'static str),
    /// A contextual tab's change, applied after the ribbon is drawn.
    Do(Box<dyn FnOnce(&mut CadApp)>),
    Tab(&'static str),
    ToggleMinimized,
    Layer(String),
    LayerToggle(String, &'static str, bool),
}

pub(crate) fn hover_fill(ui: &egui::Ui, r: Rect, resp: &egui::Response) {
    if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, Tokens::get().control_hover.gamma_multiply(0.6));
    }
}

pub(crate) fn paint_icon(p: &egui::Painter, r: Rect, cmd: &Cmd, dim: bool) {
    match cmd.icon {
        Some(icon) => icons::paint(p, r, icon, dim),
        None => {
            let t = Tokens::get();
            let col = if dim { t.text_faint } else { t.icon };
            p.rect_stroke(r.shrink(r.width() * 0.08), 2.0, Stroke::new(1.0, col), egui::StrokeKind::Inside);
            let initial: String = crate::i18n::ribbon(cmd.label).chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
            p.text(r.center(), Align2::CENTER_CENTER, initial, FontId::proportional(r.height() * 0.55), col);
        }
    }
}

pub(crate) fn tooltip(cmd: &Cmd, enabled: bool) -> String {
    let label = crate::i18n::ribbon(cmd.label);
    if enabled { format!("{label} ({})", cmd.id.to_uppercase()) } else { format!("{label} — {}", crate::tl!("not available yet")) }
}

/// A menu of command variants (or a collapsed panel's commands).
pub(crate) fn variant_menu(ui: &mut egui::Ui, items: &[Cmd], out: &mut Option<Action>) {
    for v in items {
        let enabled = known(v.id);
        let resp = ui.add_enabled(enabled, egui::Button::new(crate::i18n::ribbon(v.label)));
        crate::control::record_widget(ui, &resp, v.label, "button", None);
        if resp.clicked() {
            *out = Some(Action::Start(v.id));
            ui.close();
        }
    }
}

pub(crate) fn big(ui: &mut egui::Ui, r: Rect, cmd: &Cmd, variants: &'static [Cmd], out: &mut Option<Action>) {
    let t = Tokens::get();
    let enabled = known(cmd.id);
    let top = Rect::from_min_max(r.min, pos2(r.right(), r.top() + BIG_ICON + 8.0));
    let resp = ui.interact(if variants.is_empty() { r } else { top }, ui.id().with(("rb_big", cmd.id, cmd.label)), Sense::click());
    hover_fill(ui, if variants.is_empty() { r } else { top }, &resp);
    paint_icon(ui.painter(), Rect::from_center_size(pos2(r.center().x, r.top() + 4.0 + BIG_ICON / 2.0), vec2(BIG_ICON, BIG_ICON)), cmd, !enabled);
    let (a, b) = two_lines(cmd.label);
    let col = if enabled { t.text } else { t.text_faint };
    let y = r.top() + BIG_ICON + 10.0;
    ui.painter().text(pos2(r.center().x, y), Align2::CENTER_TOP, &a, font(), col);
    if let Some(b) = &b {
        ui.painter().text(pos2(r.center().x, y + 13.0), Align2::CENTER_TOP, b, font(), col);
    }
    icons::describe_control(ui, &resp, cmd.label, None, false);
    if resp.on_hover_text(tooltip(cmd, enabled)).clicked() && enabled {
        *out = Some(Action::Start(cmd.id));
    }
    if !variants.is_empty() {
        let low = Rect::from_min_max(pos2(r.left(), top.bottom()), r.max);
        let vr = ui.interact(low, ui.id().with(("rb_bigv", cmd.id, cmd.label)), Sense::click());
        hover_fill(ui, low, &vr);
        let line_y = y + if b.is_some() { 26.0 } else { 13.0 };
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.center().x, line_y + 4.0), vec2(9.0, 9.0)), Icon::ChevronDown, false);
        egui::Popup::menu(&vr).show(|ui| variant_menu(ui, variants, out));
    }
}

pub(crate) fn small(ui: &mut egui::Ui, r: Rect, cmd: &Cmd, variants: &'static [Cmd], out: &mut Option<Action>) {
    let t = Tokens::get();
    let enabled = known(cmd.id);
    let main = if variants.is_empty() { r } else { Rect::from_min_max(r.min, pos2(r.right() - 12.0, r.bottom())) };
    let resp = ui.interact(main, ui.id().with(("rb_small", cmd.id, cmd.label, r.top() as i32)), Sense::click());
    hover_fill(ui, main, &resp);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 2.0 + SMALL_ICON / 2.0, r.center().y), vec2(SMALL_ICON, SMALL_ICON)),
        cmd,
        !enabled,
    );
    ui.painter().text(
        pos2(r.left() + SMALL_ICON + 6.0, r.center().y),
        Align2::LEFT_CENTER,
        crate::i18n::ribbon(cmd.label),
        font(),
        if enabled { t.text } else { t.text_faint },
    );
    icons::describe_control(ui, &resp, cmd.label, None, false);
    if resp.on_hover_text(tooltip(cmd, enabled)).clicked() && enabled {
        *out = Some(Action::Start(cmd.id));
    }
    if !variants.is_empty() {
        let vr_rect = Rect::from_min_max(pos2(r.right() - 12.0, r.top()), r.max);
        let vr = ui.interact(vr_rect, ui.id().with(("rb_smallv", cmd.id, cmd.label, r.top() as i32)), Sense::click());
        hover_fill(ui, vr_rect, &vr);
        icons::paint(ui.painter(), Rect::from_center_size(vr_rect.center(), vec2(8.0, 8.0)), Icon::ChevronDown, false);
        egui::Popup::menu(&vr).show(|ui| variant_menu(ui, variants, out));
    }
}

fn icon_button(ui: &mut egui::Ui, r: Rect, cmd: &Cmd, out: &mut Option<Action>) {
    let enabled = known(cmd.id);
    let resp = ui.interact(r, ui.id().with(("rb_icon", cmd.id, cmd.label)), Sense::click());
    hover_fill(ui, r, &resp);
    paint_icon(ui.painter(), Rect::from_center_size(r.center(), vec2(SMALL_ICON, SMALL_ICON)), cmd, !enabled);
    icons::describe_control(ui, &resp, cmd.label, None, false);
    if resp.on_hover_text(tooltip(cmd, enabled)).clicked() && enabled {
        *out = Some(Action::Start(cmd.id));
    }
}

/// The current value a list shows, and the command its button opens.
fn widget_value(app: &CadApp, w: Widget) -> (String, &'static str) {
    let Ok(d) = app.session.doc() else { return (String::new(), "ui.noop") };
    let h = &d.header;
    let get = |name: &str, default: &str| {
        let v = h.str(name, default);
        if v.is_empty() { default.to_string() } else { v }
    };
    match w {
        Widget::Layer => (h.str("CLAYER", "0"), "ui.dialog.layers"),
        Widget::Color => (color_name(h.i64("CECOLOR", 256)), "color"),
        Widget::Lineweight => (lineweight_name(h.i64("CELWEIGHT", -1)), "lweight"),
        Widget::Linetype => (by_name(&get("CELTYPE", "ByLayer")), "linetype"),
        Widget::TextStyle => (get("TEXTSTYLE", "Standard"), "style"),
        Widget::DimStyle => (get("DIMSTYLE", "Standard"), "dimstyle"),
        Widget::MleaderStyle => (get("CMLEADERSTYLE", "Standard"), "mleaderstyle"),
        Widget::TableStyle => (get("CTABLESTYLE", "Standard"), "tablestyle"),
        Widget::NamedView => (crate::tl!("Unsaved View").to_string(), "view"),
    }
}

/// ByLayer/ByBlock in the interface language; other names as they are.
fn by_name(name: &str) -> String {
    if name.eq_ignore_ascii_case("ByLayer") {
        crate::tl!("ByLayer").to_string()
    } else if name.eq_ignore_ascii_case("ByBlock") {
        crate::tl!("ByBlock").to_string()
    } else {
        name.to_string()
    }
}

/// The usual CAD name for a colour index (CECOLOR).
pub fn color_name(aci: i64) -> String {
    match aci {
        256 => crate::tl!("ByLayer").to_string(),
        0 => crate::tl!("ByBlock").to_string(),
        1 => crate::tl!("Red").to_string(),
        2 => crate::tl!("Yellow").to_string(),
        3 => crate::tl!("Green").to_string(),
        4 => crate::tl!("Cyan").to_string(),
        5 => crate::tl!("Blue").to_string(),
        6 => crate::tl!("Magenta").to_string(),
        7 => crate::tl!("White").to_string(),
        n => format!("{} {n}", crate::tl!("Color")),
    }
}

/// The usual CAD name for a lineweight value (CELWEIGHT, hundredths of a millimetre).
pub fn lineweight_name(v: i64) -> String {
    match v {
        -1 => crate::tl!("ByLayer").to_string(),
        -2 => crate::tl!("ByBlock").to_string(),
        -3 => crate::tl!("Default").to_string(),
        n if n >= 0 => format!("{:.2} mm", n as f64 / 100.0),
        _ => crate::tl!("ByLayer").to_string(),
    }
}

fn list(ui: &mut egui::Ui, app: &CadApp, r: Rect, w: Widget, out: &mut Option<Action>) {
    let t = Tokens::get();
    let (value, cmd) = widget_value(app, w);
    let resp = ui.interact(r, ui.id().with(("rb_list", w as u8)), Sense::click());
    ui.painter().rect_filled(r, 2.0, if resp.hovered() { t.control_hover } else { t.control });
    let mut x = r.left() + 5.0;
    if w == Widget::Layer
        && let Ok(d) = app.session.doc()
        && let Some(l) = d.layers.iter().find(|l| l.name.eq_ignore_ascii_case(&value))
    {
        for icon in [
            if l.on { Icon::Bulb } else { Icon::BulbOff },
            if l.frozen { Icon::Snowflake } else { Icon::Sun },
            if l.locked { Icon::Lock } else { Icon::Unlock },
        ] {
            icons::paint(ui.painter(), Rect::from_min_size(pos2(x, r.center().y - 7.0), vec2(14.0, 14.0)), icon, false);
            x += 16.0;
        }
        crate::layers::swatch(ui.painter(), Rect::from_min_size(pos2(x + 1.0, r.center().y - 5.0), vec2(10.0, 10.0)), l.color);
        x += 16.0;
    }
    let galley = ui.painter().layout_no_wrap(value.clone(), font(), t.text);
    let room = (r.right() - 16.0 - x).max(0.0);
    let clip = Rect::from_min_max(pos2(x, r.top()), pos2(x + room, r.bottom()));
    ui.painter().with_clip_rect(clip).galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.right() - 8.0, r.center().y), vec2(9.0, 9.0)), Icon::ChevronDown, false);
    resp.clone().on_hover_text(&value);
    if w == Widget::Layer {
        let layers: Vec<(String, bool, bool, bool, cadcraft_color::Color)> =
            app.session.doc().map(|d| d.layers.iter().map(|l| (l.name.clone(), l.on, l.frozen, l.locked, l.color)).collect()).unwrap_or_default();
        egui::Popup::from_toggle_button_response(&resp)
            .width(r.width().max(220.0))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for (name, on, frozen, locked, color) in &layers {
                        ui.horizontal(|ui| {
                            if icons::button(ui, if *on { Icon::Bulb } else { Icon::BulbOff }, 18.0, "On/Off", false).clicked() {
                                *out = Some(Action::LayerToggle(name.clone(), "on", !*on));
                            }
                            if icons::button(ui, if *frozen { Icon::Snowflake } else { Icon::Sun }, 18.0, "Freeze/Thaw", false).clicked() {
                                *out = Some(Action::LayerToggle(name.clone(), "frozen", !*frozen));
                            }
                            if icons::button(ui, if *locked { Icon::Lock } else { Icon::Unlock }, 18.0, "Lock/Unlock", false).clicked() {
                                *out = Some(Action::LayerToggle(name.clone(), "locked", !*locked));
                            }
                            let (sr, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                            crate::layers::swatch(ui.painter(), sr, *color);
                            if ui.selectable_label(name.eq_ignore_ascii_case(&value), name).clicked() {
                                *out = Some(Action::Layer(name.clone()));
                                ui.close();
                            }
                        });
                    }
                });
            });
    } else if resp.clicked() && known(cmd) {
        *out = Some(Action::Start(cmd));
    }
}

pub(crate) fn draw_cell(ui: &mut egui::Ui, app: &CadApp, cell: &Cell, r: Rect, out: &mut Option<Action>) {
    match cell {
        Cell::Big(cmd, v) => big(ui, r.shrink2(vec2(PAD / 2.0, 0.0)), cmd, v, out),
        Cell::Small(rows) => {
            for (i, (cmd, v)) in rows.iter().enumerate() {
                let rr = Rect::from_min_size(pos2(r.left() + PAD, r.top() + i as f32 * ROW + 1.0), vec2(r.width() - 2.0 * PAD, ROW - 2.0));
                small(ui, rr, cmd, v, out);
            }
        }
        Cell::Icons(cmds) => {
            for (i, cmd) in cmds.iter().enumerate() {
                let (col, row) = (i / 3, i % 3);
                let rr = Rect::from_min_size(
                    pos2(r.left() + col as f32 * (SMALL_ICON + 8.0), r.top() + row as f32 * ROW + 1.0),
                    vec2(SMALL_ICON + 6.0, ROW - 2.0),
                );
                icon_button(ui, rr, cmd, out);
            }
        }
        Cell::Lists(ws, w) => {
            for (i, wd) in ws.iter().enumerate() {
                let rr = Rect::from_min_size(pos2(r.left() + PAD, r.top() + i as f32 * ROW + 2.0), vec2(*w, ROW - 4.0));
                list(ui, app, rr, *wd, out);
            }
        }
        Cell::ListIcons(wd, w, cmds) => {
            list(ui, app, Rect::from_min_size(pos2(r.left() + PAD, r.top() + 2.0), vec2(*w, ROW - 4.0)), *wd, out);
            let per_row = ((*w / (SMALL_ICON + 8.0)).floor() as usize).max(1);
            for (i, cmd) in cmds.iter().enumerate() {
                let (row, col) = (1 + i / per_row, i % per_row);
                let rr = Rect::from_min_size(
                    pos2(r.left() + PAD + col as f32 * (SMALL_ICON + 8.0), r.top() + row as f32 * ROW + 1.0),
                    vec2(SMALL_ICON + 6.0, ROW - 2.0),
                );
                icon_button(ui, rr, cmd, out);
            }
        }
    }
}

/// Every command of a panel, for its collapsed button's menu.
fn panel_commands(panel: &PanelDef) -> Vec<Cmd> {
    let mut v = Vec::new();
    for cell in panel.cells {
        match cell {
            Cell::Big(cmd, _) => v.push(*cmd),
            Cell::Small(rows) => v.extend(rows.iter().map(|(cmd, _)| *cmd)),
            Cell::Icons(cmds) => v.extend(cmds.iter().copied()),
            Cell::Lists(..) => {}
            Cell::ListIcons(_, _, cmds) => v.extend(cmds.iter().copied()),
        }
    }
    v.extend(panel.more.iter().copied());
    v
}

/// The ribbon (tab row and the active tab's panels).
pub fn ribbon(app: &mut CadApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let h = if app.ui.ribbon_minimized { TAB_ROW + 2.0 } else { HEIGHT };
    let mut out: Option<Action> = None;
    egui::Panel::top("cc_ribbon").exact_size(h).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let r = ui.max_rect();
        // Tabs, then the contextual tab of the selection (Hatch Editor), which opens by itself.
        let mut x = r.left() + 8.0;
        let context = crate::ribbon_hatch::context(app, ui.ctx());
        let active = match context {
            Some(c) if app.ui.ribbon_tab == c => c,
            _ => TABS.iter().find(|tab| tab.id == app.ui.ribbon_tab).map_or("home", |tab| tab.id),
        };
        for tab in TABS {
            let label = tab_label(tab.title);
            let w = ui.painter().layout_no_wrap(label.clone(), crate::theme::body(), t.text).size().x + 18.0;
            let tr = Rect::from_min_size(pos2(x, r.top()), vec2(w, TAB_ROW));
            let resp = ui.interact(tr, ui.id().with(("rb_tab", tab.id)), Sense::click());
            let is_active = tab.id == active;
            if is_active && !app.ui.ribbon_minimized {
                ui.painter().rect_filled(tr, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, t.panel);
            } else if resp.hovered() {
                ui.painter().rect_filled(tr, 3.0, t.control_hover.gamma_multiply(0.5));
            }
            ui.painter().text(tr.center(), Align2::CENTER_CENTER, label, crate::theme::body(), if is_active { t.text } else { t.text_dim });
            icons::describe_control(ui, &resp, tab.title, Some(is_active), true);
            if resp.double_clicked() {
                out = Some(Action::ToggleMinimized);
            } else if resp.clicked() {
                out = Some(Action::Tab(tab.id));
            }
            x += w + 2.0;
        }
        if let Some(id) = context {
            let label = tab_label(crate::ribbon_hatch::title(id));
            let w = ui.painter().layout_no_wrap(label.clone(), crate::theme::body(), t.text).size().x + 18.0;
            let tr = Rect::from_min_size(pos2(x + 4.0, r.top() + 1.0), vec2(w, TAB_ROW - 1.0));
            let resp = ui.interact(tr, ui.id().with(("rb_tab", id)), Sense::click());
            let is_active = id == active;
            // A contextual tab is drawn in the accent colour, as Windows CAD ribbons do.
            let fill = if is_active { t.accent } else { t.accent.gamma_multiply(if resp.hovered() { 0.75 } else { 0.55 }) };
            ui.painter().rect_filled(tr, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, fill);
            ui.painter().text(tr.center(), Align2::CENTER_CENTER, label, crate::theme::body(), Color32::WHITE);
            icons::describe_control(ui, &resp, crate::ribbon_hatch::title(id), Some(is_active), true);
            if resp.double_clicked() {
                out = Some(Action::ToggleMinimized);
            } else if resp.clicked() {
                out = Some(Action::Tab(id));
            }
        }
        if app.ui.ribbon_minimized {
            return;
        }
        // Body.
        let body = Rect::from_min_max(pos2(r.left(), r.top() + TAB_ROW), r.max);
        ui.painter().rect_filled(body, 0.0, t.panel);
        ui.painter().hline(body.x_range(), body.bottom() - 0.5, Stroke::new(1.0, t.border));
        if context == Some(active) {
            crate::ribbon_hatch::body(app, ui, body, &mut out);
            return;
        }
        let Some(tab) = TABS.iter().find(|tab| tab.id == active) else { return };
        let full: Vec<f32> = tab.panels.iter().map(|p| panel_width(ui, p)).collect();
        let collapsed: Vec<f32> = tab.panels.iter().map(|p| collapsed_width(ui, p)).collect();
        let expanded = fit(&full, &collapsed, body.width() - 12.0);
        let mut x = body.left() + 6.0;
        for (i, panel) in tab.panels.iter().enumerate() {
            let open = expanded.get(i).copied().unwrap_or(true);
            let w = if open { full.get(i).copied().unwrap_or(0.0) } else { collapsed.get(i).copied().unwrap_or(0.0) };
            let pr = Rect::from_min_size(pos2(x, body.top() + 2.0), vec2(w, BODY + TITLE));
            if open {
                let mut cx = pr.left() + PAD;
                for cell in panel.cells {
                    let cw = cell_width(ui, cell);
                    draw_cell(ui, app, cell, Rect::from_min_size(pos2(cx, pr.top()), vec2(cw, BODY)), &mut out);
                    cx += cw;
                }
                // Title row (with the expander when the panel has more commands).
                let title_r = Rect::from_min_max(pos2(pr.left(), pr.bottom() - TITLE), pr.max);
                let label = crate::i18n::ribbon(panel.title);
                if panel.more.is_empty() {
                    ui.painter().text(title_r.center(), Align2::CENTER_CENTER, label, font(), t.text_dim);
                } else {
                    let resp = ui.interact(title_r, ui.id().with(("rb_more", tab.id, panel.title)), Sense::click());
                    hover_fill(ui, title_r, &resp);
                    let tw = text_w(ui, panel.title);
                    ui.painter().text(pos2(title_r.center().x - 6.0, title_r.center().y), Align2::CENTER_CENTER, label, font(), t.text_dim);
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(pos2(title_r.center().x + tw / 2.0 + 2.0, title_r.center().y), vec2(8.0, 8.0)),
                        Icon::ChevronDown,
                        false,
                    );
                    icons::describe_control(ui, &resp, panel.title, None, false);
                    egui::Popup::menu(&resp).show(|ui| variant_menu(ui, panel.more, &mut out));
                }
            } else {
                let resp = ui.interact(pr, ui.id().with(("rb_collapsed", tab.id, panel.title)), Sense::click());
                hover_fill(ui, pr, &resp);
                let first = panel_commands(panel).into_iter().next();
                if let Some(cmd) = &first {
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(pos2(pr.center().x, pr.top() + 6.0 + BIG_ICON / 2.0), vec2(BIG_ICON, BIG_ICON)),
                        cmd,
                        false,
                    );
                }
                ui.painter().text(
                    pos2(pr.center().x, pr.top() + BIG_ICON + 14.0),
                    Align2::CENTER_TOP,
                    crate::i18n::ribbon(panel.title),
                    font(),
                    t.text,
                );
                icons::paint(ui.painter(), Rect::from_center_size(pos2(pr.center().x, pr.bottom() - 10.0), vec2(9.0, 9.0)), Icon::ChevronDown, false);
                icons::describe_control(ui, &resp, panel.title, None, false);
                let cmds = panel_commands(panel);
                egui::Popup::menu(&resp).show(|ui| variant_menu(ui, &cmds, &mut out));
            }
            x += w;
            // Panel separator.
            ui.painter().vline(x, (pr.top() + 4.0)..=(pr.bottom() - 4.0), Stroke::new(1.0, t.border));
            x += 1.0;
        }
    });
    match out {
        Some(Action::Start(id)) => app.start(id),
        Some(Action::Do(f)) => f(app),
        Some(Action::Tab(id)) => {
            app.ui.ribbon_tab = id.to_string();
            app.ui.ribbon_minimized = false;
        }
        Some(Action::ToggleMinimized) => app.ui.ribbon_minimized = !app.ui.ribbon_minimized,
        Some(Action::Layer(name)) => {
            let _ = app.run("layer.current", json!({ "name": name }));
        }
        Some(Action::LayerToggle(name, key, value)) => {
            let _ = app.run("layer.set", json!({ "name": name, key: value }));
        }
        None => {}
    }
}

/// Switch to the Windows layout: Ribbon, quick access tool bar in the title row,
/// Properties docked left, no Tool Sets or Layers palette.
pub fn apply_pc_layout(app: &mut CadApp) {
    let workspace = crate::docking::Workspace::pc();
    crate::docking::publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    app.ui.show_ribbon = true;
    app.ui.show_toolbar = false;
    app.ui.pc_layout_applied = true;
}

/// Back to CADCraft's Mac layout: Tool Bar, Tool Sets, Layers and Properties
/// palettes on the right, no Ribbon.
pub fn apply_mac_layout(app: &mut CadApp) {
    let workspace = crate::docking::Workspace::default();
    crate::docking::publish_visibility(app, &workspace);
    app.ui.docking = workspace;
    app.ui.show_ribbon = false;
    app.ui.show_toolbar = true;
    app.ui.pc_layout_applied = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_collapse_from_the_right() {
        let full = [100.0, 100.0, 100.0];
        let collapsed = [40.0, 40.0, 40.0];
        assert_eq!(fit(&full, &collapsed, 400.0), vec![true, true, true]);
        assert_eq!(fit(&full, &collapsed, 250.0), vec![true, true, false]);
        assert_eq!(fit(&full, &collapsed, 190.0), vec![true, false, false]);
        assert_eq!(fit(&full, &collapsed, 10.0), vec![false, false, false]);
    }

    #[test]
    fn tabs_match_the_windows_layout_order() {
        let titles: Vec<&str> = TABS.iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Home", "Insert", "Annotate", "Parametric", "View", "Manage", "Output", "Collaborate"]);
        let home: Vec<&str> = TABS[0].panels.iter().map(|p| p.title).collect();
        assert_eq!(home, ["Draw", "Modify", "Annotation", "Layers", "Block", "Properties", "Groups", "Utilities", "Clipboard"]);
    }

    /// The everyday drafting commands of the Home tab are all real commands (not greyed out).
    #[test]
    fn home_tab_core_commands_exist() {
        for id in [
            "line",
            "pline",
            "circle",
            "arc",
            "rectang",
            "ellipse",
            "hatch",
            "move",
            "copy",
            "stretch",
            "rotate",
            "mirror",
            "scale",
            "trim",
            "fillet",
            "arrayrect",
            "erase",
            "explode",
            "offset",
            "mtext",
            "dim",
            "dimlinear",
            "mleader",
            "table",
            "insert",
            "block",
            "matchprop",
            "laymcur",
            "layoff",
            "layiso",
            "layfrz",
            "laylck",
        ] {
            assert!(known(id), "{id} is not a command");
        }
        for v in CIRCLES.iter().chain(ARCS).chain(ELLIPSES) {
            assert!(known(v.id), "{} is not a command", v.id);
        }
    }

    #[test]
    fn colour_and_lineweight_names() {
        assert_eq!(lineweight_name(30), "0.30 mm");
        assert_eq!(lineweight_name(-1), "ByLayer");
        assert_eq!(color_name(4), "Cyan");
        assert_eq!(color_name(42), "Color 42");
    }
}
