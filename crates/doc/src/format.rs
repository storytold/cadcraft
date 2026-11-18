use std::path::Path;

/// A file format, chosen by the file name's extension (ASCII case-insensitive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Dxf,
    Dwg,
    /// A drawing template: DWG content.
    Dwt,
    Svg,
    /// Gzip-compressed SVG.
    Svgz,
    Png,
    Pdf,
}

impl Format {
    /// The formats a drawing can be read from by name. DXF and DWG are also recognised by content.
    pub const READ: &'static [Format] = &[Format::Dxf, Format::Dwg, Format::Svg, Format::Svgz];
    /// The formats a drawing can be written to.
    pub const WRITE: &'static [Format] = &[Format::Dxf, Format::Dwg, Format::Dwt, Format::Svg, Format::Png, Format::Pdf];

    /// The format `path`'s extension names. A name without an extension is DXF; an unknown
    /// extension is `None`.
    pub fn from_path(p: impl AsRef<Path>) -> Option<Format> {
        let ext = p.as_ref().extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        Some(match ext.as_str() {
            "dxf" | "" => Format::Dxf,
            "dwg" => Format::Dwg,
            "dwt" => Format::Dwt,
            "svg" => Format::Svg,
            "svgz" => Format::Svgz,
            "png" => Format::Png,
            "pdf" => Format::Pdf,
            _ => return None,
        })
    }

    /// The extension, lower case and without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Dxf => "dxf",
            Format::Dwg => "dwg",
            Format::Dwt => "dwt",
            Format::Svg => "svg",
            Format::Svgz => "svgz",
            Format::Png => "png",
            Format::Pdf => "pdf",
        }
    }

    /// Written as an image (model space fitted to a frame), so framing applies. SVGZ is read only.
    pub fn is_image(self) -> bool {
        matches!(self, Format::Svg | Format::Png | Format::Pdf)
    }

    /// A drawing file (DXF, DWG, template) rather than an image.
    pub fn is_drawing(self) -> bool {
        matches!(self, Format::Dxf | Format::Dwg | Format::Dwt)
    }

    /// Written as DWG, which drops the object types the DWG writer doesn't know.
    pub fn drops_unknown(self) -> bool {
        matches!(self, Format::Dwg | Format::Dwt)
    }

    /// `formats` as "dxf, dwg, svg": the list help text shows.
    pub fn list(formats: &[Format]) -> String {
        formats.iter().map(|f| f.extension()).collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_by_extension() {
        for f in [Format::Dxf, Format::Dwg, Format::Dwt, Format::Svg, Format::Svgz, Format::Png, Format::Pdf] {
            assert_eq!(Format::from_path(format!("a/b.{}", f.extension())), Some(f));
            assert_eq!(Format::from_path(format!("B.{}", f.extension().to_ascii_uppercase())), Some(f));
        }
        assert_eq!(Format::from_path("Drawing1"), Some(Format::Dxf), "no extension is DXF");
        assert_eq!(Format::from_path("dir.dwg/Drawing1"), Some(Format::Dxf));
        assert_eq!(Format::from_path("a."), Some(Format::Dxf));
        assert_eq!(Format::from_path("a.txt"), None);
        assert_eq!(Format::from_path("a.dxf.bak"), None);
        assert_eq!(Format::from_path(std::path::Path::new("x.PnG")), Some(Format::Png));
    }

    #[test]
    fn classes() {
        assert!(Format::READ.contains(&Format::Svgz) && !Format::WRITE.contains(&Format::Svgz));
        assert!(Format::WRITE.contains(&Format::Dwt) && !Format::READ.contains(&Format::Dwt));
        assert!(Format::WRITE.iter().filter(|f| f.is_image()).count() == 3 && !Format::Svgz.is_image());
        assert!(Format::Dwt.drops_unknown() && Format::Dwg.drops_unknown() && !Format::Dxf.drops_unknown());
        assert!(Format::Dwt.is_drawing() && !Format::Pdf.is_drawing());
        assert_eq!(Format::list(Format::READ), "dxf, dwg, svg, svgz");
    }
}
