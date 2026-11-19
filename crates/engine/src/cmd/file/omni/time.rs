//! UTC stamps: the name a version is filed under, the ISO stamp it is listed with, and back.
//!
//! The calendar is [`chrono`]'s job (issue #1785): this module owns only the two formats the
//! history uses, so a stamp means the same moment on every platform and nothing here hand-rolls
//! leap years, month lengths or the epoch shift.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};

/// Now, as (a name stamp `YYYYMMDD.HHMMSS`, Unix seconds, milliseconds).
pub(super) fn now() -> (String, i64, u32) {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
    (stamp(secs), secs, d.subsec_millis())
}

/// The name stamp of a Unix second (`20260819.153012`). Every field is zero-padded and the date
/// comes first, so a folder's names sort as text in the order they were taken.
pub(super) fn stamp(secs: i64) -> String {
    at(secs).format("%Y%m%d.%H%M%S").to_string()
}

/// A point in time as ISO-8601 UTC with milliseconds (`2026-10-14T08:35:12.482Z`).
pub(super) fn iso(secs: i64, ms: u32) -> String {
    at(secs).with_nanosecond(ms.min(999) * 1_000_000).unwrap_or_else(|| at(secs)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// The Unix second a name stamp (`20260819.153012`) stands for, or `None` when it is not one.
/// A name in a history folder is input from a file, so this refuses what it does not recognise
/// instead of guessing: the wrong moment would reorder the timeline under the user.
pub(super) fn stamp_epoch(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.split_once('.')?;
    if date.len() != 8 || time.len() != 6 || !date.bytes().all(|b| b.is_ascii_digit()) || !time.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let date = NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
    let time = NaiveTime::parse_from_str(time, "%H%M%S").ok()?;
    Some(NaiveDateTime::new(date, time).and_utc().timestamp())
}

/// A Unix second as a UTC moment. A second past the end of the calendar reads as the epoch rather
/// than panicking: it can only come from a file name, and no command replies with an error there.
fn at(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_round_trips_through_the_calendar() {
        // A known moment: 2026-10-14T08:35:12Z.
        assert_eq!(stamp(1_791_966_912), "20261014.083512");
        assert_eq!(iso(1_791_966_912, 482), "2026-10-14T08:35:12.482Z");
        assert_eq!(stamp_epoch("20261014.083512"), Some(1_791_966_912));
        // The name a snapshot takes is the moment it was taken, to the second.
        let (name, secs, _) = now();
        assert_eq!(stamp_epoch(&name), Some(secs), "{name}");
        // Every stamp the tool writes reads back as the second it was made from, a century either
        // side of now, including leap days and the ends of their months.
        for secs in [0i64, 1_760_000_000, 1_766_000_000, 4_102_444_800, 951_782_400, -1_000_000_000] {
            assert_eq!(stamp_epoch(&stamp(secs)), Some(secs), "{secs}");
        }
    }

    #[test]
    fn hostile_stamps_are_refused_not_guessed() {
        for s in [
            "",
            ".",
            "2026",
            "20261014",
            "20261014.",
            "20261014.0835",
            "x0261014.083512",
            "20261014.0835127",
            "20261314.083512",
            "20260231.083512",
            "20261014.253512",
            "é0261014.083512",
        ] {
            assert_eq!(stamp_epoch(s), None, "{s}");
        }
    }
}
