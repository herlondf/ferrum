//! AI code review of a branch or commit range: runs the AI command from
//! Settings > AI with an editable prompt and `git diff base...head`, and
//! shows the answer. Cancellable.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, RichText};
use ferrum_core::browse::Rev;
use ferrum_core::cli::Git;

use crate::command::{Command, Commands};
use crate::hosted::{HostedCmd, Pending};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::ai_message;

const MAX_DIFF: usize = 120_000;

pub struct ReviewWindow {
    dir: PathBuf,
    pub base: String,
    pub head: String,
    pub prompt: String,
    running: Option<(Pending<String>, Arc<AtomicBool>)>,
    pub output: Option<Result<String, String>>,
}

impl ReviewWindow {
    pub fn new(dir: PathBuf, base: String, head: String, prompt: String) -> Self {
        Self {
            dir,
            base,
            head,
            prompt,
            running: None,
            output: None,
        }
    }

    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    fn start(&mut self, ctx: &egui::Context, command: String) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (dir, base, head) = (self.dir.clone(), self.base.clone(), self.head.clone());
        let (template, flag) = (self.prompt.clone(), cancel.clone());
        let task = Pending::spawn(ctx, move || {
            run(&dir, &command, &template, &base, &head, &flag)
        });
        self.output = None;
        self.running = Some((task, cancel));
    }
}

/// The diff reviewed: what `head` adds since it left `base`.
pub fn review_diff(dir: &std::path::Path, base: &str, head: &str) -> Result<String, String> {
    let range = format!("{}...{}", base.trim(), head.trim());
    let diff = Git::new(dir)
        .run(&["diff", "--no-color", "--no-ext-diff", &range])
        .map_err(|e| e.to_string())?;
    if diff.trim().is_empty() {
        return Err(t!("No changes between {} and {}.", base, head));
    }
    Ok(ai_message::truncate(diff, MAX_DIFF))
}

fn run(
    dir: &std::path::Path,
    command: &str,
    template: &str,
    base: &str,
    head: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if command.trim().is_empty() {
        return Err(t!("Set the AI command in Settings first."));
    }
    let diff = review_diff(dir, base, head)?;
    let out = ai_message::run_command(
        dir,
        command,
        ai_message::build_prompt(template, &diff),
        cancel,
    )?;
    if out.trim().is_empty() {
        return Err(t!("The AI tool returned an empty answer."));
    }
    Ok(out.trim().to_owned())
}

/// The branch a review compares `rev` with: the remote's default branch, or main/master.
pub fn default_base(tab: &RepoTab, rev: &str) -> String {
    let remote = crate::hosted::hosted_remote_name(tab).unwrap_or_else(|| "origin".into());
    let names: Vec<&str> = tab.branches.iter().map(|b| b.name.as_str()).collect();
    let candidates = [
        format!("{remote}/main"),
        format!("{remote}/master"),
        format!("{remote}/develop"),
        "main".into(),
        "master".into(),
    ];
    candidates
        .into_iter()
        .find(|c| c != rev && names.contains(&c.as_str()) && !rev.ends_with(&format!("/{c}")))
        .unwrap_or_else(|| "HEAD~1".into())
}

/// "AI code review…" in branch menus.
pub fn menu_item(ui: &mut egui::Ui, tab: &RepoTab, rev: &str, cmds: &mut Commands) {
    if ui
        .button(t!("AI code review…"))
        .on_hover_text(t!(
            "Ask the AI tool from Settings > AI to review the changes of this branch"
        ))
        .clicked()
    {
        cmds.push(Command::Hosted(HostedCmd::Review {
            base: default_base(tab, rev),
            head: rev.to_owned(),
        }));
        ui.close();
    }
}

/// "AI review" in the compare view of two commits.
pub fn compare_button(ui: &mut egui::Ui, from: &Rev, to: &Rev, cmds: &mut Commands) {
    if let (Rev::Commit(a), Rev::Commit(b)) = (from, to)
        && ui
            .button(t!("AI review"))
            .on_hover_text(t!("AI code review of the changes between the two commits"))
            .clicked()
    {
        cmds.push(Command::Hosted(HostedCmd::Review {
            base: a.to_string(),
            head: b.to_string(),
        }));
    }
}

/// Returns `false` when the window is closed.
pub fn show(
    ctx: &egui::Context,
    w: &mut ReviewWindow,
    settings: &mut Settings,
    cmds: &mut Commands,
) -> bool {
    if let Some((task, _)) = &w.running
        && let Some(r) = task.poll()
    {
        w.output = Some(r);
        w.running = None;
    }
    let mut open = true;
    egui::Window::new(t!("AI code review"))
        .id(egui::Id::unique("ai-review"))
        .open(&mut open)
        .default_size([720.0, 560.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(t!("Base"));
                ui.add(egui::TextEdit::singleline(&mut w.base).desired_width(200.0));
                ui.label(t!("Changes of"));
                ui.add(egui::TextEdit::singleline(&mut w.head).desired_width(200.0));
            });
            ui.weak(t!(
                "Reviews git diff {}...{} with: {}",
                w.base,
                w.head,
                settings.ai.command
            ));
            egui::CollapsingHeader::new(t!("Prompt"))
                .default_open(w.output.is_none() && !w.running())
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut w.prompt)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(t!("Save as default prompt")).clicked() {
                            settings.forge.review_prompt = w.prompt.clone();
                        }
                        if ui.button(t!("Restore default")).clicked() {
                            w.prompt = crate::hosted::DEFAULT_REVIEW_PROMPT.into();
                        }
                    });
                });
            ui.horizontal(|ui| {
                if let Some((_, cancel)) = &w.running {
                    ui.spinner();
                    ui.label(t!("Reviewing…"));
                    if ui.button(t!("Cancel")).clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                } else {
                    let ready = !w.base.trim().is_empty() && !w.head.trim().is_empty();
                    if ui
                        .add_enabled(ready, egui::Button::new(t!("Start review")))
                        .clicked()
                    {
                        w.start(ctx, settings.ai.command.clone());
                    }
                }
                if let Some(Ok(text)) = &w.output
                    && ui.button(t!("Copy")).clicked()
                {
                    cmds.push(Command::Copy(text.clone()));
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| match &w.output {
                    Some(Ok(text)) => markdown(ui, text),
                    Some(Err(e)) => {
                        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                    }
                    None => {}
                });
        });
    if !open && let Some((_, cancel)) = &w.running {
        cancel.store(true, Ordering::Relaxed);
    }
    open
}

/// Shows Markdown-like text: headings, lists, code blocks, `code` and **bold**.
pub fn markdown(ui: &mut egui::Ui, text: &str) {
    let mut in_code = false;
    let mut code = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(code.trim_end()).monospace());
                });
                code.clear();
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(h) = trimmed.strip_prefix('#') {
            let level = 1 + h.chars().take_while(|c| *c == '#').count();
            let size = match level {
                1 => 20.0,
                2 => 17.0,
                _ => 15.0,
            };
            ui.add_space(4.0);
            ui.label(
                RichText::new(h.trim_start_matches('#').trim())
                    .size(size)
                    .strong(),
            );
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        let indent = (line.len() - trimmed.len()) as f32 * 4.0;
        let (bullet, rest) = match trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            Some(rest) => ("•  ", rest),
            None => ("", trimmed),
        };
        ui.horizontal_wrapped(|ui| {
            ui.add_space(indent);
            ui.spacing_mut().item_spacing.x = 0.0;
            if !bullet.is_empty() {
                ui.label(bullet);
            }
            inline(ui, rest);
        });
    }
    if in_code && !code.is_empty() {
        ui.label(RichText::new(code.trim_end()).monospace());
    }
}

/// One line with `code` and **bold** spans.
fn inline(ui: &mut egui::Ui, text: &str) {
    for (i, part) in text.split('`').enumerate() {
        if i % 2 == 1 {
            ui.label(RichText::new(part).monospace());
            continue;
        }
        for (j, piece) in part.split("**").enumerate() {
            if piece.is_empty() {
                continue;
            }
            if j % 2 == 1 {
                ui.label(RichText::new(piece).strong());
            } else {
                ui.label(piece);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn reviews_the_branch_diff() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.name", "T"],
            &["config", "user.email", "t@e.x"],
            &["config", "core.autocrlf", "false"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git.run(args).unwrap();
        }
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-qm", "first"]).unwrap();
        git.run(&["checkout", "-qb", "feature"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\nadded line\n").unwrap();
        git.run(&["commit", "-qam", "second"]).unwrap();
        let cancel = AtomicBool::new(false);
        // `grep` returns the matching line of the prompt (the diff).
        let out = run(
            dir.path(),
            "grep '^+added'",
            "Review:\n{diff}",
            "main",
            "feature",
            &cancel,
        )
        .unwrap();
        assert_eq!(out, "+added line");
        assert!(
            run(dir.path(), "cat", "{diff}", "feature", "main", &cancel).is_err(),
            "nothing new on main"
        );
        assert!(run(dir.path(), "", "{diff}", "main", "feature", &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(run(dir.path(), "sleep 5", "{diff}", "main", "feature", &cancel).is_err());
    }
}
