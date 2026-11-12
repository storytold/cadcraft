//! System printers on Windows: the spooler's printers, each with the papers its driver offers
//! and their printable areas (from the driver's PrintCapabilities), and printing a plot's PDF
//! through craft-print (Windows.Data.Pdf drawn as vectors into the XPS print API, no program
//! started). `winprint` and `winsafe` wrap the Win32 calls, so there is no `unsafe` here.
//!
//! The PDF is the one the PDF plotter writes for the chosen paper, so it is printed at its exact
//! size: craft-print picks the driver's paper of that size and refuses a driver that would scale
//! it. The conversions take plain values, so they are tested without a printer.

use cadcraft_doc::{DevicePaper, Orientation, PlotOutput, PlotterConfig};

/// Most printers listed.
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_PRINTERS: usize = 64;
/// Most papers of one printer.
const MAX_PAPERS: usize = 256;
/// Most copies of one job.
pub const MAX_COPIES: u32 = 999;

/// A paper of a printer driver: its PrintSchema option name (`ISOA4`), the name shown, its size
/// in micrometres and, when the driver said, its unprintable margins in millimetres (left,
/// bottom, right, top).
#[derive(Clone, Debug, PartialEq)]
pub struct WinPaper {
    pub key: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub margins: Option<[f64; 4]>,
}

/// The unprintable margins (left, bottom, right, top, in mm) from a driver's
/// `PageImageableSize`: the page size, and the printable area's origin (from the top left) and
/// extent, all in micrometres. `None` when the area is not on the page.
pub fn margins_from_imageable(size: (u32, u32), origin: (u32, u32), extent: (u32, u32)) -> Option<[f64; 4]> {
    let (w, h) = size;
    let right = w.checked_sub(origin.0)?.checked_sub(extent.0)?;
    let bottom = h.checked_sub(origin.1)?.checked_sub(extent.1)?;
    if w == 0 || h == 0 || extent.0 == 0 || extent.1 == 0 {
        return None;
    }
    let mm = |um: u32| f64::from(um) / 1000.0;
    Some([mm(origin.0), mm(bottom), mm(right), mm(origin.1)])
}

/// The device a printer becomes: a system printer (PDF in, printed by craft-print) with its
/// driver's papers. Roll papers (no length), unnamed papers and repeated names are dropped; the
/// default paper is the driver's default (`default_key`) when it is one of them, else the first.
pub fn printer_config(name: &str, default_printer: bool, papers: &[WinPaper], default_key: Option<&str>) -> PlotterConfig {
    let mut device_papers: Vec<DevicePaper> = Vec::new();
    let mut default_paper = String::new();
    for p in papers {
        if device_papers.len() >= MAX_PAPERS {
            break;
        }
        if p.width == 0 || p.height == 0 || p.name.is_empty() || device_papers.iter().any(|d| d.name.eq_ignore_ascii_case(&p.name)) {
            continue;
        }
        if default_paper.is_empty() && default_key.is_some_and(|k| k == p.key) {
            default_paper = p.name.clone();
        }
        device_papers.push(DevicePaper {
            name: p.name.clone(),
            width: f64::from(p.width) / 1000.0,
            height: f64::from(p.height) / 1000.0,
            margins: p.margins.unwrap_or([0.0; 4]),
        });
    }
    if default_paper.is_empty() {
        default_paper = device_papers.first().map(|p| p.name.clone()).unwrap_or_default();
    }
    let description = if default_printer { "Windows printer (default)" } else { "Windows printer" };
    PlotterConfig {
        name: name.to_string(),
        description: description.to_string(),
        output: PlotOutput::Pdf,
        resolution: 300,
        orientation: Orientation::Landscape,
        default_paper,
        papers: device_papers,
        system: true,
        system_default: default_printer,
    }
    .sanitized()
}

#[cfg(windows)]
mod system {
    use super::{MAX_COPIES, MAX_PAPERS, MAX_PRINTERS, PlotterConfig, WinPaper, margins_from_imageable, printer_config};
    use winprint::printer::PrinterDevice;
    use winprint::ticket::document::{PrintTicketDocument, reader::ParsableXmlDocument as _};
    use winprint::ticket::{FeatureOptionPack as _, PageImageableSize, PageMediaSize, PrintCapabilities, PrintTicketBuilder};

    /// Papers whose printable area is asked for (one driver call each); later ones print to the
    /// edge as far as the dialog knows.
    const MAX_MARGIN_QUERIES: usize = 64;

    /// The option name of the paper in the driver's default ticket (`ISOA4`).
    fn default_paper_key(device: &PrinterDevice) -> Option<String> {
        let ticket = PrintTicketBuilder::new(device).ok()?.build().ok()?;
        let document = PrintTicketDocument::parse_from_bytes(ticket.get_xml()).ok()?;
        let feature = document.features.iter().find(|f| f.name.local_name == "PageMediaSize")?;
        Some(feature.options.first()?.name.as_ref()?.local_name.clone())
    }

    /// One driver paper with its size and, when asked, its printable area.
    fn paper(device: &PrinterDevice, media: &PageMediaSize, ask_margins: bool) -> WinPaper {
        let size = media.size();
        let key = media.option().name.as_ref().map(|n| n.local_name.clone()).unwrap_or_default();
        let name = media.display_name().map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| key.clone(), str::to_string);
        let margins = ask_margins.then(|| PageImageableSize::try_fetch(device, media.clone()).ok()).flatten().and_then(|a| {
            margins_from_imageable(
                (size.width_in_micron(), size.height_in_micron()),
                (a.origin.width_in_micron(), a.origin.height_in_micron()),
                (a.extent.width_in_micron(), a.extent.height_in_micron()),
            )
        });
        WinPaper { key, name, width: size.width_in_micron(), height: size.height_in_micron(), margins }
    }

    /// Threads asking one driver for printable areas at once: each answer is a whole
    /// PrintCapabilities document, so a driver with many papers takes seconds one after another.
    const MARGIN_THREADS: usize = 8;

    /// A Windows printer as a device: its driver's papers (no rolls), their printable areas asked on a few threads.
    fn printer(device: &PrinterDevice, default_printer: bool) -> Option<PlotterConfig> {
        let capabilities = PrintCapabilities::fetch(device).ok()?;
        let medias: Vec<PageMediaSize> = capabilities.page_media_sizes().filter(|m| !m.size().is_roll()).take(MAX_PAPERS).collect();
        // Every MARGIN_THREADS-th paper on each thread; a thread that fails to start or to finish
        // leaves its papers out of the margin queries, not out of the list.
        let mut margins: Vec<Option<WinPaper>> = vec![None; medias.len()];
        std::thread::scope(|s| {
            let workers: Vec<_> = (0..MARGIN_THREADS.min(medias.len()))
                .filter_map(|t| {
                    let medias = &medias;
                    std::thread::Builder::new()
                        .name("printer-papers".into())
                        .spawn_scoped(s, move || {
                            (t..medias.len().min(MAX_MARGIN_QUERIES))
                                .step_by(MARGIN_THREADS)
                                .filter_map(|i| Some((i, paper(device, medias.get(i)?, true))))
                                .collect::<Vec<_>>()
                        })
                        .ok()
                })
                .collect();
            for w in workers {
                for (i, p) in w.join().unwrap_or_default() {
                    if let Some(slot) = margins.get_mut(i) {
                        *slot = Some(p);
                    }
                }
            }
        });
        let papers: Vec<WinPaper> = medias.iter().zip(margins).map(|(m, done)| done.unwrap_or_else(|| paper(device, m, false))).collect();
        let c = printer_config(device.name(), default_printer, &papers, default_paper_key(device).as_deref());
        (!c.papers.is_empty()).then_some(c)
    }

    /// The spooler's printers as devices (none when there is no spooler), each asked on a thread
    /// of its own. A printer whose driver gives no papers is left out.
    pub fn printers() -> Vec<PlotterConfig> {
        let default = winsafe::GetDefaultPrinter().ok().filter(|n| !n.is_empty());
        let Ok(devices) = PrinterDevice::all() else { return Vec::new() };
        let default = default.as_deref();
        std::thread::scope(|s| {
            let workers: Vec<_> = devices
                .iter()
                .take(MAX_PRINTERS)
                .filter_map(|d| {
                    std::thread::Builder::new().name("printer".into()).spawn_scoped(s, move || printer(d, default == Some(d.name()))).ok()
                })
                .collect();
            workers.into_iter().filter_map(|w| w.join().ok().flatten()).collect()
        })
    }

    /// Print `pdf` on `printer`. The paper is the PDF's own sheet size: craft-print chooses the
    /// driver's paper of that size, so `paper` only names it. Returns the spooler's reply.
    pub fn print(printer: &str, paper: &str, copies: u32, title: &str, pdf: &[u8]) -> Result<String, String> {
        let _ = paper;
        let title: String = title.chars().filter(|c| !c.is_control()).take(100).collect();
        let job = craft_print::Job {
            printer: Some(printer.to_string()),
            copies: copies.clamp(1, MAX_COPIES),
            collate: true,
            // The printer's own duplex default.
            duplex: None,
            // The plot is already at its scale: no fitting to the page.
            scaling: craft_print::Scaling::Exact,
            grayscale: false,
            title: if title.trim().is_empty() { "CADCraft".into() } else { title },
            options: Vec::new(),
        };
        craft_print::submit(pdf, &job).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
pub use system::{print, printers};

#[cfg(test)]
mod tests {
    use super::*;

    /// This machine's printers and papers, and how long listing them takes
    /// (`cargo test -p cadcraft-io --lib winprint -- --ignored --nocapture`).
    #[cfg(windows)]
    #[test]
    #[ignore = "asks this machine's printer drivers"]
    fn list_this_machines_printers() {
        let start = std::time::Instant::now();
        let names: Vec<String> =
            winprint::printer::PrinterDevice::all().map(|d| d.iter().map(|p| p.name().to_string()).collect()).unwrap_or_default();
        println!("spooler: {names:?} ({:?})", start.elapsed());
        for c in printers() {
            println!("{} [{}] default paper {:?}, {} papers", c.name, c.description, c.default_paper, c.papers.len());
            for p in c.papers.iter().take(8) {
                println!("    {} {:.1} x {:.1} mm, margins {:?}", p.name, p.width, p.height, p.margins);
            }
        }
        println!("listed in {:?}", start.elapsed());
    }

    /// The printable area's origin and size become left, bottom, right and top margins in millimetres.
    #[test]
    fn margins_from_the_printable_area() {
        // A4 with 3.4 mm unprintable left and right, 3 mm at the top and 5 mm at the bottom.
        let m = margins_from_imageable((210_000, 297_000), (3_400, 3_000), (203_200, 289_000)).unwrap();
        let want = [3.4, 5.0, 3.4, 3.0];
        for (a, b) in m.iter().zip(want) {
            assert!((a - b).abs() < 1e-9, "{m:?}");
        }
        // Borderless.
        assert_eq!(margins_from_imageable((210_000, 297_000), (0, 0), (210_000, 297_000)), Some([0.0; 4]));
        // An area past the page's edge, or none.
        assert_eq!(margins_from_imageable((210_000, 297_000), (5_000, 0), (210_000, 297_000)), None);
        assert_eq!(margins_from_imageable((210_000, 297_000), (0, 0), (0, 0)), None);
    }

    /// Driver papers become device papers in mm with their margins, duplicate names and rolls left
    /// out; the default printer is marked, and an unknown default paper falls back to the first.
    #[test]
    fn driver_papers_become_a_device() {
        let p = |key: &str, name: &str, width, height, margins| WinPaper { key: key.into(), name: name.into(), width, height, margins };
        let papers = [
            p("NorthAmericaLetter", "Letter", 215_900, 279_400, Some([3.4, 5.0, 3.4, 3.0])),
            p("ISOA4", "A4", 210_000, 297_000, None),
            p("Custom1", "a4", 210_000, 297_000, None),
            p("Roll", "Roll 24 in", 609_600, 0, None),
            p("ISOA3", "A3", 297_000, 420_000, None),
        ];
        let c = printer_config("HP LaserJet", true, &papers, Some("ISOA4"));
        assert!(c.system);
        assert_eq!(c.output, PlotOutput::Pdf);
        assert_eq!(c.description, "Windows printer (default)");
        assert!(c.system_default);
        let names: Vec<&str> = c.papers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Letter", "A4", "A3"]);
        assert_eq!(c.default_paper, "A4");
        let a4 = c.paper("A4").unwrap();
        assert!((a4.width - 210.0).abs() < 1e-9 && (a4.height - 297.0).abs() < 1e-9);
        assert_eq!(a4.margins, [0.0; 4]);
        assert_eq!(c.paper("Letter").unwrap().margins, [3.4, 5.0, 3.4, 3.0]);
        // An unknown default falls back to the first paper.
        let other = printer_config("Canon", false, &papers, Some("JapanHagaki"));
        assert_eq!(other.default_paper, "Letter");
        assert_eq!(other.description, "Windows printer");
    }
}
