//! Git-flow with plain git commands (the git-flow extension is not needed).
//! Branch names and prefixes live in the same config keys as git-flow AVH
//! (`gitflow.branch.*`, `gitflow.prefix.*`), so both tools work together.

use crate::cli::Git;
use crate::{Error, Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowConfig {
    /// Production branch (`gitflow.branch.master`).
    pub master: String,
    pub develop: String,
    pub feature: String,
    pub release: String,
    pub hotfix: String,
    pub support: String,
    /// Prefix of release and hotfix tags (`gitflow.prefix.versiontag`).
    pub version_tag: String,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            master: "master".into(),
            develop: "develop".into(),
            feature: "feature/".into(),
            release: "release/".into(),
            hotfix: "hotfix/".into(),
            support: "support/".into(),
            version_tag: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Feature,
    Release,
    Hotfix,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Feature, Kind::Release, Kind::Hotfix];

    pub fn prefix(self, cfg: &FlowConfig) -> &str {
        match self {
            Kind::Feature => &cfg.feature,
            Kind::Release => &cfg.release,
            Kind::Hotfix => &cfg.hotfix,
        }
    }

    /// Where new branches of this kind start.
    pub fn base(self, cfg: &FlowConfig) -> &str {
        match self {
            Kind::Feature | Kind::Release => &cfg.develop,
            Kind::Hotfix => &cfg.master,
        }
    }

    /// Releases and hotfixes go to the production branch and get a tag.
    pub fn is_release(self) -> bool {
        self != Kind::Feature
    }
}

const KEYS: [&str; 7] = [
    "branch.master",
    "branch.develop",
    "prefix.feature",
    "prefix.release",
    "prefix.hotfix",
    "prefix.support",
    "prefix.versiontag",
];

impl FlowConfig {
    fn values(&self) -> [&str; 7] {
        [
            &self.master,
            &self.develop,
            &self.feature,
            &self.release,
            &self.hotfix,
            &self.support,
            &self.version_tag,
        ]
    }

    /// The kind and short name of a branch, from its prefix.
    pub fn kind_of<'a>(&self, branch: &'a str) -> Option<(Kind, &'a str)> {
        Kind::ALL.into_iter().find_map(|k| {
            let p = k.prefix(self);
            (!p.is_empty())
                .then(|| branch.strip_prefix(p))
                .flatten()
                .filter(|n| !n.is_empty())
                .map(|n| (k, n))
        })
    }
}

/// The git-flow settings of a repository, or `None` when it was not initialized.
pub fn config(repo: &Repo) -> Option<FlowConfig> {
    let cfg = repo.raw().config().ok()?;
    let get = |k: &str| cfg.get_string(&format!("gitflow.{k}")).ok();
    let develop = get("branch.develop")?;
    let d = FlowConfig::default();
    Some(FlowConfig {
        master: get("branch.master").unwrap_or(d.master),
        develop,
        feature: get("prefix.feature").unwrap_or(d.feature),
        release: get("prefix.release").unwrap_or(d.release),
        hotfix: get("prefix.hotfix").unwrap_or(d.hotfix),
        support: get("prefix.support").unwrap_or(d.support),
        version_tag: get("prefix.versiontag").unwrap_or_default(),
    })
}

fn exists(git: &Git, rev: &str) -> bool {
    git.run(&["rev-parse", "-q", "--verify", &format!("{rev}^{{commit}}")])
        .is_ok()
}

fn branch_exists(git: &Git, name: &str) -> bool {
    exists(git, &format!("refs/heads/{name}"))
}

/// Writes the settings, creates the branches that are missing and checks
/// out the development branch.
pub fn init(git: &Git, cfg: &FlowConfig) -> Result<()> {
    if cfg.master.trim().is_empty() || cfg.develop.trim().is_empty() {
        return Err(Error::Other("Branch names cannot be empty.".into()));
    }
    if cfg.master == cfg.develop {
        return Err(Error::Other(
            "The production and development branches must be different.".into(),
        ));
    }
    for (k, v) in KEYS.iter().zip(cfg.values()) {
        git.run(&["config", &format!("gitflow.{k}"), v])?;
    }
    if !branch_exists(git, &cfg.master) {
        if exists(git, "HEAD") {
            return Err(Error::Other(format!(
                "The branch {} does not exist.",
                cfg.master
            )));
        }
        // A repository without commits: start the production branch.
        git.run(&[
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{}", cfg.master),
        ])?;
        git.run(&["commit", "--allow-empty", "-q", "-m", "Initial commit"])?;
    }
    if !branch_exists(git, &cfg.develop) {
        git.run(&["branch", "--no-track", &cfg.develop, &cfg.master])?;
    }
    git.run(&["checkout", "-q", &cfg.develop])?;
    Ok(())
}

/// Removes the git-flow settings (branches are kept).
pub fn deinit(git: &Git) -> Result<()> {
    for section in ["gitflow.branch", "gitflow.prefix", "gitflow.path"] {
        // Fails when the section is missing.
        let _ = git.run(&["config", "--remove-section", section]);
    }
    Ok(())
}

/// Creates and checks out `<prefix><name>` from `base` (or the default base).
pub fn start(
    git: &Git,
    cfg: &FlowConfig,
    kind: Kind,
    name: &str,
    base: Option<&str>,
) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other("The name cannot be empty.".into()));
    }
    let branch = format!("{}{name}", kind.prefix(cfg));
    if branch_exists(git, &branch) {
        return Err(Error::Other(format!("The branch {branch} already exists.")));
    }
    let base = base
        .filter(|b| !b.trim().is_empty())
        .unwrap_or(kind.base(cfg));
    git.run(&["checkout", "-q", "--no-track", "-b", &branch, base])?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishOptions {
    /// Always create a merge commit.
    pub no_ff: bool,
    pub delete_branch: bool,
    /// Tag message (releases and hotfixes); `None` means no tag.
    pub tag: Option<String>,
    /// Merge a release or hotfix back into the development branch.
    pub back_merge: bool,
    /// Push the branches (and tags) to this remote afterwards.
    pub push: Option<String>,
}

impl Default for FinishOptions {
    fn default() -> Self {
        Self {
            no_ff: true,
            delete_branch: true,
            tag: Some(String::new()),
            back_merge: true,
            push: None,
        }
    }
}

/// The tag a release or hotfix gets.
pub fn tag_name(cfg: &FlowConfig, name: &str) -> String {
    format!("{}{name}", cfg.version_tag)
}

/// Merges `src` into the checked-out `target`, unless it is already in it.
fn merge_into(git: &Git, target: &str, src: &str, no_ff: bool) -> Result<()> {
    git.run(&["checkout", "-q", target])?;
    if git
        .run(&["merge-base", "--is-ancestor", src, target])
        .is_ok()
    {
        return Ok(());
    }
    let mut args = vec!["merge", "--no-edit"];
    if no_ff {
        args.push("--no-ff");
    }
    args.push(src);
    git.run(&args).map_err(|e| {
        Error::Other(format!(
            "{e}\nResolve the conflicts and commit, then finish again."
        ))
    })?;
    Ok(())
}

/// Finishes a feature (into develop) or a release / hotfix (into the
/// production branch, tagged, and back into develop). Steps already done
/// are skipped, so it can run again after conflicts were resolved.
pub fn finish(
    git: &Git,
    cfg: &FlowConfig,
    kind: Kind,
    name: &str,
    opts: &FinishOptions,
) -> Result<()> {
    let branch = format!("{}{name}", kind.prefix(cfg));
    if !branch_exists(git, &branch) {
        return Err(Error::Other(format!("The branch {branch} does not exist.")));
    }
    let mut pushed = Vec::new();
    let mut tag = None;
    if kind.is_release() {
        merge_into(git, &cfg.master, &branch, opts.no_ff)?;
        pushed.push(cfg.master.clone());
        if let Some(msg) = &opts.tag {
            let t = tag_name(cfg, name);
            if !exists(git, &format!("refs/tags/{t}")) {
                let msg = if msg.trim().is_empty() {
                    t.as_str()
                } else {
                    msg.as_str()
                };
                git.run(&["tag", "-a", &t, "-m", msg, &cfg.master])?;
            }
            tag = Some(t);
        }
        if opts.back_merge {
            let src = tag.clone().unwrap_or_else(|| branch.clone());
            merge_into(git, &cfg.develop, &src, opts.no_ff)?;
            pushed.push(cfg.develop.clone());
        }
    } else {
        merge_into(git, &cfg.develop, &branch, opts.no_ff)?;
        pushed.push(cfg.develop.clone());
    }
    if opts.delete_branch {
        git.run(&["branch", "-d", &branch])?;
    }
    if let Some(remote) = &opts.push {
        let mut args = vec!["push".to_owned(), remote.clone()];
        args.extend(pushed);
        if let Some(t) = &tag {
            args.push(format!("refs/tags/{t}"));
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        git.run(&args)?;
        if opts.delete_branch && exists(git, &format!("refs/remotes/{remote}/{branch}")) {
            git.run(&["push", remote, "--delete", &branch])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    fn flow_repo() -> (TestRepo, FlowConfig) {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        let cfg = FlowConfig {
            master: "main".into(),
            feature: "feat/".into(),
            version_tag: "v".into(),
            ..Default::default()
        };
        init(&t.git, &cfg).unwrap();
        (t, cfg)
    }

    fn head(t: &TestRepo) -> String {
        t.git
            .run(&["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap()
            .trim()
            .to_owned()
    }

    #[test]
    fn init_writes_avh_keys_and_creates_develop() {
        let (t, cfg) = flow_repo();
        assert_eq!(head(&t), "develop");
        let read = config(&t.repo()).unwrap();
        assert_eq!(read, cfg);
        let raw = t.git.run(&["config", "gitflow.prefix.feature"]).unwrap();
        assert_eq!(raw.trim(), "feat/");
        assert_eq!(read.kind_of("feat/login"), Some((Kind::Feature, "login")));
        assert_eq!(read.kind_of("release/1.0"), Some((Kind::Release, "1.0")));
        assert_eq!(read.kind_of("develop"), None);
        deinit(&t.git).unwrap();
        assert!(config(&t.repo()).is_none());
    }

    #[test]
    fn init_on_an_empty_repository() {
        let t = TestRepo::new();
        init(&t.git, &FlowConfig::default()).unwrap();
        assert_eq!(head(&t), "develop");
        assert!(branch_exists(&t.git, "master"));
    }

    #[test]
    fn feature_start_and_finish() {
        let (t, cfg) = flow_repo();
        start(&t.git, &cfg, Kind::Feature, "login", None).unwrap();
        assert_eq!(head(&t), "feat/login");
        t.commit("b.txt", "b\n", "add b");
        finish(
            &t.git,
            &cfg,
            Kind::Feature,
            "login",
            &FinishOptions::default(),
        )
        .unwrap();
        assert_eq!(head(&t), "develop");
        assert!(!branch_exists(&t.git, "feat/login"));
        let parents = t.git.run(&["log", "-1", "--format=%p"]).unwrap();
        assert_eq!(parents.split_whitespace().count(), 2, "a merge commit");
        // main is untouched by features.
        let main = t.git.run(&["log", "--format=%s", "main"]).unwrap();
        assert_eq!(main.trim(), "first");
    }

    #[test]
    fn release_finish_tags_and_merges_back() {
        let (t, cfg) = flow_repo();
        start(&t.git, &cfg, Kind::Release, "1.0", None).unwrap();
        t.commit("v.txt", "1.0\n", "bump");
        let opts = FinishOptions {
            tag: Some("Release 1.0".into()),
            ..Default::default()
        };
        finish(&t.git, &cfg, Kind::Release, "1.0", &opts).unwrap();
        let msg = t.git.run(&["tag", "-l", "-n1", "v1.0"]).unwrap();
        assert!(msg.contains("Release 1.0"), "{msg}");
        assert_eq!(t.git.run(&["show", "main:v.txt"]).unwrap(), "1.0\n");
        assert_eq!(t.git.run(&["show", "develop:v.txt"]).unwrap(), "1.0\n");
        assert!(!branch_exists(&t.git, "release/1.0"));
    }

    #[test]
    fn hotfix_without_back_merge_keeps_branch() {
        let (t, cfg) = flow_repo();
        start(&t.git, &cfg, Kind::Hotfix, "1.0.1", None).unwrap();
        t.commit("fix.txt", "fix\n", "fix");
        let opts = FinishOptions {
            no_ff: false,
            delete_branch: false,
            tag: None,
            back_merge: false,
            push: None,
        };
        finish(&t.git, &cfg, Kind::Hotfix, "1.0.1", &opts).unwrap();
        assert!(branch_exists(&t.git, "hotfix/1.0.1"));
        assert!(t.git.run(&["show", "main:fix.txt"]).is_ok());
        assert!(t.git.run(&["show", "develop:fix.txt"]).is_err());
        assert!(t.git.run(&["tag"]).unwrap().trim().is_empty());
        // Fast-forward: no merge commit.
        let parents = t.git.run(&["log", "-1", "--format=%p", "main"]).unwrap();
        assert_eq!(parents.split_whitespace().count(), 1);
    }

    #[test]
    fn finish_pushes_to_a_remote() {
        let (t, cfg) = flow_repo();
        let remote = tempfile::tempdir().unwrap();
        Git::new(remote.path())
            .run(&["init", "-q", "--bare"])
            .unwrap();
        let url = remote.path().to_string_lossy().into_owned();
        t.git.run(&["remote", "add", "origin", &url]).unwrap();
        start(&t.git, &cfg, Kind::Release, "2.0", None).unwrap();
        t.commit("r.txt", "r\n", "r");
        let opts = FinishOptions {
            push: Some("origin".into()),
            ..Default::default()
        };
        finish(&t.git, &cfg, Kind::Release, "2.0", &opts).unwrap();
        let refs = Git::new(remote.path())
            .run(&["for-each-ref", "--format=%(refname)"])
            .unwrap();
        assert!(refs.contains("refs/heads/main"), "{refs}");
        assert!(refs.contains("refs/heads/develop"), "{refs}");
        assert!(refs.contains("refs/tags/v2.0"), "{refs}");
    }
}
