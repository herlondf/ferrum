//! Command palette (Ctrl+P): every action, searchable by name.

use eframe::egui;
use ferrum_core::BranchKind;

use crate::command::{Command, Commands, Op};
use crate::settings::Settings;
use crate::tab::{RepoTab, View};
use crate::ui::dialogs::Dialog;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    query: String,
    selected: usize,
    just_opened: bool,
}

impl Palette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.selected = 0;
        self.just_opened = self.open;
    }
}

type Make = Box<dyn Fn() -> Command>;

struct Entry {
    label: String,
    shortcut: &'static str,
    make: Make,
}

fn entry(
    label: impl Into<String>,
    shortcut: &'static str,
    make: impl Fn() -> Command + 'static,
) -> Entry {
    Entry {
        label: label.into(),
        shortcut,
        make: Box::new(make),
    }
}

fn entries(tab: Option<&RepoTab>, tab_names: &[String], settings: &Settings) -> Vec<Entry> {
    let mut list = vec![
        entry("Open repository…", "Ctrl+O", || Command::OpenRepoDialog),
        entry("Clone repository…", "", || {
            Command::Dialog(Dialog::Clone {
                url: String::new(),
                parent: String::new(),
                name: String::new(),
            })
        }),
        entry("New repository…", "", || {
            Command::Dialog(Dialog::Init {
                path: String::new(),
            })
        }),
        entry("Settings", "Ctrl+,", || Command::ShowSettings),
    ];
    if let Some(t) = tab {
        let pull = settings.pull.mode();
        list.extend([
            entry("Show history", "Ctrl+1", || Command::SetView(View::History)),
            entry("Show local changes", "Ctrl+2", || {
                Command::SetView(View::Changes)
            }),
            entry("Search commits", "Ctrl+F", || Command::FocusSearch),
            entry("Refresh", "F5", || Command::Refresh),
            entry("Undo last action", "Ctrl+Z", || Command::Undo),
            entry("Fetch all remotes", "Ctrl+Shift+F", || {
                Command::Run(Op::Fetch { remote: None })
            }),
            entry("Pull", "Ctrl+Shift+D", move || Command::Run(Op::Pull(pull))),
            entry("Pull with options…", "", || Command::Dialog(Dialog::Pull)),
            entry("Stage all changes", "", || Command::Run(Op::StageAll)),
            entry("Unstage all changes", "", || Command::Run(Op::UnstageAll)),
            entry("Stash changes…", "Ctrl+Shift+S", || {
                Command::Dialog(Dialog::stash())
            }),
            entry("Add remote…", "", || {
                Command::Dialog(Dialog::AddRemote {
                    name: String::new(),
                    url: String::new(),
                })
            }),
        ]);
        let head = t.head.clone().unwrap_or_else(|| "HEAD".into());
        let h2 = head.clone();
        list.push(entry("New branch…", "Ctrl+B", move || {
            Command::Dialog(Dialog::create_branch(&h2, &h2))
        }));
        if let Some(b) = t.head.clone() {
            let push = Dialog::push(t, &b);
            if let Dialog::Push {
                remote,
                branch,
                set_upstream,
                ..
            } = push
            {
                list.push(entry("Push…", "Ctrl+Shift+U", move || {
                    Command::Dialog(Dialog::Push {
                        remote: remote.clone(),
                        branch: branch.clone(),
                        set_upstream,
                        force: false,
                        tags: false,
                    })
                }));
            }
        }
        if let Some(s) = t.stashes.first().cloned() {
            list.push(entry(
                format!("Pop latest stash ({})", s.message),
                "",
                move || {
                    Command::Run(Op::StashApply {
                        stash: s.clone(),
                        pop: true,
                    })
                },
            ));
        }
        let id = t.identity.clone();
        list.push(entry("Set repository identity…", "", move || {
            Command::Dialog(Dialog::identity(&id))
        }));
        if t.hosted.is_some() {
            list.push(entry("Show pull requests", "", || {
                Command::SetView(View::PullRequests)
            }));
        }
        if t.uses_lfs {
            list.push(entry("Git LFS: pull", "", || Command::Run(Op::Lfs("pull"))));
            list.push(entry("Git LFS: fetch", "", || {
                Command::Run(Op::Lfs("fetch"))
            }));
        }
        if !t.submodules.is_empty() {
            list.push(entry("Update submodules", "", || {
                Command::Run(Op::SubmoduleUpdate)
            }));
        }
        let wt = Dialog::worktree_add(t);
        if let Dialog::WorktreeAdd { path, rev, .. } = wt {
            list.push(entry("Add worktree…", "", move || {
                Command::Dialog(Dialog::WorktreeAdd {
                    path: path.clone(),
                    rev: rev.clone(),
                    new_branch: String::new(),
                })
            }));
        }
        if t.head_oid.is_some() {
            list.push(entry("Interactive rebase of the last 10 commits…", "", {
                let base = t.repo.resolve("HEAD~10").ok();
                move || Command::PlanRebase(base)
            }));
        }
        for b in t
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local && !b.is_head)
        {
            let name = b.name.clone();
            list.push(entry(format!("Check out {name}"), "", move || {
                Command::Run(Op::Checkout(name.clone()))
            }));
        }
    }
    for (i, name) in tab_names.iter().enumerate() {
        list.push(entry(format!("Switch to {name}"), "Ctrl+Tab", move || {
            Command::SelectTab(i)
        }));
    }
    list
}

/// Characters of `query` appear in `text` in order (case-insensitive).
fn fuzzy_score(text: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let text = text.to_lowercase();
    if let Some(pos) = text.find(query) {
        return Some(1000 - pos as i32);
    }
    let mut chars = text.chars();
    for q in query.chars() {
        chars.find(|&c| c == q)?;
    }
    Some(0)
}

pub fn show(
    ctx: &egui::Context,
    p: &mut Palette,
    tab: Option<&RepoTab>,
    tab_names: &[String],
    settings: &Settings,
    cmds: &mut Commands,
) {
    if !p.open {
        return;
    }
    let all = entries(tab, tab_names, settings);
    let q = p.query.trim().to_lowercase();
    let mut matches: Vec<(i32, &Entry)> = all
        .iter()
        .filter_map(|e| fuzzy_score(&e.label, &q).map(|s| (s, e)))
        .collect();
    matches.sort_by_key(|m| std::cmp::Reverse(m.0));
    p.selected = p.selected.min(matches.len().saturating_sub(1));

    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if up {
        p.selected = p.selected.saturating_sub(1);
    }
    if down && p.selected + 1 < matches.len() {
        p.selected += 1;
    }

    let mut run = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::unique("palette"))
        .fixed_pos(egui::pos2(screen.center().x - 260.0, screen.top() + 80.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(520.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut p.query)
                        .hint_text("Type a command…")
                        .desired_width(f32::INFINITY),
                );
                if p.just_opened {
                    resp.request_focus();
                    p.just_opened = false;
                }
                if resp.changed() {
                    p.selected = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // Leave room for the scroll bar.
                        let width = ui.available_width() - 12.0;
                        for (i, (_, e)) in matches.iter().enumerate() {
                            let (rect, resp) = ui
                                .allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
                            let selected = i == p.selected;
                            let visuals = ui.visuals();
                            if selected {
                                ui.painter()
                                    .rect_filled(rect, 4.0, visuals.selection.bg_fill);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    4.0,
                                    visuals.widgets.hovered.weak_bg_fill,
                                );
                            }
                            let color = if selected {
                                visuals.selection.stroke.color
                            } else {
                                visuals.text_color()
                            };
                            let label_area = rect.with_max_x(rect.right() - 110.0);
                            ui.painter().with_clip_rect(label_area).text(
                                rect.left_center() + egui::vec2(8.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                &e.label,
                                egui::FontId::proportional(13.5),
                                color,
                            );
                            if !e.shortcut.is_empty() {
                                ui.painter().text(
                                    rect.right_center() - egui::vec2(8.0, 0.0),
                                    egui::Align2::RIGHT_CENTER,
                                    e.shortcut,
                                    egui::FontId::proportional(12.0),
                                    visuals.weak_text_color(),
                                );
                            }
                            if selected && (up || down) {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() {
                                run = Some(i);
                            }
                        }
                        if matches.is_empty() {
                            ui.weak("No matching command.");
                        }
                    });
            });
        });
    if enter && !matches.is_empty() {
        run = Some(p.selected);
    }
    if let Some(i) = run {
        cmds.push((matches[i].1.make)());
        p.open = false;
    }
    if esc {
        p.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::fuzzy_score;

    #[test]
    fn fuzzy_matching() {
        assert!(fuzzy_score("Fetch all remotes", "fetch").is_some());
        assert!(fuzzy_score("Fetch all remotes", "far").is_some());
        assert!(fuzzy_score("Fetch all remotes", "xyz").is_none());
        assert!(fuzzy_score("Pull", "pu") > fuzzy_score("Push upstream", "pl"));
    }
}
