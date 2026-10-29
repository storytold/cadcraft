use super::args::InfoArgs;
use super::error::Result;
use super::open;
use super::output::Streams;
use cadcraft_engine::Session;
use serde_json::json;

pub(crate) fn info(a: &InfoArgs, io: &mut Streams) -> Result {
    let mut s = Session::empty();
    open(&mut s, &a.file)?;
    let v = s.execute("drawing.inspect", &json!({ "entities": false }))?;
    io.json(&v, a.compact);
    Ok(())
}
