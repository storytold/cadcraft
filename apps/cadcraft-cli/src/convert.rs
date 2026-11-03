use super::args::ConvertArgs;
use super::error::Result;
use super::output::{Streams, export_frame, export_framed, read_note_messages, skipped_message, undrawn_message};
use super::{open, unicode};
use cadcraft_engine::Session;

pub(crate) fn convert(a: &ConvertArgs, io: &mut Streams) -> Result {
    let i = unicode(&a.input)?;
    let framing = a.framing.framing();
    let mut s = Session::empty();
    open(&mut s, &a.input)?;
    let d = s.doc()?;
    for m in read_note_messages(d, i).into_iter().chain(skipped_message(d, i)) {
        io.errln(format_args!("{m}"));
    }
    if cadcraft_io::is_image(&a.output)
        && let Some(m) = undrawn_message(s.doc()?, i)
    {
        io.errln(format_args!("{m}"));
    }
    let (window, view) = export_frame(&s, framing, None)?;
    export_framed(&s, &a.output, window, view, io)
}
