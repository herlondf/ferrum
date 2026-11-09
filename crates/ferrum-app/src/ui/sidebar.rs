//! Left sidebar: views, pinned refs, branches, remotes, tags, stashes,
//! submodules, worktrees.

use eframe::egui::{self, RichText};
use ferrum_core::{BranchInfo, BranchKind, TagInfo};

use crate::command::{Command, Commands, Op};
use crate::tab::{RepoTab, View};
use crate::ui::branch_dnd;
use crate::ui::dialogs::Dialog;
use crate::ui::forge as forge_ui;
use crate::ui::history_options;
use crate::ui::ref_dialogs;
use crate::ui::sidebar_model::{
    DragBranch, NavAction, Node, SidebarState, SortOrder, folder_paths, sort_by, tree,
};

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, cmds: &mut Commands) {
    tab.pane.sidebar_rect = Some(ui.max_rect());
    // The state is taken out so rows can change it while reading the tab.
    let mut st = std::mem::take(&mut tab.sidebar);
    if tab.head != st.last_head {
        st.last_head = tab.head.clone();
        reveal_head(&mut st, tab);
    }
    st.selection.retain(|n| {
        tab.branches
            .iter()
            .any(|b| b.kind == BranchKind::Local && &b.name == n)
    });
    filter_bar(ui, &mut st);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // Rows fill the width (no layout feedback with the resizable panel).
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                st.begin();
                views(ui, tab, &mut st, cmds);
                ui.separator();
                pinned(ui, tab, &mut st, cmds);
                local_branches(ui, tab, &mut st, cmds);
                crate::ui::stacks::section(ui, tab, &mut st, cmds);
                remotes(ui, tab, &mut st, cmds);
                tags(ui, tab, &mut st, cmds);
                stashes(ui, tab, &mut st, cmds);
                crate::ui::submodules::section(ui, tab, &mut st, cmds);
                crate::ui::worktrees::section(ui, tab, &mut st, cmds);
                st.end();
            });
        });
    branch_dnd::overlays(ui.ctx(), tab, &mut st.drop_menu, cmds);
    tab.sidebar = st;
}

/// Opens the folders of the current branch and scrolls to it.
fn reveal_head(st: &mut SidebarState, tab: &RepoTab) {
    let Some(head) = &tab.head else {
        return;
    };
    st.set_open("sec:branches", true);
    for f in folder_paths(std::iter::once(head.as_str())) {
        st.set_open(&format!("dir:local:{f}"), true);
    }
    st.reveal = true;
}

fn filter_bar(ui: &mut egui::Ui, st: &mut SidebarState) {
    ui.add_space(2.0);
    let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.menu_button("⬍", |ui| {
            ui.label(RichText::new(t!("Branches")).strong());
            sort_menu(ui, st, false);
            ui.separator();
            ui.label(RichText::new(t!("Tags")).strong());
            sort_menu(ui, st, true);
        })
        .response
        .on_hover_text(t!("Sort branches and tags"));
        if st.filtering() && ui.small_button("✖").on_hover_text(t!("Clear")).clicked() {
            st.filter.clear();
        }
        let resp = ui.add(
            egui::TextEdit::singleline(&mut st.filter)
                .hint_text(t!("🔍 Filter"))
                .desired_width(ui.available_width()),
        );
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            st.filter.clear();
        }
    });
    ui.add_space(2.0);
}

fn sort_menu(ui: &mut egui::Ui, st: &mut SidebarState, tags: bool) {
    let current = if tags {
        st.prefs.tag_sort
    } else {
        st.prefs.branch_sort
    };
    for order in SortOrder::ALL {
        if ui.radio(current == order, order.label()).clicked() {
            st.set_sort(tags, order);
            ui.close();
        }
    }
    if !tags {
        let mut first = st.prefs.folders_first;
        if ui.checkbox(&mut first, t!("Folders first")).changed() {
            st.prefs.folders_first = first;
            st.dirty = true;
        }
    }
}

// ----- Row helpers -----

/// Outlines the row under the keyboard cursor and scrolls to it.
fn mark_cursor(ui: &egui::Ui, st: &mut SidebarState, key: &str, resp: &egui::Response) {
    if st.cursor.as_deref() != Some(key) {
        return;
    }
    if st.has_focus {
        ui.painter().rect_stroke(
            resp.rect,
            2.0,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
    if st.scroll_to_cursor {
        resp.scroll_to_me(None);
        st.scroll_to_cursor = false;
    }
}

/// Highlights a row while something can be dropped on it.
fn mark_drop(ui: &egui::Ui, resp: &egui::Response) {
    branch_dnd::highlight(ui, resp.rect);
}

/// A row that is not a folder. Returns its response and a keyboard action.
pub(crate) fn leaf(
    ui: &mut egui::Ui,
    st: &mut SidebarState,
    key: &str,
    button: egui::Button<'static>,
) -> (egui::Response, Option<NavAction>) {
    let resp = ui.add(button.truncate());
    let action = st.register(key, None);
    if resp.clicked() || resp.secondary_clicked() {
        st.cursor = Some(key.to_owned());
    }
    mark_cursor(ui, st, key, &resp);
    (resp, action)
}

pub(crate) struct Header {
    pub open: bool,
    pub resp: egui::Response,
    pub add: bool,
}

/// A collapsible section, remote or folder. `add` is the tooltip of a "+" button.
pub(crate) fn header(
    ui: &mut egui::Ui,
    st: &mut SidebarState,
    key: &str,
    default_open: bool,
    label: RichText,
    count: Option<usize>,
    add: Option<String>,
) -> Header {
    // While filtering, folders open to show the matches.
    let forced = st.filtering() && key.starts_with("dir:");
    let open = forced || st.is_open(key, default_open);
    let arrow = if open { "⏷" } else { "⏵" };
    let text = RichText::new(format!("{arrow} {}", label.text()));
    let text = if key.starts_with("sec:") {
        text.strong()
            .family(egui::FontFamily::Name(crate::fonts::STRONG.into()))
    } else {
        text
    };
    let mut b = egui::Button::selectable(false, text).truncate();
    if let Some(c) = count {
        b = b.right_text(RichText::new(c.to_string()).weak());
    }
    let resp = ui.add(b);
    // The "+" sits on top of the right end of the row.
    let mut added = false;
    if let Some(tip) = &add {
        let size = egui::vec2(20.0, resp.rect.height() - 2.0);
        let rect =
            egui::Rect::from_min_size(resp.rect.right_top() + egui::vec2(-size.x - 2.0, 1.0), size);
        added = overlay_button(ui, rect, key, crate::icons::ADD, tip).clicked();
    }
    st.register(key, Some(open));
    if resp.clicked() && !forced {
        st.set_open(key, !open);
    }
    if resp.clicked() || resp.secondary_clicked() {
        st.cursor = Some(key.to_owned());
    }
    mark_cursor(ui, st, key, &resp);
    Header {
        open,
        resp,
        add: added,
    }
}

/// A small icon button drawn over a row. It does not take layout space, so it
/// cannot widen the panel.
fn overlay_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    key: &str,
    icon: &str,
    tip: &str,
) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(("add", key)), egui::Sense::click());
    // Labeled "+" like the button it replaced; the tooltip says what it adds.
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "+"));
    let v = ui.style().interact(&resp);
    ui.painter()
        .rect_filled(rect, v.corner_radius, v.weak_bg_fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(13.0),
        v.text_color(),
    );
    resp.on_hover_text(tip)
}

/// Draws the rows inside an open header.
pub(crate) fn children(
    ui: &mut egui::Ui,
    st: &mut SidebarState,
    key: &str,
    body: impl FnOnce(&mut egui::Ui, &mut SidebarState),
) {
    st.parents.push(key.to_owned());
    ui.indent(key, |ui| body(ui, st));
    st.parents.pop();
}

// ----- Views -----

fn views(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let changes = tab.status.total();
    let label = if changes > 0 {
        t!("📝 Local Changes ({})", changes)
    } else {
        t!("📝 Local Changes")
    };
    let is_changes = matches!(tab.view, View::Changes | View::Conflicts);
    let (r, a) = leaf(
        ui,
        st,
        "view:changes",
        egui::Button::selectable(is_changes, label),
    );
    if r.clicked() || a.is_some() {
        cmds.push(Command::SetView(View::Changes));
    }
    let history = tab.view == View::History;
    let (r, a) = leaf(
        ui,
        st,
        "view:history",
        egui::Button::selectable(history, t!("🕓 All Commits")),
    );
    if r.clicked() || a.is_some() {
        cmds.push(Command::SetView(View::History));
    }
    let overview = tab.view == View::Overview;
    let (r, a) = leaf(
        ui,
        st,
        "view:overview",
        egui::Button::selectable(overview, t!("📊 Overview")),
    );
    if r.clicked() || a.is_some() {
        cmds.push(Command::SetView(View::Overview));
    }
    let insights = tab.view == View::Insights;
    let label = crate::icons::text(crate::icons::INSIGHTS, t!("Insights"));
    let (r, a) = leaf(
        ui,
        st,
        "view:insights",
        egui::Button::selectable(insights, label),
    );
    if r.clicked() || a.is_some() {
        cmds.push(Command::SetView(View::Insights));
    }
    if let Some(h) = &tab.hosted {
        let label = format!("🔀 {}s", capitalize(h.kind.request_name()));
        let prs = tab.view == View::PullRequests;
        let (r, a) = leaf(ui, st, "view:prs", egui::Button::selectable(prs, label));
        if r.clicked() || a.is_some() {
            cmds.push(Command::SetView(View::PullRequests));
        }
        if h.kind.has_issues() {
            let on = tab.view == View::Issues;
            let label = t!("📋 Issues");
            let (r, a) = leaf(ui, st, "view:issues", egui::Button::selectable(on, label));
            if r.clicked() || a.is_some() {
                cmds.push(Command::SetView(View::Issues));
            }
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

// ----- Pinned -----

/// A pinned ref that still exists.
enum Pinned<'a> {
    Local(&'a BranchInfo),
    Remote(&'a BranchInfo),
    Tag(&'a TagInfo),
}

impl Pinned<'_> {
    fn name(&self) -> &str {
        match self {
            Pinned::Local(b) | Pinned::Remote(b) => &b.name,
            Pinned::Tag(t) => &t.name,
        }
    }
}

fn pinned(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let pins: Vec<(String, Pinned)> = st
        .prefs
        .pinned
        .iter()
        .filter_map(|refname| {
            let pin = if let Some(n) = refname.strip_prefix("refs/heads/") {
                Pinned::Local(find_branch(tab, BranchKind::Local, n)?)
            } else if let Some(n) = refname.strip_prefix("refs/remotes/") {
                Pinned::Remote(find_branch(tab, BranchKind::Remote, n)?)
            } else {
                let n = refname.strip_prefix("refs/tags/")?;
                Pinned::Tag(tab.tags.iter().find(|t| t.name == n)?)
            };
            st.matches(pin.name())
                .then(|| (format!("pin:{refname}"), pin))
        })
        .collect();
    if pins.is_empty() {
        return;
    }
    let label = RichText::new(t!("📌 Pinned"));
    let h = header(ui, st, "sec:pinned", true, label, None, None);
    if !h.open {
        return;
    }
    children(ui, st, "sec:pinned", |ui, st| {
        for (key, pin) in &pins {
            match pin {
                Pinned::Local(b) => branch_row(ui, tab, st, b, &b.name, key, cmds),
                Pinned::Remote(b) => remote_branch_row(ui, tab, st, b, &b.name, key, cmds),
                Pinned::Tag(t) => tag_row(ui, tab, st, t, key, cmds),
            }
        }
    });
}

fn find_branch<'a>(tab: &'a RepoTab, kind: BranchKind, name: &str) -> Option<&'a BranchInfo> {
    tab.branches
        .iter()
        .find(|b| b.kind == kind && b.name == name)
}

fn pin_button(ui: &mut egui::Ui, st: &mut SidebarState, refname: &str) {
    let label = if st.is_pinned(refname) {
        t!("Unpin")
    } else {
        t!("📌 Pin to the top")
    };
    if ui.button(label).clicked() {
        st.toggle_pin(refname);
        ui.close();
    }
}

// ----- Local branches -----

fn local_branches(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let all: Vec<&BranchInfo> = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .collect();
    let shown: Vec<(&str, i64, &BranchInfo)> = all
        .iter()
        .filter(|b| st.matches(&b.name))
        .map(|b| (b.name.as_str(), b.time, *b))
        .collect();
    let count = shown.len();
    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    let h = header(
        ui,
        st,
        "sec:branches",
        true,
        RichText::new(t!("Branches")),
        Some(count),
        Some(t!("New branch…")),
    );
    if h.add {
        cmds.push(Command::Dialog(Dialog::create_branch(&head, &head)));
    }
    // Dropping a branch here moves it out of its folder.
    if let Some(src) = h.resp.dnd_release_payload::<DragBranch>()
        && !src.remote
        && let Some((_, leaf)) = src.name.rsplit_once('/')
    {
        cmds.run(Op::RenameBranch {
            old: src.name.clone(),
            new: leaf.to_owned(),
        });
    }
    if h.resp
        .dnd_hover_payload::<DragBranch>()
        .is_some_and(|s| !s.remote && s.name.contains('/'))
    {
        mark_drop(ui, &h.resp);
    }
    let stale: Vec<String> = all
        .iter()
        .filter(|b| b.missing_upstream.is_some() && !b.is_head)
        .map(|b| b.name.clone())
        .collect();
    h.resp.context_menu(|ui| {
        if ui.button(t!("New branch…")).clicked() {
            cmds.push(Command::Dialog(Dialog::create_branch(&head, &head)));
            ui.close();
        }
        let label = t!("Select stale local branches ({})", stale.len());
        if ui
            .add_enabled(!stale.is_empty(), egui::Button::new(label))
            .on_hover_text(t!("Branches whose upstream branch was deleted"))
            .clicked()
        {
            st.selection = stale.iter().cloned().collect();
            st.set_open("sec:branches", true);
            ui.close();
        }
        ui.separator();
        ui.menu_button(t!("Sort branches"), |ui| sort_menu(ui, st, false));
    });
    if !h.open {
        return;
    }
    children(ui, st, "sec:branches", |ui, st| {
        if all.is_empty() {
            ui.weak(t!("No branches yet."));
        } else if shown.is_empty() {
            ui.weak(t!("No matches."));
        }
        selection_bar(ui, tab, st, cmds);
        let nodes = tree(shown, st.prefs.branch_sort, st.prefs.folders_first);
        local_nodes(ui, tab, st, &nodes, cmds);
    });
}

/// Actions on the branches picked with Ctrl+click or "Select stale".
fn selection_bar(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    if st.selection.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(t!("{} selected", st.selection.len()));
        let names: Vec<String> = st
            .selection
            .iter()
            .filter(|n| tab.head.as_ref() != Some(*n))
            .cloned()
            .collect();
        if ui
            .add_enabled(
                !names.is_empty() && !tab.busy(),
                egui::Button::new(t!("Delete…")).small(),
            )
            .clicked()
        {
            cmds.push(Command::Dialog(ref_dialogs::delete_branches(tab, &names)));
        }
        let all: Vec<String> = st.selection.iter().cloned().collect();
        crate::ui::sync_ui::selection_buttons(ui, tab, &all, cmds);
        if let [a, b] = all.as_slice() {
            let (a, b) = (a.clone(), b.clone());
            if ui
                .small_button(t!("Compare"))
                .on_hover_text(t!("Show the changes between {} and {}", a, b))
                .clicked()
            {
                cmds.push(history_options::cmd(move |t, _| {
                    crate::ui::compare::open_refs(t, &a, &b)
                }));
            }
        }
        if ui.small_button(t!("Clear")).clicked() {
            st.selection.clear();
        }
    });
}

fn local_nodes(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    nodes: &[Node<'_, BranchInfo>],
    cmds: &mut Commands,
) {
    for node in nodes {
        match node {
            Node::Folder {
                name,
                path,
                children: inner,
                count,
                ..
            } => {
                let key = format!("dir:local:{path}");
                let label = RichText::new(format!("📁 {name}"));
                let h = header(ui, st, &key, true, label, Some(*count), None);
                // Dropping a branch on a folder moves it there.
                let moved = |s: &DragBranch| {
                    let leaf = s.name.rsplit('/').next().unwrap_or(&s.name);
                    (!s.remote && s.name != format!("{path}/{leaf}"))
                        .then(|| format!("{path}/{leaf}"))
                };
                if let Some(src) = h.resp.dnd_release_payload::<DragBranch>()
                    && let Some(new) = moved(&src)
                {
                    cmds.run(Op::RenameBranch {
                        old: src.name.clone(),
                        new,
                    });
                }
                if h.resp
                    .dnd_hover_payload::<DragBranch>()
                    .is_some_and(|s| moved(&s).is_some())
                {
                    mark_drop(ui, &h.resp);
                }
                h.resp
                    .context_menu(|ui| local_folder_menu(ui, tab, path, cmds));
                if h.open {
                    children(ui, st, &key, |ui, st| {
                        local_nodes(ui, tab, st, inner, cmds);
                    });
                }
            }
            Node::Leaf { name, item, .. } => {
                let key = format!("ref:refs/heads/{}", item.name);
                branch_row(ui, tab, st, item, name, &key, cmds);
            }
        }
    }
}

fn local_folder_menu(ui: &mut egui::Ui, tab: &RepoTab, path: &str, cmds: &mut Commands) {
    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    if ui.button(t!("New branch in {}…", path)).clicked() {
        let d = ref_dialogs::new_branch(&head, &head, &format!("{path}/"));
        cmds.push(Command::Dialog(d));
        ui.close();
    }
    history_options::ref_menu(ui, tab, &format!("refs/heads/{path}/"), cmds);
}

/// Icon and counters on the right of a local branch.
fn upstream_badge(ui: &egui::Ui, b: &BranchInfo) -> RichText {
    let mut text = String::new();
    if let Some((ahead, behind)) = b.ahead_behind {
        if ahead > 0 {
            text.push_str(&format!("⬆{ahead} "));
        }
        if behind > 0 {
            text.push_str(&format!("⬇{behind} "));
        }
    }
    if b.missing_upstream.is_some() {
        text.push('⚠');
        return RichText::new(text).color(ui.visuals().warn_fg_color);
    }
    // Tracked branches show a cloud, local-only ones a dashed circle.
    text.push_str(if b.upstream.is_some() {
        crate::icons::REMOTE
    } else {
        crate::icons::CIRCLE_DASHED
    });
    RichText::new(text).weak()
}

fn branch_tooltip(b: &BranchInfo) -> String {
    let tracking = match (&b.upstream, b.ahead_behind, &b.missing_upstream) {
        (Some(u), Some((a, bh)), _) => t!("tracks {} ({} ahead, {} behind)", u, a, bh),
        (Some(u), None, _) => t!("tracks {}", u),
        (None, _, Some(m)) => t!("⚠ Its upstream {} no longer exists", m),
        _ => t!("Local only: not published to a remote"),
    };
    format!(
        "{}\n{}\n{}",
        t!("Local branch: {}", b.name),
        tracking,
        t!("Double-click to check out")
    )
}

fn branch_row(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    b: &BranchInfo,
    label: &str,
    key: &str,
    cmds: &mut Commands,
) {
    let label = &crate::ui::worktrees::branch_label(tab, &b.name, label);
    let rich = if b.is_head {
        RichText::new(format!("✔ {label}")).strong()
    } else {
        RichText::new(label)
    };
    let rich = if history_options::is_hidden(tab, &format!("refs/heads/{}", b.name)) {
        rich.weak()
    } else {
        rich
    };
    let selected = st.selection.contains(&b.name)
        || (b.target.is_some() && tab.history.selected == b.target && tab.view == View::History);
    let button = egui::Button::selectable(selected, rich).right_text(upstream_badge(ui, b));
    let (resp, action) = leaf(ui, st, key, button);
    let resp = resp.interact(egui::Sense::drag());
    if b.is_head && st.reveal {
        resp.scroll_to_me(Some(egui::Align::Center));
        st.reveal = false;
    }
    drag_and_drop(ui, st, &resp, b, false);
    let resp = resp.on_hover_text(branch_tooltip(b));
    let ctrl = ui.input(|i| i.modifiers.command);
    if resp.clicked() && ctrl {
        if !st.selection.remove(&b.name) {
            st.selection.insert(b.name.clone());
        }
    } else if resp.clicked() || action == Some(NavAction::Select) {
        st.selection.clear();
        if let Some(t) = b.target {
            cmds.push(Command::SelectCommit(t));
        }
    }
    let activate = resp.double_clicked() || action == Some(NavAction::Activate);
    if activate
        && !b.is_head
        && !crate::ui::worktrees::activate_branch(tab, &b.name, cmds)
        && !tab.busy()
    {
        cmds.run(Op::Checkout(b.name.clone()));
    }
    resp.context_menu(|ui| local_branch_menu(ui, tab, st, b, cmds));
}

/// Makes a branch row draggable, and a drop target for other branches.
fn drag_and_drop(
    ui: &egui::Ui,
    st: &mut SidebarState,
    resp: &egui::Response,
    b: &BranchInfo,
    remote: bool,
) {
    let branch = DragBranch {
        name: b.name.clone(),
        remote,
    };
    branch_dnd::source(resp, branch.clone());
    if let Some(menu) = branch_dnd::target(ui, resp, branch_dnd::Target::Branch(branch)) {
        st.drop_menu = Some(menu);
    }
}

fn local_branch_menu(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    b: &BranchInfo,
    cmds: &mut Commands,
) {
    ui.set_min_width(220.0);
    let name = b.name.clone();
    let current = tab.head.clone();
    ui.add_enabled_ui(!tab.busy(), |ui| {
        if !b.is_head && ui.button(t!("Check out {}", name)).clicked() {
            cmds.run(Op::Checkout(name.clone()));
            ui.close();
        }
        crate::ui::worktrees::branch_menu(ui, tab, &name, false, cmds);
        if b.is_head && ui.button(t!("Pull")).clicked() {
            cmds.push(Command::Dialog(Dialog::pull(tab)));
            ui.close();
        }
        if ui.button(t!("Push {}…", name)).clicked() {
            cmds.push(Command::Dialog(Dialog::push(tab, &name)));
            ui.close();
        }
        if let Some(cur) = current.as_ref().filter(|c| **c != name) {
            ui.separator();
            if ui.button(t!("Merge {} into {}…", name, cur)).clicked() {
                cmds.push(Command::Dialog(Dialog::merge(&name, &name)));
                ui.close();
            }
            if ui.button(t!("Rebase {} onto {}…", cur, name)).clicked() {
                cmds.push(Command::Dialog(crate::ui::flow_dialogs::rebase(tab, &name)));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(t!("New branch from {}…", name)).clicked() {
            cmds.push(Command::Dialog(Dialog::create_branch(&name, &name)));
            ui.close();
        }
        if ui.button(t!("Rename…  (F2)")).clicked() {
            cmds.push(Command::Dialog(ref_dialogs::rename_branch(tab, &name)));
            ui.close();
        }
        move_to_folder_menu(ui, tab, &name, cmds);
        if !b.is_head && ui.button(t!("Delete…")).clicked() {
            let names = [name.clone()];
            cmds.push(Command::Dialog(ref_dialogs::delete_branches(tab, &names)));
            ui.close();
        }
        crate::ui::sync_ui::branch_menu(ui, tab, b, cmds);
        crate::ui::gitflow::branch_menu(ui, tab, &name, cmds);
        crate::ui::stacks::branch_menu(ui, tab, &name, cmds);
        if let Some(m) = &b.missing_upstream {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("⚠ Its upstream {} no longer exists", m),
            );
        }
        if (b.upstream.is_some() || b.missing_upstream.is_some())
            && ui.button(t!("Stop tracking the upstream")).clicked()
        {
            cmds.run(Op::UnsetUpstream(name.clone()));
            ui.close();
        }
    });
    if let Some(h) = &tab.hosted {
        ui.separator();
        if ui.button(t!("Create {}…", h.kind.request_name())).clicked() {
            cmds.push(Command::Dialog(Dialog::Forge(forge_ui::new_pull_request(
                tab, &name,
            ))));
            ui.close();
        }
    }
    history_options::ref_menu(ui, tab, &format!("refs/heads/{name}"), cmds);
    crate::ui::custom_commands::branch_menu(ui, tab, b, cmds);
    crate::ui::ai_review::menu_item(ui, tab, &name, cmds);
    ui.separator();
    pin_button(ui, st, &format!("refs/heads/{name}"));
    if ui.button(t!("Copy name")).clicked() {
        cmds.push(Command::Copy(name));
        ui.close();
    }
}

/// Renames a branch into another existing folder (or out of its folder).
fn move_to_folder_menu(ui: &mut egui::Ui, tab: &RepoTab, name: &str, cmds: &mut Commands) {
    let (folder, leaf) = name.rsplit_once('/').unwrap_or(("", name));
    let folders: Vec<String> = folder_paths(
        tab.branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local)
            .map(|b| b.name.as_str()),
    )
    .into_iter()
    .filter(|f| f != folder)
    .collect();
    if folders.is_empty() && folder.is_empty() {
        return;
    }
    ui.menu_button(t!("Move to folder"), |ui| {
        if !folder.is_empty() && ui.button(t!("(top level)")).clicked() {
            cmds.run(Op::RenameBranch {
                old: name.to_owned(),
                new: leaf.to_owned(),
            });
            ui.close();
        }
        for f in folders {
            if ui.button(format!("📁 {f}")).clicked() {
                cmds.run(Op::RenameBranch {
                    old: name.to_owned(),
                    new: format!("{f}/{leaf}"),
                });
                ui.close();
            }
        }
    });
}

// ----- Remotes -----

fn remotes(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let h = header(
        ui,
        st,
        "sec:remotes",
        true,
        RichText::new(t!("Remotes")),
        None,
        Some(t!("Add remote…")),
    );
    if h.add {
        cmds.push(Command::Dialog(Dialog::AddRemote {
            name: String::new(),
            url: String::new(),
        }));
    }
    h.resp.context_menu(|ui| {
        if ui.button(t!("Add remote…")).clicked() {
            cmds.push(Command::Dialog(Dialog::AddRemote {
                name: String::new(),
                url: String::new(),
            }));
            ui.close();
        }
        if ui
            .add_enabled(!tab.busy(), egui::Button::new(t!("Fetch all")))
            .clicked()
        {
            cmds.run(Op::Fetch { remote: None });
            ui.close();
        }
    });
    if !h.open {
        return;
    }
    children(ui, st, "sec:remotes", |ui, st| {
        if tab.remotes.is_empty() {
            ui.weak(t!("No remotes."));
        }
        for remote in &tab.remotes {
            let prefix = format!("{}/", remote.name);
            let list: Vec<(&str, i64, &BranchInfo)> = tab
                .branches
                .iter()
                .filter(|b| b.kind == BranchKind::Remote && b.name.starts_with(&prefix))
                .filter(|b| st.matches(&b.name))
                .map(|b| (&b.name[prefix.len()..], b.time, b))
                .collect();
            if st.filtering() && list.is_empty() {
                continue;
            }
            let key = format!("dir:remote:{}", remote.name);
            let label = RichText::new(format!("☁ {}", remote.name));
            let h = header(ui, st, &key, true, label, Some(list.len()), None);
            let target = branch_dnd::Target::Remote(remote.name.clone());
            if let Some(menu) = branch_dnd::target(ui, &h.resp, target) {
                st.drop_menu = Some(menu);
            }
            h.resp.on_hover_text(&remote.url).context_menu(|ui| {
                ui.add_enabled_ui(!tab.busy(), |ui| {
                    if ui.button(t!("Fetch {}", remote.name)).clicked() {
                        cmds.run(Op::Fetch { remote: Some(remote.name.clone()) });
                        ui.close();
                    }
                    if ui.button(t!("Remove remote…")).clicked() {
                        cmds.push(Command::Dialog(Dialog::confirm(
                            &t!("Remove remote"),
                            &t!("Remove the remote {}? Its remote-tracking branches will be deleted locally.", remote.name),
                            Op::RemoveRemote(remote.name.clone()),
                            true,
                        )));
                        ui.close();
                    }
                });
                if ui.button(t!("Copy URL")).clicked() {
                    cmds.push(Command::Copy(remote.url.clone()));
                    ui.close();
                }
                crate::ui::sync_ui::remote_menu(ui, tab, remote, cmds);
                history_options::ref_menu(ui, tab, &format!("refs/remotes/{prefix}"), cmds);
            });
            if h.open {
                let nodes = tree(list, st.prefs.branch_sort, st.prefs.folders_first);
                children(ui, st, &key, |ui, st| {
                    remote_nodes(ui, tab, st, &remote.name, &nodes, cmds);
                });
            }
        }
    });
}

fn remote_nodes(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    remote: &str,
    nodes: &[Node<'_, BranchInfo>],
    cmds: &mut Commands,
) {
    for node in nodes {
        match node {
            Node::Folder {
                name,
                path,
                children: inner,
                count,
                ..
            } => {
                let key = format!("dir:remote:{remote}/{path}");
                let label = RichText::new(format!("📁 {name}"));
                let h = header(ui, st, &key, false, label, Some(*count), None);
                h.resp.context_menu(|ui| {
                    history_options::ref_menu(
                        ui,
                        tab,
                        &format!("refs/remotes/{remote}/{path}/"),
                        cmds,
                    )
                });
                if h.open {
                    children(ui, st, &key, |ui, st| {
                        remote_nodes(ui, tab, st, remote, inner, cmds);
                    });
                }
            }
            Node::Leaf { name, item, .. } => {
                let key = format!("ref:refs/remotes/{}", item.name);
                remote_branch_row(ui, tab, st, item, name, &key, cmds);
            }
        }
    }
}

fn remote_branch_row(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    b: &BranchInfo,
    label: &str,
    key: &str,
    cmds: &mut Commands,
) {
    let selected =
        b.target.is_some() && tab.history.selected == b.target && tab.view == View::History;
    let button = egui::Button::selectable(selected, label.to_owned());
    let (resp, action) = leaf(ui, st, key, button);
    let resp = resp.interact(egui::Sense::drag());
    drag_and_drop(ui, st, &resp, b, true);
    let local = tab
        .branches
        .iter()
        .find(|l| l.kind == BranchKind::Local && l.upstream.as_deref() == Some(&b.name));
    let mut tip = t!("Remote branch: {}", b.name);
    if let Some(l) = local {
        tip = format!("{tip}\n{}", t!("tracked by {}", l.name));
    }
    let resp = resp.on_hover_text(format!("{tip}\n{}", t!("Double-click to check out")));
    if (resp.clicked() || action == Some(NavAction::Select))
        && let Some(t) = b.target
    {
        cmds.push(Command::SelectCommit(t));
    }
    // Asks how to update a local branch that is behind or has diverged.
    let checkout =
        |cmds: &mut Commands| cmds.push(crate::ui::sync_dialogs::checkout_remote(tab, b));
    if (resp.double_clicked() || action == Some(NavAction::Activate)) && !tab.busy() {
        checkout(cmds);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(220.0);
        ui.add_enabled_ui(!tab.busy(), |ui| {
            let label = match local {
                Some(l) => t!("Check out {}", l.name),
                None => t!("Check out as local branch…"),
            };
            if ui.button(label).clicked() {
                checkout(cmds);
                ui.close();
            }
            crate::ui::worktrees::branch_menu(ui, tab, &b.name, true, cmds);
            if let Some(cur) = &tab.head {
                if ui.button(t!("Merge {} into {}…", b.name, cur)).clicked() {
                    cmds.push(Command::Dialog(Dialog::merge(&b.name, &b.name)));
                    ui.close();
                }
                if ui.button(t!("Rebase {} onto {}…", cur, b.name)).clicked() {
                    cmds.push(Command::Dialog(crate::ui::flow_dialogs::rebase(tab, &b.name)));
                    ui.close();
                }
            }
            if ui.button(t!("New branch from here…")).clicked() {
                cmds.push(Command::Dialog(Dialog::create_branch(&b.name, &b.name)));
                ui.close();
            }
            if let Some((remote, name)) = b.split_remote()
                && ui.button(t!("Delete from {}…", remote)).clicked()
            {
                #[rustfmt::skip]
                let text = t!("Delete the branch {} from {}? Other people using it will lose it.", name, remote);
                cmds.push(Command::Dialog(Dialog::confirm(
                    &t!("Delete remote branch"),
                    &text,
                    Op::DeleteRemoteRef {
                        remote: remote.to_owned(),
                        name: name.to_owned(),
                    },
                    true,
                )));
                ui.close();
            }
        });
        history_options::ref_menu(ui, tab, &format!("refs/remotes/{}", b.name), cmds);
        crate::ui::custom_commands::branch_menu(ui, tab, b, cmds);
        crate::ui::ai_review::menu_item(ui, tab, &b.name, cmds);
        ui.separator();
        pin_button(ui, st, &format!("refs/remotes/{}", b.name));
        if ui.button(t!("Copy name")).clicked() {
            cmds.push(Command::Copy(b.name.clone()));
            ui.close();
        }
    });
}

// ----- Tags -----

fn tags(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let mut list: Vec<&TagInfo> = tab.tags.iter().filter(|t| st.matches(&t.name)).collect();
    let time = |t: &&TagInfo| tab.index.get(&t.target).map_or(0, |&i| tab.commits[i].time);
    sort_by(&mut list, st.prefs.tag_sort, |t| &t.name, time);
    let title = t!("Tags ({})", list.len());
    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
    let h = header(
        ui,
        st,
        "sec:tags",
        false,
        RichText::new(title),
        None,
        Some(t!("New tag…")),
    );
    if h.add {
        cmds.push(Command::Dialog(Dialog::create_tag("HEAD", &head)));
    }
    h.resp.context_menu(|ui| {
        if ui.button(t!("New tag…")).clicked() {
            cmds.push(Command::Dialog(Dialog::create_tag("HEAD", &head)));
            ui.close();
        }
        crate::ui::sync_ui::push_tags_button(ui, tab, &[], cmds);
        ui.menu_button(t!("Sort tags"), |ui| sort_menu(ui, st, true));
    });
    if !h.open {
        return;
    }
    children(ui, st, "sec:tags", |ui, st| {
        for t in list {
            tag_row(ui, tab, st, t, &format!("ref:refs/tags/{}", t.name), cmds);
        }
    });
}

fn tag_tooltip(t: &TagInfo) -> String {
    let mut tip = t!("Tag: {}", t.name);
    if let Some(m) = t.message.as_deref().filter(|m| !m.is_empty()) {
        tip = format!("{tip}\n\n{m}\n");
    }
    format!("{tip}\n{}", t!("Double-click to check out (detached)"))
}

fn tag_row(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    st: &mut SidebarState,
    t: &TagInfo,
    key: &str,
    cmds: &mut Commands,
) {
    let selected = tab.history.selected == Some(t.target) && tab.view == View::History;
    let button = egui::Button::selectable(selected, format!("🏷 {}", t.name));
    let (resp, action) = leaf(ui, st, key, button);
    let resp = resp.on_hover_text(tag_tooltip(t));
    if resp.clicked() || action == Some(NavAction::Select) {
        cmds.push(Command::SelectCommit(t.target));
    }
    if (resp.double_clicked() || action == Some(NavAction::Activate)) && !tab.busy() {
        cmds.run(Op::CheckoutDetached(t.name.clone()));
    }
    resp.context_menu(|ui| {
        ui.add_enabled_ui(!tab.busy(), |ui| {
            if ui.button(t!("Check out (detached)")).clicked() {
                cmds.run(Op::CheckoutDetached(t.name.clone()));
                ui.close();
            }
            for r in &tab.remotes {
                if ui.button(t!("Push to {}", r.name)).clicked() {
                    cmds.run(Op::PushTag {
                        remote: r.name.clone(),
                        tag: t.name.clone(),
                    });
                    ui.close();
                }
            }
            if ui.button(t!("Delete…")).clicked() {
                cmds.push(Command::Dialog(Dialog::DeleteTag {
                    name: t.name.clone(),
                    remote: None,
                }));
                ui.close();
            }
            for r in &tab.remotes {
                if ui.button(t!("Delete from {}…", r.name)).clicked() {
                    #[rustfmt::skip]
                    let text = t!("Delete the tag {} from {}? The local tag is kept.", t.name, r.name);
                    cmds.push(Command::Dialog(Dialog::confirm(
                        &t!("Delete remote tag"),
                        &text,
                        Op::DeleteRemoteRef {
                            remote: r.name.clone(),
                            name: format!("refs/tags/{}", t.name),
                        },
                        true,
                    )));
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button(t!("Details…")).clicked() {
            cmds.push(Command::Dialog(ref_dialogs::tag_details(tab, &t.name)));
            ui.close();
        }
        pin_button(ui, st, &format!("refs/tags/{}", t.name));
        if ui.button(t!("Copy name")).clicked() {
            cmds.push(Command::Copy(t.name.clone()));
            ui.close();
        }
    });
}

// ----- Stashes (submodules and worktrees: see submodules.rs, worktrees.rs) -----

fn stashes(ui: &mut egui::Ui, tab: &RepoTab, st: &mut SidebarState, cmds: &mut Commands) {
    let title = t!("Stashes ({})", tab.stashes.len());
    let h = header(
        ui,
        st,
        "sec:stashes",
        true,
        RichText::new(title),
        None,
        Some(t!("Stash changes…")),
    );
    if h.add {
        cmds.push(Command::Dialog(Dialog::stash()));
    }
    h.resp
        .context_menu(|ui| ref_dialogs::stash_actions(ui, tab, cmds));
    if !h.open {
        return;
    }
    children(ui, st, "sec:stashes", |ui, st| {
        if tab.stashes.is_empty() {
            ui.weak(t!("No stashes."));
        }
        for s in &tab.stashes {
            let selected = tab
                .commit_view
                .as_ref()
                .is_some_and(|v| v.is_stash && v.details.info.id == s.id);
            let key = format!("stash:{}", s.id);
            let button = egui::Button::selectable(selected, format!("📦 {}", s.message));
            let (resp, action) = leaf(ui, st, &key, button);
            if resp.clicked() || action.is_some() {
                cmds.push(Command::ShowCommit(s.id));
            }
            resp.on_hover_text(s.refname())
                .context_menu(|ui| ref_dialogs::stash_menu(ui, tab, s, cmds));
        }
    });
}

/// `FERRUM_START` steps: `open:KEY` (e.g. `open:sec:tags`), `pin:REF`,
/// `filter:TEXT`, `focus` (keyboard focus with the ring shown).
pub fn dev_step(tab: &mut RepoTab, arg: &str) {
    let (cmd, rest) = arg.split_once(':').unwrap_or((arg, ""));
    let st = &mut tab.sidebar;
    match cmd {
        "open" => st.set_open(rest, true),
        "pin" if !st.is_pinned(rest) => st.toggle_pin(rest),
        "filter" => st.filter = rest.to_owned(),
        "focus" => {
            tab.pane.pane = crate::ui::focus::Pane::Sidebar;
            tab.pane.ring = true;
        }
        // `drop:SRC>DST`: the menu shown after dropping branch SRC on DST.
        "drop" => {
            let menu = branch_dnd::dev_menu(tab, rest);
            tab.sidebar.drop_menu = menu;
        }
        _ => eprintln!("FERRUM_START: unknown sidebar step {arg}"),
    }
}
