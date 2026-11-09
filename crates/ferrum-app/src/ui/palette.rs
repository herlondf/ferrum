//! Command palette (Ctrl+P): every action, branch, pinned ref, file, tab and
//! custom command, searchable by name and grouped by kind.
//!
//! Other modules add entries through `PROVIDERS`: write a
//! `fn(&Settings, Option<&RepoTab>, &mut Vec<Entry>)` and list it there.

use eframe::egui;
use ferrum_core::BranchKind;

use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::keymap::{self, Action};
use crate::settings::Settings;
use crate::tab::{RepoTab, View};
use crate::theme;
use crate::ui::dialogs::Dialog;

/// Adds palette entries (see the module docs).
pub type Provider = fn(&Settings, Option<&RepoTab>, &mut Vec<Entry>);

/// Extra entry sources. Add a line to register one.
const PROVIDERS: &[Provider] = &[
    crate::ui::custom_commands::palette_entries,
    crate::ui::repo_manager::palette_entries,
];

/// File matches shown at most.
const MAX_FILES: usize = 30;
/// Entries run last that are remembered, and shown first without a query.
const MAX_RECENT: usize = 12;
const SHOWN_RECENT: usize = 5;

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
    /// Run lately (shown first when nothing is typed).
    Recent,
    Commands,
    Custom,
    Branches,
    Pinned,
    Files,
    Tabs,
    Workspaces,
}

impl Group {
    fn label(self) -> String {
        match self {
            Group::Recent => t!("Recently used"),
            Group::Commands => t!("Commands"),
            Group::Custom => t!("Custom commands"),
            Group::Branches => t!("Branches"),
            Group::Pinned => t!("Pinned"),
            Group::Files => t!("Files"),
            Group::Tabs => t!("Tabs"),
            Group::Workspaces => t!("Workspaces"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Group::Recent => icons::HISTORY,
            Group::Commands => icons::COMMAND,
            Group::Custom => icons::WRENCH,
            Group::Branches => icons::BRANCH,
            Group::Pinned => icons::PIN,
            Group::Files => icons::FILE,
            Group::Tabs => icons::BROWSERS,
            Group::Workspaces => icons::REPOSITORIES,
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
    /// Shown before the label (default: the group's icon).
    pub icon: &'static str,
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
            icon: group.icon(),
            make: Box::new(make),
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
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
        action(t!("Operation timeline"), Action::Timeline, || {
            Command::Safety(crate::safety::SafetyCmd::ToggleTimeline)
        }),
        command(t!("Learning mode"), || {
            Command::Safety(crate::safety::SafetyCmd::ToggleLearning)
        }),
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
    list.extend(crate::ui::stacks::palette_entries(t));
    // Pinned refs (see the sidebar): jump to their commit.
    for refname in &t.sidebar.prefs.pinned {
        let short = ["refs/heads/", "refs/remotes/", "refs/tags/"]
            .iter()
            .find_map(|p| refname.strip_prefix(p))
            .unwrap_or(refname);
        if let Ok(id) = t.repo.resolve(refname) {
            list.push(Entry::new(
                Group::Pinned,
                t!("Show {}", short),
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
/// `recent` (labels, newest first) come first without a query, and rank
/// higher with one. Returns each entry with the group it is shown under.
fn ranked(all: Vec<Entry>, q: &str, recent: &[String]) -> Vec<(Group, Entry)> {
    let rank = |e: &Entry| recent.iter().position(|r| *r == e.label);
    let mut scored: Vec<(Group, i32, Entry)> = all
        .into_iter()
        .filter_map(|e| {
            let s = entry_score(&e, q)?;
            Some(match rank(&e) {
                Some(r) if q.is_empty() && r < SHOWN_RECENT => (Group::Recent, -(r as i32), e),
                Some(r) => (e.group, s + 200 - 10 * r as i32, e),
                None => (e.group, s, e),
            })
        })
        .collect();
    scored.sort_by_key(|(g, s, _)| (*g, std::cmp::Reverse(*s)));
    let mut groups: Vec<(Group, i32, Vec<Entry>)> = Vec::new();
    for (g, s, e) in scored {
        match groups.last_mut() {
            Some(last) if last.0 == g => last.2.push(e),
            _ => groups.push((g, s, vec![e])),
        }
    }
    if !q.is_empty() {
        // Stable: equal best scores keep the fixed group order.
        groups.sort_by_key(|g| std::cmp::Reverse(g.1));
    }
    groups
        .into_iter()
        .flat_map(|(g, _, list)| list.into_iter().map(move |e| (g, e)))
        .collect()
}

/// Character positions of `text` to highlight for `query` (lowercase): the
/// first substring match, or else the characters matched in order.
pub(crate) fn matched_chars(text: &str, query: &str) -> Vec<usize> {
    let lower: Vec<char> = text
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect();
    let q: Vec<char> = query.chars().collect();
    if q.is_empty() || q.len() > lower.len() {
        return Vec::new();
    }
    if let Some(start) = (0..=lower.len() - q.len()).find(|&i| lower[i..i + q.len()] == q[..]) {
        return (start..start + q.len()).collect();
    }
    let mut out = Vec::new();
    let mut want = q.iter().peekable();
    for (i, c) in lower.iter().enumerate() {
        if want.peek() == Some(&c) {
            out.push(i);
            want.next();
        }
    }
    if want.peek().is_some() {
        return Vec::new();
    }
    out
}

/// Remembers that `label` was run.
fn remember(recent: &mut Vec<String>, label: &str) {
    recent.retain(|l| l != label);
    recent.insert(0, label.to_owned());
    recent.truncate(MAX_RECENT);
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
            let icon = if p.ends_with('/') {
                icons::FOLDER
            } else {
                icons::FILE
            };
            Entry::new(Group::Files, p.clone(), "", move || {
                Command::FileHistory(path.clone())
            })
            .icon(icon)
        })
        .collect()
}

pub fn show(
    ctx: &egui::Context,
    p: &mut Palette,
    mut tab: Option<&mut RepoTab>,
    tab_names: &[String],
    settings: &mut Settings,
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
    let matches = ranked(all, &q, &settings.palette_recent);
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
    let width = WIDTH.min(screen.width() - 40.0);
    egui::Area::new(egui::Id::unique("palette"))
        .fixed_pos(egui::pos2(
            screen.center().x - width / 2.0,
            screen.top() + 72.0,
        ))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let frame = egui::Frame::popup(ui.style())
                .corner_radius(theme::RADIUS_LARGE)
                .inner_margin(egui::Margin::same(theme::SPACE_M as i8));
            frame.show(ui, |ui| {
                ui.set_width(width);
                if search_field(ui, p) {
                    p.selected = 0;
                }
                ui.add_space(theme::SPACE_S);
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // Leave room for the scroll bar.
                        let width = ui.available_width() - 12.0;
                        let mut group = None;
                        for (i, (g, e)) in matches.iter().enumerate() {
                            if group != Some(*g) {
                                group = Some(*g);
                                group_header(ui, *g, width, i > 0);
                            }
                            if row(ui, e, &q, width, i == p.selected, up || down) {
                                run = Some(i);
                            }
                        }
                        if matches.is_empty() {
                            ui.add_space(theme::SPACE_L);
                            ui.vertical_centered(|ui| ui.weak(t!("No matching command.")));
                            ui.add_space(theme::SPACE_L);
                        }
                    });
                ui.separator();
                footer(ui);
            });
        });
    if enter && !matches.is_empty() {
        run = Some(p.selected);
    }
    if let Some(i) = run {
        let e = &matches[i].1;
        remember(&mut settings.palette_recent, &e.label);
        cmds.push((e.make)());
        p.open = false;
    }
    if esc {
        p.open = false;
    }
}

/// Width of the palette.
const WIDTH: f32 = 580.0;

/// The query field with a search icon. Returns true when the text changed.
fn search_field(ui: &mut egui::Ui, p: &mut Palette) -> bool {
    ui.horizontal(|ui| {
        ui.add_space(theme::SPACE_S);
        ui.label(
            egui::RichText::new(icons::SEARCH)
                .size(17.0)
                .color(ui.visuals().weak_text_color()),
        );
        let resp = ui.add(
            egui::TextEdit::singleline(&mut p.query)
                .hint_text(t!("Type a command, branch or file…"))
                .font(egui::FontId::proportional(15.0))
                .frame(egui::Frame::NONE)
                .margin(egui::vec2(4.0, 6.0))
                .desired_width(f32::INFINITY),
        );
        if p.just_opened {
            resp.request_focus();
            p.just_opened = false;
        }
        resp.changed()
    })
    .inner
}

fn group_header(ui: &mut egui::Ui, g: Group, width: f32, gap: bool) {
    if gap {
        ui.add_space(theme::SPACE_S);
    }
    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    ui.painter().text(
        r.left_center() + egui::vec2(8.0, 1.0),
        egui::Align2::LEFT_CENTER,
        g.label().to_uppercase(),
        theme::strong_font(10.5),
        ui.visuals().weak_text_color(),
    );
}

/// Keys of the palette, at the bottom.
fn footer(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(theme::SPACE_S);
        let weak = ui.visuals().weak_text_color();
        for (key, what) in [
            ("↑↓", t!("move")),
            ("Enter", t!("run")),
            ("Esc", t!("close")),
        ] {
            key_cap(ui, key);
            ui.label(egui::RichText::new(what).size(11.5).color(weak));
            ui.add_space(theme::SPACE_S);
        }
    });
}

/// A key name in a small rounded box.
fn key_cap(ui: &mut egui::Ui, key: &str) {
    let v = ui.visuals();
    let font = egui::FontId::proportional(11.0);
    let color = v.text_color();
    let galley = ui.painter().layout_no_wrap(key.to_owned(), font, color);
    let size = galley.size() + egui::vec2(10.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let v = ui.visuals();
    ui.painter().rect(
        rect,
        4.0,
        v.widgets.inactive.weak_bg_fill,
        v.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// The label with the characters matching `q` in `hl`, semibold.
fn label_job(
    text: &str,
    q: &str,
    color: egui::Color32,
    hl: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let hits = matched_chars(text, q);
    let mut part = String::new();
    let mut part_hit = false;
    let flush = |job: &mut egui::text::LayoutJob, part: &mut String, hit: bool| {
        if part.is_empty() {
            return;
        }
        let format = if hit {
            egui::TextFormat::simple(theme::strong_font(13.5), hl)
        } else {
            egui::TextFormat::simple(egui::FontId::proportional(13.5), color)
        };
        job.append(part, 0.0, format);
        part.clear();
    };
    for (i, ch) in text.chars().enumerate() {
        let hit = hits.contains(&i);
        if hit != part_hit {
            flush(&mut job, &mut part, part_hit);
            part_hit = hit;
        }
        part.push(ch);
    }
    flush(&mut job, &mut part, part_hit);
    job
}

/// One result row. Returns true when clicked.
fn row(ui: &mut egui::Ui, e: &Entry, q: &str, width: f32, selected: bool, moved: bool) -> bool {
    let height = theme::row_height() + 4.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &e.label));
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, visuals.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, visuals.widgets.hovered.weak_bg_fill);
    }
    let (color, weak, hl) = if selected {
        let c = visuals.selection.stroke.color;
        (c, c.gamma_multiply(0.75), c)
    } else {
        (
            visuals.text_color(),
            visuals.weak_text_color(),
            theme::accent(ui),
        )
    };
    let painter = ui.painter();
    painter.text(
        rect.left_center() + egui::vec2(18.0, 0.0),
        egui::Align2::CENTER_CENTER,
        e.icon,
        egui::FontId::proportional(15.0),
        if selected { color } else { weak },
    );
    let mut right = rect.right() - 10.0;
    if !e.shortcut.is_empty() {
        let g = painter.layout_no_wrap(e.shortcut.clone(), egui::FontId::proportional(12.0), weak);
        right -= g.size().x;
        painter.galley(
            egui::pos2(right, rect.center().y - g.size().y / 2.0),
            g,
            weak,
        );
        right -= 16.0;
    }
    let left = rect.left() + 36.0;
    let mut job = label_job(&e.label, q, color, hl);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(10.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
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
            e(Group::Files, "feature.rs"),
            e(Group::Commands, "New branch…"),
        ];
        let out = ranked(list, "feature", &[]);
        let labels: Vec<&str> = out.iter().map(|(_, e)| e.label.as_str()).collect();
        assert_eq!(labels, ["feature.rs", "Check out feature"]);
        // Without a query: the fixed group order.
        let list = vec![e(Group::Tabs, "Switch to x"), e(Group::Commands, "Refresh")];
        assert_eq!(ranked(list, "", &[])[0].1.group, Group::Commands);
        // Keywords are searched too.
        let list = vec![e(Group::Commands, "Add worktree…").keywords("create new worktree")];
        assert_eq!(ranked(list, "create worktree", &[]).len(), 1);
    }

    #[test]
    fn recent_entries_come_first() {
        let e = |g, l: &str| Entry::new(g, l, "", || Command::Refresh);
        let list = vec![
            e(Group::Commands, "Fetch all"),
            e(Group::Commands, "Pull"),
            e(Group::Branches, "Check out topic"),
        ];
        let mut recent = Vec::new();
        remember(&mut recent, "Pull");
        remember(&mut recent, "Check out topic");
        remember(&mut recent, "Pull");
        assert_eq!(recent, ["Pull", "Check out topic"]);
        // Without a query: a group of their own, newest first.
        let out = ranked(list, "", &recent);
        let shown: Vec<(Group, &str)> = out.iter().map(|(g, e)| (*g, e.label.as_str())).collect();
        assert_eq!(
            shown,
            [
                (Group::Recent, "Pull"),
                (Group::Recent, "Check out topic"),
                (Group::Commands, "Fetch all"),
            ]
        );
        // With a query they rank higher in their group.
        let list = vec![
            e(Group::Commands, "Push…"),
            e(Group::Commands, "Pull with options…"),
        ];
        let out = ranked(list, "pu", &["Pull with options…".to_owned()]);
        assert_eq!(out[0].1.label, "Pull with options…");
    }

    #[test]
    fn highlights_matched_characters() {
        assert_eq!(matched_chars("Fetch all", "all"), [6, 7, 8]);
        assert_eq!(matched_chars("Fetch all remotes", "far"), [0, 6, 10]);
        assert_eq!(matched_chars("Ação rápida", "rá"), [5, 6]);
        assert!(matched_chars("Pull", "xyz").is_empty());
        assert!(matched_chars("Pull", "").is_empty());
    }
}
