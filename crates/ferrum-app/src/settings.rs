//! Settings saved between sessions.

use std::path::PathBuf;

use ferrum_core::forge::ForgeKind;
use serde::{Deserialize, Serialize};

pub const STORAGE_KEY: &str = "ferrum-settings";
const MAX_RECENT: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PullChoice {
    /// Follow the repository's git config.
    #[default]
    Default,
    Merge,
    Rebase,
    FastForwardOnly,
}

impl PullChoice {
    pub fn mode(self) -> ferrum_core::ops::PullMode {
        use ferrum_core::ops::PullMode;
        match self {
            PullChoice::Default => PullMode::Default,
            PullChoice::Merge => PullMode::Merge,
            PullChoice::Rebase => PullMode::Rebase,
            PullChoice::FastForwardOnly => PullMode::FastForwardOnly,
        }
    }

    pub fn label(self) -> String {
        match self {
            PullChoice::Default => t!("Default (git config)"),
            PullChoice::Merge => t!("Merge"),
            PullChoice::Rebase => t!("Rebase"),
            PullChoice::FastForwardOnly => t!("Fast-forward only"),
        }
    }
}

/// A named identity that can be applied to a repository (e.g. personal, work).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub label: String,
    pub name: String,
    pub email: String,
    /// GPG key id, or path to an SSH public key, used to sign commits.
    pub signing_key: String,
    pub sign_commits: bool,
    /// Private SSH key used for this repository's remotes.
    pub ssh_key: String,
}

impl Profile {
    pub fn identity(&self) -> ferrum_core::Identity {
        let opt = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        ferrum_core::Identity {
            name: opt(&self.name),
            email: opt(&self.email),
            signing_key: opt(&self.signing_key),
            gpg_sign: self.sign_commits && !self.signing_key.trim().is_empty(),
            ssh_command: opt(&self.ssh_key)
                .map(|k| format!("ssh -i \"{}\" -o IdentitiesOnly=yes", k.replace('\\', "/"))),
        }
    }
}

/// A GitHub or GitLab login. Tokens are kept in the app's settings file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeAccount {
    pub kind: ForgeKind,
    pub host: String,
    pub username: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: crate::i18n::Language,
    pub theme: ThemeChoice,
    pub zoom: f32,
    pub pull: PullChoice,
    pub open_repos: Vec<PathBuf>,
    pub active_repo: usize,
    pub recent: Vec<PathBuf>,
    pub profiles: Vec<Profile>,
    pub accounts: Vec<ForgeAccount>,
    /// Folder suggested when cloning.
    pub clone_dir: String,
    /// Sidebar state per repository (pinned refs, open folders, sort).
    #[serde(default)]
    pub repos: std::collections::BTreeMap<String, crate::ui::sidebar_model::RepoPrefs>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: crate::i18n::Language::System,
            theme: ThemeChoice::System,
            zoom: 1.0,
            pull: PullChoice::Default,
            open_repos: Vec::new(),
            active_repo: 0,
            recent: Vec::new(),
            profiles: Vec::new(),
            accounts: Vec::new(),
            clone_dir: String::new(),
            repos: Default::default(),
        }
    }
}

impl Settings {
    pub fn add_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }

    /// Token for a forge host: a saved account, or the GitHub CLI login.
    pub fn token_for(&self, kind: ForgeKind, host: &str) -> Option<String> {
        self.accounts
            .iter()
            .find(|a| a.kind == kind && a.host.eq_ignore_ascii_case(host))
            .map(|a| a.token.clone())
            .or_else(|| {
                (kind == ForgeKind::GitHub)
                    .then(|| ferrum_core::forge::gh_cli_token(host))
                    .flatten()
            })
    }
}
