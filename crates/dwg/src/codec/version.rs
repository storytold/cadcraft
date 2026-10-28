//! DWG version codes.
//!
//! Spec: ODA Open Design Specification for .dwg files v5.4.1, §3.2.1
//! "VERSION ID" (verified against the spec text).

use std::fmt;

/// The DWG format generations. Ordering follows release order, so
/// `v >= DwgVersion::R2000` reads as "R2000 or later".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DwgVersion {
    /// AC1012 — AutoCAD Release 13
    R13,
    /// AC1014 — AutoCAD Release 14
    R14,
    /// AC1015 — AutoCAD 2000/2000i/2002 (the spec also calls this R15)
    R2000,
    /// AC1018 — AutoCAD 2004/2005/2006
    R2004,
    /// AC1021 — AutoCAD 2007/2008/2009
    R2007,
    /// AC1024 — AutoCAD 2010/2011/2012
    R2010,
    /// AC1027 — AutoCAD 2013 through 2017
    R2013,
    /// AC1032 — AutoCAD 2018 and later (still current in 2027)
    R2018,
}

impl DwgVersion {
    /// Parse the six-byte ASCII version tag at the start of the file.
    pub fn from_magic(magic: &[u8]) -> Option<DwgVersion> {
        let tag = magic.get(..6)?;
        Some(match tag {
            b"AC1012" => DwgVersion::R13,
            b"AC1014" => DwgVersion::R14,
            b"AC1015" => DwgVersion::R2000,
            b"AC1018" => DwgVersion::R2004,
            b"AC1021" => DwgVersion::R2007,
            b"AC1024" => DwgVersion::R2010,
            b"AC1027" => DwgVersion::R2013,
            b"AC1032" => DwgVersion::R2018,
            _ => return None,
        })
    }

    /// Parse a version tag stored on its own (as the native format stores
    /// it): exactly the six characters, nothing after them.
    pub fn from_tag(tag: &str) -> Option<DwgVersion> {
        if tag.len() == 6 { DwgVersion::from_magic(tag.as_bytes()) } else { None }
    }

    pub fn magic(self) -> &'static str {
        match self {
            DwgVersion::R13 => "AC1012",
            DwgVersion::R14 => "AC1014",
            DwgVersion::R2000 => "AC1015",
            DwgVersion::R2004 => "AC1018",
            DwgVersion::R2007 => "AC1021",
            DwgVersion::R2010 => "AC1024",
            DwgVersion::R2013 => "AC1027",
            DwgVersion::R2018 => "AC1032",
        }
    }

    /// Human-readable release range.
    pub fn releases(self) -> &'static str {
        match self {
            DwgVersion::R13 => "AutoCAD R13",
            DwgVersion::R14 => "AutoCAD R14",
            DwgVersion::R2000 => "AutoCAD 2000–2002",
            DwgVersion::R2004 => "AutoCAD 2004–2006",
            DwgVersion::R2007 => "AutoCAD 2007–2009",
            DwgVersion::R2010 => "AutoCAD 2010–2012",
            DwgVersion::R2013 => "AutoCAD 2013–2017",
            DwgVersion::R2018 => "AutoCAD 2018 and later",
        }
    }

    /// R13–R15 use the flat "section locator" container; R2004 and later
    /// use the paged, compressed container (R2007 has its own variant).
    pub fn uses_paged_container(self) -> bool {
        self >= DwgVersion::R2004
    }

    /// From R2007 on, object strings live in a separate string stream and
    /// are UTF-16.
    pub fn has_string_stream(self) -> bool {
        self >= DwgVersion::R2007
    }
}

impl fmt::Display for DwgVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.magic(), self.releases())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_round_trip() {
        for v in [
            DwgVersion::R13,
            DwgVersion::R14,
            DwgVersion::R2000,
            DwgVersion::R2004,
            DwgVersion::R2007,
            DwgVersion::R2010,
            DwgVersion::R2013,
            DwgVersion::R2018,
        ] {
            assert_eq!(DwgVersion::from_magic(v.magic().as_bytes()), Some(v));
        }
        assert_eq!(DwgVersion::from_magic(b"AC1009"), None);
        assert_eq!(DwgVersion::from_magic(b"AC10"), None);
        assert_eq!(DwgVersion::from_tag("AC1018"), Some(DwgVersion::R2004));
        assert_eq!(DwgVersion::from_tag("AC1018-not-a-version"), None);
    }

    #[test]
    fn ordering() {
        assert!(DwgVersion::R2018 > DwgVersion::R2004);
        assert!(DwgVersion::R2000 < DwgVersion::R2004);
        assert!(DwgVersion::R2004.uses_paged_container());
        assert!(!DwgVersion::R2000.uses_paged_container());
    }
}
