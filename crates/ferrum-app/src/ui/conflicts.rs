//! Three-way conflict resolver: pick a side (or both) for each conflict, then
//! fine-tune the result by hand before marking the file as resolved. Binary
//! files, images, LFS files and submodules are resolved by picking a side.

use std::collections::{BTreeSet, HashSet};
use std::time::SystemTime;

use eframe::egui::{self, Color32, RichText};
use ferrum_core::conflict::{self, Choice, Chunk};
use ferrum_core::diff::{LfsPointer, parse_lfs_pointer};
use ferrum_core::integrate::{self, GITLINK, InProgress};
use ferrum_core::ops::Side;
use ferrum_core::{Oid, RepoState};

use crate::command::{Command, Commands, Op};
use crate::flow::FlowOp;
use crate::icons;
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;
use crate::ui::smart_merge::{self, CardAction, Outcome};

/// Lines of unchanged text kept visible next to each conflict.
const CONTEXT: usize = 3;

#[derive(Default)]
pub struct ConflictState {
    pub path: Option<String>,
    file: Option<FileState>,
    /// Position of the file in the conflicted list, to pick the next one once it is resolved.
    index: usize,
    /// Files resolved during the current operation, whose conflict can be brought back.
    pub resolved: Vec<String>,
    progress: Option<Progress>,
    /// Files ticked in the list of all conflicted files.
    multi: BTreeSet<String>,
    /// Conflict to scroll to.
    scroll_to: Option<usize>,
    /// Compare a file without markers left with theirs instead of ours.
    compare_theirs: bool,
    /// Semantic merge suggestions for the conflicted files.
    pub smart: smart_merge::Suggestions,
    /// Show the base version between both sides.
    show_base: bool,
    /// The suggestion's difference with ours (false) or theirs (true).
    compare: Option<bool>,
    /// The conflict last navigated to.
    current: usize,
    /// "Suggest resolution" by the AI provider.
    ai: crate::ui::ai_conflict::AiHelp,
}

/// What the stopped operation is doing, re-read when the repository changes.
struct Progress {
    key: (RepoState, Option<Oid>, usize),
    info: InProgress,
    conflicted: Vec<String>,
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
    Image,
    /// Git LFS pointers.
    Lfs,
    Submodule,
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
    /// Ours and theirs as bytes (images), with their blob ids.
    blobs: [Option<(String, Vec<u8>)>; 2],
    /// Ours and theirs commits of a submodule, with a description.
    gitlinks: [Option<(Oid, String)>; 2],
    lfs: [Option<LfsPointer>; 2],
    chunks: Vec<Chunk>,
    choices: Vec<Option<Choice>>,
    result: String,
    /// The result was edited by hand: choices no longer regenerate it.
    edited: bool,
    /// The result is the semantic merge suggestion.
    suggested: bool,
    /// Common chunks shown in full.
    expanded: HashSet<usize>,
    /// Diff of the result with one side, for files without markers: (theirs?, result, lines).
    review: Option<(bool, String, Vec<Option<conflict::DiffLine>>)>,
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

/// Refines the situation for LFS pointers and images.
fn refine(s: Situation, path: &str, lfs: &[Option<LfsPointer>; 2]) -> Situation {
    match s {
        Situation::Text | Situation::Binary if lfs.iter().all(Option::is_some) => Situation::Lfs,
        Situation::Binary if crate::ui::image_diff::is_image(path) => Situation::Image,
        s => s,
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

fn submodule_text(tab: &RepoTab, path: &str, id: Oid) -> String {
    let summary = ferrum_core::Repo::open(tab.path().join(path))
        .ok()
        .and_then(|r| r.find_commit_info(id).ok())
        .map(|c| c.summary);
    match summary {
        Some(s) => format!("{} {}", ferrum_core::short(id), s),
        None => ferrum_core::short(id),
    }
}

fn load(tab: &RepoTab, path: &str) -> FileState {
    let mut error = None;
    let stages = integrate::stages(&tab.git, path).unwrap_or_default();
    let stage = |n: u8| stages.iter().find(|s| s.stage == n);
    let gitlink = stages.iter().any(|s| s.mode == GITLINK);
    let versions = if gitlink {
        Default::default()
    } else {
        tab.repo.conflict_versions(path).unwrap_or_else(|e| {
            error = Some(e.to_string());
            Default::default()
        })
    };
    let disk = tab.repo.read_workdir_file(path).ok();
    let lfs = [
        versions[1].as_deref().and_then(parse_lfs_pointer),
        versions[2].as_deref().and_then(parse_lfs_pointer),
    ];
    let situation = if gitlink {
        Situation::Submodule
    } else {
        refine(situation(&versions, disk.as_deref()), path, &lfs)
    };
    let blobs = if situation == Situation::Image {
        [2, 3].map(|n| {
            let s = stage(n)?;
            Some((s.id.to_string(), tab.repo.blob(s.id).ok()?))
        })
    } else {
        Default::default()
    };
    let gitlinks = if gitlink {
        [2, 3].map(|n| stage(n).map(|s| (s.id, submodule_text(tab, path, s.id))))
    } else {
        Default::default()
    };
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
        blobs,
        gitlinks,
        lfs,
        result: conflict::render(&chunks, &choices),
        chunks,
        choices,
        edited: false,
        suggested: false,
        expanded: HashSet::new(),
        review: None,
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

/// What the stopped operation is doing. Also keeps the list of files
/// resolved since it stopped (cleared when another operation or step starts).
pub fn in_progress(tab: &mut RepoTab) -> InProgress {
    let key = (tab.state, tab.head_oid, tab.status.conflicted.len());
    if let Some(p) = &tab.conflicts.progress
        && p.key == key
    {
        return p.info.clone();
    }
    let info = integrate::in_progress(tab.repo.git_dir(), tab.state);
    let now: Vec<String> = tab
        .status
        .conflicted
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let st = &mut tab.conflicts;
    match st.progress.take() {
        Some(prev) if prev.key.0 == key.0 && prev.info == info => {
            for p in prev.conflicted.iter().filter(|p| !now.contains(p)) {
                if !st.resolved.contains(p) {
                    st.resolved.push(p.clone());
                }
            }
            st.resolved.retain(|p| !now.contains(p));
        }
        _ => st.resolved.clear(),
    }
    st.progress = Some(Progress {
        key,
        info: info.clone(),
        conflicted: now,
    });
    info
}

/// Names of both sides: branch names or commits instead of "ours" and "theirs".
pub fn side_names(tab: &mut RepoTab) -> (String, String) {
    let info = in_progress(tab);
    let name = |id: Oid| crate::ui::banner::name_of(tab, id);
    let head = tab.head.clone();
    let (ours, theirs) = match tab.state {
        RepoState::Merge => (
            head,
            info.merge_name
                .clone()
                .or_else(|| info.merge_heads.first().map(|id| name(*id))),
        ),
        RepoState::Rebase => (
            info.onto.map(name),
            info.current.map(|id| match &info.head_name {
                Some(b) => format!("{b} {}", ferrum_core::short(id)),
                None => ferrum_core::short(id),
            }),
        ),
        RepoState::CherryPick | RepoState::Revert => (head, info.current.map(ferrum_core::short)),
        _ => (None, None),
    };
    (
        ours.unwrap_or_else(|| t!("ours")),
        theirs.unwrap_or_else(|| t!("theirs")),
    )
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    smart_merge::update(ui.ctx(), tab);
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
                crate::ui::empty_state::show(
                    ui,
                    icons::CHECK_CIRCLE,
                    &t!("No conflicts."),
                    &t!(
                        "Every conflicted file is resolved: continue the operation from the banner."
                    ),
                    |ui| {
                        if ui
                            .button(icons::text(icons::ARROW_LEFT, t!("Local changes")))
                            .clicked()
                        {
                            cmds.push(Command::SetView(View::Changes));
                        }
                    },
                );
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

    let names = side_names(tab);
    top_bar(ui, tab, cmds, &conflicted, &path, &names);
    several_files(ui, tab, cmds, &conflicted, &names);
    ui.separator();

    let in_rebase = tab.state == RepoState::Rebase;
    let ConflictState {
        file,
        scroll_to,
        compare_theirs,
        smart,
        show_base,
        compare,
        current,
        ai,
        ..
    } = &mut tab.conflicts;
    let Some(file) = file.as_mut() else {
        return;
    };
    if !conflicted.contains(&path) {
        ui.label(t!("{} has no conflicts.", path));
        return;
    }
    if let Some(e) = &file.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    let (ours_name, theirs_name) = (&names.0, &names.1);
    if in_rebase {
        ui.weak(t!("During a rebase, {} is the branch being rebased onto and {} is your commit being replayed.", ours_name, theirs_name));
    }

    match file.situation {
        Situation::Text => {}
        Situation::DeletedByUs | Situation::DeletedByThem => {
            deleted_view(ui, file, &path, busy, cmds, &names);
            return;
        }
        Situation::Submodule => {
            submodule_view(ui, file, &path, busy, cmds, &names);
            return;
        }
        Situation::Image => {
            ui.add_space(8.0);
            ui.label(t!("Both sides changed this image. Keep one of them:"));
            ui.add_enabled_ui(!busy, |ui| whole_file_buttons(ui, &path, cmds, &names));
            ui.add_space(4.0);
            ui.weak(t!("Left: {} · Right: {}", ours_name, theirs_name));
            let side =
                |b: &Option<(String, Vec<u8>)>| b.as_ref().map(|(id, v)| (id.clone(), v.clone()));
            let (ours, theirs) = (side(&file.blobs[0]), side(&file.blobs[1]));
            crate::ui::image_diff::show_bytes(
                ui,
                &path,
                ours.as_ref().map(|(i, b)| (i.as_str(), b.as_slice())),
                theirs.as_ref().map(|(i, b)| (i.as_str(), b.as_slice())),
            );
            return;
        }
        Situation::Lfs => {
            lfs_view(ui, file, &path, busy, cmds, &names);
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
            ui.add_enabled_ui(!busy, |ui| whole_file_buttons(ui, &path, cmds, &names));
            return;
        }
    }

    match smart
        .get(&path)
        .filter(|_| !smart.dismissed.contains(&path))
    {
        Some(Outcome::Resolved(s)) => {
            let applied = file.result == s.text;
            let versions = [
                file.versions[1].as_deref().unwrap_or(""),
                file.versions[2].as_deref().unwrap_or(""),
            ];
            match smart_merge::card(ui, &s, applied, versions, &names, compare) {
                Some(CardAction::Use) => {
                    file.result = s.text.clone();
                    file.edited = true;
                    file.suggested = true;
                }
                Some(CardAction::Dismiss) => {
                    smart.dismissed.insert(path.clone());
                }
                None => {}
            }
            ui.add_space(crate::theme::SPACE_S);
        }
        Some(Outcome::Failed(f)) => {
            if let Some(why) = smart_merge::failure_text(&f) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icons::INFO).color(ui.visuals().weak_text_color()));
                    ui.weak(t!("The semantic merge left this file to you: {}.", why));
                });
            }
        }
        None => {}
    }
    if let Some(text) =
        crate::ui::ai_conflict::show(ui, ai, &path, &file.versions, &names, &file.result)
    {
        file.result = text;
        file.edited = true;
        file.suggested = false;
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
            review_view(ui, file, &names, compare_theirs);
            return;
        }
        chunks_toolbar(ui, file, &names, show_base, current, scroll_to);
        let out = egui::ScrollArea::vertical()
            .id_salt(("conflicts", &path))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                chunks_view(ui, file, &names, *show_base, scroll_to)
            });
        markers(
            ui,
            out.inner_rect,
            out.content_size.y,
            &out.inner,
            &file.choices,
            scroll_to,
        );
    });
}

fn top_bar(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    conflicted: &[String],
    path: &str,
    names: &(String, String),
) {
    ui.horizontal(|ui| {
        if ui
            .button(icons::text(icons::ARROW_LEFT, t!("Local changes")))
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
                let auto = tab.conflicts.smart.resolved(conflicted);
                for p in conflicted {
                    let text = if auto.contains(&p) {
                        icons::tinted(icons::MAGIC_WAND, crate::theme::accent(ui), p)
                    } else {
                        p.into()
                    };
                    if ui.selectable_label(p == path, text).clicked() && p != path {
                        cmds.push(Command::ResolveConflict(p.clone()));
                    }
                }
            });
        ui.weak(counter);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!tab.busy() && pos.is_some(), |ui| {
                if ui
                    .button(t!("Use {} for whole file", names.1))
                    .on_hover_text(t!("Take their version of the file and mark it resolved"))
                    .clicked()
                {
                    resolve_with(cmds, path, Side::Theirs);
                }
                if ui
                    .button(t!("Use {} for whole file", names.0))
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

/// Every conflicted file with a checkbox, to resolve several with one side.
fn several_files(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    cmds: &mut Commands,
    conflicted: &[String],
    names: &(String, String),
) {
    if conflicted.len() < 2 {
        return;
    }
    let busy = tab.busy();
    let multi = &mut tab.conflicts.multi;
    multi.retain(|p| conflicted.contains(p));
    egui::CollapsingHeader::new(t!("All conflicted files ({})", conflicted.len()))
        .id_salt("conflict-many")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button(t!("Select all")).clicked() {
                    multi.extend(conflicted.iter().cloned());
                }
                if ui.small_button(t!("Select none")).clicked() {
                    multi.clear();
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("conflict-many-list")
                .max_height(160.0)
                .show(ui, |ui| {
                    for p in conflicted {
                        let mut on = multi.contains(p);
                        if ui.checkbox(&mut on, p.as_str()).changed() {
                            if on {
                                multi.insert(p.clone());
                            } else {
                                multi.remove(p);
                            }
                        }
                    }
                });
            let n = multi.len();
            ui.add_enabled_ui(n > 0 && !busy, |ui| {
                ui.horizontal(|ui| {
                    for (side, name) in [(Side::Ours, &names.0), (Side::Theirs, &names.1)] {
                        if ui.button(t!("Use {} for {} selected", name, n)).clicked() {
                            cmds.run(Op::Flow(FlowOp::ResolveMany {
                                paths: multi.iter().cloned().collect(),
                                side,
                            }));
                            multi.clear();
                        }
                    }
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

fn whole_file_buttons(
    ui: &mut egui::Ui,
    path: &str,
    cmds: &mut Commands,
    names: &(String, String),
) {
    ui.horizontal(|ui| {
        if ui.button(t!("Use {}", names.0)).clicked() {
            resolve_with(cmds, path, Side::Ours);
        }
        if ui.button(t!("Use {}", names.1)).clicked() {
            resolve_with(cmds, path, Side::Theirs);
        }
    });
}

fn deleted_view(
    ui: &mut egui::Ui,
    file: &FileState,
    path: &str,
    busy: bool,
    cmds: &mut Commands,
    names: &(String, String),
) {
    let by_us = file.situation == Situation::DeletedByUs;
    let (deleter, changer) = if by_us {
        (&names.0, &names.1)
    } else {
        (&names.1, &names.0)
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

/// Both commits of a conflicted submodule; resolving records one of them.
fn submodule_view(
    ui: &mut egui::Ui,
    file: &FileState,
    path: &str,
    busy: bool,
    cmds: &mut Commands,
    names: &(String, String),
) {
    ui.add_space(12.0);
    ui.label(t!(
        "Both sides moved the submodule {} to different commits.",
        path
    ));
    ui.label(t!("Pick the commit to keep:"));
    ui.add_space(4.0);
    for (i, (stage, name)) in [(2u8, &names.0), (3u8, &names.1)].into_iter().enumerate() {
        ui.horizontal(|ui| {
            let text = match &file.gitlinks[i] {
                Some((_, t)) => t.clone(),
                None => t!("(deleted)"),
            };
            ui.add_enabled_ui(!busy, |ui| {
                if ui.button(t!("Use {}", name)).clicked() {
                    cmds.run(Op::Flow(FlowOp::ResolveGitlink {
                        path: path.to_owned(),
                        stage,
                    }));
                }
            });
            ui.label(RichText::new(text).monospace());
        });
    }
    ui.add_space(4.0);
    ui.weak(t!(
        "Only the recorded commit changes: update the submodule afterwards to check it out."
    ));
}

fn lfs_view(
    ui: &mut egui::Ui,
    file: &FileState,
    path: &str,
    busy: bool,
    cmds: &mut Commands,
    names: &(String, String),
) {
    ui.add_space(12.0);
    ui.label(t!(
        "This file is stored with Git LFS, so it cannot be merged line by line."
    ));
    ui.label(t!("Resolve it by keeping one version of the whole file:"));
    ui.add_space(4.0);
    egui::Grid::new("lfs-conflict")
        .num_columns(3)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for (i, name) in [&names.0, &names.1].into_iter().enumerate() {
                ui.strong(name.as_str());
                match &file.lfs[i] {
                    Some(p) => {
                        let oid = p.oid.strip_prefix("sha256:").unwrap_or(&p.oid);
                        ui.label(
                            RichText::new(oid.chars().take(12).collect::<String>()).monospace(),
                        );
                        ui.label(crate::ui::diff_extra::format_size(p.size));
                    }
                    None => {
                        ui.weak(t!("(deleted)"));
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    ui.add_enabled_ui(!busy, |ui| whole_file_buttons(ui, path, cmds, names));
}

/// A file without markers left (resolved by hand or in a merge tool): its
/// difference with one side, to check before marking it resolved.
fn review_view(
    ui: &mut egui::Ui,
    file: &mut FileState,
    names: &(String, String),
    theirs: &mut bool,
) {
    ui.add_space(4.0);
    ui.label(t!("No conflict markers are left in this file."));
    ui.weak(t!("Check the result below and mark the file as resolved."));
    ui.horizontal(|ui| {
        ui.label(t!("Changes compared with"));
        ui.selectable_value(theirs, false, names.0.as_str());
        ui.selectable_value(theirs, true, names.1.as_str());
    });
    let fresh = file
        .review
        .as_ref()
        .is_some_and(|(t, r, _)| *t == *theirs && *r == file.result);
    if !fresh {
        let side = file.versions[if *theirs { 2 } else { 1 }]
            .clone()
            .unwrap_or_default();
        let lines = conflict::diff_lines(&side, &file.result, 3);
        file.review = Some((*theirs, file.result.clone(), lines));
    }
    let Some((_, _, lines)) = &file.review else {
        return;
    };
    if lines.is_empty() {
        ui.weak(t!(
            "The result is the same as {}.",
            if *theirs { &names.1 } else { &names.0 }
        ));
        return;
    }
    let (add, del) = (crate::util::added_color(ui), ui.visuals().error_fg_color);
    let weak = ui.visuals().weak_text_color();
    egui::ScrollArea::both()
        .id_salt("conflict-review")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for line in lines {
                let (text, color) = match line {
                    None => ("…".to_owned(), weak),
                    Some((o, t)) => (
                        format!("{o} {t}"),
                        match o {
                            '+' => add,
                            '-' => del,
                            _ => weak,
                        },
                    ),
                };
                ui.add(egui::Label::new(RichText::new(text).monospace().color(color)).extend());
            }
        });
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

/// Shows the chunks; returns the top of each conflict, relative to the content.
fn chunks_view(
    ui: &mut egui::Ui,
    file: &mut FileState,
    names: &(String, String),
    show_base: bool,
    scroll_to: &mut Option<usize>,
) -> Vec<f32> {
    let origin = ui.min_rect().top();
    let last = file.chunks.len().saturating_sub(1);
    let total = file.choices.len();
    let mut n = 0;
    let mut changed = false;
    let mut tops = Vec::with_capacity(total);
    let mut nav = None;
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
                let (ch, rect, go) = conflict_view(ui, c, n, total, choice, names, show_base);
                changed |= ch;
                nav = nav.or(go);
                tops.push(rect.top() - origin);
                if *scroll_to == Some(n) {
                    ui.scroll_to_rect(rect, Some(egui::Align::TOP));
                    *scroll_to = None;
                }
                n += 1;
            }
        }
    }
    if nav.is_some() {
        *scroll_to = nav;
    }
    if changed && !file.edited {
        file.result = conflict::render(&file.chunks, &file.choices);
    }
    tops
}

/// Above the conflicts: progress, navigation, base column and whole-file choices.
fn chunks_toolbar(
    ui: &mut egui::Ui,
    file: &mut FileState,
    names: &(String, String),
    show_base: &mut bool,
    current: &mut usize,
    scroll_to: &mut Option<usize>,
) {
    let total = file.choices.len();
    let open: Vec<usize> = (0..total).filter(|&i| file.choices[i].is_none()).collect();
    let mut go = |n: usize, current: &mut usize| {
        *current = n;
        *scroll_to = Some(n);
    };
    // Next/previous unresolved conflict, also with the next/previous hunk keys.
    let next = open
        .iter()
        .copied()
        .find(|&i| i > *current)
        .or(open.first().copied());
    let prev = open
        .iter()
        .rev()
        .copied()
        .find(|&i| i < *current)
        .or(open.last().copied());
    let ctx = ui.ctx().clone();
    if !ctx.text_edit_focused() {
        use crate::keymap::{Action, pressed};
        if pressed(&ctx, Action::NextHunk)
            && let Some(n) = next
        {
            go(n, current);
        }
        if pressed(&ctx, Action::PrevHunk)
            && let Some(n) = prev
        {
            go(n, current);
        }
    }
    ui.horizontal(|ui| {
        let done = total - open.len();
        ui.add(
            egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                .desired_width(90.0)
                .desired_height(6.0),
        );
        ui.label(t!("{} of {} conflicts resolved", done, total));
        let keys = |a| crate::keymap::text(a);
        if ui
            .add_enabled(prev.is_some(), icons::icon_button(icons::CARET_UP))
            .on_hover_text(format!(
                "{} ({})",
                t!("Previous unresolved conflict"),
                keys(crate::keymap::Action::PrevHunk)
            ))
            .clicked()
            && let Some(n) = prev
        {
            go(n, current);
        }
        if ui
            .add_enabled(next.is_some(), icons::icon_button(icons::CARET_DOWN))
            .on_hover_text(format!(
                "{} ({})",
                t!("Next unresolved conflict"),
                keys(crate::keymap::Action::NextHunk)
            ))
            .clicked()
            && let Some(n) = next
        {
            go(n, current);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut all = |choice: Choice| {
                for c in file.choices.iter_mut().filter(|c| c.is_none()) {
                    *c = Some(choice);
                }
                if !file.edited {
                    file.result = conflict::render(&file.chunks, &file.choices);
                }
            };
            ui.add_enabled_ui(!open.is_empty(), |ui| {
                if ui
                    .button(t!("All {}", names.1))
                    .on_hover_text(t!("Take {} in every unresolved conflict", names.1))
                    .clicked()
                {
                    all(Choice::Theirs);
                }
                if ui
                    .button(t!("All {}", names.0))
                    .on_hover_text(t!("Take {} in every unresolved conflict", names.0))
                    .clicked()
                {
                    all(Choice::Ours);
                }
            });
            let has_base = file
                .chunks
                .iter()
                .any(|c| matches!(c, Chunk::Conflict(c) if c.base.is_some()));
            if has_base {
                ui.toggle_value(show_base, icons::text(icons::COLUMNS, t!("Show base")))
                    .on_hover_text(t!("Show the common ancestor between both sides"));
            }
        });
    });
    ui.add_space(crate::theme::SPACE_S);
}

/// Marks each conflict next to the scroll bar (amber unresolved, green
/// resolved); clicking one scrolls to it.
fn markers(
    ui: &mut egui::Ui,
    area: egui::Rect,
    content_height: f32,
    tops: &[f32],
    choices: &[Option<Choice>],
    scroll_to: &mut Option<usize>,
) {
    if content_height <= 0.0 || tops.is_empty() {
        return;
    }
    let (warn, done) = (ui.visuals().warn_fg_color, crate::util::added_color(ui));
    for (n, top) in tops.iter().enumerate() {
        let y = area.top() + (top / content_height).clamp(0.0, 1.0) * area.height();
        let r = egui::Rect::from_min_size(
            egui::pos2(area.right() - 16.0, (y - 2.0).min(area.bottom() - 4.0)),
            egui::vec2(6.0, 4.0),
        );
        let resolved = choices.get(n).copied().flatten().is_some();
        let resp = ui
            .interact(
                r.expand(2.0),
                ui.make_persistent_id(("conflict-marker", n)),
                egui::Sense::click(),
            )
            .on_hover_text(t!("Conflict {} of {}", n + 1, tops.len()));
        ui.painter()
            .rect_filled(r, 1.0, if resolved { done } else { warn });
        if resp.clicked() {
            *scroll_to = Some(n);
        }
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
/// Returns whether the choice changed, the conflict's rectangle and a
/// conflict to jump to.
fn conflict_view(
    ui: &mut egui::Ui,
    c: &conflict::Conflict,
    n: usize,
    total: usize,
    choice: &mut Option<Choice>,
    names: &(String, String),
    show_base: bool,
) -> (bool, egui::Rect, Option<usize>) {
    let before = *choice;
    let mut nav = None;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    let resp = egui::Frame::group(ui.style())
        .stroke(stroke)
        .corner_radius(crate::theme::RADIUS)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.strong(t!("Conflict {} of {}", n + 1, total));
                if ui
                    .add_enabled(n > 0, icons::icon_button(icons::CARET_UP).small())
                    .on_hover_text(t!("Previous conflict"))
                    .clicked()
                {
                    nav = Some(n - 1);
                }
                if ui
                    .add_enabled(n + 1 < total, icons::icon_button(icons::CARET_DOWN).small())
                    .on_hover_text(t!("Next conflict"))
                    .clicked()
                {
                    nav = Some(n + 1);
                }
                if choice.is_none() {
                    ui.colored_label(ui.visuals().warn_fg_color, t!("unresolved"));
                }
                ui.add_space(12.0);
                for (label, value) in [
                    (names.0.clone(), Choice::Ours),
                    (names.1.clone(), Choice::Theirs),
                    (t!("Both ({} first)", names.0), Choice::OursThenTheirs),
                    (t!("Both ({} first)", names.1), Choice::TheirsThenOurs),
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
            let base = c.base.as_ref().filter(|_| show_base);
            if let Some(base) = c.base.as_ref().filter(|_| !show_base) {
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
            ui.columns(if base.is_some() { 3 } else { 2 }, |cols| {
                let ours = ours_color(&cols[0]);
                let last = cols.len() - 1;
                let theirs = theirs_color(&cols[last]);
                side_view(
                    &mut cols[0],
                    &names.0,
                    &c.ours_label,
                    &c.ours,
                    ours,
                    uses(Choice::Ours),
                );
                if let Some(base) = base {
                    let weak = cols[1].visuals().weak_text_color();
                    side_view(&mut cols[1], &t!("Base"), "", base, weak, Some(false));
                }
                side_view(
                    &mut cols[last],
                    &names.1,
                    &c.theirs_label,
                    &c.theirs,
                    theirs,
                    uses(Choice::Theirs),
                );
            });
        })
        .response;
    ui.add_space(4.0);
    (*choice != before, resp.rect, nav)
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
            if !label.is_empty() && label != name {
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
        if file.suggested {
            ui.weak(t!(
                "The semantic merge result: check it, then mark the file as resolved."
            ));
            if ui
                .small_button(t!("Discard"))
                .on_hover_text(t!("Rebuild the result from the choices above"))
                .clicked()
            {
                file.edited = false;
                file.suggested = false;
                file.result = conflict::render(&file.chunks, &file.choices);
            }
        } else if file.edited {
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
            let save = ui.add_enabled(!busy, crate::theme::primary(t!("Mark as resolved")));
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
                file.suggested = false;
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

        let none = [None, None];
        assert_eq!(refine(Situation::Binary, "a.png", &none), Situation::Image);
        assert_eq!(refine(Situation::Binary, "a.bin", &none), Situation::Binary);
        let ptr = parse_lfs_pointer(
            "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 12\n",
        );
        let both = [ptr.clone(), ptr];
        assert_eq!(refine(Situation::Text, "a.psd", &both), Situation::Lfs);
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
