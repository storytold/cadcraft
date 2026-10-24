//! The launcher's activation token for the first window (CADCraft patch, from PdfCraft).
//!
//! A launcher with `StartupNotify=true` passes an activation token in the environment
//! (`XDG_ACTIVATION_TOKEN` on Wayland, `DESKTOP_STARTUP_ID` on X11) and shows a busy cursor
//! until a window hands it back. winit hands back only a token given through
//! `WindowAttributesExtStartupNotify::with_activation_token`, and eframe never gives one, so
//! GNOME kept the busy cursor until its 15-second timeout and the window opened without focus.
//!
//! The first window the process creates without a token now takes the one in the environment.
//! Later windows never do: the token is spent once a window has handed it back. The variable is
//! left in the environment, unlike `reset_activation_token_env`, because changing the
//! environment while other threads may read it is unsound, and an application can start
//! threads before its first window.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::window::ActivationToken;

/// Longer tokens are ignored. Launchers make tokens of a few dozen bytes; on X11 the token is
/// written to a window property and sent to the window manager, and an oversized one would
/// fail window creation.
const MAX_TOKEN_LEN: usize = 1024;

static TAKEN: AtomicBool = AtomicBool::new(false);

/// The token for a window being created: `given` if there is one, else the environment's
/// value of `var` for the process's first window.
pub(crate) fn for_new_window(given: Option<ActivationToken>, var: &str) -> Option<ActivationToken> {
    choose(given, &TAKEN, || std::env::var(var).ok())
}

fn choose(given: Option<ActivationToken>, taken: &AtomicBool, read: impl FnOnce() -> Option<String>) -> Option<ActivationToken> {
    // The first window spends the launcher's token, whether or not it was given one.
    let first = !taken.swap(true, Ordering::Relaxed);
    if given.is_some() || !first {
        return given;
    }
    read().filter(|token| !token.is_empty() && token.len() <= MAX_TOKEN_LEN).map(ActivationToken::from_raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(raw: &str) -> Option<ActivationToken> {
        Some(ActivationToken::from_raw(raw.to_owned()))
    }

    #[test]
    fn the_first_window_takes_the_launchers_token() {
        let taken = AtomicBool::new(false);
        assert_eq!(choose(None, &taken, || Some("gnome-shell/PdfCraft/1234".into())), token("gnome-shell/PdfCraft/1234"));
    }

    #[test]
    fn a_later_window_does_not_reuse_the_spent_token() {
        let taken = AtomicBool::new(false);
        assert!(choose(None, &taken, || Some("t".into())).is_some());
        assert_eq!(choose(None, &taken, || Some("t".into())), None);
    }

    #[test]
    fn a_given_token_wins_and_still_spends_the_launchers() {
        let taken = AtomicBool::new(false);
        assert_eq!(choose(token("mine"), &taken, || Some("env".into())), token("mine"));
        assert_eq!(choose(None, &taken, || Some("env".into())), None);
        assert_eq!(choose(token("again"), &taken, || Some("env".into())), token("again"));
    }

    #[test]
    fn a_missing_empty_or_oversized_token_is_ignored() {
        assert_eq!(choose(None, &AtomicBool::new(false), || None), None);
        assert_eq!(choose(None, &AtomicBool::new(false), || Some(String::new())), None);
        let long = "x".repeat(MAX_TOKEN_LEN + 1);
        assert_eq!(choose(None, &AtomicBool::new(false), || Some(long)), None);
        let limit = "x".repeat(MAX_TOKEN_LEN);
        assert!(choose(None, &AtomicBool::new(false), || Some(limit)).is_some());
    }
}
