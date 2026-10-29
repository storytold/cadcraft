//! Boolean environment switches (`CADCRAFT_VSYNC`, `CADCRAFT_WAYLAND`, ...): one parser, so every
//! variable accepts the same spellings.

/// `1/on/true/yes` or `0/off/false/no`, case- and space-insensitive. Blank or unset is `Default`;
/// anything else is `Default` and logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    Default,
}

impl Switch {
    /// Parse the value of the variable `name`; `name` only appears in the warning.
    pub fn parse(name: &str, value: Option<&str>) -> Switch {
        let Some(value) = value else { return Switch::Default };
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "on" | "true" | "yes" => Switch::On,
            "0" | "off" | "false" | "no" => Switch::Off,
            "" => Switch::Default,
            _ => {
                log::warn!("{name}={value:?} is not 1/on/true/yes or 0/off/false/no; using the default");
                Switch::Default
            }
        }
    }

    /// Whether the switch is on, with `default` for `Default`.
    pub fn or(self, default: bool) -> bool {
        match self {
            Switch::On => true,
            Switch::Off => false,
            Switch::Default => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_spelling() {
        for v in ["1", "on", "true", "yes", "ON", "True", " Yes ", "\t1\n"] {
            assert_eq!(Switch::parse("X", Some(v)), Switch::On, "{v:?}");
        }
        for v in ["0", "off", "false", "no", "OFF", "False", " No ", " 0 "] {
            assert_eq!(Switch::parse("X", Some(v)), Switch::Off, "{v:?}");
        }
        for v in [None, Some(""), Some("   "), Some("maybe"), Some("2"), Some("onn")] {
            assert_eq!(Switch::parse("X", v), Switch::Default, "{v:?}");
        }
    }
}
