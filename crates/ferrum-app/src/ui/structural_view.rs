//! The structural diff in the diff view ("Structural" in the toolbar): both
//! versions of the file are compared as syntax trees (`ferrum_core::structural`)
//! on a background thread, then shown side by side or unified. Formatting
//! changes are not marked; moved code is marked in its own color.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, FontId, Id, Rect, Sense, pos2, vec2};
use ferrum_core::structural::{self, Mark, Row, StructuralDiff, Unavailable};
use ferrum_core::syntax::Lang;
use ferrum_core::{ChangeKind, DiffSource, FileDiff, Oid, Repo};

use crate::icons;
use crate::ui::diff_prefs::DiffPrefs;
use crate::ui::diff_text::{self, LineStyle, SideHighlighter};
use crate::util::{added_color, removed_color};

/// In `structural_repos`, turns the structural diff on everywhere (`FERRUM_START`).
const EVERYWHERE: &str = "*";
/// Results kept in memory.
const MAX_CACHED: usize = 24;

/// Where to read the two versions of a file from.
#[derive(Clone)]
pub struct Source {
    pub repo_dir: PathBuf,
    /// The repository, as remembered in the preferences.
    pub repo_key: String,
    source: DiffSource,
    path: String,
    old: Option<Oid>,
    new: Option<Oid>,
    kind: ChangeKind,
    key: u64,
}

impl Source {
    /// The file's versions, when it is text in a language with a grammar.
    pub fn new(repo: &Repo, source: DiffSource, file: &FileDiff) -> Option<Source> {
        Lang::from_path(&file.path)?;
        if file.binary {
            return None;
        }
        let mut h = std::hash::DefaultHasher::new();
        (repo.path(), &file.path, file.old_id, file.new_id).hash(&mut h);
        format!("{source:?}").hash(&mut h);
        crate::ui::diff_view::file_key(file).hash(&mut h);
        Some(Source {
            repo_dir: repo.path().to_path_buf(),
            repo_key: repo.path().to_string_lossy().into_owned(),
            source,
            path: file.path.clone(),
            old: file.old_id,
            new: file.new_id,
            kind: file.kind,
            key: h.finish(),
        })
    }

    fn load(&self) -> Result<Loaded, String> {
        let repo = Repo::open(&self.repo_dir).map_err(|e| e.to_string())?;
        let blob = |id: Option<Oid>| -> Result<Vec<u8>, String> {
            match id {
                Some(id) if !id.is_zero() => repo.blob(id).map_err(|e| e.to_string()),
                _ => Ok(Vec::new()),
            }
        };
        let added = matches!(self.kind, ChangeKind::Added | ChangeKind::Untracked);
        let old = if added { Vec::new() } else { blob(self.old)? };
        let new = if self.kind == ChangeKind::Deleted {
            Vec::new()
        } else if self.source == DiffSource::Unstaged {
            repo.read_workdir_file(&self.path)
                .map_err(|e| e.to_string())?
        } else {
            blob(self.new)?
        };
        let diff = structural::diff_path(&self.path, &old, &new);
        let text = |b: Vec<u8>| String::from_utf8(b).unwrap_or_default();
        Ok(Loaded {
            diff,
            old: text(old),
            new: text(new),
        })
    }
}

struct Loaded {
    diff: Result<StructuralDiff, Unavailable>,
    old: String,
    new: String,
}

type Slot = Arc<Mutex<Option<Result<Arc<Loaded>, String>>>>;

fn cache_id() -> Id {
    Id::unique("structural-diff-cache")
}

/// The comparison for `src`, started in the background the first time.
fn loaded(ctx: &egui::Context, src: &Source) -> Option<Result<Arc<Loaded>, String>> {
    let slot = ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<HashMap<u64, Slot>>(cache_id());
        if let Some(s) = cache.get(&src.key) {
            return s.clone();
        }
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        let slot: Slot = Arc::default();
        cache.insert(src.key, slot.clone());
        let (src, out, ctx) = (src.clone(), slot.clone(), ctx.clone());
        // Deep syntax trees need a larger stack than the default.
        let spawned = std::thread::Builder::new()
            .name("structural-diff".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let r = src.load().map(Arc::new);
                *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                ctx.request_repaint();
            });
        if let Err(e) = spawned {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(e.to_string()));
        }
        slot
    });
    slot.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn is_on(prefs: &DiffPrefs, src: &Source) -> bool {
    prefs.structural_diff
        && (prefs.structural_repos.contains(&src.repo_key)
            || prefs.structural_repos.contains(EVERYWHERE))
}

fn set(prefs: &mut DiffPrefs, repo: &str, on: bool) {
    prefs.structural_repos.remove(EVERYWHERE);
    if on {
        prefs.structural_repos.insert(repo.to_owned());
    } else {
        prefs.structural_repos.remove(repo);
    }
}

fn on_for(prefs: &DiffPrefs, repo: &str) -> bool {
    prefs.structural_repos.contains(repo) || prefs.structural_repos.contains(EVERYWHERE)
}

fn tooltip() -> String {
    format!(
        "{}\n{}",
        t!("Structural diff"),
        t!("Compare the code's syntax: formatting changes are ignored and moved code is marked. Remembered per repository.")
    )
}

/// The toolbar toggle.
pub fn toggle(ui: &mut egui::Ui, prefs: &mut DiffPrefs, repo: &str) {
    let on = on_for(prefs, repo);
    let text = egui::RichText::new(icons::TREE_STRUCTURE).size(icons::ICON_SIZE);
    let text = if on {
        text.color(ui.visuals().selection.stroke.color)
    } else {
        text
    };
    let resp = ui
        .add(egui::Button::selectable(on, text).min_size(vec2(26.0, 24.0)))
        .on_hover_text(tooltip());
    if resp.clicked() {
        set(prefs, repo, !on);
    }
}

/// The same toggle in the Options popover.
pub fn checkbox(ui: &mut egui::Ui, prefs: &mut DiffPrefs, repo: &str) {
    let mut on = on_for(prefs, repo);
    if ui
        .checkbox(&mut on, t!("Structural (syntax-aware)"))
        .on_hover_text(tooltip())
        .changed()
    {
        set(prefs, repo, on);
    }
}

/// `FERRUM_START` `diff:structural`.
pub fn dev_all(prefs: &mut DiffPrefs) {
    prefs.structural_diff = true;
    prefs.structural_repos.insert(EVERYWHERE.to_owned());
}

fn reason(u: Unavailable) -> String {
    match u {
        Unavailable::Language => t!("no grammar for this file"),
        Unavailable::TooLarge => t!("the file is too large"),
        Unavailable::Binary => t!("the file is not UTF-8 text"),
        Unavailable::TooComplex => t!("the file is too complex"),
    }
}

// ----- Drawing -----

#[derive(Clone, Default)]
struct ViewState {
    key: u64,
    /// Gaps (by their first row) shown in full.
    expanded: HashSet<usize>,
    offset: f32,
    jump: Option<f32>,
    /// Syntax colors of both sides, for a dark or light theme.
    highlight: Option<(bool, Arc<Mutex<[SideHighlighter; 2]>>)>,
}

/// One line of the view.
#[derive(Clone, Copy)]
enum Line {
    /// A row of the diff; in the unified view, `Some(old?)` shows one side only.
    Row(usize, Option<bool>),
    /// Unchanged rows hidden: (first row, count).
    Gap(usize, usize),
}

struct Colors {
    text: Color32,
    weak: Color32,
    add_bg: Color32,
    del_bg: Color32,
    add_word: Color32,
    del_word: Color32,
    moved_bg: Color32,
    moved_fg: Color32,
    add_fg: Color32,
    del_fg: Color32,
}

impl Colors {
    fn new(ui: &egui::Ui) -> Self {
        let v = ui.visuals();
        let dark = v.dark_mode;
        let rgba = Color32::from_rgba_unmultiplied;
        let moved = if dark {
            Color32::from_rgb(0x5a, 0xa9, 0xe6)
        } else {
            Color32::from_rgb(0x1f, 0x6f, 0xb5)
        };
        Self {
            text: v.text_color(),
            weak: v.weak_text_color(),
            add_bg: rgba(0x2e, 0xa0, 0x43, if dark { 28 } else { 32 }),
            del_bg: rgba(0xf8, 0x51, 0x49, if dark { 28 } else { 30 }),
            add_word: rgba(0x2e, 0xa0, 0x43, if dark { 120 } else { 130 }),
            del_word: rgba(0xf8, 0x51, 0x49, if dark { 120 } else { 120 }),
            moved_bg: moved.gamma_multiply(if dark { 0.45 } else { 0.35 }),
            moved_fg: moved,
            add_fg: added_color(ui),
            del_fg: removed_color(ui),
        }
    }
}

/// Draws the structural diff of `src`. Returns false when it is not
/// available for this file (a note says why; the caller shows the line diff).
pub fn body(
    ui: &mut egui::Ui,
    id: &str,
    src: &Source,
    file: &FileDiff,
    prefs: &DiffPrefs,
    nav: i32,
) -> bool {
    let _ = file;
    let ctx = ui.ctx().clone();
    let loaded = match loaded(&ctx, src) {
        None => {
            ui.add_space(crate::theme::SPACE_L);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Comparing the syntax…"));
            });
            return true;
        }
        Some(Err(e)) => {
            unavailable_note(ui, &e);
            return false;
        }
        Some(Ok(l)) => l,
    };
    let d = match &loaded.diff {
        Ok(d) => d,
        Err(u) => {
            unavailable_note(ui, &reason(*u));
            return false;
        }
    };
    let state_id = Id::unique(("structural-view", id));
    let mut st = ctx
        .data(|data| data.get_temp::<ViewState>(state_id))
        .unwrap_or_default();
    if st.key != src.key {
        st = ViewState {
            key: src.key,
            ..Default::default()
        };
    }
    summary(ui, d);
    if !d.has_changes() {
        crate::ui::empty_state::show(
            ui,
            icons::CHECK_CIRCLE,
            &t!("No syntax changes"),
            &if loaded.old == loaded.new {
                t!("Both versions have the same content.")
            } else {
                t!("Only formatting changed: whitespace, indentation or line breaks.")
            },
            |_| {},
        );
        ctx.data_mut(|data| data.insert_temp(state_id, st));
        return true;
    }
    let lines = visible_lines(d, prefs, &st.expanded);
    draw(ui, id, d, &loaded, prefs, &lines, &mut st, nav);
    ctx.data_mut(|data| data.insert_temp(state_id, st));
    true
}

fn unavailable_note(ui: &mut egui::Ui, why: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(icons::INFO).color(ui.visuals().weak_text_color()),
        );
        ui.weak(t!(
            "Structural diff unavailable ({}): showing the line diff.",
            why
        ));
    });
}

/// Language, counts and the color legend.
fn summary(ui: &mut egui::Ui, d: &StructuralDiff) {
    let c = Colors::new(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(icons::tinted(
            icons::TREE_STRUCTURE,
            crate::theme::accent(ui),
            t!("Structural · {}", d.lang.name()),
        ));
        let chip = |ui: &mut egui::Ui, color: Color32, text: String| {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, color);
            ui.weak(text);
        };
        chip(ui, c.del_word, t!("{} removed", d.old.changed));
        chip(ui, c.add_word, t!("{} added", d.new.changed));
        if d.old.moved + d.new.moved > 0 {
            chip(ui, c.moved_bg, t!("{} moved", d.new.moved));
        }
        if d.parse_errors {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                icons::text(icons::WARNING, t!("syntax errors: the result may be rough")),
            );
        }
    });
}

fn visible_lines(d: &StructuralDiff, prefs: &DiffPrefs, expanded: &HashSet<usize>) -> Vec<Line> {
    let n = d.rows.len();
    let context = if prefs.full_file {
        n
    } else {
        prefs.context_lines as usize
    };
    // Rows within `context` of a change.
    let mut keep = vec![false; n];
    for (i, r) in d.rows.iter().enumerate() {
        if r.changed {
            for k in keep
                .iter_mut()
                .take((i + context + 1).min(n))
                .skip(i.saturating_sub(context))
            {
                *k = true;
            }
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if keep[i] {
            push_row(&mut out, &d.rows[i], i, prefs.side_by_side);
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !keep[i] {
            i += 1;
        }
        if expanded.contains(&start) {
            for (k, row) in d.rows.iter().enumerate().take(i).skip(start) {
                push_row(&mut out, row, k, prefs.side_by_side);
            }
        } else {
            out.push(Line::Gap(start, i - start));
        }
    }
    out
}

fn push_row(out: &mut Vec<Line>, r: &Row, i: usize, split: bool) {
    if split || !r.changed {
        out.push(Line::Row(i, None));
        return;
    }
    // Unified: a changed row shows its old line, then its new line.
    if r.old.is_some() {
        out.push(Line::Row(i, Some(true)));
    }
    if r.new.is_some() {
        out.push(Line::Row(i, Some(false)));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut egui::Ui,
    id: &str,
    d: &StructuralDiff,
    l: &Loaded,
    prefs: &DiffPrefs,
    lines: &[Line],
    st: &mut ViewState,
    nav: i32,
) {
    let c = Colors::new(ui);
    let size = prefs.font_size();
    let font = FontId::monospace(size);
    let num_font = FontId::monospace((size - 1.0).max(7.0));
    let row_h = (size * 1.5).round();
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'm'));
    let max_no = d.old.lines.len().max(d.new.lines.len()).max(1);
    let num_w = ((max_no.ilog10() as f32 + 1.0).max(3.0) * char_w + 12.0).round();
    let split = prefs.side_by_side;

    // Change navigation: the next or previous block of changed lines.
    if nav != 0 {
        let first = (st.offset / row_h).floor() as usize;
        let starts: Vec<usize> = (0..lines.len())
            .filter(|&i| is_change(d, lines[i]) && (i == 0 || !is_change(d, lines[i - 1])))
            .collect();
        let target = if nav > 0 {
            starts.iter().find(|&&i| i > first + 1)
        } else {
            starts.iter().rev().find(|&&i| i + 1 < first)
        };
        if let Some(&t) = target {
            st.jump = Some((t as f32 * row_h - row_h * 3.0).max(0.0));
        }
    }

    // Syntax colors, from the start of each side.
    let dark = ui.visuals().dark_mode;
    if st.highlight.as_ref().is_some_and(|(d, _)| *d != dark) {
        st.highlight = None;
    }
    let syntax = prefs
        .syntax
        .then(|| diff_text::syntax_for(&path_hint(d)))
        .flatten();
    let hl = syntax.map(|s| {
        let (_, hl) = st.highlight.get_or_insert_with(|| {
            let make = || SideHighlighter::new(s, dark);
            (dark, Arc::new(Mutex::new([make(), make()])))
        });
        hl.clone()
    });

    let tab = prefs.tab_width();
    let longest = |text: &str, side: &structural::Side| {
        side.lines
            .iter()
            .map(|r| diff_text::display_width(&text[r.clone()], tab))
            .max()
            .unwrap_or(0)
    };
    let avail = ui.available_width();
    let cols = longest(&l.old, &d.old).max(longest(&l.new, &d.new));
    let content_w = if split {
        avail
    } else {
        (num_w * 2.0 + char_w * 2.5 + cols as f32 * char_w + 24.0).max(avail)
    };

    let mut area = egui::ScrollArea::both()
        .id_salt(("structural", id))
        .auto_shrink([false, false]);
    if let Some(y) = st.jump.take() {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show_viewport(ui, |ui, viewport| {
        ui.set_min_size(vec2(content_w, lines.len() as f32 * row_h));
        let origin = ui.min_rect().min;
        let first = (viewport.top() / row_h).floor().max(0.0) as usize;
        let last = ((viewport.bottom() / row_h).ceil() as usize + 1).min(lines.len());
        if let Some(hl) = &hl {
            let mut hl = hl.lock().unwrap_or_else(|e| e.into_inner());
            let upto = |old: bool| {
                lines[first..last]
                    .iter()
                    .filter_map(|line| match *line {
                        Line::Row(i, _) => if old { d.rows[i].old } else { d.rows[i].new },
                        Line::Gap(..) => None,
                    })
                    .max()
                    .map_or(0, |n| n as usize + 1)
            };
            let (uo, un) = (upto(true), upto(false));
            let [ho, hn] = &mut *hl;
            let done_o = ho.advance(|i| &l.old[d.old.lines[i].clone()], uo.min(d.old.lines.len()));
            let done_n = hn.advance(|i| &l.new[d.new.lines[i].clone()], un.min(d.new.lines.len()));
            if !(done_o && done_n) {
                ui.ctx().request_repaint();
            }
        }
        let guard = hl.as_ref().map(|h| h.lock().unwrap_or_else(|e| e.into_inner()));
        let spans = |old: bool, n: u32| {
            guard
                .as_ref()
                .and_then(|g| g[usize::from(!old)].done.get(n as usize).cloned())
        };
        let full = Rect::from_min_size(origin, vec2(content_w, row_h));
        for (k, line) in lines.iter().enumerate().take(last).skip(first) {
            let rect = full.translate(vec2(0.0, k as f32 * row_h));
            match *line {
                Line::Gap(start, count) => {
                    let resp = ui.interact(rect, Id::unique((id, "structural-gap", start)), Sense::click());
                    let bg = if resp.hovered() {
                        ui.visuals().widgets.hovered.weak_bg_fill
                    } else {
                        ui.visuals().faint_bg_color
                    };
                    ui.painter().rect_filled(rect, 0.0, bg);
                    ui.painter().text(
                        pos2(ui.clip_rect().left().max(rect.left()) + 10.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        format!("{}  {}", icons::DOTS_THREE, t!("{} unchanged lines", count)),
                        num_font.clone(),
                        c.weak,
                    );
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        st.expanded.insert(start);
                    }
                }
                Line::Row(i, only) => {
                    let row = d.rows[i];
                    // Unified: a changed row is shown as removed and added lines.
                    let cell = |old: bool, rect: Rect, numbers: (bool, bool)| Cell {
                        old,
                        rect,
                        numbers,
                        changed: !split && row.changed,
                    };
                    let p = Painter {
                        ui,
                        c: &c,
                        font: &font,
                        num_font: &num_font,
                        num_w,
                        char_w,
                        row_h,
                        tab,
                        show_ws: prefs.show_whitespace,
                        markers: prefs.markers,
                    };
                    if split {
                        let half = (rect.width() / 2.0).floor();
                        let left = Rect::from_min_size(rect.min, vec2(half - 1.0, row_h));
                        let right = Rect::from_min_size(rect.min + vec2(half + 1.0, 0.0), vec2(half - 1.0, row_h));
                        p.side(d, l, row.old, cell(true, left, (true, false)), &spans);
                        p.side(d, l, row.new, cell(false, right, (false, true)), &spans);
                        ui.painter().vline(
                            rect.left() + half,
                            rect.y_range(),
                            ui.visuals().widgets.noninteractive.bg_stroke,
                        );
                    } else {
                        match only {
                            Some(true) => p.side(d, l, row.old, cell(true, rect, (true, true)), &spans),
                            Some(false) => p.side(d, l, row.new, cell(false, rect, (true, true)), &spans),
                            None => p.unchanged(d, l, row, rect, &spans),
                        }
                    }
                }
            }
        }
    });
    st.offset = out.state.clamped_offset().y;
}

fn is_change(d: &StructuralDiff, line: Line) -> bool {
    matches!(line, Line::Row(i, _) if d.rows[i].changed)
}

fn path_hint(d: &StructuralDiff) -> String {
    let ext = match d.lang {
        Lang::Rust => "rs",
        Lang::JavaScript => "js",
        Lang::TypeScript => "ts",
        Lang::Tsx => "tsx",
        Lang::Python => "py",
        Lang::Go => "go",
        Lang::Java => "java",
        Lang::C => "c",
        Lang::Cpp => "cpp",
        Lang::CSharp => "cs",
        Lang::Json => "json",
        Lang::Yaml => "yaml",
        Lang::Toml => "toml",
        Lang::Html => "html",
        Lang::Css => "css",
        Lang::Markdown => "md",
    };
    format!("a.{ext}")
}

struct Cell {
    old: bool,
    rect: Rect,
    /// Line number columns: (old, new).
    numbers: (bool, bool),
    /// Style the line as changed even without marks.
    changed: bool,
}

struct Painter<'a> {
    ui: &'a egui::Ui,
    c: &'a Colors,
    font: &'a FontId,
    num_font: &'a FontId,
    num_w: f32,
    char_w: f32,
    row_h: f32,
    tab: usize,
    show_ws: bool,
    markers: bool,
}

impl Painter<'_> {
    /// One side's line (or an empty cell).
    fn side(
        &self,
        d: &StructuralDiff,
        l: &Loaded,
        line: Option<u32>,
        cell: Cell,
        spans: &dyn Fn(bool, u32) -> Option<diff_text::Spans>,
    ) {
        let painter = self.ui.painter();
        let Some(n) = line else {
            painter.rect_filled(cell.rect, 0.0, self.ui.visuals().faint_bg_color);
            return;
        };
        let (side, text) = if cell.old {
            (&d.old, &l.old)
        } else {
            (&d.new, &l.new)
        };
        let changed = cell.changed || side.line_changed(n);
        let marks = &side.marks[n as usize];
        let only_moved = !marks.is_empty() && marks.iter().all(|(_, m)| *m == Mark::Moved);
        let bg = match (changed, only_moved, cell.old) {
            (false, _, _) => Color32::TRANSPARENT,
            (true, true, _) => self.c.moved_bg.gamma_multiply(0.35),
            (true, false, true) => self.c.del_bg,
            (true, false, false) => self.c.add_bg,
        };
        painter.rect_filled(cell.rect, 0.0, bg);
        let y = cell.rect.center().y;
        let mut x = cell.rect.left();
        for (show, old) in [(cell.numbers.0, true), (cell.numbers.1, false)] {
            if show {
                x += self.num_w;
                if old == cell.old {
                    self.number(x, y, n);
                }
            }
        }
        let (marker, color) = match (changed, cell.old) {
            (false, _) => (" ", self.c.text),
            (true, _) if only_moved => ("↕", self.c.moved_fg),
            (true, true) => ("-", self.c.del_fg),
            (true, false) => ("+", self.c.add_fg),
        };
        self.text(x, cell, side, text, n, changed, spans, (marker, color));
    }

    /// An unchanged row in the unified view: the new line, with both numbers.
    fn unchanged(
        &self,
        d: &StructuralDiff,
        l: &Loaded,
        row: Row,
        rect: Rect,
        spans: &dyn Fn(bool, u32) -> Option<diff_text::Spans>,
    ) {
        let y = rect.center().y;
        let mut x = rect.left() + self.num_w;
        if let Some(o) = row.old {
            self.number(x, y, o);
        }
        x += self.num_w;
        if let Some(n) = row.new {
            self.number(x, y, n);
        }
        let (old, side, text, n) = match (row.new, row.old) {
            (Some(n), _) => (false, &d.new, &l.new, n),
            (None, Some(o)) => (true, &d.old, &l.old, o),
            (None, None) => return,
        };
        let cell = Cell {
            old,
            rect,
            numbers: (true, true),
            changed: false,
        };
        self.text(x, cell, side, text, n, false, spans, (" ", self.c.text));
    }

    fn number(&self, right: f32, y: f32, n: u32) {
        self.ui.painter().text(
            pos2(right - 6.0, y),
            Align2::RIGHT_CENTER,
            (n + 1).to_string(),
            self.num_font.clone(),
            self.c.weak,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        x: f32,
        cell: Cell,
        side: &structural::Side,
        text: &str,
        n: u32,
        changed: bool,
        spans: &dyn Fn(bool, u32) -> Option<diff_text::Spans>,
        (marker, marker_color): (&str, Color32),
    ) {
        let painter = self.ui.painter();
        let marker_w = if self.markers {
            self.char_w * 2.0
        } else {
            self.char_w * 0.5
        };
        if self.markers {
            painter.text(
                pos2(x + 4.0, cell.rect.center().y),
                Align2::LEFT_CENTER,
                marker,
                self.font.clone(),
                marker_color,
            );
        }
        let line = &text[side.lines[n as usize].clone()];
        let marks = &side.marks[n as usize];
        let pick = |m: Mark| -> Vec<std::ops::Range<usize>> {
            marks
                .iter()
                .filter(|(_, k)| *k == m)
                .map(|(r, _)| r.clone())
                .collect()
        };
        let (novel, moved) = if changed {
            (pick(Mark::Novel), pick(Mark::Moved))
        } else {
            (Vec::new(), Vec::new())
        };
        let syntax = spans(cell.old, n);
        let style = LineStyle {
            font: self.font.clone(),
            color: self.c.text,
            weak: self.c.weak,
            syntax: syntax.as_ref(),
            words: &novel,
            word_bg: if cell.old {
                self.c.del_word
            } else {
                self.c.add_word
            },
            found: &moved,
            found_bg: self.c.moved_bg,
            current: None,
            current_bg: Color32::TRANSPARENT,
            tab_width: self.tab,
            show_whitespace: self.show_ws,
        };
        let mut job = diff_text::layout_line(line, &style);
        diff_text::set_wrap(&mut job, None);
        let galley = painter.layout_job(job);
        let text_x = x + marker_w + 4.0;
        let clip = Rect::from_x_y_ranges(text_x..=cell.rect.right(), cell.rect.y_range())
            .intersect(self.ui.clip_rect());
        let top = cell.rect.top() + (self.row_h - galley.rows.first().map_or(self.row_h, |r| r.height())) / 2.0;
        painter
            .with_clip_rect(clip)
            .galley(pos2(text_x, top), galley, self.c.text);
    }
}
