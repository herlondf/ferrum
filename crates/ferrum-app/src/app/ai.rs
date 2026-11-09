//! The explain panel and the split window in the main window, and
//! `Command::Ai` (see `ai.rs`).

use eframe::egui;
use ferrum_core::ai::prompts::DiffOf;

use super::FerrumApp;
use crate::ai::{AiCmd, Subject};
use crate::command::{Command, Commands, DevStep};
use crate::ui::ai_panel::{self, ExplainPanel};
use crate::ui::ai_split::{self, SplitWindow};

#[derive(Default)]
pub struct State {
    pub panel: ExplainPanel,
    split: Option<SplitWindow>,
}

impl FerrumApp {
    /// Every frame, before the views.
    pub(super) fn ai_tick(&mut self, ctx: &egui::Context) {
        crate::ui::ai_common::publish(ctx, &self.settings);
    }

    /// The explain panel of the active repository, on the right.
    pub(super) fn ai_panel(&mut self, ui: &mut egui::Ui, cmds: &mut Commands) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if !self.ai.panel.shows(tab.path()) {
            return;
        }
        egui::Panel::right("ai-explain")
            .resizable(true)
            .default_size(400.0)
            .min_size(280.0)
            .show(ui, |ui| ai_panel::show(ui, &mut self.ai.panel, cmds));
    }

    pub(super) fn ai_windows(&mut self, ctx: &egui::Context, cmds: &mut Commands) {
        if let Some(w) = &mut self.ai.split {
            let tab = self.tabs.iter().find(|t| t.path() == w.repo);
            if !ai_split::show(ctx, w, tab, &self.settings, cmds) {
                self.ai.split = None;
            }
        }
    }

    pub(super) fn handle_ai(&mut self, cmd: AiCmd, ctx: &egui::Context) {
        match cmd {
            AiCmd::Explain(subject) => {
                if let Some(t) = self.tabs.get(self.active) {
                    self.ai.panel.explain(ctx, t, subject);
                }
            }
            AiCmd::OpenSettings => {
                self.handle(Command::DevStep(DevStep::SettingsSection("ai".into())), ctx);
            }
            AiCmd::Split => {
                if let Some(t) = self.tabs.get(self.active) {
                    self.ai.split = Some(SplitWindow::new(ctx, t));
                }
            }
            AiCmd::Dev(step) => self.ai_dev_step(&step, ctx),
        }
    }

    /// `ai:explain=N` (commit N), `ai:diff=PATH`, `ai:history=PATH`, `ai:split`,
    /// `ai:suggest` (conflict help for the open file),
    /// `ai:ollama=URL,MODEL` (turns AI on with that server), `ai:search=TEXT`
    /// (the Ask search mode, read).
    fn ai_dev_step(&mut self, step: &str, ctx: &egui::Context) {
        let (what, arg) = step.split_once('=').unwrap_or((step, ""));
        if what == "ollama" {
            let (url, model) = arg.split_once(',').unwrap_or((arg, ""));
            let ai = &mut self.settings.ai;
            ai.enabled = true;
            ai.provider = crate::commit_prefs::AiProvider::Ollama;
            ai.ollama_url = url.into();
            ai.ollama_model = model.into();
            crate::ui::ai_common::publish(ctx, &self.settings);
            return;
        }
        if what == "search" {
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.history.ext.mode = crate::ui::history_model::SearchMode::Ask;
                t.history.search = arg.into();
                crate::ui::ai_search::interpret(t, ctx);
            }
            return;
        }
        let Some(t) = self.tabs.get(self.active) else {
            return;
        };
        let subject = match what {
            "explain" => {
                let Some(c) = t.commits.get(arg.parse::<usize>().unwrap_or(0)) else {
                    return;
                };
                Subject::Commit(c.id)
            }
            "diff" => Subject::Diff(DiffOf::Local {
                path: arg.into(),
                staged: false,
            }),
            "history" => Subject::History(arg.into()),
            "split" => return self.handle_ai(AiCmd::Split, ctx),
            "suggest" => return crate::ui::ai_conflict::request(ctx),
            _ => {
                eprintln!("FERRUM_START: unknown ai step {step}");
                return;
            }
        };
        self.handle_ai(AiCmd::Explain(subject), ctx);
    }
}
