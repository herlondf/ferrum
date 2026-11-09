//! The Repository Manager: the start screen and the "new tab" (Ctrl+T).
//! Large Open / Clone / New actions, a search field, and the known
//! repositories as cards or a list (recent first, then by group), with
//! workspaces and source folders on the left. The dashboard bar above them
//! (`dashboard.rs`) filters them and fetches or pulls them all.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Key, Rect, RichText, Sense, vec2};
use ferrum_core::glance::Glance;

use crate::command::{Command, Commands};
use crate::external::Launch;
use crate::icons;
use crate::repos::{Library, RepoCmd, SourceFolder};
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::dashboard::{self, Filter};
use crate::ui::dialogs::Dialog;
use crate::ui::palette::{Entry, Group};

/// How long a card's branch, time and changes dot stay fresh.
const GLANCE_TTL: Duration = Duration::from_secs(20);
const CARD_MIN_WIDTH: f32 = 270.0;
const CARD_HEIGHT: f32 = 84.0;
const CARD_GAP: f32 = 10.0;
/// Width of the centered content.
const CONTENT_WIDTH: f32 = 980.0;

#[derive(Default)]
pub struct Manager {
    filter: String,
    pub selected: BTreeSet<PathBuf>,
    anchor: Option<PathBuf>,
    /// Inline rename (F2): the repository and the name being typed.
    renaming: Option<(PathBuf, String)>,
    focus_rename: bool,
    new_folder: String,
    new_workspace: String,
    /// Rows in the order shown, for Shift+click and the arrow keys.
    order: Vec<PathBuf>,
    /// Branch, last commit and changes of each repository (see `glance`).
    glances: HashMap<PathBuf, Glance>,
    /// Repositories whose glance could not be read, with the error.
    errors: HashMap<PathBuf, String>,
    loading: Option<Receiver<(PathBuf, Result<Glance, String>)>>,
    loaded_at: Option<Instant>,
    /// Repositories asked for since `loaded_at`.
    requested: std::collections::HashSet<PathBuf>,
    /// Frame the manager was last drawn in: the search field gets the focus
    /// when it comes back into view.
    last_pass: Option<u64>,
    /// Filters and "Fetch all" / "Pull all".
    pub dash: dashboard::Dash,
    /// An app-wide job runs (no batch can start).
    pub busy: bool,
}

impl Manager {
    #[cfg(test)]
    pub fn glance(&self, path: &Path) -> Option<&Glance> {
        self.glances.get(path)
    }

    /// Like `glance`, comparing real paths: tests use temp folders that are
    /// links (macOS /var) or spelled differently (Windows) from the keys.
    #[cfg(test)]
    pub fn glance_at(&self, path: &Path) -> Option<&Glance> {
        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let want = real(path);
        self.glances
            .iter()
            .find(|(k, _)| real(k) == want)
            .map(|(_, g)| g)
    }
}

/// Keyboard and context-menu actions on the selection.
enum Action {
    Open(Vec<PathBuf>),
    Remove(Vec<PathBuf>),
    Rename(PathBuf),
}

/// Draws the manager. `open` are the paths of open tabs; `scanning` is true
/// while the source folders are being scanned.
pub fn show(
    ui: &mut egui::Ui,
    m: &mut Manager,
    settings: &mut Settings,
    open: &[PathBuf],
    scanning: bool,
    cmds: &mut Commands,
) {
    import_recent(settings);
    load_glances(ui.ctx(), m, &settings.library);
    dashboard::take_glances(&m.dash, &mut m.glances);
    let pass = ui.ctx().cumulative_pass_nr();
    let focus_search = m.last_pass.is_none_or(|p| p + 1 < pass);
    m.last_pass = Some(pass);

    let mut action = None;
    egui::Panel::left("manager-side")
        .resizable(true)
        .default_size(250.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(theme::SPACE_S);
                workspaces(ui, m, &mut settings.library, cmds);
                ui.add_space(theme::SPACE_XL);
                sources(ui, &mut settings.library, scanning, cmds);
            });
        });
    egui::CentralPanel::default_margins().show(ui, |ui| {
        let area = ui.max_rect();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let width = ui.available_width().min(CONTENT_WIDTH);
                let side = ((ui.available_width() - width) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(side);
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        hero(ui, settings, cmds);
                        let search = search_field(ui, m, focus_search, cmds);
                        if let Some(a) = search_keys(ui, m, &search) {
                            action = Some(a);
                        }
                        ui.add_space(theme::SPACE_M);
                        if !settings.library.repos.is_empty() {
                            dash_bar(ui, m, &mut settings.library, cmds);
                        }
                        ui.add_space(theme::SPACE_M);
                        let recent = settings.recent.clone();
                        let lists = repo_list(ui, m, &mut settings.library, open, &recent, cmds);
                        action = action.take().or(lists);
                        ui.add_space(theme::SPACE_XL);
                    });
                });
            });
        drop_overlay(ui, area);
    });
    if action.is_none() {
        action = keys(ui, m);
    }
    match action {
        Some(Action::Open(paths)) => {
            for p in paths {
                cmds.push(Command::OpenRepo(p));
            }
        }
        Some(Action::Remove(paths)) => {
            settings.library.remove(&paths);
            m.selected.clear();
        }
        Some(Action::Rename(p)) => {
            let name = settings
                .library
                .custom_name(&p)
                .unwrap_or_default()
                .to_owned();
            m.renaming = Some((p, name));
            m.focus_rename = true;
        }
        None => {}
    }
}

/// Copies the recent list in once, so the manager is not empty after an update.
fn import_recent(settings: &mut Settings) {
    if !settings.library.imported_recent {
        settings.library.imported_recent = true;
        for p in settings.recent.clone().into_iter().rev() {
            settings.library.remember(&p);
        }
    }
}

/// Reads branch, last commit and changes of every listed repository on a
/// thread, again when they are older than `GLANCE_TTL`.
fn load_glances(ctx: &egui::Context, m: &mut Manager, lib: &Library) {
    if let Some(rx) = &m.loading {
        loop {
            match rx.try_recv() {
                Ok((p, Ok(g))) => {
                    m.errors.remove(&p);
                    m.glances.insert(p, g);
                }
                Ok((p, Err(e))) => {
                    m.glances.remove(&p);
                    m.errors.insert(p, e);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    m.loading = None;
                    break;
                }
            }
        }
    }
    if m.loading.is_some() {
        return;
    }
    let mut paths: Vec<PathBuf> = lib.repos.iter().map(|r| r.path.clone()).collect();
    paths.extend(lib.unlisted_found());
    if m.loaded_at.is_some_and(|t| t.elapsed() < GLANCE_TTL) {
        // Fresh enough: only repositories added since.
        paths.retain(|p| !m.requested.contains(p));
        if paths.is_empty() {
            return;
        }
    } else {
        m.requested.clear();
        m.loaded_at = Some(Instant::now());
    }
    m.requested.extend(paths.iter().cloned());
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        for p in paths.into_iter().filter(|p| p.exists()) {
            let g = ferrum_core::glance::glance(&p).map_err(|e| e.to_string());
            if tx.send((p, g)).is_err() {
                return;
            }
        }
        ctx.request_repaint();
    });
    m.loading = Some(rx);
}

// ----- Header -----

/// Title and the large Open / Clone / New actions.
fn hero(ui: &mut egui::Ui, settings: &Settings, cmds: &mut Commands) {
    ui.add_space(theme::SPACE_XL);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Ferrum")
                .font(theme::strong_font(26.0))
                .color(ui.visuals().strong_text_color()),
        );
        ui.add_space(theme::SPACE_S);
        ui.label(RichText::new(t!("A fast Git client")).size(14.0).weak());
    });
    ui.add_space(theme::SPACE_L);
    let tiles = [
        (
            icons::FOLDER_OPEN,
            t!("Open"),
            t!("A repository on this computer"),
            Command::OpenRepoDialog,
        ),
        (
            icons::DOWNLOAD_SIMPLE,
            t!("Clone"),
            t!("Copy a repository from a URL"),
            Command::Dialog(Dialog::clone_repo(settings)),
        ),
        (
            icons::FOLDER_SIMPLE_PLUS,
            t!("New"),
            t!("Start an empty repository"),
            Command::Dialog(Dialog::Init {
                path: String::new(),
            }),
        ),
    ];
    let gap = CARD_GAP;
    let width = (ui.available_width() - gap * 2.0) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (icon, title, text, cmd) in tiles {
            if action_tile(ui, width, icon, &title, &text).clicked() {
                cmds.push(cmd);
            }
        }
    });
    ui.add_space(theme::SPACE_L);
}

/// A large button: an icon in an accent circle, a title and a line of text.
fn action_tile(
    ui: &mut egui::Ui,
    width: f32,
    icon: &str,
    title: &str,
    text: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 64.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let v = ui.visuals();
    let accent = theme::accent(ui);
    let (fill, stroke) = if resp.hovered() {
        (
            v.widgets.hovered.weak_bg_fill,
            egui::Stroke::new(1.0, accent),
        )
    } else {
        (v.faint_bg_color, v.widgets.noninteractive.bg_stroke)
    };
    let p = ui.painter();
    p.rect(
        rect,
        theme::RADIUS_LARGE,
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let c = egui::pos2(rect.left() + 32.0, rect.center().y);
    p.circle_filled(c, 18.0, tint(accent, if v.dark_mode { 48 } else { 36 }));
    p.text(
        c,
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(18.0),
        accent,
    );
    let x = rect.left() + 60.0;
    let clip = p.with_clip_rect(rect.shrink(4.0));
    clip.text(
        egui::pos2(x, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        theme::strong_font(14.5),
        v.strong_text_color(),
    );
    clip.text(
        egui::pos2(x, rect.center().y + 10.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(12.0),
        v.weak_text_color(),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// The search field, with Open Quickly at its end.
fn search_field(
    ui: &mut egui::Ui,
    m: &mut Manager,
    focus: bool,
    cmds: &mut Commands,
) -> egui::Response {
    let height = 36.0;
    let frame = egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.inactive.bg_stroke)
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(egui::Margin::symmetric(12, 0));
    frame
        .show(ui, |ui| {
            ui.set_height(height);
            ui.horizontal_centered(|ui| {
                ui.label(
                    RichText::new(icons::SEARCH)
                        .size(16.0)
                        .color(ui.visuals().weak_text_color()),
                );
                let quick = t!("Open Quickly…");
                let quick_w = 150.0;
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut m.filter)
                        .hint_text(t!("Filter repositories"))
                        .font(egui::FontId::proportional(14.5))
                        .frame(egui::Frame::NONE)
                        .desired_width((ui.available_width() - quick_w).max(80.0)),
                );
                if focus {
                    resp.request_focus();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = egui::Button::new(
                        RichText::new(icons::text(icons::ARROW_SQUARE_OUT, &quick)).size(12.5),
                    )
                    .frame_when_inactive(false);
                    if ui.add(b).on_hover_text("Ctrl+Shift+O").clicked() {
                        cmds.push(Command::Repos(RepoCmd::OpenQuickly));
                    }
                });
                resp
            })
            .inner
        })
        .inner
}

/// Keys in the search field: the arrows move the selection, Enter opens it
/// (or the first match) and Escape clears the text.
fn search_keys(ui: &egui::Ui, m: &mut Manager, search: &egui::Response) -> Option<Action> {
    let ctx = ui.ctx();
    if !search.has_focus() && !search.lost_focus() {
        return None;
    }
    let (lost, focused) = (search.lost_focus(), search.has_focus());
    let (up, down, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            lost && i.key_pressed(Key::Enter),
            focused && i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    if esc {
        m.filter.clear();
    }
    if up || down {
        let cur = m
            .anchor
            .as_ref()
            .and_then(|a| m.order.iter().position(|o| o == a));
        let next = match cur {
            Some(i) if up => i.saturating_sub(1),
            Some(i) => (i + 1).min(m.order.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(p) = m.order.get(next).cloned() {
            m.selected = BTreeSet::from([p.clone()]);
            m.anchor = Some(p);
        }
    }
    if enter {
        let pick = m
            .selected
            .iter()
            .find(|p| m.order.contains(p))
            .or(m.order.first())
            .cloned();
        if let Some(p) = pick.filter(|p| p.exists()) {
            return Some(Action::Open(vec![p]));
        }
    }
    None
}

/// While a folder is dragged over the window: a large "drop to open" area.
fn drop_overlay(ui: &egui::Ui, area: Rect) {
    let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
    if !hovering {
        return;
    }
    let accent = theme::accent(ui);
    let p = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("manager-drop"),
    ));
    let rect = area.shrink(8.0);
    p.rect_filled(
        rect,
        theme::RADIUS_LARGE,
        tint(ui.visuals().panel_fill, 230),
    );
    dashed_rect(&p, rect, egui::Stroke::new(2.0, accent));
    p.text(
        rect.center() - vec2(0.0, 18.0),
        egui::Align2::CENTER_CENTER,
        icons::FOLDER_OPEN,
        egui::FontId::proportional(40.0),
        accent,
    );
    p.text(
        rect.center() + vec2(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        t!("Drop the folder to open it"),
        theme::strong_font(16.0),
        ui.visuals().strong_text_color(),
    );
}

fn dashed_rect(p: &egui::Painter, r: Rect, stroke: egui::Stroke) {
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
    ];
    for i in 0..4 {
        let line = [corners[i], corners[(i + 1) % 4]];
        p.extend(egui::Shape::dashed_line(&line, stroke, 8.0, 6.0));
    }
}

// ----- Lists -----

fn matches(lib: &Library, filter: &str, p: &Path) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty()
        || p.to_string_lossy().to_lowercase().contains(&f)
        || lib
            .custom_name(p)
            .is_some_and(|n| n.to_lowercase().contains(&f))
}

/// A path without the trailing separator git adds to working folders.
pub fn shown_path(p: &Path) -> String {
    let text = p.display().to_string();
    match text.trim_end_matches(['/', '\\']) {
        "" => text,
        t => t.to_owned(),
    }
}

pub fn display_name(lib: &Library, p: &Path) -> String {
    lib.custom_name(p)
        .map(str::to_owned)
        .or_else(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| p.display().to_string())
}

/// A path with the home folder written as `~`.
fn short_path(p: &Path) -> String {
    let full = shown_path(p);
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| h.to_string_lossy().into_owned())
        .filter(|h| !h.is_empty());
    match home {
        Some(h) if full.starts_with(&h) => format!("~{}", &full[h.len()..]),
        _ => full,
    }
}

/// Paths in the order shown: recently opened first (newest first), then
/// the others as listed.
pub(crate) fn recent_first(paths: &mut [PathBuf], recent: &[PathBuf]) {
    paths.sort_by_key(|p| recent.iter().position(|r| r == p).unwrap_or(usize::MAX));
}

/// A section title: an icon, the name and a count.
fn section_title(ui: &mut egui::Ui, icon: &str, title: &str, count: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S + 2.0;
        ui.label(RichText::new(icon).color(ui.visuals().weak_text_color()));
        ui.label(theme::strong(title).size(14.0));
        crate::ui::changes_rows::count_pill(ui, count);
    });
    ui.add_space(theme::SPACE_S);
}

fn repo_list(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    open: &[PathBuf],
    recent: &[PathBuf],
    cmds: &mut Commands,
) -> Option<Action> {
    let mut action = None;
    m.order.clear();
    let missing = lib.missing();
    if !missing.is_empty() {
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t!("⚠ {} repositories no longer exist.", missing.len()),
            );
            if ui.small_button(t!("Remove missing")).clicked() {
                action = Some(Action::Remove(missing.clone()));
            }
        });
        ui.add_space(theme::SPACE_M);
    }
    if lib.repos.is_empty() && lib.found.is_empty() {
        empty_state(ui);
        return action;
    }
    let mut groups: Vec<String> = vec![String::new()];
    groups.extend(lib.folder_names());
    let folder_names = lib.folder_names();
    let workspaces = lib.workspace_names();
    let mut shown_any = false;
    for g in groups {
        let mut paths: Vec<PathBuf> = lib
            .repos
            .iter()
            .filter(|r| r.folder == g && shown(m, lib, &r.path))
            .map(|r| r.path.clone())
            .collect();
        recent_first(&mut paths, recent);
        shown_any |= !paths.is_empty();
        let menu = RowMenu {
            folders: &folder_names,
            workspaces: &workspaces,
        };
        if g.is_empty() {
            if !paths.is_empty() {
                section_title(ui, icons::REPOSITORIES, &t!("Repositories"), paths.len());
                if let Some(a) = grid(ui, m, lib, &paths, open, &menu, cmds) {
                    action = Some(a);
                }
                ui.add_space(theme::SPACE_L);
            }
        } else {
            let id = ui.make_persistent_id(("group", &g));
            let title = RichText::new(icons::text(
                icons::FOLDER,
                format!("{g}  ({})", paths.len()),
            ))
            .font(theme::strong_font(14.0));
            let resp = egui::CollapsingHeader::new(title)
                .id_salt(id)
                .default_open(true)
                .show(ui, |ui| {
                    if paths.is_empty() {
                        ui.weak(t!("Empty. Move repositories here from their menu."));
                    } else if let Some(a) = grid(ui, m, lib, &paths, open, &menu, cmds) {
                        action = Some(a);
                    }
                });
            resp.header_response.context_menu(|ui| {
                let mut name = g.clone();
                ui.label(t!("Rename group"));
                if ui.text_edit_singleline(&mut name).changed() && !name.trim().is_empty() {
                    lib.rename_folder(&g, &name);
                }
                if ui.button(t!("Delete group")).clicked() {
                    lib.delete_folder(&g);
                    ui.close();
                }
            });
            ui.add_space(theme::SPACE_M);
        }
    }
    let hidden = lib
        .repos
        .iter()
        .filter(|r| matches(lib, &m.filter, &r.path) && !shown(m, lib, &r.path))
        .count();
    dashboard::hidden_hint(ui, &mut m.dash, hidden);
    if !shown_any && !m.filter.trim().is_empty() {
        ui.add_space(theme::SPACE_L);
        ui.vertical_centered(|ui| {
            ui.weak(t!("No repository matches “{}”.", m.filter.trim()));
        });
        ui.add_space(theme::SPACE_L);
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut m.new_folder)
                .hint_text(t!("New group"))
                .desired_width(160.0),
        );
        let name = m.new_folder.trim().to_owned();
        if ui
            .add_enabled(!name.is_empty(), icons::button(icons::ADD, t!("Add group")))
            .clicked()
        {
            if !lib.folders.contains(&name) {
                lib.folders.push(name);
            }
            m.new_folder.clear();
        }
    });

    let found: Vec<PathBuf> = lib
        .unlisted_found()
        .into_iter()
        .filter(|p| matches(lib, &m.filter, p))
        .collect();
    if !found.is_empty() {
        ui.add_space(theme::SPACE_XL);
        ui.horizontal(|ui| {
            section_title(
                ui,
                icons::MAGNIFYING_GLASS,
                &t!("Found in source folders"),
                found.len(),
            );
            if ui.small_button(t!("Add all to the list")).clicked() {
                for p in &found {
                    lib.remember(p);
                }
            }
        });
        for p in &found {
            ui.horizontal(|ui| {
                let resp = ui
                    .add(egui::Button::selectable(false, display_name(lib, p)))
                    .on_hover_text(t!("Double-click to open"));
                ui.weak(shown_path(p));
                if resp.double_clicked() {
                    cmds.push(Command::OpenRepo(p.clone()));
                }
                if ui.small_button(t!("Add to the list")).clicked() {
                    lib.remember(p);
                }
            });
        }
    }
    action
}

/// No repositories yet: what to do, and a place to drop a folder.
fn empty_state(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 220.0), Sense::hover());
    let v = ui.visuals();
    let p = ui.painter();
    p.rect_filled(rect, theme::RADIUS_LARGE, v.faint_bg_color);
    dashed_rect(
        p,
        rect.shrink(1.0),
        egui::Stroke::new(1.5, v.weak_text_color().gamma_multiply(0.5)),
    );
    let c = rect.center();
    p.circle_filled(c - vec2(0.0, 48.0), 28.0, tint(theme::accent(ui), 36));
    p.text(
        c - vec2(0.0, 48.0),
        egui::Align2::CENTER_CENTER,
        icons::FOLDER_OPEN,
        egui::FontId::proportional(28.0),
        theme::accent(ui),
    );
    p.text(
        c + vec2(0.0, 2.0),
        egui::Align2::CENTER_CENTER,
        t!("No repositories yet"),
        theme::strong_font(17.0),
        v.strong_text_color(),
    );
    p.text(
        c + vec2(0.0, 28.0),
        egui::Align2::CENTER_CENTER,
        t!("Open, clone or create one above, or drop a project folder anywhere on this window."),
        egui::FontId::proportional(13.0),
        v.weak_text_color(),
    );
    p.text(
        c + vec2(0.0, 52.0),
        egui::Align2::CENTER_CENTER,
        t!("Tip: add a source folder on the left to list all your repositories."),
        egui::FontId::proportional(12.0),
        v.weak_text_color(),
    );
}

struct RowMenu<'a> {
    folders: &'a [String],
    workspaces: &'a [String],
}

/// Repository cards in as many columns as fit.
fn grid(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    paths: &[PathBuf],
    open: &[PathBuf],
    menu: &RowMenu,
    cmds: &mut Commands,
) -> Option<Action> {
    if lib.list_view {
        return list(ui, m, lib, paths, open, menu, cmds);
    }
    let avail = ui.available_width();
    let cols = (((avail + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).floor() as usize).max(1);
    let width = (avail - CARD_GAP * (cols - 1) as f32) / cols as f32;
    let mut action = None;
    for chunk in paths.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = CARD_GAP;
            for p in chunk {
                if let Some(a) = card(ui, m, lib, p, width, open.contains(p), menu, cmds) {
                    action = Some(a);
                }
            }
        });
        ui.add_space(CARD_GAP - ui.spacing().item_spacing.y);
    }
    action
}

/// One repository: color, name, changes dot, path, branch and last commit.
#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    p: &Path,
    width: f32,
    is_open: bool,
    menu: &RowMenu,
    cmds: &mut Commands,
) -> Option<Action> {
    m.order.push(p.to_path_buf());
    let exists = p.exists();
    let name = display_name(lib, p);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
    let selected = m.selected.contains(p);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
    let glance = m.glances.get(p).cloned();
    if ui.is_rect_visible(rect) {
        let look = dashboard::look(&m.dash, &m.glances, &m.errors, p);
        paint_card(
            ui,
            &resp,
            rect,
            lib,
            p,
            &name,
            glance.as_ref(),
            &look,
            selected,
            is_open,
            exists,
        );
    }
    let name_rect = Rect::from_min_size(rect.min + vec2(14.0, 8.0), vec2(width - 28.0, 24.0));
    interact(ui, m, lib, p, &name, resp, name_rect, exists, menu, cmds)
}

/// Rename, click, double-click and the context menu of a card or row.
#[allow(clippy::too_many_arguments)]
fn interact(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    p: &Path,
    name: &str,
    resp: egui::Response,
    name_rect: Rect,
    exists: bool,
    menu: &RowMenu,
    cmds: &mut Commands,
) -> Option<Action> {
    let mut action = None;
    // Inline rename over the name.
    if let Some((rp, new)) = &mut m.renaming
        && rp == p
    {
        let r = name_rect;
        let edit = ui.put(
            r,
            egui::TextEdit::singleline(new)
                .hint_text(name)
                .font(theme::strong_font(14.0)),
        );
        if m.focus_rename {
            edit.request_focus();
            m.focus_rename = false;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            m.renaming = None;
        } else if edit.lost_focus() {
            lib.entry(p).name = new.trim().to_owned();
            // The Enter that ends the rename does not also open the repository.
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
            m.renaming = None;
        }
        return None;
    }
    let resp = resp.on_hover_text(shown_path(p));
    if resp.clicked() {
        click(ui, m, p);
    }
    if resp.double_clicked() && exists {
        action = Some(Action::Open(vec![p.to_path_buf()]));
    }
    if resp.secondary_clicked() && !m.selected.contains(p) {
        m.selected = BTreeSet::from([p.to_path_buf()]);
        m.anchor = Some(p.to_path_buf());
    }
    resp.context_menu(|ui| {
        if let Some(a) = card_menu(ui, m, lib, p, exists, menu, cmds) {
            action = Some(a);
        }
    });
    action
}

#[allow(clippy::too_many_arguments)]
fn paint_card(
    ui: &egui::Ui,
    resp: &egui::Response,
    rect: Rect,
    lib: &Library,
    p: &Path,
    name: &str,
    glance: Option<&Glance>,
    look: &dashboard::Look,
    selected: bool,
    is_open: bool,
    exists: bool,
) {
    let v = ui.visuals();
    let accent = theme::accent(ui);
    let (fill, stroke) = if selected {
        (v.selection.bg_fill, egui::Stroke::new(1.5, accent))
    } else if resp.hovered() {
        (v.widgets.hovered.weak_bg_fill, v.widgets.hovered.bg_stroke)
    } else {
        (v.faint_bg_color, v.widgets.noninteractive.bg_stroke)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        theme::RADIUS_LARGE,
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    // The repository color, as a bar on the left.
    if let Some(c) = lib.color(p) {
        let bar = Rect::from_min_size(rect.min + vec2(5.0, 10.0), vec2(4.0, rect.height() - 20.0));
        painter.rect_filled(bar, 2.0, crate::ui::tabs::color32(c));
    }
    let painter = painter.with_clip_rect(rect.shrink(2.0));
    let (strong, text, weak) = if selected {
        let c = v.selection.stroke.color;
        (c, c, c.gamma_multiply(0.75))
    } else {
        (v.strong_text_color(), v.text_color(), v.weak_text_color())
    };
    let left = rect.left() + 18.0;
    let right = rect.right() - 14.0;
    // Line 1: name, changes dot, and on the right "open" or "missing".
    let mut tag_left = right;
    let problem = look.problem();
    let tag = if !exists {
        Some((t!("missing"), v.warn_fg_color))
    } else if problem.is_some() {
        Some((t!("problem"), v.warn_fg_color))
    } else if is_open {
        Some((t!("open"), weak))
    } else {
        None
    };
    let y1 = rect.top() + 20.0;
    if let Some((t, c)) = tag {
        let g = painter.layout_no_wrap(t, theme::strong_font(10.5), c);
        let size = g.size() + vec2(12.0, 4.0);
        let r = Rect::from_min_size(egui::pos2(right - size.x, y1 - size.y / 2.0), size);
        painter.rect_filled(r, 8.0, tint(c, 30));
        painter.galley(r.center() - g.size() / 2.0, g, c);
        tag_left = r.left() - 8.0;
        if let Some(prob) = &problem {
            ui.interact(r, resp.id.with("problem"), Sense::hover())
                .on_hover_text(prob);
        }
    }
    let name_g = painter.layout(
        name.to_owned(),
        theme::strong_font(14.5),
        strong,
        f32::INFINITY,
    );
    let max_name = (tag_left - left - 18.0).max(20.0);
    let name_g = if name_g.size().x > max_name {
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.to_owned(),
            theme::strong_font(14.5),
            strong,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_name);
        ui.fonts_mut(|f| f.layout_job(job))
    } else {
        name_g
    };
    let name_w = name_g.size().x;
    painter.galley(egui::pos2(left, y1 - name_g.size().y / 2.0), name_g, strong);
    if glance.is_some_and(|g| g.dirty) {
        let dot = egui::pos2(left + name_w + 9.0, y1);
        painter.circle_filled(dot, 4.0, crate::util::modified_color(ui));
        let hit = Rect::from_center_size(dot, vec2(14.0, 14.0));
        ui.interact(hit, resp.id.with("dirty"), Sense::hover())
            .on_hover_text(t!("Uncommitted changes"));
    }
    // Line 2: the folder.
    let y2 = rect.top() + 41.0;
    let font = egui::FontId::proportional(11.5);
    let path = short_path(p);
    let fits = |t: &str| {
        painter
            .layout_no_wrap(t.to_owned(), font.clone(), weak)
            .size()
            .x
            <= right - left
    };
    // Long paths keep their end, where the repository is.
    let mut chars: Vec<char> = path.chars().collect();
    let mut shown = path.clone();
    while !fits(&shown) && chars.len() > 4 {
        chars.remove(0);
        shown = format!("…{}", chars.iter().collect::<String>());
    }
    let g = painter.layout_no_wrap(shown, font, weak);
    painter.galley(egui::pos2(left, y2 - g.size().y / 2.0), g, weak);
    // Line 3: branch and last commit.
    let y3 = rect.top() + 63.0;
    let Some(gl) = glance else {
        return;
    };
    let small = egui::FontId::proportional(12.0);
    let branch = gl.branch.clone().unwrap_or_else(|| t!("detached HEAD"));
    let mut x = left;
    let icon_g = painter.layout_no_wrap(icons::BRANCH.to_owned(), small.clone(), weak);
    let iw = icon_g.size().x;
    painter.galley(egui::pos2(x, y3 - icon_g.size().y / 2.0), icon_g, weak);
    x += iw + 5.0;
    let time = gl
        .last_commit
        .map(|t| crate::util::relative_time(t, crate::util::now_secs()));
    // Ahead / behind the upstream, or the running batch, on the right.
    let mut right_x = right;
    let sync: Vec<(String, Color32)> = match &look.batch {
        Some(s @ (crate::batch::ItemState::Running(_) | crate::batch::ItemState::Waiting)) => {
            let (_, c) = dashboard::state_text(ui, s);
            vec![(icons::ARROWS_CLOCKWISE.to_owned(), c)]
        }
        _ => match gl.ahead_behind {
            Some((a, b)) => [
                (icons::ARROW_UP, a, accent),
                (icons::ARROW_DOWN, b, v.warn_fg_color),
            ]
            .into_iter()
            .filter(|x| x.1 > 0)
            .map(|(i, n, c)| (format!("{i}{n}"), if selected { strong } else { c }))
            .collect(),
            None => Vec::new(),
        },
    };
    for (t, c) in sync.into_iter().rev() {
        let g = painter.layout_no_wrap(t, theme::strong_font(12.0), c);
        right_x -= g.size().x;
        painter.galley(egui::pos2(right_x, y3 - g.size().y / 2.0), g, c);
        right_x -= 8.0;
    }
    let right = right_x;
    let time_w = time.as_ref().map_or(0.0, |t| {
        painter
            .layout_no_wrap(t.clone(), small.clone(), weak)
            .size()
            .x
            + 28.0
    });
    let mut job = egui::text::LayoutJob::simple_singleline(branch, small.clone(), text);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - x - time_w).max(30.0));
    let bg = ui.fonts_mut(|f| f.layout_job(job));
    let bw = bg.size().x;
    painter.galley(egui::pos2(x, y3 - bg.size().y / 2.0), bg, text);
    x += bw + 12.0;
    if let Some(t) = time {
        let g = painter.layout_no_wrap(format!("{}  {t}", icons::CLOCK), small, weak);
        painter.galley(egui::pos2(x, y3 - g.size().y / 2.0), g, weak);
    }
}

/// Whether `p` passes the search text and the dashboard filter.
fn shown(m: &Manager, lib: &Library, p: &Path) -> bool {
    if !matches(lib, &m.filter, p) {
        return false;
    }
    let f = m.dash.filter;
    f == Filter::All
        || dashboard::look(&m.dash, &m.glances, &m.errors, p).keep(f)
        || f == Filter::Problems && !p.exists()
}

/// The dashboard bar: view mode, filters with counts, Fetch all / Pull all
/// (on the repositories shown), then the batch progress.
fn dash_bar(ui: &mut egui::Ui, m: &mut Manager, lib: &mut Library, cmds: &mut Commands) {
    let searched: Vec<PathBuf> = lib
        .repos
        .iter()
        .filter(|r| matches(lib, &m.filter, &r.path))
        .map(|r| r.path.clone())
        .collect();
    let mut counts = [0usize; 5];
    for p in &searched {
        let look = dashboard::look(&m.dash, &m.glances, &m.errors, p);
        for (i, f) in [
            Filter::All,
            Filter::Changes,
            Filter::Ahead,
            Filter::Behind,
            Filter::Problems,
        ]
        .into_iter()
        .enumerate()
        {
            if look.keep(f) || f == Filter::Problems && !p.exists() {
                counts[i] += 1;
            }
        }
    }
    let targets: Vec<PathBuf> = searched
        .into_iter()
        .filter(|p| p.exists() && shown(m, lib, p))
        .collect();
    dashboard::bar(ui, &mut m.dash, lib, counts, &targets, m.busy, cmds);
    dashboard::batch_panel(ui, &mut m.dash, lib);
}

/// Repositories as rows of a table (the list mode).
fn list(
    ui: &mut egui::Ui,
    m: &mut Manager,
    lib: &mut Library,
    paths: &[PathBuf],
    open: &[PathBuf],
    menu: &RowMenu,
    cmds: &mut Commands,
) -> Option<Action> {
    let mut action = None;
    dashboard::header(ui);
    for p in paths {
        m.order.push(p.to_path_buf());
        let exists = p.exists();
        let name = display_name(lib, p);
        let width = ui.available_width();
        let (rect, resp) =
            ui.allocate_exact_size(vec2(width, dashboard::ROW_HEIGHT), Sense::click());
        let selected = m.selected.contains(p);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &name));
        let mut name_rect = Rect::from_min_size(rect.min + vec2(12.0, 2.0), vec2(240.0, 20.0));
        if ui.is_rect_visible(rect) {
            let look = dashboard::look(&m.dash, &m.glances, &m.errors, p);
            let color = lib.color(p).map(crate::ui::tabs::color32);
            name_rect = dashboard::paint_row(
                ui,
                &resp,
                rect,
                color,
                &name,
                &short_path(p),
                &look,
                selected,
                open.contains(p),
                exists,
            );
        }
        if let Some(a) = interact(ui, m, lib, p, &name, resp, name_rect, exists, menu, cmds) {
            action = Some(a);
        }
    }
    ui.add_space(theme::SPACE_S);
    action
}

fn card_menu(
    ui: &mut egui::Ui,
    m: &Manager,
    lib: &mut Library,
    p: &Path,
    exists: bool,
    menu: &RowMenu,
    cmds: &mut Commands,
) -> Option<Action> {
    let mut action = None;
    let sel: Vec<PathBuf> = m.selected.iter().cloned().collect();
    if ui.button(t!("Open")).clicked() {
        action = Some(Action::Open(sel.clone()));
        ui.close();
    }
    if sel.len() == 1 && ui.button(t!("Rename (F2)")).clicked() {
        action = Some(Action::Rename(p.to_path_buf()));
        ui.close();
    }
    ui.menu_button(t!("Move to group"), |ui| {
        if ui.button(t!("No group")).clicked() {
            for s in &sel {
                lib.entry(s).folder.clear();
            }
            ui.close();
        }
        for f in menu.folders {
            if ui.button(f).clicked() {
                for s in &sel {
                    lib.entry(s).folder = f.clone();
                }
                ui.close();
            }
        }
    });
    if !menu.workspaces.is_empty() {
        ui.menu_button(t!("Add to workspace"), |ui| {
            for w in menu.workspaces {
                if w != &lib.workspace && ui.button(w).clicked() {
                    for s in &sel {
                        lib.add_to_workspace(w, s);
                    }
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    if exists {
        if ui.button(t!("Open in terminal")).clicked() {
            cmds.push(Command::External(Launch::Terminal { dir: p.into() }));
            ui.close();
        }
        if ui.button(t!("Show in file manager")).clicked() {
            cmds.push(Command::External(Launch::Reveal { path: p.into() }));
            ui.close();
        }
    }
    if ui.button(t!("Copy path")).clicked() {
        cmds.push(Command::Copy(p.display().to_string()));
        ui.close();
    }
    ui.separator();
    if ui.button(t!("Remove from list (Delete)")).clicked() {
        action = Some(Action::Remove(sel.clone()));
        ui.close();
    }
    action
}

/// Click, Ctrl+click (toggle) and Shift+click (range).
fn click(ui: &egui::Ui, m: &mut Manager, p: &Path) {
    let mods = ui.input(|i| i.modifiers);
    if mods.shift
        && let Some(a) = &m.anchor
        && let (Some(x), Some(y)) = (
            m.order.iter().position(|o| o == a),
            m.order.iter().position(|o| o == p),
        )
    {
        let (lo, hi) = (x.min(y), x.max(y));
        // Rows after this one are not in `order` yet: the range stops here.
        m.selected = m.order[lo..=hi].iter().cloned().collect();
        return;
    }
    if mods.command {
        if !m.selected.remove(p) {
            m.selected.insert(p.to_path_buf());
        }
    } else {
        m.selected = BTreeSet::from([p.to_path_buf()]);
    }
    m.anchor = Some(p.to_path_buf());
}

/// Enter, Delete, F2, arrows and Ctrl+A, while no text field has the focus.
fn keys(ui: &egui::Ui, m: &mut Manager) -> Option<Action> {
    let ctx = ui.ctx();
    if m.renaming.is_some() || ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
        return None;
    }
    let (enter, delete, f2, up, down, all) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Delete),
            i.consume_key(egui::Modifiers::NONE, Key::F2),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
                || i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                || i.consume_key(egui::Modifiers::NONE, Key::ArrowRight),
            i.consume_key(egui::Modifiers::COMMAND, Key::A),
        )
    });
    let sel: Vec<PathBuf> = m.selected.iter().cloned().collect();
    if all {
        m.selected = m.order.iter().cloned().collect();
    }
    if up || down {
        let cur = m
            .anchor
            .as_ref()
            .and_then(|a| m.order.iter().position(|o| o == a));
        let next = match cur {
            Some(i) if up => i.saturating_sub(1),
            Some(i) => (i + 1).min(m.order.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(p) = m.order.get(next).cloned() {
            m.selected = BTreeSet::from([p.clone()]);
            m.anchor = Some(p);
        }
    }
    if sel.is_empty() {
        return None;
    }
    if enter {
        let existing = sel.into_iter().filter(|p| p.exists()).collect();
        return Some(Action::Open(existing));
    }
    if delete {
        return Some(Action::Remove(sel));
    }
    if f2 && sel.len() == 1 {
        return Some(Action::Rename(sel[0].clone()));
    }
    None
}

fn workspaces(ui: &mut egui::Ui, m: &mut Manager, lib: &mut Library, cmds: &mut Commands) {
    side_title(ui, icons::BROWSERS, &t!("Workspaces"));
    ui.weak(t!(
        "Each workspace remembers its own tabs. Nest them with /."
    ));
    let active = lib.workspace.clone();
    let default = ui.add(egui::Button::selectable(
        active.is_empty(),
        t!("Default workspace"),
    ));
    if default.clicked() {
        cmds.push(Command::Repos(RepoCmd::SwitchWorkspace(String::new())));
    }
    for name in lib.workspace_names() {
        let depth = name.matches('/').count();
        let leaf = name.rsplit('/').next().unwrap_or(&name).to_owned();
        let resp = ui
            .horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                ui.add(egui::Button::selectable(name == active, leaf))
            })
            .inner
            .on_hover_text(&name);
        if resp.clicked() {
            cmds.push(Command::Repos(RepoCmd::SwitchWorkspace(name.clone())));
        }
        resp.context_menu(|ui| {
            let mut new = name.clone();
            ui.label(t!("Rename workspace"));
            let r = ui.text_edit_singleline(&mut new);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                lib.rename_workspace(&name, &new);
                ui.close();
            }
            if name != active && ui.button(t!("Delete workspace")).clicked() {
                lib.delete_workspace(&name);
                ui.close();
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut m.new_workspace)
                .hint_text(t!("New workspace"))
                .desired_width(150.0),
        );
        let name = crate::repos::clean_workspace_name(&m.new_workspace);
        if ui
            .add_enabled(!name.is_empty(), egui::Button::new(t!("Add")))
            .clicked()
        {
            lib.add_workspace(&name);
            m.new_workspace.clear();
        }
    });
}

/// A title in the left column.
fn side_title(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_S + 2.0;
        ui.label(RichText::new(icon).color(ui.visuals().weak_text_color()));
        ui.label(theme::strong(title));
    });
}

fn sources(ui: &mut egui::Ui, lib: &mut Library, scanning: bool, cmds: &mut Commands) {
    side_title(ui, icons::FOLDER, &t!("Source folders"));
    ui.weak(t!(
        "Folders where your repositories live. Ferrum lists every repository found in them."
    ));
    let mut remove = None;
    let mut changed = false;
    for (i, s) in lib.sources.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(s.path.display().to_string());
            if ui
                .small_button(crate::icons::CLOSE)
                .on_hover_text(t!("Remove"))
                .clicked()
            {
                remove = Some(i);
            }
        });
        ui.horizontal(|ui| {
            ui.weak(t!("Depth"));
            let r = ui
                .add(egui::DragValue::new(&mut s.depth).range(1..=6))
                .on_hover_text(t!("How many folders deep to look"));
            changed |= r.drag_stopped() || r.lost_focus();
        });
    }
    if let Some(i) = remove {
        lib.sources.remove(i);
        changed = true;
    }
    ui.horizontal(|ui| {
        if ui.button(t!("+ Add folder…")).clicked()
            && let Some(p) = rfd::FileDialog::new().pick_folder()
        {
            lib.sources.push(SourceFolder {
                path: p,
                ..Default::default()
            });
            changed = true;
        }
        if scanning {
            ui.spinner();
            ui.weak(t!("Scanning…"));
        } else if !lib.sources.is_empty()
            && ui
                .button(t!("↻ Rescan"))
                .on_hover_text(t!("Look again for new repositories"))
                .clicked()
        {
            changed = true;
        }
    });
    if changed {
        cmds.push(Command::Repos(RepoCmd::Scan));
    }
}

/// Command palette entries: Open Quickly, tabs and "Workspace: X".
pub fn palette_entries(settings: &Settings, tab: Option<&RepoTab>, list: &mut Vec<Entry>) {
    use crate::keymap::Action;
    let repos = |c: fn() -> RepoCmd| move || Command::Repos(c());
    list.push(
        Entry::new(
            Group::Commands,
            t!("Open Quickly…"),
            "",
            repos(|| RepoCmd::OpenQuickly),
        )
        .key(Action::OpenQuickly),
    );
    list.push(
        Entry::new(
            Group::Commands,
            t!("Repository Manager"),
            "",
            repos(|| RepoCmd::NewTab),
        )
        .key(Action::NewTab),
    );
    list.push(
        Entry::new(
            Group::Commands,
            t!("Reopen closed tab"),
            "",
            repos(|| RepoCmd::ReopenClosed),
        )
        .key(Action::ReopenTab),
    );
    list.push(Entry::new(
        Group::Commands,
        t!("Rescan source folders"),
        "",
        repos(|| RepoCmd::Scan),
    ));
    if tab.is_some() {
        list.push(Entry::new(
            Group::Commands,
            t!("Repository settings…"),
            "",
            repos(|| RepoCmd::Settings),
        ));
        list.push(Entry::new(
            Group::Commands,
            t!("Show repository overview"),
            "",
            || Command::SetView(crate::tab::View::Overview),
        ));
    }
    let lib = &settings.library;
    if !lib.workspace.is_empty() {
        list.push(Entry::new(
            Group::Workspaces,
            t!("Workspace: {}", t!("Default workspace")),
            "",
            || Command::Repos(RepoCmd::SwitchWorkspace(String::new())),
        ));
    }
    for name in lib.workspace_names() {
        if name != lib.workspace {
            let n = name.clone();
            list.push(Entry::new(
                Group::Workspaces,
                t!("Workspace: {}", name),
                "",
                move || Command::Repos(RepoCmd::SwitchWorkspace(n.clone())),
            ));
        }
    }
}
