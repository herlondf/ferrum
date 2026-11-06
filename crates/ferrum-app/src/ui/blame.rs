//! File history (commits that touched a file, with their diff) and blame.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use eframe::egui::{self, Align2, FontId, Rect, Sense, pos2, vec2};
use ferrum_core::{DiffSource, FileCommit, FileDiff, Oid, Repo, short};

use crate::command::{Command, Commands};
use crate::tab::{RepoTab, View};
use crate::ui::{diff_extra, diff_view};
use crate::util::{format_date, format_day};

const HISTORY_ROW: f32 = 42.0;
const BLAME_ROW: f32 = 18.0;
const HASH_W: f32 = 62.0;
const AUTHOR_W: f32 = 140.0;
const DATE_W: f32 = 82.0;
const GUTTER_W: f32 = HASH_W + AUTHOR_W + DATE_W + 12.0;
const NUMBER_W: f32 = 52.0;

/// Data loaded on a background thread, with its own repository handle.
enum Load<T> {
    Idle,
    Pending(Receiver<Result<T, String>>),
    Done(Result<T, String>),
}

impl<T: Send + 'static> Load<T> {
    fn start(
        &mut self,
        repo: &Repo,
        work: impl FnOnce(&Repo) -> Result<T, String> + Send + 'static,
    ) {
        let path = repo.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = Repo::open(&path)
                .map_err(|e| e.to_string())
                .and_then(|r| work(&r));
            let _ = tx.send(result);
        });
        *self = Load::Pending(rx);
    }

    /// The result once loaded; keeps the UI repainting while waiting.
    fn poll(&mut self, ctx: &egui::Context) -> Option<&Result<T, String>> {
        if let Load::Pending(rx) = self {
            match rx.try_recv() {
                Ok(r) => *self = Load::Done(r),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50))
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    *self = Load::Done(Err(t!("Loading stopped unexpectedly.")))
                }
            }
        }
        match self {
            Load::Done(r) => Some(r),
            _ => None,
        }
    }
}

struct BlameRow {
    commit: Oid,
    block_start: bool,
    /// Alternates between blocks, for the background.
    odd_block: bool,
    /// Shown only on the first line of a block.
    gutter: Option<(String, String)>,
    text: String,
}

struct BlameData {
    rows: Vec<BlameRow>,
    /// Summary and full date of each commit, for tooltips.
    commits: HashMap<Oid, (String, String)>,
    /// Longest line in characters, for horizontal scrolling.
    longest: usize,
}

pub struct FileView {
    pub path: String,
    pub blame: bool,
    history: Load<Vec<FileCommit>>,
    blame_data: Load<BlameData>,
    selected: Option<usize>,
    /// Diff of the selected history entry, for the commit it belongs to.
    diff: Load<Option<FileDiff>>,
    diff_commit: Option<Oid>,
    /// Commit under the pointer in the blame gutter (from the last frame).
    hovered: Option<Oid>,
}

pub fn open(tab: &mut RepoTab, path: String, blame: bool) {
    match &mut tab.file_view {
        Some(v) if v.path == path => v.blame = blame,
        _ => {
            tab.file_view = Some(FileView {
                path,
                blame,
                history: Load::Idle,
                blame_data: Load::Idle,
                selected: None,
                diff: Load::Idle,
                diff_commit: None,
                hovered: None,
            })
        }
    }
    tab.view = View::File;
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some(view) = &mut tab.file_view else {
        return;
    };
    // Start loading what the current mode needs (once).
    if view.blame && matches!(view.blame_data, Load::Idle) {
        let path = view.path.clone();
        view.blame_data
            .start(&tab.repo, move |r| load_blame(r, &path));
    }
    if !view.blame && matches!(view.history, Load::Idle) {
        let path = view.path.clone();
        view.history.start(&tab.repo, move |r| {
            r.file_history(&path).map_err(|e| e.to_string())
        });
    }

    let dirty = [&tab.status.unstaged, &tab.status.staged]
        .iter()
        .any(|list| list.iter().any(|f| f.path == view.path));

    ui.horizontal(|ui| {
        if ui
            .button(t!("⬅ Back"))
            .on_hover_text(t!("Back to the history"))
            .clicked()
        {
            cmds.push(Command::SetView(View::History));
        }
        ui.separator();
        if ui
            .add(egui::Button::selectable(!view.blame, t!("History")))
            .clicked()
        {
            view.blame = false;
        }
        if ui
            .add(egui::Button::selectable(view.blame, "Blame"))
            .clicked()
        {
            view.blame = true;
        }
        ui.separator();
        ui.add(egui::Label::new(egui::RichText::new(&view.path).strong()).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("↻").on_hover_text(t!("Reload")).clicked() {
                if view.blame {
                    view.blame_data = Load::Idle;
                } else {
                    view.history = Load::Idle;
                    view.diff = Load::Idle;
                    view.diff_commit = None;
                }
            }
            if ui.small_button(t!("Copy path")).clicked() {
                cmds.push(Command::Copy(view.path.clone()));
            }
        });
    });
    ui.separator();

    if view.blame {
        if dirty && matches!(view.blame_data, Load::Done(Ok(_))) {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("⚠ This file has uncommitted changes. Blame shows the last committed version."),
            );
        }
        blame_view(ui, view, cmds);
    } else {
        history_view(ui, &tab.repo, view, cmds);
    }
}

fn loading_or_error<T>(ui: &mut egui::Ui, result: Option<&Result<T, String>>) -> bool {
    match result {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(t!("Loading…"));
            });
            true
        }
        Some(Err(e)) => {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.colored_label(ui.visuals().error_fg_color, e));
            true
        }
        Some(Ok(_)) => false,
    }
}

// ---- History ----

fn history_view(ui: &mut egui::Ui, repo: &Repo, view: &mut FileView, cmds: &mut Commands) {
    let ctx = ui.ctx().clone();
    if loading_or_error(ui, view.history.poll(&ctx)) {
        return;
    }
    let Load::Done(Ok(entries)) = &view.history else {
        return;
    };
    if entries.is_empty() {
        ui.weak(t!("No commits touched this file."));
        return;
    }
    if view.selected.is_none() {
        view.selected = Some(0);
    }

    // Load the diff of the selected entry when the selection changes.
    let entry = view.selected.and_then(|i| entries.get(i));
    if let Some(e) = entry
        && view.diff_commit != Some(e.commit.id)
    {
        let (id, path, old) = (e.commit.id, e.path.clone(), e.old_path.clone());
        view.diff_commit = Some(id);
        view.diff.start(repo, move |r| {
            let file = r
                .commit_file_diff(id, &path, old.as_deref())
                .map_err(|e| e.to_string())?;
            Ok(find_file(file.into_iter().collect(), &path, old.as_deref()))
        });
    }

    let mut clicked = None;
    egui::Panel::left("file-history-list")
        .resizable(true)
        .default_size(380.0)
        .show(ui, |ui| {
            ui.weak(if entries.len() == 1 {
                t!("1 commit")
            } else {
                t!("{} commits", entries.len())
            });
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt("file-history-rows")
                .auto_shrink(false)
                .show_rows(ui, HISTORY_ROW, entries.len(), |ui, range| {
                    for i in range {
                        let e = &entries[i];
                        let (rect, resp) = ui.allocate_exact_size(
                            vec2(ui.available_width(), HISTORY_ROW),
                            Sense::click(),
                        );
                        history_row(
                            ui,
                            rect,
                            e,
                            &view.path,
                            view.selected == Some(i),
                            resp.hovered(),
                        );
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        resp.context_menu(|ui| {
                            if ui.button(t!("Show in history")).clicked() {
                                cmds.push(Command::SelectCommit(e.commit.id));
                                ui.close();
                            }
                            if ui.button(t!("Copy hash")).clicked() {
                                cmds.push(Command::Copy(e.commit.id.to_string()));
                                ui.close();
                            }
                        });
                    }
                });
        });
    if clicked.is_some() {
        view.selected = clicked;
    }

    egui::CentralPanel::default_margins().show(ui, |ui| {
        let Some(e) = view.selected.and_then(|i| entries.get(i)) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.strong(&e.commit.summary);
            ui.weak(e.commit.short_id());
        });
        if let Some(old) = &e.old_path {
            ui.weak(t!("Renamed from {}", old));
        } else if e.path != view.path {
            ui.weak(t!("Named {} in this commit", e.path));
        }
        if loading_or_error(ui, view.diff.poll(&ctx)) {
            return;
        }
        let Load::Done(Ok(file)) = &view.diff else {
            return;
        };
        match file {
            None => {
                let note = t!("This commit does not change the file (for example, a merge).");
                ui.weak(note);
            }
            Some(f) => {
                diff_extra::show_file(
                    ui,
                    "file-history-diff",
                    repo,
                    DiffSource::Commit(e.commit.id),
                    f,
                    diff_view::DiffMode::ReadOnly,
                    false,
                    None,
                );
            }
        }
    });
}

/// The file of a commit's diff that is `path` (or was `old_path`) in that commit.
fn find_file(files: Vec<FileDiff>, path: &str, old_path: Option<&str>) -> Option<FileDiff> {
    let pos = files
        .iter()
        .position(|f| f.path == path)
        .or_else(|| {
            files
                .iter()
                .position(|f| old_path.is_some_and(|o| f.path == o))
        })
        .or_else(|| {
            files
                .iter()
                .position(|f| f.old_path.as_deref() == Some(path))
        })?;
    files.into_iter().nth(pos)
}

fn history_row(
    ui: &egui::Ui,
    rect: Rect,
    e: &FileCommit,
    current_path: &str,
    selected: bool,
    hovered: bool,
) {
    let visuals = ui.visuals();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 0.0, visuals.selection.bg_fill);
    } else if hovered {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let (text, weak) = if selected {
        let c = visuals.selection.stroke.color;
        (c, c)
    } else {
        (visuals.text_color(), visuals.weak_text_color())
    };
    let inner = rect.shrink2(vec2(8.0, 0.0));
    let clip = painter.with_clip_rect(inner);
    let top = rect.top() + 13.0;
    let bottom = rect.bottom() - 12.0;
    let hash_w = 60.0;
    clip.with_clip_rect(Rect::from_x_y_ranges(
        inner.left()..=inner.right() - hash_w,
        inner.y_range(),
    ))
    .text(
        pos2(inner.left(), top),
        Align2::LEFT_CENTER,
        &e.commit.summary,
        FontId::proportional(13.0),
        text,
    );
    clip.text(
        pos2(inner.right(), top),
        Align2::RIGHT_CENTER,
        e.commit.short_id(),
        FontId::monospace(11.5),
        weak,
    );
    let mut meta = format!(
        "{} · {}",
        e.commit.author,
        format_date(e.commit.time, e.commit.offset_minutes)
    );
    if e.path != current_path {
        meta.push_str(&format!(" · {}", e.path));
    }
    clip.text(
        pos2(inner.left(), bottom),
        Align2::LEFT_CENTER,
        meta,
        FontId::proportional(11.5),
        weak,
    );
}

// ---- Blame ----

fn load_blame(repo: &Repo, path: &str) -> Result<BlameData, String> {
    let lines = repo.blame(path).map_err(|e| e.to_string())?;
    let mut commits = HashMap::new();
    for l in &lines {
        commits.entry(l.commit).or_insert_with(|| {
            let summary = repo
                .find_commit_info(l.commit)
                .map(|c| c.summary)
                .unwrap_or_default();
            (summary, format_date(l.time, l.offset_minutes))
        });
    }
    let odd = odd_blocks(lines.iter().map(|l| l.block_start));
    let rows: Vec<BlameRow> = lines
        .into_iter()
        .zip(odd)
        .map(|(l, odd_block)| BlameRow {
            commit: l.commit,
            block_start: l.block_start,
            odd_block,
            gutter: l.block_start.then(|| {
                let date = format_day(l.time, l.offset_minutes);
                (l.author, date)
            }),
            text: crate::util::expand_tabs(&l.text),
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
    })
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

fn blame_view(ui: &mut egui::Ui, view: &mut FileView, cmds: &mut Commands) {
    let ctx = ui.ctx().clone();
    if loading_or_error(ui, view.blame_data.poll(&ctx)) {
        return;
    }
    let Load::Done(Ok(data)) = &view.blame_data else {
        return;
    };
    if data.rows.is_empty() {
        ui.weak(t!("The file is empty."));
        return;
    }

    let font = crate::util::code_font(12.0);
    let row_h = crate::util::code_row_height(BLAME_ROW);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'm'));
    let content_w = GUTTER_W + NUMBER_W + 16.0 + data.longest as f32 * char_w;
    let visuals = ui.visuals().clone();
    let block_bg = visuals.faint_bg_color;
    let hover_bg = visuals.selection.bg_fill.gamma_multiply(0.4);
    let gutter_bg = visuals.extreme_bg_color;
    let accent = visuals.hyperlink_color;
    let weak = visuals.weak_text_color();
    let separator = visuals.widgets.noninteractive.bg_stroke.color;
    let mut hovered = None;

    // Rows touch; show_rows must know there is no spacing between them.
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both()
        .id_salt("blame-rows")
        .auto_shrink(false)
        .show_rows(ui, row_h, data.rows.len(), |ui, range| {
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

                let painter = ui.painter();
                if row.odd_block {
                    painter.rect_filled(rect, 0.0, block_bg);
                }
                painter.rect_filled(gutter, 0.0, gutter_bg.gamma_multiply(0.6));
                // Lines from the commit under the pointer light up in the gutter.
                if view.hovered == Some(row.commit) {
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
                if let Some((summary, date)) = data.commits.get(&row.commit) {
                    resp.on_hover_ui(|ui| {
                        ui.strong(summary);
                        ui.weak(format!("{} · {date}", row.commit));
                        ui.weak(t!("Click to show in the history"));
                    });
                }
            }
        });
    view.hovered = hovered;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrum_core::ChangeKind;

    #[test]
    fn alternates_blocks() {
        let starts = [true, false, true, true, false, false];
        assert_eq!(
            odd_blocks(starts.into_iter()),
            [false, false, true, false, false, false]
        );
    }

    fn file(path: &str, old: Option<&str>) -> FileDiff {
        FileDiff {
            path: path.into(),
            old_path: old.map(Into::into),
            kind: ChangeKind::Modified,
            binary: false,
            old_id: None,
            new_id: None,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
            old_mode: 0,
            new_mode: 0,
            skipped: None,
        }
    }

    #[test]
    fn finds_the_file_in_a_commit() {
        let files = || vec![file("a", None), file("b", Some("old-b"))];
        assert_eq!(find_file(files(), "b", None).unwrap().path, "b");
        // libgit2 may name the file differently than `git log --follow`.
        assert_eq!(find_file(files(), "x", Some("a")).unwrap().path, "a");
        assert_eq!(find_file(files(), "old-b", None).unwrap().path, "b");
        assert!(find_file(files(), "zzz", None).is_none());
    }
}
