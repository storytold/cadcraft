//! One CADCraft room served over WebSocket, with an optional health and metrics endpoint.
//!
//! The drawing is read once, the room is authoritative from then on, and a snapshot of its DXF is
//! written to `--state` every `--snapshot-secs`: a restart restores it and replays nothing, because
//! the journal is not persisted yet. Tokens are verified with the cabinet's HS256 secret from
//! `--secret-file`; `--dev-token token:role` exists for a single-tenant or test deployment.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cadcraft_collab::Role;
use cadcraft_collab_server::auth::{Authenticator, Claims, JwtAuthenticator, StaticAuthenticator};
use cadcraft_collab_server::{AssetStore, Cabinet, CabinetConfig, DEFAULT_ATTEMPTS, DEFAULT_TIMEOUT_MS, Room, Server, Snapshot};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "cadcraft-collab-server", about = "Serve one CADCraft drawing to a collaboration session")]
struct Args {
    /// WebSocket listen address.
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: SocketAddr,
    /// The drawing to serve.
    #[arg(long)]
    file: PathBuf,
    /// The session id the cabinet knows.
    #[arg(long, default_value = "session")]
    session: String,
    /// A file holding the cabinet's HS256 session secret.
    #[arg(long)]
    secret_file: Option<PathBuf>,
    /// Require this `iss` claim.
    #[arg(long)]
    issuer: Option<String>,
    /// Require this `aud` claim.
    #[arg(long)]
    audience: Option<String>,
    /// A fixed token and role (`token:role`); repeatable. For tests and single-tenant deployments.
    #[arg(long = "dev-token")]
    dev_tokens: Vec<String>,
    /// Where to write the periodic DXF snapshot.
    #[arg(long)]
    state: Option<PathBuf>,
    /// The root of this room's attachment store (`--assets <dir>`); absent means the room has no
    /// store and `imageattach` takes its bytes in the operation only (ADR 0021).
    #[arg(long)]
    assets: Option<PathBuf>,
    /// The cabinet's commit endpoint (`http://…/api/v1/internal/office/sessions/{sessionId}/commit`).
    /// When set, the room posts every commit there instead of answering with the file (M87).
    #[arg(long)]
    cabinet_url: Option<String>,
    /// A file holding the cabinet service token for `--cabinet-url`.
    #[arg(long)]
    cabinet_token_file: Option<PathBuf>,
    /// The cabinet's file id this room serves (with `--cabinet-url`).
    #[arg(long)]
    file_id: Option<String>,
    /// Deadline for one cabinet request, in milliseconds.
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_MS)]
    cabinet_timeout_ms: u64,
    /// Attempts for a transient cabinet failure (`409`/`401`/`413` are never retried).
    #[arg(long, default_value_t = DEFAULT_ATTEMPTS)]
    cabinet_attempts: u32,
    /// Seconds between snapshots.
    #[arg(long, default_value_t = 30)]
    snapshot_secs: u64,
    /// HTTP health and metrics address.
    #[arg(long)]
    http: Option<SocketAddr>,
    /// Close the room after this many seconds with no participants; 0 keeps it open.
    #[arg(long, default_value_t = 600)]
    idle_secs: u64,
}

fn main() -> Result<(), String> {
    let args = Args::parse();
    let auth = authenticator(&args)?;
    // The room and the transport share one store; it is created before the drawing so a bad
    // `--assets` fails at startup, not on the first image.
    let assets = match &args.assets {
        Some(root) => Some(Arc::new(AssetStore::new(root, &args.session).map_err(|e| e.to_string())?)),
        None => None,
    };
    let mut room = load(&args)?;
    if let Some(cabinet) = cabinet(&args)? {
        room = room.with_cabinet(cabinet);
    }
    if let Some(assets) = &assets {
        room = room.with_assets(Arc::clone(assets));
    }
    let mut server = Server::new(room, auth, args.session.clone(), "cadcraft");
    if let Some(assets) = assets {
        server = server.with_assets(assets);
    }
    let server = Arc::new(server);
    let (addr, _accept_loop) = server.bind(args.listen).map_err(|e| e.to_string())?;
    println!("cadcraft-collab-server: session {} listening on ws://{addr}", args.session);

    if let Some(state) = args.state.clone() {
        let server = Arc::clone(&server);
        let interval = Duration::from_secs(args.snapshot_secs.max(1));
        thread::spawn(move || {
            loop {
                thread::sleep(interval);
                if let Ok(snapshot) = server.room().snapshot() {
                    let temporary = state.with_extension("dxf.tmp");
                    if std::fs::write(&temporary, &snapshot.bytes).is_ok() {
                        let _ = std::fs::rename(&temporary, &state);
                    }
                }
            }
        });
    }

    if let Some(http) = args.http {
        let server = Arc::clone(&server);
        thread::spawn(move || serve_http(http, server));
    }

    if args.idle_secs > 0 {
        let server = Arc::clone(&server);
        let state = args.state.clone();
        let idle_secs = args.idle_secs;
        thread::spawn(move || {
            let mut idle_since = Some(std::time::Instant::now());
            loop {
                thread::sleep(Duration::from_secs(30));
                if server.participants() > 0 {
                    idle_since = None;
                    continue;
                }
                let since = idle_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed().as_secs() < idle_secs {
                    continue;
                }
                if let Some(path) = &state {
                    let _ = server.room().compact(path);
                }
                println!("cadcraft-collab-server: no participants for {idle_secs} s; the room is closed");
                std::process::exit(0);
            }
        });
    }

    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

/// The room to serve: the last snapshot when there is one, else the drawing file.
fn load(args: &Args) -> Result<Room, String> {
    if let Some(state) = &args.state
        && let Ok(bytes) = std::fs::read(state)
        && let Ok(version) = std::fs::read_to_string(state_version(state)).and_then(|text| text.trim().parse::<u64>().map_err(std::io::Error::other))
    {
        return Room::restore_with_journal(&Snapshot { version, bytes }, &journal_path(state)).map_err(|e| e.to_string());
    }
    let bytes = std::fs::read(&args.file).map_err(|e| format!("{}: {e}", args.file.display()))?;
    let drawing = cadcraft_io::read(&bytes, &args.file).map_err(|e| e.to_string())?;
    let room = Room::new(drawing).map_err(|e| e.to_string())?;
    match &args.state {
        Some(state) => room.with_journal(journal_path(state)).map_err(|e| e.to_string()),
        None => Ok(room),
    }
}

/// The cabinet commit callback, when the deployment configured one (M87).
///
/// The token comes from a file, never from the process list or an environment dump; the URL and
/// the file id are required together with it.
fn cabinet(args: &Args) -> Result<Option<Arc<Cabinet>>, String> {
    let Some(url) = &args.cabinet_url else {
        if args.cabinet_token_file.is_some() || args.file_id.is_some() {
            return Err("--cabinet-token-file and --file-id need --cabinet-url".into());
        }
        return Ok(None);
    };
    let Some(path) = &args.cabinet_token_file else {
        return Err("--cabinet-url needs --cabinet-token-file".into());
    };
    let Some(file_id) = &args.file_id else {
        return Err("--cabinet-url needs --file-id".into());
    };
    if !url.starts_with("http://") {
        return Err("--cabinet-url must start with http:// (TLS belongs to the proxy)".into());
    }
    let token = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?.trim().to_string();
    if token.is_empty() {
        return Err(format!("{}: the cabinet token is empty", path.display()));
    }
    Ok(Some(Arc::new(Cabinet::new(CabinetConfig {
        url: url.clone(),
        token,
        file_id: file_id.clone(),
        timeout: Duration::from_millis(args.cabinet_timeout_ms),
        attempts: args.cabinet_attempts,
    }))))
}

/// The journal beside a snapshot, one applied operation per line.
fn journal_path(state: &std::path::Path) -> PathBuf {
    state.with_extension("jsonl")
}

/// The file beside a snapshot that holds its version.
fn state_version(state: &std::path::Path) -> PathBuf {
    state.with_extension("version")
}

fn authenticator(args: &Args) -> Result<Arc<dyn Authenticator>, String> {
    if let Some(path) = &args.secret_file {
        let secret = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut jwt = JwtAuthenticator::hs256(secret);
        if let Some(issuer) = &args.issuer {
            jwt = jwt.with_issuer(issuer.clone());
        }
        if let Some(audience) = &args.audience {
            jwt = jwt.with_audience(audience.clone());
        }
        return Ok(Arc::new(jwt));
    }
    if args.dev_tokens.is_empty() {
        return Err("give --secret-file or at least one --dev-token".into());
    }
    let mut entries = Vec::new();
    for entry in &args.dev_tokens {
        let (token, role) = entry.split_once(':').ok_or_else(|| format!("`{entry}` is not token:role"))?;
        let role = match role.to_ascii_lowercase().as_str() {
            "owner" => Role::Owner,
            "editor" => Role::Editor,
            "viewer" => Role::Viewer,
            other => return Err(format!("`{other}` is not owner, editor or viewer")),
        };
        entries.push((token.to_string(), Claims { sub: token.to_string(), doc: String::new(), role }));
    }
    Ok(Arc::new(StaticAuthenticator::new(entries)))
}

fn serve_http(addr: SocketAddr, server: Arc<Server>) {
    let Ok(listener) = TcpListener::bind(addr) else { return };
    for stream in listener.incoming().flatten() {
        let server = Arc::clone(&server);
        thread::spawn(move || {
            let _ = answer(stream, &server);
        });
    }
}

fn answer(mut stream: TcpStream, server: &Server) -> std::io::Result<()> {
    use std::io::Read;
    let mut request = [0u8; 1024];
    let read = stream.read(&mut request)?;
    let request = String::from_utf8_lossy(&request[..read]);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let body = match path {
        "/healthz" | "/readyz" => "ok\n".to_string(),
        "/metrics" => server.metrics().render(server.version(), server.journal_len(), server.participants()),
        _ => {
            let body = "not found\n";
            return write_response(&mut stream, "404 Not Found", body);
        }
    };
    write_response(&mut stream, "200 OK", &body)
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    let response =
        format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(response.as_bytes())
}
