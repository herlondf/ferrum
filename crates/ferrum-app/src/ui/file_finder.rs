//! "File history…" from the command palette: pick a tracked file or folder
//! by fuzzy name and open its history.

use eframe::egui::{self, Key};

use crate::command::{Command, Commands};
use crate::tab::RepoTab;
use crate::ui::blame::Load;

/// Results listed at most.
const MAX_RESULTS: usize = 200;

#[derive(Default)]
pub struct Finder {
    query: String,
    selected: usize,
    focus: bool,
    /// Indexes into the candidates matching `query`, best first; `None` when
    /// they must be recomputed.
    matches: Option<Vec<usize>>,
}

impl Finder {
    pub fn set_query(&mut self, q: &str) {
        self.query = q.to_owned();
        self.matches = None;
    }
}

/// Opens the finder, reloading the tracked files in the background (the
/// previous list is used meanwhile).
pub fn open(tab: &mut RepoTab) {
    tab.file_tools.finder = Some(Finder {
        focus: true,
        ..Default::default()
    });
    if !matches!(tab.file_tools.tracked, Load::Pending(_)) {
        tab.file_tools
            .tracked
            .start(&tab.repo, |r| r.tracked_files().map_err(|e| e.to_string()));
    }
}

/// Loads the tracked files for the command palette, unless already there.
pub fn preload(tab: &mut RepoTab) {
    let tools = &mut tab.file_tools;
    if tools.candidates.is_empty() && matches!(tools.tracked, Load::Idle) {
        tools
            .tracked
            .start(&tab.repo, |r| r.tracked_files().map_err(|e| e.to_string()));
    }
}

/// Moves freshly loaded tracked files into the candidates. Returns true then.
pub fn poll_candidates(ctx: &egui::Context, tab: &mut RepoTab) -> bool {
    let tools = &mut tab.file_tools;
    tools.tracked.poll(ctx);
    if matches!(tools.tracked, Load::Done(Ok(_)))
        && let Load::Done(Ok(list)) = std::mem::take(&mut tools.tracked)
    {
        tools.candidates = candidates(&list);
        return true;
    }
    false
}

/// The tracked files and folders loaded last (for the palette).
pub fn cached(tab: &RepoTab) -> &[String] {
    &tab.file_tools.candidates
}

/// Tracked files and their folders (folders end with `/`).
fn candidates(files: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(files.len() + files.len() / 4);
    let mut folders = std::collections::BTreeSet::new();
    for f in files {
        let mut end = 0;
        while let Some(i) = f[end..].find('/') {
            end += i + 1;
            folders.insert(&f[..end]);
        }
    }
    out.extend(folders.into_iter().map(str::to_owned));
    out.extend(files.iter().cloned());
    out
}

/// How well `path` matches `query` (lowercase): characters in order, with
/// a bonus for matches in the file name and for contiguous matches.
pub(crate) fn score(path: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let lower = path.to_lowercase();
    let trimmed = lower.trim_end_matches('/');
    let name_start = trimmed.rfind('/').map_or(0, |i| i + 1);
    let name = &trimmed[name_start..];
    if let Some(pos) = name.find(query) {
        return Some(3000 - pos as i32 * 10 - name.len() as i32);
    }
    if let Some(pos) = lower.find(query) {
        return Some(2000 - pos as i32 - lower.len() as i32);
    }
    let mut score = 1000 - lower.len() as i32;
    let mut chars = lower.char_indices();
    let mut last = None;
    for q in query.chars() {
        let (i, _) = chars.find(|&(_, c)| c == q)?;
        if last.is_some_and(|l| l + 1 == i) {
            score += 5;
        }
        if i >= name_start {
            score += 3;
        }
        last = Some(i);
    }
    Some(score)
}

pub fn show(ctx: &egui::Context, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some(mut f) = tab.file_tools.finder.take() else {
        return;
    };
    if poll_candidates(ctx, tab) {
        f.matches = None;
    }
    let tools = &mut tab.file_tools;
    let loaded = !matches!(tools.tracked, Load::Pending(_));
    let error = match &tools.tracked {
        Load::Done(Err(e)) => Some(e.clone()),
        _ => None,
    };
    let all = &tools.candidates;
    let matches = f.matches.get_or_insert_with(|| {
        let q = f.query.trim().to_lowercase();
        let mut scored: Vec<(i32, usize)> = all
            .iter()
            .enumerate()
            .filter_map(|(i, p)| score(p, &q).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| all[a.1].cmp(&all[b.1])));
        scored.truncate(MAX_RESULTS);
        scored.into_iter().map(|(_, i)| i).collect()
    });
    let matches: Vec<&String> = matches.iter().map(|&i| &all[i]).collect();
    f.selected = f.selected.min(matches.len().saturating_sub(1));

    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
            i.key_pressed(Key::Escape),
        )
    });
    if up {
        f.selected = f.selected.saturating_sub(1);
    }
    if down && f.selected + 1 < matches.len() {
        f.selected += 1;
    }
    let mut run = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("file-finder"))
        .fixed_pos(egui::pos2(screen.center().x - 300.0, screen.top() + 80.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(600.0);
                ui.horizontal(|ui| {
                    ui.strong(t!("File history"));
                    if !loaded {
                        ui.spinner();
                    }
                });
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .hint_text(t!("Type a file or folder name…"))
                        .desired_width(f32::INFINITY),
                );
                if f.focus {
                    resp.request_focus();
                    f.focus = false;
                }
                if resp.changed() {
                    f.selected = 0;
                    f.matches = None;
                }
                ui.separator();
                if let Some(e) = &error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let width = ui.available_width() - 12.0;
                        for (i, path) in matches.iter().enumerate() {
                            let (rect, resp) = ui
                                .allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
                            let selected = i == f.selected;
                            let visuals = ui.visuals();
                            if selected {
                                ui.painter()
                                    .rect_filled(rect, 4.0, visuals.selection.bg_fill);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    visuals.widgets.hovered.weak_bg_fill,
                                );
                            }
                            let color = if selected {
                                visuals.selection.stroke.color
                            } else {
                                visuals.text_color()
                            };
                            let icon = if path.ends_with('/') { "📁" } else { "📄" };
                            ui.painter().with_clip_rect(rect).text(
                                rect.left_center() + egui::vec2(8.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                format!("{icon} {path}"),
                                egui::FontId::proportional(13.5),
                                color,
                            );
                            if selected && (up || down) {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() {
                                run = Some(i);
                            }
                        }
                        if matches.is_empty() && loaded {
                            ui.weak(t!("No matching file."));
                        }
                    });
            });
        });
    if enter && !matches.is_empty() {
        run = Some(f.selected);
    }
    if let Some(i) = run {
        cmds.push(Command::FileHistory(matches[i].clone()));
        return;
    }
    if !esc {
        tab.file_tools.finder = Some(f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_and_files() {
        let files = ["src/ui/a.rs".to_owned(), "README.md".to_owned()];
        assert_eq!(
            candidates(&files),
            ["src/", "src/ui/", "src/ui/a.rs", "README.md"]
        );
    }

    #[test]
    fn prefers_file_name_matches() {
        assert!(score("src/blame.rs", "blame") > score("blame/other.rs", "blame"));
        assert!(score("src/ui/blame.rs", "blm").is_some());
        assert!(score("src/ui/blame.rs", "xyz").is_none());
        assert!(score("a/b/main.rs", "main") > score("a/main_long_name.rs", "main"));
        assert_eq!(score("anything", ""), Some(0));
    }
}
