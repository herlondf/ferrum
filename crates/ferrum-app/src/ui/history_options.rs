//! Graph options: the View menu of the history, graph filters (also used by
//! the sidebar), the filter shown in the status bar and history shortcuts.

use eframe::egui::{self, Key};
use ferrum_core::log::{LogOptions, ref_matches};

use crate::command::{Command, Commands};
use crate::settings::Settings;
use crate::tab::{RepoTab, View};
use crate::ui::history_model::{self, HistorySettings};
use crate::ui::{issue_links, stats};

/// A change to the history state, run by the app with the active tab and the settings.
pub type HistoryFn = Box<dyn FnOnce(&mut RepoTab, &mut HistorySettings)>;

pub fn cmd(f: impl FnOnce(&mut RepoTab, &mut HistorySettings) + 'static) -> Command {
    Command::History(Box::new(f))
}

/// Changes the graph options of the tab and reloads the history.
pub fn change_log(f: impl FnOnce(&mut LogOptions) + 'static) -> Command {
    cmd(move |tab, _| {
        f(&mut tab.history.ext.prefs.log);
        history_model::reload(tab);
    })
}

/// Shows only `pattern` in the graph (a full ref name, or a folder ending in `/`).
pub fn show_only(pattern: String) -> Command {
    change_log(move |o| {
        o.include = vec![pattern.clone()];
        o.hidden.retain(|h| *h != pattern);
    })
}

/// Adds `pattern` to the refs shown, keeping the ones already chosen.
pub fn add_to_filter(pattern: String) -> Command {
    change_log(move |o| {
        if !o.include.contains(&pattern) {
            o.include.push(pattern.clone());
        }
        o.hidden.retain(|h| *h != pattern);
    })
}

pub fn set_hidden(pattern: String, hide: bool) -> Command {
    change_log(move |o| {
        o.hidden.retain(|h| *h != pattern);
        if hide {
            o.hidden.push(pattern.clone());
            o.include.retain(|i| *i != pattern);
        }
    })
}

pub fn clear_filter() -> Command {
    change_log(|o| {
        o.include.clear();
        o.hidden.clear();
    })
}

/// Context menu entries for a branch, branch folder or remote in the sidebar.
/// `pattern` is a full ref name, or a folder/remote prefix ending in `/`.
pub fn ref_menu(ui: &mut egui::Ui, tab: &RepoTab, pattern: &str, cmds: &mut Commands) {
    let log = &tab.history.ext.prefs.log;
    let hidden = log.hidden.iter().any(|h| h == pattern);
    let included = log.include.iter().any(|i| i == pattern);
    ui.separator();
    if !(included && log.include.len() == 1)
        && ui.button(t!("Show only this in the graph")).clicked()
    {
        cmds.push(show_only(pattern.to_owned()));
        ui.close();
    }
    if !log.include.is_empty() && !included && ui.button(t!("Add to the graph filter")).clicked() {
        cmds.push(add_to_filter(pattern.to_owned()));
        ui.close();
    }
    if included && ui.button(t!("Remove from the graph filter")).clicked() {
        let p = pattern.to_owned();
        cmds.push(change_log(move |o| o.include.retain(|i| *i != p)));
        ui.close();
    }
    let label = if hidden {
        t!("Show in the graph")
    } else {
        t!("Hide from the graph")
    };
    if ui.button(label).clicked() {
        cmds.push(set_hidden(pattern.to_owned(), !hidden));
        ui.close();
    }
}

/// Whether a ref is hidden from the graph by the current filter.
pub fn is_hidden(tab: &RepoTab, full_ref: &str) -> bool {
    let log = &tab.history.ext.prefs.log;
    log.hidden.iter().any(|h| ref_matches(h, full_ref))
        || (!log.include.is_empty() && !log.include.iter().any(|i| ref_matches(i, full_ref)))
        || (log.hide_remotes && full_ref.starts_with("refs/remotes/"))
}

/// Short text for a filter pattern: `main`, `feature/*`, `origin/*`.
fn pattern_label(p: &str) -> String {
    let short = p
        .strip_prefix("refs/heads/")
        .or_else(|| p.strip_prefix("refs/remotes/"))
        .unwrap_or(p);
    match short.strip_suffix('/') {
        Some(folder) => format!("{folder}/*"),
        None => short.to_owned(),
    }
}

/// Filters in effect, for the status bar (empty when the full graph is shown).
pub fn filter_summary(tab: &RepoTab) -> Vec<String> {
    let ext = &tab.history.ext;
    let log = &ext.prefs.log;
    let mut parts = Vec::new();
    if !log.include.is_empty() {
        let names: Vec<String> = log.include.iter().map(|p| pattern_label(p)).collect();
        parts.push(t!("Only {}", names.join(", ")));
    }
    if !log.hidden.is_empty() {
        let names: Vec<String> = log.hidden.iter().map(|p| pattern_label(p)).collect();
        parts.push(t!("Hidden: {}", names.join(", ")));
    }
    if log.hide_remotes {
        parts.push(t!("No remote branches"));
    }
    if log.first_parent {
        parts.push(t!("First parent"));
    }
    if log.reflog {
        parts.push(t!("Reflog"));
    }
    if let Some(a) = &ext.highlight_author {
        parts.push(t!("Highlight: {}", a));
    }
    parts
}

/// The active filter in the status bar, with a button to clear it.
pub fn filter_status(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let parts = filter_summary(tab);
    if parts.is_empty() {
        return;
    }
    if ui
        .small_button(crate::icons::CLOSE)
        .on_hover_text(t!("Clear the graph filter"))
        .clicked()
    {
        cmds.push(cmd(|tab, _| {
            let ext = &mut tab.history.ext;
            ext.highlight_author = None;
            let log = &mut ext.prefs.log;
            log.include.clear();
            log.hidden.clear();
            log.hide_remotes = false;
            log.first_parent = false;
            log.reflog = false;
            history_model::reload(tab);
        }));
    }
    let text = parts.join(" · ");
    ui.add(egui::Label::new(format!("🔍 {text}")).truncate())
        .on_hover_text(text);
}

/// Toggles showing only the current branch (and its upstream).
pub fn filter_active_branch() -> Command {
    cmd(|tab, _| {
        let Some(b) = tab.current_branch().cloned() else {
            tab.info(t!("HEAD is not on a branch."));
            return;
        };
        let mut wanted = vec![format!("refs/heads/{}", b.name)];
        if let Some(u) = &b.upstream {
            wanted.push(format!("refs/remotes/{u}"));
        }
        let log = &mut tab.history.ext.prefs.log;
        if log.include == wanted {
            log.include.clear();
        } else {
            log.include = wanted;
        }
        history_model::reload(tab);
    })
}

pub fn toggle_reflog() -> Command {
    change_log(|o| o.reflog = !o.reflog)
}

pub fn jump_to_head() -> Command {
    cmd(|tab, _| {
        if let Some(h) = tab.head_oid
            && !history_model::jump_to(tab, h)
        {
            tab.info(t!("HEAD is hidden by the graph filter."));
        }
    })
}

/// Goes to a commit by hash, branch or tag name.
pub fn go_to(rev: String) -> Command {
    cmd(move |tab, _| {
        let rev = rev.trim();
        match tab.repo.resolve(rev) {
            Ok(id) => {
                if !history_model::jump_to(tab, id) {
                    // Not in the graph (filtered out): show its details anyway.
                    tab.view = View::History;
                    tab.history.selected = Some(id);
                    tab.load_commit(id, false);
                    tab.info(t!("{} is not in the graph with the current filter.", rev));
                }
            }
            Err(_) => tab.error(t!("No commit found for {}", rev)),
        }
    })
}

/// History shortcuts, checked by the app while the history is shown.
pub fn shortcuts(ctx: &egui::Context, tab: &RepoTab, cmds: &mut Commands) {
    use crate::keymap::{Action, pressed};
    if pressed(ctx, Action::Reflog) {
        cmds.push(toggle_reflog());
    }
    if pressed(ctx, Action::ActiveBranch) {
        cmds.push(filter_active_branch());
    }
    if pressed(ctx, Action::JumpToHead) {
        cmds.push(jump_to_head());
    }
    // Ctrl+1 while the history is already shown jumps to HEAD.
    let again = crate::keymap::peek(ctx, Action::ShowHistory);
    if again && tab.view == View::History {
        cmds.push(jump_to_head());
    }
    if pressed(ctx, Action::GoTo) {
        cmds.push(cmd(|tab, _| tab.history.ext.goto = Some(String::new())));
    }
    if tab.view != View::History || ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    if pressed(ctx, Action::PrevMatch) {
        cmds.push(cmd(|tab, _| history_model::step_match(tab, false)));
    } else if pressed(ctx, Action::NextMatch) {
        cmds.push(cmd(|tab, _| history_model::step_match(tab, true)));
    }
}

/// The View menu of the history.
pub fn view_menu(ui: &mut egui::Ui, tab: &RepoTab, settings: &Settings, cmds: &mut Commands) {
    ui.menu_button(t!("View"), |ui| {
        ui.set_min_width(260.0);
        let log = tab.history.ext.prefs.log.clone();
        let mut toggle =
            |ui: &mut egui::Ui, on: bool, label: String, f: fn(&mut LogOptions, bool)| {
                let mut v = on;
                if ui.checkbox(&mut v, label).changed() {
                    cmds.push(change_log(move |o| f(o, v)));
                }
            };
        toggle(ui, log.topo_order, t!("Topological order"), |o, v| {
            o.topo_order = v
        });
        toggle(ui, log.first_parent, t!("First parent only"), |o, v| {
            o.first_parent = v
        });
        toggle(ui, log.reflog, t!("Show reflog (Ctrl+Shift+.)"), |o, v| {
            o.reflog = v
        });
        toggle(ui, log.hide_remotes, t!("Hide remote branches"), |o, v| {
            o.hide_remotes = v
        });
        toggle(ui, log.hide_tags, t!("Hide tags"), |o, v| o.hide_tags = v);
        toggle(ui, log.hide_stashes, t!("Hide stashes"), |o, v| {
            o.hide_stashes = v
        });
        if tab.head.is_some() {
            let b = tab.head.clone().unwrap_or_default();
            let on = log.include.first() == Some(&format!("refs/heads/{b}"));
            let mut v = on;
            if ui
                .checkbox(&mut v, t!("Only the current branch (Ctrl+Shift+A)"))
                .changed()
            {
                cmds.push(filter_active_branch());
            }
        }
        if log.is_filtered() && ui.button(t!("Show all branches")).clicked() {
            cmds.push(clear_filter());
            ui.close();
        }
        ui.separator();
        if ui.button(t!("Collapse all merges")).clicked() {
            cmds.push(cmd(|tab, _| history_model::set_all_collapsed(tab, true)));
            ui.close();
        }
        if ui.button(t!("Expand all merges")).clicked() {
            cmds.push(cmd(|tab, _| history_model::set_all_collapsed(tab, false)));
            ui.close();
        }
        ui.separator();
        let h = settings.history.clone();
        let mut setting = |ui: &mut egui::Ui,
                           on: bool,
                           enabled: bool,
                           label: String,
                           f: fn(&mut HistorySettings, bool)| {
            let mut v = on;
            if ui
                .add_enabled(enabled, egui::Checkbox::new(&mut v, label))
                .changed()
            {
                cmds.push(cmd(move |_, s| f(s, v)));
            }
        };
        setting(ui, h.show_avatars, true, t!("Show avatars"), |s, v| {
            s.show_avatars = v
        });
        setting(
            ui,
            h.network_avatars,
            h.show_avatars,
            t!("Download avatars (Gravatar, GitHub)"),
            |s, v| s.network_avatars = v,
        );
        setting(
            ui,
            h.compact_labels,
            true,
            t!("Compact branch labels"),
            |s, v| s.compact_labels = v,
        );
        setting(
            ui,
            h.vertical_layout,
            true,
            t!("Details beside the list"),
            |s, v| s.vertical_layout = v,
        );
        setting(
            ui,
            h.single_line_rows,
            h.vertical_layout,
            t!("Single-line rows"),
            |s, v| s.single_line_rows = v,
        );
        setting(
            ui,
            h.relative_dates,
            true,
            t!("Relative dates (3 h ago)"),
            |s, v| s.relative_dates = v,
        );
        ui.separator();
        if ui.button(t!("Go to commit…  (Ctrl+G)")).clicked() {
            cmds.push(cmd(|tab, _| tab.history.ext.goto = Some(String::new())));
            ui.close();
        }
        if ui.button(t!("Jump to HEAD  (Ctrl+0)")).clicked() {
            cmds.push(jump_to_head());
            ui.close();
        }
        if ui.button(t!("Issue links…")).clicked() {
            cmds.push(cmd(|tab, _| tab.history.ext.issue_window = true));
            ui.close();
        }
        if ui.button(t!("Statistics…")).clicked() {
            cmds.push(cmd(|tab, _| stats::open(tab)));
            ui.close();
        }
    });
}

/// Windows that belong to the history: go to commit, issue links, statistics.
pub fn windows(ctx: &egui::Context, tab: &mut RepoTab, cmds: &mut Commands) {
    if let Some(text) = &mut tab.history.ext.goto {
        let mut open = true;
        let mut go = None;
        egui::Window::new(t!("Go to commit"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(t!("Hash, branch or tag"))
                        .desired_width(300.0),
                );
                if !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (enter || ui.button(t!("Go")).clicked()) && !text.trim().is_empty() {
                    go = Some(text.clone());
                }
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            open = false;
        }
        if let Some(rev) = go {
            cmds.push(go_to(rev));
            open = false;
        }
        if !open {
            tab.history.ext.goto = None;
        }
    }
    issue_links::window(ctx, tab);
    stats::window(ctx, tab);
}
