//! The commit box: subject and description, options, warnings, hook output
//! and the commit buttons.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Key};
use ferrum_core::message;
use ferrum_core::ops::PushOptions;
use ferrum_core::{ChangeKind, Oid, RepoState};

use crate::command::{Command, Commands, Op};
use crate::icons;
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::theme;
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
    ui.add_space(theme::SPACE_M);
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

    tools(ui, tab, settings, cmds);
    message_fields(ui, tab, settings, subject_id, body_id);
    options(ui, tab, settings);
    buttons(ui, tab, settings, cmds, commit_key, commit_push);
    hook_output(ui, tab, settings, cmds);
    identity(ui, tab, cmds);
    save_draft(tab, settings);
    ui.add_space(theme::SPACE_S);
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

    let (normal, over) = (ui.visuals().text_color(), ui.visuals().error_fg_color);
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
        let text = buf.as_str();
        let cut = text.char_indices().nth(hard).map_or(text.len(), |(i, _)| i);
        let font = egui::FontId::proportional(SUBJECT_SIZE);
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
    let subject_resp = ui.add(
        egui::TextEdit::singleline(&mut subject)
            .id(subject_id)
            .hint_text(t!("Summary"))
            .font(egui::FontId::proportional(SUBJECT_SIZE))
            .margin(egui::Margin {
                left: 8,
                right: 64,
                top: 7,
                bottom: 7,
            })
            .layouter(&mut layouter)
            .desired_width(f32::INFINITY),
    );
    counter(ui, subject_resp.rect, count, soft, hard);
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
        .max_height(240.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::CompletionPopup::new(body_id)
                .word_boundary(|c| c == '\n')
                .show(
                    ui,
                    egui::TextEdit::multiline(&mut body)
                        .hint_text(t!("Description (optional)"))
                        .font(font.clone())
                        .margin(egui::vec2(8.0, 6.0))
                        .desired_rows(5)
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

/// Font size of the summary field.
const SUBJECT_SIZE: f32 = 14.5;

/// The summary length, in a pill at the right end of the field: weak up to
/// `soft` characters, warning color up to `hard`, error color beyond.
fn counter(ui: &egui::Ui, field: egui::Rect, count: usize, soft: usize, hard: usize) {
    let v = ui.visuals();
    let (color, limit) = if count > hard {
        (v.error_fg_color, hard)
    } else if soft > 0 && count > soft {
        (v.warn_fg_color, hard)
    } else {
        (v.weak_text_color(), if soft > 0 { soft } else { hard })
    };
    let text = format!("{count}/{limit}");
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::strong_font(11.0), color);
    let size = galley.size() + egui::vec2(12.0, 6.0);
    let pill = egui::Rect::from_min_size(
        egui::pos2(
            field.right() - size.x - 6.0,
            field.center().y - size.y / 2.0,
        ),
        size,
    );
    if count > soft && soft > 0 {
        let fill = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 36);
        ui.painter().rect_filled(pill, theme::RADIUS, fill);
    }
    ui.painter()
        .galley(pill.center() - galley.size() / 2.0, galley, color);
    let tip = t!(
        "Subject length (up to {} recommended, {} at most)",
        soft,
        hard
    );
    let id = egui::Id::unique("commit-subject-counter");
    ui.interact(pill, id, egui::Sense::hover())
        .on_hover_text(tip);
}

fn options(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings) {
    ui.horizontal_wrapped(|ui| {
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
        ui.checkbox(
            &mut settings.commit.push_after_commit,
            t!("Push after every commit"),
        );
    });
}

/// The header over the message: recent messages, wrap and AI.
fn tools(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &mut Settings, cmds: &mut Commands) {
    ui.horizontal(|ui| {
        ui.label(theme::strong(t!("Commit message")));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ai_button(ui, tab, settings, cmds);
            let column = settings.commit.ruler_column;
            if column > 0
                && ui
                    .add(
                        egui::Button::new(icons::TEXT_ALIGN_LEFT)
                            .small()
                            .frame_when_inactive(false),
                    )
                    .on_hover_text(t!("Wrap the description at column {}", column))
                    .clicked()
            {
                let (subject, body) = message::split(&tab.changes.message);
                tab.changes.message = message::join(&subject, &message::wrap(&body, column));
            }
            let recent = &settings.commit.recent_messages;
            ui.add_enabled_ui(!recent.is_empty(), |ui| {
                let button = egui::Button::new(icons::HISTORY)
                    .small()
                    .frame_when_inactive(false);
                egui::containers::menu::MenuButton::from_button(button)
                    .ui(ui, |ui| {
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
                    .0
                    .on_hover_text(t!("Recent messages"));
            });
        });
    });
}

fn ai_button(ui: &mut egui::Ui, tab: &mut RepoTab, settings: &Settings, cmds: &mut Commands) {
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
    let cfg = crate::ui::ai_common::config(ui.ctx());
    let hover = if cfg.enabled {
        t!(
            "Write the message from the staged changes. {}",
            crate::ai::where_to(&cfg.provider).long
        )
    } else {
        t!("AI is off: turn it on in Settings > AI.")
    };
    let resp = ui
        .add_enabled(
            can,
            icons::button(icons::AI, t!("AI"))
                .small()
                .frame_when_inactive(false),
        )
        .on_hover_text(hover)
        .on_disabled_hover_text(t!("Stage some changes first"));
    if resp.clicked() {
        if !cfg.enabled {
            cmds.push(crate::command::Command::Ai(crate::ai::AiCmd::OpenSettings));
            return;
        }
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
    let why = if !tab.status.conflicted.is_empty() {
        t!("Resolve the conflicts first")
    } else if !has_message {
        t!("Write a commit message first")
    } else if staged == 0 && !tab.changes.amend {
        t!("Stage some changes first")
    } else {
        String::new()
    };
    let stage_first = staged == 0
        && !tab.status.unstaged.is_empty()
        && !tab.changes.amend
        && tab.status.conflicted.is_empty();
    let height = 32.0;
    let can_push = can_commit && tab.head.is_some();
    ui.add_space(theme::SPACE_XS);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // More ways to commit, in a menu next to the main button.
            let more = egui::Button::new(egui::RichText::new(icons::DROPDOWN).size(14.0))
                .min_size(egui::vec2(30.0, height));
            let menu = egui::containers::menu::MenuButton::from_button(more).ui(ui, |ui| {
                ui.set_min_width(220.0);
                let push = egui::Button::new(icons::text(icons::PUSH, t!("Commit and push")))
                    .shortcut_text(crate::keymap::text(crate::keymap::Action::CommitPush));
                if ui
                    .add_enabled(can_push, push)
                    .on_disabled_hover_text(&why)
                    .clicked()
                {
                    commit(tab, settings, cmds, false, true, false);
                    ui.close();
                }
                let all = egui::Button::new(icons::text(icons::CHECK, t!("Stage all & commit")));
                let can_all = has_message
                    && !tab.busy()
                    && !tab.changes.amend
                    && tab.status.conflicted.is_empty()
                    && !tab.status.unstaged.is_empty();
                if ui.add_enabled(can_all, all).clicked() {
                    commit(tab, settings, cmds, true, false, false);
                    ui.close();
                }
            });
            menu.0.on_hover_text(t!("More ways to commit"));
            if commit_push && can_push {
                commit(tab, settings, cmds, false, true, false);
            }
            let size = egui::vec2(ui.available_width(), height);
            if stage_first {
                // Nothing staged yet: the main action stages everything first.
                let n = tab.status.unstaged.len();
                let text = if n == 1 {
                    t!("Stage and commit 1 file")
                } else {
                    t!("Stage and commit {} files", n)
                };
                let resp = ui
                    .add_enabled(
                        has_message && !tab.busy(),
                        theme::primary(icons::text(icons::CHECK, text)).min_size(size),
                    )
                    .on_hover_text(t!("Stage every change, then commit"))
                    .on_disabled_hover_text(&why);
                if resp.clicked() {
                    commit(tab, settings, cmds, true, false, false);
                }
            } else {
                let resp = ui
                    .add_enabled(
                        can_commit,
                        theme::primary(icons::text(icons::CHECK, label)).min_size(size),
                    )
                    .on_hover_text(crate::ui::learning::tip(
                        ui.ctx(),
                        t!("Ctrl+Enter"),
                        Some(&Op::Commit {
                            message: String::new(),
                            amend: tab.changes.amend,
                            signoff: false,
                            no_verify: false,
                            log: Default::default(),
                        }),
                    ))
                    .on_disabled_hover_text(&why);
                if resp.clicked() || (commit_key && can_commit) {
                    commit(tab, settings, cmds, false, false, false);
                }
            }
        });
    });
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
                if !running
                    && ui
                        .small_button(crate::icons::CLOSE)
                        .on_hover_text(t!("Close"))
                        .clicked()
                {
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
    let weak = ui.visuals().weak_text_color();
    let small = |t: String| egui::RichText::new(t).size(12.0).color(weak);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match (&tab.identity.name, &tab.identity.email) {
                (Some(n), Some(e)) => {
                    ui.label(small(icons::text(icons::IDENTITY, n)))
                        .on_hover_text(t!("as {} <{}>", n, e));
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
            let branch = match &tab.head {
                Some(b) => b.clone(),
                None if tab.head_oid.is_some() => t!("detached HEAD"),
                None => return,
            };
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(small(icons::text(icons::BRANCH, &branch))).truncate())
                    .on_hover_text(t!("The commit goes on {}", branch));
            });
        });
    });
}
