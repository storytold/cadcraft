use super::common::{Common, Lineweight};
use super::text::{MText, Text};
use super::{Color, Deserialize, EntityKind, Mat3, Serialize, Vec2, Vec3};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attrib {
    pub tag: String,
    pub text: Text,
    #[serde(default)]
    pub invisible: bool,
    #[serde(default)]
    pub constant: bool,
    #[serde(default)]
    pub prompt: String,
    /// An attribute's own properties; each one left unset follows the attribute definition.
    #[serde(default, skip_serializing_if = "AttribProps::is_empty")]
    pub props: AttribProps,
    /// A multiline attribute (DXF attribute type 2, or 4 for a definition): its text as
    /// multiline text. `text.value` holds the plain text, which value edits change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiline: Option<MultilineAttrib>,
    /// Its other modes: Verify, Preset and Lock position.
    #[serde(default, skip_serializing_if = "AttribModes::is_default")]
    pub modes: AttribModes,
}

/// An attribute's modes beyond invisible and constant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttribModes {
    /// Verify: the value is asked for twice when the block is inserted (DXF flag 4).
    #[serde(default)]
    pub verify: bool,
    /// Preset: the default value is used without asking (DXF flag 8).
    #[serde(default)]
    pub preset: bool,
    /// Lock position: the attribute can't be moved on its own within a reference (DXF group
    /// 280 after the flags). A reference's attributes take it from their definitions.
    #[serde(default)]
    pub lock_position: bool,
}

impl AttribModes {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    /// The modes of a reference's attribute made from a definition with these: AutoCAD keeps
    /// only Lock position there.
    pub fn for_reference(self) -> Self {
        AttribModes { lock_position: self.lock_position, ..Self::default() }
    }
}

/// The multiline text of a multiline attribute.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultilineAttrib {
    pub mtext: MText,
    /// The plain text `mtext`'s contents read as: an attribute whose value differs was edited
    /// since, and its value replaces the contents.
    pub read_as: String,
}

impl MultilineAttrib {
    /// This text placed by `m` (a block reference's transform, for its attribute made from a
    /// definition).
    pub fn transformed(&self, m: &Mat3) -> MultilineAttrib {
        let mut k = EntityKind::MText(self.mtext.clone());
        k.transform(m);
        let mtext = if let EntityKind::MText(t) = k { t } else { self.mtext.clone() };
        MultilineAttrib { mtext, read_as: self.read_as.clone() }
    }
}

impl Attrib {
    /// Move a multiline attribute's text by `m` along with the attribute (nothing for others).
    pub fn transform_multiline(&mut self, m: &Mat3) {
        if let Some(ml) = &mut self.multiline {
            *ml = ml.transformed(m);
        }
    }

    /// A multiline attribute's text as drawn and saved: its multiline text, with the contents
    /// replaced by the value when the value was edited since it was read.
    pub fn multiline_text(&self) -> Option<MText> {
        let m = self.multiline.as_ref()?;
        let mut t = m.mtext.clone();
        if self.text.value != m.read_as {
            t.contents = mtext_literal(&self.text.value);
        }
        Some(t)
    }
}

/// `s` as MTEXT contents that read back as `s`: backslashes and braces escaped, line breaks as
/// paragraph breaks.
pub fn mtext_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\P"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Properties set on one attribute of a block reference (an ATTRIB's layer, colour, linetype and
/// lineweight), overriding those of its definition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AttribProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lineweight: Option<Lineweight>,
}

impl AttribProps {
    /// Nothing set: the attribute is drawn with its definition's properties.
    pub fn is_empty(&self) -> bool {
        *self == AttribProps::default()
    }
    /// `base` (the definition's properties) with the ones set here in place.
    pub fn over(&self, base: &Common) -> Common {
        Common {
            layer: self.layer.clone().unwrap_or_else(|| base.layer.clone()),
            color: self.color.unwrap_or(base.color),
            linetype: self.linetype.clone().unwrap_or_else(|| base.linetype.clone()),
            lineweight: self.lineweight.unwrap_or(base.lineweight),
            ..base.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insert {
    pub block: String,
    pub insert: Vec3,
    #[serde(default = "unit_scale")]
    pub scale: Vec3,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub attribs: Vec<Attrib>,
    #[serde(default = "one_u")]
    pub cols: u32,
    #[serde(default = "one_u")]
    pub rows: u32,
    #[serde(default)]
    pub col_spacing: f64,
    #[serde(default)]
    pub row_spacing: f64,
}

fn unit_scale() -> Vec3 {
    Vec3::new(1.0, 1.0, 1.0)
}

fn one_u() -> u32 {
    1
}

impl Insert {
    /// Block-to-world transform (2D part).
    pub fn transform(&self, base: Vec2) -> Mat3 {
        Mat3::translate(self.insert.xy())
            .then_before(Mat3::rotate(self.rotation))
            .then_before(Mat3::scale(self.scale.x, self.scale.y))
            .then_before(Mat3::translate(-base))
    }
}
