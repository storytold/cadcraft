use super::args::{SampleArgs, SampleName};
use super::error::Result;
use super::output::{Streams, write_atomic};

pub(crate) fn sample(a: &SampleArgs, io: &mut Streams) -> Result {
    let d = match a.name {
        SampleName::Bracket => cadcraft_engine::sample::bracket(),
        SampleName::Floorplan => cadcraft_engine::sample::floor_plan(),
    };
    let bytes = cadcraft_io::write(&d, &a.output)?;
    write_atomic(&a.output, &bytes)?;
    io.errln(format_args!("wrote {} ({} bytes)", a.output.display(), bytes.len()));
    Ok(())
}
