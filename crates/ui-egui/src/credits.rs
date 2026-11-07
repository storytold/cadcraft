//! The About window's credits: every contributor (GitHub login always; display and real names only
//! with recorded consent) and the AI models that helped. The tables are baked into the binary by
//! build.rs from contributors/contributors.json; nothing is
//! read at run time.

use std::cmp::Ordering;

use egui::RichText;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub login: &'static str,
    pub display_name: Option<&'static str>,
    pub real_name: Option<&'static str>,
    pub prs: u64,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary_added: u64,
    pub binary_deleted: u64,
    /// ISO 8601 UTC; empty for someone credited only through a merged PR.
    pub first_commit: &'static str,
    pub last_commit: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub company: &'static str,
    pub model: &'static str,
    pub version: &'static str,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
}

include!(concat!(env!("OUT_DIR"), "/credits.rs"));

/// Which name to show. Display and real names fall back to the `@username` when not given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NameMode {
    #[default]
    Username,
    DisplayName,
    RealName,
}

impl NameMode {
    pub const ALL: [NameMode; 3] = [NameMode::Username, NameMode::DisplayName, NameMode::RealName];

    pub fn label(self) -> &'static str {
        match self {
            NameMode::Username => "Username",
            NameMode::DisplayName => "Display name",
            NameMode::RealName => "Real name",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Prs,
    Commits,
    LinesAdded,
    LinesDeleted,
    LinesDelta,
    BinaryAdded,
    BinaryDeleted,
    #[default]
    FirstCommit,
    LastCommit,
}

impl SortKey {
    pub const ALL: [SortKey; 10] = [
        SortKey::Name,
        SortKey::Prs,
        SortKey::Commits,
        SortKey::LinesAdded,
        SortKey::LinesDeleted,
        SortKey::LinesDelta,
        SortKey::BinaryAdded,
        SortKey::BinaryDeleted,
        SortKey::FirstCommit,
        SortKey::LastCommit,
    ];

    /// Menu label and table header.
    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            SortKey::Name => ("Name (A–Z)", "Name"),
            SortKey::Prs => ("Merged PRs", "PRs"),
            SortKey::Commits => ("Commits", "Commits"),
            SortKey::LinesAdded => ("Lines added", "+LOC"),
            SortKey::LinesDeleted => ("Lines deleted", "−LOC"),
            SortKey::LinesDelta => ("Line delta", "ΔLOC"),
            SortKey::BinaryAdded => ("Binary assets added", "+Bin"),
            SortKey::BinaryDeleted => ("Binary assets removed", "−Bin"),
            SortKey::FirstCommit => ("First commit", "First"),
            SortKey::LastCommit => ("Last commit", "Last"),
        }
    }

    /// Names and dates read naturally oldest/A first; counts read biggest first.
    pub fn default_ascending(self) -> bool {
        matches!(self, SortKey::Name | SortKey::FirstCommit | SortKey::LastCommit)
    }
}

impl Contributor {
    pub fn name(&self, mode: NameMode) -> String {
        let chosen = match mode {
            NameMode::Username => None,
            NameMode::DisplayName => self.display_name,
            NameMode::RealName => self.real_name,
        };
        chosen.map_or_else(|| format!("@{}", self.login), str::to_string)
    }

    pub fn lines_delta(&self) -> i128 {
        i128::from(self.lines_added) - i128::from(self.lines_deleted)
    }

    /// One line with everything we know, for tooltips.
    pub fn summary(&self) -> String {
        format!(
            "@{}: {} PRs, {} commits, +{} / −{} lines (Δ {}), +{} / −{} binary assets, {} – {}",
            self.login,
            self.prs,
            self.commits,
            group(self.lines_added),
            group(self.lines_deleted),
            signed(self.lines_delta()),
            self.binary_added,
            self.binary_deleted,
            day(self.first_commit),
            day(self.last_commit),
        )
    }
}

/// Alphabetical key: case-insensitive, ignoring the `@` of usernames.
fn name_key(name: &str) -> String {
    name.trim_start_matches('@').to_lowercase()
}

fn compare(a: &Contributor, b: &Contributor, mode: NameMode, key: SortKey) -> Ordering {
    match key {
        SortKey::Name => name_key(&a.name(mode)).cmp(&name_key(&b.name(mode))),
        SortKey::Prs => a.prs.cmp(&b.prs),
        SortKey::Commits => a.commits.cmp(&b.commits),
        SortKey::LinesAdded => a.lines_added.cmp(&b.lines_added),
        SortKey::LinesDeleted => a.lines_deleted.cmp(&b.lines_deleted),
        SortKey::LinesDelta => a.lines_delta().cmp(&b.lines_delta()),
        SortKey::BinaryAdded => a.binary_added.cmp(&b.binary_added),
        SortKey::BinaryDeleted => a.binary_deleted.cmp(&b.binary_deleted),
        // A PR-only contributor (no commit date) sorts after everyone with a date.
        SortKey::FirstCommit => dated(a.first_commit).cmp(&dated(b.first_commit)),
        SortKey::LastCommit => dated(a.last_commit).cmp(&dated(b.last_commit)),
    }
}

fn dated(d: &str) -> (bool, &str) {
    (d.is_empty(), d)
}

/// Contributors in display order. Ties fall back to the name (A–Z) so the order is stable.
pub fn sorted(list: &[Contributor], mode: NameMode, key: SortKey, ascending: bool) -> Vec<&Contributor> {
    let mut v: Vec<&Contributor> = list.iter().collect();
    v.sort_by(|a, b| {
        let o = compare(a, b, mode, key);
        let o = if ascending { o } else { o.reverse() };
        o.then_with(|| compare(a, b, mode, SortKey::Name)).then_with(|| a.login.cmp(b.login))
    });
    v
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn signed(n: i128) -> String {
    let g = group(u64::try_from(n.unsigned_abs()).unwrap_or(u64::MAX));
    if n < 0 { format!("−{g}") } else { format!("+{g}") }
}

fn day(iso: &str) -> &str {
    if iso.is_empty() { "—" } else { iso.get(..10).unwrap_or(iso) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct View {
    names: NameMode,
    key: SortKey,
    ascending: bool,
    table: bool,
}

impl Default for View {
    fn default() -> Self {
        View { names: NameMode::Username, key: SortKey::FirstCommit, ascending: true, table: false }
    }
}

/// About ▸ Contributors: a name toggle, a sort, and the list as a grab bag or a table.
pub fn contributors_ui(ui: &mut egui::Ui) {
    let id = egui::Id::unique("credits_view");
    let mut v = ui.data_mut(|d| d.get_temp::<View>(id)).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.label(crate::tl!("Show"));
        for m in NameMode::ALL {
            if ui.selectable_label(v.names == m, crate::i18n::t(m.label())).clicked() {
                v.names = m;
            }
        }
        ui.separator();
        ui.label(crate::tl!("Sort"));
        craft_ui::ComboBox::from_id_salt("credits_sort").selected_text(crate::i18n::t(v.key.label().0)).show_ui(ui, |ui| {
            for k in SortKey::ALL {
                if ui.selectable_label(v.key == k, crate::i18n::t(k.label().0)).clicked() {
                    v.key = k;
                    v.ascending = k.default_ascending();
                }
            }
        });
        if ui.button(if v.ascending { "▲" } else { "▼" }).on_hover_text(crate::tl!("Reverse the order")).clicked() {
            v.ascending = !v.ascending;
        }
        ui.separator();
        if ui.selectable_label(!v.table, crate::tl!("Grab bag")).clicked() {
            v.table = false;
        }
        if ui.selectable_label(v.table, crate::tl!("Table")).clicked() {
            v.table = true;
        }
    });
    let list = sorted(CONTRIBUTORS, v.names, v.key, v.ascending);
    ui.label(
        RichText::new(crate::tf!(
            "{contributors} · {commits}",
            contributors = crate::i18n::count(list.len() as u64, "{n} contributor", "{n} contributors"),
            commits = crate::i18n::count(TOTAL_COMMITS, "{n} commit", "{n} commits")
        ))
        .small()
        .weak(),
    );
    ui.separator();
    if list.is_empty() {
        ui.label(crate::tl!("No contributor data was built into this copy."));
    } else if v.table {
        // The table scrolls vertically itself, under a fixed header; it only scrolls sideways
        // once its columns are dragged wider than the dialog.
        egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| table(ui, &list, &mut v));
    } else {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, c) in list.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("·").weak());
                    }
                    ui.hyperlink_to(c.name(v.names), format!("https://github.com/{}", c.login)).on_hover_text(c.summary());
                }
            });
        });
    }
    ui.data_mut(|d| d.insert_temp(id, v));
}

/// How the contributor table's spare width is shared out: all of it to Name.
const CONTRIBUTOR_GROW: [f32; 10] = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

/// How the model table's spare width is shared out: to Company and Model, the text columns.
const MODEL_GROW: [f32; 6] = [1.0, 2.0, 0.0, 0.0, 0.0, 0.0];

/// The narrowest a column can be dragged.
const MIN_COLUMN: f32 = 28.0;

/// The width a column needs for its header and its widest cell.
fn natural(ui: &egui::Ui, header: RichText, cells: impl Iterator<Item = String>) -> f32 {
    let measure = |text: egui::WidgetText| text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body).size().x;
    let widest = cells.map(|c| measure(c.into())).fold(measure(header.into()), f32::max);
    widest + 2.0 * ui.spacing().button_padding.x
}

/// Opening column widths: each column's natural width, with the spare room shared out by `grow`,
/// so the columns fill `room` exactly. When there is too little room, only the growing columns
/// give way, each in proportion to its width above [`MIN_COLUMN`], and none goes below that.
fn fit_widths(natural: &[f32], grow: &[f32], room: f32) -> Vec<f32> {
    let spare = room - natural.iter().sum::<f32>();
    let weight = grow.iter().sum::<f32>();
    let slack = |w: f32| (w - MIN_COLUMN).max(0.0);
    let flexible: f32 = natural.iter().zip(grow).filter(|(_, g)| **g > 0.0).map(|(w, _)| slack(*w)).sum();
    natural
        .iter()
        .zip(grow)
        .map(|(w, g)| {
            if *g > 0.0 && spare >= 0.0 {
                w + spare * g / weight
            } else if *g > 0.0 && flexible > 0.0 {
                w - (-spare).min(flexible) * slack(*w) / flexible
            } else {
                *w
            }
        })
        .collect()
}

/// A striped table whose columns open at [`fit_widths`], so it fills the width and nothing scrolls
/// sideways. Every column, the last included, can be dragged wider or narrower at its right edge:
/// only a table dragged wider than the window scrolls sideways. Text too long for its column is
/// clipped. When a column's natural width or the window changes, the columns go back to a new fit,
/// dropping any dragged widths.
fn fitted_table<'a>(ui: &'a mut egui::Ui, id: &str, natural: &[f32], grow: &[f32]) -> egui_extras::TableBuilder<'a> {
    // Every cell is followed by the item gap, and the vertical scrollbar keeps its margins.
    let s = ui.spacing();
    let gaps = s.item_spacing.x * natural.len() as f32;
    let bar = s.scroll.bar_width + s.scroll.bar_inner_margin + s.scroll.bar_outer_margin;
    let widths = fit_widths(natural, grow, (ui.available_width() - gaps - bar).max(0.0));
    // The table keeps its own column widths, so a change in what they were fitted to has to clear
    // them. A longer name can leave the fit itself unchanged, and scrolling can nudge the available
    // width, so this compares the natural widths and the clip rect, which scrolling never moves.
    let fit = egui::Id::unique(("credits_fit", id));
    let measured = (natural.to_vec(), ui.clip_rect().width());
    let refit = ui.data_mut(|d| d.get_temp::<(Vec<f32>, f32)>(fit)).as_ref() != Some(&measured);
    ui.data_mut(|d| d.insert_temp(fit, measured));
    let mut t =
        egui_extras::TableBuilder::new(ui).id_salt(id).striped(true).resizable(true).auto_shrink([false, false]).max_scroll_height(f32::INFINITY);
    if refit {
        t.reset();
    }
    for w in widths {
        t = t.column(egui_extras::Column::initial(w).at_least(MIN_COLUMN).clip(true));
    }
    t
}

/// Add a cell's widget right-aligned (counts, dates) or left-aligned (names), vertically centred.
fn aligned<R>(ui: &mut egui::Ui, right: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let layout = if right { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
    ui.with_layout(layout, add).inner
}

fn row_height(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body) + 6.0
}

/// One contributor's table cells after the name, in `SortKey::ALL` order.
fn cells(c: &Contributor) -> [String; 9] {
    [
        group(c.prs),
        group(c.commits),
        group(c.lines_added),
        group(c.lines_deleted),
        signed(c.lines_delta()),
        group(c.binary_added),
        group(c.binary_deleted),
        day(c.first_commit).to_string(),
        day(c.last_commit).to_string(),
    ]
}

/// A contributor-table header. The sorted column is highlighted; its direction shows on the
/// toolbar's sort-order button, so no arrow widens the column.
fn header_text(k: SortKey) -> RichText {
    RichText::new(crate::i18n::t(k.label().1)).strong()
}

fn table(ui: &mut egui::Ui, list: &[&Contributor], v: &mut View) {
    let h = row_height(ui);
    let rows: Vec<[String; 9]> = list.iter().map(|c| cells(c)).collect();
    let widths: Vec<f32> = SortKey::ALL
        .iter()
        .enumerate()
        .map(|(i, &k)| match i.checked_sub(1) {
            None => natural(ui, header_text(k), list.iter().map(|c| c.name(v.names))),
            Some(col) => natural(ui, header_text(k), rows.iter().filter_map(|r| r.get(col).cloned())),
        })
        .collect();
    fitted_table(ui, "credits_table", &widths, &CONTRIBUTOR_GROW)
        .header(h + 4.0, |mut header| {
            for k in SortKey::ALL {
                header.col(|ui| {
                    let r = aligned(ui, k != SortKey::Name, |ui| ui.selectable_label(v.key == k, header_text(k)))
                        .on_hover_text(crate::i18n::t(k.label().0));

                    if r.clicked() {
                        if v.key == k {
                            v.ascending = !v.ascending;
                        } else {
                            v.key = k;
                            v.ascending = k.default_ascending();
                        }
                    }
                });
            }
        })
        .body(|mut body| {
            for (c, cells) in list.iter().zip(rows) {
                body.row(h, |mut row| {
                    row.col(|ui| {
                        aligned(ui, false, |ui| ui.hyperlink_to(c.name(v.names), format!("https://github.com/{}", c.login)))
                            .on_hover_text(c.summary());
                    });
                    for text in cells {
                        row.col(|ui| {
                            aligned(ui, true, |ui| ui.label(text));
                        });
                    }
                });
            }
        });
}

/// About ▸ Models: AI models credited in Co-Authored-By trailers.
pub fn models_ui(ui: &mut egui::Ui) {
    if MODELS.is_empty() {
        ui.label(crate::tl!("No model credits were built into this copy."));
        return;
    }
    let assisted: u64 = MODELS.iter().map(|m| m.commits).max().unwrap_or(0).max(1);
    let h = row_height(ui);
    let titles = ["Company", "Model", "Version", "Commits", "% of all commits", "Lines +/−"];
    // Company, Model and Version read as text; the counts line up on the right.
    let right = [false, false, false, true, true, true];
    let rows: Vec<[String; 6]> = MODELS
        .iter()
        .map(|m| {
            [
                m.company.to_string(),
                m.model.to_string(),
                m.version.to_string(),
                group(m.commits),
                format!("{:.0}%", 100.0 * m.commits as f64 / TOTAL_COMMITS.max(assisted) as f64),
                format!("+{} / −{}", group(m.lines_added), group(m.lines_deleted)),
            ]
        })
        .collect();
    let widths: Vec<f32> = titles
        .iter()
        .enumerate()
        .map(|(i, title)| natural(ui, RichText::new(crate::i18n::t(title)).strong(), rows.iter().filter_map(|r| r.get(i).cloned())))
        .collect();
    egui::ScrollArea::horizontal().id_salt("credits_models").auto_shrink([false, false]).show(ui, |ui| {
        fitted_table(ui, "credits_models", &widths, &MODEL_GROW)
            .header(h + 4.0, |mut header| {
                for (title, r) in titles.into_iter().zip(right) {
                    header.col(|ui| {
                        aligned(ui, r, |ui| ui.label(RichText::new(crate::i18n::t(title)).strong()));
                    });
                }
            })
            .body(|mut body| {
                for cells in rows {
                    body.row(h, |mut row| {
                        for (text, r) in cells.into_iter().zip(right) {
                            row.col(|ui| {
                                aligned(ui, r, |ui| ui.label(text));
                            });
                        }
                    });
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(
        login: &'static str,
        display: Option<&'static str>,
        real: Option<&'static str>,
        added: u64,
        deleted: u64,
        first: &'static str,
    ) -> Contributor {
        Contributor {
            login,
            display_name: display,
            real_name: real,
            prs: 0,
            commits: 1,
            lines_added: added,
            lines_deleted: deleted,
            binary_added: 0,
            binary_deleted: 0,
            first_commit: first,
            last_commit: first,
        }
    }

    fn people() -> [Contributor; 4] {
        [
            c("zed", None, None, 5, 50, "2026-01-02T00:00:00Z"),
            c("Bob", Some("Ann"), None, 10, 0, "2026-01-01T00:00:00Z"),
            c("alice", None, None, 7, 1, ""),
            c("carol", None, Some("Dana"), 1, 0, "2026-01-03T00:00:00Z"),
        ]
    }

    fn logins(v: &[&Contributor]) -> Vec<&'static str> {
        v.iter().map(|c| c.login).collect()
    }

    #[test]
    fn names_fall_back_to_the_username() {
        let p = c("bob", Some("Bobby"), None, 0, 0, "");
        assert_eq!(p.name(NameMode::Username), "@bob");
        assert_eq!(p.name(NameMode::DisplayName), "Bobby");
        assert_eq!(p.name(NameMode::RealName), "@bob");
    }

    #[test]
    fn alphabetical_ignores_at_and_case() {
        let all = people();
        let v = sorted(&all, NameMode::Username, SortKey::Name, true);
        assert_eq!(logins(&v), ["alice", "Bob", "carol", "zed"]);
        let v = sorted(&all, NameMode::DisplayName, SortKey::Name, true);
        assert_eq!(logins(&v), ["alice", "Bob", "carol", "zed"]); // "Ann" sorts with the @usernames
        let v = sorted(&all, NameMode::RealName, SortKey::Name, false);
        assert_eq!(logins(&v), ["zed", "carol", "Bob", "alice"]); // "Dana" between bob and zed
    }

    #[test]
    fn delta_and_dates() {
        let all = people();
        let v = sorted(&all, NameMode::Username, SortKey::LinesDelta, false);
        assert_eq!(logins(&v), ["Bob", "alice", "carol", "zed"]);
        let v = sorted(&all, NameMode::Username, SortKey::FirstCommit, true);
        assert_eq!(logins(&v), ["Bob", "zed", "carol", "alice"]); // undated last
    }

    #[test]
    fn formatting() {
        assert_eq!(group(1234567), "1,234,567");
        assert_eq!(group(12), "12");
        assert_eq!(signed(-1000), "−1,000");
        assert_eq!(day("2026-10-07T18:45:23Z"), "2026-10-07");
        assert_eq!(day(""), "—");
    }

    #[test]
    fn columns_fill_the_room_and_spare_width_goes_to_growing_columns() {
        assert_eq!(fit_widths(&[100.0, 40.0, 60.0], &[1.0, 0.0, 0.0], 300.0), [200.0, 40.0, 60.0]);
        assert_eq!(fit_widths(&[100.0, 40.0, 60.0], &[1.0, 2.0, 0.0], 230.0), [110.0, 60.0, 60.0]);
        let w = fit_widths(&[123.0, 45.5, 67.25], &[1.0, 0.0, 2.0], 512.0);
        assert!((w.iter().sum::<f32>() - 512.0).abs() < 0.01, "a fitted table neither scrolls nor leaves a gap: {w:?}");
    }

    #[test]
    fn too_little_room_shrinks_only_growing_columns() {
        assert_eq!(fit_widths(&[100.0, 40.0, 60.0], &[1.0, 0.0, 0.0], 150.0), [50.0, 40.0, 60.0]);
        assert_eq!(fit_widths(&[100.0, 40.0, 60.0], &[1.0, 0.0, 0.0], 0.0), [MIN_COLUMN, 40.0, 60.0]);
        assert_eq!(fit_widths(&[100.0, 40.0], &[0.0, 0.0], 50.0), [100.0, 40.0]);
    }

    #[test]
    fn a_column_at_the_minimum_leaves_the_shortfall_to_the_others() {
        let w = fit_widths(&[100.0, 300.0, 50.0], &[1.0, 1.0, 0.0], 140.0);
        assert!((w.iter().sum::<f32>() - 140.0).abs() < 0.01, "the columns still fit the room: {w:?}");
        assert!(w.iter().all(|c| *c >= MIN_COLUMN), "no column goes below the minimum: {w:?}");
        assert_eq!(fit_widths(&[100.0, 300.0, 50.0], &[1.0, 1.0, 0.0], 0.0), [MIN_COLUMN, MIN_COLUMN, 50.0]);
    }

    const WINDOW: f32 = 640.5;
    const NATURAL: [f32; 3] = [120.0, 80.0, 80.0];
    const GROW: [f32; 3] = [1.0, 0.0, 0.0];

    /// One frame of a table long enough to scroll; the rects of its header cells.
    fn table_frame(ctx: &egui::Context, width: f32, natural: &[f32], events: Vec<egui::Event>) -> Vec<egui::Rect> {
        scrolled_frame(ctx, width, natural, 0.0, events)
    }

    /// [`table_frame`], scrolled `offset` points sideways.
    fn scrolled_frame(ctx: &egui::Context, width: f32, natural: &[f32], offset: f32, events: Vec<egui::Event>) -> Vec<egui::Rect> {
        let screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 300.0)));
        let raw = egui::RawInput { screen_rect, events, ..Default::default() };
        let mut cells = Vec::new();
        let out = ctx.run_ui(raw, |ui| {
            cells.clear();
            egui::ScrollArea::horizontal().auto_shrink([false, false]).horizontal_scroll_offset(offset).show(ui, |ui| {
                fitted_table(ui, "test", natural, &GROW)
                    .header(20.0, |mut header| {
                        for _ in natural {
                            header.col(|ui| cells.push(ui.max_rect()));
                        }
                    })
                    .body(|mut body| {
                        for _ in 0..40 {
                            body.row(20.0, |mut row| {
                                for _ in natural {
                                    row.col(|ui| {
                                        ui.label("x");
                                    });
                                }
                            });
                        }
                    });
            });
        });
        out.drop_without_applying_deltas();
        cells
    }

    /// A new context with the table open and settled, and the rects of its header cells.
    fn opened_table() -> (egui::Context, Vec<egui::Rect>) {
        let ctx = egui::Context::default();
        table_frame(&ctx, WINDOW, &NATURAL, Vec::new());
        let cells = table_frame(&ctx, WINDOW, &NATURAL, Vec::new());
        (ctx, cells)
    }

    /// Drags a column's right edge, which runs midway through the gap after `cell`, then lets go.
    fn drag_edge(ctx: &egui::Context, cell: egui::Rect, by: f32) {
        use egui::{Event, Modifiers, PointerButton};
        let start = egui::pos2(cell.right() + 4.0, cell.center().y);
        let halfway = start + egui::vec2(by / 2.0, 0.0);
        let end = start + egui::vec2(by, 0.0);
        let moved = Event::PointerMoved;
        let button = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for event in [moved(start), button(start, true), moved(halfway), moved(end), button(end, false)] {
            table_frame(ctx, WINDOW, &NATURAL, vec![event]);
        }
    }

    #[test]
    fn every_column_can_be_dragged_at_its_right_edge_and_stays_there() {
        let (_, opened) = opened_table();
        for (dragged, cell) in opened.iter().enumerate() {
            let (ctx, _) = opened_table();
            drag_edge(&ctx, *cell, -30.0);
            table_frame(&ctx, WINDOW, &NATURAL, Vec::new());
            let after = table_frame(&ctx, WINDOW, &NATURAL, Vec::new());
            for (column, (was, now)) in opened.iter().zip(&after).enumerate() {
                let want = if column == dragged { was.width() - 30.0 } else { was.width() };
                assert!((now.width() - want).abs() < 1.0, "dragging column {dragged}: column {column} is {now:?}, was {was:?}");
            }
        }
    }

    #[test]
    fn a_changed_fit_replaces_the_column_widths() {
        let (ctx, opened) = opened_table();
        drag_edge(&ctx, opened[0], -30.0);
        let longer_name = table_frame(&ctx, WINDOW, &[200.0, 80.0, 80.0], Vec::new());
        assert!((longer_name[0].width() - opened[0].width()).abs() < 1.0, "a longer name drops a dragged width: {longer_name:?}");
        let wider = table_frame(&ctx, WINDOW, &[120.0, 200.0, 80.0], Vec::new());
        let widths: Vec<f32> = wider.iter().map(|c| c.width().round()).collect();
        assert!(widths.ends_with(&[200.0, 80.0]), "a longer cell widens its column: {widths:?}");
        assert!(wider[2].right() < WINDOW, "the table still fits the window: {wider:?}");
        let narrower = table_frame(&ctx, 400.0, &NATURAL, Vec::new());
        assert!(narrower[2].right() < 400.0, "a narrower window refits the table: {narrower:?}");
    }

    #[test]
    fn scrolling_sideways_keeps_dragged_widths() {
        let (ctx, opened) = opened_table();
        drag_edge(&ctx, opened[0], 100.0);
        table_frame(&ctx, WINDOW, &NATURAL, Vec::new());
        let dragged = table_frame(&ctx, WINDOW, &NATURAL, Vec::new());
        assert!((dragged[0].width() - opened[0].width() - 100.0).abs() < 1.0, "the drag took: {dragged:?}");
        for offset in [13.3, 27.77, 41.1, 0.6] {
            scrolled_frame(&ctx, WINDOW, &NATURAL, offset, Vec::new());
            let scrolled = scrolled_frame(&ctx, WINDOW, &NATURAL, offset, Vec::new());
            assert!((scrolled[0].width() - dragged[0].width()).abs() < 1.0, "scrolled {offset}: {scrolled:?}");
        }
    }
}
