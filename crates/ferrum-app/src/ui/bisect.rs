//! Bisect: the start dialog, marking commits from the history menu, the
//! colored marks in the graph and the banner with the progress timeline.

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, pos2, vec2};
use ferrum_core::bisect::{self, BisectStatus, Mark};
use ferrum_core::cli::Git;
use ferrum_core::{Oid, RepoState, short};

use crate::command::{Command, Commands, Op};
use crate::tab::RepoTab;
use crate::ui::dialogs::{Dialog, buttons};

/// Operations of this module, run through `Op::Bisect`.
#[derive(Debug, Clone)]
pub enum BisectOp {
    Start { bad: String, good: Vec<String> },
    Mark(Mark, String),
    Reset,
}

impl BisectOp {
    pub fn label(&self) -> String {
        match self {
            BisectOp::Start { .. } => t!("Start bisect"),
            BisectOp::Mark(m, rev) => t!("Mark {} as {}", short_rev(rev), mark_word(*m)),
            BisectOp::Reset => t!("Reset bisect"),
        }
    }

    pub fn run(&self, git: &Git) -> ferrum_core::Result<()> {
        match self {
            BisectOp::Start { bad, good } => bisect::start(git, bad, good).map(drop),
            BisectOp::Mark(m, rev) => match bisect::mark(git, *m, rev) {
                // git exits with an error when only skipped commits are left;
                // the banner explains that.
                Err(_) if *m == Mark::Skip => Ok(()),
                r => r.map(drop),
            },
            BisectOp::Reset => bisect::reset(git),
        }
    }
}

fn short_rev(rev: &str) -> &str {
    crate::flow::short_rev(rev)
}

pub fn mark_word(m: Mark) -> String {
    match m {
        Mark::Good => t!("good"),
        Mark::Bad => t!("bad"),
        Mark::Skip => t!("skipped"),
    }
}

pub fn mark_color(m: Mark, dark: bool) -> Color32 {
    match (m, dark) {
        (Mark::Good, true) => Color32::from_rgb(0x4c, 0xc3, 0x8a),
        (Mark::Good, false) => Color32::from_rgb(0x1a, 0x7f, 0x37),
        (Mark::Bad, true) => Color32::from_rgb(0xf0, 0x5a, 0x5a),
        (Mark::Bad, false) => Color32::from_rgb(0xc0, 0x26, 0x26),
        (Mark::Skip, _) => Color32::from_gray(128),
    }
}

fn current_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xe0, 0xa8, 0x30)
    } else {
        Color32::from_rgb(0xa8, 0x6a, 0x00)
    }
}

/// The bisect status of a tab, while one runs.
pub fn status(tab: &RepoTab) -> Option<&BisectStatus> {
    tab.bisect
        .as_ref()
        .filter(|_| tab.state == RepoState::Bisect)
}

// ----- History -----

/// Paints the bisect mark of a commit row: a colored bar at its left edge and
/// a badge at `pos`. Returns where the next item starts.
pub fn paint_mark(
    painter: &egui::Painter,
    row: Rect,
    pos: Pos2,
    tab: &RepoTab,
    id: Oid,
    dark: bool,
) -> f32 {
    let Some(st) = status(tab) else {
        return pos.x;
    };
    let (text, color) = if st.first_bad == Some(id) {
        (t!("first bad"), mark_color(Mark::Bad, dark))
    } else if let Some(m) = st.mark_of(id) {
        (mark_word(m), mark_color(m, dark))
    } else if st.current == Some(id) && st.first_bad.is_none() {
        (t!("testing"), current_color(dark))
    } else {
        return pos.x;
    };
    let bar = Rect::from_min_size(row.min, vec2(3.0, row.height()));
    painter.rect_filled(bar, 0.0, color);
    let galley = painter.layout_no_wrap(text, FontId::proportional(11.0), Color32::WHITE);
    let size = galley.size() + vec2(10.0, 4.0);
    let r = Rect::from_min_size(pos2(pos.x, pos.y - size.y / 2.0), size);
    painter.rect_filled(r, 4.0, color);
    painter.galley(r.min + vec2(5.0, 2.0), galley, Color32::WHITE);
    r.right() + 4.0
}

/// Bisect entries of a commit's context menu.
pub fn commit_menu(ui: &mut egui::Ui, tab: &RepoTab, id: Oid, cmds: &mut Commands) {
    let sha = id.to_string();
    let s = short(id);
    if let Some(st) = status(tab) {
        ui.separator();
        for m in [Mark::Good, Mark::Bad, Mark::Skip] {
            let label = match m {
                Mark::Good => t!("Bisect: mark {} as good", s),
                Mark::Bad => t!("Bisect: mark {} as bad", s),
                Mark::Skip => t!("Bisect: skip {}", s),
            };
            if st.mark_of(id) != Some(m) && ui.button(label).clicked() {
                cmds.run(Op::Bisect(BisectOp::Mark(m, sha.clone())));
                ui.close();
            }
        }
        if ui.button(t!("Reset bisect")).clicked() {
            cmds.run(Op::Bisect(BisectOp::Reset));
            ui.close();
        }
        return;
    }
    if tab.state != RepoState::Clean {
        return;
    }
    ui.menu_button(t!("Bisect"), |ui| {
        if ui
            .button(t!("Start bisect: {} is bad…", s))
            .on_hover_text(t!("Find the commit that introduced a problem"))
            .clicked()
        {
            cmds.push(Command::Dialog(start(tab, &sha, "")));
            ui.close();
        }
        if ui.button(t!("Start bisect: {} is good…", s)).clicked() {
            let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
            cmds.push(Command::Dialog(start(tab, &head, &sha)));
            ui.close();
        }
    });
}

/// "Bisect between these" for two selected commits: the newer one is bad.
pub fn multi_menu(ui: &mut egui::Ui, tab: &RepoTab, many: &[Oid], cmds: &mut Commands) {
    let [a, b] = many else { return };
    if tab.state != RepoState::Clean {
        return;
    }
    let (good, bad) = if tab.repo.is_ancestor(*a, *b) {
        (*a, *b)
    } else {
        (*b, *a)
    };
    if ui.button(t!("Bisect between these…")).clicked() {
        cmds.push(Command::Dialog(start(
            tab,
            &bad.to_string(),
            &good.to_string(),
        )));
        ui.close();
    }
}

// ----- Start dialog -----

pub struct BisectDialog {
    bad: String,
    good: String,
    /// Commits between good and bad, for the pair it was counted for.
    count: Option<((Oid, Oid), usize)>,
}

pub fn start(tab: &RepoTab, bad: &str, good: &str) -> Dialog {
    let short_of = |rev: &str| match tab.repo.resolve(rev) {
        Ok(id) if rev.len() == 40 => {
            // Prefer a branch or tag name over a hash.
            let name = crate::ui::banner::name_of(tab, id);
            if name == short(id) {
                rev.to_owned()
            } else {
                name
            }
        }
        _ => rev.to_owned(),
    };
    Dialog::Bisect(BisectDialog {
        bad: short_of(bad),
        good: if good.is_empty() {
            String::new()
        } else {
            short_of(good)
        },
        count: None,
    })
}

fn commit_line(tab: &RepoTab, rev: &str) -> Option<(Oid, String)> {
    let id = tab.repo.resolve(rev.trim()).ok()?;
    let c = tab.repo.find_commit_info(id).ok()?;
    Some((id, format!("{} {}", c.short_id(), c.summary)))
}

fn refs(tab: &RepoTab) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push("HEAD".into());
    v.extend(tab.branches.iter().map(|b| b.name.clone()));
    v.extend(tab.tags.iter().map(|t| t.name.clone()));
    v
}

pub fn show_dialog(
    ui: &mut egui::Ui,
    d: &mut BisectDialog,
    tab: Option<&RepoTab>,
    cmds: &mut Commands,
    enter: bool,
) -> bool {
    let Some(tab) = tab else { return false };
    ui.heading(t!("Start bisect"));
    ui.label(t!(
        "Git checks out commits between a good and a bad one; mark each as good or bad until the commit that introduced the problem is found."
    ));
    ui.add_space(6.0);
    let items = refs(tab);
    let row = |ui: &mut egui::Ui, label: String, salt: &str, value: &mut String, m: Mark| {
        ui.horizontal(|ui| {
            ui.colored_label(mark_color(m, ui.visuals().dark_mode), "•");
            ui.label(label);
            crate::ui::filter_combo::filter_combo(ui, salt, value, &items, 280.0, true);
        });
        match commit_line(tab, value) {
            Some((id, text)) => {
                ui.weak(text);
                Some(id)
            }
            None => {
                if !value.trim().is_empty() {
                    ui.colored_label(ui.visuals().warn_fg_color, t!("Unknown revision"));
                }
                None
            }
        }
    };
    let bad = row(
        ui,
        t!("Bad (has the problem)"),
        "bisect-bad",
        &mut d.bad,
        Mark::Bad,
    );
    ui.add_space(4.0);
    let good = row(
        ui,
        t!("Good (before the problem)"),
        "bisect-good",
        &mut d.good,
        Mark::Good,
    );
    let mut ok = bad.is_some() && good.is_some();
    if let (Some(b), Some(g)) = (bad, good) {
        if b == g || !tab.repo.is_ancestor(g, b) {
            ok = false;
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("The good commit must be an ancestor of the bad one."),
            );
        } else {
            if d.count.is_none_or(|(k, _)| k != (g, b)) {
                let n = tab.repo.bisect_candidates(g, b).unwrap_or(0);
                d.count = Some(((g, b), n));
            }
            let n = d.count.map_or(0, |c| c.1);
            ui.label(t!(
                "{} commits to search, about {} steps.",
                n,
                bisect::steps_for(n)
            ));
        }
    }
    if !tab.status.is_empty() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t!("You have local changes: commit or stash them first, as bisect checks out other commits."),
        );
    }
    let r = buttons(ui, &t!("Start bisect"), ok, false, enter);
    if r == Some(true) {
        cmds.run(Op::Bisect(BisectOp::Start {
            bad: d.bad.trim().to_owned(),
            good: vec![d.good.trim().to_owned()],
        }));
    }
    r.is_none()
}

// ----- Banner -----

fn link(ui: &mut egui::Ui, tab: &RepoTab, id: Oid, text: RichText, cmds: &mut Commands) {
    if ui
        .link(text)
        .on_hover_text(t!("Show this commit"))
        .clicked()
    {
        cmds.push(if tab.index.contains_key(&id) {
            Command::SelectCommit(id)
        } else {
            Command::ShowCommit(id)
        });
    }
}

fn commit_text(tab: &RepoTab, id: Oid) -> String {
    match tab.repo.find_commit_info(id) {
        Ok(c) => {
            let mut s: String = c.summary.chars().take(50).collect();
            if c.summary.chars().count() > 50 {
                s.push('…');
            }
            format!("{} {}", c.short_id(), s)
        }
        Err(_) => short(id),
    }
}

/// The banner while bisecting: what is tested, the marks so far as a
/// timeline, the steps left and the mark buttons.
pub fn banner(ui: &mut egui::Ui, tab: &RepoTab, cmds: &mut Commands) {
    let dark = ui.visuals().dark_mode;
    let fill = ui.visuals().warn_fg_color.gamma_multiply(0.25);
    let empty = BisectStatus::default();
    let st = tab.bisect.as_ref().unwrap_or(&empty);
    egui::Frame::new()
        .fill(fill)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("⚠");
                ui.strong(t!("Bisecting"));
                if let Some(first) = st.first_bad {
                    ui.label(t!("— first bad commit:"));
                    let text = RichText::new(commit_text(tab, first))
                        .strong()
                        .color(mark_color(Mark::Bad, dark));
                    link(ui, tab, first, text, cmds);
                } else if st.bad.is_none() || st.good.is_empty() {
                    ui.label(if st.bad.is_none() {
                        t!("— mark a bad commit (one with the problem) from the history menu.")
                    } else {
                        t!("— mark a good commit (one without the problem) from the history menu.")
                    });
                } else if st.only_skipped {
                    ui.label(t!(
                        "— only skipped commits are left: the first bad commit cannot be told apart."
                    ));
                } else if let Some(cur) = st.current {
                    ui.label(t!("— testing"));
                    link(ui, tab, cur, RichText::new(commit_text(tab, cur)).strong(), cmds);
                    let left = st.candidates.saturating_sub(1);
                    ui.weak(if st.steps_left == 1 {
                        t!("({} revisions left, about 1 step)", left)
                    } else {
                        t!("({} revisions left, about {} steps)", left, st.steps_left)
                    });
                }
                timeline(ui, tab, st, cmds);
                ui.add_enabled_ui(!tab.busy(), |ui| {
                    if let Some(cur) = st.current.filter(|_| st.first_bad.is_none()) {
                        let sha = cur.to_string();
                        for (m, text) in [
                            (Mark::Good, t!("✔ Good")),
                            (Mark::Bad, t!("✖ Bad")),
                            (Mark::Skip, t!("Skip")),
                        ] {
                            let b = egui::Button::new(RichText::new(text).color(mark_color(m, dark)));
                            if ui
                                .add(b)
                                .on_hover_text(t!("Mark the checked-out commit as {}", mark_word(m)))
                                .clicked()
                            {
                                cmds.run(Op::Bisect(BisectOp::Mark(m, sha.clone())));
                            }
                        }
                    }
                    if ui
                        .button(t!("Reset"))
                        .on_hover_text(t!("End the bisect and go back to where it started"))
                        .clicked()
                    {
                        cmds.run(Op::Bisect(BisectOp::Reset));
                    }
                });
            });
        });
}

/// One dot per mark made, then hollow dots for the steps left.
fn timeline(ui: &mut egui::Ui, tab: &RepoTab, st: &BisectStatus, cmds: &mut Commands) {
    let dark = ui.visuals().dark_mode;
    let done = st.history.len();
    let left = if st.first_bad.is_some() || st.only_skipped {
        0
    } else {
        st.steps_left as usize
    };
    let n = done + left;
    if n == 0 {
        return;
    }
    const STEP: f32 = 16.0;
    let (rect, _) = ui.allocate_exact_size(vec2(STEP * n as f32 + 4.0, 18.0), egui::Sense::hover());
    let y = rect.center().y;
    let x = |i: usize| rect.left() + 8.0 + STEP * i as f32;
    let weak = ui.visuals().weak_text_color();
    ui.painter()
        .line_segment([pos2(x(0), y), pos2(x(n - 1), y)], Stroke::new(1.0, weak));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    for (i, (m, id)) in st.history.iter().enumerate() {
        let c = pos2(x(i), y);
        ui.painter().circle_filled(c, 5.0, mark_color(*m, dark));
        let hit = Rect::from_center_size(c, vec2(STEP, 18.0));
        let resp = ui.interact(
            hit,
            ui.scope_id().with(("bisect-dot", i)),
            egui::Sense::click(),
        );
        if pointer.is_some_and(|p| hit.contains(p)) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let resp = resp.on_hover_text(format!("{}: {}", mark_word(*m), commit_text(tab, *id)));
        if resp.clicked() {
            cmds.push(Command::SelectCommit(*id));
        }
    }
    for i in done..n {
        ui.painter()
            .circle_stroke(pos2(x(i), y), 4.5, Stroke::new(1.5, weak));
    }
}

/// Dev aid for `FERRUM_START`: `bisect:start=BAD,GOOD` (runs it) or
/// `bisect:dialog`.
pub fn dev_step(arg: &str, tab: Option<&RepoTab>) -> Vec<Command> {
    let Some(t) = tab else { return Vec::new() };
    match arg.split_once('=') {
        Some(("start", revs)) => {
            let (bad, good) = revs.split_once(',').unwrap_or((revs, ""));
            vec![Command::Run(Op::Bisect(BisectOp::Start {
                bad: bad.into(),
                good: vec![good.into()],
            }))]
        }
        _ => vec![Command::Dialog(start(t, "HEAD", ""))],
    }
}
