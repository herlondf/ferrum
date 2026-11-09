//! Restore points, the secrets warning and learning mode in the main window
//! (see `safety.rs`, `timeline.rs`), and `Command::Safety`.

use std::path::PathBuf;

use eframe::egui;
use ferrum_core::restore;

use super::FerrumApp;
use crate::command::{Command, Commands, Op};
use crate::safety::{Held, SafetyCmd};
use crate::timeline::{Config, UndoChoice};
use crate::ui::learning::{self, LearningPanel};
use crate::ui::secrets::Answer;
use crate::ui::timeline::{self as timeline_ui, RestoreConfirm, TimelineWindow};
use crate::ui::toasts::ToastAction;

#[derive(Default)]
pub struct State {
    pub timeline: TimelineWindow,
    confirm: Option<RestoreConfirm>,
    /// A commit or push stopped by the secret check, and its repository.
    held: Option<(PathBuf, Held)>,
    learning: LearningPanel,
}

impl FerrumApp {
    fn tab_index(&self, repo: &std::path::Path) -> Option<usize> {
        self.tabs.iter().position(|t| t.path() == repo)
    }

    /// Every frame, before the views: settings the tabs and views need.
    pub(super) fn safety_tick(&mut self, ctx: &egui::Context) {
        let config = Config::from_settings(&self.settings.safety);
        for t in &mut self.tabs {
            if t.timeline.config != config {
                t.timeline.config = config.clone();
            }
        }
        learning::set_enabled(ctx, self.settings.safety.learning);
    }

    /// After the toasts saw the new messages: operations that saved a
    /// restore point get an Undo button.
    pub(super) fn safety_toasts(&mut self) {
        for t in &mut self.tabs {
            if let Some(p) = t.timeline.fresh.take() {
                let action = ToastAction::Restore {
                    repo: t.path().to_path_buf(),
                    id: p.id,
                };
                self.toasts.attach(t!("Undo"), action);
            }
        }
    }

    pub(super) fn safety_windows(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        let tab = self
            .tabs
            .get(self.active)
            .filter(|_| !self.repos.home_active);
        timeline_ui::show(ctx, &mut self.safety.timeline, tab, &self.settings, cmds);
        if self.settings.safety.learning {
            let repo = tab.map(|t| t.path().to_path_buf());
            learning::show(ctx, &mut self.safety.learning, repo.as_deref(), cmds);
        }
        if let Some(c) = &mut self.safety.confirm {
            let keep = match self.tabs.iter().find(|t| t.path() == c.repo) {
                Some(t) => timeline_ui::confirm(ctx, c, t, cmds),
                None => false,
            };
            if !keep {
                self.safety.confirm = None;
            }
        }
        if let Some((_, held)) = &self.safety.held {
            crate::ui::secrets::show(ctx, held, cmds);
        }
    }

    /// Ctrl+Z: the quick undo, or a restore of the newest point if it is newer.
    pub(super) fn undo(&mut self, ctx: &egui::Context) {
        let Some(t) = self.tabs.get_mut(self.active) else {
            return;
        };
        if t.busy() {
            return;
        }
        match crate::timeline::undo_choice(t) {
            UndoChoice::Quick => t.run_undo(ctx),
            UndoChoice::Point(p) => {
                let repo = t.path().to_path_buf();
                self.handle_safety(SafetyCmd::ConfirmRestore(repo, p.id), ctx);
            }
            UndoChoice::Nothing => t.info(t!("Nothing to undo.")),
        }
    }

    /// A job stopped because of secrets: show the warning.
    pub(super) fn secrets_found(&mut self, tab: Option<usize>, held: Held) {
        if let Some(t) = tab.and_then(|i| self.tabs.get(i)) {
            self.safety.held = Some((t.path().to_path_buf(), held));
        }
    }

    pub(super) fn handle_safety(&mut self, cmd: SafetyCmd, ctx: &egui::Context) {
        match cmd {
            SafetyCmd::ToggleTimeline => self.safety.timeline.toggle(),
            SafetyCmd::ConfirmRestore(repo, id) => {
                let Some(i) = self.tab_index(&repo) else {
                    return;
                };
                self.active = i;
                self.repos.home_active = false;
                let t = &mut self.tabs[i];
                match restore::load(&t.repo, &id) {
                    Ok(point) => self.safety.confirm = Some(RestoreConfirm::new(t, point)),
                    Err(e) => {
                        t.timeline.forget_id(&id);
                        t.error(t!("This restore point is gone: {}", e));
                    }
                }
            }
            SafetyCmd::ToggleLearning => {
                let s = &mut self.settings.safety;
                if s.learning && self.safety.learning.hidden {
                    self.safety.learning.hidden = false;
                } else {
                    s.learning = !s.learning;
                    self.safety.learning.hidden = false;
                }
            }
            SafetyCmd::Secrets(a) => self.answer_secrets(a, ctx),
            SafetyCmd::Dev(step) => self.safety_dev_step(&step, ctx),
        }
    }

    fn answer_secrets(&mut self, a: Answer, ctx: &egui::Context) {
        let Some((repo, held)) = self.safety.held.take() else {
            return;
        };
        let Some(i) = self.tab_index(&repo) else {
            return;
        };
        let t = &mut self.tabs[i];
        match a {
            Answer::Cancel => {
                let text = if held.push {
                    t!("Nothing was pushed.")
                } else {
                    t!("Nothing was committed.")
                };
                t.info(text);
            }
            Answer::Unstage => {
                let mut paths: Vec<String> = held.findings.iter().map(|f| f.path.clone()).collect();
                paths.sort();
                paths.dedup();
                let op = if held.amend {
                    Op::UnstageAmend(paths)
                } else {
                    Op::Unstage(paths)
                };
                t.run(op, ctx);
            }
            Answer::IgnoreOnce | Answer::Allow => {
                if a == Answer::Allow {
                    self.settings.safety.allow(&self.tabs[i], &held.findings);
                }
                let t = &mut self.tabs[i];
                for op in crate::safety::unchecked(held.ops) {
                    if matches!(op, Op::Commit { .. }) {
                        t.changes.committing = true;
                    }
                    self.toasts.started(t, &op);
                    t.run(op, ctx);
                }
            }
        }
    }

    /// `FERRUM_START` steps: `timeline`, `restore:N` (N-th point),
    /// `learning`, `secrets:scan` (the warning for the staged changes),
    /// `discard-all` (discards the unstaged changes: a point and its toast).
    fn safety_dev_step(&mut self, step: &str, ctx: &egui::Context) {
        let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
        let Some(t) = self.tabs.get(self.active) else {
            return;
        };
        match cmd {
            "timeline" => self.safety.timeline.open = true,
            "learning" => self.settings.safety.learning = true,
            // Discards every unstaged change: a restore point and its toast.
            "discard-all" => {
                let files = t.status.unstaged.clone();
                self.handle(Command::Run(Op::Discard(files)), ctx);
            }
            "restore" => {
                let n = arg.parse::<usize>().unwrap_or(0);
                let points = restore::list(&t.repo).unwrap_or_default();
                if let Some(p) = points.get(n) {
                    let repo = t.path().to_path_buf();
                    self.handle_safety(SafetyCmd::ConfirmRestore(repo, p.id.clone()), ctx);
                }
            }
            "secrets" => {
                let s = &self.settings.safety;
                let (scanner, _) =
                    ferrum_core::secrets::Scanner::new(&s.secret_rules, s.allowlist(t));
                let findings = scanner.scan_staged(&t.repo).unwrap_or_default();
                if !findings.is_empty() {
                    let held = Held {
                        findings,
                        ops: Vec::new(),
                        push: arg == "push",
                        amend: false,
                    };
                    self.safety.held = Some((t.path().to_path_buf(), held));
                }
            }
            _ => eprintln!("FERRUM_START: unknown step {step}"),
        }
    }
}

/// Command for a `FERRUM_START` step of this area.
pub fn dev_command(step: &str) -> Command {
    Command::Safety(SafetyCmd::Dev(step.into()))
}
