use super::{Deserialize, Serialize, Vec2, snap};

/// Session-level drafting settings. [`Session::drafting_settings`] supplies the active drawing's
/// snap/grid spacing in the corresponding fields for UI and JSON consumers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub osmode: u32,
    pub orthomode: bool,
    pub polarmode: bool,
    /// POLARANG (radians).
    pub polarang: f64,
    pub gridmode: bool,
    pub snapmode: bool,
    /// SNAPUNIT in a drafting settings snapshot; stored in the drawing header.
    pub snapunit: Vec2,
    /// OSNAPHATCH: object snaps find hatch objects (off by default, as in AutoCAD).
    pub osnaphatch: bool,
    /// GRIDUNIT in a drafting settings snapshot; zero components follow SNAPUNIT.
    pub gridunit: Vec2,
    /// Major grid line every N minor lines (GRIDMAJOR).
    pub gridmajor: u32,
    /// ZOOMFACTOR: how far one mouse wheel step zooms (3..=100).
    pub zoomfactor: u32,
    /// MEASUREINIT: the units of a new drawing made without a template (NEW, QNEW and the first
    /// drawing at startup): `false` (0, the default) imperial, `true` (1) metric.
    pub measureinit: bool,
    /// -VIEW sEttings > Layer snapshot: named views save the layer settings with them.
    pub view_layer_snapshot: bool,
    pub dynmode: bool,
    /// DYNDIGRIP: which dimensions grip dimensional input shows while a grip is stretched (bits:
    /// 1 resulting length, 2 length change, 4 absolute angle, 8 angle change, 16 radius); read
    /// when DYNDIVIS = 2.
    pub dyndigrip: u32,
    /// DYNDIVIS: how many of them: 0 the first, 1 the first two (default), 2 all DYNDIGRIP asks for.
    pub dyndivis: u8,
    /// DYNPIFORMAT = 1: Dynamic Input shows second and next points as Cartesian `x,y` instead of
    /// polar `distance<angle` (the default).
    pub dynpi_cartesian: bool,
    /// DYNPICOORDS = 1: second and next points typed into Dynamic Input are absolute instead of
    /// relative to the last point (the default).
    pub dynpi_absolute: bool,
    /// DYNPROMPT: Dynamic Input shows the prompt beside the cursor, and Down Arrow opens the
    /// prompt's options there (on by default).
    pub dynprompt: bool,
    pub lwdisplay: bool,
    /// TRANSPARENCYDISPLAY: draw objects with their transparency on screen (on by default).
    pub transparency_display: bool,
    /// PLOTTRANSPARENCYOVERRIDE: 0 never plots transparency, 1 (default) uses the page setup's or
    /// the Plot dialog's "Plot transparency", 2 always plots it.
    pub plot_transparency_override: u8,
    /// Selection cycling on (the status-bar toggle, the sign of SELECTIONCYCLING).
    pub selection_cycling: bool,
    /// What selection cycling shows over overlapping objects (SELECTIONCYCLING's magnitude): 0
    /// nothing, 1 a badge, 2 (default) the badge and a list to pick from on a click.
    pub selection_cycling_display: u8,
    /// SELECTSIMILARMODE: what SELECTSIMILAR matches besides the object type (bits: 1 colour,
    /// 2 layer, 4 linetype, 8 linetype scale, 16 lineweight, 32 plot style, 64 object style,
    /// 128 name).
    pub selectsimilarmode: u32,
    pub otrack: bool,
    /// PICKBOX in pixels.
    pub pickbox: f64,
    /// APERTURE in pixels.
    pub aperture: f64,
    pub pickfirst: bool,
    /// ACADLSPASDOC: acad.lsp loads into every drawing, not once per session.
    pub acadlspasdoc: bool,
    /// CMDECHO: kept for routines that switch command echo around their commands; no command reads it yet.
    pub cmdecho: bool,
    /// FILEDIA: kept for routines that switch file dialogs off around a script; no command reads it yet.
    pub filedia: bool,
    pub pickadd: bool,
    /// PICKDRAG: false = two-click windows (0), true = clicks or dragging (2).
    /// Drag-only mode (1) is not supported yet.
    pub pickdrag: bool,
    /// PICKSTYLE: bit 1 picks whole selectable groups ([`cmd::groups`]), bit 2 associative hatches
    /// with their boundaries (stored, not used yet).
    pub pickstyle: u8,
    pub gripsize: f64,
    pub cursorsize: f64,
    /// MAXARRAY: the most objects one array command creates (items × selected objects).
    pub maxarray: u64,
    /// SNAPSTYL 1: isometric snap and grid, ortho and crosshair along the isometric plane's axes.
    pub isodraft: bool,
    /// SNAPISOPAIR: the current isometric plane (ISOPLANE, F5).
    pub isoplane: snap::iso::IsoPlane,
    pub annoallvisible: bool,
    pub annoautoscale: bool,
    /// QPMODE: show the Quick Properties palette when objects are selected.
    pub qpmode: bool,
    /// HIGHLIGHT: selected objects are highlighted (their grips show either way). Not saved:
    /// every session starts with it on, as in AutoCAD, so it can't stay off unnoticed.
    pub highlight: bool,
    /// SELECTIONEFFECT: how selected objects are highlighted: 0 dashed, 1 redrawn in the
    /// selection colour.
    pub selectioneffect: u8,
    /// SELECTIONPREVIEW bits: the object under the cursor is highlighted (1) when no command is
    /// active, (2) when a command asks for objects.
    pub selectionpreview: u8,
    /// PREVIEWEFFECT: how that object is highlighted: 0 dashed, 1 thicker, 2 dashed and thicker.
    pub previeweffect: u8,
    /// PREVIEWFILTER bits: objects the cursor never highlights: 1 on locked layers, 2 xrefs,
    /// 4 tables, 8 multiline text, 16 hatches, 32 in groups.
    pub previewfilter: u8,
    /// GRIPS: selected objects show their grips (0 off; AutoCAD's 2, more grips on polylines,
    /// shows them as 1 does).
    pub grips: u8,
    /// GRIPOBJLIMIT: no grips while more objects than this are selected (0: no limit).
    pub gripobjlimit: u32,
    /// SELECTIONAREA: the selection window and crossing areas are filled.
    pub selectionarea: bool,
    /// WINDOWAREACOLOR, CROSSINGAREACOLOR: their colours (ACI 1-255).
    pub windowareacolor: u8,
    pub crossingareacolor: u8,
    /// SELECTIONAREAOPACITY: their opacity, 0-100.
    pub selectionareaopacity: u8,
    /// POLARMODE bits ([`snap::tracking::polarmode`]); polar tracking itself is on while
    /// `polarmode` is (AUTOSNAP bit 8, F10).
    pub polar_flags: u32,
    /// POLARADDANG: additional polar tracking angles (radians), used with POLARMODE bit 4.
    pub polaraddang: Vec<f64>,
    /// SNAPTYPE 1: snap mode (F9) snaps along tracking paths at POLARDIST steps (PolarSnap)
    /// instead of to the grid.
    pub polarsnap: bool,
    /// POLARDIST: the PolarSnap step; 0 uses the snap X spacing.
    pub polardist: f64,
    /// AUDITCTL: 1 creates an audit report (.adt) file when AUDIT is run (default: 0).
    pub auditctl: bool,
    /// ATTREQ: 0 inserts blocks with their attributes' default values instead of asking for them
    /// (default: 1).
    pub attreq: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            osmode: 4133,
            orthomode: false,
            polarmode: true,
            polarang: std::f64::consts::FRAC_PI_2,
            gridmode: true,
            snapmode: false,
            snapunit: Vec2::new(0.5, 0.5),
            osnaphatch: false,
            gridunit: Vec2::new(0.5, 0.5),
            gridmajor: 5,
            zoomfactor: 60,
            measureinit: false,
            view_layer_snapshot: true,
            dynmode: true,
            dyndigrip: 31,
            dyndivis: 1,
            dynpi_cartesian: false,
            dynpi_absolute: false,
            dynprompt: true,
            lwdisplay: false,
            transparency_display: true,
            plot_transparency_override: 1,
            selection_cycling: false,
            selection_cycling_display: 2,
            selectsimilarmode: 130,
            otrack: true,
            pickbox: 3.0,
            aperture: 10.0,
            pickfirst: true,
            acadlspasdoc: false,
            cmdecho: true,
            filedia: true,
            pickadd: true,
            pickdrag: true,
            pickstyle: 1,
            gripsize: 5.0,
            cursorsize: 5.0,
            maxarray: 100_000,
            isodraft: false,
            isoplane: snap::iso::IsoPlane::Left,
            annoallvisible: true,
            annoautoscale: false,
            qpmode: false,
            highlight: true,
            selectioneffect: 1,
            selectionpreview: 3,
            previeweffect: 2,
            previewfilter: 3,
            grips: 1,
            gripobjlimit: 100,
            selectionarea: true,
            windowareacolor: 150,
            crossingareacolor: 100,
            selectionareaopacity: 25,
            polar_flags: 0,
            polaraddang: Vec::new(),
            polarsnap: false,
            polardist: 0.0,
            auditctl: false,
            attreq: true,
        }
    }
}
