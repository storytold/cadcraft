use crate::output::OutputError;
use crate::perf::PerfError;
use crate::run::RunError;
use crate::store::StoreError;

/// Why a subcommand failed: printed as `error: {e}`, exit 1.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    #[error(transparent)]
    Engine(#[from] cadcraft_engine::EngineError),
    #[error(transparent)]
    Doc(#[from] cadcraft_engine::doc::DocError),
    #[error(transparent)]
    Format(#[from] cadcraft_io::IoError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Output(#[from] OutputError),
    #[error(transparent)]
    Perf(#[from] PerfError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The engine and io crates take file names as text.
    #[error("{}: the file name is not valid Unicode", .0.display())]
    NotUnicode(std::path::PathBuf),
    #[error("cannot connect to {addr}: {source}")]
    McpConnect { addr: String, source: std::io::Error },
}

pub(crate) type Result<T = (), E = CliError> = std::result::Result<T, E>;
