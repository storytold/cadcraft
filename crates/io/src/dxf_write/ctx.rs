use super::{DimVal, Handle, HashMap, K, Vec3, Version, dxf_ext};

/// The `DIMASSOC` object of an associative dimension (stored in the dimension's extension
/// dictionary under `ACAD_DIMASSOC`).
pub(super) struct AssocObj {
    pub(super) xdict: String,
    pub(super) handle: String,
    /// Associativity flag (bit per point reference).
    pub(super) flags: i64,
    pub(super) refs: Vec<OsnapRef>,
}

/// An `AcDbOsnapPointRef` of a DIMASSOC object.
pub(super) struct OsnapRef {
    pub(super) osnap: i64,
    pub(super) main: Handle,
    /// The 1-based segment of a polyline the point is an end of (group 91, with subentity type
    /// 73 = 2, edge); 0 for an object made of one curve.
    pub(super) segment: i64,
    pub(super) param: f64,
    pub(super) point: Vec3,
    pub(super) other: Option<Handle>,
}

/// Handles and names the entity writer needs from the rest of the file.
#[derive(Default)]
pub(super) struct Ctx {
    /// Multiline styles: MLINESTYLE handles and their dictionary.
    pub(super) mlines: crate::dxf_mline::Plan,
    /// First viewport of each layout that has no paper-space viewport (id 1) → the layout's index;
    /// [`paper_view`] writes that layout's paper-space viewport ahead of it.
    pub(super) paper_views: HashMap<Handle, usize>,
    /// Dimension → anonymous `*D` block name.
    pub(super) dim_blocks: HashMap<Handle, String>,
    /// Upper-case text style name → STYLE record handle.
    pub(super) styles: HashMap<String, String>,
    /// Upper-case arrow name (as stored in styles/overrides) → BLOCK_RECORD handle.
    pub(super) arrows: HashMap<String, String>,
    /// Upper-case linetype name → LTYPE record handle (DIMLTYPE, DIMLTEX1, DIMLTEX2).
    pub(super) ltypes: HashMap<String, String>,
    /// Associative dimension → its DIMASSOC object.
    pub(super) assoc: HashMap<Handle, AssocObj>,
    /// Table → (anonymous `*T` block name, BLOCK_RECORD handle).
    pub(super) tables: HashMap<Handle, (String, String)>,
    /// Table style name → TABLESTYLE handle (the first is the fallback).
    pub(super) table_styles: Vec<(String, String)>,
    /// Multileader style name → MLEADERSTYLE handle (the first is the fallback).
    pub(super) mleader_styles: Vec<(String, String)>,
    /// Upper-case block name → BLOCK_RECORD handle (multileader block content).
    pub(super) block_records: HashMap<String, String>,
    /// Raster images' IMAGEDEF, IMAGEDEF_REACTOR and dictionary handles.
    pub(super) images: crate::dxf_image::Plan,
    /// The version the file ends up in (the DWG version for a DWG save): it decides which
    /// entity types exist (MULTILEADER from 2007).
    pub(super) target: Version,
}

impl Ctx {
    /// Whether multileaders are written as MULTILEADER (AutoCAD 2007 / AC1021 and later);
    /// older files get LEADER + MTEXT/INSERT.
    pub(super) fn native_mleader(&self) -> bool {
        self.target >= Version::R2007
    }
    pub(super) fn style(&self, name: &str) -> Option<String> {
        self.styles.get(&name.to_ascii_uppercase()).cloned()
    }
    fn arrow(&self, name: &str) -> Option<String> {
        self.arrows.get(&name.trim().to_ascii_uppercase()).cloned()
    }
    pub(super) fn table_style(&self, name: &str) -> String {
        Self::named(&self.table_styles, name)
    }
    pub(super) fn mleader_style(&self, name: &str) -> String {
        Self::named(&self.mleader_styles, name)
    }
    /// The handle of the style `name` in `styles`, or of the first one.
    fn named(styles: &[(String, String)], name: &str) -> String {
        styles.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).or_else(|| styles.first()).map(|(_, h)| h.clone()).unwrap_or_else(|| "0".into())
    }
    /// A dimension variable encoded with this file's handles; integers outside the 16-bit
    /// range of their groups are dropped.
    pub(super) fn encode(&self, kind: K, v: &serde_json::Value) -> Option<DimVal> {
        let ltype = |n: &str| self.ltypes.get(&n.trim().to_ascii_uppercase()).cloned();
        dxf_ext::encode(kind, v, &|n| self.style(n), &|n| self.arrow(n), &ltype)
            .filter(|x| !matches!(x, DimVal::Int(i) if i16::try_from(*i).is_err()))
    }
}
