//! AI in the app: the provider from Settings > AI, the one path every
//! request takes (off unless enabled, context capped, secrets masked,
//! answer streamed), what views ask for (`Command::Ai`) and the split
//! into commits (`Op::Ai`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use ferrum_core::ai::prompts::DiffOf;
use ferrum_core::ai::split::{self, Changes, Group};
use ferrum_core::ai::{self, AiError, Destination, Prompt, Provider};
use ferrum_core::cli::Git;
use ferrum_core::secrets::{self, Allowlist, Scanner};
use ferrum_core::{Oid, short};

use crate::command::Op;
use crate::commit_prefs::{AiPrefs, AiProvider};
use crate::icons;
use crate::jobs::{JobOutput, JobResult, Refresh};
use crate::settings::Settings;
use crate::tab::{RepoTab, UndoEntry};

/// What a request needs, copied from the settings so it can run on a thread.
#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub provider: Provider,
    /// Most bytes of context.
    pub max: usize,
    pub secrets: secrets::Config,
}

impl Config {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            enabled: settings.ai.enabled,
            provider: provider(&settings.ai),
            max: settings.ai.max_kb.clamp(4, 2048) as usize * 1024,
            secrets: settings.safety.secret_rules.clone(),
        }
    }
}

pub fn provider(p: &AiPrefs) -> Provider {
    match p.provider {
        AiProvider::Command => Provider::Command {
            command: p.command.clone(),
        },
        AiProvider::Ollama => Provider::Ollama {
            url: p.ollama_url.clone(),
            model: p.ollama_model.clone(),
        },
        AiProvider::OpenAi => Provider::OpenAi {
            url: p.openai_url.clone(),
            key: p.openai_key.clone(),
            model: p.openai_model.clone(),
        },
    }
}

/// The language answers are asked in: the one of the interface.
pub fn language() -> &'static str {
    if crate::i18n::portuguese() {
        "Brazilian Portuguese"
    } else {
        "English"
    }
}

// ----- Where data goes -----

/// An icon, a short label ("Local · llama3.1") and a sentence.
pub struct Where {
    pub icon: &'static str,
    pub short: String,
    pub long: String,
    pub local: bool,
}

pub fn where_to(p: &Provider) -> Where {
    let model = p.model().filter(|m| !m.trim().is_empty());
    let with_model = |s: String| match model {
        Some(m) => format!("{s} · {m}"),
        None => s,
    };
    let kind = match p {
        Provider::Ollama { .. } => "Ollama",
        Provider::OpenAi { .. } => "OpenAI API",
        Provider::Command { .. } => "",
    };
    match p.destination() {
        Destination::Local { host } => {
            let this_computer =
                matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "0.0.0.0")
                    || host.starts_with("127.");
            let long = if matches!(p, Provider::Command { .. }) {
                t!("Sent to a model that runs on this computer. Nothing leaves your machine.")
            } else if this_computer {
                t!(
                    "Sent to {} on this computer ({}). Nothing leaves your machine.",
                    kind,
                    host
                )
            } else {
                t!("Sent to {} on your local network ({}).", kind, host)
            };
            Where {
                icon: icons::LOCK,
                short: with_model(t!("Local")),
                long,
                local: true,
            }
        }
        Destination::Remote { name } => Where {
            icon: icons::CLOUD,
            short: with_model(t!("Cloud · {}", name)),
            long: t!("Sent over the internet to {}.", name),
            local: false,
        },
        Destination::Tool {
            program,
            service: Some(service),
        } => Where {
            icon: icons::CLOUD,
            short: t!("Cloud · {}", service),
            long: t!(
                "Sent to the {} command, which sends it to {}.",
                program,
                service
            ),
            local: false,
        },
        Destination::Tool {
            program,
            service: None,
        } => Where {
            icon: icons::TERMINAL_WINDOW,
            short: program.clone(),
            long: t!(
                "Sent to the {} command; where it goes from there depends on the tool.",
                program
            ),
            local: false,
        },
    }
}

pub fn error_text(e: &AiError) -> String {
    match e {
        AiError::Missing("model") => t!("Choose a model in Settings > AI."),
        AiError::Missing(_) => t!("Set the AI command in Settings first."),
        AiError::Start { program, detail } => t!("Could not start {}: {}", program, detail),
        AiError::Failed { program, detail } => t!("{} failed: {}", program, detail),
        AiError::Connect { url, detail } => t!("Could not connect to {}: {}", url, detail),
        AiError::Http { status, detail } => t!("The server answered {}: {}", status, detail),
        AiError::BadReply(d) => t!("Unexpected answer from the model: {}", d),
        AiError::Empty => t!("The AI tool returned an empty answer."),
        AiError::Cancelled => t!("Cancelled."),
    }
}

// ----- Requests -----

/// What went out with a request.
#[derive(Debug, Clone, Default)]
pub struct Sent {
    pub bytes: usize,
    /// Lines with secrets that were masked.
    pub masked: usize,
    pub kinds: Vec<String>,
    /// The context was cut to the size limit.
    pub cut: bool,
}

#[derive(Default)]
struct Shared {
    text: String,
    sent: Option<Sent>,
    done: Option<Result<(), String>>,
}

/// A request running on its own thread; its answer grows as it streams.
pub struct Stream {
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
}

impl Stream {
    /// `build` reads the context (git commands are fine: it runs on the
    /// thread). Nothing is sent when AI is off.
    pub fn start(
        ctx: &egui::Context,
        cfg: &Config,
        build: impl FnOnce() -> Result<Prompt, String> + Send + 'static,
    ) -> Self {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let (out, flag, ctx, cfg) = (shared.clone(), cancel.clone(), ctx.clone(), cfg.clone());
        std::thread::spawn(move || {
            let result = run(&cfg, build, &flag, &out, &ctx);
            lock(&out).done = Some(result);
            ctx.request_repaint();
        });
        Self { shared, cancel }
    }

    /// The answer so far.
    pub fn text(&self) -> String {
        lock(&self.shared).text.clone()
    }

    pub fn sent(&self) -> Option<Sent> {
        lock(&self.shared).sent.clone()
    }

    pub fn running(&self) -> bool {
        lock(&self.shared).done.is_none()
    }

    /// The whole answer (or the error) once it has finished.
    pub fn finished(&self) -> Option<Result<String, String>> {
        let s = lock(&self.shared);
        s.done.as_ref().map(|r| r.clone().map(|()| s.text.clone()))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Masks secrets and caps the request (the system prompt is ours).
pub fn prepare(cfg: &Config, mut prompt: Prompt) -> (Prompt, Sent) {
    let (scanner, _) = Scanner::new(&cfg.secrets, Allowlist::default());
    let r = ai::redact::redact(&scanner, "", &prompt.user);
    let (user, cut) = ai::cap(&r.text, cfg.max);
    prompt.user = user;
    let sent = Sent {
        bytes: prompt.len(),
        masked: r.masked,
        kinds: r.kinds,
        cut,
    };
    (prompt, sent)
}

fn run(
    cfg: &Config,
    build: impl FnOnce() -> Result<Prompt, String>,
    cancel: &AtomicBool,
    out: &Mutex<Shared>,
    ctx: &egui::Context,
) -> Result<(), String> {
    if !cfg.enabled {
        return Err(t!("AI is off. Turn it on in Settings > AI."));
    }
    let prompt = build()?;
    let (prompt, sent) = prepare(cfg, prompt);
    lock(out).sent = Some(sent);
    ctx.request_repaint();
    ai::complete(&cfg.provider, &prompt, cancel, &mut |piece| {
        lock(out).text.push_str(piece);
        ctx.request_repaint();
    })
    .map(drop)
    .map_err(|e| error_text(&e))
}

// ----- Commands -----

/// What explain is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Commit(Oid),
    Diff(DiffOf),
    History(String),
}

impl Subject {
    pub fn title(&self, tab: &RepoTab) -> String {
        match self {
            Subject::Commit(id) => {
                let summary = tab
                    .repo
                    .find_commit_info(*id)
                    .map(|c| c.summary)
                    .unwrap_or_default();
                t!("Commit {}: {}", short(*id), summary)
            }
            Subject::Diff(DiffOf::Local { path, staged: true }) => t!("Staged changes of {}", path),
            Subject::Diff(DiffOf::Local { path, .. }) => t!("Local changes of {}", path),
            Subject::Diff(DiffOf::Commit { id, path }) => t!("{} in commit {}", path, short(*id)),
            Subject::History(path) => t!("History of {}", path),
        }
    }

    /// Builds the prompt (runs git).
    pub fn prompt(
        &self,
        dir: PathBuf,
        max: usize,
    ) -> impl FnOnce() -> Result<Prompt, String> + Send + 'static {
        let subject = self.clone();
        move || {
            let git = Git::new(&dir);
            let lang = language();
            match &subject {
                Subject::Commit(id) => ai::prompts::explain_commit(&git, *id, lang, max),
                Subject::Diff(of) => ai::prompts::explain_diff(&git, of, lang, max),
                Subject::History(path) => ai::prompts::explain_history(&git, path, lang, max),
            }
            .map_err(|e| e.to_string())
        }
    }
}

pub enum AiCmd {
    /// Open the explain panel and start explaining.
    Explain(Subject),
    /// Settings > AI.
    OpenSettings,
    /// The split into commits window for the active repository.
    Split,
    /// `FERRUM_START` step (see `app/ai.rs`).
    Dev(String),
}

// ----- Split into commits -----

#[derive(Debug, Clone)]
pub enum AiOp {
    /// Commit the groups one by one.
    Split {
        changes: Box<Changes>,
        groups: Vec<Group>,
        secrets: secrets::Config,
        allow: Allowlist,
    },
}

impl AiOp {
    pub fn label(&self) -> String {
        match self {
            AiOp::Split { groups, .. } => {
                let n = groups
                    .iter()
                    .filter(|g| !g.items.is_empty() && !g.message.trim().is_empty())
                    .count();
                t!("Split the changes into {} commits", n)
            }
        }
    }

    pub fn refresh(&self) -> Refresh {
        Refresh::All
    }

    pub fn execute(&self, git: &Git, progress: &dyn Fn(&str)) -> JobResult {
        self.run(git, progress)
            .map(|()| JobOutput::Done)
            .map_err(|e| e.to_string())
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        match self {
            AiOp::Split {
                changes,
                groups,
                secrets,
                allow,
            } => {
                let (scanner, _) = Scanner::new(secrets, allow.clone());
                let check = |n: usize| -> ferrum_core::Result<()> {
                    let repo = ferrum_core::Repo::open(git.dir())?;
                    let found = scanner.scan_staged(&repo)?;
                    if let Some(f) = found.first() {
                        let message = groups[n].message.lines().next().unwrap_or_default();
                        return Err(ferrum_core::Error::Other(t!(
                            "Stopped before committing \"{}\": {} in {}. The commits before it were made; the rest are still local changes.",
                            message,
                            f.name,
                            f.path
                        )));
                    }
                    Ok(())
                };
                split::apply(git, changes, groups, check, |m| progress(m)).map(drop)
            }
        }
    }

    /// Undo: back to the commit it started from, changes kept.
    pub fn undo(&self, tab: &RepoTab) -> Option<UndoEntry> {
        Some(UndoEntry {
            label: self.label(),
            ops: vec![Op::Reset {
                rev: tab.head_oid?.to_string(),
                mode: ferrum_core::ops::ResetMode::Mixed,
            }],
            seq: 0,
        })
    }
}
