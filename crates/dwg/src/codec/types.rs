//! What the container layer reports about a file besides its bytes.

/// How serious a note about a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Worth knowing; nothing is lost.
    Info,
    /// Something did not check out and was read around.
    Warning,
    /// Something could not be read.
    Error,
}

/// A note about something in a file that did not check out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

/// One stored section of an R2004-family file, or one section locator of an R13-R2000 file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionInfo {
    pub name: String,
    pub size: u64,
    pub pages: u32,
    pub compressed: bool,
    pub encrypted: bool,
}

/// A class the file defines for its custom objects and entities.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassDef {
    pub number: u16,
    pub proxy_flags: u16,
    pub app_name: String,
    pub cpp_name: String,
    pub dxf_name: String,
    pub was_zombie: bool,
    /// 0x1F2 for entity classes, 0x1F3 for object classes.
    pub item_class_id: u16,
    /// Objects of the class the drawing has; 0 when unknown.
    pub instance_count: u32,
    /// The DWG version and maintenance release the class came with (R2004+); 0 when unknown.
    pub dwg_version: u32,
    pub maintenance_version: u32,
}

/// The name of one of DWG's fixed object types and whether it is an entity: `("LINE", true)` for
/// 0x13. `None` for class-defined types (500 and up, see [`type_name`]) and unknown numbers. The
/// numbers are the specification's (chapter 20, object type codes).
pub fn fixed_type(code: u16) -> Option<(&'static str, bool)> {
    Some(match code {
        0x01 => ("TEXT", true),
        0x02 => ("ATTRIB", true),
        0x03 => ("ATTDEF", true),
        0x04 => ("BLOCK", true),
        0x05 => ("ENDBLK", true),
        0x06 => ("SEQEND", true),
        0x07 => ("INSERT", true),
        0x08 => ("MINSERT", true),
        0x0A => ("VERTEX_2D", true),
        0x0B => ("VERTEX_3D", true),
        0x0C => ("VERTEX_MESH", true),
        0x0D => ("VERTEX_PFACE", true),
        0x0E => ("VERTEX_PFACE_FACE", true),
        0x0F => ("POLYLINE_2D", true),
        0x10 => ("POLYLINE_3D", true),
        0x11 => ("ARC", true),
        0x12 => ("CIRCLE", true),
        0x13 => ("LINE", true),
        0x14 => ("DIMENSION_ORDINATE", true),
        0x15 => ("DIMENSION_LINEAR", true),
        0x16 => ("DIMENSION_ALIGNED", true),
        0x17 => ("DIMENSION_ANG_3PT", true),
        0x18 => ("DIMENSION_ANG_2LN", true),
        0x19 => ("DIMENSION_RADIUS", true),
        0x1A => ("DIMENSION_DIAMETER", true),
        0x1B => ("POINT", true),
        0x1C => ("3DFACE", true),
        0x1D => ("POLYLINE_PFACE", true),
        0x1E => ("POLYLINE_MESH", true),
        0x1F => ("SOLID", true),
        0x20 => ("TRACE", true),
        0x21 => ("SHAPE", true),
        0x22 => ("VIEWPORT", true),
        0x23 => ("ELLIPSE", true),
        0x24 => ("SPLINE", true),
        0x25 => ("REGION", true),
        0x26 => ("3DSOLID", true),
        0x27 => ("BODY", true),
        0x28 => ("RAY", true),
        0x29 => ("XLINE", true),
        0x2A => ("DICTIONARY", false),
        0x2B => ("OLEFRAME", true),
        0x2C => ("MTEXT", true),
        0x2D => ("LEADER", true),
        0x2E => ("TOLERANCE", true),
        0x2F => ("MLINE", true),
        0x30 => ("BLOCK_CONTROL", false),
        0x31 => ("BLOCK_HEADER", false),
        0x32 => ("LAYER_CONTROL", false),
        0x33 => ("LAYER", false),
        0x34 => ("STYLE_CONTROL", false),
        0x35 => ("STYLE", false),
        0x38 => ("LTYPE_CONTROL", false),
        0x39 => ("LTYPE", false),
        0x3C => ("VIEW_CONTROL", false),
        0x3D => ("VIEW", false),
        0x3E => ("UCS_CONTROL", false),
        0x3F => ("UCS", false),
        0x40 => ("VPORT_CONTROL", false),
        0x41 => ("VPORT", false),
        0x42 => ("APPID_CONTROL", false),
        0x43 => ("APPID", false),
        0x44 => ("DIMSTYLE_CONTROL", false),
        0x45 => ("DIMSTYLE", false),
        0x46 => ("VX_CONTROL", false),
        0x47 => ("VX_TABLE_RECORD", false),
        0x48 => ("GROUP", false),
        0x49 => ("MLINESTYLE", false),
        0x4A => ("OLE2FRAME", true),
        0x4B => ("DUMMY", false),
        0x4C => ("LONG_TRANSACTION", true),
        0x4D => ("LWPOLYLINE", true),
        0x4E => ("HATCH", true),
        0x4F => ("XRECORD", false),
        0x50 => ("ACDBPLACEHOLDER", false),
        0x51 => ("VBA_PROJECT", false),
        0x52 => ("LAYOUT", false),
        0x1F2 => ("ACAD_PROXY_ENTITY", true),
        0x1F3 => ("ACAD_PROXY_OBJECT", false),
        _ => return None,
    })
}

/// The name of any object type and whether it is an entity: a fixed type's, or for 500 and up the
/// DXF name of the class the file defines for it (`CLASS_<n>` when the file defines none).
pub fn type_name(code: u16, classes: &[ClassDef]) -> (String, bool) {
    if let Some((name, entity)) = fixed_type(code) {
        return (name.to_string(), entity);
    }
    match classes.iter().find(|c| c.number == code) {
        Some(c) if !c.dxf_name.is_empty() => (c.dxf_name.clone(), c.item_class_id == 0x1F2),
        Some(c) => (format!("CLASS_{code}"), c.item_class_id == 0x1F2),
        None => (format!("CLASS_{code}"), false),
    }
}
