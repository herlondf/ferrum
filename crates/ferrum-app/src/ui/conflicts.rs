//! Three-way conflict resolver: pick a side (or both) for each conflict, then
//! fine-tune the result by hand before marking the file as resolved.

use std::collections::HashSet;
use std::time::SystemTime;

use eframe::egui::{self, Color32, RichText};
use ferrum_core::RepoState;
use ferrum_core::conflict::{self, Choice, Chunk};
use ferrum_core::ops::Side;

use crate::command::{Command, Commands, Op};
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;

/// Lines of unchanged text kept visible next to each conflict.
const CONTEXT: usize = 3;

#[derive(Default)]
pub struct ConflictState {
    pub path: Option<String>,
    file: Option<FileState>,
    /// Position of the file in the conflicted list, to pick the next one once it is resolved.
    index: usize,
}

/// What kind of conflict the file has, which decides how it can be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Situation {
    /// Text with (or without any more) conflict markers.
    Text,
    /// Deleted on our side, changed on theirs.
    DeletedByUs,
    /// Changed on our side, deleted on theirs.
    DeletedByThem,
    Binary,
    /// Not in the working copy.
    Missing,
}

struct FileState {
    path: String,
    situation: Situation,
    /// Modification time and size, to notice changes made outside the app.
    stamp: Option<(Option<SystemTime>, u64)>,
    changed_on_disk: bool,
    /// Base, ours and theirs from the index.
    versions: [Option<String>; 3],
    chunks: Vec<Chunk>,
    choices: Vec<Option<Choice>>,
    result: String,
    /// The result was edited by hand: choices no longer regenerate it.
    edited: bool,
    /// Common chunks shown in full.
    expanded: HashSet<usize>,
    error: Option<String>,
}

fn situation(versions: &[Option<String>; 3], disk: Option<&[u8]>) -> Situation {
    let [_, ours, theirs] = versions;
    match (ours, theirs, disk) {
        (None, Some(_), _) => Situation::DeletedByUs,
        (Some(_), None, _) => Situation::DeletedByThem,
        (_, _, None) => Situation::Missing,
        (_, _, Some(bytes)) => {
            let nul = |s: &str| s.contains('\0');
            if bytes.contains(&0) || versions.iter().flatten().any(|v| nul(v)) {
                Situation::Binary
            } else {
                Situation::Text
            }
        }
    }
}

/// The file to show once `current` is no longer conflicted: the one that took its place.
fn next_file(conflicted: &[String], current: &str, index: usize) -> Option<String> {
    let rest: Vec<&String> = conflicted.iter().filter(|p| *p != current).collect();
    rest.get(index.min(rest.len().saturating_sub(1)))
        .map(|p| (*p).clone())
}

fn stamp(tab: &RepoTab, path: &str) -> Option<(Option<SystemTime>, u64)> {
    let meta = std::fs::metadata(tab.path().join(path)).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

fn load(tab: &RepoTab, path: &str) -> FileState {
    let mut error = None;
    let versions = tab.repo.conflict_versions(path).unwrap_or_else(|e| {
        error = Some(e.to_string());
        Default::default()
    });
    let disk = tab.repo.read_workdir_file(path).ok();
    let situation = situation(&versions, disk.as_deref());
    let text = match (&disk, situation) {
        (Some(bytes), Situation::Text) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    };
    let chunks = conflict::parse(&text);
    let choices = vec![None; conflict::conflict_count(&chunks)];
    FileState {
        path: path.to_owned(),
        situation,
        stamp: stamp(tab, path),
        changed_on_disk: false,
        versions,
        result: conflict::render(&chunks, &choices),
        chunks,
        choices,
        edited: false,
        expanded: HashSet::new(),
        error,
    }
}

/// Opens the resolver for `path`.
pub fn open(tab: &mut RepoTab, path: String) {
    if let Some(i) = tab.status.conflicted.iter().position(|f| f.path == path) {
        tab.conflicts.index = i;
    }
    tab.conflicts.path = Some(path);
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    let conflicted: Vec<String> = tab
        .status
        .conflicted
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let Some(path) = tab.conflicts.path.clone() else {
        match conflicted.first() {
            Some(p) => open(tab, p.clone()),
            None => {
                ui.weak(t!("No conflicts."));
            }
        }
        return;
    };

    // Once the file is resolved (here or elsewhere), move on to the next one.
    if !tab.busy()
        && !conflicted.contains(&path)
        && let Some(next) = next_file(&conflicted, &path, tab.conflicts.index)
    {
        open(tab, next);
        return;
    }
    if let Some(i) = conflicted.iter().position(|p| *p == path) {
        tab.conflicts.index = i;
    }

    // Load the file, and reload it when it changes on disk.
    let current_stamp = stamp(tab, &path);
    let busy = tab.busy();
    match &mut tab.conflicts.file {
        Some(f) if f.path == path => {
            if f.stamp != current_stamp && !busy {
                if f.edited {
                    f.changed_on_disk = true;
                    f.stamp = current_stamp;
                } else {
                    tab.conflicts.file = Some(load(tab, &path));
                }
            }
        }
        _ => tab.conflicts.file = Some(load(tab, &path)),
    }

    top_bar(ui, tab, cmds, &conflicted, &path);
    ui.separator();

    let in_rebase = tab.state == RepoState::Rebase;
    let Some(file) = tab.conflicts.file.as_mut() else {
        return;
    };
    if !conflicted.contains(&path) {
        ui.label(t!("{} has no conflicts.", path));
        return;
    }
    if let Some(e) = &file.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    let (ours_label, theirs_label) = side_labels(file);
    if in_rebase {
        ui.weak(t!("During a rebase, {} is the branch being rebased onto and {} is your commit being replayed.", ours_label, theirs_label));
    }

    match file.situation {
        Situation::Text => {}
        Situation::DeletedByUs | Situation::DeletedByThem => {
            deleted_view(ui, file, &path, busy, cmds, &ours_label, &theirs_label);
            return;
        }
        Situation::Binary | Situation::Missing => {
            ui.add_space(12.0);
            ui.label(if file.situation == Situation::Binary {
                t!("This is a binary file, so it cannot be merged line by line.")
            } else {
                t!("This file is not in the working copy.")
            });
            ui.label(t!("Resolve it by keeping one version of the whole file:"));
            ui.add_enabled_ui(!busy, |ui| whole_file_buttons(ui, &path, cmds));
            return;
        }
    }

    egui::Panel::bottom("conflict-result")
        .resizable(true)
        .default_size(ui.available_height() * 0.42)
        .min_size(140.0)
        .show(ui, |ui| {
            result_panel(ui, file, &path, busy, cmds, &conflicted)
        });
    egui::CentralPanel::default_margins().show(ui, |ui| {
        if file.choices.is_empty() {
            ui.add_space(12.0);
            ui.label(t!("No conflict markers are left in this file."));
            ui.weak(t!("Check the result below and mark the file as resolved."));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("conflicts", &path))
            .auto_shrink([false, false])
            .show(ui, |ui| chunks_view(ui, file));
    });
}

fn top_bar(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    conflicted: &[String],
    path: &str,
) {
    ui.horizontal(|ui| {
        if ui
            .button(t!("⬅ Local Changes"))
            .on_hover_text(t!("Back to the list of local changes"))
            .clicked()
        {
            cmds.push(Command::SetView(View::Changes));
        }
        ui.separator();
        let pos = conflicted.iter().position(|p| p == path);
        let counter = match pos {
            Some(i) => t!("{} of {}", i + 1, conflicted.len()),
            None if conflicted.len() == 1 => t!("1 left"),
            None => t!("{} left", conflicted.len()),
        };
        egui::ComboBox::from_id_salt("conflict-file")
            .selected_text(path)
            .width(280.0)
            .truncate()
            .show_ui(ui, |ui| {
                for p in conflicted {
                    if ui.selectable_label(p == path, p).clicked() && p != path {
                        cmds.push(Command::ResolveConflict(p.clone()));
                    }
                }
            });
        ui.weak(counter);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!tab.busy() && pos.is_some(), |ui| {
                if ui
                    .button(t!("Use theirs for whole file"))
                    .on_hover_text(t!("Take their version of the file and mark it resolved"))
                    .clicked()
                {
                    resolve_with(cmds, path, Side::Theirs);
                }
                if ui
                    .button(t!("Use ours for whole file"))
                    .on_hover_text(t!("Take our version of the file and mark it resolved"))
                    .clicked()
                {
                    resolve_with(cmds, path, Side::Ours);
                }
                crate::ui::tools::merge_tool_button(ui, tab.path(), path, cmds);
            });
        });
    });
}

fn resolve_with(cmds: &mut Commands, path: &str, side: Side) {
    cmds.run(Op::ResolveWith {
        path: path.to_owned(),
        side,
    });
}

fn whole_file_buttons(ui: &mut egui::Ui, path: &str, cmds: &mut Commands) {
    ui.horizontal(|ui| {
        if ui.button(t!("Use ours")).clicked() {
            resolve_with(cmds, path, Side::Ours);
        }
        if ui.button(t!("Use theirs")).clicked() {
            resolve_with(cmds, path, Side::Theirs);
        }
    });
}

/// Names of both sides, with the labels of the first conflict's markers: "ours (HEAD)".
fn side_labels(file: &FileState) -> (String, String) {
    let first = file.chunks.iter().find_map(|c| match c {
        Chunk::Conflict(c) => Some(c),
        _ => None,
    });
    let pick = |s: Option<&String>, side: String| match s.filter(|s| !s.is_empty()) {
        Some(label) => format!("{side} ({label})"),
        None => side,
    };
    (
        pick(first.map(|c| &c.ours_label), t!("ours")),
        pick(first.map(|c| &c.theirs_label), t!("theirs")),
    )
}

fn deleted_view(
    ui: &mut egui::Ui,
    file: &FileState,
    path: &str,
    busy: bool,
    cmds: &mut Commands,
    ours_label: &str,
    theirs_label: &str,
) {
    let by_us = file.situation == Situation::DeletedByUs;
    let (deleter, changer) = if by_us {
        (ours_label, theirs_label)
    } else {
        (theirs_label, ours_label)
    };
    ui.add_space(12.0);
    ui.label(t!(
        "This file was deleted on {} and changed on {}.",
        deleter,
        changer
    ));
    ui.label(t!("Keep the changed file, or delete it:"));
    ui.add_enabled_ui(!busy, |ui| {
        ui.horizontal(|ui| {
            let (keep, delete) = if by_us {
                (Side::Theirs, Side::Ours)
            } else {
                (Side::Ours, Side::Theirs)
            };
            if ui.button(t!("Keep the file")).clicked() {
                resolve_with(cmds, path, keep);
            }
            if ui.button(t!("Delete the file")).clicked() {
                resolve_with(cmds, path, delete);
            }
        });
    });
    let kept = if by_us {
        &file.versions[2]
    } else {
        &file.versions[1]
    };
    if let Some(text) = kept {
        ui.add_space(8.0);
        ui.strong(t!("Changed version ({})", changer));
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).monospace()).extend());
            });
    }
}

fn ours_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0x5a, 0xa9, 0xe6)
    } else {
        Color32::from_rgb(0x1f, 0x6f, 0xb5)
    }
}

fn theirs_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xc5, 0x86, 0xe0)
    } else {
        Color32::from_rgb(0x8a, 0x3f, 0xb0)
    }
}

fn chunks_view(ui: &mut egui::Ui, file: &mut FileState) {
    let last = file.chunks.len().saturating_sub(1);
    let total = file.choices.len();
    let mut n = 0;
    let mut changed = false;
    for (i, chunk) in file.chunks.iter().enumerate() {
        match chunk {
            Chunk::Common(text) => {
                let expanded = file.expanded.contains(&i);
                if common_text(ui, text, i == 0, i == last, expanded) {
                    if expanded {
                        file.expanded.remove(&i);
                    } else {
                        file.expanded.insert(i);
                    }
                }
            }
            Chunk::Conflict(c) => {
                let choice = &mut file.choices[n];
                changed |= conflict_view(ui, c, n, total, choice);
                n += 1;
            }
        }
    }
    if changed && !file.edited {
        file.result = conflict::render(&file.chunks, &file.choices);
    }
}

/// Shows unchanged text dimmed, keeping only a few lines next to each conflict.
/// Returns true when the hidden part is clicked.
fn common_text(ui: &mut egui::Ui, text: &str, first: bool, last: bool, expanded: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let head = if first { 0 } else { CONTEXT };
    let tail = if last { 0 } else { CONTEXT };
    let weak = ui.visuals().weak_text_color();
    // Indented like the text inside the conflict boxes.
    let mono = |ui: &mut egui::Ui, s: &[&str]| {
        if !s.is_empty() {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(15, 0))
                .show(ui, |ui| {
                    let text = RichText::new(s.join("\n")).monospace().color(weak);
                    ui.add(egui::Label::new(text).extend());
                });
        }
    };
    if expanded || lines.len() <= head + tail + 1 {
        mono(ui, &lines);
        return expanded
            && lines.len() > head + tail + 1
            && ui.small_button(t!("Collapse")).clicked();
    }
    mono(ui, &lines[..head]);
    let hidden = lines.len() - head - tail;
    let clicked = ui
        .add(
            egui::Button::new(RichText::new(t!("… {} unchanged lines", hidden)).weak())
                .frame(false),
        )
        .on_hover_text(t!("Show all"))
        .clicked();
    mono(ui, &lines[lines.len() - tail..]);
    clicked
}

/// One conflict: both sides next to each other and the choice buttons.
/// Returns true when the choice changed.
fn conflict_view(
    ui: &mut egui::Ui,
    c: &conflict::Conflict,
    n: usize,
    total: usize,
    choice: &mut Option<Choice>,
) -> bool {
    let before = *choice;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    egui::Frame::group(ui.style())
        .stroke(stroke)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.strong(t!("Conflict {} of {}", n + 1, total));
                if choice.is_none() {
                    ui.colored_label(ui.visuals().warn_fg_color, t!("unresolved"));
                }
                ui.add_space(12.0);
                for (label, value) in [
                    (t!("Ours"), Choice::Ours),
                    (t!("Theirs"), Choice::Theirs),
                    (t!("Both (ours first)"), Choice::OursThenTheirs),
                    (t!("Both (theirs first)"), Choice::TheirsThenOurs),
                ] {
                    let selected = *choice == Some(value);
                    if ui.add(egui::Button::selectable(selected, label)).clicked() {
                        *choice = if selected { None } else { Some(value) };
                    }
                }
                if choice.is_some() && ui.small_button(t!("Clear")).clicked() {
                    *choice = None;
                }
            });
            if let Some(base) = &c.base {
                egui::CollapsingHeader::new(RichText::new(t!("Base (common ancestor)")).weak())
                    .id_salt(("base", n))
                    .show(ui, |ui| side_text(ui, base, None));
            }
            let current = *choice;
            let uses = |side: Choice| match current {
                None => None,
                Some(Choice::OursThenTheirs | Choice::TheirsThenOurs) => Some(true),
                Some(ch) => Some(ch == side),
            };
            ui.columns(2, |cols| {
                let ours = ours_color(&cols[0]);
                let theirs = theirs_color(&cols[1]);
                side_view(
                    &mut cols[0],
                    &t!("Ours"),
                    &c.ours_label,
                    &c.ours,
                    ours,
                    uses(Choice::Ours),
                );
                side_view(
                    &mut cols[1],
                    &t!("Theirs"),
                    &c.theirs_label,
                    &c.theirs,
                    theirs,
                    uses(Choice::Theirs),
                );
            });
        });
    ui.add_space(4.0);
    *choice != before
}

/// One side of a conflict. `used` is `None` while undecided.
fn side_view(
    ui: &mut egui::Ui,
    name: &str,
    label: &str,
    text: &str,
    color: Color32,
    used: Option<bool>,
) {
    let chosen = used == Some(true);
    let frame = egui::Frame::new()
        .inner_margin(6.0)
        .corner_radius(4.0)
        .stroke(egui::Stroke::new(
            if chosen { 1.5 } else { 1.0 },
            if chosen {
                color
            } else {
                color.gamma_multiply(0.35)
            },
        ))
        .fill(if chosen {
            color.gamma_multiply(0.12)
        } else {
            Color32::TRANSPARENT
        });
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.colored_label(color, RichText::new(name).strong());
            if !label.is_empty() {
                ui.add(egui::Label::new(RichText::new(label).weak()).truncate());
            }
            if chosen {
                ui.colored_label(color, "✔");
            }
        });
        let dim = used == Some(false);
        side_text(ui, text, dim.then(|| ui.visuals().weak_text_color()));
    });
}

fn side_text(ui: &mut egui::Ui, text: &str, color: Option<Color32>) {
    if text.is_empty() {
        ui.label(RichText::new(t!("(empty)")).italics().weak());
        return;
    }
    let mut rt = RichText::new(text.strip_suffix('\n').unwrap_or(text)).monospace();
    if let Some(c) = color {
        rt = rt.color(c);
    }
    // A justified layout (as in columns) would strip the indentation.
    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
        ui.add(egui::Label::new(rt).wrap());
    });
}

fn result_panel(
    ui: &mut egui::Ui,
    file: &mut FileState,
    path: &str,
    busy: bool,
    cmds: &mut Commands,
    conflicted: &[String],
) {
    let unresolved = file.choices.iter().filter(|c| c.is_none()).count();
    let has_markers = conflict::has_markers(&file.result);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.strong(t!("Result"));
        if file.edited {
            ui.weak(t!("Edited by hand: choices above no longer change it."));
            if ui
                .small_button(t!("Discard edits"))
                .on_hover_text(t!("Rebuild the result from the choices above"))
                .clicked()
            {
                file.edited = false;
                file.result = conflict::render(&file.chunks, &file.choices);
            }
        } else if unresolved > 0 {
            ui.weak(if unresolved == 1 {
                t!("1 conflict still to choose. You can also edit the text.")
            } else {
                t!(
                    "{} conflicts still to choose. You can also edit the text.",
                    unresolved
                )
            });
        } else {
            ui.weak(t!("You can still edit the text before saving."));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = ui.add_enabled(!busy, egui::Button::new(t!("Mark as resolved")));
            if save.clicked() {
                let op = Op::SaveResolution {
                    path: path.to_owned(),
                    content: file.result.clone(),
                };
                if has_markers {
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Conflict markers left"),
                        &t!(
                            "{} still contains conflict markers. Mark it as resolved anyway?",
                            path
                        ),
                        op,
                        true,
                    )));
                } else {
                    cmds.run(op);
                }
            }
            if has_markers {
                ui.colored_label(ui.visuals().warn_fg_color, t!("markers left"));
            } else {
                ui.colored_label(crate::util::added_color(ui), t!("no markers left"));
            }
            let others = conflicted.iter().filter(|p| *p != path).count();
            if others > 0 {
                ui.weak(if others == 1 {
                    t!("then 1 more file")
                } else {
                    t!("then {} more files", others)
                });
            }
        });
    });
    if file.changed_on_disk {
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("The file changed on disk since it was loaded."),
            );
            if ui.small_button(t!("Reload")).clicked() {
                file.stamp = None;
                file.edited = false;
                file.changed_on_disk = false;
            }
        });
    }
    egui::ScrollArea::both()
        .id_salt(("conflict-result", path))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let edit = egui::TextEdit::multiline(&mut file.result)
                .code_editor()
                .desired_width(f32::INFINITY)
                .min_size(egui::vec2(0.0, ui.available_height()));
            if ui.add(edit).changed() {
                file.edited = true;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_conflicts() {
        let v = |s: Option<&str>| s.map(str::to_owned);
        let text = [v(Some("b")), v(Some("o")), v(Some("t"))];
        assert_eq!(situation(&text, Some(b"x")), Situation::Text);
        assert_eq!(situation(&text, Some(b"x\0y")), Situation::Binary);
        assert_eq!(situation(&text, None), Situation::Missing);
        let ours_gone = [v(Some("b")), None, v(Some("t"))];
        assert_eq!(situation(&ours_gone, Some(b"t")), Situation::DeletedByUs);
        let theirs_gone = [v(Some("b")), v(Some("o")), None];
        assert_eq!(situation(&theirs_gone, None), Situation::DeletedByThem);
    }

    #[test]
    fn picks_the_next_file() {
        let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // "b" (index 1) was resolved: "c" took its place.
        assert_eq!(next_file(&list(&["a", "c"]), "b", 1).as_deref(), Some("c"));
        // The last file was resolved: go back to the previous one.
        assert_eq!(next_file(&list(&["a", "b"]), "c", 2).as_deref(), Some("b"));
        assert_eq!(next_file(&list(&[]), "a", 0), None);
    }
}
