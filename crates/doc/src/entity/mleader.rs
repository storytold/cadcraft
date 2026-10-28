//! Leader roots of a multileader (DXF Reference: MULTILEADER, the `LEADER{` sections of its
//! context data). A multileader has one or more roots: a landing with its dogleg and the leader
//! lines that end there. The first is the multileader's own [`MLeader::leaders`],
//! [`MLeader::landing`] and [`MLeader::dogleg`]; further ones (a text with leaders on both sides
//! has a root on each side) are [`MLeader::roots`]. Each root's dogleg runs from its landing
//! towards the content, so a root right of the text has its dogleg pointing left.

use cadcraft_color::Color;
use cadcraft_geom::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::{Lineweight, MLeader};

pub(super) fn byblock() -> Color {
    Color::ByBlock
}
pub(super) fn is_byblock(c: &Color) -> bool {
    *c == Color::ByBlock
}
pub(super) fn byblock_name() -> String {
    "ByBlock".into()
}
pub(super) fn is_byblock_name(s: &str) -> bool {
    s.eq_ignore_ascii_case("ByBlock")
}
pub(super) fn byblock_lw() -> Lineweight {
    Lineweight::ByBlock
}
pub(super) fn is_byblock_lw(l: &Lineweight) -> bool {
    *l == Lineweight::ByBlock
}

/// A further leader root of a multileader: a landing with its own leader lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MLeaderRoot {
    /// Where its leader lines end and its dogleg starts.
    pub landing: Vec3,
    /// Length of its dogleg (0: none).
    pub dogleg: f64,
    /// Its leader lines, each from the arrowhead up to (not including) the landing.
    pub leaders: Vec<Vec<Vec3>>,
}

/// One leader root as [`MLeader::all_roots`] lists it.
#[derive(Clone, Copy, Debug)]
pub struct RootRef<'a> {
    pub landing: Vec3,
    pub dogleg: f64,
    pub leaders: &'a [Vec<Vec3>],
}

impl Default for MLeader {
    fn default() -> Self {
        MLeader {
            leaders: Vec::new(),
            landing: Vec3::ZERO,
            dogleg: 0.0,
            text: None,
            style: super::standard(),
            arrow_size: 1.0,
            spline: false,
            block: None,
            roots: Vec::new(),
            leader_color: Color::ByBlock,
            leader_linetype: byblock_name(),
            leader_lineweight: Lineweight::ByBlock,
            text_color: Color::ByBlock,
            text_frame: false,
            block_color: Color::ByBlock,
        }
    }
}

impl MLeader {
    /// Every leader root, the multileader's own first.
    pub fn all_roots(&self) -> impl Iterator<Item = RootRef<'_>> {
        std::iter::once(RootRef { landing: self.landing, dogleg: self.dogleg, leaders: &self.leaders }).chain(self.roots.iter().map(|r| RootRef {
            landing: r.landing,
            dogleg: r.dogleg,
            leaders: &r.leaders,
        }))
    }

    /// Every leader line with the landing it ends at, the first root's first.
    pub fn lines(&self) -> impl Iterator<Item = (&Vec<Vec3>, Vec3)> {
        self.all_roots().flat_map(|r| r.leaders.iter().map(move |l| (l, r.landing)))
    }

    /// How many leader lines the multileader has over all its roots.
    pub fn line_count(&self) -> usize {
        self.leaders.len().saturating_add(self.roots.iter().map(|r| r.leaders.len()).sum())
    }

    /// Where the content sits: the text's or the block's insertion point.
    pub fn content_point(&self) -> Option<Vec3> {
        self.text.as_ref().map(|t| t.insert).or(self.block.as_ref().map(|b| b.insert))
    }

    /// The way a dogleg from `landing` runs: +1 towards +X (the content is right of the
    /// landing or level with it), -1 towards -X.
    pub fn dogleg_dir(&self, landing: Vec3) -> f64 {
        match self.content_point() {
            Some(c) if c.x < landing.x => -1.0,
            _ => 1.0,
        }
    }

    /// The doglegs as drawn: from each landing towards the content. The first root's dogleg is
    /// drawn for text content whatever its leader lines; a further root's, and the first root's
    /// for block content, only when that root has leader lines.
    pub fn dogleg_lines(&self) -> Vec<[Vec2; 2]> {
        if self.content_point().is_none() {
            return Vec::new();
        }
        let text = self.text.is_some();
        self.all_roots()
            .enumerate()
            .filter(|(i, r)| r.dogleg > 0.0 && r.dogleg.is_finite() && ((*i == 0 && text) || !r.leaders.is_empty()))
            .map(|(_, r)| {
                let a = r.landing.xy();
                [a, a + Vec2::new(r.dogleg * self.dogleg_dir(r.landing), 0.0)]
            })
            .collect()
    }

    /// Apply `f` to every leader vertex and every landing (not to the content).
    pub fn for_each_leader_point(&mut self, mut f: impl FnMut(&mut Vec3)) {
        self.leaders.iter_mut().flatten().for_each(&mut f);
        f(&mut self.landing);
        for r in &mut self.roots {
            r.leaders.iter_mut().flatten().for_each(&mut f);
            f(&mut r.landing);
        }
    }

    /// The leader line `index` counted over all roots (the first root's first), mutably.
    pub fn line_mut(&mut self, index: usize) -> Option<&mut Vec<Vec3>> {
        self.leaders.iter_mut().chain(self.roots.iter_mut().flat_map(|r| r.leaders.iter_mut())).nth(index)
    }

    /// Remove leader line `index` (counted over all roots, the first root's first). A further
    /// root left without lines goes too; when the first root loses its last line, the next root
    /// takes its place. False when there is no such line.
    pub fn remove_line(&mut self, index: usize) -> bool {
        if index < self.leaders.len() {
            self.leaders.remove(index);
            if self.leaders.is_empty() && !self.roots.is_empty() {
                let r = self.roots.remove(0);
                (self.landing, self.dogleg, self.leaders) = (r.landing, r.dogleg, r.leaders);
            }
            return true;
        }
        let mut i = index - self.leaders.len();
        let mut found = None;
        for (k, r) in self.roots.iter_mut().enumerate() {
            if i < r.leaders.len() {
                r.leaders.remove(i);
                found = Some((k, r.leaders.is_empty()));
                break;
            }
            i -= r.leaders.len();
        }
        match found {
            Some((k, true)) => {
                self.roots.remove(k);
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// The leader lines of every root as lines to the first landing (a further root's landing
    /// becomes the last point of its lines): for a multileader that keeps one root.
    pub fn lines_to_first_root(&self) -> Vec<Vec<Vec3>> {
        let mut out = self.leaders.clone();
        for r in &self.roots {
            out.extend(r.leaders.iter().map(|l| l.iter().copied().chain(std::iter::once(r.landing)).collect()));
        }
        out
    }

    /// Move the content (every landing, the text, the block) by `d`; the arrowheads stay.
    pub fn shift_content(&mut self, d: Vec2) {
        let mv = |v: &mut Vec3| *v = Vec3::new(v.x + d.x, v.y + d.y, v.z);
        mv(&mut self.landing);
        for r in &mut self.roots {
            mv(&mut r.landing);
        }
        if let Some(t) = &mut self.text {
            mv(&mut t.insert);
        }
        if let Some(b) = &mut self.block {
            mv(&mut b.insert);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn json_without_overrides_reads_them_byblock_and_writes_none() {
        let json = r#"{"leaders":[],"landing":{"x":1,"y":1,"z":0},"dogleg":0.5,"text":null}"#;
        let m: MLeader = serde_json::from_str(json).unwrap_or_default();
        assert_eq!((m.leader_color, m.text_color, m.block_color), (Color::ByBlock, Color::ByBlock, Color::ByBlock));
        assert_eq!((m.leader_linetype.as_str(), m.leader_lineweight, m.text_frame), ("ByBlock", Lineweight::ByBlock, false));
        let back = serde_json::to_string(&m).unwrap_or_default();
        assert!(!back.contains("leaderColor") && !back.contains("textFrame"), "{back}");
        let set = MLeader { leader_color: Color::Index(1), leader_linetype: "DASHED".into(), text_frame: true, ..m };
        let json = serde_json::to_string(&set).unwrap_or_default();
        assert_eq!(serde_json::from_str::<MLeader>(&json).ok(), Some(set));
    }

    use super::*;
    use crate::MText;

    fn two_roots() -> MLeader {
        MLeader {
            leaders: vec![vec![Vec3::new(0.0, 0.0, 0.0)], vec![Vec3::new(0.0, 2.0, 0.0)]],
            landing: Vec3::new(4.0, 5.0, 0.0),
            dogleg: 1.0,
            text: Some(MText {
                insert: Vec3::new(5.5, 5.0, 0.0),
                height: 1.0,
                width: 0.0,
                attach: 4,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "A".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }),
            roots: vec![MLeaderRoot { landing: Vec3::new(12.0, 5.0, 0.0), dogleg: 1.0, leaders: vec![vec![Vec3::new(16.0, 0.0, 0.0)]] }],
            ..MLeader::default()
        }
    }

    #[test]
    fn roots_have_their_own_landing_and_dogleg_side() {
        let m = two_roots();
        assert_eq!(m.line_count(), 3);
        let paths = m.leader_paths();
        assert_eq!(paths.len(), 3);
        assert_eq!(paths.get(2).and_then(|p| p.last()).copied(), Some(Vec2::new(12.0, 5.0)));
        let doglegs = m.dogleg_lines();
        assert_eq!(doglegs, vec![[Vec2::new(4.0, 5.0), Vec2::new(5.0, 5.0)], [Vec2::new(12.0, 5.0), Vec2::new(11.0, 5.0)]]);
    }

    #[test]
    fn removing_the_last_line_of_a_root_drops_the_root() {
        let mut m = two_roots();
        assert!(!m.remove_line(3));
        assert!(m.remove_line(2));
        assert!(m.roots.is_empty());
        assert!(m.remove_line(0));
        assert_eq!(m.line_count(), 1);
        assert!(!m.remove_line(usize::MAX));
    }

    #[test]
    fn removing_the_first_roots_last_line_makes_the_next_root_first() {
        let mut m = two_roots();
        assert!(m.remove_line(0) && m.remove_line(0));
        assert!(m.roots.is_empty());
        assert_eq!((m.landing, m.dogleg, m.line_count()), (Vec3::new(12.0, 5.0, 0.0), 1.0, 1));
    }

    #[test]
    fn moving_mirroring_and_annotation_scaling_carry_every_root() {
        let e = crate::Entity::new(crate::Handle(1), crate::EntityKind::MLeader(two_roots()));
        let mut k = e.kind.clone();
        k.transform(&cadcraft_geom::Mat3::translate(Vec2::new(1.0, 2.0)));
        let crate::EntityKind::MLeader(moved) = k else { return };
        assert_eq!(moved.roots[0].landing, Vec3::new(13.0, 7.0, 0.0));
        assert_eq!(moved.roots[0].leaders[0][0], Vec3::new(17.0, 2.0, 0.0));
        // Grips: the three leader vertices, then both landings.
        assert_eq!(crate::EntityKind::MLeader(moved.clone()).grips().len(), 5);
        // Mirrored about x = 0 the second root is left of the text: its dogleg turns round.
        let mut k = e.kind.clone();
        k.transform(&cadcraft_geom::Mat3::mirror(Vec2::ZERO, Vec2::new(0.0, 1.0)));
        let crate::EntityKind::MLeader(mirrored) = k else { return };
        assert_eq!(mirrored.dogleg_dir(mirrored.roots[0].landing), 1.0);
        // At annotation scale 2 the second landing moves away as the text doubles.
        let crate::EntityKind::MLeader(big) = crate::anno_scaled(&e, 2.0).kind else { return };
        assert_eq!((big.roots[0].landing, big.roots[0].dogleg), (Vec3::new(20.0, 5.0, 0.0), 2.0));
    }

    #[test]
    fn lines_to_first_root_end_at_their_own_landing() {
        let m = two_roots();
        let l = m.lines_to_first_root();
        assert_eq!(l.len(), 3);
        assert_eq!(l.get(2), Some(&vec![Vec3::new(16.0, 0.0, 0.0), Vec3::new(12.0, 5.0, 0.0)]));
    }

    #[test]
    fn json_without_roots_reads_and_writes_without_them() {
        let json = r#"{"leaders":[[{"x":0,"y":0,"z":0}]],"landing":{"x":1,"y":1,"z":0},"dogleg":0.5,"text":null}"#;
        let m: MLeader = serde_json::from_str(json).unwrap_or_default();
        assert_eq!(m.landing, Vec3::new(1.0, 1.0, 0.0));
        assert!(m.roots.is_empty());
        let back = serde_json::to_string(&m).unwrap_or_default();
        assert!(!back.contains("roots"), "{back}");
        let two = serde_json::to_string(&two_roots()).unwrap_or_default();
        assert_eq!(serde_json::from_str::<MLeader>(&two).ok(), Some(two_roots()));
    }
}
