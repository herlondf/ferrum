//! Pull requests on GitHub and merge requests on GitLab: the list view, the
//! "new pull request" dialog and the login dialog.
//!
//! Creating a request goes through `Command::CreatePullRequest`, which the app
//! turns into `Op::CreatePullRequest`. Being an `Op`, it waits in the tab's
//! queue behind an `Op::Push` queued just before it, so "push, then create"
//! runs in order and a failed push cancels the creation.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align2, FontId, RichText, Sense, vec2};
use ferrum_core::forge::{self, ForgeKind, HostedRepo, NewPullRequest, PullRequest};
use ferrum_core::ops::PushOptions;
use ferrum_core::{BranchInfo, BranchKind};

use crate::command::{Command, Commands, Op};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::dialogs::Dialog;
use crate::util::format_time;

/// Label of the job that loads the list (set by the app).
const LOAD_JOB: &str = "Load pull requests";
const ROW_HEIGHT: f32 = 46.0;

#[derive(Default)]
pub struct PrState {
    pub list: Option<Vec<PullRequest>>,
    pub error: Option<String>,
}

pub enum ForgeDialog {
    NewPullRequest(NewPrForm),
    Login(LoginForm),
}

pub struct NewPrForm {
    hosted: Option<HostedRepo>,
    remote: String,
    title: String,
    /// The title filled in automatically, replaced when the source changes.
    auto_title: String,
    body: String,
    source: String,
    target: String,
    locals: Vec<String>,
    targets: Vec<String>,
    draft: bool,
    push_first: bool,
    /// Whether a token is available; checked once, as it may run `gh`.
    logged_in: Option<bool>,
    /// When "Create" was pressed; the dialog waits for the jobs to finish.
    submitted: Option<Instant>,
    error: Option<String>,
}

pub struct LoginForm {
    kind: ForgeKind,
    host: String,
    token: String,
    /// GitHub CLI token for `.0`, looked up again when the host changes.
    gh: Option<(String, Option<String>)>,
}

pub fn new_pull_request(tab: &RepoTab, branch: &str) -> ForgeDialog {
    let remote = hosted_remote(tab);
    let targets: Vec<String> = tab
        .branches
        .iter()
        .filter_map(BranchInfo::split_remote)
        .filter(|(r, _)| *r == remote)
        .map(|(_, n)| n.to_owned())
        .collect();
    let remote_head = tab
        .git
        .run(&[
            "symbolic-ref",
            "--quiet",
            "--short",
            &format!("refs/remotes/{remote}/HEAD"),
        ])
        .ok();
    let remote_head = remote_head
        .as_deref()
        .map(str::trim)
        .and_then(|h| h.strip_prefix(&format!("{remote}/")));
    let target = default_target(&targets, remote_head, branch).unwrap_or_default();
    let locals = tab
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local)
        .map(|b| b.name.clone())
        .collect();
    let title = branch_summary(tab, branch);
    ForgeDialog::NewPullRequest(NewPrForm {
        hosted: tab.hosted.clone(),
        remote,
        title: title.clone(),
        auto_title: title,
        body: String::new(),
        source: branch.into(),
        target,
        locals,
        targets,
        draft: false,
        push_first: true,
        logged_in: None,
        submitted: None,
        error: None,
    })
}

pub fn login(kind: ForgeKind, host: &str) -> ForgeDialog {
    ForgeDialog::Login(LoginForm {
        kind,
        host: host.into(),
        token: String::new(),
        gh: None,
    })
}

/// The remote that points at the hosted repository (usually `origin`).
fn hosted_remote(tab: &RepoTab) -> String {
    tab.remotes
        .iter()
        .find(|r| tab.hosted.is_some() && forge::parse_remote_url(&r.url) == tab.hosted)
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "origin".into())
}

/// The summary of the last commit on `branch`, used as the default title.
fn branch_summary(tab: &RepoTab, branch: &str) -> String {
    tab.repo
        .resolve(branch)
        .and_then(|id| tab.repo.find_commit_info(id))
        .map(|c| c.summary)
        .unwrap_or_default()
}

/// Target branch to suggest: the remote's default branch, else main or master.
fn default_target(targets: &[String], remote_head: Option<&str>, source: &str) -> Option<String> {
    remote_head
        .into_iter()
        .chain(["main", "master", "develop"])
        .find(|t| *t != source && targets.iter().any(|b| b == t))
        .or_else(|| targets.iter().map(String::as_str).find(|t| *t != source))
        .map(str::to_owned)
}

/// Why `branch` has to be pushed before a request can be opened, if it does.
fn push_reason(tab: &RepoTab, branch: &str) -> Option<String> {
    let b = tab
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == branch)?;
    match (&b.upstream, b.ahead_behind) {
        (None, _) => Some(t!("{} has not been pushed yet.", branch)),
        (Some(u), Some((1, _))) => Some(t!("{} has 1 commit not pushed to {}.", branch, u)),
        (Some(u), Some((ahead, _))) if ahead > 0 => {
            Some(t!("{} has {} commits not pushed to {}.", branch, ahead, u))
        }
        _ => None,
    }
}

/// How to check out the source branch of a request, if it has been fetched.
fn checkout_command(tab: &RepoTab, remote: &str, source: &str) -> Option<Command> {
    let remote_branch = format!("{remote}/{source}");
    if !tab
        .branches
        .iter()
        .any(|b| b.kind == BranchKind::Remote && b.name == remote_branch)
    {
        return None;
    }
    let tracking = tab.branches.iter().find(|b| {
        b.kind == BranchKind::Local && b.upstream.as_deref() == Some(remote_branch.as_str())
    });
    Some(match tracking {
        Some(local) => Command::Run(Op::Checkout(local.name.clone())),
        None => Command::Dialog(Dialog::checkout_remote(&remote_branch, source)),
    })
}

/// Whether `e` is the app's "log in first" message (set in app.rs, maybe translated).
fn needs_login(e: &str) -> bool {
    const KEY: &str = "Log in to {} to see {}s.";
    e.starts_with("Log in")
        || [KEY, crate::i18n::tr(KEY)].iter().any(|k| {
            k.split("{}")
                .next()
                .is_some_and(|p| !p.is_empty() && e.starts_with(p))
        })
}

fn is_auth_error(e: &str) -> bool {
    needs_login(e) || e.contains("HTTP 401")
}

fn token_url(kind: ForgeKind, host: &str) -> String {
    match kind {
        ForgeKind::GitHub => {
            format!("https://{host}/settings/tokens/new?scopes=repo&description=Ferrum")
        }
        ForgeKind::GitLab => {
            format!("https://{host}/-/user_settings/personal_access_tokens?name=Ferrum&scopes=api")
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

// ----- Time -----

/// Parses `2026-11-01T10:00:00Z`, `…00.123Z` or `…00+02:00` into Unix seconds.
fn parse_timestamp(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Skip fractional seconds, then read the zone.
    let zone = s
        .get(19..)?
        .trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let offset = match zone {
        "" | "Z" | "z" => 0,
        z => {
            let sign = match z.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hh = z.get(1..3)?.parse::<i64>().ok()?;
            let mm = z.get(z.len() - 2..)?.parse::<i64>().ok()?;
            sign * (hh * 3600 + mm * 60)
        }
    };
    // Days-from-civil (Howard Hinnant).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

/// "5 minutes ago", "yesterday", or a date for anything older than a month.
fn relative_time(secs: i64, now: i64) -> String {
    let ago = (now - secs).max(0);
    match ago {
        0..60 => t!("just now"),
        60..120 => t!("1 minute ago"),
        120..3600 => t!("{} minutes ago", ago / 60),
        3600..7200 => t!("1 hour ago"),
        7200..86_400 => t!("{} hours ago", ago / 3600),
        86_400..172_800 => t!("yesterday"),
        172_800..2_592_000 => t!("{} days ago", ago / 86_400),
        _ => format_time(secs, 0)[..10].to_owned(),
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

// ----- View -----

/// The pull requests view (`View::PullRequests`).
pub fn show_view(ui: &mut egui::Ui, tab: &mut RepoTab, _settings: &Settings, cmds: &mut Commands) {
    let Some(hosted) = tab.hosted.clone() else {
        ui.centered_and_justified(|ui| {
            ui.weak(t!("This repository has no GitHub or GitLab remote."));
        });
        return;
    };
    let noun = hosted.kind.request_name();
    let loading = tab
        .job
        .as_ref()
        .is_some_and(|j| j.label == LOAD_JOB || j.label == crate::i18n::tr(LOAD_JOB));

    ui.horizontal(|ui| {
        ui.heading(format!("{}s", capitalize(noun)));
        if let Some(list) = &tab.prs.list
            && tab.prs.error.is_none()
        {
            ui.weak(format!("({})", list.len()));
        }
        ui.add_space(6.0);
        if ui
            .link(RichText::new(format!("{}/{}", hosted.host, hosted.path)).weak())
            .on_hover_text(t!("Open the repository in the browser"))
            .clicked()
        {
            cmds.push(Command::OpenUrl(hosted.web_url()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!loading, egui::Button::new(t!("⟳ Refresh")))
                .clicked()
            {
                cmds.push(Command::LoadPullRequests);
            }
            if let Some(head) = tab.head.clone()
                && ui.button(t!("New {}…", noun)).clicked()
            {
                cmds.push(Command::Dialog(Dialog::Forge(new_pull_request(tab, &head))));
            }
            if ui.button(t!("Open on {}", hosted.kind.name())).clicked() {
                cmds.push(Command::OpenUrl(hosted.web_url()));
            }
            if loading {
                ui.spinner();
            }
        });
    });
    ui.separator();

    if let Some(err) = tab.prs.error.clone() {
        if is_auth_error(&err) {
            not_logged_in(ui, &hosted, &err, cmds);
        } else {
            message(ui, |ui| {
                ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {err}"));
                if ui.button(t!("Try again")).clicked() {
                    cmds.push(Command::LoadPullRequests);
                }
            });
        }
        return;
    }
    let Some(list) = tab.prs.list.as_ref() else {
        message(ui, |ui| {
            if loading {
                ui.spinner();
                ui.weak(t!("Loading {}s…", noun));
            } else {
                ui.weak(t!("{}s are not loaded.", capitalize(noun)));
                if ui.button(t!("Load")).clicked() {
                    cmds.push(Command::LoadPullRequests);
                }
            }
        });
        return;
    };
    if list.is_empty() {
        message(ui, |ui| {
            ui.label(RichText::new(t!("No open {}s", noun)).size(16.0));
            let (nouns, forge) = (capitalize(noun), hosted.kind.name());
            ui.weak(t!("{}s opened on {} will show up here.", nouns, forge));
        });
        return;
    }

    let remote = hosted_remote(tab);
    let now = now_secs();
    let tab = &*tab;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for pr in list {
                let resp = pr_row(ui, pr, now, &remote, tab);
                if resp.clicked() {
                    cmds.push(Command::OpenUrl(pr.url.clone()));
                }
                resp.context_menu(|ui| pr_menu(ui, pr, &remote, tab, cmds));
            }
        });
}

/// A centered block for empty, loading and error states.
fn message(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        add(ui);
    });
}

fn not_logged_in(ui: &mut egui::Ui, hosted: &HostedRepo, err: &str, cmds: &mut Commands) {
    message(ui, |ui| {
        ui.label(RichText::new(t!("Log in to {}", hosted.kind.name())).size(16.0));
        if needs_login(err) {
            let template = t!("Ferrum needs a {} token for {} to list and create {}s.");
            let (forge, host) = (hosted.kind.name(), &hosted.host);
            let noun = hosted.kind.request_name();
            ui.label(crate::i18n::fill(&template, &[&forge, host, &noun]));
        } else {
            ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {err}"));
        }
        if hosted.kind == ForgeKind::GitHub {
            let note = t!("Logged in to the GitHub CLI (gh auth login)? Ferrum uses it.");
            ui.weak(note);
        }
        ui.horizontal(|ui| {
            // Center the two buttons.
            let w = 170.0;
            ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
            if ui.button(t!("Log in…")).clicked() {
                cmds.push(Command::Dialog(Dialog::Forge(login(
                    hosted.kind,
                    &hosted.host,
                ))));
            }
            if ui.button(t!("Try again")).clicked() {
                cmds.push(Command::LoadPullRequests);
            }
        });
    });
}

fn pr_row(
    ui: &mut egui::Ui,
    pr: &PullRequest,
    now: i64,
    remote: &str,
    tab: &RepoTab,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    if resp.hovered() || resp.context_menu_opened() {
        painter.rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let weak = visuals.weak_text_color();
    let text = visuals.strong_text_color();
    let left = rect.left() + 10.0;
    let top = rect.top() + 14.0;
    let bottom = rect.top() + 33.0;

    // Right column: last update.
    let updated = parse_timestamp(&pr.updated_at);
    let when = updated.map_or_else(|| pr.updated_at.clone(), |t| relative_time(t, now));
    let when_rect = painter.text(
        rect.right_top() + vec2(-10.0, 14.0),
        Align2::RIGHT_CENTER,
        t!("updated {}", when),
        FontId::proportional(12.0),
        weak,
    );

    // First line: number, title and draft badge.
    let num = painter.text(
        egui::pos2(left, top),
        Align2::LEFT_CENTER,
        format!("#{}", pr.number),
        FontId::monospace(13.0),
        weak,
    );
    let mut x = num.right() + 8.0;
    if pr.draft {
        let badge = painter.layout_no_wrap(t!("Draft"), FontId::proportional(11.0), weak);
        let r = egui::Rect::from_min_size(
            egui::pos2(x, top - badge.size().y / 2.0 - 2.0),
            badge.size() + vec2(12.0, 4.0),
        );
        painter.rect_stroke(
            r,
            8.0,
            egui::Stroke::new(1.0, weak),
            egui::StrokeKind::Inside,
        );
        painter.galley(r.min + vec2(6.0, 2.0), badge, weak);
        x = r.right() + 8.0;
    }
    let max_title = (when_rect.left() - 16.0 - x).max(40.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        pr.title.clone(),
        FontId::proportional(14.0),
        text,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_title);
    let title = painter.layout_job(job);
    painter.galley(egui::pos2(x, top - title.size().y / 2.0), title, text);

    // Second line: author and branches.
    let fetched = tab.branches.iter().any(|b| {
        b.kind == BranchKind::Remote && b.name == format!("{remote}/{}", pr.source_branch)
    });
    let branches = format!(
        "{}  ·  {} -> {}",
        pr.author, pr.source_branch, pr.target_branch
    );
    painter.text(
        egui::pos2(num.left(), bottom),
        Align2::LEFT_CENTER,
        branches,
        FontId::proportional(12.0),
        weak,
    );

    painter.hline(
        rect.x_range().shrink(4.0),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
    );
    let hover = match updated {
        Some(t) => t!("{}\nUpdated {} UTC", pr.url, format_time(t, 0)),
        None => pr.url.clone(),
    };
    let hover = if fetched {
        hover
    } else {
        let missing = t!("{}/{} is not fetched", remote, pr.source_branch);
        format!("{hover}\n{missing}")
    };
    resp.on_hover_text(hover)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn pr_menu(ui: &mut egui::Ui, pr: &PullRequest, remote: &str, tab: &RepoTab, cmds: &mut Commands) {
    if ui.button(t!("Open in browser")).clicked() {
        cmds.push(Command::OpenUrl(pr.url.clone()));
        ui.close();
    }
    let checkout = checkout_command(tab, remote, &pr.source_branch);
    let label = t!("Check out {}", pr.source_branch);
    let resp = ui.add_enabled(checkout.is_some(), egui::Button::new(label));
    if resp.clicked()
        && let Some(c) = checkout
    {
        cmds.push(c);
        ui.close();
    }
    let tip = t!("{}/{} does not exist. Fetch first, or the branch lives in a fork");
    let tip = crate::i18n::fill(&tip, &[&remote, &pr.source_branch]);
    resp.on_disabled_hover_text(tip);
    ui.separator();
    if ui.button(t!("Copy URL")).clicked() {
        cmds.push(Command::Copy(pr.url.clone()));
        ui.close();
    }
    if ui.button(t!("Copy branch name")).clicked() {
        cmds.push(Command::Copy(pr.source_branch.clone()));
        ui.close();
    }
}

// ----- Dialogs -----

/// Body of a forge dialog inside the modal. Returns `false` to close it.
pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut ForgeDialog,
    tab: Option<&RepoTab>,
    settings: &mut Settings,
    cmds: &mut Commands,
) -> bool {
    match d {
        ForgeDialog::NewPullRequest(f) => match tab {
            Some(tab) => {
                let mut switch_to = None;
                let keep = new_pr_dialog(ui, f, tab, settings, cmds, &mut switch_to);
                if let Some(next) = switch_to {
                    *d = next;
                    return true;
                }
                keep
            }
            None => false,
        },
        ForgeDialog::Login(f) => login_dialog(ui, f, cmds),
    }
}

fn new_pr_dialog(
    ui: &mut egui::Ui,
    f: &mut NewPrForm,
    tab: &RepoTab,
    settings: &Settings,
    cmds: &mut Commands,
    switch_to: &mut Option<ForgeDialog>,
) -> bool {
    let Some(hosted) = f.hosted.clone() else {
        ui.heading(t!("New pull request"));
        ui.label(t!("This repository has no GitHub or GitLab remote."));
        return !ui.button(t!("Close")).clicked();
    };
    let noun = hosted.kind.request_name();
    ui.heading(t!("New {}", noun));
    ui.weak(format!("{}/{}", hosted.host, hosted.path));
    ui.add_space(4.0);

    // Waiting for the push and the request to finish.
    if let Some(at) = f.submitted {
        if tab.busy() || !tab.queue.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                let label = tab.job.as_ref().map_or(String::new(), |j| j.label.clone());
                ui.label(format!("{label}…"));
            });
            ui.add_space(8.0);
            return !ui
                .button(t!("Hide"))
                .on_hover_text(t!("Keep working; the result shows in the status bar"))
                .clicked();
        }
        f.submitted = None;
        match tab.message.as_ref().filter(|m| m.error && m.at >= at) {
            Some(m) => f.error = Some(m.text.clone()),
            None => return false,
        }
    }

    let logged_in = *f
        .logged_in
        .get_or_insert_with(|| settings.token_for(hosted.kind, &hosted.host).is_some());
    if !logged_in {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("Log in to {} to create {}s from Ferrum.", hosted.host, noun),
        );
        ui.horizontal(|ui| {
            if ui.button(t!("Log in…")).clicked() {
                *switch_to = Some(login(hosted.kind, &hosted.host));
            }
            if ui.button(t!("Open in browser instead")).clicked() {
                cmds.push(Command::OpenUrl(
                    hosted.new_request_url(&f.source, &f.target),
                ));
            }
        });
        ui.add_space(4.0);
    }

    ui.label(t!("Title"));
    ui.add(
        egui::TextEdit::singleline(&mut f.title)
            .hint_text(t!("A short summary"))
            .desired_width(f32::INFINITY),
    );
    ui.label(t!("Description"));
    ui.add(
        egui::TextEdit::multiline(&mut f.body)
            .hint_text(t!("What changes and why (Markdown)"))
            .desired_rows(6)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    egui::Grid::new("pr-branches")
        .num_columns(2)
        .spacing([8.0, 6.0])
        .show(ui, |ui| {
            ui.label(t!("From"));
            let before = f.source.clone();
            egui::ComboBox::from_id_salt("pr-source")
                .selected_text(&f.source)
                .width(300.0)
                .show_ui(ui, |ui| {
                    for b in &f.locals {
                        ui.selectable_value(&mut f.source, b.clone(), b);
                    }
                });
            if f.source != before && f.title == f.auto_title {
                f.auto_title = branch_summary(tab, &f.source);
                f.title = f.auto_title.clone();
            }
            ui.end_row();
            ui.label(t!("Into"));
            egui::ComboBox::from_id_salt("pr-target")
                .selected_text(if f.target.is_empty() {
                    t!("Choose a branch")
                } else {
                    f.target.clone()
                })
                .width(300.0)
                .show_ui(ui, |ui| {
                    for b in &f.targets {
                        ui.selectable_value(&mut f.target, b.clone(), format!("{}/{b}", f.remote));
                    }
                });
            ui.end_row();
        });
    ui.checkbox(&mut f.draft, t!("Create as draft"));
    let push_reason = push_reason(tab, &f.source);
    if let Some(reason) = &push_reason {
        ui.checkbox(
            &mut f.push_first,
            t!("Push {} to {} first", f.source, f.remote),
        )
        .on_hover_text(reason);
        if f.push_first {
            ui.weak(reason);
        } else {
            ui.colored_label(ui.visuals().warn_fg_color, reason);
        }
    }
    if f.targets.is_empty() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("No branches of {} are known. Fetch it first.", f.remote),
        );
    }
    if let Some(e) = &f.error {
        ui.add_space(4.0);
        ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
    }

    let ready =
        logged_in && !f.title.trim().is_empty() && !f.target.is_empty() && f.source != f.target;
    ui.add_space(8.0);
    let mut keep = true;
    ui.horizontal(|ui| {
        if ui
            .button(t!("Open in browser instead"))
            .on_hover_text(t!("Fill in the {} on {}", noun, hosted.kind.name()))
            .clicked()
        {
            cmds.push(Command::OpenUrl(
                hosted.new_request_url(&f.source, &f.target),
            ));
            keep = false;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(ready, egui::Button::new(t!("Create {}", noun)))
                .clicked()
            {
                if push_reason.is_some() && f.push_first {
                    let upstream = tab
                        .branches
                        .iter()
                        .find(|b| b.kind == BranchKind::Local && b.name == f.source)
                        .and_then(|b| b.upstream.clone());
                    cmds.run(Op::Push(PushOptions {
                        remote: f.remote.clone(),
                        branch: f.source.clone(),
                        set_upstream: upstream.is_none(),
                        force_with_lease: false,
                        tags: false,
                    }));
                }
                cmds.push(Command::CreatePullRequest(NewPullRequest {
                    title: f.title.trim().to_owned(),
                    body: f.body.clone(),
                    source_branch: f.source.clone(),
                    target_branch: f.target.clone(),
                    draft: f.draft,
                }));
                f.error = None;
                f.submitted = Some(Instant::now());
            }
            if ui.button(t!("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    keep
}

fn login_dialog(ui: &mut egui::Ui, f: &mut LoginForm, cmds: &mut Commands) -> bool {
    ui.heading(t!("Log in to {}", f.kind.name()));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for kind in [ForgeKind::GitHub, ForgeKind::GitLab] {
            if ui.radio_value(&mut f.kind, kind, kind.name()).changed() {
                // Swap the public hosts; keep self-hosted ones.
                if ["github.com", "gitlab.com", ""].contains(&f.host.as_str()) {
                    f.host = match kind {
                        ForgeKind::GitHub => "github.com".into(),
                        ForgeKind::GitLab => "gitlab.com".into(),
                    };
                }
            }
        }
    });
    ui.label("Host");
    ui.add(
        egui::TextEdit::singleline(&mut f.host)
            .hint_text("github.com")
            .desired_width(f32::INFINITY),
    );
    let host = f.host.trim().to_ascii_lowercase();

    ui.label(t!("Personal access token"));
    let token = ui.add(
        egui::TextEdit::singleline(&mut f.token)
            .password(true)
            .hint_text(t!("Paste the token here"))
            .desired_width(f32::INFINITY),
    );
    let scopes = match f.kind {
        ForgeKind::GitHub => "repo",
        ForgeKind::GitLab => "api",
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui
            .add_enabled(!host.is_empty(), egui::Link::new(t!("Create a token")))
            .on_hover_text(token_url(f.kind, &host))
            .clicked()
        {
            cmds.push(Command::OpenUrl(token_url(f.kind, &host)));
        }
        ui.weak(t!("on {} with the \"{}\" scope.", host, scopes));
    });

    if f.kind == ForgeKind::GitHub && !host.is_empty() {
        if f.gh.as_ref().is_none_or(|(h, _)| *h != host) {
            f.gh = Some((host.clone(), forge::gh_cli_token(&host)));
        }
        if let Some((_, Some(gh))) = &f.gh {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.weak(t!("The GitHub CLI is logged in on this computer."));
                if ui.button(t!("Use GitHub CLI login")).clicked() {
                    cmds.push(Command::VerifyAccount {
                        kind: f.kind,
                        host: host.clone(),
                        token: gh.clone(),
                    });
                    f.token.clear();
                    f.host.clear();
                }
            });
            if f.host.is_empty() {
                return false;
            }
        }
    }
    ui.add_space(4.0);
    let note = t!("The token is saved in Ferrum's settings file on this computer.");
    ui.weak(note);

    let ready = !host.is_empty() && !f.token.trim().is_empty();
    let enter = token.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(8.0);
    let mut keep = true;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(ready, egui::Button::new(t!("Log in")))
                .clicked()
                || (enter && ready)
            {
                cmds.push(Command::VerifyAccount {
                    kind: f.kind,
                    host: host.clone(),
                    token: f.token.trim().to_owned(),
                });
                keep = false;
            }
            if ui.button(t!("Cancel")).clicked() {
                keep = false;
            }
        });
    });
    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2023-11-14T22:13:20Z"), Some(1_700_000_000));
        assert_eq!(
            parse_timestamp("2023-11-14T22:13:20.123Z"),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_timestamp("2023-11-15T00:13:20+02:00"),
            Some(1_700_000_000)
        );
        assert_eq!(
            parse_timestamp("2023-11-14T19:13:20.5-03:00"),
            Some(1_700_000_000)
        );
        assert_eq!(parse_timestamp("yesterday"), None);
    }

    #[test]
    fn formats_relative_time() {
        let now = 1_700_000_000;
        assert_eq!(relative_time(now - 5, now), "just now");
        assert_eq!(relative_time(now - 60, now), "1 minute ago");
        assert_eq!(relative_time(now - 3 * 3600, now), "3 hours ago");
        assert_eq!(relative_time(now - 90_000, now), "yesterday");
        assert_eq!(relative_time(now - 5 * 86_400, now), "5 days ago");
        assert_eq!(relative_time(now - 60 * 86_400, now), "2023-09-15");
        // Clock skew: a time in the future is "just now".
        assert_eq!(relative_time(now + 30, now), "just now");
    }

    #[test]
    fn picks_default_target() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let targets = t(&["develop", "feature", "main"]);
        assert_eq!(
            default_target(&targets, Some("develop"), "feature").as_deref(),
            Some("develop")
        );
        assert_eq!(
            default_target(&targets, None, "feature").as_deref(),
            Some("main")
        );
        // The source branch is never its own target.
        assert_eq!(
            default_target(&targets, Some("main"), "main").as_deref(),
            Some("develop")
        );
        assert_eq!(
            default_target(&t(&["trunk"]), None, "x").as_deref(),
            Some("trunk")
        );
        assert_eq!(default_target(&[], None, "x"), None);
    }

    #[test]
    fn builds_token_urls() {
        assert_eq!(
            token_url(ForgeKind::GitHub, "github.com"),
            "https://github.com/settings/tokens/new?scopes=repo&description=Ferrum"
        );
        assert_eq!(
            token_url(ForgeKind::GitLab, "gitlab.example.com"),
            "https://gitlab.example.com/-/user_settings/personal_access_tokens?name=Ferrum&scopes=api"
        );
    }
}
