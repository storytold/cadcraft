//! The DWG reader and writer in a child process.
//!
//! An allocation failure or stack overflow in the third-party code ends the process it runs in, and
//! `catch_unwind` can't stop that. So the app runs them in a copy of itself: [`use_own_worker`] says
//! how to start one, and [`serve_if_asked`], first thing in `main`, turns the copy into a worker
//! that reads one request from standard input and writes one reply to standard output. A file that
//! kills the worker then costs that one open or save, and every other open drawing stays safe.
//!
//! Without a worker (libraries, tests, a build that can't start one) [`decode`] and [`encode_as`]
//! run in the calling process, as they always did. There's no memory limit on the worker yet: that
//! takes per-platform system calls, which `unsafe_code = "forbid"` keeps out of this crate.
//!
//! A reply comes from a process that just handled a possibly hostile file, so it is parsed as
//! untrusted input.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use super::Limits;

/// The argument that makes the app a worker.
pub const WORKER_ARG: &str = "--dwg-worker";
/// Bumped when the messages change: an app updated while running may start a newer copy.
const PROTOCOL: u8 = 2;
const DECODE: u8 = 1;
const ENCODE: u8 = 2;
const CRASH: u8 = 0xff;
/// How long one file may take before the worker is stopped. It can't be interrupted otherwise.
const TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Most bytes accepted for what a reply carries besides the drawing.
const MAX_NOTES_BYTES: u64 = 64 << 20;
/// Most bytes accepted as a written DWG.
const MAX_DWG_REPLY: u64 = 2 << 30;
/// Most bytes one request holds: the largest input, whichever the direction, and its header.
const MAX_REQUEST: u64 = (2 << 30) + 256;

struct Worker {
    program: PathBuf,
    args: Vec<OsString>,
}

static WORKER: Mutex<Option<Worker>> = Mutex::new(None);

/// Start `program args` as the worker from now on. `program` must call [`serve_if_asked`].
pub fn use_worker(program: PathBuf, args: Vec<OsString>) {
    if let Ok(mut w) = WORKER.lock() {
        *w = Some(Worker { program, args });
    }
}

/// Use a copy of this program as the worker, started with [`WORKER_ARG`]. If its path can't be
/// found, the reader and writer stay in this process.
pub fn use_own_worker() {
    match std::env::current_exe() {
        Ok(program) => use_worker(program, vec![WORKER_ARG.into()]),
        Err(e) => log::warn!("DWG worker: can't find this program: {e}"),
    }
}

/// Serve one request and return the exit code if this process was started as a worker.
pub fn serve_if_asked() -> Option<i32> {
    (std::env::args_os().nth(1)? == WORKER_ARG).then(|| serve(std::io::stdin().lock(), std::io::stdout().lock()))
}

/// [`super::dwg_to_dxf_noted`] in the worker, when there is one.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<(Vec<u8>, Vec<String>), String> {
    // Before starting a process, and so the same limit holds with or without one.
    if bytes.len() > limits.max_dwg_bytes {
        return super::dwg_to_dxf_noted(bytes, limits);
    }
    let reply_limit = (limits.max_dxf_bytes as u64).saturating_add(MAX_NOTES_BYTES);
    match call(&request(DECODE, limits, ""), bytes, reply_limit, "reading this file") {
        Some(reply) => reply?,
        None => super::dwg_to_dxf_noted(bytes, limits),
    }
}

/// [`super::dxf_to_dwg_as`] in the worker, when there is one.
pub fn encode_as(dxf: &[u8], version: &str) -> Result<Vec<u8>, String> {
    encode_noted(dxf, version).map(|(dwg, _)| dwg)
}

/// [`super::dxf_to_dwg_noted`] in the worker, when there is one. What the writer left out comes
/// back as notes of `TYPE<tab>count`.
pub fn encode_noted(dxf: &[u8], version: &str) -> Result<(Vec<u8>, super::Unwritten), String> {
    if u8::try_from(version.len()).is_err() {
        return Err(super::unsupported(version));
    }
    match call(&request(ENCODE, Limits::DEFAULT, version), dxf, MAX_DWG_REPLY, "writing this file") {
        Some(reply) => reply?.map(|(dwg, notes)| (dwg, unwritten_from(&notes))),
        None => super::dxf_to_dwg_noted(dxf, version),
    }
}

fn unwritten_from(notes: &[String]) -> super::Unwritten {
    notes.iter().filter_map(|n| n.split_once('\t')).filter_map(|(kind, n)| Some((kind.to_string(), n.parse::<usize>().ok()?))).fold(
        super::Unwritten::new(),
        |mut m, (kind, n)| {
            *m.entry(kind).or_insert(0) = n;
            m
        },
    )
}

/// Make the worker abort, as the third-party code can: what the tests prove isolation with.
#[doc(hidden)]
pub fn crash_worker_for_test() -> Result<(), String> {
    call(&request(CRASH, Limits::DEFAULT, ""), &[], 1024, "testing").ok_or("no worker")??.map(|_| ())
}

fn request(op: u8, limits: Limits, version: &str) -> Vec<u8> {
    let mut r = vec![PROTOCOL, op];
    r.extend((limits.max_dwg_bytes as u64).to_le_bytes());
    r.extend((limits.max_dxf_bytes as u64).to_le_bytes());
    r.extend(limits.assumed_memory.to_le_bytes());
    r.push(u8::try_from(version.len()).unwrap_or(0));
    r.extend(version.as_bytes());
    r
}

type Reply = Result<(Vec<u8>, Vec<String>), String>;

/// Run one request in a worker. `None`: none could be started, so the caller does the work.
/// `Err`: the worker went wrong, and `doing` says at what, for the user.
fn call(header: &[u8], payload: &[u8], reply_limit: u64, doing: &str) -> Option<Result<Reply, String>> {
    let mut command = {
        let worker = WORKER.lock().ok()?;
        let worker = worker.as_ref()?;
        let mut c = Command::new(&worker.program);
        c.args(&worker.args);
        c
    };
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("DWG worker: can't start it: {e}");
            return None;
        }
    };
    let (Some(mut to_child), Some(from_child)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let received = std::thread::scope(|scope| {
        // Written beside the read: a worker that replies before it has read everything, or dies,
        // must block neither side. A failed write shows up as the reply going wrong.
        scope.spawn(move || {
            let _ = to_child.write_all(header).and_then(|()| to_child.write_all(payload));
        });
        let (tx, rx) = channel();
        scope.spawn(move || {
            let mut reply = Vec::new();
            let read = from_child.take(reply_limit).read_to_end(&mut reply);
            let _ = tx.send(read.map(|_| reply));
        });
        let received = rx.recv_timeout(TIMEOUT);
        if matches!(received, Err(RecvTimeoutError::Timeout)) {
            let _ = child.kill();
        }
        received
    });
    let status = child.wait();
    let stopped = || format!("the DWG worker stopped while {doing}; nothing else is affected");
    Some(match (received, status) {
        (Err(RecvTimeoutError::Timeout), _) => Err(format!("{doing} took too long, so the DWG worker was stopped; nothing else is affected")),
        (Ok(Ok(reply)), Ok(status)) if status.success() => parse_reply(reply).ok_or_else(stopped),
        _ => Err(stopped()),
    })
}

/// Take `n` bytes off the front of `b`.
fn take<'a>(b: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    let (head, tail) = b.split_at_checked(n)?;
    *b = tail;
    Some(head)
}

fn take_u32(b: &mut &[u8]) -> Option<usize> {
    let bytes: [u8; 4] = take(b, 4)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(bytes)).ok()
}

fn take_u64(b: &mut &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(take(b, 8)?.try_into().ok()?))
}

/// A reply: 0, the note count, each note with its length, then the bytes with their length; or 1
/// and a message. `None` when it isn't one, whole. Takes the reply, because the bytes can be most
/// of the memory the app may use: they are kept, not copied.
fn parse_reply(mut reply: Vec<u8>) -> Option<Reply> {
    let total = reply.len();
    let mut b = &reply[..];
    match *take(&mut b, 1)?.first()? {
        0 => {
            let count = take_u32(&mut b)?;
            let mut notes = Vec::new();
            for _ in 0..count {
                let n = take_u32(&mut b)?;
                notes.push(String::from_utf8_lossy(take(&mut b, n)?).into_owned());
            }
            let n = usize::try_from(take_u64(&mut b)?).ok()?;
            if b.len() != n {
                return None;
            }
            reply.drain(..total - n);
            Some(Ok((reply, notes)))
        }
        1 => Some(Err(String::from_utf8_lossy(b).into_owned())),
        _ => None,
    }
}

fn write_reply(out: &mut impl Write, reply: Reply) -> std::io::Result<()> {
    match reply {
        Ok((bytes, notes)) => {
            out.write_all(&[0])?;
            out.write_all(&u32::try_from(notes.len()).unwrap_or(0).to_le_bytes())?;
            for note in &notes {
                out.write_all(&u32::try_from(note.len()).unwrap_or(0).to_le_bytes())?;
                if u32::try_from(note.len()).is_ok() {
                    out.write_all(note.as_bytes())?;
                }
            }
            out.write_all(&(bytes.len() as u64).to_le_bytes())?;
            out.write_all(&bytes)?;
        }
        Err(e) => {
            out.write_all(&[1])?;
            out.write_all(e.as_bytes())?;
        }
    }
    out.flush()
}

/// What a request asks for, or why it can't be served.
fn handle(mut b: &[u8]) -> Reply {
    let bad = || "the DWG worker got a request it can't read (a different CADCraft version?)".to_string();
    if take(&mut b, 1).and_then(<[u8]>::first) != Some(&PROTOCOL) {
        return Err(bad());
    }
    let op = take(&mut b, 1).and_then(<[u8]>::first).copied().ok_or_else(bad)?;
    let max_dwg_bytes = take_u64(&mut b).and_then(|n| usize::try_from(n).ok()).ok_or_else(bad)?;
    let max_dxf_bytes = take_u64(&mut b).and_then(|n| usize::try_from(n).ok()).ok_or_else(bad)?;
    let assumed_memory = take_u64(&mut b).ok_or_else(bad)?;
    let version_len = take(&mut b, 1).and_then(<[u8]>::first).copied().ok_or_else(bad)?;
    let version = take(&mut b, usize::from(version_len)).and_then(|v| std::str::from_utf8(v).ok()).ok_or_else(bad)?;
    match op {
        DECODE => super::dwg_to_dxf_noted(b, Limits { max_dwg_bytes, max_dxf_bytes, assumed_memory }),
        ENCODE => super::dxf_to_dwg_noted(b, version).map(|(dwg, left_out)| (dwg, left_out.iter().map(|(kind, n)| format!("{kind}\t{n}")).collect())),
        CRASH => std::process::abort(),
        _ => Err(bad()),
    }
}

/// Serve the one request on `input`. The exit code is 0 unless the reply couldn't be sent.
fn serve(input: impl Read, mut output: impl Write) -> i32 {
    let mut request = Vec::new();
    let reply = match input.take(MAX_REQUEST).read_to_end(&mut request) {
        Ok(_) => handle(&request),
        Err(e) => Err(format!("the DWG worker could not read its request: {e}")),
    };
    i32::from(write_reply(&mut output, reply).is_err())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(reply: Reply) -> Vec<u8> {
        let mut out = Vec::new();
        write_reply(&mut out, reply).unwrap();
        out
    }

    #[test]
    fn what_the_writer_left_out_survives_the_reply() {
        let notes = vec!["ACME_WIDGET\t2".to_string(), "not a count".to_string(), "X\tmany".to_string()];
        assert_eq!(unwritten_from(&notes), [("ACME_WIDGET".to_string(), 2)].into_iter().collect());
    }

    #[test]
    fn replies_round_trip() {
        let ok: Reply = Ok((b"bytes".to_vec(), vec!["one".into(), "two é".into()]));
        assert_eq!(parse_reply(wire(ok.clone())), Some(ok));
        let failed: Reply = Err("DWG: no".into());
        assert_eq!(parse_reply(wire(failed.clone())), Some(failed));
    }

    #[test]
    fn a_reply_cut_short_or_made_up_is_refused() {
        let whole = wire(Ok((b"bytes".to_vec(), vec!["note".into()])));
        for n in 0..whole.len() {
            assert_eq!(parse_reply(whole[..n].to_vec()), None, "cut at {n}");
        }
        assert_eq!(parse_reply(vec![7, 0, 0]), None);
        // A note count far beyond what follows ends at the first missing note.
        assert_eq!(parse_reply(vec![0, 0xff, 0xff, 0xff, 0xff]), None);
        assert_eq!(parse_reply(vec![0, 1, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]), None);
    }

    #[test]
    fn a_request_it_cannot_read_is_an_error_not_a_crash() {
        for request in [
            &[][..],
            &[PROTOCOL + 1, DECODE],
            &[PROTOCOL],
            &[PROTOCOL, DECODE, 1, 2, 3],
            &[PROTOCOL, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ] {
            assert!(handle(request).is_err(), "{request:?}");
        }
    }

    #[test]
    fn serving_reports_a_decode_failure_as_a_reply() {
        let mut request = self::request(DECODE, Limits::DEFAULT, "");
        request.extend(b"AC1032 not really a drawing");
        let mut out = Vec::new();
        assert_eq!(serve(&request[..], &mut out), 0);
        assert!(matches!(parse_reply(out.clone()), Some(Err(_))), "{out:?}");
    }
}
