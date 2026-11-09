//! The main menu bar: File, Edit, View, Repository, Window and Help.
//!
//! Items with a keyboard shortcut run through `keymap::trigger`, so a click
//! does exactly what the key does and shows the key the user set. F10 opens
//! the first menu, Alt + the underlined letter opens a menu, Left/Right move
//! between open menus.

use eframe::egui::{self, Button, Id, Key, Modifiers, Popup, TextFormat, text::LayoutJob};

use crate::command::{Command, Commands, DevStep, Op};
use crate::keymap::{self, Action};
use crate::repos::RepoCmd;
use crate::settings::{Settings, ThemeChoice};
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;

/// What the menus need from the app.
pub struct Bar<'a> {
    pub settings: &'a mut Settings,
    pub tabs: &'a [RepoTab],
    pub active: usize,
    /// The Repository Manager is shown instead of a repository.
    pub home: bool,
    /// A closed tab can be reopened.
    pub can_reopen: bool,
}

impl Bar<'_> {
    fn tab(&self) -> Option<&RepoTab> {
        self.tabs.get(self.active).filter(|_| !self.home)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    Edit,
    View,
    Repository,
    Window,
    Help,
}

pub const MENUS: [Menu; 6] = [
    Menu::File,
    Menu::Edit,
    Menu::View,
    Menu::Repository,
    Menu::Window,
    Menu::Help,
];

impl Menu {
    pub fn title(self) -> String {
        match self {
            Menu::File => t!("File"),
            Menu::Edit => t!("Edit"),
            Menu::View => t!("View"),
            Menu::Repository => t!("Repository"),
            Menu::Window => t!("Window"),
            Menu::Help => t!("Help"),
        }
    }
}

/// Request to open (`Some(i)`) or close (`None`) a menu, kept until the bar is drawn.
fn request_id() -> Id {
    Id::unique("main-menu-request")
}

/// Popup ids of the menus, from the last frame.
fn ids_id() -> Id {
    Id::unique("main-menu-ids")
}

/// The first enabled item of the next menu drawn takes the keyboard focus.
fn focus_id() -> Id {
    Id::unique("main-menu-focus")
}

/// Opens menu `i` (dev aid and keyboard).
pub fn open(ctx: &egui::Context, i: usize) {
    ctx.data_mut(|d| d.insert_temp(request_id(), Some(i.min(MENUS.len() - 1))));
    ctx.request_repaint();
}

/// The letter of each title that opens it with Alt: the first one not taken.
pub fn mnemonics(titles: &[String]) -> Vec<Option<(usize, char)>> {
    let mut used: Vec<char> = Vec::new();
    titles
        .iter()
        .map(|t| {
            let hit = t
                .char_indices()
                .find(|(_, c)| c.is_ascii_alphabetic() && !used.contains(&c.to_ascii_lowercase()));
            if let Some((_, c)) = hit {
                used.push(c.to_ascii_lowercase());
            }
            hit
        })
        .collect()
}

/// The title with its letter underlined while Alt is held.
fn title_job(ui: &egui::Ui, title: &str, mnemonic: Option<(usize, char)>, alt: bool) -> LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = ui.visuals().widgets.inactive.fg_stroke.color;
    let plain = TextFormat::simple(font.clone(), color);
    let mut job = LayoutJob::default();
    match mnemonic.filter(|_| alt) {
        Some((at, c)) => {
            let end = at + c.len_utf8();
            job.append(&title[..at], 0.0, plain.clone());
            let mut under = plain.clone();
            under.underline = egui::Stroke::new(1.0, color);
            job.append(&title[at..end], 0.0, under);
            job.append(&title[end..], 0.0, plain);
        }
        None => job.append(title, 0.0, plain),
    }
    job
}

/// Keys that open, close or switch menus. Returns the request to apply.
fn keyboard(
    ctx: &egui::Context,
    open_now: Option<usize>,
    letters: &[Option<(usize, char)>],
) -> Option<Option<usize>> {
    let typing = ctx.text_edit_focused();
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::F10) {
            return Some(if open_now.is_some() { None } else { Some(0) });
        }
        if let Some(cur) = open_now {
            let n = MENUS.len();
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                return Some(Some((cur + n - 1) % n));
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                return Some(Some((cur + 1) % n));
            }
        }
        // Not while typing: AltGr+letter types characters (€) on many layouts.
        if typing {
            return None;
        }
        for (idx, m) in letters.iter().enumerate() {
            let Some(key) =
                m.and_then(|(_, c)| Key::from_name(&c.to_ascii_uppercase().to_string()))
            else {
                continue;
            };
            if i.consume_key(Modifiers::ALT, key) {
                return Some(Some(idx));
            }
        }
        None
    })
}

pub fn show(ui: &mut egui::Ui, mut bar: Bar, cmds: &mut Commands) {
    let ctx = ui.ctx().clone();
    let titles: Vec<String> = MENUS.iter().map(|m| m.title()).collect();
    let letters = mnemonics(&titles);
    let prev: Vec<Id> = ctx.data(|d| d.get_temp(ids_id())).unwrap_or_default();
    let open_now = prev.iter().position(|&id| Popup::is_id_open(&ctx, id));
    let mut request: Option<Option<usize>> =
        ctx.data_mut(|d| d.remove_temp::<Option<usize>>(request_id()));
    if let Some(r) = keyboard(&ctx, open_now, &letters) {
        request = Some(r);
    }
    let alt = ctx.input(|i| i.modifiers.alt && !i.modifiers.command);

    let ids = egui::MenuBar::new()
        .ui(ui, |ui| {
            let mut ids = Vec::new();
            for (idx, &menu) in MENUS.iter().enumerate() {
                let job = title_job(ui, &titles[idx], letters[idx], alt);
                let (resp, _) = egui::containers::menu::MenuButton::new(job).ui(ui, |ui| {
                    ui.set_min_width(300.0);
                    CHECK_COLUMN.set(matches!(menu, Menu::View | Menu::Window));
                    contents(ui, menu, &mut bar, cmds);
                });
                ids.push(Popup::menu(&resp).get_id());
            }
            ids
        })
        .inner;

    match request {
        Some(Some(i)) => {
            Popup::open_id(&ctx, ids[i]);
            ctx.data_mut(|d| d.insert_temp(focus_id(), true));
            ctx.request_repaint();
        }
        Some(None) => Popup::close_all(&ctx),
        None => {}
    }
    ctx.data_mut(|d| d.insert_temp(ids_id(), ids));
}

/// All menus behind one ☰ button, for when the menu bar is hidden.
pub fn compact(ui: &mut egui::Ui, mut bar: Bar, cmds: &mut Commands) {
    let button = crate::ui::toolbar::Tool::icon(crate::icons::MENU, "☰").show(ui);
    let resp = button.main.on_hover_text(t!("Menu"));
    egui::Popup::menu(&resp).show(|ui| {
        for menu in MENUS {
            ui.menu_button(menu.title(), |ui| {
                ui.set_min_width(300.0);
                CHECK_COLUMN.set(matches!(menu, Menu::View | Menu::Window));
                contents(ui, menu, &mut bar, cmds);
            });
        }
    });
}

fn contents(ui: &mut egui::Ui, menu: Menu, bar: &mut Bar, cmds: &mut Commands) {
    match menu {
        Menu::File => file(ui, bar, cmds),
        Menu::Edit => edit(ui, bar, cmds),
        Menu::View => view(ui, bar, cmds),
        Menu::Repository => repository(ui, bar, cmds),
        Menu::Window => window(ui, bar, cmds),
        Menu::Help => help(ui, cmds),
    }
}

// ----- Items -----

thread_local! {
    /// The menu being drawn has a column for check marks (View, Window).
    static CHECK_COLUMN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A menu item; true when clicked (the menu is closed then).
fn item(ui: &mut egui::Ui, label: String, shortcut: String, enabled: bool) -> bool {
    marked(ui, false, label, shortcut, enabled)
}

/// An item with a check mark when `on`.
fn check(ui: &mut egui::Ui, on: bool, label: String, shortcut: String, enabled: bool) -> bool {
    marked(ui, on, label, shortcut, enabled)
}

fn marked(ui: &mut egui::Ui, on: bool, label: String, shortcut: String, enabled: bool) -> bool {
    use egui::AtomExt as _;
    let button = if CHECK_COLUMN.get() {
        let mark = if on { "✔" } else { "" };
        let size = egui::vec2(14.0, ui.spacing().interact_size.y * 0.5);
        Button::new((mark.atom_size(size), label))
    } else {
        Button::new(label)
    };
    let resp = ui.add_enabled(enabled, button.shortcut_text(shortcut));
    take_focus(ui, &resp);
    if resp.clicked() {
        ui.close();
        return true;
    }
    false
}

fn take_focus(ui: &egui::Ui, resp: &egui::Response) {
    if resp.enabled()
        && ui
            .data_mut(|d| d.remove_temp::<bool>(focus_id()))
            .unwrap_or(false)
    {
        resp.request_focus();
    }
}

/// An item that runs action `a`, as if its key was pressed.
fn action(ui: &mut egui::Ui, label: String, a: Action, enabled: bool) {
    if item(ui, label, keymap::text(a), enabled) {
        keymap::trigger(ui.ctx(), a);
    }
}

fn command(
    ui: &mut egui::Ui,
    label: String,
    enabled: bool,
    cmds: &mut Commands,
    make: impl FnOnce() -> Command,
) {
    if item(ui, label, String::new(), enabled) {
        cmds.push(make());
    }
}

// ----- Menus -----

fn file(ui: &mut egui::Ui, bar: &Bar, cmds: &mut Commands) {
    action(ui, t!("New tab"), Action::NewTab, true);
    action(ui, t!("Open repository…"), Action::OpenRepo, true);
    action(ui, t!("Open Quickly…"), Action::OpenQuickly, true);
    command(ui, t!("Clone repository…"), true, cmds, || {
        Command::Dialog(Dialog::clone_repo(&*bar.settings))
    });
    command(ui, t!("New repository…"), true, cmds, || {
        Command::Dialog(Dialog::Init {
            path: String::new(),
        })
    });
    let recent = &bar.settings.recent;
    ui.add_enabled_ui(!recent.is_empty(), |ui| {
        ui.menu_button(t!("Recent"), |ui| {
            for p in recent {
                if ui.button(p.display().to_string()).clicked() {
                    cmds.push(Command::OpenRepo(p.clone()));
                    ui.close();
                }
            }
        });
    });
    ui.separator();
    action(
        ui,
        t!("Reopen closed tab"),
        Action::ReopenTab,
        bar.can_reopen,
    );
    let closable = bar.home && !bar.tabs.is_empty() || bar.tab().is_some();
    action(ui, t!("Close tab"), Action::CloseTab, closable);
    ui.separator();
    action(ui, t!("Settings…"), Action::Settings, true);
    ui.separator();
    action(ui, t!("Quit"), Action::Quit, true);
}

fn edit(ui: &mut egui::Ui, bar: &Bar, cmds: &mut Commands) {
    let tab = bar.tab();
    let idle = tab.is_some_and(|t| !t.busy());
    let undo = tab.and_then(|t| t.undo.last()).map(|u| u.label.clone());
    let label = match &undo {
        Some(l) => t!("Undo: {}", l),
        None => t!("Undo"),
    };
    action(ui, label, Action::Undo, idle && undo.is_some());
    ui.separator();
    let has = tab.is_some();
    action(ui, t!("Find…"), Action::Find, has);
    action(ui, t!("Find next"), Action::NextMatch, has);
    action(ui, t!("Find previous"), Action::PrevMatch, has);
    action(ui, t!("Go to commit…"), Action::GoTo, has);
    let selected = tab.is_some_and(|t| t.view == View::History && t.commit_view.is_some());
    action(ui, t!("Copy commit info"), Action::CopyCommitInfo, selected);
    ui.separator();
    let changes = tab.is_some_and(|t| !t.status.is_empty());
    command(ui, t!("Stage all changes"), idle && changes, cmds, || {
        Command::Run(Op::StageAll)
    });
    command(ui, t!("Unstage all changes"), idle && changes, cmds, || {
        Command::Run(Op::UnstageAll)
    });
    ui.separator();
    action(ui, t!("Command palette…"), Action::Palette, true);
}

fn view(ui: &mut egui::Ui, bar: &mut Bar, cmds: &mut Commands) {
    if let Some(t) = bar.tab() {
        let v = t.view;
        let history = v == View::History;
        if check(
            ui,
            history,
            t!("History"),
            keymap::text(Action::ShowHistory),
            true,
        ) {
            keymap::trigger(ui.ctx(), Action::ShowHistory);
        }
        if check(
            ui,
            v == View::Changes,
            t!("Local changes"),
            keymap::text(Action::ShowChanges),
            true,
        ) {
            keymap::trigger(ui.ctx(), Action::ShowChanges);
        }
        let mut views = vec![(View::Overview, t!("Overview"))];
        if t.hosted.is_some() {
            views.push((View::PullRequests, t!("Pull requests")));
            views.push((View::Issues, t!("Issues")));
        }
        for (target, label) in views {
            if check(ui, v == target, label, String::new(), true) {
                cmds.push(Command::SetView(target));
            }
        }
        ui.separator();
        let details = !bar.settings.details.collapsed;
        let key = keymap::text(Action::ToggleDetails);
        if check(ui, details, t!("Commit details"), key, history) {
            keymap::trigger(ui.ctx(), Action::ToggleDetails);
        }
        action(
            ui,
            t!("Only the current branch"),
            Action::ActiveBranch,
            true,
        );
        action(ui, t!("Show reflog"), Action::Reflog, true);
        action(ui, t!("Jump to HEAD"), Action::JumpToHead, true);
        ui.separator();
    }
    let spacer = egui::AtomExt::atom_size("", egui::vec2(14.0, 0.0));
    ui.menu_button((spacer, t!("Theme")), |ui| {
        let theme = &mut bar.settings.theme;
        for choice in ThemeChoice::ALL {
            if ui.radio_value(theme, choice, choice.label()).clicked() {
                ui.close();
            }
        }
    });
    let vertical = &mut bar.settings.library.vertical_tabs;
    if check(ui, *vertical, t!("Vertical tab list"), String::new(), true) {
        *vertical = !*vertical;
    }
    ui.separator();
    action(ui, t!("Refresh"), Action::Refresh, bar.tab().is_some());
}

fn repository(ui: &mut egui::Ui, bar: &Bar, cmds: &mut Commands) {
    let Some(t) = bar.tab() else {
        ui.weak(t!("No repository open"));
        ui.separator();
        action(ui, t!("Open repository…"), Action::OpenRepo, true);
        return;
    };
    ui.add_enabled_ui(!t.busy(), |ui| {
        action(ui, "Fetch".into(), Action::Fetch, true);
        command(ui, t!("Fetch with options…"), true, cmds, || {
            Command::Dialog(Dialog::Sync(crate::ui::sync_dialogs::fetch(t, None)))
        });
        action(ui, "Pull".into(), Action::Pull, true);
        command(ui, t!("Pull with options…"), true, cmds, || {
            Command::Dialog(Dialog::pull(t))
        });
        action(ui, t!("Push…"), Action::Push, t.head.is_some());
        command(ui, t!("Push tags…"), true, cmds, || {
            Command::Dialog(Dialog::Sync(crate::ui::sync_dialogs::push_tags(t, &[])))
        });
        ui.separator();
        action(ui, t!("New branch…"), Action::NewBranch, true);
        action(ui, t!("New tag…"), Action::NewTag, true);
        action(ui, t!("Check out branch…"), Action::QuickCheckout, true);
        let dirty = !t.status.is_empty();
        action(ui, t!("Stash changes…"), Action::Stash, dirty);
        command(ui, t!("Quick stash"), dirty, cmds, || {
            Command::Run(crate::ui::ref_dialogs::quick_stash())
        });
        let latest = t.stashes.first();
        command(
            ui,
            t!("Pop the latest stash"),
            latest.is_some(),
            cmds,
            || {
                Command::Run(Op::StashApply {
                    stash: latest.cloned().expect("enabled only with a stash"),
                    pop: true,
                })
            },
        );
        ui.separator();
        let rebase = t!("Interactive rebase of the last 10 commits…");
        command(ui, rebase, t.head_oid.is_some(), cmds, || {
            Command::PlanRebase(t.repo.resolve("HEAD~10").ok())
        });
        command(ui, t!("Add remote…"), true, cmds, || {
            Command::Dialog(Dialog::AddRemote {
                name: String::new(),
                url: String::new(),
            })
        });
        command(ui, t!("Add worktree…"), true, cmds, || {
            Command::Dialog(Dialog::worktree_add(t))
        });
        ui.menu_button(t!("Submodules"), |ui| {
            command(ui, t!("Add submodule…"), true, cmds, || {
                let d = crate::ui::submodules::SubmoduleDialog::add();
                Command::Dialog(Dialog::Submodule(d))
            });
            let any = !t.submodules.is_empty();
            command(ui, t!("Update submodules"), any, cmds, || {
                Command::Run(Op::SubmoduleUpdate)
            });
        });
        crate::ui::lfs::menu(ui, t, cmds);
        crate::ui::gitflow::menu(ui, t, cmds);
        crate::ui::patches::menu(ui, t, cmds);
        crate::ui::stacks::repository_menu(ui, t, cmds);
        crate::ui::custom_commands::repository_menu(ui, t, cmds);
    });
    ui.separator();
    command(ui, t!("Set repository identity…"), true, cmds, || {
        Command::Dialog(Dialog::identity(&t.identity))
    });
    command(ui, t!("Repository settings…"), true, cmds, || {
        Command::Repos(RepoCmd::Settings)
    });
    ui.separator();
    action(ui, t!("Open in terminal"), Action::Terminal, true);
    action(ui, t!("Show in file manager"), Action::FileManager, true);
    command(ui, t!("Open in editor"), true, cmds, || {
        Command::External(crate::external::Launch::Editor {
            path: t.path().into(),
            line: None,
        })
    });
    crate::ui::issues::website_button(ui, t, cmds);
}

fn window(ui: &mut egui::Ui, bar: &Bar, cmds: &mut Commands) {
    let many = bar.tabs.len() > 1;
    action(ui, t!("Next tab"), Action::NextTab, many);
    action(ui, t!("Previous tab"), Action::PrevTab, many);
    if !bar.tabs.is_empty() {
        ui.separator();
        for (i, tab) in bar.tabs.iter().enumerate() {
            let on = i == bar.active && !bar.home;
            if check(ui, on, tab.name.clone(), String::new(), true) {
                cmds.push(Command::SelectTab(i));
            }
        }
    }
    ui.separator();
    action(ui, t!("Repository Manager"), Action::NewTab, true);
    let has = bar.tab().is_some();
    command(ui, t!("Open in new window"), has, cmds, || {
        Command::Repos(RepoCmd::OpenInWindow(bar.active))
    });
    command(ui, t!("Close other tabs"), has && many, cmds, || {
        Command::Repos(RepoCmd::CloseOthers(bar.active))
    });
    command(ui, t!("Close all tabs"), !bar.tabs.is_empty(), cmds, || {
        Command::Repos(RepoCmd::CloseAll)
    });
    ui.separator();
    command(ui, t!("Activity"), true, cmds, || Command::ShowActivity);
}

fn help(ui: &mut egui::Ui, cmds: &mut Commands) {
    let url = crate::ui::settings::REPO_URL;
    let section = |s: &str| Command::DevStep(DevStep::SettingsSection(s.into()));
    command(ui, t!("Keyboard shortcuts"), true, cmds, || {
        section("shortcuts")
    });
    command(ui, t!("Documentation"), true, cmds, || {
        Command::OpenUrl(format!("{url}#readme"))
    });
    command(ui, t!("Release notes"), true, cmds, || {
        Command::OpenUrl(format!("{url}/releases"))
    });
    command(ui, t!("Report an issue"), true, cmds, || {
        Command::OpenUrl(format!("{url}/issues/new"))
    });
    ui.separator();
    command(ui, t!("About Ferrum"), true, cmds, || section("about"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_are_unique_letters() {
        let titles: Vec<String> = ["File", "Edit", "View", "Repository", "Window", "Help"]
            .map(String::from)
            .to_vec();
        let m: Vec<char> = mnemonics(&titles)
            .into_iter()
            .map(|m| m.unwrap().1)
            .collect();
        assert_eq!(m, ['F', 'E', 'V', 'R', 'W', 'H']);
        // pt-BR: Exibir and Editar both start with E.
        let pt: Vec<String> = [
            "Arquivo",
            "Editar",
            "Exibir",
            "Repositório",
            "Janela",
            "Ajuda",
        ]
        .map(String::from)
        .to_vec();
        let m: Vec<char> = mnemonics(&pt).into_iter().map(|m| m.unwrap().1).collect();
        assert_eq!(m, ['A', 'E', 'x', 'R', 'J', 'u']);
    }
}
