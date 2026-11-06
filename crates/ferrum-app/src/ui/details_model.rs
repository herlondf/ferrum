//! State of the commit details: which tab is shown, the file list and file
//! tree, the refs that contain the commit (loaded in the background),
//! commits picked for comparing, the large popup and commit windows.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui;
use ferrum_core::browse::{self, Containing};
use ferrum_core::cli::Git;
use ferrum_core::diff::DiffConfig;
use ferrum_core::{Oid, Repo};
use serde::{Deserialize, Serialize};

use crate::tab::CommitView;
use crate::ui::{compare, file_tree};

/// Branches and tags listed per kind under "Contained in".
pub const CONTAINS_LIMIT: usize = 50;
/// Wait this long on a commit before looking for the refs that contain it,
/// so moving through the list does not start a search per commit.
const CONTAINS_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DetailsTab {
    #[default]
    Commit,
    Changes,
    FileTree,
}

/// Remembered between runs (`Settings::details`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailsSettings {
    pub tab: DetailsTab,
    /// Changed files as a folder tree instead of a flat list.
    pub files_as_tree: bool,
    /// The details pane is hidden (Ctrl+Alt+D).
    pub collapsed: bool,
}

/// Per-commit state of a details pane (the main one or a window).
#[derive(Default)]
pub struct PaneState {
    pub list: file_tree::ListState,
    pub tree: file_tree::RevTree,
    pub contains: Contains,
    pub links: MessageLinks,
}

/// Per-tab state (`RepoTab::details`).
#[derive(Default)]
pub struct DetailsState {
    pub pane: PaneState,
    /// Two revisions compared in the details pane.
    pub compare: Option<compare::CompareView>,
    /// Commits picked with Ctrl+click in the history.
    pub picked: Vec<Oid>,
    /// The large details popup (Space).
    pub popup: bool,
    pub windows: Vec<CommitWindow>,
    pub next_window: u64,
    /// Show the pane if it is collapsed (something was opened in it).
    pub reveal: bool,
    /// Switch to this tab on the next frame (`FERRUM_START`).
    pub set_tab: Option<DetailsTab>,
}

/// A commit shown in its own window.
pub struct CommitWindow {
    pub key: u64,
    pub view: CommitView,
    pub pane: PaneState,
    pub prefs: DetailsSettings,
    pub open: bool,
}

/// Branches and tags that contain the shown commit.
#[derive(Default)]
pub struct Contains {
    id: Option<Oid>,
    since: Option<Instant>,
    running: Option<(Oid, mpsc::Receiver<Result<Containing, String>>)>,
    pub result: Option<Result<Containing, String>>,
}

impl Contains {
    /// The refs containing `id`, `None` while they are being looked up.
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        git: &Git,
        id: Oid,
    ) -> Option<&Result<Containing, String>> {
        if self.id != Some(id) {
            self.id = Some(id);
            self.since = Some(Instant::now());
            self.result = None;
        }
        if let Some((rid, rx)) = &self.running {
            match rx.try_recv() {
                Ok(r) => {
                    if *rid == id {
                        self.result = Some(r);
                    }
                    self.running = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.running = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        // One search at a time, after the commit stayed selected a moment.
        if self.result.is_none() && self.running.is_none() {
            let waited = self.since.map_or(CONTAINS_DELAY, |s| s.elapsed());
            if waited < CONTAINS_DELAY {
                ctx.request_repaint_after(CONTAINS_DELAY - waited);
            } else {
                let (tx, rx) = mpsc::channel();
                let (git, ctx) = (git.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let r = browse::refs_containing(&git, id, CONTAINS_LIMIT)
                        .map_err(|e| e.to_string());
                    let _ = tx.send(r);
                    ctx.request_repaint();
                });
                self.running = Some((id, rx));
            }
        }
        self.result.as_ref()
    }
}

/// Commit ids mentioned in the shown message, resolved once per commit.
#[derive(Default)]
pub struct MessageLinks {
    id: Option<Oid>,
    pub found: Vec<(std::ops::Range<usize>, Oid)>,
}

impl MessageLinks {
    pub fn update(&mut self, repo: &Repo, id: Oid, message: &str) {
        if self.id == Some(id) {
            return;
        }
        self.id = Some(id);
        self.found = browse::find_commit_ids(message)
            .into_iter()
            .filter_map(|r| {
                let target = repo.resolve(&message[r.clone()]).ok()?;
                (target != id).then_some((r, target))
            })
            .collect();
    }
}

/// Loads a commit for a details pane (files only; diffs load per file).
pub fn load_view(
    repo: &Repo,
    id: Oid,
    is_stash: bool,
    cfg: &DiffConfig,
) -> Result<CommitView, String> {
    let details = repo.commit_details(id).map_err(|e| e.to_string())?;
    let files = repo.commit_files(id).map_err(|e| e.to_string())?;
    let mut view = CommitView {
        details,
        files,
        selected_file: None,
        diff: None,
        is_stash,
    };
    select_file(repo, &mut view, 0, cfg)?;
    Ok(view)
}

/// Selects a changed file of a commit and loads its diff.
pub fn select_file(
    repo: &Repo,
    view: &mut CommitView,
    i: usize,
    cfg: &DiffConfig,
) -> Result<(), String> {
    let Some(file) = view.files.get(i) else {
        return Ok(());
    };
    view.selected_file = Some(i);
    let id = view.details.info.id;
    match repo.commit_file_diff_with(id, &file.path, file.old_path.as_deref(), cfg) {
        Ok(d) => {
            view.diff = d;
            Ok(())
        }
        Err(e) => {
            view.diff = None;
            Err(e.to_string())
        }
    }
}
