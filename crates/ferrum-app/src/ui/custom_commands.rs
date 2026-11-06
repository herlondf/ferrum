//! Custom commands in the UI: the context-menu entries (nested by `/`), the
//! form / confirmation / approval dialog, the output window, the editor in
//! Settings and the palette entries.

use std::cell::RefCell;

use eframe::egui::{self, RichText};
use ferrum_core::{BranchInfo, BranchKind};

use crate::command::{Command, Commands};
use crate::custom_commands::{
    self as cc, CustomCommand, FieldKind, Invocation, Kind, Scope, Target, Vars,
};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::palette::{Entry, Group};

// ----- Menus -----

/// The "Custom commands" submenu for `target`, if any command applies.
pub fn menu(ui: &mut egui::Ui, target: Target, cmds: &mut Commands) {
    let list = cc::for_scope(&target.repo, target.scope);
    if list.is_empty() {
        return;
    }
    ui.separator();
    ui.menu_button(t!("Custom commands"), |ui| {
        let items: Vec<(Vec<&str>, &CustomCommand, bool)> = list
            .iter()
            .map(|(c, shared)| (c.name.split('/').map(str::trim).collect(), c, *shared))
            .collect();
        tree(ui, &items, &target, cmds);
    });
}

/// The hooks of the context menus (one line each where they are used).
pub fn repository_menu(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    menu(ui, Target::repository(tab), cmds);
}

pub fn commit_menu(ui: &mut egui::Ui, tab: &RepoTab, id: ferrum_core::Oid, cmds: &mut Commands) {
    menu(ui, Target::commit(tab, id), cmds);
}

pub fn branch_menu(ui: &mut egui::Ui, tab: &RepoTab, b: &BranchInfo, cmds: &mut Commands) {
    menu(ui, Target::branch(tab, b), cmds);
}

/// Files of the working copy (`sha` = None) or of a commit.
pub fn files_menu(
    ui: &mut egui::Ui,
    repo: &std::path::Path,
    files: &[String],
    sha: Option<ferrum_core::Oid>,
    cmds: &mut Commands,
) {
    menu(ui, Target::files(repo, files, sha), cmds);
}

/// A file of a commit.
pub fn commit_file_menu(
    ui: &mut egui::Ui,
    repo: &std::path::Path,
    path: &String,
    id: ferrum_core::Oid,
    cmds: &mut Commands,
) {
    files_menu(ui, repo, std::slice::from_ref(path), Some(id), cmds);
}

pub fn submodule_menu(ui: &mut egui::Ui, tab: &RepoTab, path: &str, cmds: &mut Commands) {
    menu(ui, Target::submodule(tab, path), cmds);
}

fn tree(
    ui: &mut egui::Ui,
    items: &[(Vec<&str>, &CustomCommand, bool)],
    target: &Target,
    cmds: &mut Commands,
) {
    let mut folders: Vec<&str> = Vec::new();
    for (parts, cmd, shared) in items {
        match parts.as_slice() {
            [] => {}
            [leaf] => {
                let mut resp = ui.button(*leaf);
                if *shared {
                    resp = resp.on_hover_text(t!("Shared in {}", cc::SHARED_FILE));
                }
                if resp.clicked() {
                    cmds.push(Command::Custom(Invocation {
                        cmd: (*cmd).clone(),
                        shared: *shared,
                        target: target.clone(),
                    }));
                    ui.close();
                }
            }
            [folder, ..] if !folders.contains(folder) => {
                folders.push(folder);
                let inner: Vec<(Vec<&str>, &CustomCommand, bool)> = items
                    .iter()
                    .filter(|(p, _, _)| p.len() > 1 && p[0] == *folder)
                    .map(|(p, c, s)| (p[1..].to_vec(), *c, *s))
                    .collect();
                ui.menu_button(*folder, |ui| tree(ui, &inner, target, cmds));
            }
            _ => {}
        }
    }
}

/// Repository commands for the command palette.
pub fn palette_entries(_settings: &Settings, tab: Option<&RepoTab>, out: &mut Vec<Entry>) {
    let Some(tab) = tab else { return };
    for (cmd, shared) in cc::for_scope(tab.path(), Scope::Repository) {
        let inv = Invocation {
            cmd: cmd.clone(),
            shared,
            target: Target::repository(tab),
        };
        let label = cmd.name.replace('/', " › ");
        out.push(Entry::new(Group::Custom, label, "", move || {
            Command::Custom(inv.clone())
        }));
    }
}

// ----- Dialog -----

/// The form, confirmation or first-run approval before a command runs.
pub struct CustomDialog {
    inv: Invocation,
    vars: Vars,
    values: Vec<String>,
    /// Approval key, when this shared command was never allowed.
    approval: Option<String>,
}

impl CustomDialog {
    pub fn new(inv: Invocation, vars: Vars, approval: Option<String>) -> Self {
        let values = inv.cmd.form.iter().map(|f| f.default.clone()).collect();
        Self {
            inv,
            vars,
            values,
            approval,
        }
    }

    fn all_vars(&self) -> Vars {
        let mut v = cc::form_variables(&self.inv.cmd.form, &self.values);
        v.extend(self.vars.iter().cloned());
        v
    }
}

pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut CustomDialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let cmd = d.inv.cmd.clone();
    ui.heading(cmd.leaf());
    if d.approval.is_some() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!(
                "This command comes from {} in the repository. Run it only if you trust it:",
                cc::SHARED_FILE
            ),
        );
        ui.add_space(4.0);
        code_box(ui, cmd.text());
    }
    if !cmd.form.is_empty() {
        ui.add_space(6.0);
        egui::Grid::new("custom-form")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                for (i, f) in cmd.form.iter().enumerate() {
                    let label = if f.label.trim().is_empty() {
                        &f.name
                    } else {
                        &f.label
                    };
                    // A checkbox shows its label itself.
                    ui.label(if f.kind == FieldKind::Checkbox {
                        ""
                    } else {
                        label
                    });
                    form_field(ui, f, &mut d.values[i], i, tab, &d.inv.target);
                    ui.end_row();
                }
            });
    }
    ui.add_space(6.0);
    let vars = d.all_vars();
    let preview = if cmd.kind == Kind::Url {
        Ok(cc::expand(cmd.text().trim(), &vars, cc::Quote::Url))
    } else {
        cc::program(&cmd, &vars).map(|p| p.display())
    };
    // The approval box already shows a command without variables.
    let same = d.approval.is_some() && preview.as_deref().ok() == Some(cmd.text().trim());
    match &preview {
        Ok(_) if same => {}
        Ok(p) => {
            ui.weak(if cmd.kind == Kind::Url {
                t!("Opens:")
            } else {
                t!("Runs:")
            });
            code_box(ui, p);
        }
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }
    // Enter in a text field of the form only moves on.
    let enter = enter && cmd.form.is_empty();
    match super::dialogs::buttons(ui, &t!("Run"), preview.is_ok(), false, enter) {
        Some(true) => {
            if let Some(key) = d.approval.take() {
                settings.custom.approved.insert(key);
            }
            for c in cc::start(&cmd, &vars) {
                cmds.push(c);
            }
            false
        }
        Some(false) => false,
        None => true,
    }
}

fn code_box(ui: &mut egui::Ui, text: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .id_salt("custom-code")
            .max_height(160.0)
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).monospace()).wrap());
            });
    });
}

fn form_field(
    ui: &mut egui::Ui,
    f: &cc::FormField,
    value: &mut String,
    i: usize,
    tab: Option<&RepoTab>,
    target: &Target,
) {
    match f.kind {
        FieldKind::Text => {
            ui.add(egui::TextEdit::singleline(value).desired_width(280.0));
        }
        FieldKind::Branch => {
            let names: Vec<String> = tab
                .map(|t| {
                    let mut l: Vec<&ferrum_core::BranchInfo> = t.branches.iter().collect();
                    l.sort_by_key(|b| b.kind != BranchKind::Local);
                    l.into_iter().map(|b| b.name.clone()).collect()
                })
                .unwrap_or_default();
            super::filter_combo::filter_combo(
                ui,
                &format!("custom-branch-{i}"),
                value,
                &names,
                280.0,
                true,
            );
        }
        FieldKind::File => {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                if ui.button(t!("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .set_directory(&target.repo)
                        .pick_file()
                {
                    // Relative to the repository when inside it.
                    *value = match p.strip_prefix(&target.repo) {
                        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                        Err(_) => p.display().to_string(),
                    };
                }
            });
        }
        FieldKind::Checkbox => {
            let mut on = value == "true";
            let label = if f.label.trim().is_empty() {
                &f.name
            } else {
                &f.label
            };
            if ui.checkbox(&mut on, label.as_str()).changed() {
                *value = if on { "true".into() } else { String::new() };
            }
        }
        FieldKind::Choice => {
            let choices: Vec<&str> = f
                .choices
                .iter()
                .map(|c| c.trim())
                .filter(|c| !c.is_empty())
                .collect();
            if value.is_empty()
                && let Some(c) = choices.first()
            {
                *value = (*c).to_owned();
            }
            egui::ComboBox::from_id_salt(("custom-choice", i))
                .selected_text(value.as_str())
                .width(280.0)
                .show_ui(ui, |ui| {
                    for c in choices {
                        ui.selectable_value(value, c.to_owned(), c);
                    }
                });
        }
    }
}

// ----- Output window -----

thread_local! {
    /// The command whose output is shown: label and the first operation id it can have.
    static WATCH: RefCell<Option<(String, u64)>> = const { RefCell::new(None) };
}

/// Shows the output of the next operation named `label`.
pub fn watch_output(label: &str) {
    let next = crate::jobs::operations()
        .iter()
        .map(|o| o.id)
        .max()
        .map_or(0, |m| m + 1);
    WATCH.with(|w| *w.borrow_mut() = Some((label.to_owned(), next)));
}

/// The output window of a command run with "Show output".
pub fn output_window(ctx: &egui::Context) {
    let Some((label, first)) = WATCH.with(|w| w.borrow().clone()) else {
        return;
    };
    let op = crate::jobs::operations()
        .into_iter()
        .find(|o| o.id >= first && o.label == label);
    let mut open = true;
    egui::Window::new(t!("Output: {}", label))
        .id(egui::Id::unique("custom-command-output"))
        .open(&mut open)
        .resizable(true)
        .default_size([640.0, 320.0])
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| match &op {
            Some(op) => crate::ui::activity::details(ui, op),
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(t!("Waiting for the command to start…"));
                });
            }
        });
    if op.as_ref().is_none_or(|o| o.running()) {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    if !open {
        WATCH.with(|w| *w.borrow_mut() = None);
    }
}

// ----- Settings -----

/// Settings > Custom commands: the list (reorder, add, remove) and the editor.
pub fn settings_section(ui: &mut egui::Ui, settings: &mut Settings, cmds: &mut Commands) {
    ui.label(RichText::new(t!("Custom commands")).size(16.0).strong());
    ui.add_space(6.0);
    ui.weak(t!(
        "Shown in the context menus of their scope and in the command palette. Use / in the name for submenus."
    ));
    ui.weak(t!(
        "Variables: $SHA $SHORT_SHA $branch $file $path (repository) $remote $submodule $selected_shas, and the names of form fields."
    ));
    ui.add_space(6.0);
    let id = ui.make_persistent_id("custom-commands-selected");
    let mut selected: Option<usize> = ui.data(|d| d.get_temp(id)).flatten();
    let list = &mut settings.custom.commands;
    selected = selected.filter(|&i| i < list.len());

    ui.horizontal(|ui| {
        if ui.button(t!("✚ Add")).clicked() {
            list.push(CustomCommand::new(&t!("New command"), Scope::Repository));
            selected = Some(list.len() - 1);
        }
        let Some(i) = selected else { return };
        if ui.button(t!("Duplicate")).clicked() {
            let copy = list[i].clone();
            list.insert(i + 1, copy);
            selected = Some(i + 1);
        }
        if ui
            .add_enabled(i > 0, egui::Button::new("⬆"))
            .on_hover_text(t!("Move up"))
            .clicked()
        {
            list.swap(i, i - 1);
            selected = Some(i - 1);
        }
        if ui
            .add_enabled(i + 1 < list.len(), egui::Button::new("⬇"))
            .on_hover_text(t!("Move down"))
            .clicked()
        {
            list.swap(i, i + 1);
            selected = Some(i + 1);
        }
        if ui
            .button(t!("Copy as JSON"))
            .on_hover_text(t!("For {} in a repository", cc::SHARED_FILE))
            .clicked()
        {
            cmds.push(Command::Copy(cc::to_json(&list[i..=i])));
        }
        if ui.button(t!("Delete")).clicked() {
            list.remove(i);
            selected = None;
        }
    });
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .id_salt("custom-commands-list")
            .max_height(140.0)
            .show(ui, |ui| {
                if list.is_empty() {
                    ui.weak(t!("No custom commands yet."));
                }
                for (i, c) in list.iter().enumerate() {
                    let text = format!("{}   ({})", c.name, c.scope.label());
                    if ui.selectable_label(selected == Some(i), text).clicked() {
                        selected = Some(i);
                    }
                }
            });
    });
    if let Some(i) = selected {
        ui.add_space(6.0);
        editor(ui, &mut list[i], i);
    }
    ui.data_mut(|d| d.insert_temp(id, selected));
    ui.add_space(8.0);
    ui.weak(t!(
        "To share commands with your team, put them in {} in the repository (the format of Copy as JSON). Each one asks for approval before its first run.",
        cc::SHARED_FILE
    ));
}

fn editor(ui: &mut egui::Ui, c: &mut CustomCommand, i: usize) {
    egui::Grid::new(("custom-command-editor", i))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t!("Name"));
            ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(320.0));
            ui.end_row();
            ui.label(t!("Shown for"));
            egui::ComboBox::from_id_salt(("custom-scope", i))
                .selected_text(c.scope.label())
                .show_ui(ui, |ui| {
                    for s in Scope::ALL {
                        ui.selectable_value(&mut c.scope, s, s.label());
                    }
                });
            ui.end_row();
            ui.label(t!("Type"));
            egui::ComboBox::from_id_salt(("custom-kind", i))
                .selected_text(c.kind.label())
                .show_ui(ui, |ui| {
                    for k in Kind::ALL {
                        ui.selectable_value(&mut c.kind, k, k.label());
                    }
                });
            ui.end_row();
            ui.label(t!("Command"));
            ui.add(
                egui::TextEdit::multiline(&mut c.command)
                    .code_editor()
                    .desired_rows(3)
                    .desired_width(320.0),
            );
            ui.end_row();
        });
    egui::CollapsingHeader::new(t!("Per-system command"))
        .id_salt(("custom-os", i))
        .show(ui, |ui| {
            ui.weak(t!(
                "Used instead of the command above on that system when not empty."
            ));
            egui::Grid::new(("custom-os-grid", i))
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (name, field) in [
                        ("Linux", &mut c.linux),
                        ("macOS", &mut c.macos),
                        ("Windows", &mut c.windows),
                    ] {
                        ui.label(name);
                        let mut text = field.clone().unwrap_or_default();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .code_editor()
                                .desired_width(300.0),
                        );
                        if resp.changed() {
                            *field = Some(text).filter(|t| !t.is_empty());
                        }
                        ui.end_row();
                    }
                });
        });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut c.confirm, t!("Confirm before running"));
        ui.checkbox(&mut c.show_output, t!("Show output"));
        ui.checkbox(&mut c.refresh, t!("Refresh after"));
    });
    egui::CollapsingHeader::new(t!("Form ({} fields)", c.form.len()))
        .id_salt(("custom-form-fields", i))
        .show(ui, |ui| {
            ui.weak(t!("Asked before running; each value becomes $name."));
            let mut remove = None;
            for (j, f) in c.form.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut f.name)
                            .hint_text(t!("name"))
                            .desired_width(80.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut f.label)
                            .hint_text(t!("label"))
                            .desired_width(110.0),
                    );
                    egui::ComboBox::from_id_salt(("custom-field-kind", i, j))
                        .selected_text(f.kind.label())
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for k in FieldKind::ALL {
                                ui.selectable_value(&mut f.kind, k, k.label());
                            }
                        });
                    if ui.small_button("×").on_hover_text(t!("Remove")).clicked() {
                        remove = Some(j);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    match f.kind {
                        FieldKind::Checkbox => {
                            ui.add(
                                egui::TextEdit::singleline(&mut f.on)
                                    .hint_text(t!("value when checked"))
                                    .desired_width(130.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut f.off)
                                    .hint_text(t!("value when not"))
                                    .desired_width(130.0),
                            );
                        }
                        FieldKind::Choice => {
                            let mut text = f.choices.join(",");
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut text)
                                    .hint_text(t!("choices, separated by commas"))
                                    .desired_width(270.0),
                            );
                            if resp.changed() {
                                f.choices = text.split(',').map(str::to_owned).collect();
                            }
                        }
                        _ => {
                            ui.add(
                                egui::TextEdit::singleline(&mut f.default)
                                    .hint_text(t!("default value"))
                                    .desired_width(270.0),
                            );
                        }
                    }
                });
            }
            if let Some(j) = remove {
                c.form.remove(j);
            }
            if ui.button(t!("✚ Add field")).clicked() {
                c.form.push(cc::FormField {
                    name: format!("value{}", c.form.len() + 1),
                    ..Default::default()
                });
            }
        });
}
