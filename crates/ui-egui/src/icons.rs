//! CADCraft's icon set, drawn in code (original work, MIT OR Apache-2.0 with the codebase).
//!
//! Icons are line drawings on a 24×24 design grid: light strokes for existing geometry, blue for
//! the geometry a command creates and orange dots for picked points.

use craft_ui::buttons::{ButtonVisuals, IconButton, IconButtonStyle};
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2};

use crate::theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Plot,
    Publish,
    PageSetup,
    Preview,
    Import,
    Export,
    Attach,
    Share,
    ZoomWindow,
    Pan,
    Orbit,
    ZoomExtents,
    Properties,
    Blocks,
    Layers,
    Help,
    Line,
    Polyline,
    Circle,
    Arc,
    Rectangle,
    Polygon,
    Ellipse,
    Spline,
    Point,
    XLine,
    Ray,
    Donut,
    RevCloud,
    Hatch,
    Gradient,
    Boundary,
    Region,
    Wipeout,
    Table,
    Text,
    MText,
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Offset,
    Trim,
    Extend,
    Fillet,
    Chamfer,
    Explode,
    Erase,
    Stretch,
    ArrayRect,
    ArrayPolar,
    Break,
    Join,
    Lengthen,
    MatchProp,
    DimLinear,
    DimAligned,
    DimRadius,
    DimDiameter,
    DimAngular,
    DimArc,
    DimOrdinate,
    DimBaseline,
    DimContinue,
    QDim,
    MLeader,
    AddLeader,
    RemoveLeader,
    AlignLeaders,
    BlockCreate,
    Insert,
    BlockEdit,
    AttDef,
    LayerProps,
    LayerOff,
    LayerIso,
    LayerFreeze,
    LayerLock,
    LayerMatch,
    MakeCurrent,
    LayerPrev,
    Bulb,
    BulbOff,
    Lock,
    Unlock,
    Sun,
    Snowflake,
    Grid,
    Snap,
    Ortho,
    Polar,
    Osnap,
    OTrack,
    Lineweight,
    Transparency,
    Isodraft,
    DynInput,
    QuickProps,
    Gear,
    Annotation,
    Workspace,
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    ChevronRight,
    Plus,
    Switcher,
    Close,
    Search,
    Menu,
    Constraint,
    Coincident,
    Parallel,
    Perpendicular,
    Horizontal,
    Vertical,
    Tangent,
    Concentric,
    Equal,
    Fix,
    Collinear,
    Smooth,
    Symmetric,
    DelConstraint,
    Parameters,
    Measure,
    Distance,
    Area,
    List,
    PickAdd,
    SelectObjects,
    QuickSelect,
    Template,
    Pdf,
    Wrench,
    Quit,
    Cut,
    ClipCopy,
    Paste,
    SelectAll,
    Globe,
    Regen,
    NamedViews,
    Viewports,
    Ucs,
    Theme,
    Accessibility,
    Xref,
    DataLink,
    Layout,
    LayerState,
    ColorWheel,
    Linetype,
    TextStyle,
    DimStyle,
    Units,
    Rename,
    Script,
    Compare,
    Group,
    DrawOrder,
    Mouse3d,
    Polyline3d,
    Multiline,
    Helix,
    CenterMark,
    CenterLine,
    Dimension,
    DimJogged,
    DimSpace,
    DimBreak,
    Tolerance,
    Inspection,
    Oblique,
    AlignText,
    DimOverride,
    Object,
    Clip,
    Solid,
    Flatten,
    Overkill,
    Sample,
    Chat,
    Info,
}

#[derive(Clone, Copy)]
struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    /// The side of the grid the coordinates are on: 24 for the tool icons, 20 for the menu icons.
    g: f32,
    w: f32,
    base: Color32,
    acc: Color32,
    pt: Color32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        let s = self.sc();
        pos2(self.r.min.x + x * s, self.r.min.y + y * s)
    }
    fn sc(&self) -> f32 {
        self.r.width() / self.g
    }
    fn st(&self, c: Color32) -> Stroke {
        Stroke::new(self.w, c)
    }
    fn l(&self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.p.line_segment([self.at(x1, y1), self.at(x2, y2)], self.st(self.base));
    }
    fn la(&self, x1: f32, y1: f32, x2: f32, y2: f32) {
        self.p.line_segment([self.at(x1, y1), self.at(x2, y2)], self.st(self.acc));
    }
    fn poly(&self, pts: &[(f32, f32)], closed: bool, c: Color32) {
        let mut v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        if closed && let Some(f) = v.first().copied() {
            v.push(f);
        }
        self.p.line(v, self.st(c));
    }
    fn c(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_stroke(self.at(x, y), r * self.sc(), self.st(c));
    }
    fn fill_c(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.at(x, y), r * self.sc(), c);
    }
    /// Arc in degrees, counter-clockwise on screen (y up visually).
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, c: Color32) {
        let n = (((a1 - a0).abs() / 10.0).ceil() as usize).max(2);
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                self.at(x + r * a.cos(), y - r * a.sin())
            })
            .collect();
        self.p.line(pts, self.st(c));
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.poly(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true, c);
    }
    fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color32) {
        self.p.rect_filled(Rect::from_min_max(self.at(x, y), self.at(x + w, y + h)), 0.0, c);
    }
    fn dot(&self, x: f32, y: f32) {
        self.fill_c(x, y, 1.6, self.pt);
    }
    fn tri(&self, pts: [(f32, f32); 3], c: Color32) {
        self.p.add(egui::Shape::convex_polygon(pts.iter().map(|(x, y)| self.at(*x, *y)).collect(), c, Stroke::NONE));
    }
    fn arrow_tip(&self, x: f32, y: f32, dx: f32, dy: f32, c: Color32) {
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ux, uy) = (dx / l, dy / l);
        let (nx, ny) = (-uy, ux);
        self.tri([(x, y), (x - ux * 3.0 + nx * 1.4, y - uy * 3.0 + ny * 1.4), (x - ux * 3.0 - nx * 1.4, y - uy * 3.0 - ny * 1.4)], c);
    }
    fn text(&self, x: f32, y: f32, s: &str, size: f32, c: Color32) {
        self.p.text(self.at(x, y), egui::Align2::CENTER_CENTER, s, egui::FontId::proportional(size * self.sc()), c);
    }
}

/// Paint `icon` into `rect` (square), with `dim` for disabled.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, dim: bool) {
    paint_tinted(p, rect, icon, dim, None);
}

/// [`paint`] in the one colour `tint` (an icon on a highlighted row), or in its own colours.
pub fn paint_tinted(p: &Painter, rect: Rect, icon: Icon, dim: bool, tint: Option<Color32>) {
    let t = Tokens::get();
    let alpha = |c: Color32| {
        let c = tint.unwrap_or(c);
        if dim { c.gamma_multiply(0.4) } else { c }
    };
    let pen = Pen {
        p,
        r: rect,
        g: 24.0,
        w: (rect.width() / 16.0).clamp(1.0, 1.6),
        base: alpha(t.icon),
        acc: alpha(t.icon_accent),
        pt: alpha(t.icon_point),
    };
    let (b, a) = (pen.base, pen.acc);
    use Icon::*;
    match icon {
        New => {
            pen.poly(&[(6.0, 3.0), (15.0, 3.0), (19.0, 7.0), (19.0, 21.0), (6.0, 21.0)], true, b);
            pen.poly(&[(15.0, 3.0), (15.0, 7.0), (19.0, 7.0)], false, b);
        }
        Open => {
            pen.poly(&[(3.0, 6.0), (9.0, 6.0), (11.0, 8.0), (20.0, 8.0), (20.0, 19.0), (3.0, 19.0)], true, b);
            pen.poly(&[(3.0, 19.0), (6.0, 11.0), (22.0, 11.0), (20.0, 19.0)], false, b);
        }
        Save | SaveAs => {
            pen.poly(&[(4.0, 4.0), (17.0, 4.0), (20.0, 7.0), (20.0, 20.0), (4.0, 20.0)], true, b);
            pen.rect(8.0, 4.0, 8.0, 5.0, b);
            pen.rect(7.0, 13.0, 10.0, 7.0, b);
            if icon == SaveAs {
                pen.la(15.0, 22.0, 22.0, 15.0);
                pen.fill_c(15.0, 22.0, 1.2, a);
            }
        }
        Undo => {
            pen.arc(12.0, 13.0, 7.0, 150.0, -40.0, b);
            pen.arrow_tip(5.0, 9.0, -1.0, 2.5, b);
        }
        Redo => {
            pen.arc(12.0, 13.0, 7.0, 30.0, 220.0, b);
            pen.arrow_tip(19.0, 9.0, 1.0, 2.5, b);
        }
        Plot | Preview => {
            pen.rect(4.0, 9.0, 16.0, 8.0, b);
            pen.rect(7.0, 4.0, 10.0, 5.0, b);
            pen.rect(7.0, 14.0, 10.0, 7.0, b);
            if icon == Preview {
                pen.c(17.0, 17.0, 3.0, a);
                pen.la(19.0, 19.0, 22.0, 22.0);
            }
        }
        Publish => {
            pen.rect(3.0, 10.0, 13.0, 7.0, b);
            pen.rect(6.0, 6.0, 7.0, 4.0, b);
            pen.rect(9.0, 3.0, 12.0, 9.0, a);
        }
        PageSetup => {
            pen.rect(5.0, 3.0, 12.0, 17.0, b);
            pen.c(17.0, 17.0, 3.0, a);
            pen.fill_c(17.0, 17.0, 1.0, a);
        }
        Import | Export => {
            pen.poly(&[(5.0, 3.0), (14.0, 3.0), (18.0, 7.0), (18.0, 21.0), (5.0, 21.0)], true, b);
            if icon == Import {
                pen.la(1.0, 13.0, 11.0, 13.0);
                pen.arrow_tip(12.0, 13.0, 1.0, 0.0, a);
            } else {
                pen.la(11.0, 13.0, 21.0, 13.0);
                pen.arrow_tip(23.0, 13.0, 1.0, 0.0, a);
            }
        }
        Attach => {
            pen.rect(4.0, 5.0, 16.0, 14.0, b);
            pen.poly(&[(6.0, 17.0), (10.0, 11.0), (13.0, 15.0), (15.0, 13.0), (18.0, 17.0)], false, a);
            pen.c(15.0, 9.0, 1.5, a);
        }
        Share => {
            pen.poly(&[(4.0, 12.0), (20.0, 4.0), (14.0, 20.0), (11.0, 13.0)], true, a);
            pen.l(11.0, 13.0, 20.0, 4.0);
        }
        ZoomWindow => {
            pen.rect(3.0, 3.0, 12.0, 12.0, b);
            pen.c(14.0, 14.0, 4.5, a);
            pen.la(17.5, 17.5, 21.0, 21.0);
        }
        ZoomExtents => {
            pen.c(10.0, 10.0, 6.0, b);
            pen.l(14.5, 14.5, 20.0, 20.0);
            pen.la(7.0, 10.0, 13.0, 10.0);
            pen.la(10.0, 7.0, 10.0, 13.0);
        }
        Pan => {
            pen.poly(
                &[
                    (8.0, 20.0),
                    (5.0, 13.0),
                    (6.0, 11.0),
                    (8.0, 13.0),
                    (8.0, 5.0),
                    (10.0, 4.0),
                    (11.0, 11.0),
                    (11.0, 3.0),
                    (13.0, 3.0),
                    (14.0, 11.0),
                    (14.0, 4.0),
                    (16.0, 4.0),
                    (17.0, 12.0),
                    (17.0, 7.0),
                    (19.0, 7.0),
                    (19.0, 15.0),
                    (16.0, 20.0),
                ],
                true,
                b,
            );
        }
        Orbit => {
            pen.c(12.0, 12.0, 4.0, b);
            pen.arc(12.0, 12.0, 9.0, 20.0, 160.0, a);
            pen.arc(12.0, 12.0, 9.0, 200.0, 340.0, a);
            pen.arrow_tip(3.5, 9.5, -0.4, 1.0, a);
        }
        Properties => {
            pen.rect(4.0, 3.0, 16.0, 18.0, b);
            for y in [8.0, 12.0, 16.0] {
                pen.l(7.0, y, 10.0, y);
                pen.la(12.0, y, 17.0, y);
            }
        }
        Blocks => {
            pen.poly(&[(12.0, 3.0), (20.0, 7.0), (12.0, 11.0), (4.0, 7.0)], true, b);
            pen.poly(&[(4.0, 7.0), (4.0, 16.0), (12.0, 20.0), (20.0, 16.0), (20.0, 7.0)], false, b);
            pen.l(12.0, 11.0, 12.0, 20.0);
        }
        Layers | LayerProps => {
            for (k, y) in [5.0f32, 10.0, 15.0].iter().enumerate() {
                let c = if k == 0 { a } else { b };
                pen.poly(&[(3.0, y + 2.0), (12.0, *y - 2.0), (21.0, y + 2.0), (12.0, y + 6.0)], true, c);
            }
        }
        Help => {
            pen.c(12.0, 12.0, 9.0, b);
            pen.text(12.0, 12.5, "?", 12.0, b);
        }
        Line => {
            pen.la(4.0, 20.0, 20.0, 4.0);
            pen.dot(4.0, 20.0);
            pen.dot(20.0, 4.0);
        }
        Polyline => {
            pen.poly(&[(3.0, 19.0), (8.0, 8.0), (14.0, 15.0)], false, a);
            pen.arc(17.0, 11.0, 5.0, 220.0, 360.0 + 20.0, a);
            pen.dot(3.0, 19.0);
            pen.dot(8.0, 8.0);
            pen.dot(14.0, 15.0);
        }
        Circle => {
            pen.c(12.0, 12.0, 8.0, a);
            pen.l(12.0, 12.0, 18.0, 6.5);
            pen.dot(12.0, 12.0);
        }
        Arc => {
            pen.arc(12.0, 16.0, 9.0, 10.0, 170.0, a);
            pen.dot(3.2, 14.5);
            pen.dot(12.0, 7.0);
            pen.dot(20.8, 14.5);
        }
        Rectangle => {
            pen.rect(3.0, 6.0, 18.0, 12.0, a);
            pen.dot(3.0, 18.0);
            pen.dot(21.0, 6.0);
        }
        Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|i| ((12.0 + 8.5 * (i as f32 * 60.0f32).to_radians().cos()), 12.0 + 8.5 * (i as f32 * 60.0f32).to_radians().sin()))
                .collect();
            pen.poly(&pts, true, a);
            pen.dot(12.0, 12.0);
        }
        Ellipse => {
            let pts: Vec<(f32, f32)> = (0..36)
                .map(|i| (12.0 + 9.0 * (i as f32 * 10.0f32).to_radians().cos(), 12.0 + 5.0 * (i as f32 * 10.0f32).to_radians().sin()))
                .collect();
            pen.poly(&pts, true, a);
            pen.dot(3.0, 12.0);
            pen.dot(21.0, 12.0);
        }
        Spline => {
            let pts: Vec<(f32, f32)> =
                (0..=20).map(|i| (3.0 + 18.0 * i as f32 / 20.0, 12.0 - 6.0 * (i as f32 / 20.0 * std::f32::consts::TAU).sin())).collect();
            pen.poly(&pts, false, a);
            pen.dot(3.0, 12.0);
            pen.dot(7.5, 6.0);
            pen.dot(16.5, 18.0);
            pen.dot(21.0, 12.0);
        }
        Point => {
            pen.dot(12.0, 12.0);
            pen.l(12.0, 5.0, 12.0, 9.0);
            pen.l(12.0, 15.0, 12.0, 19.0);
            pen.l(5.0, 12.0, 9.0, 12.0);
            pen.l(15.0, 12.0, 19.0, 12.0);
        }
        XLine => {
            pen.la(1.0, 22.0, 23.0, 2.0);
            pen.dot(12.0, 12.0);
        }
        Ray => {
            pen.la(5.0, 19.0, 23.0, 3.0);
            pen.dot(5.0, 19.0);
        }
        Donut => {
            pen.c(12.0, 12.0, 8.0, a);
            pen.c(12.0, 12.0, 4.5, a);
            pen.fill_c(12.0, 12.0, 1.0, b);
        }
        RevCloud => {
            for i in 0..6 {
                let ang = i as f32 * 60.0;
                let x = 12.0 + 6.0 * ang.to_radians().cos();
                let y = 12.0 + 6.0 * ang.to_radians().sin();
                pen.arc(x, y, 3.2, -ang - 70.0, -ang + 110.0, a);
            }
        }
        Hatch => {
            pen.rect(3.0, 3.0, 18.0, 18.0, b);
            for k in 0..5 {
                let o = k as f32 * 4.5;
                pen.la((3.0 + o).min(21.0), 21.0, 21.0, (21.0 - (18.0 - o)).max(3.0));
                pen.la(3.0, 21.0 - o, (3.0 + o).min(21.0), 21.0 - o - o.min(18.0) + o);
            }
        }
        Gradient => {
            for i in 0..9 {
                let c = Color32::from_rgb(40 + i * 22, 80 + i * 16, 160 + i * 9);
                pen.fill_rect(3.0 + i as f32 * 2.0, 3.0, 2.0, 18.0, if dim { c.gamma_multiply(0.4) } else { c });
            }
            pen.rect(3.0, 3.0, 18.0, 18.0, b);
        }
        Boundary => {
            pen.rect(4.0, 4.0, 16.0, 16.0, b);
            pen.c(12.0, 12.0, 6.0, b);
            pen.poly(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)], true, a);
            pen.dot(6.0, 6.0);
        }
        Region => {
            pen.poly(&[(4.0, 18.0), (6.0, 6.0), (16.0, 4.0), (20.0, 14.0), (12.0, 20.0)], true, a);
            pen.fill_c(12.0, 12.0, 3.0, a.gamma_multiply(0.5));
        }
        Wipeout => {
            pen.rect(3.0, 3.0, 13.0, 13.0, b);
            pen.fill_rect(8.0, 8.0, 13.0, 13.0, Tokens::get().chrome);
            pen.rect(8.0, 8.0, 13.0, 13.0, a);
        }
        Table => {
            pen.rect(3.0, 4.0, 18.0, 16.0, b);
            pen.fill_rect(3.0, 4.0, 18.0, 4.0, a.gamma_multiply(0.6));
            for y in [8.0, 12.0, 16.0] {
                pen.l(3.0, y, 21.0, y);
            }
            pen.l(9.0, 8.0, 9.0, 20.0);
            pen.l(15.0, 8.0, 15.0, 20.0);
        }
        Text => pen.text(12.0, 12.5, "A", 19.0, b),
        MText => {
            pen.text(9.0, 11.0, "A", 15.0, b);
            pen.la(14.0, 15.0, 21.0, 15.0);
            pen.la(14.0, 18.0, 21.0, 18.0);
            pen.la(14.0, 21.0, 19.0, 21.0);
        }
        Move => {
            pen.la(12.0, 3.0, 12.0, 21.0);
            pen.la(3.0, 12.0, 21.0, 12.0);
            pen.arrow_tip(12.0, 2.0, 0.0, -1.0, a);
            pen.arrow_tip(12.0, 22.0, 0.0, 1.0, a);
            pen.arrow_tip(2.0, 12.0, -1.0, 0.0, a);
            pen.arrow_tip(22.0, 12.0, 1.0, 0.0, a);
        }
        Copy => {
            pen.rect(3.0, 3.0, 11.0, 13.0, b);
            pen.rect(10.0, 8.0, 11.0, 13.0, a);
        }
        Rotate => {
            pen.arc(12.0, 12.0, 8.0, 100.0, 380.0, a);
            pen.arrow_tip(10.6, 4.1, -1.0, 0.1, a);
            pen.dot(12.0, 12.0);
        }
        Scale => {
            pen.rect(3.0, 11.0, 10.0, 10.0, b);
            pen.rect(3.0, 3.0, 18.0, 18.0, a);
            pen.la(9.0, 15.0, 18.0, 6.0);
            pen.arrow_tip(19.0, 5.0, 1.0, -1.0, a);
        }
        Mirror => {
            pen.poly(&[(3.0, 18.0), (9.0, 6.0), (9.0, 18.0)], true, b);
            pen.poly(&[(21.0, 18.0), (15.0, 6.0), (15.0, 18.0)], true, a);
            for y in [3.0, 7.0, 11.0, 15.0, 19.0] {
                pen.l(12.0, y, 12.0, y + 2.0);
            }
        }
        Offset => {
            pen.arc(14.0, 18.0, 6.0, 90.0, 180.0, b);
            pen.l(14.0, 12.0, 21.0, 12.0);
            pen.l(8.0, 18.0, 8.0, 22.0);
            pen.arc(14.0, 18.0, 10.0, 90.0, 180.0, a);
            pen.la(14.0, 8.0, 21.0, 8.0);
            pen.la(4.0, 18.0, 4.0, 22.0);
        }
        Trim => {
            pen.l(8.0, 2.0, 8.0, 22.0);
            pen.l(16.0, 2.0, 16.0, 22.0);
            pen.l(2.0, 12.0, 8.0, 12.0);
            pen.l(16.0, 12.0, 22.0, 12.0);
            for x in [9.5, 12.0, 14.5] {
                pen.la(x, 12.0, x + 1.0, 12.0);
            }
            pen.poly(&[(10.0, 17.0), (12.0, 15.0), (14.0, 17.0)], false, a);
        }
        Extend => {
            pen.l(20.0, 2.0, 20.0, 22.0);
            pen.l(3.0, 12.0, 11.0, 12.0);
            pen.la(11.0, 12.0, 19.0, 12.0);
            pen.arrow_tip(19.5, 12.0, 1.0, 0.0, a);
        }
        Fillet => {
            pen.l(4.0, 20.0, 4.0, 12.0);
            pen.l(12.0, 4.0, 20.0, 4.0);
            pen.arc(12.0, 12.0, 8.0, 90.0, 180.0, a);
        }
        Chamfer => {
            pen.l(4.0, 20.0, 4.0, 10.0);
            pen.l(10.0, 4.0, 20.0, 4.0);
            pen.la(4.0, 10.0, 10.0, 4.0);
        }
        Explode => {
            pen.poly(&[(4.0, 9.0), (9.0, 4.0)], false, a);
            pen.poly(&[(15.0, 4.0), (20.0, 9.0)], false, a);
            pen.poly(&[(20.0, 15.0), (15.0, 20.0)], false, a);
            pen.poly(&[(9.0, 20.0), (4.0, 15.0)], false, a);
            pen.fill_c(12.0, 12.0, 2.0, pen.pt);
        }
        Erase => {
            pen.poly(&[(4.0, 16.0), (13.0, 7.0), (19.0, 13.0), (12.0, 20.0), (8.0, 20.0)], true, b);
            pen.la(9.0, 11.0, 15.0, 17.0);
            pen.l(8.0, 20.0, 20.0, 20.0);
        }
        Stretch => {
            pen.poly(&[(3.0, 18.0), (3.0, 8.0), (10.0, 8.0)], false, b);
            pen.poly(&[(10.0, 8.0), (20.0, 8.0), (20.0, 18.0), (3.0, 18.0)], false, a);
            for x in [12.0, 15.0, 18.0, 21.0] {
                pen.l(x, 5.0, x + 1.5, 5.0);
            }
        }
        ArrayRect => {
            for i in 0..3 {
                for j in 0..3 {
                    pen.rect(3.0 + i as f32 * 7.0, 3.0 + j as f32 * 7.0, 4.0, 4.0, if i + j == 0 { b } else { a });
                }
            }
        }
        ArrayPolar => {
            pen.dot(12.0, 12.0);
            for i in 0..6 {
                let ang = (i as f32 * 60.0).to_radians();
                pen.c(12.0 + 7.5 * ang.cos(), 12.0 + 7.5 * ang.sin(), 2.0, if i == 0 { b } else { a });
            }
        }
        Break => {
            pen.l(3.0, 12.0, 9.0, 12.0);
            pen.l(15.0, 12.0, 21.0, 12.0);
            pen.dot(9.0, 12.0);
            pen.dot(15.0, 12.0);
        }
        Join => {
            pen.l(3.0, 16.0, 11.0, 8.0);
            pen.la(11.0, 8.0, 21.0, 16.0);
            pen.dot(11.0, 8.0);
        }
        Lengthen => {
            pen.l(3.0, 12.0, 13.0, 12.0);
            pen.la(13.0, 12.0, 20.0, 12.0);
            pen.arrow_tip(21.0, 12.0, 1.0, 0.0, a);
        }
        MatchProp => {
            pen.rect(4.0, 3.0, 14.0, 6.0, b);
            pen.l(18.0, 6.0, 20.0, 6.0);
            pen.poly(&[(20.0, 6.0), (20.0, 11.0), (11.0, 11.0), (11.0, 14.0)], false, b);
            pen.fill_rect(9.5, 14.0, 3.0, 8.0, a);
        }
        DimLinear | DimAligned | DimBaseline | DimContinue | QDim => {
            let (y0, y1) = (8.0, 18.0);
            pen.l(4.0, y1, 4.0, 5.0);
            pen.l(20.0, y1, 20.0, 5.0);
            pen.la(4.0, y0, 20.0, y0);
            pen.arrow_tip(4.0, y0, -1.0, 0.0, a);
            pen.arrow_tip(20.0, y0, 1.0, 0.0, a);
            match icon {
                DimBaseline => pen.la(4.0, 4.0, 13.0, 4.0),
                DimContinue => {
                    pen.l(12.0, y1, 12.0, 5.0);
                }
                QDim => pen.text(12.0, 15.0, "Q", 7.0, b),
                DimAligned => {}
                _ => pen.text(12.0, 14.5, "1.5", 6.0, b),
            }
        }
        DimRadius => {
            pen.arc(12.0, 14.0, 8.0, 0.0, 180.0, b);
            pen.la(12.0, 14.0, 17.6, 8.4);
            pen.arrow_tip(17.6, 8.4, 1.0, -1.0, a);
            pen.dot(12.0, 14.0);
        }
        DimDiameter => {
            pen.c(12.0, 12.0, 8.0, b);
            pen.la(6.4, 17.6, 17.6, 6.4);
            pen.arrow_tip(6.4, 17.6, -1.0, 1.0, a);
            pen.arrow_tip(17.6, 6.4, 1.0, -1.0, a);
        }
        DimAngular => {
            pen.l(4.0, 20.0, 20.0, 20.0);
            pen.l(4.0, 20.0, 16.0, 6.0);
            pen.arc(4.0, 20.0, 11.0, 0.0, 49.0, a);
        }
        DimArc => {
            pen.arc(12.0, 18.0, 8.0, 20.0, 160.0, b);
            pen.arc(12.0, 18.0, 12.0, 25.0, 155.0, a);
        }
        DimOrdinate => {
            pen.l(4.0, 20.0, 4.0, 4.0);
            pen.l(4.0, 20.0, 20.0, 20.0);
            pen.la(10.0, 20.0, 10.0, 8.0);
            pen.la(16.0, 20.0, 16.0, 11.0);
        }
        MLeader | AddLeader | RemoveLeader | AlignLeaders => {
            pen.la(3.0, 20.0, 10.0, 9.0);
            pen.la(10.0, 9.0, 14.0, 9.0);
            pen.arrow_tip(3.0, 20.0, -0.55, 1.0, a);
            pen.l(15.0, 6.0, 22.0, 6.0);
            pen.l(15.0, 10.0, 21.0, 10.0);
            match icon {
                AddLeader => pen.text(19.0, 18.0, "+", 9.0, pen.pt),
                RemoveLeader => pen.text(19.0, 18.0, "−", 9.0, pen.pt),
                _ => {}
            }
        }
        BlockCreate | Insert | BlockEdit | AttDef => {
            pen.poly(&[(9.0, 4.0), (17.0, 8.0), (9.0, 12.0), (1.0, 8.0)], true, b);
            pen.poly(&[(1.0, 8.0), (1.0, 15.0), (9.0, 19.0), (17.0, 15.0), (17.0, 8.0)], false, b);
            match icon {
                BlockCreate => pen.text(19.0, 5.0, "✳", 9.0, pen.pt),
                Insert => {
                    pen.la(14.0, 20.0, 22.0, 20.0);
                    pen.arrow_tip(13.0, 20.0, -1.0, 0.0, a);
                }
                BlockEdit => pen.la(14.0, 22.0, 22.0, 14.0),
                _ => pen.text(19.0, 19.0, "A", 9.0, a),
            }
        }
        LayerOff | LayerIso | LayerFreeze | LayerLock | LayerMatch | MakeCurrent | LayerPrev => {
            pen.poly(&[(2.0, 9.0), (10.0, 5.0), (18.0, 9.0), (10.0, 13.0)], true, b);
            pen.poly(&[(2.0, 13.0), (10.0, 17.0), (18.0, 13.0)], false, b);
            let (x, y) = (18.0, 17.0);
            match icon {
                LayerOff => pen.fill_c(x, y, 3.5, Color32::from_rgb(0x8a, 0x8a, 0x5a)),
                LayerIso => pen.fill_c(x, y, 3.5, a),
                LayerFreeze => pen.text(x, y, "❄", 9.0, Color32::from_rgb(0x9f, 0xd4, 0xff)),
                LayerLock => pen.rect(x - 3.0, y - 1.0, 6.0, 5.0, Color32::from_rgb(0xe6, 0xb4, 0x3c)),
                LayerMatch => pen.la(x - 4.0, y, x + 4.0, y),
                MakeCurrent => pen.text(x, y, "✓", 9.0, Color32::from_rgb(0x6f, 0xe0, 0x6f)),
                _ => {
                    pen.arc(x, y, 3.5, 90.0, 360.0, a);
                }
            }
        }
        Bulb | BulbOff => {
            let c = if icon == Bulb { Color32::from_rgb(0xf5, 0xd0, 0x3b) } else { Color32::from_rgb(0x70, 0x78, 0x86) };
            pen.fill_c(12.0, 10.0, 6.0, alpha(c));
            pen.fill_rect(9.5, 15.0, 5.0, 4.0, alpha(Color32::from_rgb(0xb0, 0xb4, 0xba)));
        }
        Lock | Unlock => {
            let c = alpha(Color32::from_rgb(0xe6, 0xb4, 0x3c));
            pen.fill_rect(6.0, 11.0, 12.0, 9.0, c);
            if icon == Lock {
                pen.arc(12.0, 11.0, 4.0, 0.0, 180.0, c);
            } else {
                pen.arc(15.0, 9.0, 4.0, 0.0, 180.0, c);
            }
        }
        Sun => {
            let c = alpha(Color32::from_rgb(0xf5, 0xd0, 0x3b));
            pen.fill_c(12.0, 12.0, 4.0, c);
            for i in 0..8 {
                let ang = (i as f32 * 45.0).to_radians();
                pen.poly(&[(12.0 + 6.0 * ang.cos(), 12.0 + 6.0 * ang.sin()), (12.0 + 9.0 * ang.cos(), 12.0 + 9.0 * ang.sin())], false, c);
            }
        }
        Snowflake => {
            let c = alpha(Color32::from_rgb(0x9f, 0xd4, 0xff));
            for i in 0..3 {
                let ang = (i as f32 * 60.0).to_radians();
                pen.poly(&[(12.0 - 8.0 * ang.cos(), 12.0 - 8.0 * ang.sin()), (12.0 + 8.0 * ang.cos(), 12.0 + 8.0 * ang.sin())], false, c);
            }
        }
        Grid => {
            for k in 0..4 {
                let v = 4.0 + k as f32 * 5.33;
                pen.l(v, 3.0, v, 21.0);
                pen.l(3.0, v, 21.0, v);
            }
        }
        Snap => {
            for i in 0..4 {
                for j in 0..4 {
                    pen.fill_c(4.0 + i as f32 * 5.33, 4.0 + j as f32 * 5.33, 1.0, b);
                }
            }
        }
        Ortho => {
            pen.poly(&[(4.0, 3.0), (4.0, 20.0), (21.0, 20.0)], false, b);
        }
        Polar => {
            pen.l(4.0, 20.0, 21.0, 20.0);
            pen.l(4.0, 20.0, 18.0, 6.0);
            pen.arc(4.0, 20.0, 9.0, 0.0, 45.0, b);
            pen.c(4.0, 20.0, 1.2, b);
        }
        Osnap => {
            pen.rect(7.0, 7.0, 10.0, 10.0, b);
            pen.l(12.0, 2.0, 12.0, 7.0);
            pen.l(12.0, 17.0, 12.0, 22.0);
            pen.l(2.0, 12.0, 7.0, 12.0);
            pen.l(17.0, 12.0, 22.0, 12.0);
        }
        OTrack => {
            pen.rect(9.0, 9.0, 6.0, 6.0, b);
            for x in [2.0, 5.0, 17.0, 20.0] {
                pen.l(x, 12.0, x + 1.5, 12.0);
            }
            for y in [2.0, 5.0, 17.0, 20.0] {
                pen.l(12.0, y, 12.0, y + 1.5);
            }
        }
        Lineweight => {
            for (k, w) in [1.0f32, 2.0, 3.5].iter().enumerate() {
                let y = 6.0 + k as f32 * 6.0;
                p.line_segment([pen.at(3.0, y), pen.at(21.0, y)], Stroke::new(w * pen.sc(), b));
            }
        }
        Transparency => {
            pen.fill_rect(3.0, 3.0, 12.0, 12.0, b.gamma_multiply(0.7));
            pen.fill_rect(9.0, 9.0, 12.0, 12.0, b.gamma_multiply(0.35));
        }
        Isodraft => {
            pen.poly(&[(12.0, 3.0), (20.0, 8.0), (20.0, 17.0), (12.0, 21.0), (4.0, 17.0), (4.0, 8.0)], true, b);
            pen.poly(&[(4.0, 8.0), (12.0, 12.0), (20.0, 8.0)], false, b);
            pen.l(12.0, 12.0, 12.0, 21.0);
        }
        DynInput => {
            pen.rect(3.0, 6.0, 12.0, 7.0, b);
            pen.l(6.0, 9.5, 12.0, 9.5);
            pen.l(17.0, 13.0, 21.0, 21.0);
            pen.l(17.0, 13.0, 17.0, 21.0);
        }
        Gear => {
            pen.c(12.0, 12.0, 4.0, b);
            for i in 0..8 {
                let ang = (i as f32 * 45.0).to_radians();
                p.line_segment(
                    [pen.at(12.0 + 6.0 * ang.cos(), 12.0 + 6.0 * ang.sin()), pen.at(12.0 + 9.0 * ang.cos(), 12.0 + 9.0 * ang.sin())],
                    Stroke::new(2.2 * pen.sc(), b),
                );
            }
            pen.c(12.0, 12.0, 6.5, b);
        }
        Annotation => {
            pen.poly(&[(12.0, 3.0), (21.0, 20.0), (3.0, 20.0)], true, b);
            pen.l(12.0, 9.0, 12.0, 15.0);
            pen.fill_c(12.0, 17.5, 1.0, b);
        }
        Workspace => {
            pen.rect(3.0, 4.0, 18.0, 16.0, b);
            pen.l(9.0, 4.0, 9.0, 20.0);
            pen.l(9.0, 9.0, 21.0, 9.0);
        }
        ChevronDown => pen.poly(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)], false, b),
        ChevronUp => pen.poly(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)], false, b),
        ChevronLeft => pen.poly(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)], false, b),
        ChevronRight => pen.poly(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)], false, b),
        Plus => {
            pen.l(12.0, 4.0, 12.0, 20.0);
            pen.l(4.0, 12.0, 20.0, 12.0);
        }
        Switcher => {
            for (x, y) in [(3.0, 3.0), (13.0, 3.0), (3.0, 13.0), (13.0, 13.0)] {
                pen.fill_rect(x, y, 8.0, 8.0, if x == 3.0 && y == 3.0 { a } else { b });
            }
        }
        Close => {
            pen.l(6.0, 6.0, 18.0, 18.0);
            pen.l(18.0, 6.0, 6.0, 18.0);
        }
        Search => {
            pen.c(10.0, 10.0, 6.0, b);
            pen.l(14.5, 14.5, 20.0, 20.0);
        }
        Menu => {
            for y in [7.0, 12.0, 17.0] {
                pen.l(4.0, y, 20.0, y);
            }
        }
        Constraint | Fix => {
            pen.fill_rect(7.0, 11.0, 10.0, 9.0, Color32::from_rgb(0xe6, 0xb4, 0x3c));
            pen.arc(12.0, 11.0, 4.0, 0.0, 180.0, Color32::from_rgb(0xe6, 0xb4, 0x3c));
        }
        Collinear => {
            pen.l(3.0, 19.0, 9.0, 15.0);
            pen.la(12.0, 13.0, 21.0, 7.0);
        }
        Smooth => {
            // A line running into an arc without a corner.
            pen.l(3.0, 18.0, 11.0, 18.0);
            pen.arc(11.0, 10.0, 8.0, -90.0, 0.0, a);
            pen.dot(11.0, 18.0);
        }
        Symmetric => {
            pen.poly(&[(9.0, 5.0), (4.0, 12.0), (9.0, 19.0)], false, b);
            pen.poly(&[(15.0, 5.0), (20.0, 12.0), (15.0, 19.0)], false, b);
            for y in [3.0, 9.0, 15.0] {
                pen.la(12.0, y, 12.0, y + 4.0);
            }
        }
        DelConstraint => {
            let lock = alpha(Color32::from_rgb(0xe6, 0xb4, 0x3c));
            pen.fill_rect(3.0, 12.0, 9.0, 8.0, lock);
            pen.arc(7.5, 12.0, 3.5, 0.0, 180.0, lock);
            pen.l(15.0, 13.0, 21.0, 19.0);
            pen.l(21.0, 13.0, 15.0, 19.0);
        }
        Parameters => {
            pen.rect(3.0, 4.0, 18.0, 16.0, b);
            pen.text(12.0, 12.0, "fx", 10.0, a);
        }
        Coincident => {
            pen.l(3.0, 20.0, 12.0, 12.0);
            pen.l(12.0, 12.0, 21.0, 18.0);
            pen.fill_c(12.0, 12.0, 2.5, a);
        }
        Parallel => {
            pen.l(4.0, 18.0, 16.0, 4.0);
            pen.la(9.0, 20.0, 21.0, 6.0);
        }
        Perpendicular => {
            pen.l(4.0, 20.0, 20.0, 20.0);
            pen.la(12.0, 20.0, 12.0, 4.0);
            pen.rect(12.0, 16.0, 4.0, 4.0, b);
        }
        Horizontal => {
            pen.la(3.0, 12.0, 21.0, 12.0);
            pen.dot(3.0, 12.0);
            pen.dot(21.0, 12.0);
        }
        Vertical => {
            pen.la(12.0, 3.0, 12.0, 21.0);
            pen.dot(12.0, 3.0);
            pen.dot(12.0, 21.0);
        }
        Tangent => {
            pen.c(10.0, 13.0, 6.0, b);
            pen.la(3.0, 7.0, 21.0, 7.0);
        }
        Concentric => {
            pen.c(12.0, 12.0, 8.0, b);
            pen.c(12.0, 12.0, 4.0, a);
        }
        QuickProps => {
            // A small palette window with property rows and a pointer.
            pen.rect(3.0, 4.0, 15.0, 13.0, b);
            pen.l(3.0, 7.0, 18.0, 7.0);
            for y in [10.0, 13.0] {
                pen.l(5.0, y, 9.0, y);
                pen.la(11.0, y, 16.0, y);
            }
            pen.poly(&[(15.0, 14.0), (15.0, 22.0), (17.0, 20.0), (19.5, 22.5), (20.5, 21.5), (18.0, 19.0), (21.0, 18.5)], true, b);
        }
        Equal => {
            pen.l(5.0, 9.0, 19.0, 9.0);
            pen.la(5.0, 15.0, 19.0, 15.0);
        }
        Measure | Distance => {
            pen.poly(&[(3.0, 15.0), (21.0, 15.0), (21.0, 20.0), (3.0, 20.0)], true, b);
            for x in [6.0, 9.0, 12.0, 15.0, 18.0] {
                pen.l(x, 15.0, x, 17.5);
            }
            pen.la(3.0, 8.0, 21.0, 8.0);
            pen.arrow_tip(3.0, 8.0, -1.0, 0.0, a);
            pen.arrow_tip(21.0, 8.0, 1.0, 0.0, a);
        }
        Area => {
            pen.poly(&[(4.0, 18.0), (6.0, 5.0), (19.0, 7.0), (20.0, 19.0)], true, a);
            pen.text(12.0, 12.5, "A", 9.0, b);
        }
        List => {
            for y in [6.0, 11.0, 16.0] {
                pen.fill_c(5.0, y, 1.2, b);
                pen.l(8.0, y, 20.0, y);
            }
        }
        PickAdd => {
            // An object box with a plus: picks add to the selection.
            pen.rect(3.0, 3.0, 11.0, 11.0, b);
            pen.la(17.0, 11.0, 17.0, 21.0);
            pen.la(12.0, 16.0, 22.0, 16.0);
        }
        SelectObjects => {
            // A pointer picking an object box.
            pen.rect(12.0, 3.0, 9.0, 9.0, a);
            pen.poly(&[(4.0, 6.0), (4.0, 20.0), (7.5, 16.5), (10.5, 21.5), (12.5, 20.5), (9.5, 15.5), (14.0, 15.0)], true, b);
        }
        QuickSelect => {
            // A filter funnel with a spark.
            pen.poly(&[(3.0, 4.0), (19.0, 4.0), (13.0, 11.0), (13.0, 19.0), (9.0, 17.0), (9.0, 11.0)], true, b);
            pen.poly(&[(20.0, 10.0), (16.5, 15.5), (20.0, 15.5), (17.0, 21.0)], false, a);
        }
        Template | Pdf | Wrench | Quit | Cut | ClipCopy | Paste | SelectAll | Globe => menu_icon(&Pen { g: 20.0, ..pen }, icon),
        Regen | NamedViews | Viewports | Ucs | Theme | Accessibility | Xref | DataLink | Layout | LayerState | ColorWheel | Linetype | TextStyle
        | DimStyle | Units | Rename => menu_icon(&Pen { g: 20.0, ..pen }, icon),
        Script | Compare | Group | DrawOrder | Mouse3d => menu_icon(&Pen { g: 20.0, ..pen }, icon),
        Polyline3d | Multiline | Helix | CenterMark | CenterLine | Dimension | DimJogged | DimSpace | DimBreak | Tolerance | Inspection | Oblique
        | AlignText | DimOverride => menu_icon(&Pen { g: 20.0, ..pen }, icon),
        Object | Clip | Solid | Flatten | Overkill | Sample | Chat | Info => menu_icon(&Pen { g: 20.0, ..pen }, icon),
    }
}

/// The icons that only menu rows use, drawn on a 20-unit grid.
fn menu_icon(m: &Pen, icon: Icon) {
    let (b, a) = (m.base, m.acc);
    use Icon::*;
    match icon {
        Template => {
            // A sheet with ruled lines.
            m.rect(5.0, 3.0, 10.0, 14.0, b);
            m.la(8.0, 7.0, 12.0, 7.0);
            m.l(8.0, 10.0, 12.0, 10.0);
            m.l(8.0, 13.0, 10.0, 13.0);
        }
        Pdf => {
            // A page with a folded corner and two lines of text.
            m.poly(&[(5.0, 3.0), (12.0, 3.0), (15.0, 6.0), (15.0, 17.0), (5.0, 17.0)], true, b);
            m.la(8.0, 11.0, 12.0, 11.0);
            m.la(8.0, 14.0, 12.0, 14.0);
        }
        Wrench => {
            // A wrench: an open jaw on a slanted handle.
            m.arc(13.0, 7.0, 4.0, 80.0, 200.0, b);
            m.arc(13.0, 7.0, 4.0, 250.0, 370.0, b);
            m.poly(&[(13.7, 3.05), (12.5, 7.5), (16.95, 6.3)], false, b);
            m.poly(&[(9.25, 8.35), (3.9, 13.7), (6.3, 16.1), (11.65, 10.75)], false, b);
        }
        Quit => {
            // A power symbol.
            m.arc(10.0, 10.0, 6.0, 132.0, 408.0, b);
            m.la(10.0, 3.0, 10.0, 10.0);
        }
        Cut => {
            // Scissors.
            m.c(6.0, 15.0, 2.0, b);
            m.c(14.0, 15.0, 2.0, b);
            m.la(7.3, 13.4, 14.0, 3.0);
            m.la(12.7, 13.4, 6.0, 3.0);
        }
        ClipCopy => {
            // Two overlapping sheets.
            m.rect(3.0, 3.0, 10.0, 10.0, b);
            m.rect(7.0, 7.0, 10.0, 10.0, a);
        }
        Paste => {
            // A clipboard.
            m.rect(6.0, 4.0, 8.0, 13.0, b);
            m.rect(8.0, 3.0, 4.0, 2.0, a);
        }
        SelectAll => {
            // A dashed selection box.
            m.poly(&[(3.0, 6.0), (3.0, 3.0), (6.0, 3.0)], false, b);
            m.poly(&[(14.0, 3.0), (17.0, 3.0), (17.0, 6.0)], false, b);
            m.poly(&[(17.0, 14.0), (17.0, 17.0), (14.0, 17.0)], false, b);
            m.poly(&[(6.0, 17.0), (3.0, 17.0), (3.0, 14.0)], false, b);
            m.la(9.0, 3.0, 11.0, 3.0);
            m.la(17.0, 9.0, 17.0, 11.0);
            m.la(11.0, 17.0, 9.0, 17.0);
            m.la(3.0, 11.0, 3.0, 9.0);
        }
        Globe => {
            // A globe with its equator and two meridians.
            m.c(10.0, 10.0, 7.0, b);
            m.l(3.0, 10.0, 17.0, 10.0);
            for s in [-1.0, 1.0] {
                let side: Vec<(f32, f32)> = [(0.0, 3.0), (1.6, 4.9), (2.5, 7.2), (2.8, 10.0), (2.5, 12.8), (1.6, 15.1), (0.0, 17.0)]
                    .iter()
                    .map(|(x, y)| (10.0 + s * x, *y))
                    .collect();
                m.poly(&side, false, a);
            }
        }
        Regen => {
            // A circular arrow.
            m.arc(9.8, 10.0, 6.0, 30.0, 332.0, b);
            m.poly(&[(16.0, 3.0), (16.0, 7.0), (12.0, 7.0)], false, a);
        }
        NamedViews => {
            // A camera.
            m.rect(3.0, 6.0, 10.0, 8.0, b);
            m.poly(&[(13.0, 9.0), (17.0, 7.0), (17.0, 13.0), (13.0, 11.0)], false, a);
        }
        Viewports => {
            // A window split in four.
            m.rect(3.0, 4.0, 14.0, 12.0, b);
            m.la(10.0, 4.0, 10.0, 16.0);
            m.la(3.0, 10.0, 17.0, 10.0);
        }
        Ucs => {
            // The X and Y axes with arrowheads.
            m.l(4.0, 16.0, 4.0, 5.0);
            m.poly(&[(2.5, 7.0), (4.0, 5.0), (5.5, 7.0)], false, b);
            m.la(4.0, 16.0, 15.0, 16.0);
            m.poly(&[(13.0, 14.5), (15.0, 16.0), (13.0, 17.5)], false, a);
        }
        Theme => {
            // A disc, half shaded.
            m.c(10.0, 10.0, 7.0, b);
            m.l(10.0, 3.0, 10.0, 17.0);
            m.la(12.5, 3.6, 12.5, 16.4);
            m.la(15.0, 5.2, 15.0, 14.8);
        }
        Accessibility => {
            // A figure with open arms.
            m.c(10.0, 4.5, 1.0, a);
            m.l(5.0, 7.0, 15.0, 7.0);
            m.l(10.0, 7.0, 10.0, 11.5);
            m.l(10.0, 11.5, 7.0, 16.5);
            m.l(10.0, 11.5, 13.0, 16.5);
        }
        Xref => {
            // Two chain links.
            m.poly(&[(7.0, 9.0), (5.0, 11.0), (4.3, 12.5), (4.6, 14.2), (5.8, 15.4), (7.5, 15.7), (9.0, 15.0), (11.0, 13.0)], false, b);
            m.poly(&[(13.0, 11.0), (15.0, 9.0), (15.7, 7.5), (15.4, 5.8), (14.2, 4.6), (12.5, 4.3), (11.0, 5.0), (9.0, 7.0)], false, b);
            m.la(8.0, 12.0, 12.0, 8.0);
        }
        DataLink => {
            // A sheet of cells.
            m.rect(4.0, 4.0, 12.0, 12.0, b);
            m.la(4.0, 8.0, 16.0, 8.0);
            m.l(4.0, 12.0, 16.0, 12.0);
            m.l(9.0, 4.0, 9.0, 16.0);
        }
        Layout => {
            // A sheet with a viewport and a title line.
            m.rect(3.0, 4.0, 14.0, 12.0, b);
            m.rect(12.0, 6.0, 3.0, 4.0, a);
            m.l(5.0, 13.0, 10.0, 13.0);
        }
        LayerState => {
            // Stacked layers with a plus.
            m.poly(&[(10.0, 3.0), (17.0, 7.0), (10.0, 11.0), (3.0, 7.0)], true, b);
            m.poly(&[(3.0, 11.0), (10.0, 15.0), (12.5, 13.6)], false, b);
            m.la(14.0, 15.5, 18.0, 15.5);
            m.la(16.0, 13.5, 16.0, 17.5);
        }
        ColorWheel => {
            // A wheel in three sectors.
            m.c(10.0, 10.0, 7.0, b);
            m.l(10.0, 3.0, 10.0, 10.0);
            m.l(10.0, 10.0, 3.95, 13.5);
            m.la(10.0, 10.0, 16.05, 13.5);
        }
        Linetype => {
            // A continuous, a dashed and a dotted line.
            m.l(3.0, 6.0, 17.0, 6.0);
            for (x0, x1) in [(3.0, 6.0), (8.0, 12.0), (14.0, 17.0)] {
                m.la(x0, 10.0, x1, 10.0);
            }
            for x in [3.0, 6.0, 9.0, 12.0, 15.0] {
                m.l(x, 14.0, x + 1.0, 14.0);
            }
        }
        TextStyle => {
            // A letter on a baseline.
            m.poly(&[(5.0, 15.0), (10.0, 3.0), (15.0, 15.0)], false, b);
            m.l(7.0, 11.0, 13.0, 11.0);
            m.la(4.0, 18.0, 16.0, 18.0);
        }
        DimStyle => {
            // A dimension line with open arrowheads.
            m.l(3.0, 6.0, 3.0, 14.0);
            m.l(17.0, 6.0, 17.0, 14.0);
            m.la(3.0, 10.0, 17.0, 10.0);
            m.poly(&[(5.0, 8.0), (3.0, 10.0), (5.0, 12.0)], false, a);
            m.poly(&[(15.0, 8.0), (17.0, 10.0), (15.0, 12.0)], false, a);
        }
        Units => {
            // A ruler.
            m.rect(3.0, 8.0, 14.0, 5.0, b);
            for (x, y) in [(6.0, 10.0), (9.0, 11.0), (12.0, 10.0), (15.0, 11.0)] {
                m.la(x, 8.0, x, y);
            }
        }
        Rename => {
            // A name field with a text cursor.
            m.rect(3.0, 7.0, 11.0, 6.0, b);
            m.la(16.0, 5.0, 16.0, 15.0);
            m.la(15.0, 5.0, 17.0, 5.0);
            m.la(15.0, 15.0, 17.0, 15.0);
        }
        Script => {
            // A sheet with a command prompt.
            m.rect(5.0, 3.0, 10.0, 14.0, b);
            m.poly(&[(7.5, 8.0), (9.5, 10.0), (7.5, 12.0)], false, a);
            m.la(11.0, 12.0, 13.0, 12.0);
        }
        Compare => {
            // Two sheets side by side, one line apart.
            m.rect(3.0, 4.0, 6.0, 12.0, b);
            m.rect(11.0, 4.0, 6.0, 12.0, b);
            m.l(5.0, 8.0, 7.0, 8.0);
            m.l(13.0, 8.0, 15.0, 8.0);
            m.l(5.0, 11.0, 7.0, 11.0);
            m.la(13.0, 12.0, 15.0, 12.0);
        }
        Group => {
            // Four boxes held together.
            for (x, y) in [(3.0, 3.0), (12.0, 12.0)] {
                m.rect(x, y, 5.0, 5.0, b);
            }
            for (x, y) in [(3.0, 12.0), (12.0, 3.0)] {
                m.rect(x, y, 5.0, 5.0, a);
            }
        }
        DrawOrder => {
            // One box in front of another.
            m.rect(4.0, 4.0, 8.0, 8.0, b);
            m.rect(8.0, 8.0, 8.0, 8.0, a);
        }
        Mouse3d => {
            // A 3D mouse: a cap on a round base.
            let base: Vec<(f32, f32)> =
                (0..24).map(|i| (i as f32 * 15.0f32).to_radians()).map(|t| (10.0 + 6.0 * t.cos(), 13.0 + 3.0 * t.sin())).collect();
            m.poly(&base, true, b);
            m.la(7.0, 12.0, 7.0, 7.0);
            m.arc(10.0, 7.0, 3.0, 0.0, 180.0, a);
            m.la(13.0, 7.0, 13.0, 12.0);
        }
        Polyline3d => {
            // A polyline climbing off its base line.
            m.l(3.0, 15.0, 6.0, 15.0);
            m.poly(&[(3.0, 15.0), (7.0, 9.0), (11.0, 12.0), (17.0, 5.0)], false, a);
            m.fill_c(7.0, 9.0, 1.3, m.pt);
            m.fill_c(11.0, 12.0, 1.3, m.pt);
        }
        Multiline => {
            // Two parallel lines, capped.
            m.la(3.0, 8.0, 17.0, 8.0);
            m.la(3.0, 12.0, 17.0, 12.0);
            m.l(3.0, 8.0, 3.0, 12.0);
            m.l(17.0, 8.0, 17.0, 12.0);
        }
        Helix => {
            // A spring seen from the side.
            let coil: Vec<(f32, f32)> = (0..=54)
                .map(|i| i as f32 * 20.0f32.to_radians())
                .map(|t| (10.0 + 5.0 * t.cos(), 4.0 + t * 12.0 / (6.0 * std::f32::consts::PI)))
                .collect();
            m.poly(&coil, false, a);
        }
        CenterMark => {
            // A cross with a gap round its centre.
            m.l(10.0, 3.0, 10.0, 7.0);
            m.l(10.0, 13.0, 10.0, 17.0);
            m.l(3.0, 10.0, 7.0, 10.0);
            m.l(13.0, 10.0, 17.0, 10.0);
            m.la(9.0, 10.0, 11.0, 10.0);
            m.la(10.0, 9.0, 10.0, 11.0);
        }
        CenterLine => {
            // A dashed line midway between two lines.
            m.l(5.0, 4.0, 5.0, 16.0);
            m.l(15.0, 4.0, 15.0, 16.0);
            for (y0, y1) in [(2.0, 5.0), (7.0, 9.0), (11.0, 13.0), (15.0, 18.0)] {
                m.la(10.0, y0, 10.0, y1);
            }
        }
        Dimension => {
            // A dimension line between two extension lines.
            m.l(3.0, 6.0, 3.0, 14.0);
            m.l(17.0, 6.0, 17.0, 14.0);
            m.la(3.0, 10.0, 17.0, 10.0);
        }
        DimJogged => {
            // An arc with a jogged radius line.
            m.arc(15.0, 15.0, 12.0, 180.0, 90.0, b);
            m.poly(&[(15.0, 15.0), (12.0, 11.0), (14.0, 10.0), (10.0, 6.0)], false, a);
        }
        DimSpace => {
            // Evenly spaced dimension lines.
            for y in [6.0, 10.0, 14.0] {
                m.l(3.0, y, 14.0, y);
            }
            m.la(17.0, 4.0, 17.0, 16.0);
            m.la(16.0, 6.0, 18.0, 6.0);
            m.la(16.0, 10.0, 18.0, 10.0);
            m.la(16.0, 14.0, 18.0, 14.0);
        }
        DimBreak => {
            // A dimension line broken where another line crosses.
            m.l(3.0, 10.0, 8.0, 10.0);
            m.l(12.0, 10.0, 17.0, 10.0);
            m.la(9.0, 6.0, 11.0, 14.0);
        }
        Tolerance => {
            // A feature control frame.
            m.rect(3.0, 6.0, 14.0, 8.0, b);
            m.l(8.0, 6.0, 8.0, 14.0);
            m.l(12.0, 6.0, 12.0, 14.0);
            m.c(5.5, 10.0, 1.3, a);
        }
        Inspection => {
            // An inspection frame with round ends.
            m.l(6.0, 7.0, 14.0, 7.0);
            m.l(6.0, 13.0, 14.0, 13.0);
            m.arc(6.0, 10.0, 3.0, 90.0, 270.0, b);
            m.arc(14.0, 10.0, 3.0, -90.0, 90.0, b);
            m.la(8.0, 7.0, 8.0, 13.0);
            m.la(12.0, 7.0, 12.0, 13.0);
        }
        Oblique => {
            // Slanted extension lines.
            m.l(5.0, 4.0, 3.0, 16.0);
            m.l(17.0, 4.0, 15.0, 16.0);
            m.la(4.0, 10.0, 16.0, 10.0);
        }
        AlignText => {
            // Text centred between two lines.
            m.l(3.0, 6.0, 17.0, 6.0);
            m.la(6.0, 10.0, 14.0, 10.0);
            m.l(3.0, 14.0, 17.0, 14.0);
        }
        DimOverride => {
            // A pencil.
            m.poly(&[(4.0, 13.0), (11.0, 6.0), (14.0, 9.0), (7.0, 16.0), (4.0, 16.0)], true, b);
            m.la(12.0, 5.0, 15.0, 8.0);
        }
        Object => {
            // A box inside a frame.
            m.rect(4.0, 4.0, 12.0, 12.0, b);
            m.rect(8.0, 8.0, 4.0, 4.0, a);
        }
        Clip => {
            // A frame cut by a diagonal.
            m.rect(4.0, 4.0, 12.0, 12.0, b);
            m.la(8.0, 8.0, 16.0, 16.0);
        }
        Solid => {
            // A cube.
            m.poly(&[(10.0, 3.0), (16.0, 6.5), (16.0, 13.5), (10.0, 17.0), (4.0, 13.5), (4.0, 6.5)], true, b);
            m.poly(&[(4.0, 6.5), (10.0, 10.0), (16.0, 6.5)], false, a);
            m.la(10.0, 10.0, 10.0, 17.0);
        }
        Flatten => {
            // A shape pressed onto a plane.
            m.l(3.0, 14.0, 17.0, 14.0);
            m.poly(&[(6.0, 10.0), (10.0, 5.0), (14.0, 10.0)], false, a);
        }
        Overkill => {
            // Two overlapping boxes with a line struck out.
            m.rect(3.0, 3.0, 10.0, 10.0, b);
            m.rect(7.0, 7.0, 10.0, 10.0, b);
            m.la(9.0, 12.0, 15.0, 12.0);
        }
        Sample => {
            // A drawing sheet with a view and a title line.
            m.rect(4.0, 3.0, 12.0, 14.0, b);
            m.rect(7.0, 7.0, 6.0, 4.0, a);
            m.l(7.0, 14.0, 13.0, 14.0);
        }
        Chat => {
            // A speech bubble.
            m.poly(&[(4.0, 5.0), (16.0, 5.0), (16.0, 13.0), (9.0, 13.0), (5.0, 16.0), (5.0, 13.0), (4.0, 13.0)], true, b);
            m.la(7.0, 9.0, 13.0, 9.0);
        }
        Info => {
            // An i in a circle.
            m.c(10.0, 10.0, 7.0, b);
            m.la(10.0, 9.0, 10.0, 14.0);
            m.fill_c(10.0, 6.5, 0.9, a);
        }
        _ => {}
    }
}

/// A shared button interaction with CADCraft's original multicolor icon painting.
pub fn button(ui: &mut egui::Ui, icon: Icon, size: f32, tooltip: &str, selected: bool) -> egui::Response {
    let t = Tokens::get();
    let visuals = |fill| ButtonVisuals { fill, stroke: Stroke::NONE, icon: t.icon };
    let style = IconButtonStyle {
        normal: visuals(Color32::TRANSPARENT),
        hovered: visuals(t.control_hover.gamma_multiply(0.6)),
        pressed: None,
        selected: visuals(t.tab_active),
        corner_radius: egui::CornerRadius::same(3),
        focus_stroke: Stroke::new(1.0, t.accent),
    };
    let resp = IconButton::new(crate::i18n::t(tooltip), egui::vec2(size, size), &style).selected(selected).show(ui, |ui, rect, _| {
        // A single tint would erase the blue geometry and orange picked points. Keep the
        // original painter and its explicit dimming on the same parent-enabled UI.
        paint(ui.painter(), rect.shrink(size * 0.08), icon, !ui.is_enabled());
    });
    crate::control::record_widget(ui, &resp, tooltip, "button", Some(selected));
    resp.on_hover_text(crate::i18n::t(tooltip))
}

/// Labels and focus outlines for controls whose existing geometry is painted by the app.
/// Pointer focus is opt-in for navigation tabs; drafting buttons keep their command-line flow.
pub fn describe_control(ui: &egui::Ui, response: &egui::Response, label: &str, selected: Option<bool>, tab: bool) {
    let label = crate::i18n::t(label);
    response.widget_info(|| match selected {
        Some(selected) => egui::WidgetInfo::selected(egui::WidgetType::Button, response.enabled(), selected, label),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label),
    });
    if tab {
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_role(egui::accesskit::Role::Tab);
            node.clear_toggled();
            if let Some(selected) = selected {
                node.set_selected(selected);
            }
        });
        if response.clicked() {
            response.request_focus();
        }
    }
    if response.has_focus() && response.rect.width() > 4.0 && response.rect.height() > 4.0 {
        ui.painter().rect_stroke(response.rect.shrink(2.0), 2.0, Stroke::new(1.0, Tokens::get().accent), egui::StrokeKind::Inside);
    }
    crate::control::record_widget(ui, response, label, if tab { "tab" } else { "button" }, selected);
}
