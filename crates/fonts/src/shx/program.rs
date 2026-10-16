//! Bounded interpreter of the publicly documented SHP/SHX pen commands.

use std::f64::consts::{FRAC_PI_4, TAU};

use cadcraft_geom::Vec2;

use super::{Cursor, Error, Font, Glyph};

const MAX_DEPTH: usize = 32;
const MAX_STACK: usize = 32;
const MAX_STEPS: usize = 65_536;
const MAX_POINTS: usize = 8192;
const MAX_COORD: f64 = 1.0e6;

struct Pen<'a> {
    font: &'a Font,
    pos: Vec2,
    down: bool,
    scale: f64,
    stack: Vec<Vec2>,
    strokes: Vec<Vec<Vec2>>,
    active: Vec<Vec2>,
    steps: usize,
    points: usize,
}

pub(super) fn decode(font: &Font, id: u16) -> Result<Glyph, Error> {
    let mut pen =
        Pen { font, pos: Vec2::ZERO, down: true, scale: 1.0, stack: Vec::new(), strokes: Vec::new(), active: Vec::new(), steps: 0, points: 0 };
    pen.run(id, 0)?;
    pen.flush();
    if !pen.stack.is_empty() {
        return Err(Error::Invalid);
    }
    Ok(Glyph { strokes: pen.strokes, advance: pen.pos.x })
}

impl Pen<'_> {
    fn step(&mut self) -> Result<(), Error> {
        self.steps += 1;
        if self.steps > MAX_STEPS { Err(Error::Limit) } else { Ok(()) }
    }

    fn flush(&mut self) {
        if self.active.len() >= 2 {
            self.strokes.push(std::mem::take(&mut self.active));
        } else {
            self.active.clear();
        }
    }

    fn move_to(&mut self, target: Vec2) -> Result<(), Error> {
        if !target.is_finite() || target.x.abs().max(target.y.abs()) > MAX_COORD {
            return Err(Error::Limit);
        }
        if self.down {
            let added = if self.active.is_empty() { 2 } else { 1 };
            self.points += added;
            if self.points > MAX_POINTS {
                return Err(Error::Limit);
            }
            if self.active.is_empty() {
                self.active.push(self.pos);
            }
            self.active.push(target);
        } else {
            self.flush();
        }
        self.pos = target;
        Ok(())
    }

    fn displacement(&mut self, dx: f64, dy: f64) -> Result<(), Error> {
        self.move_to(self.pos + Vec2::new(dx, dy) * self.scale)
    }

    fn arc(&mut self, radius: f64, start: f64, sweep: f64) -> Result<(), Error> {
        if !radius.is_finite() || radius <= 0.0 || radius > MAX_COORD || !sweep.is_finite() || sweep.abs() > TAU + 1e-8 {
            return Err(Error::Invalid);
        }
        let center = self.pos - Vec2::new(start.cos(), start.sin()) * radius;
        let count = (sweep.abs() / (TAU / 128.0)).ceil().clamp(1.0, 128.0) as usize;
        let count = if self.down { count } else { 1 };
        for i in 1..=count {
            let angle = start + sweep * i as f64 / count as f64;
            self.move_to(center + Vec2::new(angle.cos(), angle.sin()) * radius)?;
        }
        Ok(())
    }

    fn bulge(&mut self, dx: f64, dy: f64, bulge: f64) -> Result<(), Error> {
        let delta = Vec2::new(dx, dy) * self.scale;
        if bulge == 0.0 || !self.down {
            return self.move_to(self.pos + delta);
        }
        if bulge.abs() > 127.0 || delta.len2() == 0.0 {
            return Err(Error::Invalid);
        }
        let b = bulge / 127.0;
        let center = self.pos + delta * 0.5 + Vec2::new(-delta.y, delta.x) * ((1.0 - b * b) / (4.0 * b));
        let relative = self.pos - center;
        self.arc(relative.len(), relative.y.atan2(relative.x), 4.0 * b.atan())
    }

    fn run(&mut self, id: u16, depth: usize) -> Result<(), Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::Limit);
        }
        let program = self.font.programs.get(&id).ok_or(Error::Invalid)?;
        let mut r = Cursor::new(program);
        let mut skip = false;
        loop {
            self.step()?;
            let code = r.byte()?;
            let execute = !skip;
            skip = false;
            match code {
                0 => return if r.done() { Ok(()) } else { Err(Error::Invalid) },
                1 => {
                    if execute {
                        self.down = true;
                    }
                }
                2 => {
                    if execute {
                        self.down = false;
                        self.flush();
                    }
                }
                3 | 4 => {
                    let factor = f64::from(r.byte()?);
                    if factor == 0.0 {
                        return Err(Error::Invalid);
                    }
                    if execute {
                        self.scale = if code == 3 { self.scale / factor } else { self.scale * factor };
                        if !(1e-6..=MAX_COORD).contains(&self.scale) {
                            return Err(Error::Limit);
                        }
                    }
                }
                5 => {
                    if execute {
                        if self.stack.len() >= MAX_STACK {
                            return Err(Error::Limit);
                        }
                        self.stack.push(self.pos);
                    }
                }
                6 => {
                    if execute {
                        self.flush();
                        self.pos = self.stack.pop().ok_or(Error::Invalid)?;
                    }
                }
                7 => {
                    let id = if self.font.unicode {
                        // Unicode subshape operands are high byte first; record ids are LE.
                        (u16::from(r.byte()?) << 8) | u16::from(r.byte()?)
                    } else {
                        u16::from(r.byte()?)
                    };
                    if execute {
                        self.run(id, depth + 1)?;
                    }
                }
                8 | 9 | 12 | 13 => loop {
                    self.step()?;
                    let dx = f64::from(i8::from_ne_bytes([r.byte()?]));
                    let dy = f64::from(i8::from_ne_bytes([r.byte()?]));
                    if matches!(code, 9 | 13) && dx == 0.0 && dy == 0.0 {
                        break;
                    }
                    let bulge = if matches!(code, 12 | 13) { f64::from(i8::from_ne_bytes([r.byte()?])) } else { 0.0 };
                    if execute {
                        self.bulge(dx, dy, bulge)?;
                    }
                    if matches!(code, 8 | 12) {
                        break;
                    }
                },
                10 | 11 => {
                    let (offset, end, radius) = if code == 11 {
                        let offset = f64::from(r.byte()?) / 256.0;
                        let end = r.byte()?;
                        let end = if end == 0 { 1.0 } else { f64::from(end) / 256.0 };
                        let radius = (u16::from(r.byte()?) << 8) | u16::from(r.byte()?);
                        (offset, end, f64::from(radius))
                    } else {
                        (0.0, 1.0, f64::from(r.byte()?))
                    };
                    // The sign is sign-magnitude for arc octants, not two's-complement.
                    let spec = r.byte()?;
                    let sign = if spec & 128 == 0 { 1.0 } else { -1.0 };
                    let octant = (spec & 127) >> 4;
                    let span = spec & 15;
                    if octant > 7 || span > 7 {
                        return Err(Error::Invalid);
                    }
                    let span = if span == 0 { 8.0 } else { f64::from(span) };
                    let start = (f64::from(octant) + sign * offset) * FRAC_PI_4;
                    let sweep = sign * (span - 1.0 + end - offset) * FRAC_PI_4;
                    if execute {
                        self.arc(radius * self.scale, start, sweep)?;
                    }
                }
                14 => {
                    if self.font.dual && execute {
                        skip = true;
                    }
                }
                15 => return Err(Error::Unsupported),
                _ => {
                    let length = f64::from(code >> 4);
                    let direction = code & 15;
                    // Directions at 22.5-degree steps use half-grid intermediate vectors.
                    let (dx, dy) = match direction {
                        0 => (1.0, 0.0),
                        1 => (1.0, 0.5),
                        2 => (1.0, 1.0),
                        3 => (0.5, 1.0),
                        4 => (0.0, 1.0),
                        5 => (-0.5, 1.0),
                        6 => (-1.0, 1.0),
                        7 => (-1.0, 0.5),
                        8 => (-1.0, 0.0),
                        9 => (-1.0, -0.5),
                        10 => (-1.0, -1.0),
                        11 => (-0.5, -1.0),
                        12 => (0.0, -1.0),
                        13 => (0.5, -1.0),
                        14 => (1.0, -1.0),
                        _ => (1.0, -0.5),
                    };
                    if execute {
                        self.displacement(dx * length, dy * length)?;
                    }
                }
            }
        }
    }
}
