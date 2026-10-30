//! DWG support through a DXF bridge.
//!
//! DWG files are read with the `acadrust` crate (MPL-2.0, used unmodified as a dependency) and
//! converted to DXF bytes, which CADCraft's own DXF reader maps to its document model; saving
//! goes the other way. Keeping the dependency behind this byte-level API isolates it: nothing
//! else in CADCraft depends on its types. Native targets only (it memory-maps files).
//!
//! # Guardrails around the third-party decoder
//!
//! The decoder allocates tables and section buffers from sizes a (possibly damaged or hostile)
//! file declares, and once it starts it cannot be interrupted (an allocation failure aborts the
//! process, which `catch_unwind` cannot recover). Because the dependency is used unmodified, this
//! crate guards it from the outside:
//!
//! - Both directions reject oversized input *before* the decoder runs ([`Limits::max_dwg_bytes`]
//!   for DWG→DXF, [`Limits::max_dxf_bytes`] for DXF→DWG), and the DWG→DXF output is capped while
//!   it is written (`CappedWriter`).
//! - The reader and writer run inside `catch_unwind`, so a decode failure is reported instead of
//!   crashing.
//!
//! These bound the input- and output-size variants. A small file that declares a huge internal
//! table or section can still make the dependency allocate before any size is known; fully fixing
//! that requires an upstream fix in `acadrust` (validate declared sizes against a decoded-memory
//! budget) or running the decode in a separate worker with an OS memory limit. The limits here
//! keep the blast radius bounded in the meantime.
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
        let orphans = guarded(AssertUnwindSafe(|| post_process(&mut doc))).map_err(|_| "the DWG post-processing failed on this file".to_string())?;
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
        table_merges_within(doc, MAX_MERGE_CELLS);
    }

    /// The most table cells [`table_merges`] visits in one document: each range is capped, but
    /// can cover rows × cols cells, so the total is bounded too.
    const MAX_MERGE_CELLS: usize = 1 << 24;

    /// The fix-ups made to a document after the DWG read, returning how many orphaned DGN
    /// components were dropped. The document comes from a third-party reader and is hostile.
    fn post_process(doc: &mut acadrust::CadDocument) -> usize {
        super::fail_in_test("post_process");
        current_style_names(doc);
        table_merges(doc);
        table_style_rows(doc, false);
        keep_sort_handles(doc);
        drop_orphaned_dgn_components(doc)
    }

    /// Runs `f`, turning a panic into an error (the reader and writer are guarded the same way).
    fn guarded<R>(f: impl FnOnce() -> R + std::panic::UnwindSafe) -> Result<R, ()> {
        std::panic::catch_unwind(f).map_err(|_| ())
    }

    /// [`table_merges`] visiting at most `budget` cells in all; a range that would pass it is
    /// left unmerged. Returns the cells visited.
    fn table_merges_within(doc: &mut acadrust::CadDocument, budget: usize) -> usize {
        let mut left = budget;
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
                let area = (bottom - r.top_row + 1).saturating_mul(right - r.left_col + 1);
                if area > left {
                    continue;
                }
                left -= area;
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
        budget - left
    }

    /// Draw-order tables (SORTENTSTABLE) keep the sort handles the file gave them. acadrust's DXF
    /// writer renumbers them past every entity handle, but an entity left out of a table sorts by
    /// its own handle, so after renumbering it would draw below every listed entity (#2410). Giving
    /// the table its codes verbatim makes the writer emit them unchanged.
    fn keep_sort_handles(doc: &mut acadrust::CadDocument) {
        use acadrust::objects::ObjectType;
        let version = doc.version;
        for o in doc.objects.values_mut() {
            let ObjectType::SortEntitiesTable(t) = o else { continue };
            let hex = |h: acadrust::types::Handle| format!("{:X}", h.value());
            let mut codes =
                vec![(5, hex(t.handle)), (330, hex(t.owner_handle)), (100, "AcDbSortentsTable".to_string()), (330, hex(t.block_owner_handle))];
            for e in t.entries() {
                codes.push((331, hex(e.entity_handle)));
                codes.push((5, hex(e.sort_handle)));
            }
            t.raw_dxf_codes = Some(codes);
            t.raw_dxf_version = Some(version);
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

    /// The DWG dimension flags byte. Its bit 1 is DXF group 70's bit 32 (ODA DWG specification,
    /// common dimension data), which acadrust keeps no copy of from DXF: it is set here as
    /// CADCraft's DXF writer always sets it (#1094). Bit 0, the opposite of group 70's bit 128,
    /// acadrust 0.6.5 reads and writes itself.
    fn dimension_flags(doc: &mut acadrust::CadDocument) {
        for e in doc.entities_mut() {
            let acadrust::entities::EntityType::Dimension(d) = e else { continue };
            d.base_mut().dwg_flags_byte |= 0x02;
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
            // DWG holds radians; `$ANGBASE` is in degrees.
            ("$ANGBASE", 50, h.angle_base.to_degrees().to_string()),
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
        convert(dxf, None, None)
    }

    /// Convert DXF bytes into a DWG file of `version`, a code from [`super::VERSIONS`] ("AC1032"),
    /// whatever version the DXF declares.
    pub fn dxf_to_dwg_as(dxf: &[u8], version: &str) -> Result<Vec<u8>, String> {
        convert(dxf, Some(writable(version)?), None)
    }

    /// Like [`dxf_to_dwg_as`], for a DXF that declares an older version than the groups it carries
    /// over from another file. CADCraft's R2000 DWG bridge holds the objects it keeps without
    /// modelling them (render settings, fields, associative and dynamic block data) as they were
    /// read from a file of version `kept` ("AC1032"), and acadrust reads several of those
    /// differently by version: read as R2000, an AC1032 file's mental ray render settings came out
    /// a class version too high, for which the ODA rejects the whole DWG, and a field lost its
    /// cached value. So the DXF is read as `kept` declares. A `kept` version that is unknown, or
    /// not newer than the DXF's own, changes nothing.
    pub fn dxf_to_dwg_kept_as(dxf: &[u8], version: &str, kept: &str) -> Result<Vec<u8>, String> {
        let kept = acadrust::DxfVersion::from_version_string(kept.trim().to_ascii_uppercase().as_str());
        let kept = (kept != acadrust::DxfVersion::Unknown).then_some(kept);
        convert(dxf, Some(writable(version)?), kept)
    }

    /// The DWG version for a code from [`super::VERSIONS`].
    fn writable(version: &str) -> Result<acadrust::DxfVersion, String> {
        Ok(match version.trim().to_ascii_uppercase().as_str() {
            "AC1015" => acadrust::DxfVersion::AC1015,
            "AC1018" => acadrust::DxfVersion::AC1018,
            "AC1021" => acadrust::DxfVersion::AC1021,
            "AC1024" => acadrust::DxfVersion::AC1024,
            "AC1027" => acadrust::DxfVersion::AC1027,
            "AC1032" => acadrust::DxfVersion::AC1032,
            _ => return Err(super::unsupported(version)),
        })
    }

    /// The ASCII DXF with its `$ACADVER` set to `kept` when that is newer than the version it
    /// declares, or `None` to read it as it is (binary, no `$ACADVER`, or non-ASCII text without a
    /// `$DWGCODEPAGE` to decode it by). From AutoCAD 2007 (AC1021) a DXF is UTF-8, so other bytes
    /// are first decoded with the code page the DXF declares; `\U+XXXX` escapes read the same in
    /// every version, and CADCraft's R2000 DXF is ASCII with such escapes.
    pub(super) fn declare_version(dxf: &[u8], kept: acadrust::DxfVersion) -> Option<Vec<u8>> {
        if dxf.starts_with(b"AutoCAD Binary DXF") {
            return None;
        }
        let (_, declared) = header_value(dxf, b"$ACADVER")?;
        let declared = acadrust::DxfVersion::from_version_string(std::str::from_utf8(declared).ok()?);
        if declared >= kept {
            return None;
        }
        let text: std::borrow::Cow<'_, [u8]> = if dxf.is_ascii() || kept < acadrust::DxfVersion::AC1021 {
            std::borrow::Cow::Borrowed(dxf)
        } else {
            // As acadrust decodes an older DXF: by its code page, Windows-1252 for one it doesn't
            // know; a code page that names UTF-8 needs no decoding.
            let (_, code_page) = header_value(dxf, b"$DWGCODEPAGE")?;
            match acadrust::io::dxf::code_page::encoding_from_code_page(std::str::from_utf8(code_page).ok()?) {
                Some(encoding) => {
                    let (text, errors) = encoding.decode_without_bom_handling(dxf);
                    if errors {
                        return None;
                    }
                    std::borrow::Cow::Owned(text.into_owned().into_bytes())
                }
                None => std::borrow::Cow::Borrowed(std::str::from_utf8(dxf).ok()?.as_bytes()),
            }
        };
        let (at, value) = header_value(&text, b"$ACADVER")?;
        let code = kept.to_string();
        let mut out = Vec::with_capacity(text.len().saturating_add(code.len()));
        out.extend_from_slice(text.get(..at)?);
        out.extend_from_slice(code.as_bytes());
        out.extend_from_slice(text.get(at.checked_add(value.len())?..)?);
        Some(out)
    }

    /// The byte offset and the trimmed value of header variable `name` in an ASCII DXF: the value
    /// of the group that follows the `9` group naming it. The file is read as (group code,
    /// value) line pairs, in one pass with no buffering, up to the first `0`/`ENDSEC`, the end
    /// of the HEADER section: a large input without one costs time, not memory (a line index of
    /// a 32 MiB file used 0.8 GB). Lines may end in `\r\n`.
    fn header_value<'a>(dxf: &'a [u8], name: &[u8]) -> Option<(usize, &'a [u8])> {
        let mut offset = 0usize;
        let mut lines = dxf.split(|b| *b == b'\n').map(|line| {
            let at = offset.saturating_add(line.len() - line.trim_ascii_start().len());
            offset = offset.saturating_add(line.len()).saturating_add(1);
            (at, line.trim_ascii())
        });
        while let (Some((_, code)), Some((_, value))) = (lines.next(), lines.next()) {
            if code == b"0" && value == b"ENDSEC" {
                return None;
            }
            if code == b"9" && value == name {
                let (_, code) = lines.next()?;
                let (at, value) = lines.next()?;
                return (code != b"0" && code != b"9").then_some((at, value));
            }
        }
        None
    }

    /// The current dimension and text styles' names from their handles: a DWG stores the
    /// handles, and the reader leaves the names at "Standard", so a drawing reopened from DWG
    /// lost its current dimension style (#1051).
    fn current_style_names(doc: &mut acadrust::CadDocument) {
        let h = doc.header.current_dimstyle_handle;
        if let Some(n) = doc.dim_styles.iter().find(|ds| ds.handle == h && !h.is_null()).map(|ds| ds.name.clone()) {
            doc.header.current_dimstyle_name = n;
        }
        let h = doc.header.current_text_style_handle;
        if let Some(n) = doc.text_styles.iter().find(|ts| ts.handle == h && !h.is_null()).map(|ts| ts.name.clone()) {
            doc.header.current_text_style_name = n;
        }
    }

    /// The current dimension style (`$DIMSTYLE`) by its name: the DXF gives the name, and the
    /// handle read with it needn't be the style's in the document, in which case the DWG writer
    /// fell back to Standard and AutoCAD opened the drawing with Standard current (#1051).
    fn current_dimstyle(doc: &mut acadrust::CadDocument) {
        let name = doc.header.current_dimstyle_name.clone();
        if let Some(h) = doc.dim_styles.iter().find(|ds| ds.name.eq_ignore_ascii_case(&name)).map(|ds| ds.handle) {
            doc.header.current_dimstyle_handle = h;
        }
    }

    /// The current text style (`$TEXTSTYLE`) by its name, the same way as [`current_dimstyle`]:
    /// the handle read with the DXF needn't be the style's in the document, in which case the DWG
    /// writer fell back to Standard and AutoCAD opened the drawing with Standard current (#1650).
    fn current_text_style(doc: &mut acadrust::CadDocument) {
        let name = doc.header.current_text_style_name.clone();
        if let Some(h) = doc.text_styles.iter().find(|ts| ts.name.eq_ignore_ascii_case(&name)).map(|ts| ts.handle) {
            doc.header.current_text_style_handle = h;
        }
    }

    /// The DXF names of an ASCII DXF's CLASS records, in CLASSES order: a proxy's class number
    /// (group 91) is 500 plus its class's place here (DXF Reference, ACAD_PROXY_OBJECT). Empty for
    /// one without a CLASSES section. `None` for a binary DXF, whose classes this doesn't read:
    /// CADCraft's own bridge DXF is always ASCII, and only a binary DXF given to the public
    /// conversion functions comes here.
    pub(super) fn dxf_class_names(dxf: &[u8]) -> Option<Vec<String>> {
        let mut names = Vec::new();
        if dxf.starts_with(b"AutoCAD Binary DXF") {
            return None;
        }
        let mut lines = dxf.split(|b| *b == b'\n').map(|l| l.trim_ascii());
        let (mut in_classes, mut in_class) = (false, false);
        while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
            match (code, value) {
                (b"0", b"ENDSEC") if in_classes => break,
                (b"2", b"CLASSES") => in_classes = true,
                (b"0", v) if in_classes => in_class = v == b"CLASS",
                (b"1", v) if in_class => {
                    names.push(String::from_utf8_lossy(v).into_owned());
                    in_class = false;
                }
                _ => {}
            }
        }
        Some(names)
    }

    /// Proxy objects' and entities' class numbers as the DWG numbers its classes. A DXF proxy
    /// names its class by its place in the DXF's CLASSES (`classes`, from 500), but acadrust's
    /// reader puts its own default classes first and numbers the DXF's after them, so the DXF
    /// number named another class in the DWG: a 2018 drawing's piping proxies became
    /// LARGE_RADIAL_DIMENSION and SECTIONOBJECT, and the ODA File Converter refused the file. A
    /// number that names no class of the DXF is left as it is.
    ///
    /// Before 2013 (AC1027) acadrust writes only the classes it knows a pre-2013 DWG needs, and a
    /// proxy's class is not one of them: the proxy would name a missing class (the ODA refuses
    /// the file: "No ClassId") or, by its old number, some other class. Such a DWG leaves the
    /// proxies out instead, as it does other programs' data it can't write (`not_saved` reports
    /// the entities). So does a DXF whose classes are unknown (`classes` is `None`, a binary
    /// DXF): its proxies' numbers can't be mapped, and kept as they are they would name the
    /// wrong classes and the ODA would refuse the file. Returns how many proxies were left out.
    fn proxy_classes(doc: &mut acadrust::CadDocument, classes: Option<&[String]>) -> usize {
        let classes = match classes {
            Some(c) if doc.version >= acadrust::DxfVersion::AC1027 => c,
            _ => {
                let entities: Vec<acadrust::Handle> = doc
                    .entities()
                    .filter_map(|e| match e {
                        acadrust::entities::EntityType::Extended(x) if matches!(x.data, acadrust::entities::ExtendedEntityData::Proxy(_)) => {
                            Some(x.common.handle)
                        }
                        _ => None,
                    })
                    .collect();
                let objects: Vec<acadrust::Handle> =
                    doc.objects.iter().filter_map(|(h, o)| matches!(o, acadrust::objects::ObjectType::ProxyObject(_)).then_some(*h)).collect();
                for h in &entities {
                    doc.remove_entity(*h);
                }
                for h in &objects {
                    doc.objects.remove(h);
                }
                return entities.len() + objects.len();
            }
        };
        let number = |id: i32| -> Option<i32> {
            let name = classes.get(usize::try_from(id.checked_sub(500)?).ok()?)?;
            doc.classes.get_by_name(name).map(|c| i32::from(c.class_number))
        };
        let objects: Vec<(acadrust::Handle, i32)> = doc
            .objects
            .iter()
            .filter_map(|(h, o)| match o {
                acadrust::objects::ObjectType::ProxyObject(p) => number(p.class_id).map(|n| (*h, n)),
                _ => None,
            })
            .collect();
        let entities: Vec<(acadrust::Handle, i32)> = doc
            .entities()
            .filter_map(|e| match e {
                acadrust::entities::EntityType::Extended(x) => match &x.data {
                    acadrust::entities::ExtendedEntityData::Proxy(p) => number(p.class_id).map(|n| (x.common.handle, n)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        for (h, n) in objects {
            if let Some(acadrust::objects::ObjectType::ProxyObject(p)) = doc.objects.get_mut(&h) {
                p.class_id = n;
            }
        }
        let entities: std::collections::HashMap<acadrust::Handle, i32> = entities.into_iter().collect();
        for e in doc.entities_mut() {
            if let acadrust::entities::EntityType::Extended(x) = e {
                let Some(n) = entities.get(&x.common.handle).copied() else { continue };
                if let acadrust::entities::ExtendedEntityData::Proxy(p) = &mut x.data {
                    p.class_id = n;
                }
            }
        }
        0
    }

    /// The current layer (`$CLAYER`) by its name, the same way as [`current_dimstyle`]: a DWG
    /// stores the handle of the layer, and the one read with the DXF needn't be the layer's in
    /// the document, so the drawing reopened with layer "0" current (#2252).
    fn current_layer(doc: &mut acadrust::CadDocument) {
        let name = doc.header.current_layer_name.clone();
        // The table's own lookup compares Unicode case-insensitively ("Survey_Ä" = "Survey_ä").
        if let Some(layer) = doc.layers.get(&name) {
            doc.header.current_layer_handle = layer.handle;
        }
    }

    /// `kept`: the version the DXF's carried-over groups were read from ([`dxf_to_dwg_kept_as`]).
    fn convert(dxf: &[u8], version: Option<acadrust::DxfVersion>, kept: Option<acadrust::DxfVersion>) -> Result<Vec<u8>, String> {
        // Reject a pathologically large DXF before handing it to the third-party decoder, which
        // allocates tables and section buffers from the input's declared sizes and cannot be
        // interrupted once it starts (CWE-789). This bounds the large-input variant; the decoder
        // runs inside `catch_unwind` so a decode failure is reported rather than crashing.
        if dxf.len() > Limits::DEFAULT.max_dxf_bytes {
            return Err(format!(
                "DXF: the input is {}, larger than the {} CADCraft converts to DWG",
                mb(dxf.len() as u64),
                mb(Limits::DEFAULT.max_dxf_bytes as u64)
            ));
        }
        // Decoding a code page to UTF-8 can grow the text, so the limit applies again.
        let data = match kept.and_then(|k| declare_version(dxf, k)) {
            Some(d) if d.len() > Limits::DEFAULT.max_dxf_bytes => {
                return Err(format!(
                    "DXF: the input is {} as UTF-8, larger than the {} CADCraft converts to DWG",
                    mb(d.len() as u64),
                    mb(Limits::DEFAULT.max_dxf_bytes as u64)
                ));
            }
            Some(d) => d,
            None => dxf.to_vec(),
        };
        let classes = dxf_class_names(&data);
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
        // The fix-ups run on the third-party reader's document, so a panic in one is an error, not a crash.
        guarded(AssertUnwindSafe(|| {
            super::fail_in_test("convert");
            proxy_classes(&mut doc, classes.as_deref());
            other_layout_owners(&mut doc);
            table_style_rows(&mut doc, true);
            dimension_flags(&mut doc);
            drop_unwritable(&mut doc);
            current_dimstyle(&mut doc);
            add_variable_dictionary(&mut doc);
            current_text_style(&mut doc);
            current_layer(&mut doc);
        }))
        .map_err(|_| "the DXF to DWG conversion failed on this file".to_string())?;
        std::panic::catch_unwind(AssertUnwindSafe(|| acadrust::DwgWriter::write_to_vec(&doc)))
            .map_err(|_| "the DWG writer failed on this drawing".to_string())?
            .map_err(|e| format!("DWG write: {e}"))
    }

    /// AutoCAD's entry name for the drawing variables dictionary (the DXF Reference calls it the
    /// ACDBVARIABLEDICTIONARY; dictionary keys are case-insensitive).
    const VARIABLE_DICTIONARY: &str = "AcDbVariableDictionary";

    /// A style or scale name, or `default` when the drawing has none.
    fn name_or_default(name: &str, default: &str) -> String {
        let name = name.trim();
        if name.is_empty() { default.to_string() } else { name.to_string() }
    }

    /// A drawing variable's value as text, or `default` when the header holds a value the
    /// variable doesn't accept (a hostile file can put any integer in it).
    fn variable_value(value: i16, valid: &[i16], default: i16) -> String {
        if valid.contains(&value) { value.to_string() } else { default.to_string() }
    }

    /// Add the drawing variables dictionary AutoCAD always writes into the named object
    /// dictionary, with the variables it keeps there, when the drawing has none (#1813).
    /// Vectric Aspire 10.5 reads a 2004 (AC1018) DWG without it as empty ("No data was imported
    /// from the DXF / DWG file"), though it reads the same drawing as DXF — which has no such
    /// dictionary either, as CADCraft's and ezdxf's writers don't make one. AutoCAD's AC1032 DWGs
    /// read there without it, so newer output is left as it was, and a dictionary the drawing
    /// does have keeps its own entries and values.
    fn add_variable_dictionary(doc: &mut acadrust::CadDocument) {
        use acadrust::objects::{Dictionary, DictionaryVariable, ObjectType};
        if doc.version >= acadrust::DxfVersion::AC1032 {
            return;
        }
        let root_handle = doc.header.named_objects_dict_handle;
        match doc.objects.get(&root_handle) {
            Some(ObjectType::Dictionary(root)) => {
                if root.get(VARIABLE_DICTIONARY).is_some() {
                    return;
                }
            }
            _ => return,
        }
        // The variables AutoCAD writes for a drawing with default styles and a 1:1 annotation
        // scale. DIMASSOC and HIDETEXT keep the drawing's own header values; LAYEREVAL and
        // LAYERNOTIFY have no DXF header variable.
        let variables = [
            ("CMLEADERSTYLE", name_or_default(&doc.header.current_mleader_style_name, "Standard")),
            ("CTABLESTYLE", name_or_default(&doc.header.current_table_style_name, "Standard")),
            ("CANNOSCALE", name_or_default(&doc.header.current_annotation_scale, "1:1")),
            ("DIMASSOC", variable_value(doc.header.dimension_associativity, &[0, 1, 2], 2)),
            ("HIDETEXT", variable_value(doc.header.hide_text, &[0, 1], 1)),
            ("LAYEREVAL", "0".to_string()),
            ("LAYERNOTIFY", "0".to_string()),
        ];
        let dictionary_handle = doc.allocate_handle();
        let mut dictionary = Dictionary::new();
        dictionary.handle = dictionary_handle;
        dictionary.owner = root_handle;
        for (name, value) in variables {
            let handle = doc.allocate_handle();
            let mut variable = DictionaryVariable::new(name, &value);
            variable.handle = handle;
            variable.owner_handle = dictionary_handle;
            dictionary.add_entry(name, handle);
            doc.objects.insert(handle, ObjectType::DictionaryVariable(variable));
        }
        doc.objects.insert(dictionary_handle, ObjectType::Dictionary(dictionary));
        if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_handle) {
            root.add_entry(VARIABLE_DICTIONARY, dictionary_handle);
        }
    }

    /// Entities of a layout other than the one in `*Paper_Space` are written owned by their block
    /// (entity mode 0, with its handle). acadrust writes every paper-space entity with mode 1,
    /// "paper space", which AutoCAD reads as the `*Paper_Space` block's: everything drawn on a
    /// second layout landed on the first one, and the second stayed empty (#1057).
    fn other_layout_owners(doc: &mut acadrust::CadDocument) {
        let others: std::collections::HashSet<acadrust::Handle> = doc
            .block_records
            .iter()
            .filter(|br| {
                let n = br.name.to_ascii_uppercase();
                n.len() > 12 && n.starts_with("*PAPER_SPACE") && n[12..].bytes().all(|b| b.is_ascii_digit())
            })
            .map(|br| br.handle)
            .collect();
        if others.is_empty() {
            return;
        }
        for e in doc.entities_mut() {
            let c = e.common_mut();
            if others.contains(&c.owner_handle) {
                c.entity_mode = Some(0);
            }
        }
    }

    #[cfg(test)]
    mod post_tests {
        use super::*;
        use acadrust::entities::{CellRange, EntityType, Table};

        fn doc_with_table(rows: usize, cols: usize, ranges: usize) -> acadrust::CadDocument {
            let mut t = Table::new(acadrust::types::Vector3::new(0.0, 0.0, 0.0), rows, cols);
            t.merged_ranges = (0..ranges).map(|_| CellRange::new(0, 0, rows - 1, cols - 1)).collect();
            let mut doc = acadrust::CadDocument::new();
            assert!(doc.add_entity(EntityType::Table(Box::new(t))).is_ok());
            doc
        }

        /// Overlapping whole-table ranges can't make the merge pass visit more than its budget (#1934).
        #[test]
        fn merge_work_is_bounded_by_the_cell_budget() {
            let mut doc = doc_with_table(10, 10, 50);
            assert_eq!(table_merges_within(&mut doc, 250), 200, "two whole-table ranges fit, the rest are skipped");
            let mut doc = doc_with_table(10, 10, 1);
            assert_eq!(table_merges_within(&mut doc, usize::MAX), 100);
        }

        /// A panic in the post-read fix-ups is an error, not a crash (#1934).
        #[test]
        fn a_panic_in_post_processing_is_an_error() {
            #[allow(clippy::panic)]
            let r = guarded(AssertUnwindSafe(|| -> usize { panic!("boom") }));
            assert!(r.is_err());
            assert_eq!(guarded(|| 3), Ok(3));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{dwg_to_dxf, dwg_to_dxf_noted, dwg_to_dxf_with, dxf_to_dwg, dxf_to_dwg_as, dxf_to_dwg_kept_as};

// Test-only: the stage ("post_process" or "convert") whose guard the current test checks. The
// stage then panics on entry, so a test through the public API shows whether it is guarded.
#[cfg(all(test, not(target_arch = "wasm32")))]
thread_local! {
    static PANIC_IN: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Panics when a test asked for `stage` to fail (see [`PANIC_IN`]); does nothing otherwise.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::panic)]
fn fail_in_test(_stage: &str) {
    #[cfg(test)]
    if PANIC_IN.with(|p| p.get()) == Some(_stage) {
        panic!("test-forced panic in {_stage}");
    }
}

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
#[cfg(target_arch = "wasm32")]
pub fn dxf_to_dwg_kept_as(_dxf: &[u8], _version: &str, _kept: &str) -> Result<Vec<u8>, String> {
    Err("DWG files can't be written in the web build yet; save as DXF".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DXF of an empty drawing, enough for the bridge to make a small DWG.
    #[cfg(not(target_arch = "wasm32"))]
    const MINIMAL_DXF: &str = "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1032\n  0\nENDSEC\n  0\nEOF\n";

    /// Makes `stage` panic on the current thread until dropped.
    #[cfg(not(target_arch = "wasm32"))]
    struct PanicIn;

    #[cfg(not(target_arch = "wasm32"))]
    impl PanicIn {
        fn at(stage: &'static str) -> Self {
            PANIC_IN.with(|p| p.set(Some(stage)));
            Self
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl Drop for PanicIn {
        fn drop(&mut self) {
            PANIC_IN.with(|p| p.set(None));
        }
    }

    /// The public read reports a panic in the post-read fix-ups as an error: this fails if the
    /// `guarded` call around `post_process` is taken out (#2402).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reading_a_dwg_survives_a_panic_in_post_processing() {
        let dwg = dxf_to_dwg(MINIMAL_DXF.as_bytes()).unwrap();
        assert!(dwg_to_dxf(&dwg).is_ok(), "the DWG reads without the forced panic");
        let _panic = PanicIn::at("post_process");
        let err = dwg_to_dxf(&dwg).unwrap_err();
        assert!(err.contains("the DWG post-processing failed on this file"), "{err}");
    }

    /// The DXF to DWG conversion reports a panic in its fix-ups as an error, not a crash (#2402).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn converting_to_dwg_survives_a_panic_in_the_fix_ups() {
        let _panic = PanicIn::at("convert");
        let err = dxf_to_dwg(MINIMAL_DXF.as_bytes()).unwrap_err();
        assert!(err.contains("the DXF to DWG conversion failed on this file"), "{err}");
    }

    #[test]
    fn detects_magic() {
        assert!(is_dwg(b"AC1032\0\0\0\0"));
        assert!(!is_dwg(b"  0\r\nSECTION"));
        assert_eq!(version(b"AC1018xxxx").as_deref(), Some("AC1018"));
    }

    /// A DXF with ROMANS as `$TEXTSTYLE` gives a DWG whose current text style is ROMANS, not
    /// Standard: the DXF names the style, and the handle read with it was Standard's (#1650).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn current_text_style_reaches_the_dwg() {
        let header = "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1032\n  9\n$TEXTSTYLE\n  7\nROMANS\n  0\nENDSEC\n";
        let style = "  0\nSTYLE\n  5\n1F0\n100\nAcDbSymbolTableRecord\n100\nAcDbTextStyleTableRecord\n  2\nROMANS\n 70\n0\n";
        let style = format!("{style} 40\n0\n 41\n1\n 50\n0\n 71\n0\n 42\n2.5\n  3\nromans.shx\n  4\n\n");
        let dxf = format!("{header}  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nSTYLE\n 70\n1\n{style}  0\nENDTAB\n  0\nENDSEC\n  0\nEOF\n");
        let dwg = dxf_to_dwg(dxf.as_bytes()).unwrap();
        let doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(&dwg)).read().unwrap();
        let romans = doc.text_styles.iter().find(|ts| ts.name == "ROMANS").map(|ts| ts.handle);
        assert!(romans.is_some(), "ROMANS is missing from the DWG");
        assert_eq!(Some(doc.header.current_text_style_handle), romans);
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(dwg_to_dxf(b"AC1032 this is not a dwg file at all").is_err());
        assert!(dwg_to_dxf(b"").is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dxf_to_dwg_rejects_garbage_without_crashing() {
        // The DXF→DWG path runs the third-party reader inside catch_unwind and reports failures
        // rather than crashing (and rejects oversized input before decoding).
        assert!(dxf_to_dwg(b"not a dxf file").is_err());
        assert!(dxf_to_dwg(b"").is_err());
    }

    /// A DXF with one DIMENSION per (text, group 70, subclass, points) row.
    #[cfg(not(target_arch = "wasm32"))]
    fn dims_dxf(dims: &[(&str, i64, &str, &[(i32, [f64; 2])])]) -> String {
        let mut s = String::from("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1032\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n");
        for (text, flags, sub, pts) in dims {
            s += "  0\nDIMENSION\n  8\n0\n100\nAcDbEntity\n100\nAcDbDimension\n 10\n0\n 20\n0\n 30\n0\n";
            s += &format!(" 11\n50\n 21\n-20\n 31\n0\n 70\n{flags}\n  1\n{text}\n  3\nStandard\n100\n{sub}\n");
            for (code, [x, y]) in pts.iter() {
                s += &format!("{code:>3}\n{x}\n{:>3}\n{y}\n{:>3}\n0\n", code + 10, code + 20);
            }
        }
        s + "  0\nENDSEC\n  0\nEOF\n"
    }

    /// DWG stores DXF group 70's bit 128 inverted, in bit 0 of the dimension flags byte (set:
    /// the text is at its default position), and bit 32 in bit 1 (ODA DWG specification, common
    /// dimension data). The bytes are read back raw, so a reader that inverts bit 0 the same way
    /// as the writer can't hide a wrong byte.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dimension_text_position_flags_reach_the_dwg_as_autocad_reads_them() {
        let linear: &[(i32, [f64; 2])] = &[(13, [0.0, 0.0]), (14, [100.0, 0.0])];
        let radius: &[(i32, [f64; 2])] = &[(15, [10.0, 0.0])];
        let angular: &[(i32, [f64; 2])] = &[(13, [0.0, 0.0]), (14, [10.0, 0.0]), (15, [0.0, 0.0]), (16, [0.0, 10.0])];
        let kinds = [
            (0, "AcDbAlignedDimension", linear),
            (1, "AcDbAlignedDimension", linear),
            (4, "AcDbRadialDimension", radius),
            (2, "AcDb2LineAngularDimension", angular),
        ];
        let mut dims = Vec::new();
        for (ty, sub, pts) in kinds {
            dims.push(("HOME", ty | 32, sub, pts));
            dims.push(("MOVED", ty | 32 | 128, sub, pts));
        }
        let dwg = dxf_to_dwg(dims_dxf(&dims).as_bytes()).unwrap();
        let doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(&dwg)).read().unwrap();
        let mut seen = Vec::new();
        for e in doc.entities() {
            let acadrust::entities::EntityType::Dimension(d) = e else { continue };
            let (text, byte) = (d.base().text_override().unwrap_or_default(), d.base().dwg_flags_byte);
            seen.push((text.to_string(), byte & 0x03));
            assert_eq!(byte & 0x02, 0x02, "{text} {:?}: bit 32 of group 70 is lost (flags byte {byte:#04x})", d.base().dimension_type);
            let default_pos = text == "HOME";
            assert_eq!(byte & 0x01 != 0, default_pos, "{text} {:?}: flags byte {byte:#04x}", d.base().dimension_type);
        }
        for text in ["HOME", "MOVED"] {
            assert_eq!(seen.iter().filter(|(t, _)| t == text).count(), 4, "{seen:?}");
        }

        // And CADCraft reads its own DWG back as it wrote it: 128 only on the moved texts.
        let dxf = String::from_utf8(dwg_to_dxf(&dwg).unwrap()).unwrap();
        let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
        let mut flags = Vec::new();
        for rec in lines.chunks(2).collect::<Vec<_>>().split(|p| p.first() == Some(&"0")) {
            let value = |code: &str| rec.iter().find(|p| p.first() == Some(&code)).and_then(|p| p.get(1).copied());
            if let (Some(text @ ("HOME" | "MOVED")), Some(v)) = (value("1"), value("70")) {
                flags.push((text, v.parse::<i64>().unwrap_or(-1) & 128));
            }
        }
        assert_eq!(flags.len(), 8, "{flags:?}");
        assert!(flags.iter().all(|(l, f)| (*f == 128) == (*l == "MOVED")), "{flags:?}");
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

    /// An R2000 DXF header (`\r\n` lines, as CADCraft writes them) with `extra` after `$ACADVER`.
    #[cfg(not(target_arch = "wasm32"))]
    fn r2000(extra: &[u8]) -> Vec<u8> {
        let mut dxf = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\nAC1015\r\n  9\r\n$DWGCODEPAGE\r\n  3\r\nANSI_1252\r\n".to_vec();
        dxf.extend_from_slice(b"  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nTEXT\r\n  1\r\n");
        dxf.extend_from_slice(extra);
        dxf.extend_from_slice(b"\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n");
        dxf
    }

    /// The kept version replaces only the `$ACADVER` value; everything else stays byte for byte.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declare_version_changes_only_acadver() {
        let dxf = r2000(b"A\\U+00E7\\U+00E3o");
        let out = native::declare_version(&dxf, acadrust::DxfVersion::AC1032).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), String::from_utf8(dxf).unwrap().replacen("AC1015", "AC1032", 1));
    }

    /// A kept version not newer than the declared one, a binary DXF and a DXF without `$ACADVER`
    /// are read as they are.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declare_version_leaves_what_it_need_not_change() {
        let dxf = r2000(b"x");
        assert_eq!(native::declare_version(&dxf, acadrust::DxfVersion::AC1015), None);
        assert_eq!(native::declare_version(&dxf, acadrust::DxfVersion::AC1009), None);
        assert_eq!(native::declare_version(b"AutoCAD Binary DXF\r\n\x1a\0", acadrust::DxfVersion::AC1032), None);
        let no_version = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
        assert_eq!(native::declare_version(no_version, acadrust::DxfVersion::AC1032), None);
        // A `$ACADVER` outside the HEADER section is not the file's version.
        let late = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  0\r\nENDSEC\r\n  9\r\n$ACADVER\r\n  1\r\nAC1015\r\n";
        assert_eq!(native::declare_version(late, acadrust::DxfVersion::AC1032), None);
    }

    /// The header is read as group code and value pairs up to the first `0`/`ENDSEC`: a `$ACADVER`
    /// name whose value would come after it, or a file that never ends its HEADER, is no version
    /// to replace, and `\r\n` lines keep the value's offset right.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declare_version_reads_the_header_in_pairs_up_to_endsec() {
        let cut = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  0\r\nENDSEC\r\n  1\r\nAC1015\r\n";
        assert_eq!(native::declare_version(cut, acadrust::DxfVersion::AC1032), None);
        let endless = vec![b'\n'; 1 << 20];
        assert_eq!(native::declare_version(&endless, acadrust::DxfVersion::AC1032), None);
        let crlf = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\n  AC1015\r\n  0\r\nENDSEC\r\n";
        let out = native::declare_version(crlf, acadrust::DxfVersion::AC1032).unwrap();
        assert_eq!(out, b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\n  AC1032\r\n  0\r\nENDSEC\r\n");
    }

    /// Declared as 2007 or later, a DXF is UTF-8: code page text is decoded first. Declared as an
    /// older version, the code page bytes stay as they are.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declare_version_decodes_a_code_page_for_utf8_versions() {
        let dxf = r2000(b"A\xe7\xe3o");
        let utf8 = native::declare_version(&dxf, acadrust::DxfVersion::AC1032).unwrap();
        let text = String::from_utf8(utf8).unwrap();
        assert!(text.contains("\r\nAção\r\n") && text.contains("$ACADVER\r\n  1\r\nAC1032\r\n"), "{text}");
        let legacy = native::declare_version(&dxf, acadrust::DxfVersion::AC1018).unwrap();
        assert!(legacy.windows(4).any(|w| w == b"A\xe7\xe3o"), "the code page bytes stay");
        assert!(legacy.windows(6).any(|w| w == b"AC1018"));
        // A code page it doesn't know is read as Windows-1252, as acadrust reads an older DXF.
        let at = dxf.windows(9).position(|w| w == b"ANSI_1252").unwrap();
        let unknown = [&dxf[..at], b"NO_SUCH_CP".as_slice(), &dxf[at + 9..]].concat();
        let text = String::from_utf8(native::declare_version(&unknown, acadrust::DxfVersion::AC1032).unwrap()).unwrap();
        assert!(text.contains("\r\nAção\r\n"), "{text}");
        // Without a `$DWGCODEPAGE` there is nothing to decode it by: it is read as it is.
        let none = [&dxf[..at - 21], &dxf[at + 11..]].concat();
        assert!(!none.windows(12).any(|w| w == b"$DWGCODEPAGE"), "{}", String::from_utf8_lossy(&none));
        assert_eq!(native::declare_version(&none, acadrust::DxfVersion::AC1032), None);
    }

    /// An unknown kept version converts as [`dxf_to_dwg_as`] does.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn unknown_kept_version_converts_as_before() {
        let dxf = r2000(b"x");
        assert_eq!(dxf_to_dwg_kept_as(&dxf, "AC1032", "AC9999"), dxf_to_dwg_as(&dxf, "AC1032"));
        assert!(dxf_to_dwg_kept_as(&dxf, "AC0000", "AC1032").is_err(), "an unwritable version is still an error");
    }

    /// The CLASS names in CLASSES order; nothing for a binary DXF or one without CLASSES.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dxf_class_names_in_classes_order() {
        let dxf = b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nCLASSES\r\n  0\r\nCLASS\r\n  1\r\nWIDGETA\r\n  2\r\nAcDbWidgetA\r\n  3\r\nApp\r\n  0\r\nCLASS\r\n  1\r\nWIDGETB\r\n  2\r\nAcDbWidgetB\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nTEXT\r\n  1\r\nNOT_A_CLASS\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
        assert_eq!(native::dxf_class_names(dxf).unwrap(), ["WIDGETA", "WIDGETB"]);
        assert_eq!(native::dxf_class_names(b"  0\nSECTION\n  2\nHEADER\n  0\nENDSEC\n  0\nEOF\n"), Some(Vec::new()));
        assert_eq!(native::dxf_class_names(b"AutoCAD Binary DXF\r\n\x1a\0"), None);
    }
}
