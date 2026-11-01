//! Helpers for clap parsers shared by the apps: environment variables as flag fallbacks.
//!
//! Apply [`blank_env_is_unset`] to the arguments, so an empty variable counts as unset, and pass a
//! parse error through [`name_env_vars`], so a bad value names the variable it came from.

use std::ffi::{OsStr, OsString};

use clap::{Arg, Command};

/// For `Command::mut_args`: an empty or blank variable counts as unset, so `CADCRAFT_CONTROL_PORT=`
/// turns a fallback off without `unset`. clap would pass the empty value to the parser. `var`
/// looks the variable up.
pub fn blank_env_is_unset(arg: Arg, var: impl Fn(&OsStr) -> Option<OsString>) -> Arg {
    let blank = arg.get_env().and_then(var).is_some_and(|v| v.to_str().is_some_and(|v| v.trim().is_empty()));
    if blank { arg.env(None::<&str>) } else { arg }
}

/// clap names the flag when a value from its variable is bad or conflicts with another, as if the
/// flag had been given: name the variable instead when the flag isn't among `args` (without the
/// binary name; a first one naming a subcommand selects whose arguments to look in).
pub fn name_env_vars(mut error: clap::Error, cmd: &Command, args: &[OsString]) -> clap::Error {
    use clap::error::{ContextKind, ContextValue};
    // `--flag value` or `--flag=value`.
    let given =
        |long: &str| args.iter().filter_map(|a| a.to_str()?.strip_prefix("--")?.strip_prefix(long)).any(|r| r.is_empty() || r.starts_with('='));
    let sub = args.first().and_then(|name| cmd.find_subcommand(name)).unwrap_or(cmd);
    for kind in [ContextKind::InvalidArg, ContextKind::PriorArg] {
        let Some(ContextValue::String(shown)) = error.get(kind) else { continue };
        let Some(arg) = sub.get_arguments().find(|a| a.to_string() == *shown) else { continue };
        let (Some(long), Some(var)) = (arg.get_long(), arg.get_env()) else { continue };
        if !given(long) {
            error.insert(kind, ContextValue::String(var.to_string_lossy().into_owned()));
        }
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_variables_count_as_unset() {
        let arg = || Arg::new("control").long("control").env("CADCRAFT_CONTROL_PORT");
        for value in ["", "  "] {
            assert_eq!(blank_env_is_unset(arg(), |_| Some(value.into())).get_env(), None, "{value:?}");
        }
        assert!(blank_env_is_unset(arg(), |_| Some(" 50494 ".into())).get_env().is_some());
        assert!(blank_env_is_unset(arg(), |_| None).get_env().is_some());
    }
}
