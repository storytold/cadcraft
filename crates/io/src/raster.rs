//! Raster image files decoded to pixels (IMAGEATTACH): PNG, JPEG, BMP and TIFF, the format taken
//! from the content. Files are hostile input: the size is read from the header and checked
//! before any pixel is decoded.

use crate::{IoError, Result};
use cadcraft_doc::Raster;
use craft_image_codecs::{ChannelLayout, DecodeOptions, DecodeWarning, Format, Limits, SampleType};

/// Largest image file read (bytes).
pub const MAX_IMAGE_BYTES: usize = 256 << 20;
/// Most pixels decoded per image (four bytes each in memory).
pub const MAX_IMAGE_PIXELS: u64 = 128_000_000;

fn bad(e: impl std::fmt::Display) -> IoError {
    IoError::Format(format!("image: {e}"))
}

/// The width and height in pixels of the first stored image, read without decoding pixels.
pub fn probe(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(bad(format!("the file is larger than {} MB", MAX_IMAGE_BYTES >> 20)));
    }
    let format = craft_image_codecs::detect(bytes).ok_or_else(|| bad("not a PNG, JPEG, BMP or TIFF file"))?;
    if !matches!(format, Format::Png | Format::Jpeg | Format::Bmp | Format::Tiff) {
        return Err(bad("not a PNG, JPEG, BMP or TIFF file"));
    }
    // Match ImageReader's original header budget; check pixels ourselves for the existing diagnostic.
    let limits = Limits::new(u32::MAX, u32::MAX, u64::MAX, 512 << 20);
    let (width, height) = craft_image_codecs::probe_stored_size(bytes, &limits).map_err(bad)?;
    if width == 0 || height == 0 {
        return Err(bad("the image is empty"));
    }
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS {
        return Err(bad(format!("{width} x {height} pixels is more than {} megapixels", MAX_IMAGE_PIXELS / 1_000_000)));
    }
    Ok((width, height))
}

/// Decode the first image, keeping its stored orientation.
pub fn decode(bytes: &[u8]) -> Result<Raster> {
    let (width, height) = probe(bytes)?;
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    let limits = Limits::new(width, height, MAX_IMAGE_PIXELS, pixels.saturating_mul(16).saturating_add(64 << 20));
    let opts = DecodeOptions { limits, keep_orientation: true };
    let format = craft_image_codecs::detect(bytes).ok_or_else(|| bad("not an image"))?;
    let image = if format == Format::Tiff {
        craft_image_codecs::decode_tiff_page(bytes, 0, &opts)
    } else {
        craft_image_codecs::decode_as_with(format, bytes, &opts)
    }
    .map_err(bad)?;
    if image.warnings.iter().any(|w| matches!(w, DecodeWarning::Truncated { .. })) {
        return Err(bad("image data is truncated"));
    }
    if image.dimensions() != (width, height) {
        return Err(bad("decoded image size does not match its header"));
    }
    let rgba = if image.layout() == ChannelLayout::Rgba && image.sample_type() == SampleType::U8 { image.into_data() } else { image.to_rgba8() };
    Raster::new(width, height, rgba).ok_or_else(|| bad("the image is empty"))
}

/// `raster` as a PNG file, encoded in rows to avoid a full extra image copy.
pub fn encode_png(raster: &Raster) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    cadcraft_render::raster::write_rgba_png(&mut out, raster.width(), raster.height(), raster.rgba()).ok_or_else(|| bad("PNG encoding failed"))?;
    Ok(out)
}
