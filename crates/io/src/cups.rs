//! System printers through CUPS (macOS and Linux): the destinations `lpstat` lists, each with
//! the papers and printable areas of its PPD, and printing a plot's PDF with `lp`. A destination
//! without a PPD file the user can read (a driverless IPP queue CUPS made on the fly, or a PPD
//! readable only by root) takes its papers from `lpoptions -l` instead, with the margins IPP
//! printers commonly report. `lpstat -d` names the system's default printer.
//!
//! The PDF is the one the PDF plotter writes for the chosen paper, so it is sent unscaled: the
//! plot's scale is already applied. The parsers take text, so they are tested without a printer.

use cadcraft_doc::{DevicePaper, Orientation, PlotOutput, PlotterConfig};

/// Most destinations listed.
const MAX_PRINTERS: usize = 64;
/// Largest PPD file read (bytes).
#[cfg_attr(not(all(unix, not(target_arch = "wasm32"))), allow(dead_code))]
const MAX_PPD: u64 = 4 << 20;
/// Most copies of one job.
pub const MAX_COPIES: u32 = 999;
/// PostScript points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// A paper size of a PPD: its option keyword, the name shown, size and printable area (points).
#[derive(Clone, Debug, PartialEq)]
pub struct PpdPaper {
    pub keyword: String,
    pub name: String,
    pub width: f64,
    pub height: f64,
    /// Printable area: lower-left x, y and upper-right x, y (points).
    pub imageable: Option<[f64; 4]>,
}

/// What a PPD says about a printer's papers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ppd {
    pub nick_name: String,
    /// The default paper's keyword.
    pub default_size: String,
    pub papers: Vec<PpdPaper>,
}

/// A PPD main keyword line: `*Key Option/Label: Value` → (key, option, label, value).
fn ppd_line(line: &str) -> Option<(&str, &str, &str, &str)> {
    let rest = line.strip_prefix('*')?;
    let (head, value) = rest.split_once(':')?;
    let (key, option) = head.split_once(' ').unwrap_or((head, ""));
    let (option, label) = option.split_once('/').unwrap_or((option, ""));
    Some((key.trim(), option.trim(), label.trim(), value.trim().trim_matches('"').trim()))
}

/// Exactly `N` finite numbers separated by spaces (a PPD's sizes and areas), else `None`.
fn numbers<const N: usize>(s: &str) -> Option<[f64; N]> {
    let v: Vec<f64> = s.split_whitespace().filter_map(|t| t.parse().ok()).filter(|x: &f64| x.is_finite()).collect();
    v.try_into().ok()
}

/// The name a paper is shown with: its label, else its keyword; a borderless variant
/// (`Letter.Fullbleed`) says so.
fn paper_name(keyword: &str, label: &str) -> String {
    let (base, borderless) = match keyword.strip_suffix(".Fullbleed") {
        Some(b) => (b, true),
        None => (keyword, false),
    };
    let shown = if label.is_empty() { base } else { label.trim_end_matches(" (Borderless)").trim_end_matches(" Borderless") };
    if borderless { format!("{shown} (borderless)") } else { shown.to_string() }
}

/// Parse a PPD's nickname, default page size and page sizes (with `PaperDimension` and
/// `ImageableArea`). Page sizes without dimensions are dropped.
pub fn parse_ppd(text: &str) -> Ppd {
    let mut ppd = Ppd::default();
    let mut sizes: Vec<(String, String)> = Vec::new();
    let mut dims: Vec<(String, [f64; 2])> = Vec::new();
    let mut areas: Vec<(String, [f64; 4])> = Vec::new();
    for line in text.lines() {
        let Some((key, option, label, value)) = ppd_line(line) else { continue };
        match key {
            "NickName" => ppd.nick_name = value.to_string(),
            "DefaultPageSize" => ppd.default_size = value.to_string(),
            "PageSize" if !option.is_empty() && !sizes.iter().any(|(k, _)| k == option) => sizes.push((option.into(), label.into())),
            "PaperDimension" => {
                if let Some(d) = numbers::<2>(value) {
                    dims.push((option.into(), d));
                }
            }
            "ImageableArea" => {
                if let Some(a) = numbers::<4>(value) {
                    areas.push((option.into(), a));
                }
            }
            _ => {}
        }
    }
    for (keyword, label) in sizes {
        let Some((_, [w, h])) = dims.iter().find(|(k, _)| *k == keyword) else { continue };
        if *w <= 0.0 || *h <= 0.0 {
            continue;
        }
        let imageable = areas.iter().find(|(k, _)| *k == keyword).map(|(_, a)| *a);
        ppd.papers.push(PpdPaper { name: paper_name(&keyword, &label), keyword, width: *w, height: *h, imageable });
    }
    ppd
}

/// The margin IPP printers commonly report for each edge of a sheet that is not borderless
/// (`media-*-margin` 423, in hundredths of a millimetre), used when nothing better is known.
const IPP_MARGIN_MM: f64 = 4.23;

/// The size of a media keyword `lpoptions -l` lists, in points: a PWG self-describing name
/// (`iso_a4_210x297mm`, `na_letter_8.5x11in`), Adobe's `w612h792` (points), or a standard size
/// CADCraft knows (`A4`, `Letter`, `Legal`, `Tabloid`…, shown by its keyword). `None` for custom
/// and unknown ones.
fn media_size(keyword: &str) -> Option<(String, f64, f64)> {
    let k = keyword.trim();
    if k.is_empty() || k.to_ascii_lowercase().starts_with("custom") {
        return None;
    }
    // PWG 5101.1: `class_name_WxHunit`, the size as the last part.
    if let Some((head, dims)) = k.rsplit_once('_') {
        let (dims, mm) = match (dims.strip_suffix("mm"), dims.strip_suffix("in")) {
            (Some(d), _) => (d, 1.0),
            (None, Some(d)) => (d, 25.4),
            (None, None) => ("", 0.0),
        };
        if let Some((w, h)) = dims.split_once('x')
            && let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>())
            && w.is_finite()
            && h.is_finite()
            && w > 0.0
            && h > 0.0
        {
            // `iso_a4` → "A4", `na_letter` → "Letter": the name part after the class.
            let name = head.split_once('_').map_or(head, |(_, n)| n);
            let shown = if name.is_empty() { k.to_string() } else { pwg_name(name) };
            return Some((shown, w * mm * PT_PER_MM, h * mm * PT_PER_MM));
        }
    }
    if let Some(rest) = k.strip_prefix('w')
        && let Some((w, h)) = rest.split_once('h')
        && let (Ok(w), Ok(h)) = (w.parse::<f64>(), h.parse::<f64>())
        && w.is_finite()
        && h.is_finite()
        && w > 0.0
        && h > 0.0
    {
        return Some((format!("{:.0} x {:.0} mm", w / PT_PER_MM, h / PT_PER_MM), w, h));
    }
    // A PPD's `B4`/`B5` are the JIS sizes, not ISO's: left out rather than guessed.
    let base = k.strip_suffix(".Fullbleed").unwrap_or(k);
    if base.starts_with(['B', 'b']) {
        return None;
    }
    let p = cadcraft_render::paper_size(base)?;
    Some((base.to_string(), p.width_mm * PT_PER_MM, p.height_mm * PT_PER_MM))
}

/// A PWG media name shown as people write it: `a4` → "A4", `letter` → "Letter",
/// `govt-letter` → "Govt Letter".
fn pwg_name(name: &str) -> String {
    let words: Vec<String> = name
        .split(['-', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            // ISO and JIS sizes are written in capitals (A4, B5, C6).
            if w.len() <= 3 && w.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && w.chars().skip(1).all(|c| c.is_ascii_digit()) {
                w.to_ascii_uppercase()
            } else {
                let mut c = w.chars();
                c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
            }
        })
        .collect();
    words.join(" ")
}

/// `lpoptions -p NAME -l` (in the C locale): the `PageSize` (or `media`) choices as papers with
/// [`IPP_MARGIN_MM`] margins, the default (marked `*`) as the default size. Keywords whose size
/// isn't known are left out.
pub fn parse_lpoptions(text: &str) -> Ppd {
    let mut ppd = Ppd::default();
    let line = text
        .lines()
        .find(|l| l.starts_with("PageSize/") || l.starts_with("PageSize:"))
        .or_else(|| text.lines().find(|l| l.starts_with("media/") || l.starts_with("media:")));
    let Some((_, choices)) = line.and_then(|l| l.split_once(':')) else { return ppd };
    let m = IPP_MARGIN_MM * PT_PER_MM;
    for choice in choices.split_whitespace().take(MAX_LPOPTIONS_CHOICES) {
        let (keyword, default) = match choice.strip_prefix('*') {
            Some(k) => (k, true),
            None => (choice, false),
        };
        let Some((name, w, h)) = media_size(keyword) else { continue };
        let borderless = keyword.ends_with(".Fullbleed") || keyword.contains(".Borderless");
        let name = if borderless { format!("{name} (borderless)") } else { name };
        if ppd.papers.iter().any(|p| p.name == name) {
            continue;
        }
        if default {
            ppd.default_size = keyword.to_string();
        }
        let imageable = (!borderless && w > 2.0 * m && h > 2.0 * m).then_some([m, m, w - m, h - m]);
        ppd.papers.push(PpdPaper { keyword: keyword.to_string(), name, width: w, height: h, imageable });
    }
    ppd
}

/// Most `lpoptions` choices read for one option.
const MAX_LPOPTIONS_CHOICES: usize = 500;

/// `lpstat -d` (in the C locale): the default destination, if there is one.
pub fn parse_default_destination(text: &str) -> Option<String> {
    let name = text.lines().find_map(|l| l.trim().strip_prefix("system default destination:"))?.trim();
    valid_name(name).then(|| name.to_string())
}

/// The device a destination becomes: a system printer (PDF in, printed with `lp`) with the
/// PPD's papers.
pub fn printer_config(name: &str, ppd: &Ppd) -> PlotterConfig {
    let papers: Vec<DevicePaper> = ppd
        .papers
        .iter()
        .map(|p| {
            let margins = p.imageable.map_or([0.0; 4], |[llx, lly, urx, ury]| {
                [llx / PT_PER_MM, lly / PT_PER_MM, (p.width - urx) / PT_PER_MM, (p.height - ury) / PT_PER_MM]
            });
            DevicePaper { name: p.name.clone(), width: p.width / PT_PER_MM, height: p.height / PT_PER_MM, margins }
        })
        .collect();
    let default_paper = ppd.papers.iter().find(|p| p.keyword == ppd.default_size).map(|p| p.name.clone()).unwrap_or_default();
    let description = if ppd.nick_name.is_empty() { "System printer".to_string() } else { ppd.nick_name.clone() };
    PlotterConfig {
        name: name.to_string(),
        description,
        output: PlotOutput::Pdf,
        resolution: 300,
        orientation: Orientation::Landscape,
        default_paper,
        papers,
        system: true,
        system_default: false,
    }
    .sanitized()
}

/// `lpstat -e`: one destination per line.
pub fn parse_destinations(text: &str) -> Vec<String> {
    let mut v: Vec<String> = text.lines().map(str::trim).filter(|n| valid_name(n)).map(String::from).collect();
    v.dedup();
    v.truncate(MAX_PRINTERS);
    v
}

/// `lp`'s reply: the job id from `request id is NAME-12 (1 file(s))`.
pub fn parse_request_id(text: &str) -> Option<String> {
    let rest = text.split("request id is ").nth(1)?;
    rest.split_whitespace().next().map(String::from)
}

/// A destination name CUPS could have made (no path separators or control characters).
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 127 && !n.starts_with('.') && !n.contains(['/', '\\', '#']) && !n.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The `lp` option choosing the paper `keyword`: `PageSize` for a paper of the printer's PPD,
/// IPP's `media` for one `lpoptions` listed for a queue without one.
pub fn paper_option(keyword: &str, from_ppd: bool) -> String {
    if from_ppd { format!("PageSize={keyword}") } else { format!("media={keyword}") }
}

/// The `lp` arguments for a job (the PDF goes to standard input); `paper` is a
/// [`paper_option`].
pub fn lp_args(printer: &str, paper: Option<&str>, copies: u32, title: &str) -> Vec<String> {
    let mut a = vec!["-d".into(), printer.into(), "-n".into(), copies.clamp(1, MAX_COPIES).to_string()];
    let title: String = title.chars().filter(|c| !c.is_control()).take(100).collect();
    if !title.trim().is_empty() {
        a.extend(["-t".into(), title]);
    }
    if let Some(o) = paper {
        a.extend(["-o".into(), o.to_string()]);
    }
    // The plot is already at its scale: no fitting to the page.
    a.extend(["-o".into(), "print-scaling=none".into(), "-o".into(), "fit-to-page=false".into(), "-".into()]);
    a
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod system {
    use super::{
        MAX_PPD, PlotterConfig, Ppd, lp_args, paper_option, parse_default_destination, parse_destinations, parse_lpoptions, parse_ppd,
        parse_request_id, printer_config, valid_name,
    };
    use std::io::Write;
    use std::process::{Command, Stdio};

    /// The destination's PPD from the CUPS folder, when it is readable and not too large.
    fn ppd(name: &str) -> Option<Ppd> {
        let path = format!("/etc/cups/ppd/{name}.ppd");
        if std::fs::metadata(&path).ok()?.len() > MAX_PPD {
            return None;
        }
        Some(parse_ppd(&std::fs::read_to_string(path).ok()?))
    }

    /// Longest a CUPS command may take: a CUPS that doesn't answer gives no printers instead of
    /// a listing that never ends.
    const CUPS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    /// Longest `lp` may take to take a job (it carries the whole PDF).
    const LP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    /// A CUPS command's output in the C locale (its messages untranslated, so they parse), or
    /// `None` when it fails or takes longer than [`CUPS_TIMEOUT`] (then it is stopped).
    fn cups_output(program: &str, args: &[&str]) -> Option<String> {
        use std::io::Read;
        let mut child = Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        // Read on a thread, so a full pipe can't keep the command from ending.
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = stdout.take(MAX_PPD).read_to_string(&mut s);
            s
        });
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return reader.join().ok(),
                Ok(Some(_)) | Err(_) => return None,
                Ok(None) if start.elapsed() > CUPS_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
    }

    /// The papers of destination `name`: its PPD's, else (no PPD file the user can read) the
    /// ones `lpoptions` lists. The flag says which.
    fn papers(name: &str) -> Option<(Ppd, bool)> {
        if let Some(p) = ppd(name).filter(|p| !p.papers.is_empty()) {
            return Some((p, true));
        }
        let p = parse_lpoptions(&cups_output("lpoptions", &["-p", name, "-l"])?);
        (!p.papers.is_empty()).then_some((p, false))
    }

    /// The CUPS destinations as devices (none when CUPS is missing), the system's default
    /// printer marked. A destination whose papers can't be read is left out.
    pub fn printers() -> Vec<PlotterConfig> {
        let Some(out) = cups_output("lpstat", &["-e"]) else { return Vec::new() };
        let default = cups_output("lpstat", &["-d"]).and_then(|t| parse_default_destination(&t));
        parse_destinations(&out)
            .into_iter()
            .filter_map(|n| {
                let (p, _) = papers(&n)?;
                let mut c = printer_config(&n, &p);
                c.system_default = default.as_deref() == Some(n.as_str());
                Some(c)
            })
            .collect()
    }

    /// Send `pdf` to `printer` on the paper called `paper` (a name [`printers`] gave).
    /// Returns the job id.
    pub fn print(printer: &str, paper: &str, copies: u32, title: &str, pdf: &[u8]) -> Result<String, String> {
        if !valid_name(printer) {
            return Err(format!("`{printer}` is not a printer name"));
        }
        let option = papers(printer)
            .and_then(|(p, from_ppd)| p.papers.into_iter().find(|x| x.name.eq_ignore_ascii_case(paper)).map(|x| paper_option(&x.keyword, from_ppd)));
        let mut child = Command::new("lp")
            .args(lp_args(printer, option.as_deref(), copies, title))
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("lp: {e}"))?;
        // The PDF goes in and the answers come out on threads, and `lp` is stopped when it doesn't
        // finish in time, so a CUPS that stopped answering can't hold PLOT (and the window) up.
        let writer = child.stdin.take().map(|mut stdin| {
            let pdf = pdf.to_vec();
            std::thread::spawn(move || stdin.write_all(&pdf))
        });
        let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(p) = pipe {
                    let _ = std::io::Read::read_to_string(&mut std::io::Read::take(p, 64 * 1024), &mut s);
                }
                s
            })
        };
        let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
        let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
        let start = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Err(e) => return Err(format!("lp: {e}")),
                Ok(None) if start.elapsed() > LP_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("lp did not answer within {} s", LP_TIMEOUT.as_secs()));
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        };
        if let Some(Ok(Err(e))) = writer.map(std::thread::JoinHandle::join) {
            return Err(format!("lp: {e}"));
        }
        let said = out.join().unwrap_or_default();
        if !status.success() {
            let err = err.join().unwrap_or_default().trim().to_string();
            return Err(if err.is_empty() { format!("lp failed ({status})") } else { err });
        }
        Ok(parse_request_id(&said).unwrap_or_else(|| said.trim().to_string()))
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub use system::{print, printers};

#[cfg(test)]
mod tests {
    use super::*;

    const PPD: &str = r#"*PPD-Adobe: "4.3"
*NickName: "EPSON Example Series-AirPrint"
*OpenUI *PageSize/Media Size: PickOne
*DefaultPageSize: Letter
*PageSize A4: "<</PageSize[595.2756 841.8898]>>setpagedevice"
*PageSize Letter/US Letter: "<</PageSize[612 792]>>setpagedevice"
*PageSize Letter.Fullbleed: "<</PageSize[612 792]>>setpagedevice"
*PageSize Roll610/Roll 24 in: "<</PageSize[1728 2592]>>setpagedevice"
*PageSize NoDims: "<</PageSize[1 1]>>setpagedevice"
*CloseUI: *PageSize
*PageRegion A4: "<</PageSize[595.2756 841.8898]>>setpagedevice"
*PaperDimension A4: "595.2756 841.8898"
*ImageableArea A4: "8.5039 8.5039 586.7717 833.3858"
*PaperDimension Letter/US Letter: "612.0000 792.0000"
*ImageableArea Letter/US Letter: "18 36 594 756"
*PaperDimension Letter.Fullbleed: "612 792"
*ImageableArea Letter.Fullbleed: "0 0 612 792"
*PaperDimension Roll610: "1728 2592"
"#;

    /// A PPD gives the papers with their sizes and printable areas, borderless and roll papers named as such.
    #[test]
    fn ppd_papers_sizes_and_printable_areas() {
        let p = parse_ppd(PPD);
        assert_eq!(p.nick_name, "EPSON Example Series-AirPrint");
        assert_eq!(p.default_size, "Letter");
        let names: Vec<&str> = p.papers.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["A4", "US Letter", "Letter (borderless)", "Roll 24 in"]);
        let c = printer_config("EPSON_Example", &p);
        assert!(c.system);
        assert_eq!(c.output, PlotOutput::Pdf);
        assert_eq!(c.default_paper, "US Letter");
        let letter = c.paper("US Letter").unwrap();
        assert!((letter.width - 215.9).abs() < 0.01 && (letter.height - 279.4).abs() < 0.01);
        let [l, b, r, t] = letter.margins;
        assert!((l - 6.35).abs() < 0.01 && (b - 12.7).abs() < 0.01 && (r - 6.35).abs() < 0.01 && (t - 12.7).abs() < 0.01);
        assert_eq!(c.paper("Letter (borderless)").unwrap().margins, [0.0; 4]);
        // No ImageableArea: the whole sheet.
        assert_eq!(c.paper("Roll 24 in").unwrap().margins, [0.0; 4]);
    }

    /// Destination names and job ids are read from lpstat and lp, and the lp arguments are safe and complete.
    #[test]
    fn lpstat_and_lp_replies() {
        assert_eq!(parse_destinations("EPSON_A\nEPSON_B\n\nbad/name\n"), ["EPSON_A", "EPSON_B"]);
        assert_eq!(parse_request_id("request id is EPSON_A-42 (1 file(s))\n").as_deref(), Some("EPSON_A-42"));
        assert_eq!(parse_request_id("nothing"), None);
        let a = lp_args("EPSON_A", Some(&paper_option("Letter", true)), 0, "Plan\u{7}.dwg - Model");
        assert_eq!(
            a,
            [
                "-d",
                "EPSON_A",
                "-n",
                "1",
                "-t",
                "Plan.dwg - Model",
                "-o",
                "PageSize=Letter",
                "-o",
                "print-scaling=none",
                "-o",
                "fit-to-page=false",
                "-"
            ]
        );
        assert_eq!(lp_args("P", None, 5000, "")[3], "999");
        assert_eq!(paper_option("iso_a4_210x297mm", false), "media=iso_a4_210x297mm");
    }

    /// A queue without a readable PPD: its papers from `lpoptions -l`, PWG, Adobe and plain
    /// keywords, the default marked, unknown and custom sizes left out.
    #[test]
    fn lpoptions_papers_of_a_driverless_queue() {
        let text = "ColorModel/Color Mode: *RGB Gray\n\
                    PageSize/Media Size: iso_a3_297x420mm *iso_a4_210x297mm na_letter_8.5x11in w612h1008 Legal A4.Fullbleed Custom.WIDTHxHEIGHT EnvWeird\n\
                    Duplex/2-Sided Printing: *None DuplexNoTumble\n";
        let p = parse_lpoptions(text);
        let names: Vec<&str> = p.papers.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["A3", "A4", "Letter", "216 x 356 mm", "Legal", "A4 (borderless)"]);
        assert_eq!(p.default_size, "iso_a4_210x297mm");
        let c = printer_config("Office", &p);
        assert_eq!(c.default_paper, "A4");
        assert_eq!(c.description, "System printer");
        let a4 = c.paper("A4").unwrap();
        assert!((a4.width - 210.0).abs() < 0.01 && (a4.height - 297.0).abs() < 0.01, "{a4:?}");
        assert!(a4.margins.iter().all(|m| (m - IPP_MARGIN_MM).abs() < 0.01), "{a4:?}");
        let letter = c.paper("Letter").unwrap();
        assert!((letter.width - 215.9).abs() < 0.01 && (letter.height - 279.4).abs() < 0.01);
        assert_eq!(c.paper("A4 (borderless)").unwrap().margins, [0.0; 4]);
        // A `media` option when there is no PageSize; nothing at all gives no papers.
        assert_eq!(parse_lpoptions("media/Media: *na_legal_8.5x14in\n").papers.len(), 1);
        assert!(parse_lpoptions("Duplex/2-Sided Printing: *None\n").papers.is_empty());
        assert!(parse_lpoptions("PageSize/Media Size: iso_a4_0x0mm iso_a4_NaNxNaNmm w-1h5\n").papers.is_empty());
    }

    /// The default printer is read from `lpstat -d`; none, or a bad name, gives none.
    #[test]
    fn the_default_destination() {
        assert_eq!(parse_default_destination("system default destination: Office_T120\n").as_deref(), Some("Office_T120"));
        assert_eq!(parse_default_destination("no system default destination\n"), None);
        assert_eq!(parse_default_destination("system default destination: bad/name\n"), None);
    }
}
