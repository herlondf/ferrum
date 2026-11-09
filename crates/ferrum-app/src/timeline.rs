//! Restore points around operations (see `ferrum_core::restore`).
//!
//! Which operations get one is decided here, per `Op`, and the point is taken
//! in the job runner (`RepoTab::start_job`) right before the operation runs,
//! so every way of starting it (menus, dialogs, palette, queue, undo) is
//! covered. The quick undo stack (`tab.undo`) stays for simple inverses (a
//! checkout goes back with a checkout); Ctrl+Z picks whichever of the two is
//! newer, and the timeline window lists every point.

use std::sync::{Arc, Mutex};

use ferrum_core::ChangeKind;
use ferrum_core::cli::Git;
use ferrum_core::ops::PatchTarget;
use ferrum_core::restore::{self, RestoreMode, SnapshotOptions};

use crate::command::Op;
use crate::jobs::{JobOutput, JobResult};
use crate::tab::RepoTab;

/// Receives the id of the point a job took.
pub type Slot = Arc<Mutex<Option<String>>>;

#[derive(Debug, Clone)]
pub enum TimelineOp {
    /// Bring back point `id`, taken before `label`.
    Restore {
        id: String,
        label: String,
        mode: RestoreMode,
        untracked: bool,
    },
    Delete(Vec<String>),
}

impl TimelineOp {
    pub fn label(&self) -> String {
        match self {
            TimelineOp::Restore { label, .. } => t!("Restore to before {}", label),
            TimelineOp::Delete(ids) if ids.len() == 1 => t!("Delete restore point"),
            TimelineOp::Delete(ids) => t!("Delete {} restore points", ids.len()),
        }
    }

    pub fn run(&self, git: &Git) -> ferrum_core::Result<()> {
        match self {
            TimelineOp::Restore { .. } => self.restore(git).map(drop),
            TimelineOp::Delete(ids) => ids.iter().try_for_each(|id| restore::delete(git, id)),
        }
    }

    /// Restores; returns the id of the point saved first.
    fn restore(&self, git: &Git) -> ferrum_core::Result<String> {
        let TimelineOp::Restore {
            id,
            mode,
            untracked,
            ..
        } = self
        else {
            return Ok(String::new());
        };
        let opts = SnapshotOptions {
            untracked: *untracked,
            ..Default::default()
        };
        restore::restore(git, id, *mode, &self.label(), &opts)
    }
}

/// When to take restore points; copied from the settings into each tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub untracked: bool,
    pub keep_days: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            untracked: true,
            keep_days: 14,
        }
    }
}

impl Config {
    pub fn from_settings(s: &crate::safety::SafetySettings) -> Self {
        Self {
            enabled: s.restore_points,
            untracked: s.untracked,
            keep_days: s.keep_days.max(1),
        }
    }
}

/// A restore point to take before a job.
pub struct Plan {
    label: String,
    opts: SnapshotOptions,
    keep_days: u32,
}

/// Untracked paths the operation deletes, or `None` when it needs no point:
/// only operations that rewrite or drop work get one.
fn needs_point(op: &Op) -> Option<Vec<String>> {
    use crate::flow::FlowOp;
    use crate::ref_ops::RefOp;
    use ferrum_core::integrate::LocalChanges;
    Some(match op {
        Op::Discard(files) => files
            .iter()
            .filter(|f| f.kind == ChangeKind::Untracked)
            .map(|f| f.path.clone())
            .collect(),
        Op::ApplyPatch {
            target: PatchTarget::WorkingCopy,
            reverse: true,
            ..
        }
        | Op::ResetFileTo { .. }
        | Op::Commit { amend: true, .. }
        | Op::Pull(_)
        | Op::PullWith(_)
        | Op::DeleteBranch { .. }
        | Op::MoveBranch { .. }
        | Op::Merge { .. }
        | Op::Rebase { .. }
        | Op::InteractiveRebase { .. }
        | Op::Reset { .. }
        | Op::DeleteTag(_)
        | Op::StashDrop(_)
        | Op::StashApply { pop: true, .. }
        | Op::Abort(_)
        | Op::Skip(_)
        | Op::Sequence(..)
        | Op::Custom(_)
        | Op::Refs(RefOp::StashDrop(_))
        | Op::Refs(RefOp::StashApply { pop: true, .. })
        | Op::GitFlow(crate::ui::gitflow::GitFlowOp::Finish { .. })
        | Op::Patch(crate::ui::patches::PatchOp::Apply { .. }) => Vec::new(),
        Op::Push(o) if o.force_with_lease => Vec::new(),
        Op::PushRefs(r) if r.force_with_lease => Vec::new(),
        Op::Flow(f) if f.rewrites_head() => Vec::new(),
        Op::Flow(FlowOp::WithLocalChanges {
            mode: LocalChanges::Discard,
            ..
        }) => Vec::new(),
        Op::Cleanup(c) if c.rewrites() => Vec::new(),
        Op::SecretCheck(c) => return needs_point(&c.op),
        _ => return None,
    })
}

/// Whether `op` gets a restore point (when they are on).
#[cfg(test)]
pub fn saves_point(op: &Op) -> bool {
    needs_point(op).is_some()
}

pub fn plan(op: &Op, config: &Config) -> Option<Plan> {
    if !config.enabled {
        return None;
    }
    let paths = needs_point(op)?;
    Some(Plan {
        label: op.label(),
        opts: SnapshotOptions {
            untracked: config.untracked,
            paths,
            ..Default::default()
        },
        keep_days: config.keep_days,
    })
}

/// Runs a job's operation, with a restore point first when `plan` asks.
/// `git` runs the snapshot (not listed in the Activity window), `logged`
/// the operation itself.
pub fn run_job(
    op: &Op,
    plan: Option<Plan>,
    git: &Git,
    logged: &Git,
    slot: &Slot,
    progress: &dyn Fn(&str),
) -> JobResult {
    if let Op::Timeline(t @ TimelineOp::Restore { .. }) = op {
        let id = t.restore(logged).map_err(|e| e.to_string())?;
        set(slot, id);
        return Ok(JobOutput::Done);
    }
    let Some(plan) = plan else {
        return op.execute(logged, progress);
    };
    let snap = restore::create(git, &plan.label, &plan.opts).map_err(|e| {
        t!("Could not save a restore point ({}). Turn restore points off in Settings > Safety to run it anyway.", e)
    })?;
    let result = op.execute(logged, progress);
    match &result {
        // Nothing happened: the commit or push was stopped before running.
        Ok(JobOutput::Secrets(_)) => {
            let _ = restore::delete(git, snap.id());
        }
        _ => {
            let error = result.as_ref().err().cloned();
            let _ = restore::finish(git, &snap, error);
            set(slot, snap.id().to_owned());
        }
    }
    let _ = restore::prune(git, plan.keep_days);
    result
}

fn set(slot: &Slot, id: String) {
    if let Ok(mut s) = slot.lock() {
        *s = Some(id);
    }
}

/// A point taken in this session, with the number of the job that took it.
#[derive(Debug, Clone)]
pub struct SessionPoint {
    pub seq: u64,
    pub id: String,
    pub label: String,
}

/// Restore points of a tab.
#[derive(Default)]
pub struct TabTimeline {
    pub config: Config,
    slot: Slot,
    /// Counts finished jobs; undo entries and points carry the number.
    seq: u64,
    /// Points taken in this session, oldest first.
    pub session: Vec<SessionPoint>,
    /// Bumped when points were added or removed (the timeline reloads).
    pub generation: u64,
    /// A point just taken, for the Undo button of the toast.
    pub fresh: Option<SessionPoint>,
    restoring: bool,
}

impl TabTimeline {
    /// A job starts: what to save first, and where its point id goes.
    pub fn begin(&mut self, op: &Op, background: bool) -> (Option<Plan>, Slot) {
        self.restoring = matches!(op, Op::Timeline(TimelineOp::Restore { .. }));
        if matches!(op, Op::Timeline(_)) {
            self.generation += 1;
        }
        if let Ok(mut s) = self.slot.lock() {
            *s = None;
        }
        let plan = if background {
            None
        } else {
            plan(op, &self.config)
        };
        (plan, self.slot.clone())
    }

    /// A job finished: its number, and whether it was a restore.
    pub fn finished(&mut self, label: &str) -> (u64, bool) {
        self.seq += 1;
        let id = self.slot.lock().ok().and_then(|mut s| s.take());
        if let Some(id) = id {
            let p = SessionPoint {
                seq: self.seq,
                id,
                label: label.to_owned(),
            };
            self.session.push(p.clone());
            self.fresh = Some(p);
            self.generation += 1;
        }
        let restored = std::mem::take(&mut self.restoring);
        if restored {
            self.generation += 1;
        }
        (self.seq, restored)
    }

    /// The quick undo of job `seq` ran: its point is not offered again.
    pub fn forget(&mut self, seq: u64) {
        self.session.retain(|p| p.seq != seq);
    }

    pub fn forget_id(&mut self, id: &str) {
        self.session.retain(|p| p.id != id);
    }
}

/// What Ctrl+Z does now.
pub enum UndoChoice {
    /// Run the quick undo of `tab.undo`.
    Quick,
    /// Ask to restore this point.
    Point(SessionPoint),
    Nothing,
}

pub fn undo_choice(tab: &RepoTab) -> UndoChoice {
    let quick = tab.undo.last().map(|u| u.seq);
    let point = tab.timeline.session.last();
    match (quick, point) {
        (Some(q), Some(p)) if p.seq > q => UndoChoice::Point(p.clone()),
        (Some(_), _) => UndoChoice::Quick,
        (None, Some(p)) => UndoChoice::Point(p.clone()),
        (None, None) => UndoChoice::Nothing,
    }
}

/// The label of what Ctrl+Z undoes, if anything.
pub fn undo_label(tab: &RepoTab) -> Option<String> {
    match undo_choice(tab) {
        UndoChoice::Quick => tab.undo.last().map(|u| u.label.clone()),
        UndoChoice::Point(p) => Some(p.label),
        UndoChoice::Nothing => None,
    }
}
