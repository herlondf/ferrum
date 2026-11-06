//! Command palette (Ctrl+P): every action, branch, pinned ref, file, tab and
//! custom command, searchable by name and grouped by kind.
//!
//! Other modules add entries through `PROVIDERS`: write a
//! `fn(&Settings, Option<&RepoTab>, &mut Vec<Entry>)` and list it there.

use eframe::egui;
use ferrum_core::BranchKind;

use crate::command::{Command, Commands, Op};
use crate::keymap::{self, Action};
use crate::settings::Settings;
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;

/// Adds palette entries (see the module docs).
pub type Provider = fn(&Settings, Option<&RepoTab>, &mut Vec<Entry>);

/// Extra entry sources. Add a line to register one.
const PROVIDERS: &[Provider] = &[crate::ui::custom_commands::palette_entries];

/// File matches shown at most.
const MAX_FILES: usize = 30;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    query: String,
    selected: usize,
    just_opened: bool,
}

impl Palette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
        self.just_opened = self.open;
    }

    /// Dev aid: open with a query.
    pub fn open_with(&mut self, query: &str) {
        self.open = true;
        self.query = query.to_owned();
        self.selected = 0;
        self.just_opened = true;
    }
}

/// The kind of an entry; results are grouped under these headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Commands,
    Custom,
    Branches,
    Pinned,
    Files,
    Tabs,
    /// For the workspaces' provider.
    #[allow(dead_code)]
    Workspaces,
}

impl Group {
    fn label(self) -> String {
        match self {
            Group::Commands => t!("Commands"),
            Group::Custom => t!("Custom commands"),
            Group::Branches => t!("Branches"),
            Group::Pinned => t!("Pinned"),
            Group::Files => t!("Files"),
            Group::Tabs => t!("Tabs"),
            Group::Workspaces => t!("Workspaces"),
        }
    }
}

type Make = Box<dyn Fn() -> Command>;

pub struct Entry {
    pub group: Group,
    pub label: String,
    /// Also searched, not shown.
    pub keywords: String,
    pub shortcut: String,
    make: Make,
}

impl Entry {
    pub fn new(
        group: Group,
        label: impl Into<String>,
        shortcut: impl Into<String>,
        make: impl Fn() -> Command + 'static,
    ) -> Self {
        Self {
            group,
            label: label.into(),
            keywords: String::new(),
            shortcut: shortcut.into(),
            make: Box::new(make),
        }
    }

    /// Shows the key of `a` (from the keymap).
    pub fn key(mut self, a: Action) -> Self {
        self.shortcut = keymap::text(a);
        self
    }

    pub fn keywords(mut self, words: &str) -> Self {
        self.keywords = words.to_owned();
        self
    }
}

/// A command entry with the key of `a`.
fn action(label: String, a: Action, make: impl Fn() -> Command + 'static) -> Entry {
    Entry::new(Group::Commands, label, "", make).key(a)
}

fn command(label: String, make: impl Fn() -> Command + 'static) -> Entry {
    Entry::new(Group::Commands, label, "", make)
}

fn entries(tab: Option<&RepoTab>, tab_names: &[String], settings: &Settings) -> Vec<Entry> {
    let mut list = vec![
        action(t!("Open repository…"), Action::OpenRepo, || {
            Command::OpenRepoDialog
        }),
        command(t!("Clone repository…"), || {
            Command::Dialog(Dialog::Clone {
                url: String::new(),
                parent: String::new(),
                name: String::new(),
            })
        }),
        command(t!("New repository…"), || {
            Command::Dialog(Dialog::Init {
                path: String::new(),
            })
        }),
        action(t!("Settings"), Action::Settings, || Command::ShowSettings),
        command(t!("Keyboard shortcuts"), || {
            Command::DevStep(crate::command::DevStep::SettingsSection("shortcuts".into()))
        })
        .keywords("keymap keys bindings"),
        command(t!("Custom commands"), || {
            Command::DevStep(crate::command::DevStep::SettingsSection("commands".into()))
        }),
    ];
    if let Some(t) = tab {
        commands_for(t, settings, &mut list);
    }
    for (i, name) in tab_names.iter().enumerate() {
        list.push(
            Entry::new(Group::Tabs, t!("Switch to {}", name), "", move || {
                Command::SelectTab(i)
            })
            .key(Action::NextTab),
        );
    }
    for p in PROVIDERS {
        p(settings, tab, &mut list);
    }
    list
}

fn commands_for(t: &RepoTab, settings: &Settings, list: &mut Vec<Entry>) {
    let pull = settings.pull.mode();
    let path = t.path().to_path_buf();
    let p2 = path.clone();
    list.extend([
        action(t!("Show history"), Action::ShowHistory, || {
            Command::SetView(View::History)
        }),
        action(t!("Show local changes"), Action::ShowChanges, || {
            Command::SetView(View::Changes)
        }),
        action(t!("Search commits"), Action::Find, || Command::FocusSearch),
        command(t!("File history…"), || {
            crate::ui::history_options::cmd(|t, _| crate::ui::file_finder::open(t))
        }),
        action(t!("Refresh"), Action::Refresh, || Command::Refresh),
        action(t!("Undo last action"), Action::Undo, || Command::Undo),
        action(t!("Fetch all remotes"), Action::Fetch, || {
            Command::Run(Op::Fetch { remote: None })
        }),
        action(t!("Pull"), Action::Pull, move || {
            Command::Run(Op::Pull(pull))
        }),
        {
            let d = crate::ui::sync_dialogs::pull(t);
            command(t!("Pull with options…"), move || {
                Command::Dialog(Dialog::Sync(d.clone()))
            })
        },
        {
            let d = crate::ui::sync_dialogs::fetch(t, None);
            command(t!("Fetch with options…"), move || {
                Command::Dialog(Dialog::Sync(d.clone()))
            })
        },
        {
            let d = crate::ui::sync_dialogs::push_tags(t, &[]);
            command(t!("Push tags…"), move || {
                Command::Dialog(Dialog::Sync(d.clone()))
            })
        },
        command(t!("Stage all changes"), || Command::Run(Op::StageAll)),
        command(t!("Unstage all changes"), || Command::Run(Op::UnstageAll)),
        action(t!("Stash changes…"), Action::Stash, || {
            Command::Dialog(Dialog::stash())
        }),
        command(t!("Add remote…"), || {
            Command::Dialog(Dialog::AddRemote {
                name: String::new(),
                url: String::new(),
            })
        }),
        action(t!("Open in terminal"), Action::Terminal, move || {
            Command::External(crate::external::Launch::Terminal { dir: path.clone() })
        }),
        action(t!("Show in file manager"), Action::FileManager, move || {
            Command::External(crate::external::Launch::Reveal { path: p2.clone() })
        }),
    ]);
    let head = t.head.clone().unwrap_or_else(|| "HEAD".into());
    let h2 = head.clone();
    list.push(action(t!("New branch…"), Action::NewBranch, move || {
        Command::Dialog(Dialog::create_branch(&h2, &h2))
    }));
    let h3 = head.clone();
    list.push(action(t!("New tag…"), Action::NewTag, move || {
        Command::Dialog(Dialog::create_tag("HEAD", &h3))
    }));
    list.push(action(
        t!("Check out branch…"),
        Action::QuickCheckout,
        || Command::Dialog(crate::ui::ref_dialogs::quick_checkout()),
    ));
    list.push(command(t!("Quick stash"), || {
        Command::Run(crate::ui::ref_dialogs::quick_stash())
    }));
    if let Some(b) = t.head.clone() {
        let push = crate::ui::sync_dialogs::push(t, &b);
        list.push(action(t!("Push…"), Action::Push, move || {
            Command::Dialog(Dialog::Sync(push.clone()))
        }));
    }
    if let Some(s) = t.stashes.first().cloned() {
        list.push(command(t!("Pop latest stash ({})", s.message), move || {
            Command::Run(Op::StashApply {
                stash: s.clone(),
                pop: true,
            })
        }));
    }
    let id = t.identity.clone();
    list.push(command(t!("Set repository identity…"), move || {
        Command::Dialog(Dialog::identity(&id))
    }));
    if let Some(h) = &t.hosted {
        list.push(command(t!("Show pull requests"), || {
            Command::SetView(View::PullRequests)
        }));
        if t.head.is_some() {
            let d = Command::Dialog(Dialog::Forge(crate::ui::forge::new_pull_request(t, &head)));
            list.push(
                command(t!("Create {}…", h.kind.request_name()), once(d))
                    .keywords("create pull request merge request pr mr"),
            );
        }
    }
    if t.uses_lfs {
        list.push(command("Git LFS: pull".into(), || {
            Command::Run(Op::Lfs("pull"))
        }));
        list.push(command("Git LFS: fetch".into(), || {
            Command::Run(Op::Lfs("fetch"))
        }));
    }
    if !t.submodules.is_empty() {
        list.push(command(t!("Update submodules"), || {
            Command::Run(Op::SubmoduleUpdate)
        }));
    }
    if let Dialog::WorktreeAdd { path, rev, .. } = Dialog::worktree_add(t) {
        list.push(
            command(t!("Add worktree…"), move || {
                Command::Dialog(Dialog::WorktreeAdd {
                    path: path.clone(),
                    rev: rev.clone(),
                    new_branch: String::new(),
                })
            })
            .keywords("create new worktree"),
        );
    }
    if t.head_oid.is_some() {
        let base = t.repo.resolve("HEAD~10").ok();
        list.push(command(
            t!("Interactive rebase of the last 10 commits…"),
            move || Command::PlanRebase(base),
        ));
    }
    // Pinned refs (see the sidebar): jump to their commit.
    for refname in &t.sidebar.prefs.pinned {
        let short = ["refs/heads/", "refs/remotes/", "refs/tags/"]
            .iter()
            .find_map(|p| refname.strip_prefix(p))
            .unwrap_or(refname);
        if let Ok(id) = t.repo.resolve(refname) {
            list.push(Entry::new(
                Group::Pinned,
                t!("📌 Show {}", short),
                "",
                move || Command::SelectCommit(id),
            ));
        }
    }
    for b in t.branches.iter().filter(|b| !b.is_head) {
        match b.kind {
            BranchKind::Local => {
                let name = b.name.clone();
                list.push(Entry::new(
                    Group::Branches,
                    t!("Check out {}", name),
                    "",
                    move || Command::Run(Op::Checkout(name.clone())),
                ));
            }
            BranchKind::Remote => {
                let tracked = t
                    .branches
                    .iter()
                    .any(|l| l.kind == BranchKind::Local && l.upstream.as_deref() == Some(&b.name));
                if !tracked && !b.name.ends_with("/HEAD") {
                    let cmd = crate::ui::sync_dialogs::checkout_remote(t, b);
                    list.push(Entry::new(
                        Group::Branches,
                        t!("Check out {}", b.name),
                        "",
                        once(cmd),
                    ));
                }
            }
        }
    }
}

/// A command built ahead (entries run at most once).
fn once(cmd: Command) -> impl Fn() -> Command + 'static {
    let cell = std::cell::RefCell::new(Some(cmd));
    move || cell.borrow_mut().take().unwrap_or(Command::Refresh)
}

/// Characters of `query` appear in `text` in order (case-insensitive).
pub(crate) fn fuzzy_score(text: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let text = text.to_lowercase();
    if let Some(pos) = text.find(query) {
        return Some(1000 - pos as i32);
    }
    let mut chars = text.chars();
    for q in query.chars() {
        chars.find(|&c| c == q)?;
    }
    Some(0)
}

fn entry_score(e: &Entry, q: &str) -> Option<i32> {
    let label = fuzzy_score(&e.label, q);
    let words = (!e.keywords.is_empty())
        .then(|| {
            let lower = e.keywords.to_lowercase();
            q.split_whitespace()
                .all(|w| lower.contains(w))
                .then_some(500)
        })
        .flatten();
    label.max(words)
}

/// Entries matching `q`, grouped (best group first) and sorted by score.
fn ranked(all: Vec<Entry>, q: &str) -> Vec<Entry> {
    let mut scored: Vec<(i32, Entry)> = all
        .into_iter()
        .filter_map(|e| entry_score(&e, q).map(|s| (s, e)))
        .collect();
    scored.sort_by_key(|(s, e)| (e.group, std::cmp::Reverse(*s)));
    let mut groups: Vec<(Group, i32, Vec<Entry>)> = Vec::new();
    for (s, e) in scored {
        match groups.last_mut() {
            Some(g) if g.0 == e.group => g.2.push(e),
            _ => groups.push((e.group, s, vec![e])),
        }
    }
    if !q.is_empty() {
        // Stable: equal best scores keep the fixed group order.
        groups.sort_by_key(|g| std::cmp::Reverse(g.1));
    }
    groups.into_iter().flat_map(|g| g.2).collect()
}

fn files(tab: &RepoTab, q: &str) -> Vec<Entry> {
    if q.is_empty() {
        return Vec::new();
    }
    let all = crate::ui::file_finder::cached(tab);
    let mut scored: Vec<(i32, &String)> = all
        .iter()
        .filter_map(|p| crate::ui::file_finder::score(p, q).map(|s| (s, p)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(MAX_FILES)
        .map(|(_, p)| {
            let path = p.clone();
            let icon = if p.ends_with('/') { "📁" } else { "📄" };
            Entry::new(Group::Files, format!("{icon} {p}"), "", move || {
                Command::FileHistory(path.clone())
            })
        })
        .collect()
}

pub fn show(
    ctx: &egui::Context,
    p: &mut Palette,
    mut tab: Option<&mut RepoTab>,
    tab_names: &[String],
    settings: &Settings,
    cmds: &mut Commands,
) {
    if !p.open {
        return;
    }
    if let Some(t) = tab.as_deref_mut() {
        crate::ui::file_finder::preload(t);
        crate::ui::file_finder::poll_candidates(ctx, t);
    }
    let tab = tab.as_deref();
    let q = p.query.trim().to_lowercase();
    let mut all = entries(tab, tab_names, settings);
    if let Some(t) = tab {
        all.extend(files(t, &q));
    }
    let matches = ranked(all, &q);
    p.selected = p.selected.min(matches.len().saturating_sub(1));

    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if up {
        p.selected = p.selected.saturating_sub(1);
    }
    if down && p.selected + 1 < matches.len() {
        p.selected += 1;
    }

    let mut run = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("palette"))
        .fixed_pos(egui::pos2(screen.center().x - 260.0, screen.top() + 80.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(520.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut p.query)
                        .hint_text(t!("Type a command, branch or file…"))
                        .desired_width(f32::INFINITY),
                );
                if p.just_opened {
                    resp.request_focus();
                    p.just_opened = false;
                }
                if resp.changed() {
                    p.selected = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // Leave room for the scroll bar.
                        let width = ui.available_width() - 12.0;
                        let mut group = None;
                        for (i, e) in matches.iter().enumerate() {
                            if group != Some(e.group) {
                                group = Some(e.group);
                                if i > 0 {
                                    ui.add_space(4.0);
                                }
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(width, 20.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().text(
                                    r.left_center() + egui::vec2(6.0, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    e.group.label(),
                                    egui::FontId::proportional(11.0),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                            if row(ui, e, width, i == p.selected, up || down) {
                                run = Some(i);
                            }
                        }
                        if matches.is_empty() {
                            ui.weak(t!("No matching command."));
                        }
                    });
            });
        });
    if enter && !matches.is_empty() {
        run = Some(p.selected);
    }
    if let Some(i) = run {
        cmds.push((matches[i].make)());
        p.open = false;
    }
    if esc {
        p.open = false;
    }
}

/// One result row. Returns true when clicked.
fn row(ui: &mut egui::Ui, e: &Entry, width: f32, selected: bool, moved: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &e.label));
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if selected {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };
    let label_area = rect.with_max_x(rect.right() - 110.0);
    ui.painter().with_clip_rect(label_area).text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &e.label,
        egui::FontId::proportional(13.5),
        color,
    );
    if !e.shortcut.is_empty() {
        ui.painter().text(
            rect.right_center() - egui::vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            &e.shortcut,
            egui::FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    }
    if selected && moved {
        resp.scroll_to_me(None);
    }
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching() {
        assert!(fuzzy_score("Fetch all remotes", "fetch").is_some());
        assert!(fuzzy_score("Fetch all remotes", "far").is_some());
        assert!(fuzzy_score("Fetch all remotes", "xyz").is_none());
        assert!(fuzzy_score("Pull", "pu") > fuzzy_score("Push upstream", "pl"));
    }

    #[test]
    fn groups_results_best_first() {
        let e = |g, l: &str| Entry::new(g, l, "", || Command::Refresh);
        let list = vec![
            e(Group::Commands, "Fetch all remotes"),
            e(Group::Branches, "Check out feature"),
            e(Group::Files, "📄 feature.rs"),
            e(Group::Commands, "New branch…"),
        ];
        let out = ranked(list, "feature");
        let labels: Vec<&str> = out.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["📄 feature.rs", "Check out feature"]);
        // Without a query: the fixed group order.
        let list = vec![e(Group::Tabs, "Switch to x"), e(Group::Commands, "Refresh")];
        assert_eq!(ranked(list, "")[0].group, Group::Commands);
        // Keywords are searched too.
        let list = vec![e(Group::Commands, "Add worktree…").keywords("create new worktree")];
        assert_eq!(ranked(list, "create worktree").len(), 1);
    }
}
