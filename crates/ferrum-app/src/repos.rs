//! Known repositories (the Repository Manager's list), source folders,
//! workspaces, tab names and colors, and the requests Ferrum is started with
//! (a repository, a file's history or an "Open in Desktop" link).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What the tab bar, the Repository Manager and the error helpers ask for
/// (handled in `app/repos.rs`).
pub enum RepoCmd {
    /// Open the Repository Manager in a new tab (Ctrl+T).
    NewTab,
    CloseHome,
    /// Reopen the last closed tab (Ctrl+Shift+T).
    ReopenClosed,
    /// Fuzzy repository picker (Ctrl+Shift+O).
    OpenQuickly,
    CloseAll,
    CloseOthers(usize),
    /// Close a tab here and add it to another workspace.
    MoveToWorkspace(usize, String),
    SwitchWorkspace(String),
    /// Look for repositories in the source folders.
    Scan,
    /// Show a tab in a window of its own.
    OpenInWindow(usize),
    /// Move a tab to another place in the tab bar (drag and drop).
    MoveTab(usize, usize),
    /// `git config --global --add safe.directory`, then open the folder.
    TrustFolder(PathBuf),
    /// Settings for the repository of the active tab.
    Settings,
    /// A `FERRUM_START` step (see `app/repos.rs`).
    Dev(String),
    /// Fetch or pull (fast-forward only) several repositories (see `batch.rs`).
    Batch(crate::batch::Kind, Vec<PathBuf>),
}

/// Repository settings and fixes, run as jobs.
#[derive(Debug, Clone)]
pub enum RepoOp {
    DeleteLock(PathBuf),
    /// Set (`None`: remove) `commit.template`.
    CommitTemplate(Option<String>),
}

impl RepoOp {
    pub fn label(&self) -> String {
        match self {
            RepoOp::DeleteLock(_) => t!("Delete the lock file"),
            RepoOp::CommitTemplate(_) => t!("Set commit template"),
        }
    }

    pub fn run(&self, git: &ferrum_core::cli::Git) -> ferrum_core::Result<()> {
        use ferrum_core::library;
        match self {
            RepoOp::DeleteLock(p) => library::delete_index_lock(p),
            RepoOp::CommitTemplate(p) => library::set_commit_template(git, p.as_deref()),
        }
    }
}

/// A repository listed in the Repository Manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KnownRepo {
    pub path: PathBuf,
    /// Name shown instead of the folder name (empty: the folder name).
    pub name: String,
    /// Group in the Repository Manager (empty: none).
    pub folder: String,
    /// Tab color.
    pub color: Option<[u8; 3]>,
}

/// A folder scanned for repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceFolder {
    pub path: PathBuf,
    /// How many folders deep to look (1: only the folders right inside).
    pub depth: usize,
}

impl Default for SourceFolder {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            depth: 2,
        }
    }
}

/// A named set of tabs. Names may be nested with `/` ("work/client").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Workspace {
    pub name: String,
    pub tabs: Vec<PathBuf>,
    pub active: usize,
}

/// Settings of one repository that all its worktrees share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RepoConfig {
    /// Remote whose host is used for pull requests (empty: origin).
    pub pr_remote: String,
}

/// Everything the Repository Manager remembers (`Settings::library`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Library {
    pub repos: Vec<KnownRepo>,
    /// Groups, also the empty ones.
    pub folders: Vec<String>,
    pub sources: Vec<SourceFolder>,
    /// Repositories found by the last scan of `sources`.
    pub found: Vec<PathBuf>,
    /// Saved tabs of the workspaces that are not active. The default
    /// workspace is named "".
    pub workspaces: Vec<Workspace>,
    /// The active workspace ("" is the default one).
    pub workspace: String,
    /// Tabs in a list on the left instead of a row on top.
    pub vertical_tabs: bool,
    /// Per repository, by `repo_settings::shared_key`.
    pub config: BTreeMap<String, RepoConfig>,
    /// The recent list was copied in once (from versions without the list).
    pub imported_recent: bool,
    /// The Repository Manager shows a list instead of cards.
    #[serde(default)]
    pub list_view: bool,
}

impl Library {
    pub fn get(&self, path: &Path) -> Option<&KnownRepo> {
        self.repos.iter().find(|r| r.path == path)
    }

    /// The entry of `path`, added when missing.
    pub fn entry(&mut self, path: &Path) -> &mut KnownRepo {
        let i = match self.repos.iter().position(|r| r.path == path) {
            Some(i) => i,
            None => {
                self.repos.push(KnownRepo {
                    path: path.to_path_buf(),
                    ..Default::default()
                });
                self.repos.len() - 1
            }
        };
        &mut self.repos[i]
    }

    pub fn remember(&mut self, path: &Path) {
        self.entry(path);
    }

    pub fn remove(&mut self, paths: &[PathBuf]) {
        self.repos.retain(|r| !paths.contains(&r.path));
    }

    /// The name chosen for `path`, if any.
    pub fn custom_name(&self, path: &Path) -> Option<&str> {
        self.get(path)
            .map(|r| r.name.trim())
            .filter(|n| !n.is_empty())
    }

    pub fn color(&self, path: &Path) -> Option<[u8; 3]> {
        self.get(path).and_then(|r| r.color)
    }

    /// Listed repositories whose folder is gone.
    pub fn missing(&self) -> Vec<PathBuf> {
        self.repos
            .iter()
            .filter(|r| !r.path.exists())
            .map(|r| r.path.clone())
            .collect()
    }

    /// Found by a scan and not in the list yet.
    pub fn unlisted_found(&self) -> Vec<PathBuf> {
        self.found
            .iter()
            .filter(|p| self.get(p).is_none())
            .cloned()
            .collect()
    }

    /// Every group name: declared ones and those used by repositories.
    pub fn folder_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.folders.clone();
        for r in &self.repos {
            if !r.folder.is_empty() && !names.contains(&r.folder) {
                names.push(r.folder.clone());
            }
        }
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    pub fn rename_folder(&mut self, old: &str, new: &str) {
        let new = new.trim();
        for f in &mut self.folders {
            if f == old {
                *f = new.to_owned();
            }
        }
        for r in &mut self.repos {
            if r.folder == old {
                r.folder = new.to_owned();
            }
        }
        self.folders.dedup();
    }

    /// Removes a group; its repositories stay, outside any group.
    pub fn delete_folder(&mut self, name: &str) {
        self.rename_folder(name, "");
        self.folders.retain(|f| !f.is_empty());
    }

    // ----- Workspaces -----

    /// Workspace names, sorted, without the default one.
    pub fn workspace_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .workspaces
            .iter()
            .map(|w| w.name.clone())
            .filter(|n| !n.is_empty())
            .collect();
        if !self.workspace.is_empty() && !names.contains(&self.workspace) {
            names.push(self.workspace.clone());
        }
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    fn slot(&mut self, name: &str) -> &mut Workspace {
        let i = match self.workspaces.iter().position(|w| w.name == name) {
            Some(i) => i,
            None => {
                self.workspaces.push(Workspace {
                    name: name.to_owned(),
                    ..Default::default()
                });
                self.workspaces.len() - 1
            }
        };
        &mut self.workspaces[i]
    }

    pub fn add_workspace(&mut self, name: &str) {
        let name = clean_workspace_name(name);
        if !name.is_empty() {
            self.slot(&name);
        }
    }

    /// Saves the open tabs under the active workspace and makes `to` active.
    /// Returns the tabs to open, or `None` when `to` is already active.
    pub fn switch_workspace(
        &mut self,
        to: &str,
        tabs: Vec<PathBuf>,
        active: usize,
    ) -> Option<(Vec<PathBuf>, usize)> {
        let to = clean_workspace_name(to);
        if to == self.workspace {
            return None;
        }
        let current = self.workspace.clone();
        let slot = self.slot(&current);
        slot.tabs = tabs;
        slot.active = active;
        let next = self.slot(&to);
        let open = (std::mem::take(&mut next.tabs), next.active);
        self.workspace = to;
        Some(open)
    }

    /// Adds a repository to the saved tabs of another workspace.
    pub fn add_to_workspace(&mut self, name: &str, path: &Path) {
        let slot = self.slot(&clean_workspace_name(name));
        if !slot.tabs.iter().any(|p| p == path) {
            slot.tabs.push(path.to_path_buf());
        }
    }

    /// Deletes a workspace and the ones nested in it. The active one stays.
    pub fn delete_workspace(&mut self, name: &str) {
        let nested = format!("{name}/");
        let active = self.workspace.clone();
        self.workspaces
            .retain(|w| w.name == active || (w.name != name && !w.name.starts_with(&nested)));
    }

    /// Renames a workspace and the ones nested in it.
    pub fn rename_workspace(&mut self, old: &str, new: &str) {
        let new = clean_workspace_name(new);
        if new.is_empty() || old.is_empty() {
            return;
        }
        let rename = |n: &mut String| {
            if n == old {
                *n = new.clone();
            } else if let Some(rest) = n.strip_prefix(&format!("{old}/")) {
                *n = format!("{new}/{rest}");
            }
        };
        for w in &mut self.workspaces {
            rename(&mut w.name);
        }
        rename(&mut self.workspace);
    }

    pub fn config(&self, key: &str) -> RepoConfig {
        self.config.get(key).cloned().unwrap_or_default()
    }

    pub fn set_config(&mut self, key: String, c: RepoConfig) {
        if c == RepoConfig::default() {
            self.config.remove(&key);
        } else {
            self.config.insert(key, c);
        }
    }
}

/// Trims each `/`-separated part and drops empty ones.
pub fn clean_workspace_name(name: &str) -> String {
    name.split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Colors offered for tabs.
pub const TAB_COLORS: &[[u8; 3]] = &[
    [220, 76, 70],
    [230, 145, 56],
    [214, 190, 50],
    [80, 175, 90],
    [60, 160, 200],
    [80, 110, 220],
    [150, 95, 210],
    [210, 90, 160],
];

/// Tab labels made unique: tabs with the same label get the nearest parent
/// folder that tells them apart, e.g. "app — client-a" and "app — client-b".
pub fn unique_labels(items: &[(String, PathBuf)]) -> Vec<String> {
    let mut out: Vec<String> = items.iter().map(|(l, _)| l.clone()).collect();
    for (i, (label, _)) in items.iter().enumerate() {
        let same: Vec<usize> = (0..items.len()).filter(|&j| items[j].0 == *label).collect();
        if same.len() < 2 || same[0] != i {
            continue;
        }
        let parents: Vec<Vec<String>> = same
            .iter()
            .map(|&j| {
                let p = items[j].1.parent().unwrap_or(Path::new(""));
                let mut parts: Vec<String> = p
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                parts.reverse();
                parts
            })
            .collect();
        let longest = parents.iter().map(Vec::len).max().unwrap_or(0);
        // The first level (from the end) where the parents differ.
        let level = (0..longest).find(|&k| {
            let at: Vec<Option<&String>> = parents.iter().map(|p| p.get(k)).collect();
            at.iter()
                .enumerate()
                .all(|(a, x)| at.iter().skip(a + 1).all(|y| x != y))
        });
        for (n, &j) in same.iter().enumerate() {
            let hint = match level {
                Some(k) => parents[n].get(k).cloned().unwrap_or_default(),
                None => items[j].1.display().to_string(),
            };
            if !hint.is_empty() {
                out[j] = format!("{label} — {hint}");
            }
        }
    }
    out
}

/// The tab label of a repository: its chosen name, or the folder name; a
/// linked worktree shows its main repository: "repo (worktree)".
pub fn base_label(lib: &Library, tab: &crate::tab::RepoTab) -> String {
    if let Some(n) = lib.custom_name(tab.path()) {
        return n.to_owned();
    }
    let git_dir = std::fs::canonicalize(tab.repo.git_dir()).ok();
    let common = std::fs::canonicalize(tab.repo.common_dir()).ok();
    if git_dir.is_some()
        && git_dir != common
        && let Some(main) = common.as_deref().and_then(main_repo_name)
    {
        return format!("{main} ({})", tab.name);
    }
    tab.name.clone()
}

/// Name of the repository owning a common git folder: `/x/app/.git` → "app",
/// `/x/app.git` → "app".
fn main_repo_name(common: &Path) -> Option<String> {
    let name = common.file_name()?.to_string_lossy();
    if name == ".git" {
        Some(common.parent()?.file_name()?.to_string_lossy().into_owned())
    } else {
        Some(name.trim_end_matches(".git").to_owned())
    }
}

// ----- Launch requests -----

/// What Ferrum is asked to do by its command line (or a second instance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open a repository folder.
    Open(PathBuf),
    /// Show the history of a file (absolute path).
    FileLog(PathBuf),
    /// Open the clone dialog for a URL.
    Clone(String),
    /// Just bring the window to the front.
    Focus,
}

/// Reads `ferrum [PATH…]`, `ferrum log [--] FILE` and link arguments.
/// Relative paths are resolved against `cwd`.
pub fn parse_args(args: &[OsString], cwd: &Path) -> Vec<Request> {
    let abs = |a: &OsString| {
        let p = PathBuf::from(a);
        if p.is_absolute() { p } else { cwd.join(p) }
    };
    if args.first().is_some_and(|a| a == "log") {
        return args[1..]
            .iter()
            .filter(|a| *a != "--")
            .map(|a| Request::FileLog(abs(a)))
            .collect();
    }
    args.iter()
        .map(|a| {
            let text = a.to_string_lossy();
            if let Some(path) = text.strip_prefix("file://") {
                // Desktop launchers may pass folders as file:// URLs.
                Request::Open(abs(&percent_decode(path).into()))
            } else if text.contains("://") && !Path::new(a).exists() {
                match clone_url_from_link(&text) {
                    Some(url) => Request::Clone(url),
                    None => Request::Focus,
                }
            } else {
                Request::Open(abs(a))
            }
        })
        .collect()
}

/// One request per line, to send to the running instance.
pub fn encode(requests: &[Request]) -> String {
    let mut out = String::new();
    for r in requests {
        let line = match r {
            Request::Open(p) => format!("open\t{}", p.to_string_lossy()),
            Request::FileLog(p) => format!("log\t{}", p.to_string_lossy()),
            Request::Clone(u) => format!("clone\t{u}"),
            Request::Focus => "focus".to_owned(),
        };
        out.push_str(&line.replace('\n', " "));
        out.push('\n');
    }
    if requests.is_empty() {
        out.push_str("focus\n");
    }
    out
}

pub fn decode(text: &str) -> Vec<Request> {
    text.lines()
        .filter_map(|l| {
            let (kind, arg) = l.split_once('\t').unwrap_or((l, ""));
            Some(match kind {
                "open" => Request::Open(arg.into()),
                "log" => Request::FileLog(arg.into()),
                "clone" => Request::Clone(arg.to_owned()),
                "focus" => Request::Focus,
                _ => return None,
            })
        })
        .collect()
}

/// The repository URL in an "Open in Desktop" link:
/// `x-github-client://openRepo/https://github.com/o/r` (also `github-mac://`,
/// `github-windows://`) and `sourcetree://cloneRepo?cloneUrl=…` or
/// `sourcetree://cloneRepo/URL`.
pub fn clone_url_from_link(link: &str) -> Option<String> {
    let (scheme, rest) = link.trim().split_once("://")?;
    let url = match scheme.to_lowercase().as_str() {
        "x-github-client" | "github-mac" | "github-windows" => {
            let rest = strip_prefix_ci(rest, "openrepo/")?;
            // Query parameters (?branch=…) belong to the link, not the URL.
            percent_decode(rest.split('?').next()?)
        }
        "sourcetree" => {
            let rest = strip_prefix_ci(rest, "clonerepo")?;
            if let Some(query) = rest.strip_prefix('?').or_else(|| rest.strip_prefix("/?")) {
                let value = query.split('&').find_map(|kv| {
                    let (k, v) = kv.split_once('=')?;
                    k.eq_ignore_ascii_case("cloneUrl").then_some(v)
                })?;
                percent_decode(&value.replace('+', " "))
            } else {
                let url = percent_decode(rest.strip_prefix('/')?);
                // Some links put the kind first: "git|https://…".
                match url.split_once('|') {
                    Some((_, u)) => u.to_owned(),
                    None => url,
                }
            }
        }
        _ => return None,
    };
    is_remote_url(&url).then_some(url)
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Only network remotes: no local paths or helper transports (`ext::`).
fn is_remote_url(url: &str) -> bool {
    let u = url.to_lowercase();
    let ok = ["https://", "http://", "ssh://", "git://"]
        .iter()
        .any(|p| u.starts_with(p))
        || (u.starts_with("git@") && u.contains(':'));
    ok && !url.contains(char::is_whitespace) && !url.starts_with('-')
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(h, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspaces_keep_their_tabs() {
        let mut lib = Library::default();
        let a = PathBuf::from("/r/a");
        let b = PathBuf::from("/r/b");
        // Leaving the default workspace saves its tabs.
        let (open, _) = lib
            .switch_workspace(" work / client ", vec![a.clone()], 0)
            .unwrap();
        assert!(open.is_empty());
        assert_eq!(lib.workspace, "work/client");
        assert_eq!(lib.workspace_names(), ["work/client"]);
        assert!(lib.switch_workspace("work/client", vec![], 0).is_none());
        let (open, active) = lib.switch_workspace("", vec![b.clone()], 0).unwrap();
        assert_eq!((open, active), (vec![a.clone()], 0));
        lib.add_to_workspace("work/client", &a);
        let (open, _) = lib.switch_workspace("work/client", vec![], 0).unwrap();
        assert_eq!(open, vec![b, a]);
        lib.add_workspace("work/other");
        lib.rename_workspace("work", "job");
        assert_eq!(lib.workspace, "job/client");
        assert_eq!(lib.workspace_names(), ["job/client", "job/other"]);
        lib.delete_workspace("job");
        // The active one is kept.
        assert_eq!(lib.workspace_names(), ["job/client"]);
    }

    #[test]
    fn folders_and_missing_repos() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::default();
        lib.entry(dir.path()).folder = "Work".into();
        lib.remember(Path::new("/no/such/repo"));
        lib.remember(dir.path());
        assert_eq!(lib.repos.len(), 2);
        assert_eq!(lib.missing(), [PathBuf::from("/no/such/repo")]);
        lib.folders.push("Empty".into());
        assert_eq!(lib.folder_names(), ["Empty", "Work"]);
        lib.rename_folder("Work", "Job");
        assert_eq!(lib.get(dir.path()).unwrap().folder, "Job");
        lib.delete_folder("Job");
        assert_eq!(lib.folder_names(), ["Empty"]);
        lib.remove(&lib.missing());
        assert_eq!(lib.repos.len(), 1);
    }

    #[test]
    fn labels_tell_same_names_apart() {
        let items = vec![
            ("app".to_owned(), PathBuf::from("/w/client-a/app")),
            ("app".to_owned(), PathBuf::from("/w/client-b/app")),
            ("lib".to_owned(), PathBuf::from("/w/lib")),
            ("x".to_owned(), PathBuf::from("/a/same/x")),
            ("x".to_owned(), PathBuf::from("/b/same/x")),
        ];
        assert_eq!(
            unique_labels(&items),
            ["app — client-a", "app — client-b", "lib", "x — a", "x — b"]
        );
        assert_eq!(main_repo_name(Path::new("/x/app/.git")).unwrap(), "app");
        assert_eq!(main_repo_name(Path::new("/x/app.git")).unwrap(), "app");
    }

    #[test]
    fn reads_command_lines() {
        let cwd = Path::new("/home/u/src");
        let args = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            parse_args(&args(&["repo", "/abs"]), cwd),
            [
                Request::Open("/home/u/src/repo".into()),
                Request::Open("/abs".into())
            ]
        );
        assert_eq!(
            parse_args(&args(&["log", "--", "app/main.rs"]), cwd),
            [Request::FileLog("/home/u/src/app/main.rs".into())]
        );
        assert_eq!(
            parse_args(
                &args(&["x-github-client://openRepo/https://github.com/o/r"]),
                cwd
            ),
            [Request::Clone("https://github.com/o/r".into())]
        );
        let reqs = vec![
            Request::Open("/a b".into()),
            Request::FileLog("/c".into()),
            Request::Clone("https://h/r.git".into()),
            Request::Focus,
        ];
        assert_eq!(
            parse_args(&args(&["file:///home/u/my%20repo"]), cwd),
            [Request::Open("/home/u/my repo".into())]
        );
        assert_eq!(decode(&encode(&reqs)), reqs);
        assert_eq!(decode(&encode(&[])), [Request::Focus]);
    }

    #[test]
    fn reads_open_in_desktop_links() {
        let url = |l| clone_url_from_link(l);
        assert_eq!(
            url("x-github-client://openRepo/https://github.com/o/r?branch=main").as_deref(),
            Some("https://github.com/o/r")
        );
        assert_eq!(
            url("github-windows://openRepo/https%3A%2F%2Fgithub.com%2Fo%2Fr").as_deref(),
            Some("https://github.com/o/r")
        );
        assert_eq!(
            url("sourcetree://cloneRepo?type=bitbucket&cloneUrl=https%3A%2F%2Fbitbucket.org%2Ft%2Fr.git").as_deref(),
            Some("https://bitbucket.org/t/r.git")
        );
        assert_eq!(
            url("sourcetree://cloneRepo/git|ssh://git@host/r.git").as_deref(),
            Some("ssh://git@host/r.git")
        );
        assert_eq!(
            url("sourcetree://cloneRepo/git@github.com:o/r.git").as_deref(),
            Some("git@github.com:o/r.git")
        );
        // Local paths and helper transports are refused.
        assert_eq!(url("sourcetree://cloneRepo/ext::sh -c touch% /tmp/x"), None);
        assert_eq!(url("x-github-client://openRepo/file:///etc"), None);
        assert_eq!(url("https://github.com/o/r"), None);
    }
}
