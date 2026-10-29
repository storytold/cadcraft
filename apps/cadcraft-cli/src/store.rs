//! The `store-*` subcommands: raw bytes to and from a storage backend (a local, NAS or synced
//! folder, or a WebDAV collection), for storytold/wordcraft#518. Credentials stay out of the
//! command registry and the engine: the backend is chosen by flags (or `CRAFT_DAV_TOKEN`) here.

use std::path::PathBuf;

use cadcraft_storage::local::LocalBackend;
use cadcraft_storage::webdav::WebDavBackend;
use cadcraft_storage::{StorageBackend, StorageError, WriteOptions};
use clap::{Args, ValueEnum};
use serde_json::json;

use super::error::Result;
use super::output::{Streams, write_atomic};

/// Which backend the `--store*` flags name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum StoreKind {
    /// A folder on disk (a NAS mount or a synced folder too), at --store-root
    Local,
    /// A WebDAV collection (Nextcloud, ownCloud, oCIS), at --store-url
    Webdav,
}

/// Where the bytes live. WebDAV is plain `http://` only (no TLS in this build): credentials are
/// sent only to a loopback host unless --store-insecure-http says the network is trusted.
#[derive(Args, Debug)]
#[command(next_help_heading = "Storage")]
pub(crate) struct StoreArgs {
    /// The backend [default: webdav with --store-url, else local]
    #[arg(long = "store", value_name = "KIND", value_enum)]
    pub(crate) kind: Option<StoreKind>,
    /// The folder (--store local)
    // Required unless a URL picks WebDAV. A typed `--store local` with a URL is a usage error
    // (the folder is then required, and it conflicts with the URL).
    #[arg(long, value_name = "DIR", required_if_eq("kind", "local"), required_unless_present = "store_url", conflicts_with = "store_url")]
    pub(crate) store_root: Option<PathBuf>,
    /// The collection URL, e.g. http://localhost:9200/dav/spaces/<id>/ (--store webdav)
    #[arg(long, value_name = "URL", required_if_eq("kind", "webdav"))]
    pub(crate) store_url: Option<String>,
    /// Basic auth user (with --store-password)
    #[arg(long, value_name = "USER", requires = "store_password", conflicts_with = "store_token")]
    pub(crate) store_user: Option<String>,
    /// Basic auth password (with --store-user)
    #[arg(long, value_name = "PASSWORD", requires = "store_user", conflicts_with = "store_token")]
    pub(crate) store_password: Option<String>,
    /// OIDC/Bearer access token, instead of Basic auth
    #[arg(long, value_name = "TOKEN", env = "CRAFT_DAV_TOKEN", hide_env_values = true)]
    pub(crate) store_token: Option<String>,
    /// Send credentials over plain HTTP to a host that is not loopback (a trusted LAN or VPN only:
    /// anyone on the path can read them)
    #[arg(long)]
    pub(crate) store_insecure_http: bool,
}

impl StoreArgs {
    /// The backend kind: as given, else WebDAV when a URL names one, else a local folder.
    pub(crate) fn kind(&self) -> StoreKind {
        self.kind.unwrap_or(if self.store_url.is_some() { StoreKind::Webdav } else { StoreKind::Local })
    }

    /// Open the backend the flags name.
    pub(crate) fn open(&self) -> Result<Box<dyn StorageBackend>> {
        match self.kind() {
            StoreKind::Local => {
                let root = self.store_root.as_deref().ok_or(StoreError::Missing("--store local needs --store-root DIR"))?;
                Ok(Box::new(LocalBackend::open(root).map_err(StoreError::Backend)?))
            }
            StoreKind::Webdav => {
                let url = self.store_url.as_deref().ok_or(StoreError::Missing("--store webdav needs --store-url URL"))?;
                let backend = match (&self.store_token, &self.store_user) {
                    (Some(t), None) => WebDavBackend::with_bearer(url, t),
                    _ => WebDavBackend::new(url, self.store_user.as_deref(), self.store_password.as_deref()),
                }
                .map_err(StoreError::Backend)?;
                Ok(Box::new(if self.store_insecure_http { backend.allow_cleartext_credentials() } else { backend }))
            }
        }
    }
}

/// Why a `store-*` command failed: printed as `error: {e}`, exit 1.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
    #[error("{0}")]
    Missing(&'static str),
    #[error(transparent)]
    Backend(StorageError),
    #[error("{path}: {source}")]
    At { path: String, source: StorageError },
}

fn at(path: &str) -> impl FnOnce(StorageError) -> StoreError + '_ {
    move |source| StoreError::At { path: path.to_string(), source }
}

/// Upload a local file to storage at PATH, as JSON (path, size, ETag)
#[derive(Args, Debug)]
pub(crate) struct StoreWriteArgs {
    /// The storage path to write
    pub(crate) path: String,
    /// The local file to upload
    pub(crate) file: PathBuf,
    /// Write only if the stored version still carries this ETag (from store-stat)
    #[arg(long, value_name = "TAG")]
    pub(crate) etag: Option<String>,
    /// Stage and rename, so a crash never leaves a half-written file (local only)
    #[arg(long)]
    pub(crate) atomic: bool,
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

/// Download storage PATH to a local file
#[derive(Args, Debug)]
pub(crate) struct StoreReadArgs {
    /// The storage path to read
    pub(crate) path: String,
    /// The local file to write
    pub(crate) out: PathBuf,
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

/// Print a stored file's metadata (size, ETag, modified) as JSON
#[derive(Args, Debug)]
pub(crate) struct StoreStatArgs {
    /// The storage path
    pub(crate) path: String,
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

/// Print a storage folder's direct children as JSON
#[derive(Args, Debug)]
pub(crate) struct StoreListArgs {
    /// The folder (default: the root)
    pub(crate) prefix: Option<String>,
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

/// Delete a stored file (a missing one is success)
#[derive(Args, Debug)]
pub(crate) struct StoreRemoveArgs {
    /// The storage path
    pub(crate) path: String,
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

/// Print a backend's capabilities, and what the server reports, as JSON
#[derive(Args, Debug)]
pub(crate) struct StoreProbeArgs {
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

/// Finish or drop writes a crash interrupted, as JSON (safe at every startup)
#[derive(Args, Debug)]
pub(crate) struct StoreRecoverArgs {
    #[command(flatten)]
    pub(crate) store: StoreArgs,
}

pub(crate) fn store_write(a: &StoreWriteArgs, io: &mut Streams) -> Result {
    let bytes = std::fs::read(&a.file)?;
    let opts = WriteOptions { if_match: a.etag.clone(), atomic: a.atomic };
    let meta = a.store.open()?.write(&a.path, &bytes, &opts).map_err(at(&a.path))?;
    io.json(&json!({"path": a.path, "size": meta.size, "etag": meta.etag}), false);
    io.errln(format_args!("wrote {} ({} bytes)", a.path, bytes.len()));
    Ok(())
}

pub(crate) fn store_read(a: &StoreReadArgs, io: &mut Streams) -> Result {
    let bytes = a.store.open()?.read(&a.path).map_err(at(&a.path))?;
    write_atomic(&a.out, &bytes)?;
    io.errln(format_args!("wrote {} ({} bytes)", a.out.display(), bytes.len()));
    Ok(())
}

pub(crate) fn store_stat(a: &StoreStatArgs, io: &mut Streams) -> Result {
    let meta = a.store.open()?.stat(&a.path).map_err(at(&a.path))?;
    io.json(&json!({"path": a.path, "size": meta.size, "etag": meta.etag, "modified": meta.modified, "isDir": meta.is_dir}), false);
    Ok(())
}

pub(crate) fn store_list(a: &StoreListArgs, io: &mut Streams) -> Result {
    let prefix = a.prefix.as_deref().unwrap_or("");
    let entries = a.store.open()?.list(prefix).map_err(at(if prefix.is_empty() { "(root)" } else { prefix }))?;
    let list: Vec<_> = entries.iter().map(|e| json!({"path": e.path, "size": e.meta.size, "etag": e.meta.etag, "isDir": e.meta.is_dir})).collect();
    io.json(&json!(list), false);
    Ok(())
}

pub(crate) fn store_remove(a: &StoreRemoveArgs, io: &mut Streams) -> Result {
    a.store.open()?.remove(&a.path).map_err(at(&a.path))?;
    io.errln(format_args!("removed {}", a.path));
    Ok(())
}

pub(crate) fn store_probe(a: &StoreProbeArgs, io: &mut Streams) -> Result {
    let store = a.store.open()?;
    let probe = store.probe().map_err(StoreError::Backend)?;
    // What the server said, not what the backend assumed before asking.
    let caps = &probe.capabilities;
    io.json(
        &json!({
            "atomicWrite": caps.atomic_write, "conditionalWrite": caps.conditional_write, "list": caps.list,
            "etag": caps.etag, "persistent": caps.persistent, "davClasses": probe.dav_classes, "notes": probe.notes,
        }),
        false,
    );
    Ok(())
}

pub(crate) fn store_recover(a: &StoreRecoverArgs, io: &mut Streams) -> Result {
    let report = a.store.open()?.recover().map_err(StoreError::Backend)?;
    io.json(&json!({"completed": report.completed, "dropped": report.dropped, "removedTemp": report.removed_temp}), false);
    Ok(())
}
