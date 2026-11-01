//! Bounded, dependency-free decoding and rest calibration of 3Dconnexion HID input reports.

use std::time::{Duration, Instant};

const FULL_SCALE: f64 = 500.0;
const DEADZONE: f64 = 0.04;
const REST_LIMIT: f64 = 60.0;
const REST_RANGE: f64 = 10.0;
const CALIBRATION_TIME: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    /// Mask 1 = translation, 2 = rotation, 3 = a combined six-axis report.
    Axes {
        values: [i16; 6],
        mask: u8,
    },
    Buttons(u8),
}

/// hidapi includes the report ID. A legacy motion report has six payload bytes; combined
/// report 1 has twelve. Windows may pad the short button report to the input-report size.
pub fn decode(bytes: &[u8]) -> Option<Report> {
    let (&id, payload) = bytes.split_first()?;
    if id == 3 {
        return payload.first().copied().map(Report::Buttons);
    }
    let (mask, offset, count) = match id {
        1 if payload.len() >= 12 => (3, 0, 6),
        1 if payload.len() >= 6 => (1, 0, 3),
        2 if payload.len() >= 6 => (2, 3, 3),
        _ => return None,
    };
    let mut values = [0; 6];
    for (out, pair) in values.iter_mut().skip(offset).take(count).zip(payload.as_chunks::<2>().0.iter()) {
        *out = i16::from_le_bytes([*pair.first()?, *pair.get(1)?]);
    }
    Some(Report::Axes { values, mask })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub pan: [f64; 2],
    pub zoom: f64,
}

impl Motion {
    pub fn active(self) -> bool {
        self.pan.iter().any(|v| *v != 0.0) || self.zoom != 0.0
    }
}

#[derive(Default)]
struct Rest {
    since: Option<Instant>,
    count: u32,
    sum: [f64; 6],
    min: [f64; 6],
    max: [f64; 6],
}

impl Rest {
    fn observe(&mut self, raw: [f64; 6], now: Instant) -> Option<[f64; 6]> {
        if raw.iter().any(|v| v.abs() > REST_LIMIT) {
            *self = Self::default();
            return None;
        }
        if self.since.is_none() || raw.iter().zip(self.min.iter().zip(&self.max)).any(|(v, (min, max))| v.max(*max) - v.min(*min) > REST_RANGE) {
            *self = Self { since: Some(now), count: 1, sum: raw, min: raw, max: raw };
            return None;
        }
        for ((min, max), value) in self.min.iter_mut().zip(&mut self.max).zip(raw) {
            *min = min.min(value);
            *max = max.max(value);
        }
        // A fixed bound even if a device sends far more than the expected 125 reports/sec.
        if self.count < 128 {
            self.count += 1;
            for (sum, value) in self.sum.iter_mut().zip(raw) {
                *sum += value;
            }
        }
        if self.count >= 16 && self.since.is_some_and(|since| now.saturating_duration_since(since) >= CALIBRATION_TIME) {
            Some(self.sum.map(|sum| sum / f64::from(self.count)))
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct Filter {
    raw: [f64; 6],
    bias: Option<[f64; 6]>,
    rest: Rest,
    parts: u8,
    seen: [Option<Instant>; 2],
    idle_since: Option<Instant>,
}

impl Filter {
    pub fn calibrated(&self) -> bool {
        self.bias.is_some()
    }

    pub fn recalibrate(&mut self) {
        *self = Self::default();
    }

    fn update_drift(&mut self, complete: bool, now: Instant, bias: &mut [f64; 6]) {
        if !complete {
            return;
        }
        if self.raw.iter().zip(*bias).all(|(raw, b)| (*raw - b).abs() <= FULL_SCALE * DEADZONE) {
            let since = *self.idle_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= Duration::from_secs(2) {
                for (b, raw) in bias.iter_mut().zip(self.raw) {
                    *b += (raw - *b) * 0.01;
                }
                self.bias = Some(*bias);
            }
        } else {
            self.idle_since = None;
        }
    }

    pub fn update(&mut self, values: [i16; 6], mask: u8, now: Instant) -> Motion {
        for (index, seen) in self.seen.iter_mut().enumerate() {
            if mask & (1 << index) != 0 {
                *seen = Some(now);
            }
        }
        for (index, (raw, value)) in self.raw.iter_mut().zip(values).enumerate() {
            if mask & if index < 3 { 1 } else { 2 } != 0 {
                *raw = f64::from(value);
            }
        }
        self.parts |= mask & 3;
        let complete = self.parts == 3;
        if complete {
            self.parts = 0;
        }
        if self.bias.is_none() && complete {
            self.bias = self.rest.observe(self.raw, now);
        }
        let Some(mut bias) = self.bias else { return Motion::default() };
        // Follow small resting drift only after two seconds entirely inside the deadzone.
        self.update_drift(complete, now, &mut bias);
        let mut axes = [0.0; 6];
        for ((axis, raw), bias) in axes.iter_mut().zip(self.raw).zip(bias) {
            let value = ((raw - bias) / FULL_SCALE).clamp(-1.0, 1.0);
            *axis = if value.abs() <= DEADZONE { 0.0 } else { value.signum() * (value.abs() - DEADZONE) / (1.0 - DEADZONE) };
        }
        // A missing rotation report must not keep zooming while translations still arrive.
        for (group, seen) in axes.as_chunks_mut::<3>().0.iter_mut().zip(self.seen) {
            if seen.is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_millis(250)) {
                group.fill(0.0);
            }
        }
        let [tx, ty, tz, _rx, _ry, rz] = axes;
        Motion { pan: [tx, ty], zoom: (tz + rz).clamp(-1.0, 1.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_reports() {
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[3]), None);
        assert_eq!(decode(&[3, 5]), Some(Report::Buttons(5)));
        let values = [1_i16, -2, 3, 4, -5, 6];
        let mut bytes = vec![1_u8];
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(decode(&bytes), Some(Report::Axes { values, mask: 3 }));
        bytes[0] = 2;
        assert_eq!(decode(&bytes), Some(Report::Axes { values: [0, 0, 0, 1, -2, 3], mask: 2 }));
        assert_eq!(decode(&[2, 1, 2, 3]), None, "too short for a rotation report");
    }

    /// Sixteen complete reports at rest, spread over more than CALIBRATION_TIME, all stamped
    /// on a made-up point in time big enough that they sit in the past of later updates.
    fn calibrated(t0: Instant) -> Filter {
        let mut f = Filter::default();
        for i in 0..17 {
            let at = t0 + Duration::from_millis(50 * i);
            assert_eq!(f.update([0; 6], 1, at), Motion::default());
            assert_eq!(f.update([0; 6], 2, at), Motion::default());
        }
        assert!(f.calibrated());
        f
    }

    #[test]
    fn axes_deflect_and_expire() {
        let t0 = Instant::now();
        let mut f = calibrated(t0);
        let at = t0 + Duration::from_secs(2);
        let ms = Duration::from_millis;
        let m = f.update([500, 500, 0, 0, 0, 0], 1, at);
        assert!((m.pan[0] - 1.0).abs() < 1e-9);
        assert!((m.pan[1] - 1.0).abs() < 1e-9);
        assert_eq!(m.zoom, 0.0, "no rotation input");
        // A fresh rotation report drives the zoom.
        let m = f.update([0, 0, 0, 0, 0, 500], 2, at + ms(100));
        assert!((m.zoom - 1.0).abs() < 1e-9);
        // The rotation axes went quiet for over 250 ms while translations still arrive.
        let m = f.update([500, 500, 0, 0, 0, 0], 1, at + ms(400));
        assert_eq!(m.zoom, 0.0);
        // A fresh rotation report puts the zoom back.
        let m = f.update([500, 500, 0, 0, 0, 500], 2, at + ms(500));
        assert!((m.zoom - 1.0).abs() < 1e-9);
    }

    #[test]
    fn drift_tracks_rest_after_two_idle_seconds() {
        // Five axes units of drift sit inside the deadzone (FULL_SCALE * DEADZONE = 20).
        let mut f = Filter { raw: [5.0; 6], bias: Some([0.0; 6]), ..Filter::default() };
        let t0 = Instant::now();
        let mut b = [0.0; 6];
        f.update_drift(true, t0, &mut b); // starts the idle window: no shift yet
        assert_eq!(b, [0.0; 6]);
        f.update_drift(true, t0 + Duration::from_secs(3), &mut b);
        assert!((b[0] - 0.05).abs() < 1e-12, "bias follows resting drift");
        assert_eq!(f.bias, Some(b));
        // Motion outside the deadzone resets the idle window.
        f.raw = [100.0; 6];
        f.update_drift(true, t0 + Duration::from_secs(4), &mut b);
        assert_eq!(f.idle_since, None);
        f.update_drift(true, t0 + Duration::from_secs(5), &mut b);
        assert!((b[0] - 0.05).abs() < 1e-12, "no shift before two seconds at rest");
    }
}
