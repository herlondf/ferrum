//! History of a range of lines (`git log -L`), opened from the blame or from
//! a line of a diff ("History of this line").

use eframe::egui::{self, Align2, FontId, Sense, pos2, vec2};
use ferrum_core::{DiffSource, FileDiff, LineCommit, LineKind, Repo};

use crate::command::{Command, Commands};
use crate::tab::{LineSelection, RepoTab, View};
use crate::ui::blame::{Load, Mode, loading_or_error};
use crate::ui::diff_view;
use crate::util::format_date;

const ROW: f32 = 42.0;

pub struct LinesState {
    pub path: String,
    pub start: u32,
    pub end: u32,
    /// Revision the lines are counted in.
    pub rev: String,
    data: Load<Vec<LineCommit>>,
    selected: usize,
}

impl LinesState {
    pub fn new(path: String, start: u32, end: u32, rev: String) -> Self {
        LinesState {
            path,
            start: start.min(end),
            end: end.max(start),
            rev,
            data: Load::Idle,
            selected: 0,
        }
    }

    pub fn reload(&mut self) {
        self.data = Load::Idle;
    }

    /// Commits listed, once loaded.
    #[cfg(test)]
    pub fn count(&self) -> Option<usize> {
        self.data.ok().map(Vec::len)
    }
}

/// Mode button label: "Lines 3–7".
pub fn label(st: &LinesState) -> String {
    if st.start == st.end {
        t!("Line {}", st.start)
    } else {
        t!("Lines {}–{}", st.start, st.end)
    }
}

pub fn show(ui: &mut egui::Ui, repo: &Repo, st: &mut LinesState, cmds: &mut Commands, id: &str) {
    if st.data.is_idle() {
        let (path, a, b, rev) = (st.path.clone(), st.start, st.end, st.rev.clone());
        st.data.start(repo, move |r| {
            r.line_history(&path, a, b, &rev).map_err(|e| e.to_string())
        });
    }
    let short_rev = if st.rev.len() == 40 {
        st.rev[..7].to_owned()
    } else {
        st.rev.clone()
    };
    ui.weak(t!(
        "Commits that changed {} of {} (as of {})",
        label(st).to_lowercase(),
        st.path,
        short_rev
    ));
    if loading_or_error(ui, st.data.poll(ui.ctx())) {
        return;
    }
    let Some(list) = st.data.ok() else {
        return;
    };
    if list.is_empty() {
        ui.weak(t!("No commits changed these lines."));
        return;
    }
    let mut clicked = None;
    egui::Panel::left(("line-history-list", id))
        .resizable(true)
        .default_size(380.0)
        .show(ui, |ui| {
            ui.weak(if list.len() == 1 {
                t!("1 commit")
            } else {
                t!("{} commits", list.len())
            });
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .id_salt(("line-history-rows", id))
                .auto_shrink(false)
                .show_rows(ui, ROW, list.len(), |ui, range| {
                    for i in range {
                        let c = &list[i].commit;
                        let (rect, resp) =
                            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                        let visuals = ui.visuals();
                        let selected = i == st.selected;
                        if selected {
                            ui.painter()
                                .rect_filled(rect, 0.0, visuals.selection.bg_fill);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                visuals.widgets.hovered.weak_bg_fill,
                            );
                        }
                        let (text, weak) = if selected {
                            let c = visuals.selection.stroke.color;
                            (c, c)
                        } else {
                            (visuals.text_color(), visuals.weak_text_color())
                        };
                        let inner = rect.shrink2(vec2(8.0, 0.0));
                        let p = ui.painter().with_clip_rect(inner);
                        p.text(
                            pos2(inner.left(), rect.top() + 13.0),
                            Align2::LEFT_CENTER,
                            &c.summary,
                            FontId::proportional(13.0),
                            text,
                        );
                        p.text(
                            pos2(inner.left(), rect.bottom() - 12.0),
                            Align2::LEFT_CENTER,
                            format!(
                                "{} · {} · {}",
                                c.short_id(),
                                c.author,
                                format_date(c.time, c.offset_minutes)
                            ),
                            FontId::proportional(11.5),
                            weak,
                        );
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        resp.context_menu(|ui| {
                            if ui.button(t!("Show in history")).clicked() {
                                cmds.push(Command::SelectCommit(c.id));
                                ui.close();
                            }
                            if ui.button(t!("Copy hash")).clicked() {
                                cmds.push(Command::Copy(c.id.to_string()));
                                ui.close();
                            }
                        });
                    }
                });
        });
    if let Some(i) = clicked {
        st.selected = i;
    }
    let Some(entry) = list.get(st.selected) else {
        return;
    };
    egui::CentralPanel::default_margins().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong(&entry.commit.summary);
            ui.weak(entry.commit.short_id());
        });
        match &entry.diff {
            Some(f) => {
                diff_view::show(ui, id, f, diff_view::DiffMode::ReadOnly, false, None);
            }
            None => {
                ui.weak(t!("Git showed no changes for this commit."));
            }
        }
    });
}

// ----- "History of this line" in diff views -----

/// A request from a diff view's line menu, handled by `take_request`.
#[derive(Clone)]
struct Request {
    view: String,
    path: String,
    start: u32,
    end: u32,
    /// The numbers are of the old side of the diff.
    old_side: bool,
}

fn request_id() -> egui::Id {
    egui::Id::unique("ferrum-line-history-request")
}

/// The lines a diff menu acts on: the selection if `at` is in it, else the
/// line itself. Returns 1-based numbers and whether they are old-side numbers.
fn diff_range(
    file: &FileDiff,
    selection: Option<&LineSelection>,
    at: (usize, usize),
    old_first: bool,
) -> Option<(u32, u32, bool)> {
    let picked: Vec<(usize, usize)> = match selection {
        Some(s) if s.lines.contains(&at) => s.lines.iter().copied().collect(),
        _ => vec![at],
    };
    let lines: Vec<_> = picked
        .iter()
        .filter_map(|&(h, l)| file.hunks.get(h)?.lines.get(l))
        .collect();
    let side = |old: bool| {
        let nos: Vec<u32> = lines
            .iter()
            .filter_map(|l| if old { l.old_no } else { l.new_no })
            .collect();
        Some((*nos.iter().min()?, *nos.iter().max()?, old))
    };
    let only_removed = lines.iter().all(|l| l.kind == LineKind::Removed);
    if old_first || only_removed {
        side(true).or_else(|| (!old_first).then(|| side(false)).flatten())
    } else {
        side(false).or_else(|| side(true))
    }
}

/// Adds "History of this line" to the context menu of a diff line.
pub fn diff_line_menu(
    resp: &egui::Response,
    view: &str,
    file: &FileDiff,
    selection: Option<&LineSelection>,
    at: (usize, usize),
) {
    // Diffs of a line history are already one.
    if view.starts_with("line-history-diff") {
        return;
    }
    resp.context_menu(|ui| {
        // Working copy lines are counted in the committed version (the old side).
        let working = view.starts_with("changes-diff");
        let range = diff_range(file, selection, at, working);
        let text = match range {
            Some((a, b, _)) if a != b => t!("History of lines {}–{}", a, b),
            _ => t!("History of this line"),
        };
        let resp = ui
            .add_enabled(range.is_some(), egui::Button::new(text))
            .on_disabled_hover_text(t!("These lines are not committed yet."));
        if resp.clicked()
            && let Some((start, end, old_side)) = range
        {
            request(ui.ctx(), view, &file.path, (start, end), old_side);
            ui.close();
        }
    });
}

/// Asks for the history of lines `start..=end` of `path` shown in the diff
/// view `view` (handled after the frame by `take_request`).
pub(crate) fn request(
    ctx: &egui::Context,
    view: &str,
    path: &str,
    (start, end): (u32, u32),
    old_side: bool,
) {
    let req = Request {
        view: view.to_owned(),
        path: path.to_owned(),
        start,
        end,
        old_side,
    };
    ctx.data_mut(|d| d.insert_temp(request_id(), req));
}

/// Opens the line history asked for by a diff view, if any.
pub fn take_request(ctx: &egui::Context, tab: &mut RepoTab) {
    let Some(req) = ctx.data(|d| d.get_temp::<Request>(request_id())) else {
        return;
    };
    ctx.data_mut(|d| d.remove::<Request>(request_id()));
    let view = req
        .view
        .strip_suffix(diff_view::LARGE_SUFFIX)
        .unwrap_or(&req.view);
    // The revision the line numbers belong to.
    let new_rev = match view {
        "commit-diff" => tab
            .commit_view
            .as_ref()
            .map(|c| c.details.info.id.to_string()),
        "changes-diff" => match tab.changes.selected.as_ref().map(|s| s.0) {
            Some(DiffSource::Commit(id)) => Some(id.to_string()),
            _ => Some("HEAD".to_owned()),
        },
        _ => crate::ui::file_window::diff_key_of(tab, view).map(|k| k.new.to_string()),
    };
    let Some(new_rev) = new_rev else {
        return;
    };
    let rev = if req.old_side && view != "changes-diff" {
        format!("{new_rev}^")
    } else {
        new_rev
    };
    let (path, start, end) = (req.path, req.start, req.end);
    crate::ui::blame::open_mode(tab, path.clone(), Mode::Lines);
    if let Some(v) = &mut tab.file_view {
        v.lines = Some(LinesState::new(path, start, end, rev));
        v.mode = Mode::Lines;
    }
    tab.view = View::File;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrum_core::{ChangeKind, DiffLine, Hunk};

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            text: String::new(),
            old_no: old,
            new_no: new,
            no_newline: false,
        }
    }

    #[test]
    fn picks_the_line_numbers_of_a_diff_selection() {
        let file = FileDiff {
            path: "f".into(),
            old_path: None,
            kind: ChangeKind::Modified,
            binary: false,
            old_id: None,
            new_id: None,
            hunks: vec![Hunk {
                header: String::new(),
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 3,
                lines: vec![
                    line(LineKind::Context, Some(1), Some(1)),
                    line(LineKind::Removed, Some(2), None),
                    line(LineKind::Added, None, Some(2)),
                    line(LineKind::Added, None, Some(3)),
                ],
            }],
            additions: 2,
            deletions: 1,
            old_mode: 0,
            new_mode: 0,
            skipped: None,
        };
        assert_eq!(diff_range(&file, None, (0, 3), false), Some((3, 3, false)));
        // A removed line only exists on the old side.
        assert_eq!(diff_range(&file, None, (0, 1), false), Some((2, 2, true)));
        let mut sel = LineSelection::default();
        sel.lines.extend([(0, 2), (0, 3)]);
        assert_eq!(
            diff_range(&file, Some(&sel), (0, 2), false),
            Some((2, 3, false))
        );
        // Outside the selection: just the clicked line.
        assert_eq!(
            diff_range(&file, Some(&sel), (0, 0), false),
            Some((1, 1, false))
        );
        // Working copy: added lines are not committed yet.
        assert_eq!(diff_range(&file, Some(&sel), (0, 2), true), None);
        assert_eq!(diff_range(&file, None, (0, 0), true), Some((1, 1, true)));
    }
}
