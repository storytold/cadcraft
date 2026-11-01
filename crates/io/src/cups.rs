//! System printers through CUPS (macOS and Linux): the destinations `lpstat` lists, each with
//! the papers and printable areas of its PPD, and printing a plot's PDF with `lp`.
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

/// The `lp` arguments for a job (the PDF goes to standard input).
pub fn lp_args(printer: &str, page_size: Option<&str>, copies: u32, title: &str) -> Vec<String> {
    let mut a = vec!["-d".into(), printer.into(), "-n".into(), copies.clamp(1, MAX_COPIES).to_string()];
    let title: String = title.chars().filter(|c| !c.is_control()).take(100).collect();
    if !title.trim().is_empty() {
        a.extend(["-t".into(), title]);
    }
    if let Some(k) = page_size {
        a.extend(["-o".into(), format!("PageSize={k}")]);
    }
    // The plot is already at its scale: no fitting to the page.
    a.extend(["-o".into(), "print-scaling=none".into(), "-o".into(), "fit-to-page=false".into(), "-".into()]);
    a
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod system {
    use super::{MAX_PPD, PlotterConfig, Ppd, lp_args, parse_destinations, parse_ppd, parse_request_id, printer_config, valid_name};
    use std::io::{Read, Write};
    use std::process::{Command, ExitStatus, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    fn ppd(name: &str) -> Option<Ppd> {
        let path = format!("/etc/cups/ppd/{name}.ppd");
        if std::fs::metadata(&path).ok()?.len() > MAX_PPD {
            return None;
        }
        Some(parse_ppd(&std::fs::read_to_string(path).ok()?))
    }

    /// Longest `lpstat` may take: a CUPS that doesn't answer gives no printers instead of a
    /// listing that never ends.
    const LPSTAT_TIMEOUT: Duration = Duration::from_secs(5);
    /// Longest `lp` may take to queue a plot: a CUPS that doesn't answer is an error, not an app
    /// that hangs.
    const LP_TIMEOUT: Duration = Duration::from_secs(60);
    /// How often [`run`] looks whether the command has ended.
    const POLL: Duration = Duration::from_millis(20);
    /// How long, once the command has ended, [`run`] waits for the rest of its output (only
    /// something else holding its pipes makes this wait).
    const GRACE: Duration = Duration::from_secs(1);

    /// How a command [`run`] ran ended.
    struct Ran {
        status: ExitStatus,
        stdout: String,
        stderr: String,
        /// Why its input wasn't all written (it stopped reading, e.g. it ended early).
        write_error: Option<String>,
    }

    /// Run `f` on a thread; its result comes back on the receiver.
    fn on_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx
    }

    /// Everything `r` gives, read on a thread (so a full pipe can't keep the command from ending).
    fn read_all(r: Option<impl Read + Send + 'static>) -> Receiver<String> {
        on_thread(move || {
            let mut bytes = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).to_string()
        })
    }

    /// Run `cmd` (named `name` in messages) with `input` on its standard input (none: nothing to
    /// read), writing the input and reading its output on threads so no pipe can fill up and
    /// block either side. A command still running after `timeout` is stopped, reaped and reported.
    /// A write that fails (the command ended without reading it all) doesn't end the wait: the
    /// command's own status and messages tell why.
    fn run(name: &str, cmd: &mut Command, input: Option<Vec<u8>>, timeout: Duration) -> Result<Ran, String> {
        let stdin = if input.is_some() { Stdio::piped() } else { Stdio::null() };
        let mut child = cmd.stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("{name}: {e}"))?;
        // The command's input is closed once written (the thread ends), so it sees where it ends.
        let writer = child.stdin.take().zip(input).map(|(mut w, bytes)| on_thread(move || w.write_all(&bytes)));
        let (stdout, stderr) = (read_all(child.stdout.take()), read_all(child.stderr.take()));
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
                stuck => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(match stuck {
                        Err(e) => format!("{name}: {e}"),
                        _ => format!("{name} did not finish within {timeout:?} and was stopped"),
                    });
                }
            }
        };
        let write_error = writer.and_then(|w| match w.recv_timeout(GRACE) {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(_) => Some("its input was not all read".to_string()),
        });
        let (stdout, stderr) = (stdout.recv_timeout(GRACE).unwrap_or_default(), stderr.recv_timeout(GRACE).unwrap_or_default());
        Ok(Ran { status, stdout, stderr, write_error })
    }

    /// What `name` printed, or why it failed: its own message when it gives one.
    fn output(name: &str, ran: Ran) -> Result<String, String> {
        if !ran.status.success() {
            let err = ran.stderr.trim();
            return Err(if err.is_empty() { format!("{name} failed ({})", ran.status) } else { err.to_string() });
        }
        match ran.write_error {
            Some(e) => Err(format!("{name}: {e}")),
            None => Ok(ran.stdout),
        }
    }

    /// `lpstat -e`'s output, or `None` when it fails or takes longer than [`LPSTAT_TIMEOUT`]
    /// (then it is stopped).
    fn lpstat_destinations() -> Option<String> {
        output("lpstat", run("lpstat", Command::new("lpstat").arg("-e"), None, LPSTAT_TIMEOUT).ok()?).ok()
    }

    /// The CUPS destinations as devices (none when CUPS is missing). A destination without a
    /// readable PPD (an IPP queue the system made on the fly) offers no papers of its own and is
    /// left out.
    pub fn printers() -> Vec<PlotterConfig> {
        let Some(out) = lpstat_destinations() else { return Vec::new() };
        parse_destinations(&out).into_iter().filter_map(|n| ppd(&n).filter(|p| !p.papers.is_empty()).map(|p| printer_config(&n, &p))).collect()
    }

    /// Send `pdf` to `printer` on the paper called `paper` (a name [`printers`] gave).
    /// Returns the job id. An `lp` that doesn't finish within [`LP_TIMEOUT`] is stopped.
    pub fn print(printer: &str, paper: &str, copies: u32, title: &str, pdf: &[u8]) -> Result<String, String> {
        if !valid_name(printer) {
            return Err(format!("`{printer}` is not a printer name"));
        }
        let keyword = ppd(printer).and_then(|p| p.papers.into_iter().find(|x| x.name.eq_ignore_ascii_case(paper)).map(|x| x.keyword));
        let mut lp = Command::new("lp");
        lp.args(lp_args(printer, keyword.as_deref(), copies, title));
        let said = output("lp", run("lp", &mut lp, Some(pdf.to_vec()), LP_TIMEOUT)?)?;
        Ok(parse_request_id(&said).unwrap_or_else(|| said.trim().to_string()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A command that ends without reading its input is reported by its own message and exit
        /// status, not as the broken pipe writing the input ran into.
        #[test]
        fn a_command_that_ends_early_is_reported_by_its_own_message() {
            let mut sh = Command::new("sh");
            sh.args(["-c", "echo nope >&2; exit 1"]);
            let ran = run("lp", &mut sh, Some(vec![b'%'; 8 << 20]), Duration::from_secs(30)).unwrap();
            assert_eq!(output("lp", ran), Err("nope".to_string()));
        }

        /// A command that doesn't finish in time is stopped and reaped (no process left behind),
        /// and that is the error.
        #[test]
        fn a_command_that_does_not_finish_is_stopped_and_reaped() {
            let pid_file = std::env::temp_dir().join(format!("cadcraft-cups-run-{}.pid", std::process::id()));
            let _ = std::fs::remove_file(&pid_file);
            let mut sh = Command::new("sh");
            sh.args(["-c", "echo $$ > \"$0\"; exec sleep 30"]).arg(&pid_file);
            let start = Instant::now();
            let err = run("lp", &mut sh, Some(b"%PDF".to_vec()), Duration::from_millis(300)).err().unwrap();
            assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
            assert_eq!(err, "lp did not finish within 300ms and was stopped");
            let pid = std::fs::read_to_string(&pid_file).unwrap();
            let _ = std::fs::remove_file(&pid_file);
            // A process killed but not waited for is still there (a zombie) for `kill -0`.
            let alive = Command::new("kill").args(["-0", pid.trim()]).stderr(Stdio::null()).status().unwrap().success();
            assert!(!alive, "process {} left behind", pid.trim());
        }
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

    #[test]
    fn lpstat_and_lp_replies() {
        assert_eq!(parse_destinations("EPSON_A\nEPSON_B\n\nbad/name\n"), ["EPSON_A", "EPSON_B"]);
        assert_eq!(parse_request_id("request id is EPSON_A-42 (1 file(s))\n").as_deref(), Some("EPSON_A-42"));
        assert_eq!(parse_request_id("nothing"), None);
        let a = lp_args("EPSON_A", Some("Letter"), 0, "Plan\u{7}.dwg - Model");
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
    }
}
