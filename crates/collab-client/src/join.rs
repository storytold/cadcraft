//! A snapshot brought into a replica without blocking the frame that received it.
//!
//! The browser has one thread, so a large drawing's snapshot cannot be decoded and parsed in one
//! call: the page would freeze for seconds. [`Join`] spreads the work: one base64 part per turn,
//! then the DXF group scan, then bounded batches of entity records. A caller gives it a time
//! budget per frame and keeps the events it already has in the meantime.
//!
//! The result is the same drawing [`cadcraft_io::read`] produces and the same version the welcome
//! named, so a transport can replace its replica with it exactly as a one-call join would.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::time::Duration;

use cadcraft_collab::message::Snapshot;
use cadcraft_doc::Drawing;
use cadcraft_io::Incremental;
use web_time::Instant;

use crate::driver::SessionError;

/// A snapshot on its way in: parts to decode, then a drawing to read in batches.
pub(crate) struct Join {
    parts: Vec<Snapshot>,
    next_part: usize,
    /// Decoded DXF bytes, while the parts are being joined.
    bytes: Vec<u8>,
    /// The bounded reader, once every part is decoded.
    reader: Option<Incremental>,
}

impl Join {
    /// Start bringing in a snapshot's parts.
    pub(crate) fn new(parts: Vec<Snapshot>) -> Self {
        Join { parts, next_part: 0, bytes: Vec::new(), reader: None }
    }

    /// The version the snapshot was taken at; the replica's version once the join is done.
    pub(crate) fn version(&self) -> u64 {
        self.parts.first().map_or(0, |part| part.version)
    }

    /// Work for at most `budget`, reading at most `batch` entity records per step, and return
    /// whether the drawing is complete. At least one unit of work happens per call, so a zero
    /// budget still makes progress and the caller cannot spin forever on a stalled join.
    pub(crate) fn advance(&mut self, budget: Duration, batch: usize) -> Result<bool, SessionError> {
        let start = Instant::now();
        loop {
            if self.next_part < self.parts.len() {
                let part = &self.parts[self.next_part];
                let bytes = cadcraft_collab::message::decode_bytes(&part.bytes_base64).map_err(SessionError::Snapshot)?;
                self.bytes.extend(bytes);
                self.next_part += 1;
            } else if self.reader.is_none() {
                let reader = cadcraft_io::read_incremental(&self.bytes).map_err(|error| SessionError::Snapshot(error.to_string()))?;
                self.reader = Some(reader);
                // The bytes are in the reader's sections now.
                self.bytes = Vec::new();
            } else {
                let Some(reader) = self.reader.as_mut() else { return Ok(false) };
                if reader.step(batch).map_err(|error| SessionError::Snapshot(error.to_string()))? {
                    return Ok(true);
                }
            }
            if start.elapsed() >= budget {
                return Ok(false);
            }
        }
    }

    /// How many entity records are read, of the file's total; `None` while the parts are still
    /// being decoded or the reader is not built.
    pub(crate) fn progress(&self) -> Option<(usize, usize)> {
        self.reader.as_ref().map(|reader| (reader.read_records(), reader.total_records()))
    }

    /// The finished drawing. Only meaningful once [`advance`](Self::advance) returned `true`.
    pub(crate) fn drawing(self) -> Option<Drawing> {
        self.reader.map(Incremental::drawing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{Common, EntityKind, Line, Space};

    fn drawing() -> Drawing {
        let mut drawing = Drawing::new_metric();
        for i in 0..20_000 {
            let x = i as f64;
            drawing
                .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x, 10.0, 0.0) }))
                .expect("a line");
        }
        drawing
    }

    /// A snapshot joins in bounded units: the parts, the group scan and the entity batches take
    /// several turns, and the result is the one-call reader's drawing at the welcome's version.
    #[test]
    fn a_snapshot_is_joined_in_bounded_units() {
        let bytes = cadcraft_io::write(&drawing(), "join.dxf").expect("written");
        let complete = cadcraft_io::read(&bytes, "join.dxf").expect("read");
        let parts = Snapshot::encode(7, &bytes);
        assert!(parts.len() > 1, "the fixture spans more than one part");

        let mut join = Join::new(parts);
        let mut turns = 0;
        while !join.advance(Duration::ZERO, 16).expect("joined") {
            turns += 1;
            assert!(turns < 100_000, "the join must finish");
        }
        assert!(turns > 4, "the drawing was not read in one call");
        assert_eq!(join.version(), 7);
        assert_eq!(join.drawing().expect("a drawing"), complete);
    }

    /// Once the parts are decoded the progress is the reader's records, so a UI can show
    /// `read N/M` while the drawing arrives.
    #[test]
    fn progress_counts_the_entity_records() {
        let bytes = cadcraft_io::write(&drawing(), "join.dxf").expect("written");
        let mut join = Join::new(Snapshot::encode(3, &bytes));
        assert_eq!(join.progress(), None, "no reader before the parts are in");
        // One unit per call: the parts, then the group scan, then batches.
        while join.progress().is_none() {
            assert!(!join.advance(Duration::ZERO, 16).expect("joined"));
        }
        let (read, total) = join.progress().expect("the reader is built");
        assert_eq!(read, 0);
        assert!(total >= 500, "500 lines and their records: {total}");
        join.advance(Duration::from_secs(1), 64).expect("joined");
        let (read, total_after) = join.progress().expect("still reading");
        assert!(read > 0 && read <= total_after);
    }

    /// A corrupt part is reported instead of producing a half-readable drawing.
    #[test]
    fn a_corrupt_part_is_an_error() {
        let parts = vec![Snapshot { version: 1, seq: 0, more: false, bytes_base64: "not base64!!".into() }];
        let mut join = Join::new(parts);
        assert!(join.advance(Duration::ZERO, 16).is_err());
    }
}
