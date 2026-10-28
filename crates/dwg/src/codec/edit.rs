//! Changing objects inside a DWG file without reading them into a drawing: an object's persistent
//! reactors ([`with_reactors`]), and an R2004-family file written again around changed objects
//! ([`replace_objects`]): every other section, object and byte the file holds stays as it was.
//!
//! The object layout is the ODA DWG specification's (§20.4 common entity and non-entity object
//! data): the reactor count sits in the data stream after the extended data (and, for an entity,
//! after its graphics and mode), the reactor handles in the handle stream after the owner. Only
//! those two places change; every other bit of the object is copied.

use std::collections::BTreeMap;

use super::bits::{BitReader, BitWriter};
use super::crc::crc16;
use super::error::{Error, ErrorKind, Result};
use super::pack::{self, FileInfo, Section};
use super::read::{RawObject, read_container};
use super::version::DwgVersion as V;
use super::{DwgVersion, r2004};

/// The largest page size rewritten, as the reader's limit for one page (AutoCAD's are 0x7400 for
/// compressed sections and up to 0x9000 for the preview).
const MAX_PAGE_SIZE: usize = 0x10_0000;

/// Seed of an object's checksum (§20.2: over its size and data, stored little-endian after it).
const OBJECT_CRC_SEED: u16 = 0xC0C1;

/// Where the parts of an object that [`with_reactors`] changes sit, in bits from its data start.
struct Layout {
    /// The 32-bit size of the data stream in bits (2000 to 2007), `None` from 2010.
    size_at: Option<usize>,
    /// The reactor count (BL), from and to.
    count_at: usize,
    count_end: usize,
    count: u32,
    /// The end of the data stream: the handle stream starts here.
    data_end: usize,
    /// In the handle stream: after the owner (or at its start when there is none), and after the
    /// reactors.
    owner_end: usize,
    reactors_end: usize,
    total: usize,
}

/// Find an object's [`Layout`].
fn layout(o: &RawObject, version: DwgVersion, entity: bool) -> Result<Layout> {
    if version < V::R2000 {
        return Err(Error::new(ErrorKind::Unsupported, "objects before 2000 aren't rewritten"));
    }
    let total = o.data.len().checked_mul(8).ok_or_else(|| Error::format("object size overflows"))?;
    let mut dat = BitReader::new(&o.data);
    if version >= V::R2010 {
        dat.ot()?;
    } else {
        dat.bs()?;
    }
    let (size_at, size_bits) = if version <= V::R2007 { (Some(dat.position()), Some(dat.rl()?)) } else { (None, None) };
    dat.handle()?;
    super::read::extended_data(&mut dat)?;
    let block_entity = if entity { super::read::entity_mode(&mut dat, version)? == super::EntitySpace::Block } else { false };
    let count_at = dat.position();
    let count = dat.bl()?;
    let count_end = dat.position();
    let data_end = match size_bits {
        Some(bits) => usize::try_from(bits).map_err(|_| Error::format("object data size overflows"))?,
        None => usize::try_from(o.handle_stream_bits)
            .ok()
            .and_then(|h| total.checked_sub(h))
            .ok_or_else(|| Error::format("handle stream larger than the object"))?,
    };
    if data_end < count_end || data_end > total {
        return Err(Error::format("the handle stream starts inside the object's common data"));
    }
    let mut hs = BitReader::new(&o.data);
    hs.set_position(data_end)?;
    // Objects, and entities in a block or owned by another entity, name their owner first.
    if !entity || block_entity {
        hs.handle()?;
    }
    let owner_end = hs.position();
    for _ in 0..count {
        hs.handle()?;
    }
    Ok(Layout { size_at, count_at, count_end, count, data_end, owner_end, reactors_end: hs.position(), total })
}

/// Copy bits `from..to` of `src` to `w`.
fn copy_bits(src: &[u8], from: usize, to: usize, w: &mut BitWriter) -> Result<()> {
    let mut r = BitReader::new(src);
    r.set_position(from)?;
    let mut left = to.checked_sub(from).ok_or_else(|| Error::format("bit range runs backwards"))?;
    while left > 0 {
        let n = left.min(32);
        let n32 = u32::try_from(n).map_err(|_| Error::format("bit count overflows"))?;
        w.bits(r.bits(n32)?, n32);
        left -= n;
    }
    Ok(())
}

/// The number of bits a BL takes for `v`.
fn bl_bits(v: u32) -> usize {
    let mut w = BitWriter::new();
    w.bl(v);
    w.position()
}

/// The object stored with `reactors` (absolute handles, in order) as its persistent reactors:
/// size, handle stream size (2010 and later), data and checksum, ready to sit in an objects
/// section. Everything else of the object is copied bit for bit. `entity` says which common data
/// layout it has ([`super::type_name`]). Handles that don't fit a 32-bit reactor count are an
/// error.
pub fn with_reactors(o: &RawObject, version: DwgVersion, entity: bool, reactors: &[u64]) -> Result<Vec<u8>> {
    let l = layout(o, version, entity)?;
    let count = u32::try_from(reactors.len()).map_err(|_| Error::format("too many reactors"))?;
    let data_bits = l.count_at + bl_bits(count) + (l.data_end - l.count_end);
    let mut w = BitWriter::new();
    match l.size_at {
        Some(at) => {
            copy_bits(&o.data, 0, at, &mut w)?;
            w.rl(u32::try_from(data_bits).map_err(|_| Error::format("object data size overflows"))?);
            copy_bits(&o.data, at + 32, l.count_at, &mut w)?;
        }
        None => copy_bits(&o.data, 0, l.count_at, &mut w)?,
    }
    w.bl(count);
    copy_bits(&o.data, l.count_end, l.data_end, &mut w)?;
    if w.position() != data_bits {
        return Err(Error::format("rewritten data stream has an unexpected size"));
    }
    copy_bits(&o.data, l.data_end, l.owner_end, &mut w)?;
    for &r in reactors {
        // Soft pointers (code 4) to the reactors, as AutoCAD writes them.
        w.handle(4, r);
    }
    copy_bits(&o.data, l.reactors_end, l.total, &mut w)?;
    w.pad_to_byte();
    finish_stored(w.into_bytes(), version, data_bits)
}

/// The reactor count an object says it has, and the reactor handles from its handle stream.
pub fn reactors(o: &RawObject, version: DwgVersion, entity: bool) -> Result<Vec<u64>> {
    let l = layout(o, version, entity)?;
    let mut hs = BitReader::new(&o.data);
    hs.set_position(l.owner_end)?;
    let own = o.handle;
    (0..l.count).map(|_| hs.handle().map(|h| h.absolute(own))).collect()
}

/// The object stored with type code `type_code` instead of its own (a class object moved to a
/// file that numbers its classes differently): size, handle stream size (2010 and later), data and
/// checksum. Every other bit is copied.
pub fn with_type(o: &RawObject, version: DwgVersion, type_code: u16) -> Result<Vec<u8>> {
    if version < V::R2000 {
        return Err(Error::new(ErrorKind::Unsupported, "objects before 2000 aren't rewritten"));
    }
    let total = o.data.len().checked_mul(8).ok_or_else(|| Error::format("object size overflows"))?;
    let mut dat = BitReader::new(&o.data);
    let mut w = BitWriter::new();
    let data_bits = if version >= V::R2010 {
        dat.ot()?;
        w.ot(type_code);
        let data_end = usize::try_from(o.handle_stream_bits)
            .ok()
            .and_then(|h| total.checked_sub(h))
            .ok_or_else(|| Error::format("handle stream larger than the object"))?;
        let rest = dat.position();
        copy_bits(&o.data, rest, total, &mut w)?;
        data_end.checked_sub(rest).and_then(|d| d.checked_add(type_bits(type_code, version)))
    } else {
        dat.bs()?;
        let after_type = dat.position();
        let data_end = usize::try_from(dat.rl()?).map_err(|_| Error::format("object data size overflows"))?;
        let rest = dat.position();
        let new_end = data_end.checked_sub(after_type).and_then(|d| d.checked_add(type_bits(type_code, version)));
        w.bs(type_code);
        w.rl(new_end.and_then(|e| u32::try_from(e).ok()).ok_or_else(|| Error::format("object data size overflows"))?);
        copy_bits(&o.data, rest, total, &mut w)?;
        new_end
    }
    .ok_or_else(|| Error::format("object data size overflows"))?;
    w.pad_to_byte();
    finish_stored(w.into_bytes(), version, data_bits)
}

/// A stored object (size, data and checksum: [`with_reactors`]'s or [`with_type`]'s output) read
/// back as the reader sees it.
pub fn parse_stored(version: DwgVersion, stored: &[u8]) -> Result<RawObject> {
    super::read::object_at(version, stored, 0)
}

/// The number of bits the type code `t` takes (OT from 2010, BS before).
fn type_bits(t: u16, version: DwgVersion) -> usize {
    let mut w = BitWriter::new();
    if version >= V::R2010 {
        w.ot(t);
    } else {
        w.bs(t);
    }
    w.position()
}

/// An object's data `body` (data stream of `data_bits` bits, then the handle stream) stored:
/// size, handle stream size (2010 and later), data and checksum.
fn finish_stored(body: Vec<u8>, version: DwgVersion, data_bits: usize) -> Result<Vec<u8>> {
    let mut out = BitWriter::new();
    out.ms(u32::try_from(body.len()).map_err(|_| Error::format("object too large"))?);
    if version >= V::R2010 {
        let hs_bits = body.len().checked_mul(8).and_then(|t| t.checked_sub(data_bits)).ok_or_else(|| Error::format("object size overflows"))?;
        out.mc_unsigned(u64::try_from(hs_bits).map_err(|_| Error::format("handle stream size overflows"))?);
    }
    let mut stored = out.into_bytes();
    stored.extend_from_slice(&body);
    let crc = crc16(OBJECT_CRC_SEED, &stored);
    stored.extend_from_slice(&crc.to_le_bytes());
    Ok(stored)
}

/// Every handle an object references, in its handle stream's order (absolute; null handles
/// included): what it points to, whatever its type.
pub fn handle_refs(o: &RawObject, version: DwgVersion) -> Result<Vec<u64>> {
    let start = handle_stream_start(o, version)?;
    let mut hs = BitReader::new(&o.data);
    hs.set_position(start)?;
    let mut out = Vec::new();
    // A handle takes at least 8 bits; what is left after the last one is padding.
    while hs.remaining() >= 8 {
        let Ok(h) = hs.handle() else { break };
        out.push(h.absolute(o.handle));
    }
    Ok(out)
}

/// Where an object's handle stream starts, in bits of its data.
fn handle_stream_start(o: &RawObject, version: DwgVersion) -> Result<usize> {
    if version < V::R2000 {
        return Err(Error::new(ErrorKind::Unsupported, "objects before 2000 aren't read for their handles"));
    }
    let total = o.data.len().checked_mul(8).ok_or_else(|| Error::format("object size overflows"))?;
    if version >= V::R2010 {
        return usize::try_from(o.handle_stream_bits)
            .ok()
            .and_then(|h| total.checked_sub(h))
            .ok_or_else(|| Error::format("handle stream larger than the object"));
    }
    let mut dat = BitReader::new(&o.data);
    dat.bs()?;
    let start = usize::try_from(dat.rl()?).map_err(|_| Error::format("object data size overflows"))?;
    if start > total {
        return Err(Error::format("handle stream starts past the object"));
    }
    Ok(start)
}

/// The object stored with its `index`th reference ([`handle_refs`]'s order) naming `handle`, of the
/// same kind (hard or soft, pointer or owner): size, handle stream size (2010 and later), data and
/// checksum. Every other bit is copied. A reference stored relative to the object's own handle is
/// not rewritten (an error), nor is one past the handle stream's end.
pub fn with_reference(o: &RawObject, version: DwgVersion, index: usize, handle: u64) -> Result<Vec<u8>> {
    let start = handle_stream_start(o, version)?;
    let mut hs = BitReader::new(&o.data);
    hs.set_position(start)?;
    for _ in 0..index {
        hs.handle()?;
    }
    let from = hs.position();
    let code = hs.handle()?.code;
    if !(2..=5).contains(&code) {
        return Err(Error::new(ErrorKind::Unsupported, "relative references aren't rewritten"));
    }
    let to = hs.position();
    let total = o.data.len().checked_mul(8).ok_or_else(|| Error::format("object size overflows"))?;
    // A handle is whole bytes, so the rest (the padding too, as the file had it) stays aligned.
    let mut w = BitWriter::new();
    copy_bits(&o.data, 0, from, &mut w)?;
    w.handle(code, handle);
    copy_bits(&o.data, to, total, &mut w)?;
    finish_stored(w.into_bytes(), version, start)
}

/// The length in bytes of the object stored at `offset` of `objects`: its size prefix, data and
/// checksum.
pub(crate) fn stored_len(version: DwgVersion, objects: &[u8], offset: usize) -> Result<usize> {
    let mut r = BitReader::new(objects);
    r.set_position(offset.checked_mul(8).ok_or_else(|| Error::format("object offset overflows"))?)?;
    let size = usize::try_from(r.ms()?).map_err(|_| Error::format("object size overflows"))?;
    if version >= V::R2010 {
        r.mc_unsigned()?;
    }
    let start = r.position() / 8;
    start.checked_add(size).and_then(|e| e.checked_add(2)).and_then(|e| e.checked_sub(offset)).ok_or_else(|| Error::format("object size overflows"))
}

/// An R2004-family file (2004, 2010, 2013, 2018) written again with the stored objects of
/// `replace` (by handle: [`with_reactors`]'s output) in place of the file's own. The objects
/// section and the object map are rebuilt around them; every other section is written back as
/// read, in the file's own order and page sizes. Files of other versions, with encrypted
/// sections, or with a section that doesn't read, are refused.
pub fn replace_objects(file: &[u8], replace: &BTreeMap<u64, Vec<u8>>) -> Result<Vec<u8>> {
    let c = read_container(file)?;
    let version = c.version;
    if !matches!(version, V::R2004 | V::R2010 | V::R2013 | V::R2018) {
        return Err(Error::new(ErrorKind::Unsupported, format!("{} files aren't rewritten", version.magic())));
    }
    let raw = r2004::read(file, version)?;
    if raw.layout.iter().any(|s| s.encrypted) {
        return Err(Error::new(ErrorKind::Unsupported, "a section is encrypted"));
    }
    // The objects section, rebuilt in file order with every gap kept.
    let mut by_offset: Vec<(usize, u64)> = c.map.iter().map(|r| (r.offset, r.handle)).collect();
    by_offset.sort_unstable();
    let mut objects = Vec::with_capacity(raw.objects.len());
    let mut entries: Vec<(u64, u64)> = Vec::with_capacity(by_offset.len());
    let mut at = 0usize;
    for (offset, handle) in by_offset {
        if offset < at {
            return Err(Error::format(format!("object {handle:X} overlaps the one before it")));
        }
        let len = stored_len(version, &raw.objects, offset)?;
        let end = offset.checked_add(len).ok_or_else(|| Error::format("object size overflows"))?;
        objects.extend_from_slice(raw.objects.get(at..offset).ok_or_else(|| Error::truncated("objects section"))?);
        entries.push((handle, u64::try_from(objects.len()).map_err(|_| Error::format("objects section too large"))?));
        match replace.get(&handle) {
            Some(stored) => objects.extend_from_slice(stored),
            None => objects.extend_from_slice(raw.objects.get(offset..end).ok_or_else(|| Error::truncated("objects section"))?),
        }
        at = end;
    }
    objects.extend_from_slice(raw.objects.get(at..).unwrap_or_default());
    entries.sort_unstable();
    let handles = pack::object_map(&entries)?;
    // Every section as read, in its file order; the map order is the section map's.
    let mut layout: Vec<&super::container::SectionLayout> = raw.layout.iter().filter(|s| !s.name.is_empty()).collect();
    let map_order: Vec<&str> = layout.iter().map(|s| s.name.as_str()).collect();
    layout.sort_by_key(|s| s.first_page);
    let mut sections = Vec::with_capacity(layout.len());
    for s in &layout {
        let data = match s.name.as_str() {
            "AcDb:Header" => raw.header.clone(),
            "AcDb:Classes" => raw.classes.clone(),
            "AcDb:Handles" => Some(handles.clone()),
            "AcDb:AcDbObjects" => Some(objects.clone()),
            name => raw.others.iter().find(|(n, _)| n == name).map(|(_, d)| d.clone()),
        }
        .ok_or_else(|| Error::format(format!("section {} didn't read", s.name)))?;
        // Pages are padded to their size when written: a size beyond what the reader takes for a
        // page (a damaged section map can claim gigabytes) is refused.
        let page_size = usize::try_from(s.page_size)
            .ok()
            .filter(|p| (1..=MAX_PAGE_SIZE).contains(p))
            .ok_or_else(|| Error::format(format!("section {} has a page size of {:#x}", s.name, s.page_size)))?;
        sections.push(Section { name: s.name.clone(), data, compressed: s.compressed, page_size });
    }
    let byte = |i: usize| file.get(i).copied().ok_or_else(|| Error::truncated("file header"));
    let mut version_tag = [0u8; 6];
    version_tag.copy_from_slice(file.get(..6).ok_or_else(|| Error::truncated("file header"))?);
    let info = FileInfo {
        version: version_tag,
        maintenance: byte(0x0B)?,
        app_version: byte(0x11)?,
        app_maintenance: byte(0x12)?,
        codepage: c.codepage,
        acad_maintenance: file.get(0x3C..0x40).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes),
    };
    let mut out = pack::pack(&info, &sections, &map_order)?;
    keep_file_header(file, &mut out)?;
    Ok(out)
}

/// The fields of the plain file header (0x00 to 0x80) that say where sections start (§4.1: the
/// preview at 0x0D, the summary info at 0x20, the application info at 0x2C and 0x30): the packer
/// sets them for the file it lays out. Every other byte of the plain header is the source file's.
const HEADER_ADDRESSES: [std::ops::Range<usize>; 4] = [0x0D..0x11, 0x20..0x24, 0x2C..0x30, 0x30..0x34];

/// Copy the source file's plain header over the packed one, but for [`HEADER_ADDRESSES`], so
/// bytes the specification leaves open keep the writer's values.
fn keep_file_header(source: &[u8], out: &mut [u8]) -> Result<()> {
    let mut header: Vec<u8> = source.get(..0x80).ok_or_else(|| Error::truncated("file header"))?.to_vec();
    for r in HEADER_ADDRESSES {
        let packed = out.get(r.clone()).ok_or_else(|| Error::truncated("packed file header"))?;
        header.get_mut(r).ok_or_else(|| Error::truncated("file header"))?.copy_from_slice(packed);
    }
    out.get_mut(..0x80).ok_or_else(|| Error::truncated("packed file header"))?.copy_from_slice(&header);
    Ok(())
}
