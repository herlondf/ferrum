//! Details of the selected commit, in three tabs: Commit (author, committer,
//! parents, refs that contain it, message, files and diff), Changes (files
//! and diff) and File Tree (the repository at that commit). Also the large
//! popup (Space), commit windows and the keyboard shortcuts of the pane.

use eframe::egui::{self, Event, Key, KeyboardShortcut, Modifiers};
use ferrum_core::browse;
use ferrum_core::cli::Git;
use ferrum_core::diff::DiffConfig;
use ferrum_core::{CommitDetails, DiffSource, Oid, Repo, StashInfo, short};

use crate::command::{Command, Commands, Op};
use crate::tab::{CommitView, RepoTab, View};
use crate::ui::details_model::{self, CommitWindow, DetailsSettings, DetailsTab, PaneState};
use crate::ui::file_actions::{self, DiffSide};
use crate::ui::focus::Pane;
use crate::ui::{compare, diff_extra, diff_view, file_tree};
use crate::util::format_date;

/// What a details pane reads from the tab.
#[derive(Clone, Copy)]
struct Env<'a> {
    repo: &'a Repo,
    git: &'a Git,
    cfg: DiffConfig,
    /// Clone URL of the main remote, for "Open in browser".
    remote_url: Option<&'a str>,
    stashes: &'a [StashInfo],
    busy: bool,
    /// Tells the widgets of the main pane, the popup and windows apart.
    salt: &'a str,
    /// The main pane (not the popup or a window).
    main: bool,
}

fn remote_url(remotes: &[ferrum_core::RemoteInfo]) -> Option<&str> {
    remotes
        .iter()
        .find(|r| r.name == "origin")
        .or_else(|| remotes.first())
        .map(|r| r.url.as_str())
}

/// Shortcuts of the history view: Ctrl+Alt+D shows or hides the details,
/// Space opens the large popup (when the commit list has the keyboard),
/// Ctrl+C / Ctrl+Shift+C copy the commit id / info, Ctrl+D the external diff.
/// Call before the panes are drawn.
pub fn shortcuts(
    ctx: &egui::Context,
    tab: &mut RepoTab,
    prefs: &mut DetailsSettings,
    cmds: &mut Commands,
) {
    if tab.details.reveal {
        tab.details.reveal = false;
        prefs.collapsed = false;
    }
    if let Some(t) = tab.details.set_tab.take() {
        prefs.tab = t;
    }
    if tab.view != View::History {
        return;
    }
    let toggle = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::ALT, Key::D);
    if ctx.input_mut(|i| i.consume_shortcut(&toggle)) {
        prefs.collapsed = !prefs.collapsed;
    }
    if ctx.text_edit_focused() || egui::Popup::is_any_open(ctx) {
        return;
    }
    let space = |ctx: &egui::Context| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Space));
    if tab.details.popup {
        if space(ctx) {
            tab.details.popup = false;
        }
    } else if tab.pane.pane == Pane::Commits && tab.commit_view.is_some() && space(ctx) {
        tab.details.popup = true;
    }
    if file_actions::external_diff_pressed(ctx) {
        let cmd = if tab.details.compare.is_some() {
            compare::external_diff(tab)
        } else {
            tab.commit_view
                .as_ref()
                .and_then(|v| v.diff.as_ref())
                .map(|f| file_actions::external_diff(&tab.repo, f, DiffSide::Blob, DiffSide::Blob))
        };
        if let Some(c) = cmd {
            cmds.push(c);
        }
    }
    if tab.pane.pane != Pane::Commits || tab.details.popup {
        return;
    }
    let Some(view) = &tab.commit_view else { return };
    let info = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
    if ctx.input_mut(|i| i.consume_shortcut(&info)) {
        cmds.push(Command::Copy(commit_text(&view.details)));
        return;
    }
    // The system turns Ctrl+C into a copy event.
    let copy = ctx.input_mut(|i| {
        let copy = i.events.iter().any(|e| matches!(e, Event::Copy));
        i.events.retain(|e| !matches!(e, Event::Copy));
        copy || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::C))
    });
    if copy {
        cmds.push(Command::Copy(view.details.info.id.to_string()));
    }
}

/// A `details:` step of `FERRUM_START`: `commit`, `changes`, `tree`,
/// `popup`, `window`, `open=DIR` / `select=PATH` (File Tree), `compare=N,M`
/// (commits by position), `workdir=N` (commit N with the working copy).
pub fn dev_step(arg: &str) -> Command {
    let arg = arg.to_owned();
    crate::ui::history_options::cmd(move |t, _| {
        let (key, val) = arg.split_once('=').unwrap_or((arg.as_str(), ""));
        let nth = |t: &RepoTab, s: &str| {
            t.commits
                .get(s.trim().parse::<usize>().unwrap_or(0))
                .map(|c| c.id)
        };
        match key {
            "commit" => t.details.set_tab = Some(DetailsTab::Commit),
            "changes" => t.details.set_tab = Some(DetailsTab::Changes),
            "tree" => t.details.set_tab = Some(DetailsTab::FileTree),
            "popup" => t.details.popup = true,
            "window" => {
                if let Some(v) = &t.commit_view {
                    let id = v.details.info.id;
                    open_window(t, id, &DetailsSettings::default());
                }
            }
            "open" => {
                let mut dir = String::new();
                for part in val.split('/') {
                    dir = if dir.is_empty() {
                        part.to_owned()
                    } else {
                        format!("{dir}/{part}")
                    };
                    t.details.pane.tree.open.insert(dir.clone());
                }
            }
            "select" => {
                t.details.pane.tree.selected = Some(ferrum_core::browse::TreeEntry {
                    name: val.rsplit('/').next().unwrap_or(val).to_owned(),
                    path: val.to_owned(),
                    kind: ferrum_core::browse::EntryKind::File,
                    id: Oid::ZERO_SHA1,
                });
            }
            "compare" => {
                let (a, b) = val.split_once(',').unwrap_or((val, "1"));
                if let (Some(a), Some(b)) = (nth(t, a), nth(t, b)) {
                    let (from, to) = (compare::commit_side(t, b), compare::commit_side(t, a));
                    compare::open(t, from, to);
                }
            }
            "workdir" => {
                if let Some(id) = nth(t, val) {
                    let from = compare::commit_side(t, id);
                    compare::open(t, from, compare::workdir_side());
                }
            }
            _ => eprintln!("FERRUM_START: unknown details step {arg}"),
        }
    })
}

/// The commit as text: id, author, committer and message.
pub fn commit_text(d: &CommitDetails) -> String {
    let c = &d.info;
    format!(
        "Commit: {}\nAuthor: {} <{}>\nDate: {}\nCommitter: {} <{}>\nCommit date: {}\n\n{}\n",
        c.id,
        c.author,
        c.email,
        crate::util::format_time(c.time, c.offset_minutes),
        d.committer,
        d.committer_email,
        crate::util::format_time(d.commit_time, d.commit_offset_minutes),
        d.message.trim_end()
    )
}

/// Opens a commit in its own window.
pub fn open_window(tab: &mut RepoTab, id: Oid, prefs: &DetailsSettings) {
    let is_stash = tab.stashes.iter().any(|s| s.id == id);
    match details_model::load_view(&tab.repo, id, is_stash, &tab.diff_options.config) {
        Ok(view) => {
            let d = &mut tab.details;
            d.next_window += 1;
            d.windows.push(CommitWindow {
                key: d.next_window,
                view,
                pane: PaneState::default(),
                prefs: prefs.clone(),
                open: true,
            });
        }
        Err(e) => tab.error(e),
    }
}

pub fn show(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    prefs: &mut DetailsSettings,
    cmds: &mut Commands,
) {
    if tab.details.compare.is_some() {
        compare::show(ui, tab, prefs, cmds);
        return;
    }
    if tab.commit_view.is_none() {
        ui.centered_and_justified(|ui| ui.weak(t!("Select a commit to see its details.")));
        return;
    }
    let mut open = None;
    {
        let RepoTab {
            repo,
            git,
            commit_view,
            details,
            stashes,
            remotes,
            diff_options,
            job,
            ..
        } = tab;
        let env = Env {
            repo,
            git,
            cfg: diff_options.config,
            remote_url: remote_url(remotes),
            stashes,
            busy: job.is_some(),
            salt: "",
            main: true,
        };
        let view = commit_view.as_mut().expect("checked above");
        if pane(ui, &env, view, &mut details.pane, prefs, cmds) {
            open = Some(view.details.info.id);
        }
    }
    if let Some(id) = open {
        open_window(tab, id, prefs);
    }
}

/// The large popup and the commit windows. Call after the panes are drawn.
pub fn windows(
    ctx: &egui::Context,
    tab: &mut RepoTab,
    prefs: &mut DetailsSettings,
    cmds: &mut Commands,
) {
    let RepoTab {
        repo,
        git,
        commit_view,
        details,
        stashes,
        remotes,
        diff_options,
        job,
        view: tab_view,
        ..
    } = tab;
    let base = Env {
        repo,
        git,
        cfg: diff_options.config,
        remote_url: remote_url(remotes),
        stashes,
        busy: job.is_some(),
        salt: "-popup",
        main: false,
    };
    if details.popup {
        match commit_view.as_mut().filter(|_| *tab_view == View::History) {
            Some(view) => {
                let screen = ctx.content_rect();
                let resp =
                    egui::Modal::new(egui::Id::unique("commit-details-popup")).show(ctx, |ui| {
                        ui.set_width(screen.width() * 0.92);
                        ui.set_height(screen.height() * 0.88);
                        let close = title_bar(ui, view, true);
                        ui.separator();
                        pane(ui, &base, view, &mut details.pane, prefs, cmds);
                        close
                    });
                if resp.inner || resp.should_close() {
                    details.popup = false;
                }
            }
            None => details.popup = false,
        }
    }
    for w in &mut details.windows {
        let c = &w.view.details.info;
        let title = format!("{} — {} — Ferrum", short(c.id), c.summary);
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1100.0, 760.0]);
        let vid = egui::ViewportId::from_hash_of(("commit-window", w.key));
        let salt = format!("-w{}", w.key);
        ctx.show_viewport_immediate(vid, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                // Inside the main window: give the panes a fixed size.
                ui.set_width(900.0);
                ui.set_height(600.0);
            }
            let mut close = false;
            egui::Panel::top(egui::Id::unique(("commit-window-top", w.key))).show(ui, |ui| {
                close = title_bar(ui, &w.view, false);
            });
            egui::CentralPanel::default_margins().show(ui, |ui| {
                let env = Env {
                    salt: &salt,
                    ..base
                };
                pane(ui, &env, &mut w.view, &mut w.pane, &mut w.prefs, cmds);
            });
            if close || ui.input(|i| i.viewport().close_requested()) {
                w.open = false;
            }
        });
    }
    details.windows.retain(|w| w.open);
}

/// Short id and summary, and a Close button. Returns true when it is clicked.
fn title_bar(ui: &mut egui::Ui, view: &CommitView, popup: bool) -> bool {
    let c = &view.details.info;
    ui.horizontal(|ui| {
        ui.monospace(short(c.id));
        ui.strong(&c.summary);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = ui.button(t!("Close")).clicked();
            if popup {
                ui.weak(t!("Space or Esc closes this window"));
            }
            close
        })
        .inner
    })
    .inner
}

/// A details pane. Returns true when "Open in new window" was clicked.
fn pane(
    ui: &mut egui::Ui,
    env: &Env,
    view: &mut CommitView,
    pane: &mut PaneState,
    prefs: &mut DetailsSettings,
    cmds: &mut Commands,
) -> bool {
    let mut open_window = false;
    ui.horizontal(|ui| {
        // Same word in every language; "Commit" alone is the button's key.
        ui.selectable_value(&mut prefs.tab, DetailsTab::Commit, "Commit");
        ui.selectable_value(&mut prefs.tab, DetailsTab::Changes, t!("Changes"));
        ui.selectable_value(&mut prefs.tab, DetailsTab::FileTree, t!("File Tree"));
        if env.main {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                open_window = ui
                    .small_button(t!("Open in new window"))
                    .on_hover_text(t!("Show this commit in its own window"))
                    .clicked();
                if ui
                    .small_button(t!("Large view"))
                    .on_hover_text(t!("Space in the commit list"))
                    .clicked()
                {
                    cmds.push(crate::ui::history_options::cmd(|t, _| {
                        t.details.popup = true
                    }));
                }
            });
        }
    });
    let id = view.details.info.id;
    match prefs.tab {
        DetailsTab::Commit => {
            egui::Panel::left(egui::Id::unique(("details-info", env.salt)))
                .resizable(true)
                .default_size(360.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("details-info-scroll", env.salt))
                        .max_height(ui.available_height() * 0.6)
                        .show(ui, |ui| {
                            info(ui, env, view, pane, cmds);
                        });
                    if view.is_stash {
                        stash_actions(ui, env, view, cmds);
                    }
                    ui.separator();
                    files(ui, env, view, pane, prefs, cmds);
                });
        }
        DetailsTab::Changes => {
            egui::Panel::left(egui::Id::unique(("details-files", env.salt)))
                .resizable(true)
                .default_size(360.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.monospace(short(id));
                        ui.add(egui::Label::new(&view.details.info.summary).truncate());
                    });
                    ui.separator();
                    files(ui, env, view, pane, prefs, cmds);
                });
        }
        DetailsTab::FileTree => {
            file_tree::show_tree(ui, env.salt, env.repo, id, &mut pane.tree, cmds);
            return open_window;
        }
    }
    egui::CentralPanel::default_margins().show(ui, |ui| diff(ui, env, view, cmds));
    open_window
}

fn diff(ui: &mut egui::Ui, env: &Env, view: &CommitView, cmds: &mut Commands) {
    let Some(file) = view.selected_file.and(view.diff.as_ref()) else {
        ui.centered_and_justified(|ui| ui.weak(t!("No file selected.")));
        return;
    };
    ui.horizontal(|ui| {
        ui.strong(&file.path);
        if ui
            .small_button(t!("External diff"))
            .on_hover_text("Ctrl+D")
            .clicked()
        {
            cmds.push(file_actions::external_diff(
                env.repo,
                file,
                DiffSide::Blob,
                DiffSide::Blob,
            ));
        }
    });
    let id = format!("commit-diff{}", env.salt);
    diff_view::offer_reveal(ui.ctx(), &id);
    let action = diff_extra::show_file(
        ui,
        &id,
        env.repo,
        DiffSource::Commit(view.details.info.id),
        file,
        diff_view::DiffMode::ReadOnly,
        false,
        None,
    );
    if let Some(diff_view::DiffAction::Reveal(line)) = action {
        cmds.push(file_actions::reveal_line(env.repo.path(), file, line));
    }
}

/// A link to a commit: shows it on hover, selects it on click.
fn commit_link(ui: &mut egui::Ui, repo: &Repo, text: &str, id: Oid, cmds: &mut Commands) {
    let resp = ui
        .link(egui::RichText::new(text).monospace())
        .on_hover_ui(|ui| preview(ui, repo, id));
    if resp.clicked() {
        cmds.push(Command::SelectCommit(id));
    }
}

/// A short preview of a commit, for tooltips.
fn preview(ui: &mut egui::Ui, repo: &Repo, id: Oid) {
    match repo.find_commit_info(id) {
        Ok(c) => {
            ui.set_max_width(420.0);
            ui.horizontal(|ui| {
                ui.monospace(short(id));
                ui.strong(&c.summary);
            });
            ui.weak(format!(
                "{} <{}> — {}",
                c.author,
                c.email,
                format_date(c.time, c.offset_minutes)
            ));
            ui.weak(t!("Click to select it"));
        }
        Err(e) => {
            ui.weak(e.to_string());
        }
    }
}

fn info(
    ui: &mut egui::Ui,
    env: &Env,
    view: &CommitView,
    pane: &mut PaneState,
    cmds: &mut Commands,
) {
    let d = &view.details;
    let c = &d.info;
    pane.links.update(env.repo, c.id, &d.message);
    message(ui, env.repo, d.message.trim_end(), &pane.links.found, cmds);
    ui.add_space(6.0);
    let person = |ui: &mut egui::Ui, name: &str, email: &str, time: i64, offset: i32| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(egui::Label::new(format!("{name} <{email}>")).truncate());
            ui.weak(format_date(time, offset));
        });
    };
    egui::Grid::new(("commit-info", env.salt))
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            ui.weak("SHA");
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(c.id.to_string()).monospace()).truncate(),
                );
            });
            ui.end_row();
            ui.weak(t!("Author"));
            person(ui, &c.author, &c.email, c.time, c.offset_minutes);
            ui.end_row();
            ui.weak(t!("Committer"));
            let (t, o) = (d.commit_time, d.commit_offset_minutes);
            person(ui, &d.committer, &d.committer_email, t, o);
            ui.end_row();
            if !c.parents.is_empty() {
                ui.weak(if c.parents.len() > 1 {
                    t!("Parents")
                } else {
                    t!("Parent")
                });
                ui.horizontal_wrapped(|ui| {
                    for p in &c.parents {
                        commit_link(ui, env.repo, &short(*p), *p, cmds);
                    }
                });
                ui.end_row();
            }
            if !view.is_stash {
                contained_in(ui, env, c.id, pane);
            }
        });
    ui.horizontal_wrapped(|ui| {
        if ui
            .small_button(t!("Copy SHA"))
            .on_hover_text("Ctrl+C")
            .clicked()
        {
            cmds.push(Command::Copy(c.id.to_string()));
        }
        if ui
            .small_button(t!("Copy info"))
            .on_hover_text("Ctrl+Shift+C")
            .clicked()
        {
            cmds.push(Command::Copy(commit_text(d)));
        }
        let web = env
            .remote_url
            .and_then(|u| browse::commit_web_url(u, &c.id.to_string()));
        if let Some(url) = web
            && !view.is_stash
            && ui
                .small_button(t!("Open in browser"))
                .on_hover_text(&url)
                .clicked()
        {
            cmds.push(Command::OpenUrl(url));
        }
    });
}

/// Branches and tags that contain the commit (two grid rows).
fn contained_in(ui: &mut egui::Ui, env: &Env, id: Oid, pane: &mut PaneState) {
    let ctx = ui.ctx().clone();
    match pane.contains.get(&ctx, env.git, id) {
        None => {
            ui.weak(t!("Branches"));
            ui.weak(t!("Searching…"));
            ui.end_row();
        }
        Some(Err(e)) => {
            ui.weak(t!("Branches"));
            ui.weak(e.as_str());
            ui.end_row();
        }
        Some(Ok(c)) => {
            for (label, list) in [(t!("Branches"), &c.branches), (t!("Tags"), &c.tags)] {
                ui.weak(label);
                ui.horizontal_wrapped(|ui| {
                    if list.is_empty() {
                        ui.weak("—");
                    }
                    for name in list {
                        ui.label(egui::RichText::new(name).monospace());
                    }
                });
                ui.end_row();
            }
            if c.more > 0 {
                ui.label("");
                ui.weak(t!("and {} more", c.more));
                ui.end_row();
            }
        }
    }
}

/// The message; commit ids in it are links with a preview.
fn message(
    ui: &mut egui::Ui,
    repo: &Repo,
    text: &str,
    links: &[(std::ops::Range<usize>, Oid)],
    cmds: &mut Commands,
) {
    let mut start = 0;
    for (n, line) in text.split('\n').enumerate() {
        let end = start + line.len();
        let in_line: Vec<_> = links
            .iter()
            .filter(|(r, _)| r.start >= start && r.end <= end)
            .collect();
        let rich = |s: &str| {
            let t = egui::RichText::new(s);
            if n == 0 { t.strong() } else { t }
        };
        if line.trim().is_empty() {
            ui.add_space(4.0);
        } else if in_line.is_empty() {
            ui.label(rich(line));
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let mut pos = start;
                for (r, id) in in_line {
                    if r.start > pos {
                        ui.label(rich(&text[pos..r.start]));
                    }
                    commit_link(ui, repo, &text[r.clone()], *id, cmds);
                    pos = r.end;
                }
                if pos < end {
                    ui.label(rich(&text[pos..end]));
                }
            });
        }
        start = end + 1;
    }
}

fn stash_actions(ui: &mut egui::Ui, env: &Env, view: &CommitView, cmds: &mut Commands) {
    let Some(stash) = env.stashes.iter().find(|s| s.id == view.details.info.id) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!env.busy, |ui| {
            if ui.button(t!("Apply")).clicked() {
                cmds.run(Op::StashApply {
                    stash: stash.clone(),
                    pop: false,
                });
            }
            if ui.button(t!("Pop")).clicked() {
                cmds.run(Op::StashApply {
                    stash: stash.clone(),
                    pop: true,
                });
            }
            if ui.button(t!("Drop…")).clicked() {
                cmds.push(Command::Dialog(crate::ui::dialogs::Dialog::confirm(
                    &t!("Drop stash"),
                    &t!("Delete {}? This cannot be undone.", stash.refname()),
                    Op::StashDrop(stash.clone()),
                    true,
                )));
            }
        });
        if ui.button(t!("Compare with working copy")).clicked() {
            cmds.push(compare_with_workdir(stash.id, Some(stash.refname())));
        }
    });
}

/// Compares a commit (or stash) with the working copy.
pub fn compare_with_workdir(id: Oid, label: Option<String>) -> Command {
    crate::ui::history_options::cmd(move |t, _| {
        let mut from = compare::commit_side(t, id);
        if let Some(l) = label {
            from.label = l;
        }
        compare::open(t, from, compare::workdir_side());
    })
}

fn files(
    ui: &mut egui::Ui,
    env: &Env,
    view: &mut CommitView,
    pane: &mut PaneState,
    prefs: &mut DetailsSettings,
    cmds: &mut Commands,
) {
    file_tree::list_toolbar(ui, &view.files, &mut prefs.files_as_tree, &mut pane.list);
    let root = env.repo.path().to_path_buf();
    let id = view.details.info.id;
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .id_salt(("commit-files", env.salt))
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut menu = |ui: &mut egui::Ui, f: &ferrum_core::FileDiff| {
                if ui.button(t!("Copy path")).clicked() {
                    cmds.push(Command::Copy(f.path.clone()));
                    ui.close();
                }
                if ui.button(t!("File history")).clicked() {
                    cmds.push(Command::FileHistory(f.path.clone()));
                    ui.close();
                }
                if ui.button(t!("Blame")).clicked() {
                    cmds.push(Command::Blame(f.path.clone()));
                    ui.close();
                }
                if f.kind != ferrum_core::ChangeKind::Deleted
                    && ui.button(t!("Save file…")).clicked()
                {
                    ui.close();
                    let (repo, path) = (env.repo, f.path.clone());
                    let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                    file_actions::save_as(
                        &name,
                        || {
                            repo.file_at(browse::Rev::Commit(id), &path)
                                .map_err(|e| e.to_string())?
                                .ok_or_else(|| t!("{} does not exist in this commit.", path))
                        },
                        cmds,
                    );
                }
                ui.separator();
                let button = egui::Button::new(t!("External diff")).shortcut_text("Ctrl+D");
                if ui.add(button).clicked() {
                    let cmd = env
                        .repo
                        .commit_file_diff_with(id, &f.path, f.old_path.as_deref(), &env.cfg)
                        .map_err(|e| e.to_string())
                        .and_then(|d| d.ok_or_else(|| t!("No changes to show.")))
                        .map(|d| {
                            file_actions::external_diff(
                                env.repo,
                                &d,
                                DiffSide::Blob,
                                DiffSide::Blob,
                            )
                        })
                        .unwrap_or_else(Command::Error);
                    cmds.push(cmd);
                    ui.close();
                }
                file_actions::workdir_menu(ui, &root, &f.path, cmds);
            };
            clicked = file_tree::changed_files(
                ui,
                &view.files,
                view.selected_file,
                prefs.files_as_tree,
                &mut pane.list,
                &mut menu,
            );
        });
    if let Some(i) = clicked
        && let Err(e) = details_model::select_file(env.repo, view, i, &env.cfg)
    {
        cmds.push(Command::Error(e));
    }
}
