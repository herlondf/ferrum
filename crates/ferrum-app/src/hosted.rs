//! Hosted services (GitHub, GitLab, Bitbucket, Azure DevOps, Gitea): saved
//! preferences, which remote a repository uses for pull requests and issues,
//! and small background requests that are not git jobs.

use std::collections::BTreeMap;
use std::sync::RwLock;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use ferrum_core::forge::{self, Credentials, HostedRepo, IssueFilter};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;
use crate::tab::RepoTab;

pub const DEFAULT_REVIEW_PROMPT: &str = "Review the following changes as an experienced code reviewer. \
List bugs, risky changes, missing tests and unclear code, most important first, with the file and line. \
Be concise; skip praise. Answer in Markdown.\n\n{diff}";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ForgePrefs {
    /// Remote used for pull requests and issues, per repository (shared key).
    pub remotes: BTreeMap<String, String>,
    /// Last issue searches, newest first.
    pub recent_issue_searches: Vec<IssueFilter>,
    /// Prompt of the AI code review; `{diff}` is replaced by the diff.
    pub review_prompt: String,
    /// Ask for HTTPS passwords and SSH passphrases in a window.
    pub askpass: bool,
    /// Remember answers to prompts until Ferrum closes.
    pub remember_answers: bool,
    /// SSH private keys outside `~/.ssh` shown in the key manager.
    pub extra_keys: Vec<String>,
    /// How often GitHub notifications are checked, in minutes.
    pub notification_minutes: u32,
}

impl Default for ForgePrefs {
    fn default() -> Self {
        Self {
            remotes: BTreeMap::new(),
            recent_issue_searches: Vec::new(),
            review_prompt: DEFAULT_REVIEW_PROMPT.into(),
            askpass: true,
            remember_answers: false,
            extra_keys: Vec::new(),
            notification_minutes: 5,
        }
    }
}

const MAX_RECENT_SEARCHES: usize = 8;

impl ForgePrefs {
    pub fn remember_search(&mut self, f: &IssueFilter) {
        if f.is_default() {
            return;
        }
        self.recent_issue_searches.retain(|s| s != f);
        self.recent_issue_searches.insert(0, f.clone());
        self.recent_issue_searches.truncate(MAX_RECENT_SEARCHES);
    }
}

/// What views ask the app to do about hosted services.
pub enum HostedCmd {
    /// Open the AI code review window for `base...head`.
    Review {
        base: String,
        head: String,
    },
    ToggleNotifications,
    /// Read a server's host keys and offer to trust them.
    TrustHost {
        host: String,
        port: Option<u16>,
    },
    /// The first-time SSH setup assistant.
    SshSetup,
    /// Use this remote for pull requests and issues in the active repository.
    SetRemote(String),
    /// Open the clone dialog with this URL.
    Clone(String),
    /// `FERRUM_START` step (see `app/hosted.rs`).
    Dev(String),
}

// ----- Which remote -----

/// Repository key -> preferred remote, copied from the settings so the
/// background reload (which has no settings) can use it.
static PREFERRED: RwLock<BTreeMap<String, String>> = RwLock::new(BTreeMap::new());

/// Shares the accounts' hosts and the preferred remotes with the reloads.
/// Adds to what is known (removed accounts are forgotten at restart), so
/// several app instances in tests do not undo each other. Cheap when
/// nothing changed.
pub fn sync_registry(settings: &Settings) {
    let missing = |p: &BTreeMap<String, String>| {
        settings
            .forge
            .remotes
            .iter()
            .any(|(k, v)| p.get(k) != Some(v))
    };
    if PREFERRED.read().map(|p| missing(&p)).unwrap_or(false)
        && let Ok(mut p) = PREFERRED.write()
    {
        for (k, v) in &settings.forge.remotes {
            p.insert(k.clone(), v.clone());
        }
    }
    let mut known = forge::known_hosts();
    let mut changed = false;
    for a in &settings.accounts {
        let entry = (a.host.clone(), a.kind);
        if Some(a.host.as_str()) != a.kind.default_host() && !known.contains(&entry) {
            known.push(entry);
            changed = true;
        }
    }
    if changed {
        forge::set_known_hosts(known);
    }
}

/// Remotes of `tab` that point at a hosted repository.
pub fn hosted_remotes(tab: &RepoTab) -> Vec<(String, HostedRepo)> {
    tab.remotes
        .iter()
        .filter_map(|r| forge::parse_remote_url(&r.url).map(|h| (r.name.clone(), h)))
        .collect()
}

/// The hosted repository used for pull requests and issues: the chosen
/// remote, else `origin`, else the first hosted remote.
pub fn hosted_for(tab: &RepoTab) -> Option<HostedRepo> {
    let remotes = hosted_remotes(tab);
    let key = crate::repo_settings::shared_key(tab);
    let preferred = PREFERRED.read().ok().and_then(|p| p.get(&key).cloned());
    preferred
        .iter()
        .map(String::as_str)
        .chain(["origin"])
        .find_map(|name| remotes.iter().find(|(n, _)| n == name))
        .or_else(|| remotes.first())
        .map(|(_, h)| h.clone())
}

/// The remote whose URL is `tab.hosted`.
pub fn hosted_remote_name(tab: &RepoTab) -> Option<String> {
    let hosted = tab.hosted.as_ref()?;
    hosted_remotes(tab)
        .into_iter()
        .find(|(_, h)| h == hosted)
        .map(|(n, _)| n)
}

/// Lets the user pick the remote for pull requests and issues when the
/// repository has several hosted ones.
pub fn remote_picker(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut crate::command::Commands) {
    let remotes = hosted_remotes(tab);
    if remotes.len() < 2 {
        return;
    }
    let current = hosted_remote_name(tab).unwrap_or_default();
    egui::ComboBox::from_id_salt("hosted-remote")
        .selected_text(t!("Remote: {}", current))
        .show_ui(ui, |ui| {
            for (name, h) in &remotes {
                let label = format!("{name}  ({} {}/{})", h.kind.name(), h.host, h.path);
                if ui.selectable_label(*name == current, label).clicked() && *name != current {
                    cmds.push(crate::command::Command::Hosted(HostedCmd::SetRemote(
                        name.clone(),
                    )));
                }
            }
        })
        .response
        .on_hover_text(t!("Remote used for pull requests and issues"));
}

// ----- Credentials -----

pub fn credentials(settings: &Settings, repo: &HostedRepo) -> Option<Credentials> {
    settings.token_for(repo.kind, &repo.host)
}

// ----- Background requests -----

/// A request running on its own thread.
pub struct Pending<T>(Receiver<Result<T, String>>);

impl<T: Send + 'static> Pending<T> {
    pub fn spawn(
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        Self(rx)
    }

    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.0.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(t!("The request stopped."))),
        }
    }
}

/// Polls `slot`; when the request has finished, takes it out and returns
/// the result.
pub fn take_done<T: Send + 'static>(slot: &mut Option<Pending<T>>) -> Option<Result<T, String>> {
    let result = slot.as_ref()?.poll()?;
    *slot = None;
    Some(result)
}
