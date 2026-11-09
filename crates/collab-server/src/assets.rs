//! The room's attachment store: the one place a shared session reads an image's bytes from.
//!
//! A client never names a file here. It sends the bytes and their SHA-256; the room checks that
//! the address is the bytes it received, then writes the blob under `<room>/<sha256>.<ext>` inside
//! the root given by `--assets`. A drawing then refers to the blob by a relative label, so a
//! commit opens for the cabinet instead of pointing at the author's disk (ADR 0021).
//!
//! The store is content-addressed and therefore has no path a client can steer: the only names it
//! builds are 64 hexadecimal characters plus an extension taken from the bytes' own signature.
//! It still checks that a resolved file is inside the root, so a symlink planted in the store
//! cannot turn a read into a look at another file.
//!
//! Limits are per room: one blob is at most [`MAX_ASSET_BYTES`], a room holds at most
//! [`MAX_ASSET_FILES`] blobs and [`MAX_ASSET_TOTAL`] bytes. They are checked against what is on
//! disk, so a restart does not reset the quota.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The largest one attachment may be, in bytes.
pub const MAX_ASSET_BYTES: u64 = 256 << 20;
/// The most blobs one room may store.
pub const MAX_ASSET_FILES: usize = 1000;
/// The most bytes one room's attachments may take on disk.
pub const MAX_ASSET_TOTAL: u64 = 2 << 30;

/// How much one room may store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetLimits {
    /// The largest one blob may be.
    pub max_bytes: u64,
    /// The most blobs the room may hold.
    pub max_files: usize,
    /// The most bytes the room may hold.
    pub max_total: u64,
}

impl Default for AssetLimits {
    fn default() -> Self {
        AssetLimits { max_bytes: MAX_ASSET_BYTES, max_files: MAX_ASSET_FILES, max_total: MAX_ASSET_TOTAL }
    }
}

/// Why the store refused an attachment operation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AssetError {
    /// The name is not a content address.
    #[error("the attachment id must be 64 lower-case hexadecimal characters")]
    BadId,
    /// The bytes do not hash to the address the client gave.
    #[error("the bytes are not the attachment the id names")]
    Mismatch,
    /// The bytes are not one of the raster formats a drawing can hold.
    #[error("the attachment is not a PNG, JPEG, BMP or TIFF image")]
    NotAnImage,
    /// The blob is larger than [`AssetLimits::max_bytes`].
    #[error("the attachment is {bytes} bytes; the limit for one file is {limit}")]
    TooLarge { bytes: u64, limit: u64 },
    /// The room already holds the maximum number of blobs.
    #[error("the room already holds {files} attachments; the limit is {limit}")]
    TooManyFiles { files: usize, limit: usize },
    /// The room already holds the maximum number of bytes.
    #[error("the room's attachments take {bytes} bytes; the limit is {limit}")]
    Quota { bytes: u64, limit: u64 },
    /// No blob has this address.
    #[error("no attachment has this id")]
    NotFound,
    /// The room id cannot name a directory.
    #[error("`{room}` cannot name a room's attachment directory")]
    BadRoom { room: String },
    /// The store's own file system failed.
    #[error("the attachment store could not be read or written: {0}")]
    Io(String),
}

impl From<std::io::Error> for AssetError {
    fn from(error: std::io::Error) -> Self {
        AssetError::Io(error.to_string())
    }
}

/// What a successful [`AssetStore::put`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    /// The content address.
    pub sha256: String,
    /// The size of the blob.
    pub bytes: u64,
    /// Whether this call created the file; `false` means it was already stored.
    pub stored: bool,
    /// The label a drawing uses, relative to the `--assets` root (`<room>/<sha256>.<ext>`).
    pub label: String,
}

/// The attachments of one room, under one directory.
#[derive(Debug)]
pub struct AssetStore {
    root: PathBuf,
    room: String,
    limits: AssetLimits,
}

impl AssetStore {
    /// Open the store of `room` under `root`, creating the room's directory when needed.
    ///
    /// `room` is the session id: the cabinet's name for the room, not a client path, but it still
    /// has to name a single directory and is checked here.
    pub fn new(root: impl Into<PathBuf>, room: &str) -> Result<Self, AssetError> {
        if !valid_room(room) {
            return Err(AssetError::BadRoom { room: room.to_string() });
        }
        let root = root.into().join(room);
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(&root)?;
        Ok(AssetStore { root, room: room.to_string(), limits: AssetLimits::default() })
    }

    /// Use `limits` instead of the defaults. A deployment may tighten them; the tests use small
    /// numbers so a quota is reachable without writing gigabytes.
    pub fn with_limits(mut self, limits: AssetLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The canonical directory this store writes to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The room's own name.
    pub fn room(&self) -> &str {
        &self.room
    }

    /// Store `bytes` under `sha256`, or recognise that the blob is already there.
    ///
    /// The address is checked against the bytes before the size and quota, so a client cannot
    /// fill the room by lying about an address; deduplication happens before the quota because a
    /// repeat of a stored blob takes no more space.
    pub fn put(&self, sha256: &str, bytes: &[u8]) -> Result<Stored, AssetError> {
        if !cadcraft_collab::is_asset_id(sha256) {
            return Err(AssetError::BadId);
        }
        if cadcraft_collab::asset_id(bytes) != sha256 {
            return Err(AssetError::Mismatch);
        }
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if let Some(path) = self.find(sha256)? {
            let label = self.label_of(&path);
            return Ok(Stored { sha256: sha256.to_string(), bytes: size, stored: false, label });
        }
        if size > self.limits.max_bytes {
            return Err(AssetError::TooLarge { bytes: size, limit: self.limits.max_bytes });
        }
        let extension = cadcraft_collab::asset_extension(bytes).ok_or(AssetError::NotAnImage)?;
        let (files, total) = self.usage()?;
        if files >= self.limits.max_files {
            return Err(AssetError::TooManyFiles { files, limit: self.limits.max_files });
        }
        if total.saturating_add(size) > self.limits.max_total {
            return Err(AssetError::Quota { bytes: total.saturating_add(size), limit: self.limits.max_total });
        }
        let name = format!("{sha256}.{extension}");
        let target = self.root.join(&name);
        let temporary = self.temporary_path(sha256);
        {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            // If writing fails, the partial blob is removed below; the drawing never learns of it.
            if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
                drop(file);
                let _ = std::fs::remove_file(&temporary);
                return Err(error.into());
            }
        }
        if let Err(error) = std::fs::rename(&temporary, &target) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        sync_directory(&self.root);
        Ok(Stored { sha256: sha256.to_string(), bytes: size, stored: true, label: format!("{}/{name}", self.room) })
    }

    /// The bytes stored under `sha256`.
    ///
    /// The file is looked up by its content address, then resolved and checked to stay under the
    /// room's root: a symlink that points elsewhere is refused like a missing blob.
    pub fn get(&self, sha256: &str) -> Result<Vec<u8>, AssetError> {
        if !cadcraft_collab::is_asset_id(sha256) {
            return Err(AssetError::BadId);
        }
        let Some(path) = self.find(sha256)? else {
            return Err(AssetError::NotFound);
        };
        let resolved = std::fs::canonicalize(&path)?;
        if !resolved.starts_with(&self.root) {
            return Err(AssetError::NotFound);
        }
        let limit = self.limits.max_bytes.saturating_add(1);
        let file = std::fs::File::open(&resolved)?;
        let mut bytes = Vec::new();
        file.take(limit).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.limits.max_bytes {
            return Err(AssetError::TooLarge { bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX), limit: self.limits.max_bytes });
        }
        Ok(bytes)
    }

    /// Whether the store holds the blob.
    pub fn contains(&self, sha256: &str) -> Result<bool, AssetError> {
        if !cadcraft_collab::is_asset_id(sha256) {
            return Err(AssetError::BadId);
        }
        Ok(self.find(sha256)?.is_some())
    }

    /// The label a drawing uses for a stored blob, or `None` when the store does not have it.
    pub fn label(&self, sha256: &str) -> Result<Option<String>, AssetError> {
        if !cadcraft_collab::is_asset_id(sha256) {
            return Err(AssetError::BadId);
        }
        Ok(self.find(sha256)?.map(|path| self.label_of(&path)))
    }

    /// Remove a blob, if it is there. Used when an operation that caused a fresh write is refused,
    /// so a damaged image does not take room in the quota.
    pub fn remove(&self, sha256: &str) -> Result<(), AssetError> {
        if !cadcraft_collab::is_asset_id(sha256) {
            return Err(AssetError::BadId);
        }
        if let Some(path) = self.find(sha256)? {
            std::fs::remove_file(path)?;
            sync_directory(&self.root);
        }
        Ok(())
    }

    /// The number of blobs and the bytes they take.
    pub fn usage(&self) -> Result<(usize, u64), AssetError> {
        let mut files = 0usize;
        let mut bytes = 0u64;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !is_blob_name(name) {
                continue;
            }
            // A directory or a broken symlink with a blob's name is not a blob.
            if let Ok(metadata) = entry.metadata()
                && metadata.is_file()
            {
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(metadata.len());
            }
        }
        Ok((files, bytes))
    }

    /// The blob file of `sha256`, when one is in the room.
    fn find(&self, sha256: &str) -> Result<Option<PathBuf>, AssetError> {
        let prefix = format!("{sha256}.");
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Some(extension) = name.strip_prefix(&prefix)
                && is_image_extension(extension)
            {
                return Ok(Some(self.root.join(name)));
            }
        }
        Ok(None)
    }

    /// The label of a stored blob, relative to the `--assets` root.
    fn label_of(&self, path: &Path) -> String {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        format!("{}/{name}", self.room)
    }

    /// A name no other writer uses, beside the final file.
    fn temporary_path(&self, sha256: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        self.root.join(format!(".{sha256}.{}.{nanos}.{serial}.tmp", std::process::id()))
    }
}

/// Flush the directory entry so a blob survives a crash after its rename.
fn sync_directory(root: &Path) {
    #[cfg(unix)]
    if let Ok(directory) = std::fs::File::open(root) {
        let _ = directory.sync_all();
    }
    #[cfg(not(unix))]
    let _ = root;
}

/// Whether `name` is a blob the store wrote: an address and one of the raster extensions.
fn is_blob_name(name: &str) -> bool {
    let Some((id, extension)) = name.split_once('.') else { return false };
    cadcraft_collab::is_asset_id(id) && is_image_extension(extension)
}

/// Whether `extension` is one [`cadcraft_collab::asset_extension`] can produce.
fn is_image_extension(extension: &str) -> bool {
    matches!(extension, "png" | "jpg" | "bmp" | "tif")
}

/// Whether `room` can name one directory under the store's root.
fn valid_room(room: &str) -> bool {
    !room.is_empty()
        && room.len() <= 128
        && room != "."
        && room != ".."
        && room.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 red PNG: a raster IMAGEATTACH decodes.
    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

    fn png() -> Vec<u8> {
        cadcraft_collab::message::decode_bytes(PNG_1X1).expect("the fixture is base64")
    }

    fn workspace(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cadcraft-assets-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.subsec_nanos()).unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn small_limits() -> AssetLimits {
        AssetLimits { max_bytes: 10_000, max_files: 2, max_total: 15_000 }
    }

    #[test]
    fn a_blob_round_trips_and_is_named_by_its_content() {
        let dir = workspace("roundtrip");
        let store = AssetStore::new(&dir, "demo").unwrap();
        let bytes = png();
        let id = cadcraft_collab::asset_id(&bytes);

        let stored = store.put(&id, &bytes).unwrap();
        assert!(stored.stored);
        assert_eq!(stored.label, format!("demo/{id}.png"));
        assert_eq!(store.get(&id).unwrap(), bytes);
        assert!(store.contains(&id).unwrap());
        assert_eq!(store.usage().unwrap(), (1, bytes.len() as u64));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repeat_is_deduplicated() {
        let dir = workspace("dedupe");
        let store = AssetStore::new(&dir, "demo").unwrap();
        let bytes = png();
        let id = cadcraft_collab::asset_id(&bytes);
        assert!(store.put(&id, &bytes).unwrap().stored);
        let again = store.put(&id, &bytes).unwrap();
        assert!(!again.stored, "the second call finds the blob");
        assert_eq!(store.usage().unwrap().0, 1, "one file, not two");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_address_and_a_non_image_are_refused_before_anything_is_written() {
        let dir = workspace("refuse");
        let store = AssetStore::new(&dir, "demo").unwrap();
        let bytes = png();
        assert_eq!(store.put(&cadcraft_collab::asset_id(b"other"), &bytes), Err(AssetError::Mismatch));
        assert_eq!(store.put("../escape", &bytes), Err(AssetError::BadId));
        assert_eq!(store.put(&cadcraft_collab::asset_id(b"not a raster"), b"not a raster"), Err(AssetError::NotAnImage));
        assert_eq!(store.usage().unwrap(), (0, 0), "nothing was written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_size_limit_is_refused_before_the_write() {
        let dir = workspace("size");
        let store = AssetStore::new(&dir, "demo").unwrap().with_limits(AssetLimits { max_bytes: 16, ..small_limits() });
        let bytes = png();
        assert!(bytes.len() > 16);
        match store.put(&cadcraft_collab::asset_id(&bytes), &bytes) {
            Err(AssetError::TooLarge { limit, .. }) => assert_eq!(limit, 16),
            other => panic!("expected TooLarge, got {other:?}"),
        }
        assert_eq!(store.usage().unwrap(), (0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_room_quota_counts_files_and_bytes() {
        let dir = workspace("quota");
        let store = AssetStore::new(&dir, "demo").unwrap().with_limits(AssetLimits { max_bytes: 10_000, max_files: 1, max_total: 15_000 });
        let first = png();
        let second = vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
        store.put(&cadcraft_collab::asset_id(&first), &first).unwrap();
        match store.put(&cadcraft_collab::asset_id(&second), &second) {
            Err(AssetError::TooManyFiles { files: 1, limit: 1 }) => {}
            other => panic!("expected TooManyFiles, got {other:?}"),
        }

        let store = AssetStore::new(&dir, "by-bytes").unwrap().with_limits(AssetLimits { max_bytes: 10_000, max_files: 10, max_total: 4 });
        let bytes = vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
        match store.put(&cadcraft_collab::asset_id(&bytes), &bytes) {
            Err(AssetError::Quota { limit: 4, .. }) => {}
            other => panic!("expected Quota, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_room_cannot_read_another_rooms_blob() {
        let dir = workspace("rooms-isolation");
        let a = AssetStore::new(&dir, "room-a").unwrap();
        let b = AssetStore::new(&dir, "room-b").unwrap();
        let bytes = png();
        let id = cadcraft_collab::asset_id(&bytes);
        a.put(&id, &bytes).unwrap();
        assert_eq!(b.get(&id), Err(AssetError::NotFound), "another room's attachment is a 404, not a read");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_keeps_the_blobs_and_the_quota() {
        let dir = workspace("restart");
        let bytes = png();
        let id = cadcraft_collab::asset_id(&bytes);
        {
            let store = AssetStore::new(&dir, "demo").unwrap();
            store.put(&id, &bytes).unwrap();
        }
        let reopened = AssetStore::new(&dir, "demo").unwrap().with_limits(AssetLimits { max_bytes: 10_000, max_files: 1, max_total: 15_000 });
        assert_eq!(reopened.get(&id).unwrap(), bytes, "a blob survives a restart");
        assert_eq!(reopened.usage().unwrap().0, 1, "the quota is counted from the disk");
        let other = vec![0xff, 0xd8, 0xff, 0xe0, 9];
        match reopened.put(&cadcraft_collab::asset_id(&other), &other) {
            Err(AssetError::TooManyFiles { files: 1, limit: 1 }) => {}
            result => panic!("a restart must not reset the quota: {result:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_room_that_cannot_name_a_directory_is_refused() {
        let dir = workspace("room");
        let store = AssetStore::new(&dir, "demo").unwrap();
        assert_eq!(store.get(&"0".repeat(64)), Err(AssetError::NotFound));
        assert_eq!(store.put("../escape", b"x"), Err(AssetError::BadId));
        assert!(AssetStore::new(&dir, "../demo").is_err());
        assert!(AssetStore::new(&dir, "..").is_err());
        assert!(AssetStore::new(&dir, "").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_root_is_not_read() {
        let dir = workspace("symlink");
        let store = AssetStore::new(&dir, "demo").unwrap();
        let bytes = png();
        let id = cadcraft_collab::asset_id(&bytes);
        let outside = dir.join("outside.png");
        std::fs::write(&outside, &bytes).unwrap();
        std::os::unix::fs::symlink(&outside, store.root().join(format!("{id}.png"))).unwrap();
        assert_eq!(store.get(&id), Err(AssetError::NotFound), "a symlink out of the room is not an attachment");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_blob_the_size_of_the_limit_is_refused_on_read() {
        let dir = workspace("read-limit");
        let bytes = png();
        let id = cadcraft_collab::asset_id(&bytes);
        let store = AssetStore::new(&dir, "demo").unwrap();
        store.put(&id, &bytes).unwrap();
        let reopened = AssetStore::new(&dir, "demo").unwrap().with_limits(AssetLimits { max_bytes: 4, ..small_limits() });
        match reopened.get(&id) {
            Err(AssetError::TooLarge { limit: 4, .. }) => {}
            other => panic!("expected TooLarge, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
