//! `cadcraft-cli ask`: the AI Assistant carries out one prompt on a drawing, through a FANTASTICO
//! account. The agent is `cadcraft-assistant`'s, over the MCP tools on a headless session, the
//! same one the desktop palette runs; here the account is called directly with `fantastico-sdk`,
//! with the key from `$FANTASTICO_KEY`.

use std::io::IsTerminal;

use cadcraft_engine::Session;
use serde_json::Value;

use super::args::AskArgs;
use super::error::{CliError, Result};
use super::output::{Streams, export, export_framed};

/// At most `max` characters of `s`, for traces.
fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", s.get(..at).unwrap_or(s)),
        None => s.to_string(),
    }
}

/// The assistant's model endpoint: a FANTASTICO account.
struct FantasticoProvider(fantastico_sdk::Fantastico);

impl cadcraft_assistant::Provider for FantasticoProvider {
    fn complete(&mut self, request: &Value) -> std::result::Result<Value, String> {
        self.0.chat_completion(request).map_err(|e| format!("{} ({})", e.message, e.code))
    }
}

/// `ask` on a terminal: progress on stderr, confirmations asked on stdin.
struct AskObserver<'s, 'a> {
    yes: bool,
    io: &'s mut Streams<'a>,
}

impl cadcraft_assistant::Observer for AskObserver<'_, '_> {
    fn confirm(&mut self, name: &str, arguments: &Value) -> bool {
        if self.yes {
            return true;
        }
        if !std::io::stdin().is_terminal() {
            self.io.errln(format_args!("   declined `{name}`: pass --yes to allow it"));
            return false;
        }
        eprint!("   allow `{name}` {}? [y/N] ", clip(&arguments.to_string(), 200));
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    }

    fn event(&mut self, e: cadcraft_assistant::Event<'_>) {
        use cadcraft_assistant::Event;
        match e {
            Event::ToolCall { name, arguments } => self.io.errln(format_args!("-> {name} {}", clip(&arguments.to_string(), 200))),
            Event::ToolResult { ok: false, text, .. } => self.io.errln(format_args!("   error: {}", clip(text, 300))),
            Event::Declined { name } => self.io.errln(format_args!("   declined `{name}`")),
            // The reply goes to stdout at the end.
            Event::Text(_) | Event::ToolResult { .. } => {}
        }
    }
}

fn ask_error(e: impl std::fmt::Display) -> CliError {
    CliError::Ask(e.to_string())
}

pub(crate) fn ask(a: &AskArgs, io: &mut Streams) -> Result {
    if a.prompt.trim().is_empty() {
        return Err(ask_error("ask needs a prompt, e.g. cadcraft-cli ask \"draw a 100x60 rectangle at 0,0\""));
    }
    let key = std::env::var("FANTASTICO_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| ask_error("ask needs a FANTASTICO key in $FANTASTICO_KEY"))?;
    let opts = fantastico_sdk::Options {
        title: "CADCraft".into(),
        referer: "https://github.com/storytold/cadcraft".into(),
        ..fantastico_sdk::Options::default()
    };
    let client = fantastico_sdk::Fantastico::new(key, opts).map_err(|e| ask_error(e.message))?;
    // The account suggests a model; without one (a project key, or an account that names none)
    // the request leaves it empty and the model the key was minted for answers.
    let model = match (&a.model, client.surface()) {
        (Some(m), _) => m.clone(),
        (None, fantastico_sdk::Surface::Edge) => client.config().map_err(|e| ask_error(e.message))?.model.unwrap_or_default(),
        (None, fantastico_sdk::Surface::Runtime) => String::new(),
    };
    let mut s = Session::empty();
    if a.sample {
        s.open_drawing(cadcraft_engine::sample::default_sample(), "Bracket", None);
    } else if a.metric {
        s.new_drawing(true);
    } else if let Some(f) = &a.open {
        super::open(&mut s, f)?;
    }
    if s.docs.is_empty() {
        s.new_drawing(false);
    }
    let mut backend = cadcraft_mcp::Headless { session: s };
    let mut provider = FantasticoProvider(client);
    let mut config = cadcraft_assistant::Config::new(model);
    if let Some(n) = a.max_steps {
        config.max_steps = usize::from(n);
    }
    let turn = cadcraft_assistant::Agent::new(config).send(&mut provider, &mut backend, &a.prompt, &mut AskObserver { yes: a.yes, io });
    if let Ok(t) = &turn {
        io.outln(format_args!("{}", t.reply.trim()));
    }
    // Write what was done even when the turn stopped early, or stdout was closed: the files were
    // asked for.
    let s = &backend.session;
    if let Some(p) = &a.save {
        export(s, p, io)?;
    }
    if let Some(p) = &a.export {
        export_framed(s, p, None, io)?;
    }
    turn.map(|_| ()).map_err(ask_error)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use crate::args::{Command, parse};

    fn ask(a: &[&str]) -> std::result::Result<crate::args::AskArgs, clap::Error> {
        let args: Vec<OsString> = std::iter::once("ask").chain(a.iter().copied()).map(OsString::from).collect();
        match parse(&args)?.command {
            Command::Ask(a) => Ok(a),
            _ => Err(clap::Error::new(clap::error::ErrorKind::InvalidSubcommand)),
        }
    }

    #[test]
    fn ask_takes_one_prompt_and_options() {
        let a = ask(&["draw a circle", "--sample", "--model", "m", "--max-steps", "5", "--yes", "--save", "o.dxf"]).unwrap();
        assert_eq!(a.prompt, "draw a circle");
        assert!(a.sample && a.yes);
        assert_eq!((a.model.as_deref(), a.max_steps, a.save.as_deref()), (Some("m"), Some(5), Some(std::path::Path::new("o.dxf"))));
        assert!(ask(&["p", "-y"]).unwrap().yes);
    }

    #[test]
    fn ask_rejects_bad_arguments() {
        for bad in [
            &[][..],
            &["a", "b"],
            &["p", "--model"],
            &["p", "--max-steps", "0"],
            &["p", "--max-steps", "21"],
            &["p", "--max-steps", "99999999999999999999999"],
            &["p", "--max-steps", "-1"],
            &["p", "--sample", "--metric"],
            &["p", "--open", "a.dxf", "--sample"],
            &["p", "--frobnicate"],
        ] {
            assert!(ask(bad).is_err(), "{bad:?}");
        }
    }

    /// `--save` or `--export` onto the `--open` file needs `--in-place`, as in `run`.
    #[test]
    fn ask_wont_replace_the_open_file_unasked() {
        let dir = std::env::temp_dir().join(format!("cadcraft-ask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.dxf");
        std::fs::write(
            &input, "0
EOF
",
        )
        .unwrap();
        let input = input.to_string_lossy().to_string();
        let args = |extra: &[&str]| -> Vec<OsString> {
            ["ask", "p", "--open", input.as_str(), "--save", input.as_str()].iter().chain(extra).map(OsString::from).collect()
        };
        let e = parse(&args(&[])).err().unwrap();
        assert!(e.to_string().contains("--in-place"), "{e}");
        assert!(parse(&args(&["--in-place"])).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// No key and a blank prompt are refused before anything is opened or sent.
    #[test]
    fn ask_needs_a_prompt_and_a_key() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(crate::run(["ask", "  "], &mut out, &mut err), 1);
        assert!(String::from_utf8_lossy(&err).contains("needs a prompt"), "{}", String::from_utf8_lossy(&err));
    }
}
