//! Smaller tools of the local changes view: .gitignore editing, patches and
//! resetting a file to a revision.

use eframe::egui;
use ferrum_core::gitignore;
use ferrum_core::{BranchKind, DiffSource};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;

/// A window opened from the file lists.
pub enum Tool {
    /// Ignore files matching a pattern typed by the user.
    Pattern {
        pattern: String,
        /// Tracked files the pattern was started from.
        tracked: Vec<String>,
        untrack: bool,
    },
    Templates {
        chosen: Vec<bool>,
    },
    ResetTo {
        path: String,
        rev: String,
    },
}

impl Tool {
    pub fn pattern(path: &str, tracked: Vec<String>) -> Self {
        Tool::Pattern {
            pattern: gitignore::file_pattern(path),
            untrack: !tracked.is_empty(),
            tracked,
        }
    }

    pub fn templates() -> Self {
        Tool::Templates {
            chosen: vec![false; gitignore::TEMPLATES.len()],
        }
    }

    pub fn reset_to(path: &str) -> Self {
        Tool::ResetTo {
            path: path.to_owned(),
            rev: "HEAD".into(),
        }
    }
}

/// Adds patterns to `.gitignore`, then stops tracking `untrack` (if any).
pub fn ignore(tab: &RepoTab, cmds: &mut Commands, patterns: &str, untrack: Vec<String>) {
    match gitignore::append(tab.path(), patterns) {
        Ok(_) if !untrack.is_empty() => cmds.run(Op::Untrack(untrack)),
        Ok(_) => cmds.push(Command::Refresh),
        Err(e) => cmds.push(Command::Error(t!("Could not update .gitignore: {}", e))),
    }
}

fn patch(
    tab: &RepoTab,
    cmds: &mut Commands,
    source: DiffSource,
    paths: &[String],
) -> Option<String> {
    match tab.repo.patch(source, paths) {
        Ok(p) if p.trim().is_empty() => {
            cmds.push(Command::Error(t!(
                "There are no changes to put in a patch."
            )));
            None
        }
        Ok(p) => Some(p),
        Err(e) => {
            cmds.push(Command::Error(e.to_string()));
            None
        }
    }
}

pub fn copy_patch(tab: &RepoTab, cmds: &mut Commands, source: DiffSource, paths: &[String]) {
    if let Some(p) = patch(tab, cmds, source, paths) {
        cmds.push(Command::Copy(p));
    }
}

pub fn save_patch(tab: &RepoTab, cmds: &mut Commands, source: DiffSource, paths: &[String]) {
    let Some(p) = patch(tab, cmds, source, paths) else {
        return;
    };
    let name = match paths {
        [one] => format!("{}.patch", one.rsplit('/').next().unwrap_or(one)),
        _ => "changes.patch".to_owned(),
    };
    let Some(file) = rfd::FileDialog::new()
        .set_file_name(name)
        .add_filter("Patch", &["patch", "diff"])
        .save_file()
    else {
        return;
    };
    if let Err(e) = std::fs::write(&file, p) {
        cmds.push(Command::Error(t!(
            "Could not save {}: {}",
            file.display(),
            e
        )));
    }
}

/// Shows the open tool window, if any.
pub fn show(ctx: &egui::Context, tab: &mut RepoTab, cmds: &mut Commands) {
    let Some(mut tool) = tab.changes.ui.tool.take() else {
        return;
    };
    let modal = egui::Modal::new(egui::Id::unique("changes-tool")).show(ctx, |ui| {
        ui.set_width(440.0);
        match &mut tool {
            Tool::Pattern {
                pattern,
                tracked,
                untrack,
            } => pattern_ui(ui, tab, cmds, pattern, tracked, untrack),
            Tool::Templates { chosen } => templates_ui(ui, tab, cmds, chosen),
            Tool::ResetTo { path, rev } => reset_ui(ui, tab, cmds, path, rev),
        }
    });
    if modal.inner && !modal.should_close() && tab.changes.ui.tool.is_none() {
        tab.changes.ui.tool = Some(tool);
    }
}

/// Cancel and OK buttons. Returns `Some(true)` for OK, `Some(false)` for cancel.
fn buttons(ui: &mut egui::Ui, ok: &str, enabled: bool) -> Option<bool> {
    ui.add_space(8.0);
    let mut out = None;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                enabled,
                egui::Button::new(ok).min_size(egui::vec2(90.0, 24.0)),
            )
            .clicked()
        {
            out = Some(true);
        }
        if ui.button(t!("Cancel")).clicked() {
            out = Some(false);
        }
    });
    out
}

fn pattern_ui(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    pattern: &mut String,
    tracked: &[String],
    untrack: &mut bool,
) -> bool {
    ui.heading(t!("Ignore by pattern"));
    ui.label(t!("Pattern to add to .gitignore:"));
    let resp = ui.add(egui::TextEdit::singleline(pattern).desired_width(f32::INFINITY));
    if ui.ctx().memory(|m| m.focused().is_none()) {
        resp.request_focus();
    }
    ui.weak(t!("Examples: *.log, build/, /docs/*.pdf, !keep.log"));
    if !tracked.is_empty() {
        ui.checkbox(
            untrack,
            t!("Stop tracking the selected files ({})", tracked.len()),
        )
        .on_hover_text(t!(
            "Removes them from the repository but keeps them on disk (git rm --cached)"
        ));
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let ok = !pattern.trim().is_empty();
    match buttons(ui, &t!("Ignore"), ok) {
        Some(true) => {}
        Some(false) => return false,
        None if enter && ok => {}
        None => return true,
    }
    let untrack = if *untrack {
        tracked.to_vec()
    } else {
        Vec::new()
    };
    ignore(tab, cmds, pattern, untrack);
    false
}

fn templates_ui(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    chosen: &mut [bool],
) -> bool {
    ui.heading(t!("Add .gitignore template"));
    ui.label(t!("Patterns of the chosen templates are added to .gitignore (those already there are skipped)."));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (i, (name, _)) in gitignore::TEMPLATES.iter().enumerate() {
            ui.toggle_value(&mut chosen[i], *name);
        }
    });
    let text: String = gitignore::TEMPLATES
        .iter()
        .zip(chosen.iter())
        .filter(|(_, on)| **on)
        .map(|((name, p), _)| format!("# {name}\n{p}"))
        .collect();
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if text.is_empty() {
                ui.weak(t!("Choose one or more templates."));
            } else {
                ui.monospace(&text);
            }
        });
    match buttons(ui, &t!("Add to .gitignore"), !text.is_empty()) {
        Some(true) => {
            // Skip the "# name" lines: only the patterns are added.
            let patterns: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
            ignore(tab, cmds, &patterns.join("\n"), Vec::new());
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn reset_ui(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    cmds: &mut Commands,
    path: &str,
    rev: &mut String,
) -> bool {
    ui.heading(t!("Reset file to a revision"));
    ui.label(t!("Replace {} with its content at:", path));
    ui.add(
        egui::TextEdit::singleline(rev)
            .hint_text(t!("Branch, tag or commit"))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(200.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for b in tab.branches.iter().filter(|b| b.kind == BranchKind::Local) {
                if ui
                    .selectable_label(*rev == b.name, format!("🌿 {}", b.name))
                    .clicked()
                {
                    *rev = b.name.clone();
                }
            }
            for c in tab.commits.iter().take(20) {
                let id = c.id.to_string();
                let short = ferrum_core::short(c.id);
                if ui
                    .selectable_label(*rev == id, format!("{short}  {}", c.summary))
                    .clicked()
                {
                    *rev = id;
                }
            }
        });
    ui.colored_label(
        ui.visuals().warn_fg_color,
        t!("Uncommitted changes in this file will be lost."),
    );
    let valid = tab.repo.resolve(rev.trim()).is_ok();
    match buttons(ui, &t!("Reset file"), valid && !tab.busy()) {
        Some(true) => {
            cmds.run(Op::ResetFileTo {
                path: path.to_owned(),
                rev: rev.trim().to_owned(),
            });
            false
        }
        Some(false) => false,
        None => true,
    }
}
