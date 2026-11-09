//! Content addresses for collaboration attachments (ADR 0021).
//!
//! A shared session never reads a file by the path a client names. The bytes of an attachment
//! travel to the room once and are named by their SHA-256, so both sides can ask for the same
//! blob and the store can deduplicate it without trusting a file name. The label saved in the
//! drawing is derived from the address too: a commit then points at the room's store, not at a
//! disk the other participants cannot see.

/// The length of an attachment id: SHA-256 in lower-case hexadecimal.
pub const ASSET_ID_LEN: usize = 64;

/// Whether `id` is a content address: 64 lower-case hexadecimal characters.
///
/// Upper case is refused rather than normalized so one blob has one spelling; [`asset_id`]
/// produces that spelling.
pub fn is_asset_id(id: &str) -> bool {
    id.len() == ASSET_ID_LEN && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The content address of `bytes`, lower-case hexadecimal.
pub fn asset_id(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(bytes);
    let mut id = String::with_capacity(ASSET_ID_LEN);
    for byte in digest {
        use std::fmt::Write as _;
        // Writing into a String cannot fail; the result is kept in `id` either way.
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// The file extension an attachment's bytes imply, by their signature.
///
/// The extension is taken from the content, never from a name a client chose. `None` means the
/// bytes are not one of the raster formats a drawing can hold.
pub fn asset_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        Some("tif")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_the_sha256_of_the_bytes() {
        // The empty string's SHA-256, a published vector.
        assert_eq!(asset_id(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(is_asset_id(&asset_id(b"raster")));
        assert!(!is_asset_id(&asset_id(b"raster").to_ascii_uppercase()), "one blob, one spelling");
        assert!(!is_asset_id("../../etc/passwd"));
        assert!(!is_asset_id(&asset_id(b"x")[..ASSET_ID_LEN - 1]));
        assert!(!is_asset_id(&format!("{}0", asset_id(b"x"))), "65 characters is not an address");
    }

    #[test]
    fn the_extension_comes_from_the_signature() {
        assert_eq!(asset_extension(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(asset_extension(b"\xff\xd8\xff\xe0"), Some("jpg"));
        assert_eq!(asset_extension(b"BM0000"), Some("bmp"));
        assert_eq!(asset_extension(b"II*\0"), Some("tif"));
        assert_eq!(asset_extension(b"MM\0*"), Some("tif"));
        assert_eq!(asset_extension(b"<svg/>"), None);
        assert_eq!(asset_extension(b""), None);
    }
}
