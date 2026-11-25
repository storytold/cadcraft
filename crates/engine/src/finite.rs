//! Edits whose result has coordinates too large to represent (NaN or infinite) are refused.
//!
//! Finite inputs can still overflow: moving by `[1e308, 1e308]` twice, scaling by a huge factor or
//! offsetting by a huge distance. Infinite coordinates break extents, inspection and rendering, and
//! are silently changed by a DXF save and reopen, so the command fails and the drawing stays as it
//! was. Only objects the command changed are checked: every number in their serialized fields, and
//! their extents.

use std::collections::HashSet;
use std::sync::Arc;

use cadcraft_doc::{Drawing, Entity, EntityStore, Handle, entity_bounds};
use serde::Serialize;
use serde::ser;

use crate::{EngineError, Session};

impl Session {
    /// After command `cmd` changed the drawing from `before` (the document `uid`, or the active
    /// one): when an object it changed has NaN or infinite numbers, put the drawing and selection
    /// back and return the error to report.
    pub(crate) fn refuse_non_finite(&mut self, uid: Option<u64>, before: &Arc<Drawing>, selection: &[Handle], cmd: &str) -> crate::Result<()> {
        let st = match uid {
            Some(uid) => self.docs.iter_mut().find(|d| d.uid == uid),
            None => self.state_mut().ok(),
        };
        let Some(st) = st.filter(|st| !Arc::ptr_eq(before, &st.doc)) else { return Ok(()) };
        let Some(h) = first_non_finite(before, &st.doc) else { return Ok(()) };
        st.doc = before.clone();
        st.selection = selection.to_vec();
        // An earlier state, as after Undo: names of INSERTs' records end (#1660).
        st.expire_names();
        Err(EngineError::BadParams {
            cmd: cmd.into(),
            msg: format!("the result is too large: object {} would get infinite or undefined coordinates. Nothing was changed", h.hex()),
        })
    }
}

/// The first object changed between `before` and `after` that has a NaN or infinite number, or
/// whose extents overflow.
pub(crate) fn first_non_finite(before: &Drawing, after: &Drawing) -> Option<Handle> {
    let layouts = after.layouts.iter().map(|l| (&l.entities, before.layout(&l.name).map(|b| &b.entities)));
    let blocks = after
        .blocks
        .iter()
        .filter(|(name, b)| before.blocks.get(*name).is_none_or(|old| !Arc::ptr_eq(old, b)))
        .map(|(name, b)| (&b.entities, before.blocks.get(name).map(|old| &old.entities)));
    std::iter::once((&after.model, Some(&before.model))).chain(layouts).chain(blocks).find_map(|(a, b)| changed_non_finite(after, b, a))
}

fn changed_non_finite(d: &Drawing, before: Option<&EntityStore>, after: &EntityStore) -> Option<Handle> {
    if before.is_some_and(|b| b.same_as(after)) {
        return None;
    }
    // Chunks shared with `before` hold only unchanged objects.
    let shared: HashSet<usize> = before.map(|b| b.chunk_slices().map(|(key, _)| key).collect()).unwrap_or_default();
    let finite = |e: &Entity| {
        let b = entity_bounds(d, e, 0);
        is_finite(e) && (b.is_empty() || (b.min.is_finite() && b.max.is_finite()))
    };
    after
        .chunk_slices()
        .filter(|(key, _)| !shared.contains(key))
        .flat_map(|(_, items)| items.iter())
        .filter(|e| before.and_then(|b| b.get(e.handle)).is_none_or(|old| !Arc::ptr_eq(old, e)))
        .find(|e| !finite(e))
        .map(|e| e.handle)
}

/// Whether every number in `v` is finite.
pub(crate) fn is_finite<T: Serialize + ?Sized>(v: &T) -> bool {
    !matches!(v.serialize(Check), Err(Stop::NonFinite))
}

/// Why the walk stopped: a non-finite number, or a value that can't be serialized (not ours to judge).
#[derive(Debug)]
enum Stop {
    NonFinite,
    Other,
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Stop::NonFinite => "non-finite number",
            Stop::Other => "not serializable",
        })
    }
}

impl std::error::Error for Stop {}

impl ser::Error for Stop {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Stop::Other
    }
}

/// A serializer that only looks at floats.
#[derive(Clone, Copy)]
struct Check;

type R = Result<(), Stop>;

macro_rules! ignore {
    ($($name:ident: $t:ty),*) => { $(fn $name(self, _: $t) -> R { Ok(()) })* };
}

impl ser::Serializer for Check {
    type Ok = ();
    type Error = Stop;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64, serialize_i128: i128,
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64, serialize_u128: u128, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8], serialize_unit_struct: &'static str);

    fn serialize_f32(self, v: f32) -> R {
        if v.is_finite() { Ok(()) } else { Err(Stop::NonFinite) }
    }
    fn serialize_f64(self, v: f64) -> R {
        if v.is_finite() { Ok(()) } else { Err(Stop::NonFinite) }
    }
    fn serialize_none(self) -> R {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_unit(self) -> R {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> R {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, v: &T) -> R {
        v.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Stop> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self, Stop> {
        Ok(self)
    }
}

impl ser::SerializeSeq for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTuple for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeMap for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> R {
        k.serialize(*self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeStruct for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

impl ser::SerializeStructVariant for Check {
    type Ok = ();
    type Error = Stop;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> R {
        v.serialize(*self)
    }
    fn end(self) -> R {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nan_and_infinity_anywhere() {
        assert!(is_finite(&(1.0, vec![Some(2.0f32)], "inf")));
        assert!(!is_finite(&vec![(0.0, f64::INFINITY)]));
        assert!(!is_finite(&Some(f32::NAN)));
        let mut m = std::collections::BTreeMap::new();
        m.insert("x", f64::NEG_INFINITY);
        assert!(!is_finite(&m));
    }
}
