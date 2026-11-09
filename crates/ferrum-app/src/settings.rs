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
    /// Dark with stronger contrast.
    HighContrast,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 4] = [
        ThemeChoice::System,
        ThemeChoice::Light,
        ThemeChoice::Dark,
        ThemeChoice::HighContrast,
    ];

    pub fn label(self) -> String {
        match self {
            ThemeChoice::System => t!("System"),
            ThemeChoice::Light => t!("Light"),
            ThemeChoice::Dark => t!("Dark"),
            ThemeChoice::HighContrast => t!("High contrast"),
        }
    }
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

/// A login on a hosted service. Tokens are kept in the app's settings file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeAccount {
    pub kind: ForgeKind,
    pub host: String,
    pub username: String,
    pub token: String,
    /// Bitbucket Cloud: the username for an app password. Azure DevOps: the organization.
    #[serde(default)]
    pub user: String,
    /// Show this account's GitHub notifications in the toolbar.
    #[serde(default = "yes")]
    pub notifications: bool,
}

fn yes() -> bool {
    true
}

impl ForgeAccount {
    pub fn credentials(&self) -> ferrum_core::forge::Credentials {
        ferrum_core::forge::Credentials {
            token: self.token.clone(),
            user: self.user.clone(),
        }
    }
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
    /// Appearance and behaviour (Settings > Appearance, Git).
    pub prefs: crate::prefs::Prefs,
    /// Editor, terminal, diff and merge tools (Settings > Tools).
    pub external: crate::external::ExternalTools,
    /// Sidebar state per repository (pinned refs, open folders, sort).
    #[serde(default)]
    pub repos: std::collections::BTreeMap<String, crate::ui::sidebar_model::RepoPrefs>,
    pub background: BackgroundSettings,
    /// History view options, global and per repository.
    pub history: crate::ui::history_model::HistorySettings,
    pub commit: crate::commit_prefs::CommitPrefs,
    pub ai: crate::commit_prefs::AiPrefs,
    #[serde(default)]
    pub diff: crate::ui::diff_prefs::DiffPrefs,
    /// Branch, tag and stash dialog choices.
    #[serde(default)]
    pub refs: crate::ui::ref_dialogs::RefPrefs,
    /// Submodule options (Settings > Git).
    #[serde(default)]
    pub submodules: crate::repo_parts::SubmodulePrefs,
    /// Fetch, pull and push options and the automatic fetch.
    #[serde(default)]
    pub sync: crate::sync::SyncSettings,
    /// Commit details: active tab, file list mode, collapsed pane.
    #[serde(default)]
    pub details: crate::ui::details_model::DetailsSettings,
    /// Repository Manager list, source folders, workspaces, tab looks.
    #[serde(default)]
    pub library: crate::repos::Library,
    /// Keyboard shortcut overrides (Settings > Shortcuts), by action id.
    #[serde(default)]
    pub keymap: crate::keymap::Overrides,
    /// Custom commands and approved shared ones.
    #[serde(default)]
    pub custom: crate::custom_commands::CustomSettings,
    /// Hosted services: remote per repository, issue searches, review prompt.
    #[serde(default)]
    pub forge: crate::hosted::ForgePrefs,
    /// Command palette entries run last, newest first.
    #[serde(default)]
    pub palette_recent: Vec<String>,
}

/// File watching, refreshes, notifications and update checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundSettings {
    /// Watch the working folder for changes made by other programs.
    pub watch_files: bool,
    /// How often to look for changes when not watching, in seconds.
    pub poll_secs: u32,
    /// Reload everything when the window gets the focus back.
    pub refresh_on_focus: bool,
    /// Desktop notification when a long operation finishes in the background.
    pub notifications: bool,
    /// Look for a new release on GitHub once a day.
    pub check_updates: bool,
    /// Unix time of the last update check.
    pub last_update_check: u64,
    /// Newer release found by the last check: (version, page URL).
    pub update_available: Option<(String, String)>,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            watch_files: true,
            poll_secs: 3,
            refresh_on_focus: true,
            notifications: true,
            check_updates: true,
            last_update_check: 0,
            update_available: None,
        }
    }
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
            prefs: Default::default(),
            external: Default::default(),
            repos: Default::default(),
            background: BackgroundSettings::default(),
            history: Default::default(),
            commit: Default::default(),
            ai: Default::default(),
            diff: Default::default(),
            refs: Default::default(),
            submodules: Default::default(),
            sync: Default::default(),
            details: Default::default(),
            library: Default::default(),
            keymap: Default::default(),
            custom: Default::default(),
            forge: Default::default(),
            palette_recent: Vec::new(),
        }
    }
}

impl Settings {
    pub fn add_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
    }

    /// Token for a forge host: the first saved account for it, or the GitHub CLI login.
    pub fn token_for(
        &self,
        kind: ForgeKind,
        host: &str,
    ) -> Option<ferrum_core::forge::Credentials> {
        self.accounts
            .iter()
            .find(|a| a.kind == kind && a.host.eq_ignore_ascii_case(host))
            .map(ForgeAccount::credentials)
            .or_else(|| {
                (kind == ForgeKind::GitHub)
                    .then(|| ferrum_core::forge::gh_cli_token(host))
                    .flatten()
                    .map(|t| ferrum_core::forge::Credentials::token(&t))
            })
    }
}
