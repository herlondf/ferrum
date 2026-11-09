//! Toasts: short notifications in the bottom-right corner for finished
//! operations ("Pushed 3 commits to origin/main"). They fade out after a few
//! seconds (not while hovered); errors stay until dismissed and offer Copy.
//!
//! They come from the messages tabs already set (`RepoTab::info/error`); an
//! operation started from the UI leaves a nicer text for its "done" message
//! (`Toasts::started`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, RichText};

use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::tab::{Message, RepoTab};
use crate::theme;

/// How long a success or info toast stays.
const LIFE: Duration = Duration::from_millis(4500);
const FADE: f32 = 0.4;
const WIDTH: f32 = 340.0;
const MAX: usize = 5;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Success,
    Info,
    Error,
}

#[derive(Debug)]
pub struct Toast {
    id: u64,
    pub kind: Kind,
    pub text: String,
    /// The repository, for messages of a tab that is not the active one.
    pub repo: Option<String>,
    /// Offers Undo (history was rewritten in the active tab).
    pub undo: bool,
    born: Instant,
    /// A button, e.g. "Undo" after an operation that saved a restore point.
    pub action: Option<(String, ToastAction)>,
}

#[derive(Debug, Clone)]
pub enum ToastAction {
    /// Ask to restore point `id` of the repository at `repo`.
    Restore { repo: PathBuf, id: String },
}

struct Pending {
    repo: PathBuf,
    /// The message the tab sets when the operation is done.
    done: String,
    text: String,
    undo: bool,
}

#[derive(Default)]
pub struct Toasts {
    pub list: Vec<Toast>,
    /// The time of the last message seen per repository (`None`: the app's own).
    seen: HashMap<Option<PathBuf>, Instant>,
    pending: Vec<Pending>,
    next: u64,
}

impl Toasts {
    pub fn push(&mut self, kind: Kind, text: impl Into<String>, repo: Option<String>) {
        let text = text.into();
        // The same message again (e.g. F5 twice) restarts the toast instead.
        if let Some(t) = self
            .list
            .iter_mut()
            .rev()
            .find(|t| t.text == text && t.kind == kind && t.repo == repo)
        {
            t.born = Instant::now();
            return;
        }
        self.next += 1;
        self.list.push(Toast {
            id: self.next,
            kind,
            text,
            repo,
            undo: false,
            born: Instant::now(),
            action: None,
        });
        // Too many: the oldest that is not an error goes first.
        while self.list.len() > MAX {
            let i = self
                .list
                .iter()
                .position(|t| t.kind != Kind::Error)
                .unwrap_or(0);
            self.list.remove(i);
        }
    }

    /// An operation starts in `tab`: when it is done, say what it did.
    pub fn started(&mut self, tab: &RepoTab, op: &Op) {
        let Some(text) = describe(tab, op) else {
            return;
        };
        if self.pending.len() > 20 {
            self.pending.remove(0);
        }
        self.pending.push(Pending {
            repo: tab.path().to_path_buf(),
            done: t!("{} — done", op.label()),
            text,
            undo: matches!(op, Op::Stack(s) if s.rewrites()),
        });
    }

    /// Turns new messages of the tabs (and of the app) into toasts.
    pub fn observe(&mut self, tabs: &[RepoTab], active: usize, app: Option<&Message>) {
        for (i, tab) in tabs.iter().enumerate() {
            let Some(m) = &tab.message else { continue };
            let key = Some(tab.path().to_path_buf());
            if self.seen.get(&key).is_some_and(|at| *at >= m.at) {
                continue;
            }
            self.seen.insert(key, m.at);
            let repo = (i != active && tabs.len() > 1).then(|| tab.name.clone());
            let before = self.next;
            self.message(m, Some(tab.path().to_path_buf()), repo);
            // A rewrite that stopped for conflicts did not finish: no Undo.
            if tab.state != ferrum_core::RepoState::Clean
                && let Some(t) = self.list.last_mut().filter(|t| t.id > before && t.undo)
            {
                t.undo = false;
                t.kind = Kind::Info;
                t.text = t!("Stopped for conflicts: resolve them, then continue or abort.");
            }
        }
        if let Some(m) = app
            && !self.seen.get(&None).is_some_and(|at| *at >= m.at)
        {
            self.seen.insert(None, m.at);
            self.message(m, None, None);
        }
    }

    fn message(&mut self, m: &Message, path: Option<PathBuf>, repo: Option<String>) {
        if m.error {
            self.pending.retain(|p| Some(&p.repo) != path.as_ref());
            self.push(Kind::Error, m.text.clone(), repo);
            return;
        }
        if let Some(i) = self
            .pending
            .iter()
            .position(|p| Some(&p.repo) == path.as_ref() && p.done == m.text)
        {
            let p = self.pending.remove(i);
            let undo = p.undo && repo.is_none();
            self.push(Kind::Success, p.text, repo);
            if let Some(t) = self.list.last_mut() {
                t.undo = undo;
            }
            return;
        }
        let kind = if is_done(&m.text) {
            Kind::Success
        } else {
            Kind::Info
        };
        self.push(kind, m.text.clone(), repo);
    }

    /// Gives the newest toast a button.
    pub fn attach(&mut self, label: String, action: ToastAction) {
        if let Some(t) = self.list.last_mut() {
            t.action = Some((label, action));
            t.born = Instant::now();
        }
    }

    /// Draws the toasts above the status bar, whose top is at `bottom`.
    pub fn show(&mut self, ctx: &egui::Context, bottom: f32, cmds: &mut Commands) {
        let now = Instant::now();
        self.list
            .retain(|t| t.kind == Kind::Error || now.duration_since(t.born) < LIFE);
        if self.list.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let offset = egui::vec2(-theme::SPACE_L, bottom - screen.bottom() - theme::SPACE_M);
        let mut closed = Vec::new();
        let mut keep = Vec::new();
        egui::Area::new(egui::Id::unique("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, offset)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = theme::SPACE_M;
                for t in &self.list {
                    let left = LIFE
                        .saturating_sub(now.duration_since(t.born))
                        .as_secs_f32();
                    let alpha = if t.kind == Kind::Error {
                        1.0
                    } else {
                        (left / FADE).clamp(0.0, 1.0)
                    };
                    let r = ui.scope(|ui| {
                        ui.multiply_opacity(alpha);
                        toast(ui, t, cmds)
                    });
                    if r.inner {
                        closed.push(t.id);
                    }
                    if r.response.contains_pointer() {
                        keep.push(t.id);
                    }
                }
            });
        for t in &mut self.list {
            // Hovering keeps a toast up.
            if keep.contains(&t.id) && t.kind != Kind::Error {
                t.born = now;
            }
        }
        self.list.retain(|t| !closed.contains(&t.id));
        if self.list.iter().any(|t| t.kind != Kind::Error) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

/// Whether `text` is a "… — done" message.
fn is_done(text: &str) -> bool {
    let pattern = t!("{} — done", "\u{0}");
    match pattern.split_once('\u{0}') {
        Some((pre, post)) => {
            text.len() > pre.len() + post.len() && text.starts_with(pre) && text.ends_with(post)
        }
        None => false,
    }
}

/// One toast; returns whether it was closed.
fn toast(ui: &mut egui::Ui, t: &Toast, cmds: &mut Commands) -> bool {
    let v = ui.visuals().clone();
    let (icon, color) = match t.kind {
        Kind::Success => (icons::CHECK_CIRCLE, crate::util::added_color(ui)),
        Kind::Info => (icons::INFO, theme::accent(ui)),
        Kind::Error => (icons::WARNING_CIRCLE, v.error_fg_color),
    };
    let mut closed = false;
    let frame = egui::Frame::new()
        .fill(v.window_fill)
        .stroke(v.window_stroke)
        .corner_radius(theme::RADIUS_LARGE)
        .shadow(v.popup_shadow)
        .inner_margin(egui::Margin {
            left: 12,
            right: 8,
            top: 10,
            bottom: 10,
        });
    let resp = frame.show(ui, |ui| {
        ui.set_width(WIDTH);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_M;
            ui.label(RichText::new(icon).size(18.0).color(color));
            let w = ui.available_width() - 24.0;
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing.y = theme::SPACE_XS;
                if let Some(repo) = &t.repo {
                    ui.label(theme::strong(repo).size(11.5).color(v.weak_text_color()));
                }
                let mut lines = t.text.lines();
                let first = lines.next().unwrap_or_default();
                let rest: Vec<&str> = lines.collect();
                ui.add(egui::Label::new(theme::strong(first).color(v.strong_text_color())).wrap());
                if !rest.is_empty() {
                    // Long git output: a few lines here, all of it in the tooltip.
                    let mut shown = rest.iter().take(4).copied().collect::<Vec<_>>().join("\n");
                    if rest.len() > 4 {
                        shown.push_str("\n…");
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(shown).size(11.5).color(v.weak_text_color()),
                        )
                        .wrap(),
                    )
                    .on_hover_text(&t.text);
                }
                // Stack rewrites undo through their backup (unless a restore point offers it).
                if t.undo && t.action.is_none() {
                    ui.add_space(theme::SPACE_XS);
                    let label = t!("Undo");
                    let undo = egui::Button::new(icons::text(icons::UNDO, &label)).small();
                    let resp = ui.add(undo);
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                    });
                    if resp.on_hover_text("Ctrl+Z").clicked() {
                        cmds.push(Command::Undo);
                        closed = true;
                    }
                }
                if t.kind == Kind::Error || t.action.is_some() {
                    ui.add_space(theme::SPACE_XS);
                    ui.horizontal(|ui| {
                        if let Some((label, action)) = &t.action
                            && ui.small_button(label).clicked()
                        {
                            cmds.push(action_command(action));
                            closed = true;
                        }
                        if t.kind == Kind::Error {
                            if ui.small_button(t!("Copy")).clicked() {
                                cmds.push(Command::Copy(t.text.clone()));
                            }
                            crate::ui::repo_dialogs::error_actions(ui, &t.text, cmds);
                        }
                    });
                }
            });
            let dismiss = t!("Dismiss");
            let resp = ui.add(
                egui::Button::new(RichText::new(icons::CLOSE).color(v.weak_text_color()))
                    .frame_when_inactive(false)
                    .small(),
            );
            // Named for screen readers, not by its icon.
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &dismiss));
            if resp.on_hover_text(&dismiss).clicked() {
                closed = true;
            }
        });
    });
    // A colored edge on the left says what kind of message it is.
    let r = resp.response.rect.shrink(1.0);
    let edge = egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 3.0, r.bottom()));
    ui.painter().rect_filled(
        edge,
        egui::CornerRadius {
            nw: theme::RADIUS_LARGE - 1,
            sw: theme::RADIUS_LARGE - 1,
            ne: 0,
            se: 0,
        },
        color,
    );
    closed
}

fn action_command(a: &ToastAction) -> Command {
    match a {
        ToastAction::Restore { repo, id } => Command::Safety(
            crate::safety::SafetyCmd::ConfirmRestore(repo.clone(), id.clone()),
        ),
    }
}

fn commits(n: usize) -> String {
    if n == 1 {
        t!("1 commit")
    } else {
        t!("{} commits", n)
    }
}

fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or_default().trim();
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(59).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// The `FERRUM_START` step `toast:success|info|error` shows a sample toast.
pub fn dev_step(toasts: &mut Toasts, step: &str) {
    let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
    match (cmd, arg) {
        ("toast", "error") => toasts.push(
            Kind::Error,
            "git push failed: ! [rejected] main -> main (fetch first)\nhint: Updates were rejected because the remote contains work that you do not have locally.",
            None,
        ),
        ("toast", "info") => toasts.push(Kind::Info, t!("Copied to the clipboard"), None),
        ("toast", _) => toasts.push(
            Kind::Success,
            t!("Pushed {} to {}", commits(3), "origin/main"),
            Some("ferrum".into()),
        ),
        _ => eprintln!("FERRUM_START: unknown step {step}"),
    }
}

/// What a finished operation did, in words; `None` keeps "… — done".
pub fn describe(tab: &RepoTab, op: &Op) -> Option<String> {
    let current = tab.current_branch();
    let (ahead, behind) = current.and_then(|b| b.ahead_behind).unwrap_or((0, 0));
    let upstream = current.and_then(|b| b.upstream.clone());
    let fetched = |remotes: &[String]| match remotes {
        [] if tab.remotes.len() == 1 => t!("Fetched {}", tab.remotes[0].name),
        [] => t!("Fetched {} remotes", tab.remotes.len()),
        [r] => t!("Fetched {}", r),
        list => t!("Fetched {} remotes", list.len()),
    };
    let pulled = |from: Option<String>| match (from, behind) {
        (Some(u), n) if n > 0 => Some(t!("Pulled {} from {}", commits(n), u)),
        (Some(u), _) => Some(t!("Pulled from {}", u)),
        (None, _) => None,
    };
    Some(match op {
        Op::Fetch { remote: Some(r) } => fetched(std::slice::from_ref(r)),
        Op::Fetch { remote: None } => fetched(&[]),
        Op::FetchWith(o) => fetched(&o.remotes),
        Op::Pull(_) => pulled(upstream)?,
        Op::PullWith(o) => match (&o.remote, &o.branch) {
            (Some(r), Some(b)) => t!("Pulled from {}/{}", r, b),
            _ => pulled(upstream)?,
        },
        Op::PushRefs(r) if r.tags.is_empty() && !r.all_tags => match r.branches.as_slice() {
            [(local, to)] => {
                let target = format!("{}/{to}", r.remote);
                let is_current = tab.head.as_deref() == Some(local.as_str());
                if r.set_upstream {
                    t!("Published {} to {}", local, target)
                } else if is_current && ahead > 0 {
                    t!("Pushed {} to {}", commits(ahead), target)
                } else {
                    t!("Pushed branch {} to {}", local, target)
                }
            }
            list => t!("Pushed {} branches to {}", list.len(), r.remote),
        },
        Op::Commit { message, amend, .. } => {
            if *amend {
                t!("Amended “{}”", first_line(message))
            } else {
                t!("Committed “{}”", first_line(message))
            }
        }
        Op::StashPush { .. } => t!("Stashed the local changes"),
        Op::StashApply { stash, pop: true } => t!("Popped {}", first_line(&stash.message)),
        Op::Checkout(b) => t!("Switched to {}", b),
        Op::CreateBranch { name, .. } => t!("Created branch {}", name),
        Op::Stack(s) => s.describe()?,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_messages_are_successes() {
        assert!(is_done(&t!("{} — done", "Commit")));
        assert!(!is_done("Refreshed"));
    }

    #[test]
    fn errors_stay_and_others_are_capped() {
        let mut t = Toasts::default();
        t.push(Kind::Error, "boom", None);
        for i in 0..10 {
            t.push(Kind::Info, format!("info {i}"), None);
        }
        assert_eq!(t.list.len(), MAX);
        assert_eq!(t.list[0].kind, Kind::Error);
        // The same text again does not stack.
        t.push(Kind::Info, "info 9", None);
        assert_eq!(t.list.len(), MAX);
    }
}
