//! "Ferrum never lets you lose work": settings of the restore points (see
//! `timeline.rs`), the secret check before commits and pushes, and learning
//! mode (see `ui::learning`).
//!
//! The secret check runs inside the operation's job, right before the commit
//! or push, so it sees exactly what will be committed and never blocks the
//! window. When it finds something, the job stops and the app asks what to do
//! (`ui::secrets`).

use std::collections::BTreeMap;

use ferrum_core::cli::Git;
use ferrum_core::secrets::{self, Allowlist, Finding, Scanner};
use serde::{Deserialize, Serialize};

use crate::command::Op;
use crate::jobs::{JobOutput, JobResult};
use crate::settings::Settings;
use crate::tab::RepoTab;

/// Settings > Safety.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetySettings {
    /// Save a restore point before operations that rewrite or drop work.
    pub restore_points: bool,
    /// Restore points older than this are deleted.
    pub keep_days: u32,
    /// Save untracked files in restore points too.
    pub untracked: bool,
    /// Scan staged changes for secrets before committing.
    pub secrets: bool,
    /// Scan the commits being pushed.
    pub scan_push: bool,
    pub secret_rules: secrets::Config,
    /// Accepted secrets and paths, per repository (`repo_settings::shared_key`).
    pub allowlists: BTreeMap<String, Allowlist>,
    /// Show the git commands of each action, with explanations.
    pub learning: bool,
}

impl Default for SafetySettings {
    fn default() -> Self {
        Self {
            restore_points: true,
            keep_days: 14,
            untracked: true,
            secrets: true,
            scan_push: true,
            secret_rules: Default::default(),
            allowlists: BTreeMap::new(),
            learning: false,
        }
    }
}

impl SafetySettings {
    pub fn allowlist(&self, tab: &RepoTab) -> Allowlist {
        self.allowlists
            .get(&crate::repo_settings::shared_key(tab))
            .cloned()
            .unwrap_or_default()
    }

    pub fn allow(&mut self, tab: &RepoTab, findings: &[Finding]) {
        let list = self
            .allowlists
            .entry(crate::repo_settings::shared_key(tab))
            .or_default();
        for f in findings {
            list.add(f);
        }
    }
}

/// What views ask the app to do about restore points, secrets and learning mode.
pub enum SafetyCmd {
    /// Open or close the operation timeline.
    ToggleTimeline,
    /// Ask before restoring point `id` of the repository at the path.
    ConfirmRestore(std::path::PathBuf, String),
    /// Open or close the learning panel.
    ToggleLearning,
    /// The user's answer to the secrets warning.
    Secrets(crate::ui::secrets::Answer),
    /// `FERRUM_START` steps: `timeline`, `restore:N`, `learning`, `secrets:demo`.
    Dev(String),
}

/// An operation that runs only if no secret is found in what it adds.
#[derive(Debug, Clone)]
pub struct SecretCheck {
    pub op: Box<Op>,
    pub config: secrets::Config,
    pub allow: Allowlist,
}

/// A commit or push stopped by the check, with the operations queued after it.
#[derive(Debug, Clone)]
pub struct Held {
    pub findings: Vec<Finding>,
    pub ops: Vec<Op>,
    /// Found in commits about to be pushed, not in staged changes.
    pub push: bool,
    pub amend: bool,
}

/// Wraps a commit or push in a secret check when the settings ask for it.
pub fn wrap(op: Op, settings: &Settings, tab: &RepoTab) -> Op {
    let s = &settings.safety;
    let check = match &op {
        Op::Commit { .. } => s.secrets,
        Op::Push(_) | Op::PushRefs(_) => s.secrets && s.scan_push,
        _ => false,
    };
    if !check {
        return op;
    }
    Op::SecretCheck(SecretCheck {
        op: Box::new(op),
        config: s.secret_rules.clone(),
        allow: s.allowlist(tab),
    })
}

impl SecretCheck {
    /// The scan, then the operation when nothing was found.
    pub fn execute(&self, git: &Git, progress: &dyn Fn(&str)) -> JobResult {
        let findings = self.scan(git).map_err(|e| e.to_string())?;
        if findings.is_empty() {
            return self.op.execute(git, progress);
        }
        let push = !matches!(*self.op, Op::Commit { .. });
        let amend = matches!(*self.op, Op::Commit { amend: true, .. });
        Ok(JobOutput::Secrets(Box::new(Held {
            findings,
            ops: vec![(*self.op).clone()],
            push,
            amend,
        })))
    }

    fn scan(&self, git: &Git) -> ferrum_core::Result<Vec<Finding>> {
        let repo = ferrum_core::Repo::open(git.dir())?;
        let (scanner, _) = Scanner::new(&self.config, self.allow.clone());
        match &*self.op {
            Op::Commit { amend: true, .. } => scanner.scan_amend(&repo),
            Op::Commit { .. } => scanner.scan_staged(&repo),
            Op::Push(o) => scanner.scan_unpushed(&repo, &o.branch, &o.remote, &o.branch),
            Op::PushRefs(r) => {
                let mut out = Vec::new();
                for (local, to) in &r.branches {
                    out.extend(scanner.scan_unpushed(&repo, local, &r.remote, to)?);
                }
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }
}

/// The operation inside a check (or the operation itself).
pub fn inner(op: &Op) -> &Op {
    match op {
        Op::SecretCheck(c) => &c.op,
        other => other,
    }
}

/// Operations run again without the check ("Ignore once", "Add to allowlist").
pub fn unchecked(ops: Vec<Op>) -> Vec<Op> {
    ops.into_iter()
        .map(|op| match op {
            Op::SecretCheck(c) => *c.op,
            other => other,
        })
        .collect()
}
