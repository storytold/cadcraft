use super::{Color, Common, Lineweight, Rgb, Tag, Transparency, Vec2, Vec3};

/// Value lookup helpers over an entity's tags.
pub(super) struct T<'a>(pub(super) &'a [Tag]);

impl T<'_> {
    pub(super) fn f(&self, code: i32) -> Option<f64> {
        self.0.iter().find(|t| t.code == code).map(Tag::f64)
    }
    pub(super) fn fd(&self, code: i32, d: f64) -> f64 {
        self.f(code).unwrap_or(d)
    }
    pub(super) fn i(&self, code: i32) -> Option<i64> {
        self.0.iter().find(|t| t.code == code).map(Tag::i64)
    }
    pub(super) fn s(&self, code: i32) -> Option<String> {
        self.0.iter().find(|t| t.code == code).map(Tag::str)
    }
    pub(super) fn p(&self, code: i32) -> Vec3 {
        Vec3::new(self.fd(code, 0.0), self.fd(code + 10, 0.0), self.fd(code + 20, 0.0))
    }
    pub(super) fn all_f(&self, code: i32) -> Vec<f64> {
        self.0.iter().filter(|t| t.code == code).map(Tag::f64).collect()
    }
    pub(super) fn pts(&self, code: i32) -> Vec<Vec2> {
        let xs = self.all_f(code);
        let ys = self.all_f(code + 10);
        xs.into_iter().zip(ys).map(|(x, y)| Vec2::new(x, y)).collect()
    }
}

/// An entity's group 440: `0x01` in the top byte is ByBlock, `0x02` a fixed alpha (mapped to a
/// percentage as for layers), anything else ByLayer.
fn transparency_440(v: i64) -> Transparency {
    match (v >> 24) & 0xff {
        1 => Transparency::ByBlock,
        2 => {
            let alpha = u32::try_from(v & 0xff).unwrap_or(255);
            let opaque = (alpha * 100 + 127) / 255;
            Transparency::Percent(u8::try_from(100u32.saturating_sub(opaque).min(90)).unwrap_or(90))
        }
        _ => Transparency::ByLayer,
    }
}

pub(super) fn common(t: &T) -> Common {
    let mut c = Common { layer: t.s(8).unwrap_or_else(|| "0".into()), linetype: t.s(6).unwrap_or_else(|| "ByLayer".into()), ..Common::default() };
    if let Some(v) = t.i(62) {
        c.color = Color::from_aci(v as i16);
        if v < 0 {
            c.color = Color::ByLayer;
            c.visible = true;
        }
    }
    if let Some(tc) = t.i(420) {
        c.color = Color::True(Rgb::from_u32(tc as u32));
    }
    if let Some(lw) = t.i(370) {
        c.lineweight = Lineweight::from_dxf(lw as i16);
    }
    if let Some(s) = t.f(48) {
        c.ltscale = if s > 0.0 { s } else { 1.0 };
    }
    if t.i(60) == Some(1) {
        c.visible = false;
    }
    if let Some(v) = t.i(440) {
        c.transparency = transparency_440(v);
    }
    c.thickness = t.fd(39, 0.0);
    c.extrusion = Vec3::new(t.fd(210, 0.0), t.fd(220, 0.0), t.fd(230, 1.0));
    c
}

/// Apply the OCS → WCS transform for 2D entities with a non-default extrusion.
pub(super) fn ocs(c: &Common, p: Vec3) -> Vec3 {
    ocs_n(c.extrusion, p)
}

pub(super) fn ocs_n(n: Vec3, p: Vec3) -> Vec3 {
    if (n.z - 1.0).abs() < 1e-12 && n.x.abs() < 1e-12 && n.y.abs() < 1e-12 { p } else { cadcraft_geom::Mat4::ocs(n).apply(p) }
}
