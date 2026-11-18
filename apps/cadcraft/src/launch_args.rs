//! The desktop app's command line, as clap types: `cadcraft --help` comes from these doc comments
//! and its Environment section from [`crate::env::VARS`], so the help and the parsers can't disagree. Anything they don't declare is a usage error naming
//! it (exit status 2), so a typo such as `--sampel` doesn't open the window and then fail to open
//! the flag as a file; so is a flag without its value, a repeated flag and a bad port, from the
//! flag or from `CADCRAFT_CONTROL_PORT`. A value never starts with `-`, and a file whose name
//! starts with `-` goes after `--`.

use std::ffi::OsString;
use std::path::PathBuf;

use cadcraft_mcp::args::{blank_env_is_unset, name_env_vars};
use clap::{Command, CommandFactory, FromArgMatches, Parser};

/// Opens CADCraft with the given files
#[derive(Parser)]
#[command(name = "cadcraft", version = cadcraft_engine::build_version(), after_help = crate::env::help())]
pub(crate) struct LaunchArgs {
    /// Files to open. Put `--` before a name that starts with `-`
    pub(crate) files: Vec<PathBuf>,
    /// Start the localhost JSON-lines control server on this port
    // allow_negative_numbers makes `--control -1` a bad value rather than a missing one.
    #[arg(long, value_name = "PORT", env = "CADCRAFT_CONTROL_PORT", value_parser = control_port, allow_negative_numbers = true)]
    pub(crate) control: Option<u16>,
    /// Open with the sample drawing
    #[arg(long)]
    pub(crate) sample: bool,
    /// Join a shared session at this WebSocket URL
    #[arg(long, value_name = "URL", env = "CADCRAFT_COLLAB_URL")]
    pub(crate) collab_url: Option<String>,
    /// The session token (a JWT) for the shared session; prefer the environment, which is not
    /// visible in the process list
    #[arg(long, value_name = "TOKEN", env = "CADCRAFT_COLLAB_TOKEN", hide = true)]
    pub(crate) collab_token: Option<String>,
    /// The drawing id the shared session is about
    #[arg(long, value_name = "DOC", env = "CADCRAFT_COLLAB_DOC")]
    pub(crate) collab_doc: Option<String>,
}

/// The command line. An empty or blank variable counts as unset.
pub(crate) fn command() -> Command {
    LaunchArgs::command().mut_args(|a| blank_env_is_unset(a, |k| std::env::var_os(k)))
}

/// Parse the launch arguments (without the binary name) with `cmd`, from [`command`]. Old macOS
/// passes a process serial number (`-psn_…`) when launched from Finder: it is dropped first, unless
/// it comes after `--`. A bad value from a variable is a usage error naming the variable.
pub(crate) fn parse(mut cmd: Command, args: impl IntoIterator<Item = OsString>) -> Result<LaunchArgs, clap::Error> {
    let mut options = true;
    let args: Vec<OsString> = args
        .into_iter()
        .filter(|a| {
            options &= a != "--";
            !(options && a.to_str().is_some_and(|s| s.starts_with("-psn_")))
        })
        .collect();
    let argv = std::iter::once(OsString::from("cadcraft")).chain(args.iter().cloned());
    let matches = cmd.try_get_matches_from_mut(argv).map_err(|e| name_env_vars(e, &cmd, &args))?;
    LaunchArgs::from_arg_matches(&matches)
}

/// The shared session's settings: the URL, the token and the drawing, or none of them.
///
/// A partial set is a usage error, not a silent offline start: the user asked for a shared
/// session and would not hear that the request was incomplete. A shared session also opens the
/// room's drawing, so a launch that also names a file (or `--sample`) would mix the room's changes
/// into a local file; it is refused too (CodeRabbit on #2936).
pub(crate) fn collab_config(args: &LaunchArgs) -> Result<Option<(String, String, String)>, String> {
    let config = match (&args.collab_url, &args.collab_token, &args.collab_doc) {
        (None, None, None) => return Ok(None),
        (Some(url), Some(token), Some(doc)) => (url.clone(), token.clone(), doc.clone()),
        (url, token, doc) => {
            let mut missing = Vec::new();
            if url.is_none() {
                missing.push("--collab-url");
            }
            if token.is_none() {
                missing.push("--collab-token");
            }
            if doc.is_none() {
                missing.push("--collab-doc");
            }
            return Err(format!("a shared session needs all three of --collab-url, --collab-token and --collab-doc; missing {}", missing.join(", ")));
        }
    };
    if args.sample || !args.files.is_empty() {
        let what = if args.sample { "--sample" } else { "a file to open" };
        return Err(format!("a shared session opens the room's drawing, not a local one: leave out {what}"));
    }
    Ok(Some(config))
}

/// A control port, for clap's `value_parser`: `--control` and `CADCRAFT_CONTROL_PORT`, which may
/// carry spaces.
fn control_port(s: &str) -> Result<u16, NotAPort> {
    s.trim().parse().map_err(|_| NotAPort)
}

/// A [`control_port`] value that isn't a port.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("expected a port, 0-65535")]
struct NotAPort;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    /// Parse without the variables, so one exported where the tests run can't change them.
    fn parse_args(args: &[&str]) -> Result<LaunchArgs, clap::Error> {
        parse(command().mut_args(|a| a.env(None::<&str>)), args.iter().map(OsString::from))
    }

    fn usage_error(args: &[&str]) -> String {
        let Err(e) = parse_args(args) else { panic!("{args:?} parsed") };
        assert_eq!(e.exit_code(), 2, "{args:?}: {e}");
        e.to_string()
    }

    #[test]
    fn values_are_parsed() {
        let parsed = parse_args(&["--control", " 50494 ", "--sample", "a.dxf", "b.dwg"]).unwrap();
        assert_eq!((parsed.control, parsed.sample), (Some(50494), true));
        assert_eq!(parsed.files, [PathBuf::from("a.dxf"), PathBuf::from("b.dwg")]);
        let parsed = parse_args(&[]).unwrap();
        assert_eq!((parsed.control, parsed.sample, parsed.files.len()), (None, false, 0));
    }

    #[test]
    fn control_port_rejects_unparseable_values_naming_them() {
        for bad in ["78787", "-1", "abc", "99999999999999999999"] {
            assert!(usage_error(&["--control", bad]).contains(&format!("invalid value '{bad}' for '--control <PORT>'")), "{bad}");
        }
        assert!(usage_error(&["--control="]).contains("invalid value '' for '--control <PORT>'"));
    }

    #[test]
    fn usage_errors_exit_2() {
        assert!(usage_error(&["--sampel"]).contains("unexpected argument '--sampel'"));
        assert!(usage_error(&["--control"]).contains("a value is required for '--control <PORT>'"));
        // A following flag is never taken as the value.
        assert!(usage_error(&["--control", "--sample"]).contains("a value is required for '--control <PORT>'"));
        assert!(usage_error(&["--sample", "--sample"]).contains("cannot be used multiple times"));
        assert!(usage_error(&["--control", "1", "--control", "2"]).contains("cannot be used multiple times"));
    }

    #[test]
    fn version_stops_parsing() {
        let e = parse_args(&["-V", "--control"]).err().unwrap();
        assert_eq!((e.kind(), e.exit_code()), (ErrorKind::DisplayVersion, 0));
        assert_eq!(e.to_string(), format!("{}\n", crate::version_string()));
        let e = parse_args(&["--version", "--bogus"]).err().unwrap();
        assert_eq!(e.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn help_stops_parsing_and_double_dash_ends_options() {
        let e = parse_args(&["-h", "--sampel", "--control"]).err().unwrap();
        assert_eq!((e.kind(), e.exit_code()), (ErrorKind::DisplayHelp, 0));
        let help = parse_args(&["--help"]).err().unwrap().to_string();
        assert!(help.contains("--control <PORT>") && help.contains("CADCRAFT_CONFIG_DIR"), "{help}");

        assert!(usage_error(&["--sampel", "--", "--help"]).contains("unexpected argument '--sampel'"));
        let parsed = parse_args(&["a.dxf", "--", "--help", "-x"]).unwrap();
        assert_eq!(parsed.files, ["a.dxf", "--help", "-x"].map(PathBuf::from));
    }

    #[test]
    fn a_finder_process_serial_number_is_dropped() {
        let parsed = parse_args(&["-psn_0_12345", "a.dxf"]).unwrap();
        assert_eq!(parsed.files, [PathBuf::from("a.dxf")]);
        let parsed = parse_args(&["--", "-psn_x.dxf"]).unwrap();
        assert_eq!(parsed.files, [PathBuf::from("-psn_x.dxf")]);
    }

    /// A shared session's three settings are one switch: a partial set is a usage error naming
    /// what is missing, not a silent offline start (CodeRabbit on #2936).
    #[test]
    fn a_partial_collaboration_configuration_is_refused() {
        let error = collab_config(&parse_args(&["--collab-url", "wss://office/sync"]).unwrap()).unwrap_err();
        assert!(error.ends_with("missing --collab-token, --collab-doc"), "{error}");
        let error = collab_config(&parse_args(&["--collab-token", "t"]).unwrap()).unwrap_err();
        assert!(error.ends_with("missing --collab-url, --collab-doc"), "{error}");

        assert!(collab_config(&parse_args(&[]).unwrap()).unwrap().is_none(), "no flags, no session");
        let full = parse_args(&["--collab-url", "wss://office/sync", "--collab-token", "t", "--collab-doc", "file-1"]).unwrap();
        let (url, token, doc) = collab_config(&full).unwrap().expect("all three");
        assert_eq!((url.as_str(), token.as_str(), doc.as_str()), ("wss://office/sync", "t", "file-1"));
    }

    /// A shared session opens the room's drawing: a launch that also names a local file (or the
    /// sample) would let the room's changes land in that file (CodeRabbit on #2936).
    #[test]
    fn a_shared_session_refuses_a_local_drawing() {
        let error =
            collab_config(&parse_args(&["--sample", "--collab-url", "wss://o/s", "--collab-token", "t", "--collab-doc", "f"]).unwrap()).unwrap_err();
        assert!(error.contains("--sample"), "{error}");
        let error =
            collab_config(&parse_args(&["a.dxf", "--collab-url", "wss://o/s", "--collab-token", "t", "--collab-doc", "f"]).unwrap()).unwrap_err();
        assert!(error.contains("a file to open"), "{error}");
    }

    /// A name that isn't valid Unicode is kept as given.
    #[cfg(unix)]
    #[test]
    fn non_unicode_file_names_are_kept() {
        use std::os::unix::ffi::OsStringExt as _;
        let latin1 = OsString::from_vec(b"caf\xe9.dxf".to_vec());
        let parsed = parse(command().mut_args(|a| a.env(None::<&str>)), [latin1.clone(), OsString::from("b.dxf")]).unwrap();
        assert_eq!(parsed.files, [PathBuf::from(&latin1), PathBuf::from("b.dxf")]);
    }

    /// `--control` falls back to `PATH` here instead of `CADCRAFT_CONTROL_PORT`: `PATH` is set
    /// wherever the tests run and is never a port, and tests can't set a variable (`set_var` is
    /// unsafe).
    #[test]
    fn the_flag_wins_over_its_variable_and_a_bad_value_names_the_variable() {
        let Some(path) = std::env::var_os("PATH").filter(|p| !p.is_empty()) else { return };
        let path_fallback = || command().mut_args(|a| a.env(None::<&str>)).mut_arg("control", |a| a.env("PATH"));
        let e = parse(path_fallback(), []).err().unwrap();
        assert_eq!(e.exit_code(), 2);
        let text = e.to_string();
        assert!(text.contains(&format!("invalid value '{}' for 'PATH'", path.to_string_lossy())), "{text}");
        assert!(!text.contains("--control <PORT>'"), "{text}");
        assert_eq!(parse(path_fallback(), ["--control", "7"].map(OsString::from)).unwrap().control, Some(7));
    }
}
