//! The commit box: subject and description, options, warnings, hook output
//! and the commit buttons.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Key};
use ferrum_core::message;
use ferrum_core::ops::PushOptions;
use ferrum_core::{ChangeKind, Oid, RepoState};

use crate::command::{Command, Commands, Op};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::ui::ai_message::AiTask;
use crate::ui::dialogs::Dialog;

/// Output of the hooks of a running commit, shared with the job.
pub type HookLog = Arc<Mutex<Vec<String>>>;

/// A path and its size in bytes.
type LargeFile = (String, u64);

#[derive(Default)]
pub struct CommitBox {
    draft_loaded: bool,
    /// A commit started here is running.
    in_flight: bool,
    /// The last commit failed (a hook may have rejected it).
    failed: bool,
    log: HookLog,
    ai: Option<AiTask>,
    /// `head_is_pushed` for the HEAD it was checked for.
    pushed: Option<(Option<Oid>, bool)>,
    /// Large staged files, for the staged paths they were checked for.
    large: Option<(Vec<String>, Vec<LargeFile>)>,
    /// `Name <email>` of everyone in the history.
    authors: Vec<String>,
    authors_loaded: bool,
    template: Option<Option<String>>,
}

pub fn format_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn draft_key(tab: &RepoTab) -> String {
    tab.path().display().to_string()
}

pub fn show(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings, cmds: &mut Commands) {
    ui.add_space(4.0);
    load_message(tab, settings);
    track_commit(tab);
    poll_ai(tab);
    warnings(ui, tab, settings, cmds);

    let subject_id = ui.make_persistent_id("commit-subject");
    let body_id = ui.make_persistent_id("commit-body");
    // Read the shortcuts before the text fields see the keys.
    use crate::keymap::{Action, pressed};
    let commit_push = pressed(ui.ctx(), Action::CommitPush);
    let in_message = ui.memory(|m| m.has_focus(subject_id) || m.has_focus(body_id));
    let commit_key = in_message && pressed(ui.ctx(), Action::Commit);

    tools(ui, tab, settings);
    message_fields(ui, tab, settings, subject_id, body_id);
    options(ui, tab, settings);
    buttons(ui, tab, settings, cmds, commit_key, commit_push);
    hook_output(ui, tab, settings, cmds);
    identity(ui, tab, cmds);
    save_draft(tab, settings);
    ui.add_space(4.0);
}

/// Fills an empty message: the saved draft, a prepared merge/revert message,
/// or `commit.template`.
fn load_message(tab: &mut RepoTab, settings: &Settings) {
    let st = &mut tab.changes.ui.commit;
    if !st.draft_loaded {
        st.draft_loaded = true;
        if tab.changes.message.is_empty() {
            match settings.commit.drafts.get(&draft_key(tab)) {
                Some(d) => tab.changes.message = d.clone(),
                None => apply_template(tab),
            }
        }
    }
    // Merges, reverts and cherry-picks come with a prepared message.
    if tab.changes.message.is_empty() && tab.state != RepoState::Clean && !tab.changes.prefilled {
        if let Some(msg) = tab.repo.prepared_message() {
            tab.changes.message = msg;
        }
        tab.changes.prefilled = true;
    }
    if tab.state == RepoState::Clean {
        tab.changes.prefilled = false;
    }
}

/// The `commit.template` message, read once per tab.
fn template(tab: &mut RepoTab) -> Option<String> {
    let st = &mut tab.changes.ui.commit;
    st.template
        .get_or_insert_with(|| tab.repo.commit_template())
        .clone()
}

fn apply_template(tab: &mut RepoTab) {
    if tab.state == RepoState::Clean
        && let Some(t) = template(tab)
    {
        tab.changes.message = t;
    }
}

fn save_draft(tab: &mut RepoTab, settings: &mut Settings) {
    let key = draft_key(tab);
    let is_template = template(tab).is_some_and(|t| t == tab.changes.message);
    let msg = &tab.changes.message;
    let drafts = &mut settings.commit.drafts;
    if msg.trim().is_empty() || is_template {
        drafts.remove(&key);
    } else if drafts.get(&key) != Some(msg) {
        drafts.insert(key, msg.clone());
    }
}

/// Notices when a commit started here finishes.
fn track_commit(tab: &mut RepoTab) {
    let st = &mut tab.changes.ui.commit;
    if st.in_flight && !tab.changes.committing {
        st.in_flight = false;
        // The message is cleared only when the commit worked.
        st.failed = !tab.changes.message.is_empty();
        if !st.failed {
            st.pushed = None;
            apply_template(tab);
        }
    }
}

fn poll_ai(tab: &mut RepoTab) {
    let Some(result) = tab.changes.ui.commit.ai.as_ref().and_then(AiTask::poll) else {
        return;
    };
    tab.changes.ui.commit.ai = None;
    match result {
        Ok(msg) => tab.changes.message = msg,
        Err(e) => tab.error(e),
    }
}

fn warning(ui: &mut egui::Ui, text: String) -> egui::Response {
    ui.colored_label(ui.visuals().warn_fg_color, text)
}

fn warnings(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &Settings, cmds: &mut Commands) {
    if tab.head.is_none() && tab.head_oid.is_some() && tab.state == RepoState::Clean {
        ui.horizontal_wrapped(|ui| {
            warning(
                ui,
                t!("⚠ HEAD is detached: the commit will not be on any branch."),
            );
            if ui.small_button(t!("Create branch…")).clicked()
                && let Some(id) = tab.head_oid
            {
                cmds.push(Command::Dialog(Dialog::create_branch(
                    &id.to_string(),
                    &ferrum_core::short(id),
                )));
            }
        });
    }
    if tab.changes.amend {
        let head = tab.head_oid;
        let st = &mut tab.changes.ui.commit;
        if st.pushed.is_none_or(|(h, _)| h != head) {
            st.pushed = Some((head, tab.repo.head_is_pushed()));
        }
        if st.pushed.is_some_and(|(_, p)| p) {
            warning(
                ui,
                t!(
                    "⚠ The last commit was already pushed. Amending it rewrites published history and needs a force push."
                ),
            );
        }
    }
    if let Some(limit) = settings.commit.large_file_threshold() {
        let paths: Vec<String> = tab
            .status
            .staged
            .iter()
            .filter(|f| f.kind != ChangeKind::Deleted)
            .map(|f| f.path.clone())
            .collect();
        let st = &mut tab.changes.ui.commit;
        if st.large.as_ref().is_none_or(|(p, _)| *p != paths) {
            let big = tab.repo.large_files(&paths, limit);
            st.large = Some((paths, big));
        }
        if let Some((_, big)) = &st.large
            && !big.is_empty()
        {
            let list: Vec<String> = big
                .iter()
                .map(|(p, s)| format!("{p} ({})", format_size(*s)))
                .collect();
            warning(ui, t!("⚠ Large files not in Git LFS: {}", list.join(", "))).on_hover_text(t!(
                "Big files make the repository slow to clone. Consider tracking them with Git LFS."
            ));
        }
    }
}

fn authors(tab: &mut RepoTab) {
    let st = &mut tab.changes.ui.commit;
    if st.authors_loaded {
        return;
    }
    st.authors_loaded = true;
    let me = tab.identity.email.clone().unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    for c in &tab.commits {
        if c.email != me && seen.insert(c.email.to_lowercase()) {
            st.authors.push(format!("{} <{}>", c.author, c.email));
        }
    }
}

fn message_fields(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &Settings,
    subject_id: egui::Id,
    body_id: egui::Id,
) {
    let prefs = &settings.commit;
    let (mut subject, mut body) = message::split(&tab.changes.message);
    let count = subject.chars().count();
    let (soft, hard) = (prefs.subject_soft_limit, prefs.subject_hard_limit.max(1));

    let subject_resp = ui
        .horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = if count > hard {
                    ui.visuals().error_fg_color
                } else if soft > 0 && count > soft {
                    ui.visuals().warn_fg_color
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.colored_label(color, count.to_string()).on_hover_text(t!(
                    "Subject length (up to {} recommended, {} at most)",
                    soft,
                    hard
                ));
                let (normal, over) = (ui.visuals().text_color(), ui.visuals().error_fg_color);
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let text = buf.as_str();
                    let cut = text.char_indices().nth(hard).map_or(text.len(), |(i, _)| i);
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &text[..cut],
                        0.0,
                        egui::TextFormat::simple(font.clone(), normal),
                    );
                    job.append(
                        &text[cut..],
                        0.0,
                        egui::TextFormat {
                            font_id: font,
                            color: over,
                            background: over.gamma_multiply(0.15),
                            ..Default::default()
                        },
                    );
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                ui.add(
                    egui::TextEdit::singleline(&mut subject)
                        .id(subject_id)
                        .hint_text(t!("Summary"))
                        .layouter(&mut layouter)
                        .desired_width(f32::INFINITY),
                )
            })
            .inner
        })
        .inner;
    // Enter in the subject moves on to the description.
    if subject_resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.is_none())
    {
        ui.memory_mut(|m| m.request_focus(body_id));
    }

    authors(tab);
    let authors = &tab.changes.ui.commit.authors;
    let font = egui::FontId::monospace(12.5);
    let out = egui::ScrollArea::vertical()
        .id_salt("commit-body-scroll")
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::CompletionPopup::new(body_id)
                .word_boundary(|c| c == '\n')
                .show(
                    ui,
                    egui::TextEdit::multiline(&mut body)
                        .hint_text(t!("Description (optional)"))
                        .font(font.clone())
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                    |q| {
                        let Some(query) = message::co_author_query(q.word) else {
                            return Vec::new();
                        };
                        let query = query.to_lowercase();
                        authors
                            .iter()
                            .filter(|a| a.to_lowercase().contains(&query))
                            .take(8)
                            .map(|a| egui::Suggestion::new(format!("Co-authored-by: {a}")))
                            .collect()
                    },
                )
        })
        .inner;

    // The ruler: a thin line at the wrap column.
    let column = prefs.ruler_column;
    if column > 0 {
        let width = ui.fonts_mut(|f| f.glyph_width(&font, 'x'));
        let x = out.text_edit.galley_pos.x + width * column as f32;
        let clip = out.text_edit.text_clip_rect;
        if x < clip.right() {
            let color = ui.visuals().weak_text_color().gamma_multiply(0.4);
            ui.painter().with_clip_rect(clip).vline(
                x,
                clip.y_range(),
                egui::Stroke::new(1.0, color),
            );
        }
    }

    let new = message::join(&subject, &body);
    if new != tab.changes.message {
        tab.changes.message = new;
    }
}

fn options(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings) {
    ui.horizontal(|ui| {
        let was = tab.changes.amend;
        ui.add_enabled_ui(tab.head_oid.is_some(), |ui| {
            ui.checkbox(&mut tab.changes.amend, t!("Amend last commit"))
                .on_hover_text(t!(
                    "Replace the last commit with the staged changes and this message"
                ));
        });
        if tab.changes.amend != was {
            if tab.changes.amend && tab.changes.message.trim().is_empty() {
                tab.changes.message = tab.repo.last_commit_message().unwrap_or_default();
            }
            // The staged list shows the last commit's changes while amending.
            tab.refresh_status();
        }
        ui.checkbox(&mut settings.commit.signoff, t!("Sign off"))
            .on_hover_text(t!("Add a Signed-off-by line (--signoff)"));
    });
}

/// The header over the message: recent messages, wrap and AI.
fn tools(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings) {
    ui.horizontal(|ui| {
        ui.weak(t!("Commit message"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ai_button(ui, tab, settings);
            let column = settings.commit.ruler_column;
            if column > 0
                && ui
                    .small_button(t!("Wrap"))
                    .on_hover_text(t!("Wrap the description at column {}", column))
                    .clicked()
            {
                let (subject, body) = message::split(&tab.changes.message);
                tab.changes.message = message::join(&subject, &message::wrap(&body, column));
            }
            let recent = &settings.commit.recent_messages;
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                ui.menu_button("🕓", |ui| {
                    ui.set_max_width(360.0);
                    for m in recent {
                        let first = m.lines().next().unwrap_or_default();
                        if ui
                            .add(egui::Button::new(first).truncate())
                            .on_hover_text(m)
                            .clicked()
                        {
                            tab.changes.message = m.clone();
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(t!("Recent messages"));
            });
        });
    });
}

fn ai_button(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &Settings) {
    if let Some(task) = &tab.changes.ui.commit.ai {
        ui.spinner();
        if ui
            .small_button(t!("Cancel"))
            .on_hover_text(t!("Stop writing the message"))
            .clicked()
        {
            task.cancel();
        }
        return;
    }
    let can = !tab.status.staged.is_empty();
    let resp = ui
        .add_enabled(can, egui::Button::new(t!("AI")).small())
        .on_hover_text(t!(
            "Write the message from the staged changes with {}",
            settings.ai.command
        ))
        .on_disabled_hover_text(t!("Stage some changes first"));
    if resp.clicked() {
        let task = AiTask::start(
            tab.path().to_path_buf(),
            &settings.ai,
            tab.changes.amend,
            ui.ctx(),
        );
        tab.changes.ui.commit.ai = Some(task);
    }
}

fn push_op(tab: &RepoTab) -> Option<Op> {
    let branch = tab.head.clone()?;
    let upstream = tab
        .current_branch()
        .and_then(|b| b.upstream.as_deref())
        .and_then(|u| u.split_once('/'))
        .map(|(r, _)| r.to_owned());
    let remote = upstream.clone().or_else(|| tab.default_remote())?;
    Some(Op::Push(PushOptions {
        remote,
        branch,
        set_upstream: upstream.is_none(),
        force_with_lease: false,
        tags: false,
    }))
}

fn commit(
    tab: &mut RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    stage_all: bool,
    push: bool,
    no_verify: bool,
) {
    let message = tab.changes.message.clone();
    settings.commit.add_recent_message(&message);
    let st = &mut tab.changes.ui.commit;
    if let Ok(mut l) = st.log.lock() {
        l.clear();
    }
    st.in_flight = true;
    st.failed = false;
    tab.changes.committing = true;
    if stage_all {
        cmds.run(Op::StageAll);
    }
    cmds.run(Op::Commit {
        message,
        amend: tab.changes.amend,
        signoff: settings.commit.signoff,
        no_verify,
        log: st.log.clone(),
    });
    if push || settings.commit.push_after_commit {
        match push_op(tab) {
            Some(op) => cmds.run(op),
            None => cmds.push(Command::Error(t!(
                "Committed, but there is no branch or remote to push to."
            ))),
        }
    }
}

fn buttons(
    ui: &mut egui::Ui,
    tab: &mut RepoTab,
    settings: &mut Settings,
    cmds: &mut Commands,
    commit_key: bool,
    commit_push: bool,
) {
    let staged = tab.status.staged.len();
    let has_message = !tab.changes.message.trim().is_empty();
    let can_commit = has_message
        && (staged > 0 || tab.changes.amend)
        && !tab.busy()
        && tab.status.conflicted.is_empty();
    let label = match (tab.changes.amend, staged) {
        (true, _) => t!("Amend commit"),
        (false, 1) => t!("Commit 1 file"),
        (false, n) => t!("Commit {} files", n),
    };
    let why = if !has_message {
        t!("Write a commit message first")
    } else if staged == 0 && !tab.changes.amend {
        t!("Stage some changes first")
    } else {
        String::new()
    };
    ui.horizontal(|ui| {
        let resp = ui
            .add_enabled(
                can_commit,
                egui::Button::new(label).min_size(egui::vec2(140.0, 26.0)),
            )
            .on_hover_text(t!("Ctrl+Enter"))
            .on_disabled_hover_text(&why);
        if resp.clicked() || (commit_key && can_commit) {
            commit(tab, settings, cmds, false, false, false);
        }
        let push = ui
            .add_enabled(
                can_commit && tab.head.is_some(),
                egui::Button::new(t!("Commit and push")).min_size(egui::vec2(0.0, 26.0)),
            )
            .on_hover_text(t!("Ctrl+Shift+Enter"))
            .on_disabled_hover_text(&why);
        if push.clicked() || (commit_push && can_commit && tab.head.is_some()) {
            commit(tab, settings, cmds, false, true, false);
        }
        if staged == 0
            && !tab.status.unstaged.is_empty()
            && has_message
            && !tab.changes.amend
            && ui
                .add_enabled(!tab.busy(), egui::Button::new(t!("Stage all & commit")))
                .clicked()
        {
            commit(tab, settings, cmds, true, false, false);
        }
    });
    ui.checkbox(
        &mut settings.commit.push_after_commit,
        t!("Push after every commit"),
    );
}

/// Live output of the commit hooks, and a way to skip them after a failure.
fn hook_output(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings, cmds: &mut Commands) {
    let st = &tab.changes.ui.commit;
    let lines = st.log.lock().map(|l| l.clone()).unwrap_or_default();
    if lines.is_empty() && !st.failed {
        return;
    }
    let (running, failed) = (st.in_flight, st.failed);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            if running {
                ui.spinner();
                ui.strong(t!("Running hooks…"));
            } else if failed {
                ui.colored_label(ui.visuals().error_fg_color, t!("The commit failed"));
            } else {
                ui.strong(t!("Hook output"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !running && ui.small_button("×").on_hover_text(t!("Close")).clicked() {
                    let st = &mut tab.changes.ui.commit;
                    st.failed = false;
                    if let Ok(mut l) = st.log.lock() {
                        l.clear();
                    }
                }
            });
        });
        if !lines.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("hook-output")
                .max_height(120.0)
                .stick_to_bottom(true)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for l in &lines {
                        ui.label(egui::RichText::new(l).monospace().size(11.5));
                    }
                });
        }
        if failed
            && ui
                .add_enabled(!tab.busy(), egui::Button::new(t!("Commit without hooks")))
                .on_hover_text(t!(
                    "Commit again, skipping the pre-commit and commit-msg hooks (--no-verify)"
                ))
                .clicked()
        {
            commit(tab, settings, cmds, false, false, true);
        }
    });
}

fn identity(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    match (&tab.identity.name, &tab.identity.email) {
        (Some(n), Some(e)) => {
            ui.weak(t!("as {} <{}>", n, e))
                .on_hover_text(t!("Change in the identity menu at the bottom right"));
        }
        _ => {
            if ui
                .link(t!("⚠ No author identity set — click to choose one"))
                .clicked()
            {
                cmds.push(Command::Dialog(Dialog::identity(&tab.identity)));
            }
        }
    }
}
