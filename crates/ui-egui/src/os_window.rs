//! The app id and the channel name every CADCraft OS window and caption carries, plus the
//! startup identity the desktop selects before any UI service exists.
//!
//! The identity is immutable once selected: a stable build and a nightly build never share a
//! window class, AUMID or app id, so docks, window rules and the taskbar keep them apart.

/// Matches the `.desktop` file and hicolor icon name, so Wayland docks pick up the icon.
pub const APP_ID: &str = "ai.storyteller.cadcraft";

/// Immutable startup identity. The desktop selects it before any UI/runtime service is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchIdentity {
    Stable,
    NightlyV1,
}

static IDENTITY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn select_identity(identity: LaunchIdentity) -> Result<(), String> {
    let value = match identity {
        LaunchIdentity::Stable => 1,
        LaunchIdentity::NightlyV1 => 2,
    };
    IDENTITY
        .compare_exchange(0, value, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| "the window launch identity was already selected".to_string())
}

pub fn identity() -> LaunchIdentity {
    if IDENTITY.load(std::sync::atomic::Ordering::Acquire) == 2 { LaunchIdentity::NightlyV1 } else { LaunchIdentity::Stable }
}

pub const fn id_for(identity: LaunchIdentity) -> &'static str {
    match identity {
        LaunchIdentity::Stable => APP_ID,
        LaunchIdentity::NightlyV1 => "ai.storyteller.cadcraft.nightly-v1",
    }
}

pub fn app_id() -> &'static str {
    id_for(identity())
}

pub const fn name_for(identity: LaunchIdentity) -> &'static str {
    match identity {
        LaunchIdentity::Stable => "CADCraft",
        LaunchIdentity::NightlyV1 => "CADCraft Nightly",
    }
}

/// The drawn caption for the window ([`crate::chrome`]): stable draws its name exactly as before.
pub fn drawn_name() -> &'static str {
    name_for(identity())
}

/// Whether the stable-only services (updater, control server, agent links) may run this launch.
pub const fn stable_services_for(identity: LaunchIdentity) -> bool {
    matches!(identity, LaunchIdentity::Stable)
}

pub fn stable_services_allowed() -> bool {
    stable_services_for(identity())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nightly_identity_is_distinct_and_never_enables_stable_services() {
        assert_eq!(id_for(LaunchIdentity::Stable), APP_ID);
        assert_ne!(id_for(LaunchIdentity::NightlyV1), APP_ID);
        assert_eq!(name_for(LaunchIdentity::Stable), "CADCraft");
        assert_eq!(name_for(LaunchIdentity::NightlyV1), "CADCraft Nightly");
        assert!(stable_services_for(LaunchIdentity::Stable));
        assert!(!stable_services_for(LaunchIdentity::NightlyV1));
    }

    #[test]
    fn the_window_id_differs_from_the_package_identity() {
        // Two strings on purpose: the package/bundle id is `...cadcraft.nightly`, the window's is
        // `...cadcraft.nightly-v1` (see the nightly contract).
        assert_eq!(id_for(LaunchIdentity::NightlyV1), "ai.storyteller.cadcraft.nightly-v1");
    }
}
