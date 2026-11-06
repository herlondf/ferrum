//! The Issues view next to Pull Requests: list with filters (state, author,
//! assignee, label, words), recent searches, and a simple "new issue" form.

use eframe::egui::{self, RichText};
use ferrum_core::forge::{self, HostedRepo, Issue, IssueFilter, IssueState as Which, NewIssue};

use crate::command::{Command, Commands};
use crate::hosted::{Pending, take_done};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::util::{now_secs, relative_time};

#[derive(Default)]
pub struct IssueState {
    pub list: Option<Vec<Issue>>,
    pub error: Option<String>,
    loading: Option<Pending<Vec<Issue>>>,
    /// The filter being edited, and the one the list was loaded with.
    pub filter: IssueFilter,
    loaded_with: Option<(HostedRepo, IssueFilter)>,
    new: Option<NewIssueForm>,
}

struct NewIssueForm {
    title: String,
    body: String,
    sending: Option<Pending<Issue>>,
    error: Option<String>,
}

impl IssueState {
    pub fn reset(&mut self) {
        *self = IssueState {
            filter: std::mem::take(&mut self.filter),
            ..Default::default()
        };
    }

    pub fn loading(&self) -> bool {
        self.loading.is_some()
    }
}

/// "Open website" in the Repository menu.
pub fn website_button(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    if let Some(h) = &tab.hosted
        && ui
            .button(t!("Open website"))
            .on_hover_text(h.web_url())
            .clicked()
    {
        cmds.push(Command::OpenUrl(h.web_url()));
        ui.close();
    }
}

fn load(ctx: &egui::Context, tab: &mut RepoTab, settings: &mut Settings) {
    let Some(hosted) = tab.hosted.clone() else {
        return;
    };
    let st = &mut tab.issues;
    let filter = st.filter.clone();
    st.loaded_with = Some((hosted.clone(), filter.clone()));
    let Some(creds) = crate::hosted::credentials(settings, &hosted) else {
        st.error = Some(t!("Log in to {} to see issues.", hosted.host));
        return;
    };
    settings.forge.remember_search(&filter);
    st.error = None;
    st.loading = Some(Pending::spawn(ctx, move || {
        forge::list_issues(&hosted, &creds, &filter).map_err(|e| e.to_string())
    }));
}

fn state_label(s: Which) -> String {
    match s {
        Which::Open => t!("Open issues"),
        Which::Closed => t!("Closed"),
        Which::All => t!("All"),
    }
}

/// Short text for a saved search.
fn describe(f: &IssueFilter) -> String {
    let mut parts = vec![state_label(f.state)];
    for (label, v) in [
        (t!("author"), &f.author),
        (t!("assignee"), &f.assignee),
        (t!("label"), &f.label),
    ] {
        if !v.trim().is_empty() {
            parts.push(format!("{label}:{}", v.trim()));
        }
    }
    if !f.text.trim().is_empty() {
        parts.push(format!("\"{}\"", f.text.trim()));
    }
    parts.join("  ")
}

pub fn show_view(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
) {
    let ctx = ui.ctx().clone();
    let Some(hosted) = tab.hosted.clone() else {
        ui.centered_and_justified(|ui| {
            ui.weak(t!(
                "This repository has no remote on a known hosting service."
            ));
        });
        return;
    };
    // Poll the requests.
    if let Some(r) = take_done(&mut tab.issues.loading) {
        match r {
            Ok(list) => {
                tab.issues.list = Some(list);
                tab.issues.error = None;
            }
            Err(e) => tab.issues.error = Some(e),
        }
    }
    let stale = tab
        .issues
        .loaded_with
        .as_ref()
        .is_none_or(|(h, _)| *h != hosted);
    if stale && !tab.issues.loading() && hosted.kind.has_issues() {
        tab.issues.list = None;
        load(&ctx, tab, settings);
    }

    let mut reload = false;
    ui.horizontal(|ui| {
        ui.heading(t!("Issues"));
        if let Some(list) = &tab.issues.list {
            ui.weak(format!("({})", list.len()));
        }
        ui.add_space(6.0);
        if ui
            .link(RichText::new(format!("{}/{}", hosted.host, hosted.path)).weak())
            .clicked()
        {
            cmds.push(Command::OpenUrl(hosted.web_url()));
        }
        crate::hosted::remote_picker(ui, tab, cmds);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = tab.issues.loading();
            reload |= ui
                .add_enabled(!busy, egui::Button::new(t!("⟳ Refresh")))
                .clicked();
            if hosted.kind.has_issues() && ui.button(t!("New issue…")).clicked() {
                tab.issues.new = Some(NewIssueForm {
                    title: String::new(),
                    body: String::new(),
                    sending: None,
                    error: None,
                });
            }
            if let Some(url) = hosted.issues_url()
                && ui.button(t!("Open on {}", hosted.kind.name())).clicked()
            {
                cmds.push(Command::OpenUrl(url));
            }
            if busy {
                ui.spinner();
            }
        });
    });
    ui.separator();
    if !hosted.kind.has_issues() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.weak(t!("{} has no issues Ferrum can list.", hosted.kind.name()));
        });
        return;
    }

    reload |= filter_bar(ui, tab, settings);
    new_issue_form(ui, &ctx, tab, settings);
    ui.separator();
    if reload {
        load(&ctx, tab, settings);
    }

    if let Some(err) = tab.issues.error.clone() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {err}"));
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - 170.0) / 2.0).max(0.0));
                if ui.button(t!("Log in…")).clicked() {
                    let d = crate::ui::forge::login(hosted.kind, &hosted.host);
                    cmds.push(Command::Dialog(Dialog::Forge(d)));
                }
                if ui.button(t!("Try again")).clicked() {
                    load(&ctx, tab, settings);
                }
            });
        });
        return;
    }
    let Some(list) = &tab.issues.list else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.weak(t!("Loading issues…"));
        });
        return;
    };
    if list.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("No issues match")).size(16.0));
        });
        return;
    }
    let now = now_secs();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for issue in list {
                issue_row(ui, issue, now, cmds);
            }
        });
}

/// Returns true when a search should run.
fn filter_bar(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings) -> bool {
    let f = &mut tab.issues.filter;
    let mut go = false;
    ui.horizontal_wrapped(|ui| {
        let before = f.state;
        egui::ComboBox::from_id_salt("issue-state")
            .selected_text(state_label(f.state))
            .width(90.0)
            .show_ui(ui, |ui| {
                for s in [Which::Open, Which::Closed, Which::All] {
                    ui.selectable_value(&mut f.state, s, state_label(s));
                }
            });
        go |= f.state != before;
        let text = |ui: &mut egui::Ui, value: &mut String, hint: String, w: f32| {
            let r = ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(hint)
                    .desired_width(w),
            );
            r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        };
        go |= text(ui, &mut f.text, t!("Search titles"), 180.0);
        go |= text(ui, &mut f.author, t!("Author"), 110.0);
        go |= text(ui, &mut f.assignee, t!("Assignee"), 110.0);
        go |= text(ui, &mut f.label, t!("Label"), 100.0);
        go |= ui.button(t!("Search")).clicked();
        if !f.is_default() && ui.button(t!("Clear")).clicked() {
            *f = IssueFilter::default();
            go = true;
        }
        let recent = settings.forge.recent_issue_searches.clone();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button(t!("Recent ⏷"), |ui| {
                for r in &recent {
                    if ui.button(describe(r)).clicked() {
                        *f = r.clone();
                        go = true;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t!("Clear recent searches")).clicked() {
                    settings.forge.recent_issue_searches.clear();
                    ui.close();
                }
            });
        });
    });
    go
}

fn new_issue_form(ui: &mut egui::Ui, ctx: &egui::Context, tab: &mut RepoTab, settings: &Settings) {
    let Some(hosted) = tab.hosted.clone() else {
        return;
    };
    let Some(form) = tab.issues.new.as_mut() else {
        return;
    };
    if let Some(r) = take_done(&mut form.sending) {
        match r {
            Ok(issue) => {
                let msg = t!("Created #{} {}", issue.number, issue.title);
                if let Some(list) = &mut tab.issues.list {
                    list.insert(0, issue);
                }
                tab.issues.new = None;
                tab.info(msg);
                return;
            }
            Err(e) => form.error = Some(e),
        }
    }
    let mut close = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.strong(t!("New issue"));
        ui.add(
            egui::TextEdit::singleline(&mut form.title)
                .hint_text(t!("Issue title"))
                .desired_width(f32::INFINITY),
        );
        ui.add(
            egui::TextEdit::multiline(&mut form.body)
                .hint_text(t!("Describe the problem or idea (Markdown)"))
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        if let Some(e) = &form.error {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
        }
        ui.horizontal(|ui| {
            let sending = form.sending.is_some();
            let ready = !form.title.trim().is_empty() && !sending;
            if ui
                .add_enabled(ready, egui::Button::new(t!("Create issue")))
                .clicked()
            {
                match crate::hosted::credentials(settings, &hosted) {
                    Some(creds) => {
                        let new = NewIssue {
                            title: form.title.trim().to_owned(),
                            body: form.body.clone(),
                        };
                        let h = hosted.clone();
                        form.error = None;
                        form.sending = Some(Pending::spawn(ctx, move || {
                            forge::create_issue(&h, &creds, &new).map_err(|e| e.to_string())
                        }));
                    }
                    None => form.error = Some(t!("Log in to {} to see issues.", hosted.host)),
                }
            }
            close = ui.button(t!("Cancel")).clicked();
            if sending {
                ui.spinner();
            }
        });
    });
    if close {
        tab.issues.new = None;
    }
}

fn issue_row(ui: &mut egui::Ui, issue: &Issue, now: i64, cmds: &mut Commands) {
    let resp = ui
        .horizontal(|ui| {
            ui.set_min_height(40.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (color, mark) = if issue.open {
                        (egui::Color32::from_rgb(60, 170, 90), "○")
                    } else {
                        (ui.visuals().weak_text_color(), "✔")
                    };
                    ui.colored_label(color, mark);
                    ui.label(
                        RichText::new(format!("#{}", issue.number))
                            .monospace()
                            .weak(),
                    );
                    ui.label(RichText::new(&issue.title).strong());
                    for l in &issue.labels {
                        ui.label(RichText::new(l).small().weak())
                            .on_hover_text(t!("Label"));
                    }
                });
                let when = crate::ui::forge::parse_timestamp(&issue.updated_at)
                    .map_or_else(|| issue.updated_at.clone(), |t| relative_time(t, now));
                let mut line = t!("by {}, updated {}", issue.author, when);
                if !issue.assignees.is_empty() {
                    line.push_str(&format!(
                        "  ·  {}",
                        t!("assigned to {}", issue.assignees.join(", "))
                    ));
                }
                if issue.comments > 0 {
                    line.push_str(&format!("  ·  {}", t!("{} comments", issue.comments)));
                }
                ui.weak(line);
            });
        })
        .response
        .interact(egui::Sense::click());
    ui.separator();
    if resp.clicked() {
        cmds.push(Command::OpenUrl(issue.url.clone()));
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    resp.context_menu(|ui| {
        if ui.button(t!("Open in browser")).clicked() {
            cmds.push(Command::OpenUrl(issue.url.clone()));
            ui.close();
        }
        if ui.button(t!("Copy URL")).clicked() {
            cmds.push(Command::Copy(issue.url.clone()));
            ui.close();
        }
        if ui.button(t!("Copy number")).clicked() {
            cmds.push(Command::Copy(format!("#{}", issue.number)));
            ui.close();
        }
    });
}

/// Opens the "new issue" form.
pub fn open_new(tab: &mut RepoTab) {
    tab.issues.new = Some(NewIssueForm {
        title: String::new(),
        body: String::new(),
        sending: None,
        error: None,
    });
}
