//! Blame of a file at any revision: back/forward navigation, a timeline of
//! the file's commits, Go to line (Ctrl+G) and line selection.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, FontId, Key, Rect, Sense, pos2, vec2};
use ferrum_core::{FileCommit, Oid, Repo, short};

use crate::command::{Command, Commands};
use crate::tab::RepoTab;
use crate::ui::blame::{FileView, Load, Mode, loading_or_error};
use crate::ui::file_lines;
use crate::util::{format_date, format_day};

const BLAME_ROW: f32 = 18.0;
const HASH_W: f32 = 62.0;
const AUTHOR_W: f32 = 140.0;
const DATE_W: f32 = 82.0;
const GUTTER_W: f32 = HASH_W + AUTHOR_W + DATE_W + 12.0;
const NUMBER_W: f32 = 52.0;
const TIMELINE_H: f32 = 24.0;

/// A place in the blame: revision (`None`: the view's branch), path there and
/// first visible line (0-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pos {
    pub rev: Option<Oid>,
    pub path: String,
    pub line: usize,
}

struct BlameRow {
    commit: Oid,
    block_start: bool,
    /// Alternates between blocks, for the background.
    odd_block: bool,
    /// Shown only on the first line of a block.
    gutter: Option<(String, String)>,
    text: String,
    /// Line number in the commit that last changed it (1-based).
    orig_line: u32,
    /// Age of the line's commit among the file's commits: 0 oldest, 1 newest.
    heat: f32,
}

struct CommitMeta {
    summary: String,
    date: String,
    /// Path of the file in the commit.
    path: String,
    /// Parent commit and path there, if the file existed before.
    previous: Option<(Oid, String)>,
}

pub(crate) struct BlameData {
    rows: Vec<BlameRow>,
    commits: HashMap<Oid, CommitMeta>,
    /// Longest line in characters, for horizontal scrolling.
    longest: usize,
    /// Dates of the oldest and newest commit, for the age legend.
    age_range: Option<(String, String)>,
}

pub struct BlameState {
    pub at: Pos,
    data: Load<BlameData>,
    /// What `data` was loaded for: (revision, path).
    loaded: Option<(String, String)>,
    back: Vec<Pos>,
    forward: Vec<Pos>,
    /// Line to bring into view on the next frame.
    scroll_to: Option<usize>,
    top_line: usize,
    /// Selected lines (anchor, end), 0-based.
    pub sel: Option<(usize, usize)>,
    /// Text of the Go to line field, while it is open.
    pub goto: Option<String>,
    goto_focus: bool,
    /// Commit under the pointer in the gutter (from the last frame).
    hovered: Option<Oid>,
}

impl BlameState {
    pub fn new(path: &str) -> Self {
        BlameState {
            at: Pos {
                rev: None,
                path: path.to_owned(),
                line: 0,
            },
            data: Load::Idle,
            loaded: None,
            back: Vec::new(),
            forward: Vec::new(),
            scroll_to: None,
            top_line: 0,
            sel: None,
            goto: None,
            goto_focus: false,
            hovered: None,
        }
    }

    pub fn reload(&mut self) {
        self.loaded = None;
    }

    /// Lines of the blame, once loaded.
    #[cfg(test)]
    pub fn line_count(&self) -> Option<usize> {
        self.data.ok().map(|d| d.rows.len())
    }

    /// Back to the tip of the (new) branch, forgetting the navigation.
    pub fn reset_to_base(&mut self, path: &str) {
        *self = BlameState::new(path);
    }

    fn current(&self) -> Pos {
        Pos {
            line: self.top_line,
            ..self.at.clone()
        }
    }

    /// Goes to another revision, remembering where we were.
    pub fn navigate(&mut self, to: Pos) {
        if to.rev == self.at.rev && to.path == self.at.path {
            self.scroll_to = Some(to.line);
            return;
        }
        self.back.push(self.current());
        self.forward.clear();
        self.go(to);
    }

    fn go(&mut self, to: Pos) {
        self.scroll_to = Some(to.line);
        self.at = to;
        self.sel = None;
    }

    pub fn go_back(&mut self) {
        if let Some(p) = self.back.pop() {
            self.forward.push(self.current());
            self.go(p);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(p) = self.forward.pop() {
            self.back.push(self.current());
            self.go(p);
        }
    }

    /// Selected line range, 1-based and ordered.
    pub fn selected_lines(&self) -> Option<(u32, u32)> {
        let (a, b) = self.sel?;
        Some((a.min(b) as u32 + 1, a.max(b) as u32 + 1))
    }

    /// Scrolls to a 1-based line and selects it.
    pub fn go_to_line(&mut self, line: usize) {
        let i = line.saturating_sub(1);
        self.scroll_to = Some(i);
        self.sel = Some((i, i));
    }
}

// ----- Keyboard routing -----

fn active_id() -> egui::Id {
    egui::Id::unique("ferrum-blame-active")
}

fn goto_request_id() -> egui::Id {
    egui::Id::unique("ferrum-blame-goto")
}

/// The blame view (by view id) that had the pointer or focus last frame.
fn active_view(ctx: &egui::Context) -> Option<u64> {
    let (view, frame) = ctx.data(|d| d.get_temp::<(u64, u64)>(active_id()))?;
    (ctx.cumulative_frame_nr() <= frame + 1).then_some(view)
}

/// Opens Go to line in the blame under the pointer, if any. Returns whether
/// Ctrl+G was taken (otherwise it is "go to commit" in the history).
pub fn claim_goto(ctx: &egui::Context) -> bool {
    let Some(view) = active_view(ctx) else {
        return false;
    };
    if !crate::keymap::pressed(ctx, crate::keymap::Action::GoTo) {
        return false;
    }
    ctx.data_mut(|d| d.insert_temp(goto_request_id(), view));
    true
}

fn take_goto_request(ctx: &egui::Context, view: u64) -> bool {
    let asked = ctx.data(|d| d.get_temp::<u64>(goto_request_id())) == Some(view);
    if asked {
        ctx.data_mut(|d| d.remove::<u64>(goto_request_id()));
    }
    asked
}

// ----- Loading -----

fn load_blame(repo: &Repo, path: &str, rev: &str) -> Result<BlameData, String> {
    let lines = repo.blame_at(path, rev).map_err(|e| e.to_string())?;
    let mut commits = HashMap::new();
    for l in &lines {
        commits.entry(l.commit).or_insert_with(|| {
            let summary = repo
                .find_commit_info(l.commit)
                .map(|c| c.summary)
                .unwrap_or_default();
            CommitMeta {
                summary,
                date: format_date(l.time, l.offset_minutes),
                path: l.path.clone(),
                previous: l.previous.clone(),
            }
        });
    }
    let odd = odd_blocks(lines.iter().map(|l| l.block_start));
    let times: Vec<i64> = lines.iter().map(|l| l.time).collect();
    let heats = heat_ranks(&times);
    let age_range = lines
        .iter()
        .min_by_key(|l| l.time)
        .zip(lines.iter().max_by_key(|l| l.time))
        .map(|(a, b)| {
            (
                format_day(a.time, a.offset_minutes),
                format_day(b.time, b.offset_minutes),
            )
        });
    let rows: Vec<BlameRow> = lines
        .into_iter()
        .zip(odd)
        .zip(heats)
        .map(|((l, odd_block), heat)| BlameRow {
            commit: l.commit,
            block_start: l.block_start,
            odd_block,
            gutter: l.block_start.then(|| {
                let date = format_day(l.time, l.offset_minutes);
                (l.author, date)
            }),
            text: crate::util::expand_tabs(&l.text),
            orig_line: l.orig_line,
            heat,
        })
        .collect();
    let longest = rows
        .iter()
        .map(|r| r.text.chars().count())
        .max()
        .unwrap_or(0);
    Ok(BlameData {
        rows,
        commits,
        longest,
        age_range,
    })
}

/// For each time, its rank among the distinct times, from 0 (oldest) to 1
/// (newest). Ranks spread the colors evenly even when a few commits are
/// much older than the rest.
fn heat_ranks(times: &[i64]) -> Vec<f32> {
    let mut distinct: Vec<i64> = times.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    let last = distinct.len().saturating_sub(1).max(1) as f32;
    times
        .iter()
        .map(|t| {
            if distinct.len() < 2 {
                1.0
            } else {
                distinct.binary_search(t).unwrap_or(0) as f32 / last
            }
        })
        .collect()
}

/// The gutter color of a line by age: the accent, stronger for newer lines.
fn heat_color(accent: egui::Color32, heat: f32) -> egui::Color32 {
    accent.gamma_multiply(0.05 + 0.45 * heat.powf(1.6))
}

/// "Older ▮▮▮▮▮ Newer": what the gutter colors mean.
fn age_legend(ui: &mut egui::Ui, range: &(String, String)) {
    let accent = crate::theme::accent(ui);
    let small = |s: &str| egui::RichText::new(s).small().weak();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(small(&range.1));
        let (rect, _) = ui.allocate_exact_size(vec2(90.0, 10.0), Sense::hover());
        let steps = 9;
        let w = rect.width() / steps as f32;
        for k in 0..steps {
            let x0 = (rect.left() + k as f32 * w).round();
            let x1 = (rect.left() + (k + 1) as f32 * w).round();
            let r = Rect::from_x_y_ranges(x0..=x1, rect.y_range());
            let heat = k as f32 / (steps - 1) as f32;
            ui.painter().rect_filled(r, 0.0, heat_color(accent, heat));
        }
        ui.painter().rect_stroke(
            rect,
            2.0,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Outside,
        );
        ui.label(small(&range.0));
        ui.label(small(&t!("Line age"))).on_hover_text(t!(
            "The gutter is colored by the age of each line: newer lines are stronger."
        ));
    });
}

/// For each line, whether it belongs to an odd-numbered block.
fn odd_blocks(starts: impl Iterator<Item = bool>) -> Vec<bool> {
    let mut odd = true;
    starts
        .map(|start| {
            if start {
                odd = !odd;
            }
            odd
        })
        .collect()
}

// ----- The view -----

/// Something a line's menu or the timeline asked for.
enum Act {
    Navigate(Pos),
    /// Select the commit in the file history.
    InHistory(Oid),
    Lines(u32, u32),
}

pub fn show(ui: &mut egui::Ui, tab: &RepoTab, view: &mut FileView, cmds: &mut Commands) {
    let ctx = ui.ctx().clone();
    let st = &mut view.blame;
    let rev = st.at.rev.map_or_else(
        || view.base.clone().unwrap_or_else(|| "HEAD".into()),
        |r| r.to_string(),
    );
    let key = (rev.clone(), st.at.path.clone());
    if st.loaded.as_ref() != Some(&key) {
        st.loaded = Some(key);
        let path = st.at.path.clone();
        let r = rev.clone();
        st.data
            .start(&tab.repo, move |repo| load_blame(repo, &path, &r));
    }
    st.data.poll(&ctx);

    // Keys go to the blame under the pointer (or with Go to line focused).
    let active = active_view(&ctx) == Some(view.id);
    if take_goto_request(&ctx, view.id)
        || (active && crate::keymap::pressed(&ctx, crate::keymap::Action::GoTo))
    {
        st.goto = Some(String::new());
        st.goto_focus = true;
    }
    if active && st.goto.is_none() {
        if crate::keymap::pressed(&ctx, crate::keymap::Action::BlameBack) {
            st.go_back();
        }
        if crate::keymap::pressed(&ctx, crate::keymap::Action::BlameForward) {
            st.go_forward();
        }
    }

    let mut act = None;
    nav_bar(ui, view, &mut act);

    let entries = view.history.ok().map_or(&[][..], Vec::as_slice);
    let st = &mut view.blame;
    let in_blame: HashSet<Oid> = st
        .data
        .ok()
        .map(|d| d.commits.keys().copied().collect())
        .unwrap_or_default();
    if entries.len() > 1
        && let Some(i) = timeline(ui, entries, st.at.rev, &in_blame)
    {
        act = Some(Act::Navigate(Pos {
            rev: Some(entries[i].commit.id),
            path: entries[i].path.clone(),
            line: st.top_line,
        }));
    }

    let dirty = st.at.rev.is_none()
        && view.base.is_none()
        && [&tab.status.unstaged, &tab.status.staged]
            .iter()
            .any(|list| list.iter().any(|f| f.path == st.at.path));
    if dirty && st.data.ok().is_some() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("⚠ This file has uncommitted changes. Blame shows the last committed version."),
        );
    }
    ui.separator();

    if !loading_or_error(ui, st.data.poll(&ctx)) {
        let body = ui.available_rect_before_wrap();
        if ui.rect_contains_pointer(body) {
            mark_active(&ctx, view.id);
        }
        if let Some(a) = rows(ui, st, &rev, cmds) {
            act = Some(a);
        }
    }

    match act {
        Some(Act::Navigate(p)) => view.blame.navigate(p),
        Some(Act::InHistory(id)) => {
            if !view.select_commit(id) {
                cmds.push(Command::SelectCommit(id));
            }
        }
        Some(Act::Lines(a, b)) => {
            let path = view.blame.at.path.clone();
            view.lines = Some(file_lines::LinesState::new(path, a, b, rev));
            view.mode = Mode::Lines;
        }
        None => {}
    }
}

fn mark_active(ctx: &egui::Context, view: u64) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(active_id(), (view, frame)));
}

/// Back/forward, the revision shown and Go to line.
fn nav_bar(ui: &mut egui::Ui, view: &mut FileView, act: &mut Option<Act>) {
    let ctx = ui.ctx().clone();
    let entries = view.history.ok().map_or(&[][..], Vec::as_slice);
    let st = &mut view.blame;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!st.back.is_empty(), egui::Button::new("⬅").small())
            .on_hover_text(t!("Back (Alt+Left)"))
            .clicked()
        {
            st.go_back();
        }
        if ui
            .add_enabled(!st.forward.is_empty(), egui::Button::new("➡").small())
            .on_hover_text(t!("Forward (Alt+Right)"))
            .clicked()
        {
            st.go_forward();
        }
        match st.at.rev {
            None => {
                let base = view.base.clone().unwrap_or_else(|| "HEAD".into());
                ui.label(t!("Blame at {}", base));
            }
            Some(id) => {
                let summary = entries
                    .iter()
                    .find(|e| e.commit.id == id)
                    .map(|e| e.commit.summary.clone())
                    .unwrap_or_default();
                ui.label(t!("Blame at {}", short(id)));
                ui.add(egui::Label::new(egui::RichText::new(summary).weak()).truncate());
                if ui
                    .small_button(t!("Latest"))
                    .on_hover_text(t!("Blame of the newest version"))
                    .clicked()
                {
                    *act = Some(Act::Navigate(Pos {
                        rev: None,
                        path: view_path(entries, &st.at.path),
                        line: st.top_line,
                    }));
                }
            }
        }
        if st.at.path != view_path(entries, &st.at.path) {
            ui.weak(t!("(named {} here)", st.at.path));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(text) = &mut st.goto {
                let resp = ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(t!("Line number"))
                        .desired_width(90.0),
                );
                if st.goto_focus {
                    resp.request_focus();
                    st.goto_focus = false;
                }
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let escape = ui.input(|i| i.key_pressed(Key::Escape));
                let total = st.data.ok().map_or(0, |d| d.rows.len());
                if enter {
                    if let Ok(n) = text.trim().parse::<usize>()
                        && n >= 1
                        && n <= total
                    {
                        st.go_to_line(n);
                    }
                    st.goto = None;
                } else if escape || (resp.lost_focus() && !enter) {
                    st.goto = None;
                } else if resp.has_focus() {
                    mark_active(&ctx, view.id);
                }
                ui.label(t!("Go to line"));
            } else if ui
                .small_button(t!("Go to line…"))
                .on_hover_text("Ctrl+G")
                .clicked()
            {
                st.goto = Some(String::new());
                st.goto_focus = true;
            }
            if let Some((a, b)) = st.selected_lines()
                && ui
                    .small_button(lines_label(a, b))
                    .on_hover_text(t!("Commits that changed the selected lines"))
                    .clicked()
            {
                *act = Some(Act::Lines(a, b));
            }
            if let Some(r) = st.data.ok().and_then(|d| d.age_range.as_ref()) {
                ui.add_space(crate::theme::SPACE_L);
                age_legend(ui, r);
            }
        });
    });
}

/// The newest name of the file (the view's path follows renames).
fn view_path(entries: &[FileCommit], fallback: &str) -> String {
    entries
        .first()
        .map_or_else(|| fallback.to_owned(), |e| e.path.clone())
}

fn lines_label(a: u32, b: u32) -> String {
    if a == b {
        t!("History of line {}", a)
    } else {
        t!("History of lines {}–{}", a, b)
    }
}

/// A strip with the file's commits, oldest on the left. Returns the index
/// (in `entries`) of a clicked commit.
fn timeline(
    ui: &mut egui::Ui,
    entries: &[FileCommit],
    current: Option<Oid>,
    in_blame: &HashSet<Oid>,
) -> Option<usize> {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), TIMELINE_H), Sense::click());
    let visuals = ui.visuals().clone();
    let painter = ui.painter_at(rect);
    let label_w = 76.0;
    let line = Rect::from_x_y_ranges(
        rect.left() + label_w..=rect.right() - label_w,
        rect.y_range(),
    );
    let y = rect.center().y;
    painter.hline(
        line.x_range(),
        y,
        (1.0, visuals.widgets.noninteractive.bg_stroke.color),
    );
    let n = entries.len();
    let x_of = |k: usize| {
        // k: position from the oldest.
        if n <= 1 {
            line.center().x
        } else {
            line.left() + line.width() * k as f32 / (n - 1) as f32
        }
    };
    let oldest = &entries[n - 1].commit;
    let newest = &entries[0].commit;
    let weak = visuals.weak_text_color();
    painter.text(
        pos2(rect.left() + 2.0, y),
        Align2::LEFT_CENTER,
        format_day(oldest.time, oldest.offset_minutes),
        FontId::proportional(11.0),
        weak,
    );
    painter.text(
        pos2(rect.right() - 2.0, y),
        Align2::RIGHT_CENTER,
        format_day(newest.time, newest.offset_minutes),
        FontId::proportional(11.0),
        weak,
    );
    let current = current.map_or(0, |id| {
        entries
            .iter()
            .position(|e| e.commit.id == id)
            .unwrap_or(usize::MAX)
    });
    let hovered = resp.hover_pos().and_then(|p| {
        let k = ((p.x - line.left()) / line.width() * (n - 1) as f32).round();
        let k = k.clamp(0.0, (n - 1) as f32) as usize;
        ((x_of(k) - p.x).abs() < 10.0).then_some(n - 1 - k)
    });
    let accent = visuals.hyperlink_color;
    for (i, e) in entries.iter().enumerate() {
        let x = x_of(n - 1 - i);
        let (r, color) = if i == current {
            (6.0, visuals.selection.bg_fill)
        } else if in_blame.contains(&e.commit.id) {
            (4.0, accent)
        } else {
            (3.0, weak)
        };
        let r = if hovered == Some(i) { r + 2.0 } else { r };
        painter.circle_filled(pos2(x, y), r, color);
        if i == current {
            painter.circle_stroke(pos2(x, y), r + 1.5, (1.5, visuals.text_color()));
        }
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(i) = hovered {
        let c = &entries[i].commit;
        resp.clone().on_hover_ui_at_pointer(|ui| {
            ui.strong(&c.summary);
            ui.weak(format!(
                "{} · {} · {}",
                c.short_id(),
                c.author,
                format_date(c.time, c.offset_minutes)
            ));
            ui.weak(t!("Click to blame at this revision"));
        });
    } else {
        resp.clone().on_hover_text(t!(
            "Commits of this file: click one to blame at that revision"
        ));
    }
    if resp.clicked() { hovered } else { None }
}

fn rows(ui: &mut egui::Ui, st: &mut BlameState, rev: &str, cmds: &mut Commands) -> Option<Act> {
    let scroll_to = st.scroll_to.take();
    let data = st.data.ok()?;
    if data.rows.is_empty() {
        ui.weak(t!("The file is empty."));
        return None;
    }
    let font = crate::util::code_font(12.0);
    let row_h = crate::util::code_row_height(BLAME_ROW);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'm'));
    let content_w = GUTTER_W + NUMBER_W + 16.0 + data.longest as f32 * char_w;
    let visuals = ui.visuals().clone();
    let block_bg = visuals.faint_bg_color;
    let hover_bg = visuals.selection.bg_fill.gamma_multiply(0.4);
    let sel_bg = visuals.selection.bg_fill.gamma_multiply(0.55);
    let gutter_bg = visuals.extreme_bg_color;
    let accent = visuals.hyperlink_color;
    let heat_base = crate::theme::accent(ui);
    let weak = visuals.weak_text_color();
    let separator = visuals.widgets.noninteractive.bg_stroke.color;
    let mut hovered = None;
    let mut act = None;
    let mut clicked_line = None;
    let selected = st.selected_lines();

    // Rows touch; show_rows must know there is no spacing between them.
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut area = egui::ScrollArea::both()
        .id_salt(("blame-rows", rev, &st.at.path))
        .auto_shrink(false);
    if let Some(line) = scroll_to {
        let line = line.min(data.rows.len() - 1);
        // Leave a few lines of context above.
        area = area.vertical_scroll_offset(line.saturating_sub(3) as f32 * row_h);
    }
    let output = area.show_rows(ui, row_h, data.rows.len(), |ui, range| {
        let width = ui.available_width().max(content_w);
        for i in range {
            let row = &data.rows[i];
            let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());
            // The gutter stays put while the code scrolls sideways.
            let left = ui.clip_rect().left();
            let gutter = Rect::from_x_y_ranges(left..=left + GUTTER_W, rect.y_range());
            let resp = ui
                .interact(
                    gutter,
                    egui::Id::unique(("blame-gutter", i)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                hovered = Some(row.commit);
            }
            let code = Rect::from_x_y_ranges(gutter.right()..=rect.right(), rect.y_range());
            let code_resp = ui.interact(code, egui::Id::unique(("blame-code", i)), Sense::click());

            let painter = ui.painter();
            let is_sel =
                selected.is_some_and(|(a, b)| (a as usize..=b as usize).contains(&(i + 1)));
            if is_sel {
                painter.rect_filled(rect, 0.0, sel_bg);
            } else if row.odd_block {
                painter.rect_filled(rect, 0.0, block_bg);
            }
            painter.rect_filled(gutter, 0.0, gutter_bg.gamma_multiply(0.6));
            painter.rect_filled(gutter, 0.0, heat_color(heat_base, row.heat));
            // A bar on the left edge, full strength for the newest lines.
            let bar = Rect::from_x_y_ranges(gutter.left()..=gutter.left() + 3.0, rect.y_range());
            painter.rect_filled(bar, 0.0, heat_base.gamma_multiply(0.15 + 0.85 * row.heat));
            // Lines from the commit under the pointer light up in the gutter.
            if st.hovered == Some(row.commit) {
                painter.rect_filled(gutter, 0.0, hover_bg);
            }
            if row.block_start && i > 0 {
                painter.hline(rect.x_range(), rect.top(), (1.0, separator));
            }
            let y = rect.center().y;
            if let Some((author, date)) = &row.gutter {
                let x = gutter.left() + 6.0;
                painter.text(
                    pos2(x, y),
                    Align2::LEFT_CENTER,
                    short(row.commit),
                    FontId::monospace(11.5),
                    accent,
                );
                painter
                    .with_clip_rect(Rect::from_x_y_ranges(
                        x + HASH_W..=x + HASH_W + AUTHOR_W - 8.0,
                        rect.y_range(),
                    ))
                    .text(
                        pos2(x + HASH_W, y),
                        Align2::LEFT_CENTER,
                        author,
                        FontId::proportional(12.0),
                        visuals.text_color(),
                    );
                painter
                    .with_clip_rect(Rect::from_x_y_ranges(
                        x + HASH_W + AUTHOR_W..=gutter.right() - 4.0,
                        rect.y_range(),
                    ))
                    .text(
                        pos2(x + HASH_W + AUTHOR_W, y),
                        Align2::LEFT_CENTER,
                        date,
                        FontId::proportional(12.0),
                        weak,
                    );
            }
            painter.text(
                pos2(gutter.right() + NUMBER_W - 8.0, y),
                Align2::RIGHT_CENTER,
                (i + 1).to_string(),
                crate::util::code_font(11.0),
                weak,
            );
            let code_left = gutter.right() + NUMBER_W;
            painter
                .with_clip_rect(Rect::from_x_y_ranges(
                    code_left..=ui.clip_rect().right(),
                    rect.y_range(),
                ))
                .text(
                    pos2(rect.left() + GUTTER_W + NUMBER_W + 4.0, y),
                    Align2::LEFT_CENTER,
                    &row.text,
                    font.clone(),
                    visuals.text_color(),
                );

            if resp.clicked() {
                cmds.push(Command::SelectCommit(row.commit));
            }
            if code_resp.clicked() {
                clicked_line = Some((i, ui.input(|inp| inp.modifiers.shift)));
            }
            let meta = data.commits.get(&row.commit);
            for r in [&resp, &code_resp] {
                r.context_menu(|ui| {
                    if let Some(a) = line_menu(ui, data, st, i, cmds) {
                        act = Some(a);
                    }
                });
            }
            if let Some(m) = meta {
                resp.on_hover_ui(|ui| {
                    ui.strong(&m.summary);
                    ui.weak(format!("{} · {}", row.commit, m.date));
                    ui.weak(t!("Click to show in the history"));
                });
            }
        }
    });
    st.top_line = (output.state.clamped_offset().y / row_h).floor().max(0.0) as usize;
    st.hovered = hovered;
    if let Some((i, shift)) = clicked_line {
        st.sel = match (shift, st.sel) {
            (true, Some((anchor, _))) => Some((anchor, i)),
            _ if st.sel == Some((i, i)) => None,
            _ => Some((i, i)),
        };
    }
    act
}

/// Context menu of a blame line.
fn line_menu(
    ui: &mut egui::Ui,
    data: &BlameData,
    st: &BlameState,
    i: usize,
    cmds: &mut Commands,
) -> Option<Act> {
    let row = &data.rows[i];
    let meta = data.commits.get(&row.commit);
    let mut act = None;
    // The selection if the line is in it, else just this line.
    let (a, b) = st
        .selected_lines()
        .filter(|(a, b)| (*a as usize..=*b as usize).contains(&(i + 1)))
        .unwrap_or((i as u32 + 1, i as u32 + 1));
    if ui.button(lines_label(a, b)).clicked() {
        act = Some(Act::Lines(a, b));
        ui.close();
    }
    ui.separator();
    let previous = meta.and_then(|m| m.previous.clone());
    let before = ui
        .add_enabled(
            previous.is_some(),
            egui::Button::new(t!("Blame before this change")),
        )
        .on_hover_text(t!(
            "Blame at the parent of the commit that last changed this line"
        ))
        .on_disabled_hover_text(t!("The file was added in this commit."));
    if before.clicked()
        && let Some((id, path)) = previous
    {
        act = Some(Act::Navigate(Pos {
            rev: Some(id),
            path,
            line: (row.orig_line as usize).saturating_sub(1),
        }));
        ui.close();
    }
    if ui.button(t!("Blame at this commit")).clicked() {
        act = Some(Act::Navigate(Pos {
            rev: Some(row.commit),
            path: meta.map_or_else(|| st.at.path.clone(), |m| m.path.clone()),
            line: (row.orig_line as usize).saturating_sub(1),
        }));
        ui.close();
    }
    if ui
        .button(t!("Select the commit in the file history"))
        .clicked()
    {
        act = Some(Act::InHistory(row.commit));
        ui.close();
    }
    if ui.button(t!("Show in history")).clicked() {
        cmds.push(Command::SelectCommit(row.commit));
        ui.close();
    }
    ui.separator();
    if ui.button(t!("Copy hash")).clicked() {
        cmds.push(Command::Copy(row.commit.to_string()));
        ui.close();
    }
    if ui.button(t!("Copy line")).clicked() {
        cmds.push(Command::Copy(row.text.clone()));
        ui.close();
    }
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_line_ages() {
        assert_eq!(heat_ranks(&[5, 1, 9, 5]), vec![0.5, 0.0, 1.0, 0.5]);
        assert_eq!(heat_ranks(&[3, 3]), vec![1.0, 1.0]);
    }

    #[test]
    fn alternates_blocks() {
        let starts = [true, false, true, true, false, false];
        assert_eq!(
            odd_blocks(starts.into_iter()),
            [false, false, true, false, false, false]
        );
    }

    #[test]
    fn navigates_back_and_forward() {
        let mut st = BlameState::new("a");
        let id = |n: u8| Some(Oid::from_bytes(&[n; 20]).unwrap());
        let pos = |n: u8, line| Pos {
            rev: id(n),
            path: "a".into(),
            line,
        };
        // What the view does on the next frame: scroll where it was asked.
        let scrolled = |st: &mut BlameState| {
            if let Some(l) = st.scroll_to.take() {
                st.top_line = l;
            }
        };
        st.top_line = 7;
        st.navigate(pos(1, 3));
        scrolled(&mut st);
        st.navigate(pos(2, 5));
        scrolled(&mut st);
        assert_eq!(st.at.rev, id(2));
        st.go_back();
        assert_eq!((st.at.rev, st.scroll_to), (id(1), Some(3)));
        scrolled(&mut st);
        st.go_back();
        assert_eq!((st.at.rev, st.scroll_to), (None, Some(7)));
        scrolled(&mut st);
        st.go_forward();
        scrolled(&mut st);
        st.go_forward();
        assert_eq!((st.at.rev, st.scroll_to), (id(2), Some(5)));
        // A new place drops the forward history.
        st.go_back();
        st.navigate(pos(3, 0));
        assert!(st.forward.is_empty());
    }
}
