use super::{cdate, julian_date};

fn close(got: Option<f64>, want: f64) -> bool {
    got.is_some_and(|g| (g - want).abs() < 1e-7)
}

#[test]
fn cdate_is_yyyymmdd_dot_hhmmss() {
    assert!(close(cdate(0), 19_700_101.0));
    // 2026-10-11 13:42:05 UTC.
    assert!(close(cdate(1_791_676_800 + 13 * 3600 + 42 * 60 + 5), 20_261_011.134_205));
    // Leap day 2000-02-29 01:01.
    assert!(close(cdate(951_782_400 + 3_660), 20_000_229.010_1));
    // The last second of a day stays on that day.
    assert!(close(cdate(1_791_676_800 - 1), 20_261_010.235_959));
}

#[test]
fn date_is_julian_day_and_fraction_since_midnight() {
    assert_eq!(julian_date(0), Some(2_440_588.0));
    // 1998-01-01 12:00 UTC, AutoCAD's documented example.
    assert_eq!(julian_date(883_656_000), Some(2_450_815.5));
    assert!(close(julian_date(1_791_676_800 + 6 * 3600), 2_461_325.25));
}

#[test]
fn cdate_and_date_are_read_only() {
    assert!(super::is_read_only("cdate"));
    assert!(super::is_read_only("DATE"));
}
