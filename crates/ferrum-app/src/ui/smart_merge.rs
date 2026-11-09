//! Semantic merge suggestions: when a merge, rebase or cherry-pick stops with
//! conflicts, each conflicted file is merged declaration by declaration in
//! the background (`ferrum_core::semantic_merge`, or the `mergiraf` program
//! when installed and allowed). Files it resolves are offered in the conflict
//! editor as "Resolved automatically — review": nothing is written or staged
//! until the user takes the result and marks the file as resolved.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use ferrum_core::conflict;
use ferrum_core::semantic_merge::{self, Decision, Failure, How, mergiraf};
use ferrum_core::{Oid, Repo, RepoState};

use crate::icons;
use crate::tab::RepoTab;
use crate::ui::diff_prefs;

#[derive(Clone)]
pub enum Outcome {
    Resolved(Arc<Suggestion>),
    Failed(Failure),
}

pub struct Suggestion {
    pub text: String,
    pub decisions: Vec<Decision>,
    /// Made by the mergiraf program rather than Ferrum.
    pub by_mergiraf: bool,
}

type Key = (RepoState, Option<Oid>, Vec<String>, bool, bool);

/// Suggestions for the conflicted files of a tab.
#[derive(Default)]
pub struct Suggestions {
    key: Option<Key>,
    results: Arc<Mutex<HashMap<String, Outcome>>>,
    /// Files whose suggestion the user dismissed.
    pub dismissed: HashSet<String>,
}

impl Suggestions {
    pub fn get(&self, path: &str) -> Option<Outcome> {
        self.results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .cloned()
    }

    /// Files resolved automatically (and not dismissed), in `order`.
    pub fn resolved<'a>(&self, order: &'a [String]) -> Vec<&'a String> {
        let results = self.results.lock().unwrap_or_else(|e| e.into_inner());
        order
            .iter()
            .filter(|p| {
                !self.dismissed.contains(*p)
                    && matches!(results.get(*p), Some(Outcome::Resolved(_)))
            })
            .collect()
    }
}

/// Starts the suggestions again when the conflicts or the settings change.
pub fn update(ctx: &egui::Context, tab: &mut RepoTab) {
    let prefs = diff_prefs::get(ctx);
    let paths: Vec<String> = tab
        .status
        .conflicted
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let key = (
        tab.state,
        tab.head_oid,
        paths.clone(),
        prefs.semantic_merge,
        prefs.mergiraf,
    );
    let dir = tab.path().to_path_buf();
    let s = &mut tab.conflicts.smart;
    if s.key.as_ref() == Some(&key) {
        return;
    }
    if s.key.as_ref().is_none_or(|k| k.0 != key.0 || k.1 != key.1) {
        s.dismissed.clear();
    }
    s.key = Some(key);
    let results: Arc<Mutex<HashMap<String, Outcome>>> = Arc::default();
    s.results = results.clone();
    if !prefs.semantic_merge || paths.is_empty() || tab.state == RepoState::Clean {
        return;
    }
    let ctx = ctx.clone();
    let use_mergiraf = prefs.mergiraf;
    // Deep syntax trees need a larger stack than the default.
    let _ = std::thread::Builder::new()
        .name("semantic-merge".into())
        .stack_size(64 << 20)
        .spawn(move || {
            let Ok(repo) = Repo::open(&dir) else { return };
            let exe = use_mergiraf.then(mergiraf::find).flatten();
            for path in paths {
                if let Some(o) = suggest(&repo, &path, exe.as_deref()) {
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(path, o);
                    ctx.request_repaint();
                }
            }
        });
}

fn suggest(repo: &Repo, path: &str, mergiraf: Option<&std::path::Path>) -> Option<Outcome> {
    let [base, ours, theirs] = repo.conflict_versions(path).ok()?;
    // Deleted on one side: there is nothing to merge.
    let (Some(ours), Some(theirs)) = (ours, theirs) else {
        return None;
    };
    match semantic_merge::merge(path, base.as_deref(), &ours, &theirs) {
        Ok(m) => Some(Outcome::Resolved(Arc::new(Suggestion {
            text: m.text,
            decisions: m.decisions,
            by_mergiraf: false,
        }))),
        Err(f) => {
            if let Some(exe) = mergiraf {
                let base = base.as_deref().unwrap_or("");
                if let Ok(Some(text)) = mergiraf::merge(exe, path, base, &ours, &theirs) {
                    return Some(Outcome::Resolved(Arc::new(Suggestion {
                        text,
                        decisions: Vec::new(),
                        by_mergiraf: true,
                    })));
                }
            }
            Some(Outcome::Failed(f))
        }
    }
}

/// What the user chose in the suggestion card.
pub enum CardAction {
    /// Put the suggestion in the result editor.
    Use,
    Dismiss,
}

fn how_text(how: How, names: &(String, String)) -> (&'static str, String) {
    match how {
        How::AddedOurs => (icons::PLUS, t!("added on {}", names.0)),
        How::AddedTheirs => (icons::PLUS, t!("added on {}", names.1)),
        How::ChangedOurs => (icons::PENCIL_SIMPLE, t!("changed on {}", names.0)),
        How::ChangedTheirs => (icons::PENCIL_SIMPLE, t!("changed on {}", names.1)),
        How::DeletedOurs => (icons::MINUS, t!("deleted on {}", names.0)),
        How::DeletedTheirs => (icons::MINUS, t!("deleted on {}", names.1)),
        How::SameChange => (icons::CHECK, t!("same change on both sides")),
        How::MergedLines => (
            icons::ARROWS_MERGE,
            t!("changed on both sides, merged line by line"),
        ),
    }
}

/// Why the semantic merge left the file alone; `None` for files it does not handle.
pub fn failure_text(f: &Failure) -> Option<String> {
    match f {
        Failure::Unsupported | Failure::TooLarge => None,
        Failure::Syntax => Some(t!("a version has syntax errors")),
        Failure::Markers => Some(t!("a version contains conflict markers")),
        Failure::Conflict(item) => Some(t!("both sides changed {}", item)),
        Failure::SamePlace(item) if item.is_empty() => {
            Some(t!("both sides added code at the same place"))
        }
        Failure::SamePlace(item) => Some(t!("both sides added code at the same place in {}", item)),
    }
}

/// The suggestion for a file: what was decided, its difference with each
/// side and the buttons to use or dismiss it.
pub fn card(
    ui: &mut egui::Ui,
    s: &Suggestion,
    applied: bool,
    versions: [&str; 2],
    names: &(String, String),
    compare: &mut Option<bool>,
) -> Option<CardAction> {
    let mut action = None;
    let accent = crate::theme::accent(ui);
    crate::theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::MAGIC_WAND).size(18.0).color(accent));
            ui.label(crate::theme::strong(t!("Resolved automatically — review")).size(14.0));
            let by = if s.by_mergiraf {
                t!("by mergiraf")
            } else {
                t!("by Ferrum's semantic merge")
            };
            ui.weak(by);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(icons::X).frame_when_inactive(false))
                    .on_hover_text(t!("Dismiss the suggestion"))
                    .clicked()
                {
                    action = Some(CardAction::Dismiss);
                }
                if applied {
                    ui.colored_label(
                        crate::util::added_color(ui),
                        icons::text(icons::CHECK, t!("In the result below")),
                    );
                } else if ui
                    .add(crate::theme::primary(icons::text(icons::CHECK, t!("Use this result"))))
                    .on_hover_text(t!("Put the merged file in the result editor; nothing is staged until you mark it as resolved"))
                    .clicked()
                {
                    action = Some(CardAction::Use);
                }
            });
        });
        ui.add_space(crate::theme::SPACE_S);
        if s.decisions.is_empty() {
            ui.weak(t!("Both sides' changes were merged by syntax. Check the result before marking the file as resolved."));
        } else {
            for d in s.decisions.iter().take(8) {
                let (icon, text) = how_text(d.how, names);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new(icon).color(ui.visuals().weak_text_color()));
                    ui.label(RichText::new(&d.item).monospace());
                    ui.weak(text);
                });
            }
            if s.decisions.len() > 8 {
                ui.weak(t!("… and {} more", s.decisions.len() - 8));
            }
        }
        ui.add_space(crate::theme::SPACE_S);
        ui.horizontal(|ui| {
            ui.label(t!("Compare with"));
            for (theirs, name) in [(false, &names.0), (true, &names.1)] {
                let on = *compare == Some(theirs);
                if ui.selectable_label(on, name.as_str()).clicked() {
                    *compare = if on { None } else { Some(theirs) };
                }
            }
        });
        if let Some(theirs) = *compare {
            let side = versions[usize::from(theirs)];
            diff_lines(ui, &conflict::diff_lines(side, &s.text, 2));
        }
    });
    action
}

/// A small read-only diff.
fn diff_lines(ui: &mut egui::Ui, lines: &[Option<conflict::DiffLine>]) {
    if lines.is_empty() {
        ui.weak(t!("No differences."));
        return;
    }
    let (add, del) = (crate::util::added_color(ui), crate::util::removed_color(ui));
    let weak = ui.visuals().weak_text_color();
    egui::ScrollArea::both()
        .id_salt("smart-merge-diff")
        .max_height(180.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for line in lines {
                let (text, color) = match line {
                    None => ("…".to_owned(), weak),
                    Some((o, t)) => (
                        format!("{o} {}", crate::util::expand_tabs(t)),
                        match o {
                            '+' => add,
                            '-' => del,
                            _ => weak,
                        },
                    ),
                };
                ui.add(egui::Label::new(RichText::new(text).monospace().color(color)).extend());
            }
        });
}

/// A line for the operation banner: how many files can be resolved automatically.
pub fn banner_hint(ui: &mut egui::Ui, tab: &RepoTab) -> Option<String> {
    let order: Vec<String> = tab
        .status
        .conflicted
        .iter()
        .map(|f| f.path.clone())
        .collect();
    let resolved = tab.conflicts.smart.resolved(&order);
    let first = resolved.first().map(|p| (*p).clone())?;
    let text = if resolved.len() == 1 {
        t!("1 file can be resolved automatically")
    } else {
        t!("{} files can be resolved automatically", resolved.len())
    };
    let accent = crate::theme::accent(ui);
    ui.add(egui::Link::new(icons::tinted(
        icons::MAGIC_WAND,
        accent,
        text,
    )))
    .on_hover_text(t!("Review the suggested merge"))
    .clicked()
    .then_some(first)
}

/// Settings > Tools: structural diff and semantic merge.
pub fn settings_block(ui: &mut egui::Ui, prefs: &mut diff_prefs::DiffPrefs) {
    ui.add_space(crate::theme::SPACE_XL);
    ui.label(crate::theme::strong(t!("Structural diff and semantic merge")).size(15.0));
    ui.add_space(crate::theme::SPACE_S);
    let langs: Vec<&str> = ferrum_core::syntax::available()
        .iter()
        .map(|l| l.name())
        .collect();
    ui.checkbox(
        &mut prefs.structural_diff,
        t!("Offer the structural diff (Structural in the diff toolbar)"),
    );
    ui.checkbox(
        &mut prefs.semantic_merge,
        t!("Try a semantic merge of conflicted files and suggest the result"),
    );
    ui.add_enabled_ui(prefs.semantic_merge, |ui| {
        let found = mergiraf_path(ui.ctx());
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut prefs.mergiraf,
                t!("Also try mergiraf when it is installed"),
            );
            match &found {
                Some(p) => ui.weak(t!("found: {}", p)),
                None => ui.weak(t!("not found on the PATH")),
            };
        });
    });
    ui.add(
        egui::Label::new(
            RichText::new(t!("Languages: {}.", langs.join(", ")))
                .weak()
                .small(),
        )
        .wrap(),
    );
}

/// Where mergiraf is, looked up once per run.
fn mergiraf_path(ctx: &egui::Context) -> Option<String> {
    let id = egui::Id::unique("mergiraf-path");
    if let Some(p) = ctx.data(|d| d.get_temp::<Option<String>>(id)) {
        return p;
    }
    let p = mergiraf::find().map(|p| p.display().to_string());
    ctx.data_mut(|d| d.insert_temp(id, p.clone()));
    p
}
