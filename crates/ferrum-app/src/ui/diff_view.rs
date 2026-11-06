//! Text diff of one file: unified or side by side, with syntax colors,
//! word-level changes, search, a map of the changes and line selection for
//! staging. Only the visible rows are laid out, so huge diffs stay smooth.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Id, Key, KeyboardShortcut, Layout, Modifiers, Rect,
    Sense, UiBuilder, pos2, vec2,
};
use ferrum_core::diff::{FILEMODE_LINK, FILEMODE_SUBMODULE};
use ferrum_core::{ChangeKind, DiffLine, FileDiff, LineKind};

use crate::tab::LineSelection;
use crate::ui::diff_prefs::{self, CONTEXT_CHOICES, DiffPrefs, FONT_SIZES};
use crate::ui::diff_text::{self, LineStyle, SideHighlighter};
use crate::util::{added_color, removed_color};

/// Marks "no row" / "no line" in the row tables.
const NONE: u32 = u32::MAX;
const MINIMAP_W: f32 = 12.0;
const HSCROLL_H: f32 = 7.0;
/// Views whose id ends with this are the large diff window.
pub const LARGE_SUFFIX: &str = "-large";
/// Scroll positions remembered per view.
const MAX_REMEMBERED: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// A commit: nothing to stage.
    ReadOnly,
    /// Working copy changes: stage or discard.
    Unstaged,
    /// Index changes: unstage.
    Staged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Stage,
    Unstage,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffAction {
    /// Act on a whole hunk.
    Hunk(Action, usize),
    /// Act on the selected lines.
    Lines(Action),
    /// Load the diff of a large untracked file anyway.
    LoadFull,
    /// Open the file in the editor at this line (see `offer_reveal`).
    Reveal(u32),
}

// ----- State kept between frames -----

#[derive(Clone, Default)]
struct ViewState {
    find: String,
    find_open: bool,
    find_focus: bool,
    find_current: usize,
    /// Scroll position per file.
    scroll: HashMap<String, f32>,
    /// File shown last frame.
    path: String,
    offset: f32,
    hscroll: f32,
    /// Scroll here on the next frame.
    jump: Option<f32>,
    /// The large diff window is open.
    large: bool,
}

fn state_id(id: &str) -> Id {
    Id::unique(("diff-view", id))
}

fn load_state(ctx: &egui::Context, id: &str) -> ViewState {
    ctx.data(|d| d.get_temp::<ViewState>(state_id(id)))
        .unwrap_or_default()
}

fn store_state(ctx: &egui::Context, id: &str, st: ViewState) {
    ctx.data_mut(|d| d.insert_temp(state_id(id), st));
}

fn active_id() -> Id {
    Id::unique("diff-view-active")
}

/// The view that had the pointer or keyboard focus in the last frame.
fn active_view(ctx: &egui::Context) -> Option<String> {
    let (view, frame) = ctx.data(|d| d.get_temp::<(String, u64)>(active_id()))?;
    (ctx.cumulative_frame_nr() <= frame + 1).then_some(view)
}

/// Marks a view as the one keyboard shortcuts go to.
pub fn mark_active(ctx: &egui::Context, id: &str) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(active_id(), (id.to_owned(), frame)));
}

/// Opens the find bar of the diff under the pointer, if any. Returns whether
/// Ctrl+F was taken.
pub fn claim_find(ctx: &egui::Context) -> bool {
    let Some(view) = active_view(ctx) else {
        return false;
    };
    let mut st = load_state(ctx, &view);
    st.find_open = true;
    st.find_focus = true;
    store_state(ctx, &view, st);
    true
}

/// Dev aid (`FERRUM_START`): opens the find bar with `find`, or the large window.
pub fn dev_open(ctx: &egui::Context, id: &str, find: Option<&str>) {
    let mut st = load_state(ctx, id);
    match find {
        Some(q) => {
            st.find_open = true;
            st.find = q.to_owned();
        }
        None => st.large = true,
    }
    store_state(ctx, id, st);
}

pub fn large_open(ctx: &egui::Context, id: &str) -> bool {
    load_state(ctx, id).large
}

/// Lets the next `show` of diff `id` offer "Open in editor at line" in the
/// context menu of a line; the caller handles `DiffAction::Reveal`.
pub fn offer_reveal(ctx: &egui::Context, id: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::unique(("diff-offer-reveal", id)), true));
}

fn reveal_offered(ctx: &egui::Context, id: &str) -> bool {
    let base = id.strip_suffix(LARGE_SUFFIX).unwrap_or(id);
    ctx.data(|d| d.get_temp::<bool>(Id::unique(("diff-offer-reveal", base))))
        .unwrap_or(false)
}

pub fn set_large(ctx: &egui::Context, id: &str, open: bool) {
    let mut st = load_state(ctx, id);
    st.large = open;
    store_state(ctx, id, st);
}

// ----- Rows -----

/// One row on screen: a hunk header, or a line (unified: `left == right`;
/// side by side: either may be `NONE`).
#[derive(Clone, Copy)]
struct VRow {
    hunk: u32,
    left: u32,
    right: u32,
}

impl VRow {
    fn hunk(h: usize) -> Self {
        Self {
            hunk: h as u32,
            left: NONE,
            right: NONE,
        }
    }
    fn pair(left: u32, right: u32) -> Self {
        Self {
            hunk: NONE,
            left,
            right,
        }
    }
}

struct RowLayout {
    key: (bool, bool, u32, u32, usize),
    /// Top of each row, plus the total height at the end.
    tops: Vec<f32>,
    /// Widest line in columns: [unified or left, right].
    max_cols: [usize; 2],
}

struct Found {
    row: usize,
    side: usize,
    range: Range<usize>,
}

type MinimapKey = (u32, usize, String, u32);

/// Everything derived from one diff, kept while the diff does not change.
struct Model {
    key: u64,
    /// (hunk, line) of each line, numbered across hunks.
    lines: Vec<(usize, usize)>,
    /// Lines of the old and new side, in order.
    sides: [Vec<u32>; 2],
    /// Position of each line in `sides` (`NONE` when not on that side).
    side_pos: Vec<[u32; 2]>,
    /// The removed/added line paired with each line, for word changes.
    partner: Vec<u32>,
    /// Rows: [unified, side by side].
    rows: [Vec<VRow>; 2],
    /// First row of each block of changes, and hunk rows: [unified, side by side].
    blocks: [Vec<usize>; 2],
    hunks: [Vec<usize>; 2],
    highlight: Option<(bool, [SideHighlighter; 2])>,
    words: HashMap<u32, Vec<Range<usize>>>,
    found: Option<(String, bool, Vec<Found>)>,
    layout: Option<RowLayout>,
    minimap: Option<(MinimapKey, Vec<u8>)>,
}

fn file_key(file: &FileDiff) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    file.path.hash(&mut h);
    for hunk in &file.hunks {
        hunk.old_start.hash(&mut h);
        hunk.new_start.hash(&mut h);
        for l in &hunk.lines {
            (l.kind as u8).hash(&mut h);
            l.text.hash(&mut h);
            l.no_newline.hash(&mut h);
        }
    }
    h.finish()
}

impl Model {
    fn build(file: &FileDiff, key: u64) -> Self {
        let mut lines = Vec::new();
        let mut offsets = Vec::new();
        for (h, hunk) in file.hunks.iter().enumerate() {
            offsets.push(lines.len());
            lines.extend((0..hunk.lines.len()).map(|l| (h, l)));
        }
        let kind = |g: usize| {
            let (h, l) = lines[g];
            file.hunks[h].lines[l].kind
        };
        let n = lines.len();
        let mut sides = [Vec::new(), Vec::new()];
        let mut side_pos = vec![[NONE; 2]; n];
        for (g, pos) in side_pos.iter_mut().enumerate() {
            let k = kind(g);
            if k != LineKind::Added {
                pos[0] = sides[0].len() as u32;
                sides[0].push(g as u32);
            }
            if k != LineKind::Removed {
                pos[1] = sides[1].len() as u32;
                sides[1].push(g as u32);
            }
        }
        let mut partner = vec![NONE; n];
        let (mut unified, mut split) = (Vec::new(), Vec::new());
        for (h, hunk) in file.hunks.iter().enumerate() {
            unified.push(VRow::hunk(h));
            split.push(VRow::hunk(h));
            let base = offsets[h];
            let len = hunk.lines.len();
            let mut i = 0;
            while i < len {
                let g = (base + i) as u32;
                if kind(base + i) == LineKind::Context {
                    unified.push(VRow::pair(g, g));
                    split.push(VRow::pair(g, g));
                    i += 1;
                    continue;
                }
                // A run of removed lines followed by the added lines that replace them.
                let rs = i;
                while i < len && kind(base + i) == LineKind::Removed {
                    i += 1;
                }
                let (re, as_) = (i, i);
                while i < len && kind(base + i) == LineKind::Added {
                    i += 1;
                }
                let ae = i;
                for k in (rs..re).chain(as_..ae) {
                    let g = (base + k) as u32;
                    unified.push(VRow::pair(g, g));
                }
                for k in 0..(re - rs).max(ae - as_) {
                    let l = (k < re - rs).then(|| (base + rs + k) as u32);
                    let r = (k < ae - as_).then(|| (base + as_ + k) as u32);
                    if let (Some(l), Some(r)) = (l, r) {
                        partner[l as usize] = r;
                        partner[r as usize] = l;
                    }
                    split.push(VRow::pair(l.unwrap_or(NONE), r.unwrap_or(NONE)));
                }
            }
        }
        let rows = [unified, split];
        let mut blocks = [Vec::new(), Vec::new()];
        let mut hunks = [Vec::new(), Vec::new()];
        for m in 0..2 {
            let mut prev_change = false;
            for (i, r) in rows[m].iter().enumerate() {
                if r.hunk != NONE {
                    hunks[m].push(i);
                    prev_change = false;
                    continue;
                }
                let change = r.left != r.right || kind(r.left as usize) != LineKind::Context;
                if change && !prev_change {
                    blocks[m].push(i);
                }
                prev_change = change;
            }
        }
        Self {
            key,
            lines,
            sides,
            side_pos,
            partner,
            rows,
            blocks,
            hunks,
            highlight: None,
            words: HashMap::new(),
            found: None,
            layout: None,
            minimap: None,
        }
    }

    fn line<'a>(&self, file: &'a FileDiff, g: u32) -> &'a DiffLine {
        let (h, l) = self.lines[g as usize];
        &file.hunks[h].lines[l]
    }

    /// Changed byte ranges of line `g` compared with its partner.
    fn words(&mut self, file: &FileDiff, g: u32) -> &[Range<usize>] {
        let p = self.partner[g as usize];
        if p == NONE {
            return &[];
        }
        if !self.words.contains_key(&g) {
            let (old, new) = if self.line(file, g).kind == LineKind::Removed {
                (g, p)
            } else {
                (p, g)
            };
            let (a, b) =
                diff_text::word_changes(&self.line(file, old).text, &self.line(file, new).text);
            self.words.insert(old, a);
            self.words.insert(new, b);
        }
        &self.words[&g]
    }
}

type SharedModel = Arc<Mutex<Option<Model>>>;

fn model_for(ctx: &egui::Context, id: &str, file: &FileDiff) -> SharedModel {
    let mid = Id::unique(("diff-model", id));
    let shared: SharedModel =
        ctx.data_mut(|d| d.get_temp_mut_or_default::<SharedModel>(mid).clone());
    let key = file_key(file);
    {
        let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().is_none_or(|m| m.key != key) {
            *guard = Some(Model::build(file, key));
        }
    }
    shared
}

// ----- Colors and sizes -----

struct Paint {
    font: FontId,
    num_font: FontId,
    char_w: f32,
    row_h: f32,
    num_w: f32,
    marker_w: f32,
    text: Color32,
    weak: Color32,
    add_fg: Color32,
    del_fg: Color32,
    add_bg: Color32,
    del_bg: Color32,
    add_word: Color32,
    del_word: Color32,
    sel_bg: Color32,
    found_bg: Color32,
    current_bg: Color32,
    empty_bg: Color32,
    hover: Color32,
}

impl Paint {
    fn new(ui: &egui::Ui, prefs: &DiffPrefs, max_line_no: u32) -> Self {
        let size = prefs.font_size();
        let font = FontId::monospace(size);
        let num_font = FontId::monospace((size - 1.0).max(7.0));
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'm'));
        let num_char_w = ui.fonts_mut(|f| f.glyph_width(&num_font, '0'));
        let digits = max_line_no.max(1).ilog10() as f32 + 1.0;
        let v = ui.visuals();
        let dark = v.dark_mode;
        let rgba = Color32::from_rgba_unmultiplied;
        let (add_bg, del_bg, add_word, del_word) = if dark {
            (
                rgba(0x2e, 0xa0, 0x43, 40),
                rgba(0xf8, 0x51, 0x49, 40),
                rgba(0x2e, 0xa0, 0x43, 110),
                rgba(0xf8, 0x51, 0x49, 110),
            )
        } else {
            (
                rgba(0x2e, 0xa0, 0x43, 45),
                rgba(0xf8, 0x51, 0x49, 40),
                rgba(0x2e, 0xa0, 0x43, 120),
                rgba(0xf8, 0x51, 0x49, 110),
            )
        };
        Self {
            row_h: (size * 1.5).round(),
            num_w: (digits.max(3.0) * num_char_w + 12.0).round(),
            marker_w: if prefs.markers {
                char_w * 2.0
            } else {
                char_w * 0.5
            },
            font,
            num_font,
            char_w,
            text: v.text_color(),
            weak: v.weak_text_color(),
            add_fg: added_color(ui),
            del_fg: removed_color(ui),
            add_bg,
            del_bg,
            add_word,
            del_word,
            sel_bg: v.selection.bg_fill.gamma_multiply(0.7),
            found_bg: rgba(0xf0, 0xc0, 0x20, if dark { 90 } else { 120 }),
            current_bg: rgba(0xff, 0x8c, 0x00, if dark { 170 } else { 190 }),
            empty_bg: v.faint_bg_color,
            hover: v.selection.stroke.color,
        }
    }

    /// Width left for text in a cell of `width` with `numbers` number columns.
    fn text_width(&self, width: f32, numbers: usize) -> f32 {
        (width - self.num_w * numbers as f32 - self.marker_w - 4.0).max(self.char_w * 4.0)
    }
}

/// One side of a row.
struct Cell {
    rect: Rect,
    /// Line numbers to show: (old, new).
    numbers: (bool, bool),
}

// ----- The view -----

/// Draws the diff. `selection` enables picking lines (only in staging modes).
pub fn show(
    ui: &mut egui::Ui,
    id: &str,
    file: &FileDiff,
    mode: DiffMode,
    line_staging: bool,
    mut selection: Option<&mut LineSelection>,
) -> Option<DiffAction> {
    let ctx = ui.ctx().clone();
    let mut prefs = diff_prefs::get(&ctx);
    let original_prefs = prefs.clone();
    let mut st = load_state(&ctx, id);
    let large_window = id.ends_with(LARGE_SUFFIX);
    if st.path != file.path {
        st.jump = Some(st.scroll.get(&file.path).copied().unwrap_or(0.0));
        st.hscroll = 0.0;
        st.find_current = 0;
        st.path = file.path.clone();
    }
    // A diff that ignores whitespace cannot be applied back to the files.
    let ws_locked = prefs.ignore_whitespace && mode != DiffMode::ReadOnly;
    let mode = if ws_locked { DiffMode::ReadOnly } else { mode };
    let line_staging = line_staging && !ws_locked;
    let mut action = None;
    let mut nav = None;

    toolbar(
        ui,
        &mut prefs,
        &mut st,
        file,
        (mode, line_staging),
        selection.as_deref_mut(),
        &mut action,
        &mut nav,
        large_window,
    );
    if ws_locked {
        ui.weak(t!(
            "Staging lines and hunks is off while whitespace is ignored."
        ));
    }
    meta_info(ui, file);
    let find_focused = st.find_open && find_bar(ui, &mut st, &mut nav);
    ui.separator();

    let body_rect = ui.available_rect_before_wrap();
    if ui.rect_contains_pointer(body_rect) || find_focused {
        mark_active(&ctx, id);
    }

    if file.hunks.is_empty() {
        ui.weak(if file.binary {
            t!("Binary file.")
        } else {
            t!("No changes to show (empty file or mode change).")
        });
    } else {
        if active_view(&ctx).as_deref() == Some(id) {
            let pressed = |k| {
                ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, k)))
            };
            if pressed(Key::ArrowDown) {
                nav = Some(Nav::NextHunk);
            }
            if pressed(Key::ArrowUp) {
                nav = Some(Nav::PrevHunk);
            }
        }
        let model = model_for(&ctx, id, file);
        let mut guard = model.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = guard.as_mut() {
            let input = BodyInput {
                id,
                file,
                mode,
                line_staging,
                prefs: &prefs,
            };
            if let Some(a) = body(ui, &input, m, &mut st, selection, nav) {
                action = Some(a);
            }
        }
    }

    if st.scroll.len() > MAX_REMEMBERED {
        st.scroll.clear();
    }
    st.scroll.insert(file.path.clone(), st.offset);
    store_state(&ctx, id, st);
    if prefs != original_prefs {
        diff_prefs::set(&ctx, prefs);
    }
    action
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    NextChange,
    PrevChange,
    NextHunk,
    PrevHunk,
    NextFound,
    PrevFound,
    /// The query changed: go to the first match from the current position.
    Found,
}

fn toggle(ui: &mut egui::Ui, value: &mut bool, label: String, tip: String) {
    if ui
        .selectable_label(*value, label)
        .on_hover_text(tip)
        .clicked()
    {
        *value = !*value;
    }
}

#[allow(clippy::too_many_arguments)]
fn toolbar(
    ui: &mut egui::Ui,
    prefs: &mut DiffPrefs,
    st: &mut ViewState,
    file: &FileDiff,
    (mode, line_staging): (DiffMode, bool),
    selection: Option<&mut LineSelection>,
    action: &mut Option<DiffAction>,
    nav: &mut Option<Nav>,
    large_window: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(added_color(ui), format!("+{}", file.additions));
        ui.colored_label(removed_color(ui), format!("−{}", file.deletions));
        let selected = selection.as_ref().map_or(0, |s| s.lines.len());
        if selected > 0 && line_staging {
            ui.separator();
            ui.label(if selected == 1 {
                t!("1 line selected")
            } else {
                t!("{} lines selected", selected)
            });
            selection_buttons(ui, mode, action);
            if ui.small_button(t!("Clear")).clicked()
                && let Some(s) = selection
            {
                s.clear();
            }
        } else if line_staging && mode != DiffMode::ReadOnly {
            ui.weak(t!("Click lines to select them, Shift+click for a range."));
        }
        ui.separator();

        toggle(
            ui,
            &mut prefs.side_by_side,
            t!("Side by side"),
            t!("Show the old and new version next to each other"),
        );
        toggle(ui, &mut prefs.wrap, t!("Wrap"), t!("Wrap long lines"));
        toggle(
            ui,
            &mut prefs.ignore_whitespace,
            t!("Ignore whitespace"),
            t!("Hide changes that only touch whitespace"),
        );
        toggle(
            ui,
            &mut prefs.full_file,
            t!("Entire file"),
            t!("Show the whole file, not just the changed parts"),
        );
        ui.add_enabled_ui(!prefs.full_file, |ui| {
            egui::ComboBox::from_id_salt("diff-context")
                .selected_text(t!("Context: {}", prefs.context_lines))
                .show_ui(ui, |ui| {
                    for n in CONTEXT_CHOICES {
                        ui.selectable_value(&mut prefs.context_lines, n, t!("{} lines", n));
                    }
                })
                .response
                .on_hover_text(t!("Unchanged lines shown around each change"));
        });
        ui.separator();
        let size = prefs.font_size();
        if ui
            .add_enabled(size > *FONT_SIZES.start(), egui::Button::new("A−").small())
            .on_hover_text(t!("Smaller text"))
            .clicked()
        {
            prefs.font_size = size - 1.0;
        }
        if ui
            .add_enabled(size < *FONT_SIZES.end(), egui::Button::new("A+").small())
            .on_hover_text(t!("Larger text"))
            .clicked()
        {
            prefs.font_size = size + 1.0;
        }
        ui.separator();
        if ui
            .small_button("⏶")
            .on_hover_text(t!("Previous change"))
            .clicked()
        {
            *nav = Some(Nav::PrevChange);
        }
        if ui
            .small_button("⏷")
            .on_hover_text(t!("Next change"))
            .clicked()
        {
            *nav = Some(Nav::NextChange);
        }
        if ui
            .selectable_label(st.find_open, "🔍")
            .on_hover_text(t!("Find in diff (Ctrl+F)"))
            .clicked()
        {
            st.find_open = !st.find_open;
            st.find_focus = st.find_open;
        }
        ui.menu_button("⚙", |ui| {
            ui.checkbox(&mut prefs.show_whitespace, t!("Show whitespace"));
            ui.horizontal(|ui| {
                ui.label(t!("Tab width"));
                let mut w = prefs.tab_width();
                ui.add(egui::DragValue::new(&mut w).range(1..=16));
                prefs.tab_width = w as u8;
            });
            ui.checkbox(&mut prefs.markers, t!("Show -/+ markers"));
            ui.checkbox(&mut prefs.syntax, t!("Syntax highlighting"));
            ui.checkbox(&mut prefs.word_diff, t!("Highlight changed words"));
            ui.checkbox(&mut prefs.minimap, t!("Map of changes"));
        })
        .response
        .on_hover_text(t!("Diff options"));
        if !large_window
            && ui
                .small_button("⛶")
                .on_hover_text(t!("Open in a large window (Space)"))
                .clicked()
        {
            st.large = true;
        }
    });
}

fn selection_buttons(ui: &mut egui::Ui, mode: DiffMode, action: &mut Option<DiffAction>) {
    match mode {
        DiffMode::Unstaged => {
            if ui.button(t!("Stage lines")).clicked() {
                *action = Some(DiffAction::Lines(Action::Stage));
            }
            if ui.button(t!("Discard lines")).clicked() {
                *action = Some(DiffAction::Lines(Action::Discard));
            }
        }
        DiffMode::Staged => {
            if ui.button(t!("Unstage lines")).clicked() {
                *action = Some(DiffAction::Lines(Action::Unstage));
            }
        }
        DiffMode::ReadOnly => {}
    }
}

fn mode_label(mode: u32) -> String {
    let name = match mode {
        0o100755 => t!("executable"),
        FILEMODE_LINK => t!("symbolic link"),
        FILEMODE_SUBMODULE => t!("submodule"),
        _ => t!("regular file"),
    };
    format!("{mode:o} ({name})")
}

/// Rename and file mode changes.
pub fn meta_info(ui: &mut egui::Ui, file: &FileDiff) {
    let mode_changed = file.old_mode != 0 && file.new_mode != 0 && file.old_mode != file.new_mode;
    let unusual_new = file.old_mode == 0
        && matches!(file.kind, ChangeKind::Added | ChangeKind::Untracked)
        && ![0, 0o100644].contains(&file.new_mode);
    if file.old_path.is_none() && !mode_changed && !unusual_new {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        if let Some(old) = &file.old_path {
            ui.label(t!("Renamed: {} -> {}", old, file.path));
        }
        if mode_changed {
            ui.label(t!(
                "Mode changed: {} -> {}",
                mode_label(file.old_mode),
                mode_label(file.new_mode)
            ));
        }
        if unusual_new {
            ui.label(t!("New file with mode {}", mode_label(file.new_mode)));
        }
    });
}

/// The find bar. Returns whether its text field has the focus.
fn find_bar(ui: &mut egui::Ui, st: &mut ViewState, nav: &mut Option<Nav>) -> bool {
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut st.find)
                .hint_text(t!("Find in diff"))
                .desired_width(220.0),
        );
        if st.find_focus {
            r.request_focus();
            st.find_focus = false;
        }
        if r.changed() {
            *nav = Some(Nav::Found);
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            *nav = Some(if ui.input(|i| i.modifiers.shift) {
                Nav::PrevFound
            } else {
                Nav::NextFound
            });
            r.request_focus();
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) && (r.has_focus() || r.lost_focus()) {
            st.find_open = false;
        }
        if ui
            .small_button("⏶")
            .on_hover_text(t!("Previous match (Shift+Enter)"))
            .clicked()
        {
            *nav = Some(Nav::PrevFound);
        }
        if ui
            .small_button("⏷")
            .on_hover_text(t!("Next match (Enter)"))
            .clicked()
        {
            *nav = Some(Nav::NextFound);
        }
        // The count is painted by the body, which knows the matches.
        let count = ui
            .allocate_space(vec2(110.0, ui.spacing().interact_size.y))
            .1;
        ui.data_mut(|d| d.insert_temp(Id::unique("diff-find-count-rect"), count));
        if ui.small_button("✖").on_hover_text(t!("Close")).clicked() {
            st.find_open = false;
        }
        r.has_focus()
    })
    .inner
}

struct BodyInput<'a> {
    id: &'a str,
    file: &'a FileDiff,
    mode: DiffMode,
    line_staging: bool,
    prefs: &'a DiffPrefs,
}

fn body(
    ui: &mut egui::Ui,
    inp: &BodyInput,
    m: &mut Model,
    st: &mut ViewState,
    mut selection: Option<&mut LineSelection>,
    nav: Option<Nav>,
) -> Option<DiffAction> {
    let (file, prefs) = (inp.file, inp.prefs);
    let split = prefs.side_by_side;
    let mi = split as usize;
    let max_no = file
        .hunks
        .iter()
        .map(|h| (h.old_start + h.old_lines).max(h.new_start + h.new_lines))
        .max()
        .unwrap_or(1);
    let p = Paint::new(ui, prefs, max_no);
    let mut action = None;
    let reveal = reveal_offered(ui.ctx(), inp.id);

    let avail = ui.available_rect_before_wrap();
    let map_w = if prefs.minimap { MINIMAP_W + 2.0 } else { 0.0 };
    let area = Rect::from_min_max(avail.min, pos2(avail.right() - map_w, avail.bottom()));
    let bar_w = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin * 2.0;
    let width = (area.width() - bar_w).max(100.0);
    let gap = 6.0;
    let half = ((width - gap) / 2.0).floor();
    let text_w = if split {
        p.text_width(half, 1)
    } else {
        p.text_width(width, 2)
    };
    let tab = prefs.tab_width();

    // Row heights, recomputed when what they depend on changes.
    let lkey = (
        split,
        prefs.wrap,
        if prefs.wrap { text_w as u32 } else { 0 },
        (p.char_w * 100.0) as u32,
        tab,
    );
    if m.layout.as_ref().is_none_or(|l| l.key != lkey) {
        let per_row = ((text_w / p.char_w).floor() as usize).max(1);
        let mut tops = Vec::with_capacity(m.rows[mi].len() + 1);
        let mut max_cols = [0usize; 2];
        let mut y = 0.0;
        for r in &m.rows[mi] {
            tops.push(y);
            if r.hunk != NONE {
                y += p.row_h;
                continue;
            }
            let mut n = 1;
            for (s, g) in [r.left, r.right].into_iter().enumerate() {
                if g == NONE || (!split && s == 1) {
                    continue;
                }
                let line = m.line(file, g);
                let c = diff_text::display_width(&line.text, tab)
                    + if line.no_newline { 24 } else { 0 };
                max_cols[s] = max_cols[s].max(c);
                if prefs.wrap {
                    n = n.max(c.div_ceil(per_row));
                }
            }
            y += p.row_h * n as f32;
        }
        tops.push(y);
        m.layout = Some(RowLayout {
            key: lkey,
            tops,
            max_cols,
        });
    }

    // Search matches.
    let query = if st.find_open {
        st.find.trim().to_owned()
    } else {
        String::new()
    };
    if m.found
        .as_ref()
        .is_none_or(|(q, s, _)| *q != query || *s != split)
    {
        let mut found = Vec::new();
        if !query.is_empty() {
            for (i, r) in m.rows[mi].iter().enumerate() {
                if r.hunk != NONE {
                    continue;
                }
                for (s, g) in [r.left, r.right].into_iter().enumerate() {
                    if g == NONE || (!split && s == 1) {
                        continue;
                    }
                    for range in diff_text::find_all(&m.line(file, g).text, &query) {
                        found.push(Found {
                            row: i,
                            side: s,
                            range,
                        });
                    }
                }
            }
        }
        m.found = Some((query.clone(), split, found));
    }
    let found_len = m.found.as_ref().map_or(0, |f| f.2.len());
    if found_len > 0 {
        st.find_current = st.find_current.min(found_len - 1);
    }

    let layout = m.layout.as_ref().expect("layout computed above");
    let total_h = *layout.tops.last().unwrap_or(&0.0);
    let view_h = area.height();
    let row_top = |row: usize| layout.tops[row];

    // Navigation: decide where to scroll.
    let margin = p.row_h * 2.0;
    let current = st.offset;
    let jump_to_row = |rows: &[usize], forward: bool| -> Option<f32> {
        let target = |r: usize| (row_top(r) - margin).max(0.0);
        if forward {
            rows.iter().map(|&r| target(r)).find(|&t| t > current + 1.0)
        } else {
            rows.iter()
                .rev()
                .map(|&r| target(r))
                .find(|&t| t < current - 1.0)
        }
    };
    let mut reveal_found = false;
    match nav {
        Some(Nav::NextChange) => st.jump = jump_to_row(&m.blocks[mi], true).or(st.jump),
        Some(Nav::PrevChange) => st.jump = jump_to_row(&m.blocks[mi], false).or(st.jump),
        Some(Nav::NextHunk) => st.jump = jump_to_row(&m.hunks[mi], true).or(st.jump),
        Some(Nav::PrevHunk) => st.jump = jump_to_row(&m.hunks[mi], false).or(st.jump),
        Some(Nav::NextFound) if found_len > 0 => {
            st.find_current = (st.find_current + 1) % found_len;
            reveal_found = true;
        }
        Some(Nav::PrevFound) if found_len > 0 => {
            st.find_current = (st.find_current + found_len - 1) % found_len;
            reveal_found = true;
        }
        Some(Nav::Found) if found_len > 0 => {
            // First match at or after the top of the view.
            let found = &m.found.as_ref().expect("computed").2;
            st.find_current = found
                .iter()
                .position(|f| row_top(f.row) >= current)
                .unwrap_or(0);
            reveal_found = true;
        }
        _ => {}
    }
    if reveal_found {
        let f = &m.found.as_ref().expect("computed").2[st.find_current];
        let top = row_top(f.row);
        if top < current || top + p.row_h > current + view_h {
            st.jump = Some((top - view_h / 3.0).max(0.0));
        }
    }
    // Match count in the find bar.
    if st.find_open
        && let Some(rect) = ui.data(|d| d.get_temp::<Rect>(Id::unique("diff-find-count-rect")))
    {
        let text = if query.is_empty() {
            String::new()
        } else if found_len == 0 {
            t!("No matches")
        } else {
            t!("{} of {}", st.find_current + 1, found_len)
        };
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.0),
            p.weak,
        );
    }

    // Horizontal scrolling (without wrap): line numbers stay in place.
    let max_cols = layout.max_cols[0].max(layout.max_cols[1]);
    let content_w = (max_cols + 2) as f32 * p.char_w;
    let max_h = if prefs.wrap {
        0.0
    } else {
        (content_w - text_w).max(0.0)
    };
    if ui.rect_contains_pointer(area) {
        let dx = ui.input(|i| i.smooth_scroll_delta.x);
        st.hscroll -= dx;
    }
    st.hscroll = st.hscroll.clamp(0.0, max_h);

    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(("diff-scroll", inp.id))
        .auto_shrink(false);
    if let Some(y) = st.jump.take() {
        scroll = scroll.vertical_scroll_offset(y.clamp(0.0, (total_h - view_h).max(0.0)));
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(area));
    let mut selected_rect: Option<Rect> = None;
    let output = scroll.show_viewport(&mut child, |ui, viewport| {
        ui.set_height(total_h);
        ui.set_width(width);
        let origin = ui.min_rect().min;
        let layout = m.layout.as_ref().expect("layout computed above");
        let first = layout
            .tops
            .partition_point(|&t| t <= viewport.min.y)
            .saturating_sub(1);
        let mut last = first;
        while last < m.rows[mi].len() && layout.tops[last] < viewport.max.y {
            last += 1;
        }
        let visible: Vec<(usize, f32, f32)> = (first..last)
            .map(|i| (i, layout.tops[i], layout.tops[i + 1] - layout.tops[i]))
            .collect();

        // Syntax colors for the visible lines (and everything above them).
        highlight_upto(ui, m, file, prefs, &visible, mi);

        for (i, top, h) in visible {
            let row = m.rows[mi][i];
            let rect = Rect::from_min_size(origin + vec2(0.0, top), vec2(width, h));
            if row.hunk != NONE {
                if let Some(a) = hunk_row(ui, rect, file, row.hunk as usize, inp.mode, &p) {
                    action = Some(a);
                }
                continue;
            }
            let cells: Vec<(u32, usize, Cell)> = if split {
                let left = Rect::from_min_size(rect.min, vec2(half, h));
                let right = Rect::from_min_size(rect.min + vec2(half + gap, 0.0), vec2(half, h));
                ui.painter().vline(
                    rect.left() + half + gap / 2.0,
                    rect.y_range(),
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );
                vec![
                    (
                        row.left,
                        0,
                        Cell {
                            rect: left,
                            numbers: (true, false),
                        },
                    ),
                    (
                        row.right,
                        1,
                        Cell {
                            rect: right,
                            numbers: (false, true),
                        },
                    ),
                ]
            } else {
                vec![(
                    row.left,
                    0,
                    Cell {
                        rect,
                        numbers: (true, true),
                    },
                )]
            };
            for (g, side, cell) in cells {
                if g == NONE {
                    ui.painter().rect_filled(cell.rect, 0.0, p.empty_bg);
                    continue;
                }
                let line = m.line(file, g);
                let (h_idx, l_idx) = m.lines[g as usize];
                let selectable =
                    selection.is_some() && inp.line_staging && line.kind != LineKind::Context;
                // Clicks select lines; every line has a context menu.
                let resp = ui.interact(
                    cell.rect,
                    Id::unique((inp.id, "row", i, side)),
                    Sense::click(),
                );
                let is_sel = selection
                    .as_ref()
                    .is_some_and(|s| s.lines.contains(&(h_idx, l_idx)));
                if is_sel {
                    selected_rect = Some(cell.rect);
                }
                let found = m.found.as_ref().map(|f| &f.2[..]).unwrap_or(&[]);
                let start = found.partition_point(|f| (f.row, f.side) < (i, side));
                let end = start
                    + found[start..]
                        .iter()
                        .take_while(|f| f.row == i && f.side == side)
                        .count();
                let ranges: Vec<Range<usize>> =
                    found[start..end].iter().map(|f| f.range.clone()).collect();
                let current = (st.find_current >= start && st.find_current < end)
                    .then(|| found[st.find_current].range.clone());
                let hl_side = if split {
                    side
                } else if line.kind == LineKind::Removed {
                    0
                } else {
                    1
                };
                paint_line(
                    ui,
                    &p,
                    m,
                    file,
                    LineCell {
                        g,
                        cell: &cell,
                        hl_side,
                        found: &ranges,
                        current,
                        hscroll: st.hscroll,
                        wrap: prefs.wrap.then_some(text_w),
                        selected: is_sel,
                        hovered: selectable && resp.hovered(),
                    },
                    prefs,
                );
                // One menu for every line: open in the editor, copy, line history.
                resp.context_menu(|ui| {
                    if reveal {
                        let no = if line.kind == LineKind::Removed {
                            line.old_no
                        } else {
                            line.new_no
                        };
                        if let Some(no) = no
                            && ui.button(t!("Open in editor at line {}", no)).clicked()
                        {
                            action = Some(DiffAction::Reveal(no));
                            ui.close();
                        }
                    }
                    if ui.button(t!("Copy line")).clicked() {
                        ui.ctx().copy_text(line.text.clone());
                        ui.close();
                    }
                    crate::ui::file_lines::diff_line_menu(
                        ui,
                        inp.id,
                        file,
                        selection.as_deref(),
                        (h_idx, l_idx),
                    );
                });
                if resp.clicked()
                    && selectable
                    && let Some(sel) = selection.as_deref_mut()
                {
                    let shift = ui.input(|i| i.modifiers.shift);
                    let at = (h_idx, l_idx);
                    match (shift, sel.anchor) {
                        (true, Some(anchor)) => select_range(file, sel, anchor, at),
                        _ => {
                            if !sel.lines.remove(&at) {
                                sel.lines.insert(at);
                            }
                            sel.anchor = Some(at);
                        }
                    }
                }
            }
        }
    });
    st.offset = output.state.clamped_offset().y;
    ui.allocate_rect(avail, Sense::hover());
    let inner = output.inner_rect;

    // Horizontal scrollbar.
    if max_h > 0.0 {
        let left = if split { 0.0 } else { p.num_w * 2.0 };
        let track = Rect::from_min_max(
            pos2(inner.left() + left, inner.bottom() - HSCROLL_H),
            pos2(inner.right(), inner.bottom()),
        );
        let resp = ui.interact(track, Id::unique((inp.id, "hscroll")), Sense::drag());
        let thumb_w = (track.width() * text_w / content_w).max(24.0);
        let span = (track.width() - thumb_w).max(1.0);
        if resp.dragged() {
            st.hscroll = (st.hscroll + resp.drag_delta().x / span * max_h).clamp(0.0, max_h);
        }
        let x = track.left() + st.hscroll / max_h * span;
        let thumb = Rect::from_min_size(pos2(x, track.top() + 1.0), vec2(thumb_w, HSCROLL_H - 2.0));
        let color = if resp.hovered() || resp.dragged() {
            ui.visuals().widgets.hovered.bg_fill
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        ui.painter().rect_filled(thumb, 3.0, color);
    }

    // Floating buttons next to the selected lines.
    let selected = selection.as_ref().map_or(0, |s| s.lines.len());
    if selected > 0
        && inp.line_staging
        && inp.mode != DiffMode::ReadOnly
        && let Some(rect) = selected_rect
        && inner.intersects(rect)
    {
        let y = (rect.bottom() + 2.0).min(inner.bottom() - 30.0);
        egui::Area::new(Id::unique((inp.id, "floating-actions")))
            .order(egui::Order::Foreground)
            .pivot(Align2::RIGHT_TOP)
            .fixed_pos(pos2(inner.right() - 16.0, y))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| selection_buttons(ui, inp.mode, &mut action));
                });
            });
    }

    if prefs.minimap {
        let map = Rect::from_min_max(
            pos2(area.right() + 2.0, inner.top()),
            pos2(avail.right(), inner.bottom()),
        );
        let view = MapView {
            offset: st.offset,
            view_h,
            total_h,
            mi,
            query,
        };
        if let Some(y) = minimap(ui, map, m, file, &p, &view, inp.id) {
            st.jump = Some(y);
            ui.ctx().request_repaint();
        }
    }
    action
}

/// Advances syntax highlighting far enough for the visible rows.
fn highlight_upto(
    ui: &egui::Ui,
    m: &mut Model,
    file: &FileDiff,
    prefs: &DiffPrefs,
    visible: &[(usize, f32, f32)],
    mi: usize,
) {
    if !prefs.syntax {
        return;
    }
    let dark = ui.visuals().dark_mode;
    if m.highlight.as_ref().is_none_or(|(d, _)| *d != dark) {
        m.highlight = diff_text::syntax_for(&file.path).map(|s| {
            (
                dark,
                [SideHighlighter::new(s, dark), SideHighlighter::new(s, dark)],
            )
        });
    }
    let mut upto = [0usize; 2];
    for &(i, _, _) in visible {
        let r = m.rows[mi][i];
        for g in [r.left, r.right] {
            if g == NONE {
                continue;
            }
            for (s, u) in upto.iter_mut().enumerate() {
                let pos = m.side_pos[g as usize][s];
                if pos != NONE {
                    *u = (*u).max(pos as usize + 1);
                }
            }
        }
    }
    let Model {
        highlight,
        sides,
        lines,
        ..
    } = m;
    let Some((_, hl)) = highlight else { return };
    let mut finished = true;
    for (s, side) in hl.iter_mut().enumerate() {
        let text = |k: usize| {
            let (h, l) = lines[sides[s][k] as usize];
            file.hunks[h].lines[l].text.as_str()
        };
        finished &= side.advance(text, upto[s]);
    }
    if !finished {
        ui.ctx().request_repaint();
    }
}

fn hunk_row(
    ui: &mut egui::Ui,
    rect: Rect,
    file: &FileDiff,
    h: usize,
    mode: DiffMode,
    p: &Paint,
) -> Option<DiffAction> {
    let mut action = None;
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
    let visible = ui.clip_rect();
    ui.painter().text(
        pos2(visible.left().max(rect.left()) + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        &file.hunks[h].header,
        p.num_font.clone(),
        p.weak,
    );
    // Buttons stay visible on the right edge of the viewport.
    let right = visible.right().min(rect.right());
    let area = Rect::from_x_y_ranges((right - 240.0)..=right - 6.0, rect.y_range());
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(area)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            let mut btn = |ui: &mut egui::Ui, label: String, a: Action| {
                if ui.small_button(label).clicked() {
                    action = Some(DiffAction::Hunk(a, h));
                }
            };
            match mode {
                DiffMode::Unstaged => {
                    btn(ui, t!("Discard hunk"), Action::Discard);
                    btn(ui, t!("Stage hunk"), Action::Stage);
                }
                DiffMode::Staged => btn(ui, t!("Unstage hunk"), Action::Unstage),
                DiffMode::ReadOnly => {}
            }
        },
    );
    action
}

struct LineCell<'a> {
    g: u32,
    cell: &'a Cell,
    /// Which side's syntax colors to use.
    hl_side: usize,
    found: &'a [Range<usize>],
    current: Option<Range<usize>>,
    hscroll: f32,
    wrap: Option<f32>,
    selected: bool,
    hovered: bool,
}

fn paint_line(
    ui: &egui::Ui,
    p: &Paint,
    m: &mut Model,
    file: &FileDiff,
    c: LineCell,
    prefs: &DiffPrefs,
) {
    let line = m.line(file, c.g);
    let rect = c.cell.rect;
    let painter = ui.painter();
    let bg = match line.kind {
        LineKind::Added => p.add_bg,
        LineKind::Removed => p.del_bg,
        LineKind::Context => Color32::TRANSPARENT,
    };
    painter.rect_filled(rect, 0.0, if c.selected { p.sel_bg } else { bg });
    if c.hovered {
        painter.rect_stroke(rect, 0.0, (1.0, p.hover), egui::StrokeKind::Inside);
    }
    let y = rect.top() + p.row_h / 2.0;
    let mut x = rect.left();
    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
    for (show, n) in [
        (c.cell.numbers.0, line.old_no),
        (c.cell.numbers.1, line.new_no),
    ] {
        if show {
            x += p.num_w;
            painter.text(
                pos2(x - 6.0, y),
                Align2::RIGHT_CENTER,
                num(n),
                p.num_font.clone(),
                p.weak,
            );
        }
    }
    if prefs.markers {
        let (marker, color) = match line.kind {
            LineKind::Added => ("+", p.add_fg),
            LineKind::Removed => ("-", p.del_fg),
            LineKind::Context => (" ", p.text),
        };
        painter.text(
            pos2(x + 4.0, y),
            Align2::LEFT_CENTER,
            marker,
            p.font.clone(),
            color,
        );
    }
    let text_x = x + p.marker_w + 4.0;

    let syntax = m
        .highlight
        .as_ref()
        .and_then(|(_, hl)| {
            let pos = m.side_pos[c.g as usize][c.hl_side];
            (pos != NONE)
                .then(|| hl[c.hl_side].done.get(pos as usize))
                .flatten()
        })
        .cloned();
    let words: Vec<Range<usize>> = if prefs.word_diff {
        m.words(file, c.g).to_vec()
    } else {
        Vec::new()
    };
    let style = LineStyle {
        font: p.font.clone(),
        color: p.text,
        weak: p.weak,
        syntax: syntax.as_ref(),
        words: &words,
        word_bg: if line.kind == LineKind::Added {
            p.add_word
        } else {
            p.del_word
        },
        found: c.found,
        found_bg: p.found_bg,
        current: c.current,
        current_bg: p.current_bg,
        tab_width: prefs.tab_width(),
        show_whitespace: prefs.show_whitespace,
    };
    let mut job = diff_text::layout_line(&line.text, &style);
    if line.no_newline {
        diff_text::append_note(
            &mut job,
            &format!("  {}", t!("(no newline at end)")),
            &style,
        );
    }
    // Wrap at a whole number of characters, as the row heights assume.
    let wrap_at = c
        .wrap
        .map(|w| (w / p.char_w).floor().max(1.0) * p.char_w + 0.5);
    diff_text::set_wrap(&mut job, wrap_at);
    let galley = painter.layout_job(job);
    let clip =
        Rect::from_x_y_ranges(text_x..=rect.right(), rect.y_range()).intersect(ui.clip_rect());
    let offset = if c.wrap.is_some() { 0.0 } else { c.hscroll };
    let first_row_h = galley.rows.first().map_or(p.row_h, |r| r.height());
    let top = rect.top() + (p.row_h - first_row_h) / 2.0;
    painter
        .with_clip_rect(clip)
        .galley(pos2(text_x - offset, top), galley, p.text);
}

struct MapView {
    offset: f32,
    view_h: f32,
    total_h: f32,
    mi: usize,
    query: String,
}

/// A strip beside the scrollbar with a mark for every change and search
/// match. Returns the scroll offset to go to when clicked or dragged.
fn minimap(
    ui: &mut egui::Ui,
    rect: Rect,
    m: &mut Model,
    file: &FileDiff,
    p: &Paint,
    v: &MapView,
    id: &str,
) -> Option<f32> {
    if rect.height() < 10.0 || v.total_h <= 0.0 {
        return None;
    }
    // Short diffs fill only the top of the map, like they fill the view.
    let span = v.total_h.max(v.view_h);
    let px = rect.height().round().max(1.0) as u32;
    let key: MinimapKey = (px, v.mi, v.query.clone(), span as u32);
    // One cell per pixel: bit 0 removed, bit 1 added, bit 2 search match.
    if m.minimap.as_ref().is_none_or(|(k, _)| *k != key) {
        let layout = m.layout.as_ref().expect("layout computed");
        let mut cells = vec![0u8; px as usize];
        let scale = px as f32 / span;
        let mut mark = |top: f32, h: f32, bit: u8| {
            let a = (top * scale) as usize;
            let b = (((top + h) * scale).ceil() as usize).max(a + 1);
            for c in cells.iter_mut().take(b.min(px as usize)).skip(a) {
                *c |= bit;
            }
        };
        for (i, r) in m.rows[v.mi].iter().enumerate() {
            if r.hunk != NONE {
                continue;
            }
            let (top, h) = (layout.tops[i], layout.tops[i + 1] - layout.tops[i]);
            for g in [r.left, r.right] {
                if g == NONE {
                    continue;
                }
                match m.line(file, g).kind {
                    LineKind::Removed => mark(top, h, 1),
                    LineKind::Added => mark(top, h, 2),
                    LineKind::Context => {}
                }
            }
        }
        if let Some((_, _, found)) = &m.found {
            for f in found {
                mark(layout.tops[f.row], p.row_h, 4);
            }
        }
        m.minimap = Some((key, cells));
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    if let Some((_, cells)) = &m.minimap {
        let (l, r) = (rect.left(), rect.right());
        let mid = (l + r) / 2.0;
        for (y, &c) in cells.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let y0 = rect.top() + y as f32;
            let band = |x0: f32, x1: f32, color| {
                painter.rect_filled(Rect::from_x_y_ranges(x0..=x1, y0..=y0 + 1.0), 0.0, color);
            };
            match c & 3 {
                1 => band(l, r, p.del_fg),
                2 => band(l, r, p.add_fg),
                3 => {
                    band(l, mid, p.del_fg);
                    band(mid, r, p.add_fg);
                }
                _ => {}
            }
            if c & 4 != 0 {
                band(l, l + 4.0, p.current_bg);
            }
        }
    }
    // The visible part.
    let scale = rect.height() / span.max(1.0);
    let shown = Rect::from_x_y_ranges(
        rect.x_range(),
        rect.top() + v.offset * scale
            ..=rect.top() + ((v.offset + v.view_h) * scale).min(rect.height()),
    );
    painter.rect_stroke(shown, 1.0, (1.0, p.weak), egui::StrokeKind::Inside);
    let resp = ui
        .interact(rect, Id::unique((id, "minimap")), Sense::click_and_drag())
        .on_hover_text(t!("Map of changes: click to jump"));
    if (resp.clicked() || resp.dragged())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let y = (pos.y - rect.top()) / scale - v.view_h / 2.0;
        return Some(y.clamp(0.0, (v.total_h - v.view_h).max(0.0)));
    }
    None
}

/// Selects every changed line between two positions.
fn select_range(file: &FileDiff, sel: &mut LineSelection, a: (usize, usize), b: (usize, usize)) {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    for (h, hunk) in file.hunks.iter().enumerate() {
        for (l, line) in hunk.lines.iter().enumerate() {
            if (h, l) >= start && (h, l) <= end && line.kind != LineKind::Context {
                sel.lines.insert((h, l));
            }
        }
    }
}
