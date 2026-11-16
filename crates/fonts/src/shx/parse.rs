//! Bridge the shared bounded container reader into CADCraft's font and glyph cache.

use super::{ShxError, ShxFont};
pub(super) use craft_shx::parse::MAX_FILE_BYTES;
#[cfg(test)]
pub(super) use craft_shx::parse::{BIGFONT, SHAPES_10, UNIFONT};
use std::sync::RwLock;

pub(super) fn parse(bytes: &[u8]) -> Result<ShxFont, ShxError> {
    let font = craft_shx::parse::parse(bytes)?;
    Ok(ShxFont {
        kind: font.kind,
        name: font.name,
        header: font.header,
        above: font.above,
        below: font.below,
        dual: font.dual,
        encoding: font.encoding,
        char_width: font.char_width,
        ranges: font.ranges,
        data: font.data,
        records: font.records,
        cache: RwLock::default(),
        hebrew: std::sync::OnceLock::new(),
    })
}
