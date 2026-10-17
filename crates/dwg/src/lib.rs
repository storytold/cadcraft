//! DWG support through a DXF bridge.
//!
//! DWG files are read with the `acadrust` crate (MPL-2.0, used unmodified as a dependency) and
//! converted to DXF bytes, which CADCraft's own DXF reader maps to its document model; saving
//! goes the other way. Keeping the dependency behind this byte-level API isolates it: nothing
//! else in CADCraft depends on its types. Native targets only (it memory-maps files).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// DWG version codes we can write.
pub const VERSIONS: &[(&str, &str)] =
    &[("AC1015", "2000"), ("AC1018", "2004"), ("AC1021", "2007"), ("AC1024", "2010"), ("AC1027", "2013"), ("AC1032", "2018")];

/// True when the bytes look like a DWG file (`AC10xx` magic).
pub fn is_dwg(bytes: &[u8]) -> bool {
    bytes.len() > 6 && bytes.starts_with(b"AC10") && bytes.get(4..6).is_some_and(|v| v.iter().all(u8::is_ascii_digit))
}

/// The DWG version code of a file (e.g. "AC1032").
pub fn version(bytes: &[u8]) -> Option<String> {
    is_dwg(bytes).then(|| String::from_utf8_lossy(bytes.get(0..6).unwrap_or_default()).to_string())
}

/// Size limits for reading a DWG file. DWG is compressed and the DXF text it converts to is
/// several times larger (about 6x in our tests: a 10 MB DWG becomes about 60 MB of DXF), so
/// the limits are generous for real drawings. They exist because a damaged file, or one the reader
/// misreads, can make the DWG reader produce a drawing thousands of times larger than the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest DWG file read at all (checked before reading starts).
    pub max_dwg_bytes: usize,
    /// Largest DXF rendition of a DWG (checked while it is written).
    pub max_dxf_bytes: usize,
}

impl Limits {
    /// 512 MB of DWG, 1 GB of DXF (more than the DXF reader's group-code limit fills).
    pub const DEFAULT: Limits = Limits { max_dwg_bytes: 512 << 20, max_dxf_bytes: 1 << 30 };
}

/// A byte count in megabytes for messages ("9.9 MB", "1024 MB").
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn mb(n: u64) -> String {
    let m = n as f64 / f64::from(1u32 << 20);
    if m >= 100.0 { format!("{m:.0} MB") } else { format!("{m:.1} MB") }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::{Cursor, Write};
    use std::panic::AssertUnwindSafe;

    use super::{Limits, mb};

    /// Read DWG bytes and return an ASCII DXF rendition, within [`Limits::DEFAULT`].
    pub fn dwg_to_dxf(bytes: &[u8]) -> Result<Vec<u8>, String> {
        dwg_to_dxf_with(bytes, Limits::DEFAULT)
    }

    /// [`dwg_to_dxf`] with explicit limits. Both are checked before the expensive step they
    /// guard: the input size before the DWG reader runs, the DXF size while it is written, so a
    /// runaway conversion stops at the limit instead of filling memory first.
    pub fn dwg_to_dxf_with(bytes: &[u8], limits: Limits) -> Result<Vec<u8>, String> {
        dwg_to_dxf_noted(bytes, limits).map(|(dxf, _)| dxf)
    }

    /// [`dwg_to_dxf_with`], also returning notes on what the read left out. The reader runs in
    /// failsafe mode: an object record it can't decode is skipped instead of failing the whole
    /// file, and the notes say how many were skipped and of which types, so a partial read never
    /// passes for a complete one.
    pub fn dwg_to_dxf_noted(bytes: &[u8], limits: Limits) -> Result<(Vec<u8>, Vec<String>), String> {
        if bytes.len() > limits.max_dwg_bytes {
            return Err(format!("DWG: the file is {}, larger than the {} CADCraft opens", mb(bytes.len() as u64), mb(limits.max_dwg_bytes as u64)));
        }
        let outcome = std::panic::catch_unwind(|| {
            acadrust::DwgReader::from_stream_with_options(Cursor::new(bytes), acadrust::DwgReadOptions::failsafe()).read_with_stats()
        })
        .map_err(|_| "the DWG reader failed on this file".to_string())?
        .map_err(|e| format!("DWG: {e}"))?;
        // Failsafe skips records, not whole files: when the objects stream itself couldn't be
        // read (a damaged header or section map, or not a DWG at all), nothing was read.
        if !outcome.stats.record_stream_read {
            let why = outcome.stats.diagnostics.first().map(|d| d.message.trim_start_matches("Failsafe: ").to_string());
            return Err(format!("DWG: {}", why.unwrap_or_else(|| "the file's objects couldn't be read".to_string())));
        }
        let mut notes = read_notes(&outcome.stats);
        let mut doc = outcome.document;
        table_merges(&mut doc);
        table_style_rows(&mut doc, false);
        let orphans = drop_orphaned_dgn_components(&mut doc);
        if orphans > 0 {
            let (s, were) = if orphans == 1 { ("", "was") } else { ("s", "were") };
            notes.push(format!("DWG: {orphans} DGN line style component{s} that no line style definition uses {were} left out"));
        }
        let mut out = CappedWriter { buf: Vec::new(), limit: limits.max_dxf_bytes, exceeded: false };
        let written = std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DxfWriter::new(&doc).write_to_writer(&mut out)))
            .map_err(|_| "the DWG→DXF conversion failed on this file".to_string())?;
        if out.exceeded {
            return Err(format!(
                "DWG: converting this {} file produced more than {} of drawing data (the limit); \
                 the file is probably damaged, or uses objects the DWG reader misreads",
                mb(bytes.len() as u64),
                mb(limits.max_dxf_bytes as u64)
            ));
        }
        written.map_err(|e| format!("DWG→DXF: {e}"))?;
        Ok((add_header_vars(out.buf, &doc.header), notes))
    }

    /// Most record types named in the skipped-records note.
    const NOTE_TYPES: usize = 6;

    /// The note on object records a failsafe read skipped: how many, and how many of each type
    /// (most first), or nothing when the read was complete.
    pub(crate) fn read_notes(stats: &acadrust::ReadStats) -> Vec<String> {
        let skipped = stats.skipped_source_records;
        if skipped == 0 {
            return Vec::new();
        }
        let label = |t: Option<&str>| match t.map(str::parse::<i16>) {
            Some(Ok(code)) => type_name(code).map_or_else(|| format!("type {code}"), str::to_string),
            Some(Err(_)) => t.unwrap_or_default().to_string(),
            None => "unknown".to_string(),
        };
        let mut by_type: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for d in stats.diagnostics.iter().filter(|d| d.record_handle.is_some()) {
            *by_type.entry(label(d.record_type.as_deref())).or_default() += 1;
        }
        let mut types: Vec<(String, usize)> = by_type.into_iter().collect();
        types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut list: Vec<String> = types.iter().take(NOTE_TYPES).map(|(t, n)| format!("{t} × {n}")).collect();
        if types.len() > NOTE_TYPES {
            list.push(format!("{} more types", types.len() - NOTE_TYPES));
        }
        let of = if stats.source_records > 0 { format!(" of {}", stats.source_records) } else { String::new() };
        let records = if skipped == 1 && of.is_empty() { "object record" } else { "object records" };
        let (were, them) = if skipped == 1 { ("was", "it") } else { ("were", "them") };
        let which = if list.is_empty() { String::new() } else { format!(" ({})", list.join(", ")) };
        vec![format!(
            "DWG: {skipped}{of} {records} couldn't be read and {were} left out{which}; \
             saving over the original file would lose {them}"
        )]
    }

    /// The DXF name of a DWG object type code with a fixed number (classes, 500 and up, are
    /// numbered per file).
    fn type_name(code: i16) -> Option<&'static str> {
        Some(match code {
            1 => "TEXT",
            2 => "ATTRIB",
            3 => "ATTDEF",
            4 => "BLOCK",
            5 => "ENDBLK",
            6 => "SEQEND",
            7 => "INSERT",
            8 => "MINSERT",
            10..=14 => "VERTEX",
            15 | 16 | 29 | 30 => "POLYLINE",
            17 => "ARC",
            18 => "CIRCLE",
            19 => "LINE",
            20..=26 => "DIMENSION",
            27 => "POINT",
            28 => "3DFACE",
            31 => "SOLID",
            32 => "TRACE",
            33 => "SHAPE",
            34 => "VIEWPORT",
            35 => "ELLIPSE",
            36 => "SPLINE",
            37 => "REGION",
            38 => "3DSOLID",
            39 => "BODY",
            40 => "RAY",
            41 => "XLINE",
            42 => "DICTIONARY",
            43 => "OLEFRAME",
            44 => "MTEXT",
            45 => "LEADER",
            46 => "TOLERANCE",
            47 => "MLINE",
            48 => "BLOCK_CONTROL",
            49 => "BLOCK_RECORD",
            50 => "LAYER_CONTROL",
            51 => "LAYER",
            52 => "STYLE_CONTROL",
            53 => "STYLE",
            56 => "LTYPE_CONTROL",
            57 => "LTYPE",
            60 => "VIEW_CONTROL",
            61 => "VIEW",
            62 => "UCS_CONTROL",
            63 => "UCS",
            64 => "VPORT_CONTROL",
            65 => "VPORT",
            66 => "APPID_CONTROL",
            67 => "APPID",
            68 => "DIMSTYLE_CONTROL",
            69 => "DIMSTYLE",
            70 => "VX_CONTROL",
            71 => "VX_TABLE_RECORD",
            72 => "GROUP",
            73 => "MLINESTYLE",
            74 => "OLE2FRAME",
            77 => "LWPOLYLINE",
            78 => "HATCH",
            79 => "XRECORD",
            80 => "ACDBPLACEHOLDER",
            82 => "LAYOUT",
            _ => return None,
        })
    }

    /// Merged table cells onto the cells. acadrust 0.6.3 reads a 2010+ (AC1024+) DWG table's
    /// merged cells into the table's `merged_ranges` only, while its DXF writer writes each cell's
    /// own merge groups (173, 175, 176), so without this the merges would be lost. Tables whose
    /// cells already carry merges are left alone. Each range's first cell gets the span; the
    /// cells it covers are marked merged with no span of their own, as CADCraft's DXF writer does.
    fn table_merges(doc: &mut acadrust::CadDocument) {
        for e in doc.entities_mut() {
            let acadrust::entities::EntityType::Table(t) = e else { continue };
            if t.merged_ranges.is_empty() || t.rows.iter().flat_map(|r| &r.cells).any(|c| c.merge_width > 1 || c.merge_height > 1) {
                continue;
            }
            let (rows, cols) = (t.rows.len(), t.columns.len());
            // A real table has at most one merge per cell; more ranges than that come from a
            // damaged file and would only make this rows × cols × ranges.
            let max_ranges = rows.saturating_mul(cols).min(1 << 16);
            for r in t.merged_ranges.iter().take(max_ranges).cloned().collect::<Vec<_>>() {
                // Clamp to the table: the ranges come from the file.
                let (bottom, right) = (r.bottom_row.min(rows.saturating_sub(1)), r.right_col.min(cols.saturating_sub(1)));
                if r.top_row > bottom || r.left_col > right || rows == 0 || cols == 0 || (r.top_row, r.left_col) == (bottom, right) {
                    continue;
                }
                let span = |n: usize| i32::try_from(n + 1).unwrap_or(i32::MAX);
                for (ri, row) in t.rows.iter_mut().enumerate().take(bottom + 1).skip(r.top_row) {
                    for (ci, cell) in row.cells.iter_mut().enumerate().take(right + 1).skip(r.left_col) {
                        cell.merged = 1;
                        let first = (ri, ci) == (r.top_row, r.left_col);
                        cell.merge_width = if first { span(right - r.left_col) } else { 0 };
                        cell.merge_height = if first { span(bottom - r.top_row) } else { 0 };
                    }
                }
            }
        }
    }

    /// Table styles' title and header suppression between the two places acadrust 0.6.3 keeps
    /// it. 2010+ (AC1024+) DWG stores it only in the style's flags (title 1, header 2), while
    /// acadrust's DXF reader and writer use the `title_suppressed`/`header_suppressed` fields
    /// (groups 280, 281). `to_flags` copies the fields into the flags before a DWG is written;
    /// otherwise the flags are copied into the fields after one is read. Older versions store
    /// both and are left alone.
    fn table_style_rows(doc: &mut acadrust::CadDocument, to_flags: bool) {
        use acadrust::objects::{ObjectType, TableStyleFlags};
        if doc.version < acadrust::DxfVersion::AC1024 {
            return;
        }
        for o in doc.objects.values_mut() {
            let ObjectType::TableStyle(s) = o else { continue };
            for (bit, suppressed) in
                [(TableStyleFlags::TITLE_SUPPRESSED, &mut s.title_suppressed), (TableStyleFlags::HEADER_SUPPRESSED, &mut s.header_suppressed)]
            {
                if to_flags {
                    s.flags.set(bit, *suppressed);
                } else {
                    *suppressed = s.flags.contains(bit);
                }
            }
        }
    }

    /// Leave out DGN line style components (`LS*COMPONENT`) that no line style definition uses
    /// (#1482). A drawing that once came from MicroStation can carry hundreds of thousands of
    /// them, orphaned copies piled up by earlier DGN imports and copy-pastes (AutoCAD's PURGE
    /// removes them as orphaned data): in a real 83 MB site plan they were 888,483 of 889,053
    /// components and made its DXF rendition 1.2 GB, over [`Limits::DEFAULT`], so the file
    /// didn't open at all. No definition that was read refers to them (a definition the reader
    /// skipped can't use them either) and CADCraft doesn't draw DGN line styles, so nothing
    /// visible is lost.
    /// Every component a definition reaches (directly, or through compound and point
    /// components) is kept, as are the definitions; dictionary entries that named a dropped
    /// component go with it. Returns how many were left out.
    fn drop_orphaned_dgn_components(doc: &mut acadrust::CadDocument) -> usize {
        use acadrust::objects::{DgnLineStyleData, ObjectType};
        use std::collections::{HashMap, HashSet};
        // Each component's references, and the definitions' root components to start from.
        let mut refs: HashMap<acadrust::Handle, Vec<acadrust::Handle>> = HashMap::new();
        let mut stack: Vec<acadrust::Handle> = Vec::new();
        for (h, o) in &doc.objects {
            let ObjectType::DgnLineStyle(ls) = o else { continue };
            match &ls.data {
                DgnLineStyleData::Definition { root_component, .. } => stack.push(*root_component),
                DgnLineStyleData::Component { component, .. } => {
                    refs.insert(*h, component.references());
                }
                DgnLineStyleData::Registered { .. } => {}
            }
        }
        if refs.is_empty() {
            return 0;
        }
        // Each component is visited once, so cycles and shared subtrees end.
        let mut used: HashSet<acadrust::Handle> = HashSet::new();
        while let Some(h) = stack.pop() {
            if let Some(r) = refs.get(&h)
                && used.insert(h)
            {
                stack.extend(r.iter().copied());
            }
        }
        let orphans: HashSet<acadrust::Handle> = refs.into_keys().filter(|h| !used.contains(h)).collect();
        if orphans.is_empty() {
            return 0;
        }
        doc.objects.retain(|h, _| !orphans.contains(h));
        doc.dgn_ls_components.retain(|h, _| !orphans.contains(h));
        for o in doc.objects.values_mut() {
            let ObjectType::Dictionary(d) = o else { continue };
            if d.entries.iter().any(|(_, e)| orphans.contains(e)) {
                d.entries.retain(|(_, e)| !orphans.contains(e));
                let keys: HashSet<&str> = d.entries.iter().map(|(k, _)| k.as_str()).collect();
                d.hard_owner_entries.retain(|k| keys.contains(k.as_str()));
            }
        }
        orphans.len()
    }

    /// Leave out what acadrust can't write as this DWG version: other programs' custom entities
    /// and objects without raw DWG data of that version. acadrust 0.6.4 refuses the whole file
    /// for one of them (0.6.3 left them out); CADCraft already tells the user which entity types
    /// a DWG doesn't keep (`not_saved`), so the rest of the drawing is written. Returns how many.
    fn drop_unwritable(doc: &mut acadrust::CadDocument) -> usize {
        let version = doc.version;
        let unwritable: Vec<acadrust::Handle> = doc
            .entities()
            .filter_map(|e| match e {
                acadrust::entities::EntityType::Unknown(u)
                    if u.raw_dwg_data.is_none() || u.dwg_source_version.is_some_and(|v| v != version) || version == acadrust::DxfVersion::AC1009 =>
                {
                    Some(u.common.handle)
                }
                _ => None,
            })
            .collect();
        for h in &unwritable {
            doc.remove_entity(*h);
        }
        let objects: Vec<acadrust::Handle> = doc
            .objects
            .iter()
            .filter_map(|(h, o)| match o {
                acadrust::objects::ObjectType::Unknown { raw_dwg_data, raw_dwg_version, .. }
                    if raw_dwg_data.is_none() || raw_dwg_version.is_some_and(|v| v != version) || version == acadrust::DxfVersion::AC1009 =>
                {
                    Some(*h)
                }
                _ => None,
            })
            .collect();
        for h in &objects {
            doc.objects.remove(h);
        }
        unwritable.len() + objects.len()
    }

    /// Header variables the DWG reader fills in but acadrust's DXF writer (0.6.3) leaves out of
    /// the HEADER section: (name, group code, value).
    fn unwritten_header_vars(h: &acadrust::document::HeaderVariables) -> [(&'static str, i32, String); 7] {
        [
            ("$ANGBASE", 50, h.angle_base.to_string()),
            ("$ANGDIR", 70, h.angle_direction.to_string()),
            ("$FILLETRAD", 40, h.fillet_radius.to_string()),
            ("$CHAMFERA", 40, h.chamfer_distance_a.to_string()),
            ("$CHAMFERB", 40, h.chamfer_distance_b.to_string()),
            ("$ELEVATION", 40, h.elevation.to_string()),
            ("$THICKNESS", 40, h.thickness.to_string()),
        ]
    }

    /// Add the [`unwritten_header_vars`] to the HEADER section of acadrust's DXF text, before its
    /// ENDSEC; a variable the writer already wrote is left alone. Text of another layout than
    /// acadrust's (`  0\r\nSECTION\r\n  2\r\nHEADER\r\n` … `  0\r\nENDSEC\r\n`) is returned unchanged.
    fn add_header_vars(mut dxf: Vec<u8>, h: &acadrust::document::HeaderVariables) -> Vec<u8> {
        const START: &[u8] = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n";
        const END: &[u8] = b"\r\n  0\r\nENDSEC\r\n";
        if !dxf.starts_with(START) {
            return dxf;
        }
        let Some(end) = dxf.windows(END.len()).position(|w| w == END).map(|p| p + 2) else { return dxf };
        let section = dxf.get(..end).unwrap_or_default();
        let mut add = String::new();
        for (name, code, value) in unwritten_header_vars(h) {
            let written = section.windows(name.len() + 2).any(|w| w.starts_with(name.as_bytes()) && w.ends_with(b"\r\n"));
            if !written && value.parse::<f64>().is_ok_and(f64::is_finite) {
                add.push_str(&format!("  9\r\n{name}\r\n{code:>3}\r\n{value}\r\n"));
            }
        }
        dxf.splice(end..end, add.into_bytes());
        dxf
    }

    /// Collects the DXF text and fails the write once it would grow past `limit`.
    struct CappedWriter {
        buf: Vec<u8>,
        limit: usize,
        exceeded: bool,
    }

    impl Write for CappedWriter {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            // Sticky: once over, every later write fails too.
            if self.exceeded || self.buf.len().saturating_add(b.len()) > self.limit {
                self.exceeded = true;
                return Err(std::io::Error::other("DXF size limit reached"));
            }
            self.buf.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Oldest DWG version written; older DXF input is written as this version.
    const MIN_DWG_VERSION: acadrust::DxfVersion = acadrust::DxfVersion::AC1018;

    /// Convert DXF bytes into a DWG file of the DXF's version, and at least AutoCAD 2004
    /// (AC1018).
    pub fn dxf_to_dwg(dxf: &[u8]) -> Result<Vec<u8>, String> {
        convert(dxf, None)
    }

    /// Convert DXF bytes into a DWG file of `version`, a code from [`super::VERSIONS`] ("AC1032"),
    /// whatever version the DXF declares.
    pub fn dxf_to_dwg_as(dxf: &[u8], version: &str) -> Result<Vec<u8>, String> {
        let v = match version.trim().to_ascii_uppercase().as_str() {
            "AC1015" => acadrust::DxfVersion::AC1015,
            "AC1018" => acadrust::DxfVersion::AC1018,
            "AC1021" => acadrust::DxfVersion::AC1021,
            "AC1024" => acadrust::DxfVersion::AC1024,
            "AC1027" => acadrust::DxfVersion::AC1027,
            "AC1032" => acadrust::DxfVersion::AC1032,
            _ => return Err(super::unsupported(version)),
        };
        convert(dxf, Some(v))
    }

    fn convert(dxf: &[u8], version: Option<acadrust::DxfVersion>) -> Result<Vec<u8>, String> {
        let data = dxf.to_vec();
        let mut doc = std::panic::catch_unwind(move || acadrust::DxfReader::from_reader(Cursor::new(data)).and_then(|r| r.read()))
            .map_err(|_| "the DXF→DWG conversion failed".to_string())?
            .map_err(|e| format!("DXF: {e}"))?;
        match version {
            Some(v) => doc.version = v,
            // Without a version asked for, write at least AutoCAD 2004 (AC1018) DWG: R2000 DWG
            // has no true colours, transparency, table styles or gradients. The DXF stays R2000;
            // only the DWG is newer.
            None if doc.version < MIN_DWG_VERSION => doc.version = MIN_DWG_VERSION,
            None => {}
        }
        table_style_rows(&mut doc, true);
        drop_unwritable(&mut doc);
        std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DwgWriter::write_to_vec(&doc)))
            .map_err(|_| "the DWG writer failed on this drawing".to_string())?
            .map_err(|e| format!("DWG write: {e}"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{dwg_to_dxf, dwg_to_dxf_noted, dwg_to_dxf_with, dxf_to_dwg, dxf_to_dwg_as};

/// The error for a DWG version CADCraft can't write.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn unsupported(version: &str) -> String {
    let list: Vec<&str> = VERSIONS.iter().map(|(_, year)| *year).collect();
    format!("DWG version `{version}` can't be written (CADCraft writes {})", list.join(", "))
}

#[cfg(target_arch = "wasm32")]
pub fn dwg_to_dxf(_bytes: &[u8]) -> Result<Vec<u8>, String> {
    Err("DWG files can't be opened in the web build yet; save as DXF".into())
}
#[cfg(target_arch = "wasm32")]
pub fn dwg_to_dxf_noted(_bytes: &[u8], _limits: Limits) -> Result<(Vec<u8>, Vec<String>), String> {
    Err("DWG files can't be opened in the web build yet; save as DXF".into())
}
#[cfg(target_arch = "wasm32")]
pub fn dwg_to_dxf_with(_bytes: &[u8], _limits: Limits) -> Result<Vec<u8>, String> {
    Err("DWG files can't be opened in the web build yet; save as DXF".into())
}
#[cfg(target_arch = "wasm32")]
pub fn dxf_to_dwg(_dxf: &[u8]) -> Result<Vec<u8>, String> {
    Err("DWG files can't be written in the web build yet; save as DXF".into())
}
#[cfg(target_arch = "wasm32")]
pub fn dxf_to_dwg_as(_dxf: &[u8], _version: &str) -> Result<Vec<u8>, String> {
    Err("DWG files can't be written in the web build yet; save as DXF".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_magic() {
        assert!(is_dwg(b"AC1032\0\0\0\0"));
        assert!(!is_dwg(b"  0\r\nSECTION"));
        assert_eq!(version(b"AC1018xxxx").as_deref(), Some("AC1018"));
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(dwg_to_dxf(b"AC1032 this is not a dwg file at all").is_err());
        assert!(dwg_to_dxf(b"").is_err());
    }

    /// The handles a DXF text refers to or gives its records (groups 5, 105 and 330–369).
    #[cfg(not(target_arch = "wasm32"))]
    fn dxf_handles(dxf: &str) -> std::collections::HashSet<u64> {
        let lines: Vec<&str> = dxf.lines().collect();
        lines
            .chunks(2)
            .filter_map(|p| match p {
                [c, v] if matches!(c.trim().parse::<i32>(), Ok(5 | 105 | 330..=369)) => u64::from_str_radix(v.trim(), 16).ok(),
                _ => None,
            })
            .collect()
    }

    /// Issue #1482: a DWG that once came from MicroStation can carry hundreds of thousands of DGN
    /// line style components no definition uses (a real 83 MB site plan: 888,483 of 889,053, a
    /// 1.2 GB DXF rendition over the limit, so it didn't open). They're left out with a note, and
    /// their dictionary entries with them; the definition and every component it reaches —
    /// through compound and point components, around a cycle — are kept.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn orphaned_dgn_line_style_components_are_left_out() {
        use acadrust::objects::*;
        let mut doc = acadrust::CadDocument::new();
        let container = doc.allocate_handle();
        let nod = doc.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&nod) else { panic!("no named object dictionary") };
        root.entries.push(("ACAD_DGNLINESTYLECOMP".into(), container));
        let pattern = || DgnLsStrokePattern {
            has_iteration_limit: false,
            is_single_segment: false,
            iteration_limit: 0,
            auto_phase: 0.0,
            phase: 0.0,
            phase_mode: DgnLsPhaseMode::Distance,
            strokes: vec![DgnLsStroke {
                is_dash: true,
                bypass_corner: false,
                can_be_scaled: true,
                invert_at_origin: false,
                invert_at_end: false,
                length: 2.5,
                start_width: 0.0,
                end_width: 0.0,
                width_mode: 0,
                cap_mode: 0,
            }],
        };
        let kind = |c: &DgnLsComponentData| match c {
            DgnLsComponentData::Symbol(_) => DgnLsComponentType::Symbol,
            DgnLsComponentData::Compound(_) => DgnLsComponentType::Compound,
            DgnLsComponentData::Stroke(_) => DgnLsComponentType::Stroke,
            DgnLsComponentData::Point(_) => DgnLsComponentType::Point,
            DgnLsComponentData::Internal(_) => DgnLsComponentType::Internal,
        };
        let compound = |of: &[acadrust::Handle]| {
            DgnLsComponentData::Compound(DgnLsCompoundComponent {
                entries: of.iter().map(|&component| DgnLsCompoundEntry { component, offset: 0.0 }).collect(),
            })
        };
        let mut entries = Vec::new();
        let mut put = |doc: &mut acadrust::CadDocument, h: acadrust::Handle, data: DgnLineStyleData| {
            entries.push((format!("{:X}", h.value()), h));
            let reactors = vec![container];
            doc.objects
                .insert(h, ObjectType::DgnLineStyle(DgnLineStyleObject { handle: h, owner: container, reactors, xdictionary_handle: None, data }));
        };
        let component = |c: DgnLsComponentData| DgnLineStyleData::Component {
            kind: kind(&c),
            description: "dash".into(),
            version: 1,
            component_uid: [7; 16],
            scale: 1.0,
            property_flags: 0,
            component: c,
            properties: vec![],
        };
        let h: Vec<acadrust::Handle> = (0..9).map(|_| doc.allocate_handle()).collect();
        let [def, root, stroke, point, back, orphan, orphan_cycle, internal, shared] = h[..] else { panic!("nine handles") };
        put(
            &mut doc,
            def,
            DgnLineStyleData::Definition {
                description: "{ dash }".into(),
                version: 1,
                style_number: 1,
                component_uid: [1; 16],
                is_continuous: false,
                unit_definition: 1.0,
                unit_scale: 1.0,
                units_type: 0,
                is_element: false,
                is_physical: false,
                is_scale_independent: false,
                is_snappable: false,
                root_component: root,
                properties: vec![],
            },
        );
        // Used: def → root → {stroke, point → shared, back → root (a cycle)}.
        put(&mut doc, root, component(compound(&[stroke, point, back])));
        put(&mut doc, stroke, component(DgnLsComponentData::Stroke(pattern())));
        put(&mut doc, point, component(DgnLsComponentData::Point(DgnLsPointComponent { stroke_component: shared, symbols: vec![] })));
        put(&mut doc, back, component(compound(&[root])));
        put(&mut doc, shared, component(DgnLsComponentData::Stroke(pattern())));
        // Orphans: an internal component, and two compounds that refer to each other and to a
        // used component (which stays).
        let pattern_internal =
            DgnLsInternalComponent { pattern: pattern(), internal_version: 0, hardware_style: 0, is_hardware_style: false, line_code: 0 };
        put(&mut doc, internal, component(DgnLsComponentData::Internal(pattern_internal)));
        put(&mut doc, orphan, component(compound(&[orphan_cycle, shared])));
        put(&mut doc, orphan_cycle, component(compound(&[orphan])));
        let mut dict = Dictionary::new();
        dict.handle = container;
        dict.owner = nod;
        dict.entries = entries;
        doc.objects.insert(container, ObjectType::Dictionary(dict));
        let dwg = acadrust::DwgWriter::write_to_vec(&doc).unwrap();

        let (dxf, notes) = dwg_to_dxf_noted(&dwg, Limits::DEFAULT).unwrap();
        let dxf = String::from_utf8(dxf).unwrap();
        let handles = dxf_handles(&dxf);
        for kept in [def, root, stroke, point, back, shared] {
            assert!(handles.contains(&kept.value()), "{:X} was left out", kept.value());
        }
        for gone in [internal, orphan, orphan_cycle] {
            assert!(!handles.contains(&gone.value()), "{:X} was kept or is still referred to", gone.value());
        }
        let lines: Vec<&str> = dxf.lines().collect();
        let ls: Vec<&str> = lines.chunks(2).filter_map(|p| p.get(1).filter(|_| p[0].trim() == "0" && p[1].starts_with("LS")).copied()).collect();
        assert_eq!(ls.iter().filter(|t| t.ends_with("COMPONENT")).count(), 5, "{ls:?}");
        assert!(ls.contains(&"LSDEFINITION") && !ls.contains(&"LSINTERNALCOMPONENT"), "{ls:?}");
        assert_eq!(notes, ["DWG: 3 DGN line style components that no line style definition uses were left out"]);
    }
}
