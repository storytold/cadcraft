//! Raster image files decoded to pixels (IMAGEATTACH): PNG, JPEG, BMP and TIFF, the format taken
//! from the content. Files are hostile input: the size is read from the header and checked
//! before any pixel is decoded.

use std::io::Cursor;

use cadcraft_doc::Raster;

use crate::{IoError, Result};

/// Largest image file read (bytes).
pub const MAX_IMAGE_BYTES: usize = 256 << 20;
/// Most pixels decoded per image (four bytes each in memory).
pub const MAX_IMAGE_PIXELS: u64 = 128_000_000;

fn bad(e: impl std::fmt::Display) -> IoError {
    IoError::Format(format!("image: {e}"))
}

/// A reader over an image file, once it is small enough and of a format we read.
fn reader(bytes: &[u8]) -> Result<image::ImageReader<Cursor<&[u8]>>> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(bad(format!("the file is larger than {} MB", MAX_IMAGE_BYTES >> 20)));
    }
    // The format comes from the file's first bytes, not from its name.
    let file = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(bad)?;
    if file.format().is_none() {
        return Err(bad("not a PNG, JPEG, BMP or TIFF file"));
    }
    Ok(file)
}

/// The width and height in pixels of the image in `bytes`, read from its header.
pub fn probe(bytes: &[u8]) -> Result<(u32, u32)> {
    let (width, height) = reader(bytes)?.into_dimensions().map_err(bad)?;
    if width == 0 || height == 0 {
        return Err(bad("the image is empty"));
    }
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS {
        return Err(bad(format!("{width} x {height} pixels is more than {} megapixels", MAX_IMAGE_PIXELS / 1_000_000)));
    }
    Ok((width, height))
}

/// Decode the image in `bytes`.
pub fn decode(bytes: &[u8]) -> Result<Raster> {
    // The size is checked first, from the header alone.
    let (width, height) = probe(bytes)?;
    let mut file = reader(bytes)?;
    file.limits(decode_limits(width, height));
    let pixels = file.decode().map_err(bad)?.into_rgba8();
    Raster::new(pixels.width(), pixels.height(), pixels.into_raw()).ok_or_else(|| bad("the image is empty"))
}

/// What the decoder may allocate for an image of this size: the pixels themselves (up to 16
/// bytes each before they become 8-bit RGBA) and its own working buffers.
fn decode_limits(width: u32, height: u32) -> image::Limits {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(pixels.saturating_mul(16).saturating_add(64 << 20));
    limits
}

/// `raster` as a PNG file.
pub fn encode_png(raster: &Raster) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        raster.rgba(),
        raster.width(),
        raster.height(),
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(bad)?;
    Ok(out.into_inner())
}
