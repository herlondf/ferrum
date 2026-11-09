//! The main toolbar, Fork style: large buttons with an icon and the label
//! underneath, in groups (sync, branches and stashes, undo, tools), with
//! ahead/behind badges and a spinner on the button whose operation runs.
//! When the window is too narrow for the labels, the buttons show only
//! their icons.

use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::command::{Command, Commands, Op};
use crate::external::Launch;
use crate::icons;
use crate::prefs::ToolbarButton as Tb;
use crate::settings::Settings;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::dialogs::Dialog;

/// Width of the ⏷ part of a split button.
const CARET_WIDTH: f32 = 15.0;
const ICON_SIZE: f32 = 18.0;
const LABEL_SIZE: f32 = 11.5;

/// The background of the window's top area (menu bar, toolbar and tab
/// strip): a shade apart from the panels, so the active tab joins the view.
pub fn chrome_fill(v: &egui::Visuals) -> Color32 {
    if v.panel_fill == Color32::BLACK {
        return Color32::BLACK;
    }
    let t = if v.dark_mode { 0.28 } else { 0.055 };
    v.panel_fill.lerp_to_gamma(Color32::BLACK, t)
}

/// The frame of the top panels.
pub fn chrome_frame(ui: &egui::Ui, margin: egui::Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(chrome_fill(ui.visuals()))
        .inner_margin(margin)
}

/// One toolbar button.
pub struct Tool<'a> {
    icon: &'a str,
    label: String,
    /// What screen readers and tests see.
    a11y: String,
    enabled: bool,
    badge: Option<String>,
    busy: bool,
    caret: bool,
    /// The ⏷ part works even when the button does not.
    caret_enabled: bool,
    show_label: bool,
    /// The icon in the accent color.
    accent: bool,
}

impl<'a> Tool<'a> {
    pub fn new(icon: &'a str, label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            icon,
            a11y: icons::text(icon, &label),
            label,
            enabled: true,
            badge: None,
            busy: false,
            caret: false,
            caret_enabled: false,
            show_label: true,
            accent: false,
        }
    }

    /// An icon-only button; `a11y` names it.
    pub fn icon(icon: &'a str, a11y: impl Into<String>) -> Self {
        Self {
            a11y: a11y.into(),
            show_label: false,
            ..Self::new(icon, "")
        }
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self.caret_enabled = on;
        self
    }

    /// A count on the icon's corner (hidden when 0).
    pub fn badge(mut self, n: usize) -> Self {
        self.badge = (n > 0).then(|| n.to_string());
        self
    }

    pub fn badge_text(mut self, text: Option<String>) -> Self {
        self.badge = text;
        self
    }

    /// Draws the icon in the accent color, to stand out.
    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    /// A spinner instead of the icon.
    pub fn busy(mut self, on: bool) -> Self {
        self.busy = on;
        self
    }

    /// A ⏷ part on the right, for options or a menu.
    pub fn caret(mut self) -> Self {
        self.caret = true;
        self.caret_enabled = self.enabled;
        self
    }

    /// Enables the ⏷ part on its own (set after `enabled`/`caret`).
    pub fn caret_enabled(mut self, on: bool) -> Self {
        self.caret_enabled = on;
        self
    }

    pub fn label_shown(mut self, on: bool) -> Self {
        self.show_label = on && !self.label.is_empty();
        self
    }

    fn height(&self) -> f32 {
        height(self.show_label)
    }

    fn main_width(&self, ui: &egui::Ui) -> f32 {
        let min = if self.show_label { 48.0 } else { 32.0 };
        if !self.show_label {
            return min;
        }
        let w = ui
            .painter()
            .layout_no_wrap(self.label.clone(), label_font(), Color32::WHITE)
            .size()
            .x;
        (w + 16.0).max(min)
    }

    /// Width the button takes, with its caret.
    pub fn width(&self, ui: &egui::Ui) -> f32 {
        self.main_width(ui) + if self.caret { CARET_WIDTH } else { 0.0 }
    }

    pub fn show(self, ui: &mut egui::Ui) -> ToolResponse {
        let h = self.height();
        let main_w = self.main_width(ui);
        let sense = |on| if on { Sense::click() } else { Sense::hover() };
        let (rect, main) = ui.allocate_exact_size(vec2(main_w, h), sense(self.enabled));
        let enabled = self.enabled;
        let a11y = self.a11y.clone();
        main.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &a11y));
        let caret = self.caret.then(|| {
            // No gap between the two parts.
            let r = Rect::from_min_size(pos2(rect.right(), rect.top()), vec2(CARET_WIDTH, h));
            let on = self.caret_enabled;
            let resp = ui.interact(r, main.id.with("caret"), sense(on));
            ui.advance_cursor_after_rect(r);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, on, "⏷"));
            resp
        });
        if ui.is_rect_visible(rect) {
            self.paint(ui, rect, &main, caret.as_ref());
        }
        ToolResponse { main, caret }
    }

    fn paint(
        &self,
        ui: &egui::Ui,
        rect: Rect,
        main: &egui::Response,
        caret: Option<&egui::Response>,
    ) {
        let v = ui.visuals();
        let painter = ui.painter();
        let radius = theme::RADIUS;
        let hovered = |r: &egui::Response| self.enabled && r.hovered();
        let pressed = |r: &egui::Response| self.enabled && r.is_pointer_button_down_on();
        let any_hover = hovered(main) || caret.is_some_and(hovered);
        let fill_for = |r: &egui::Response| {
            if pressed(r) {
                v.widgets.active.weak_bg_fill
            } else {
                v.widgets.hovered.weak_bg_fill
            }
        };
        let whole = match caret {
            Some(c) => rect.union(c.rect),
            None => rect,
        };
        if any_hover {
            // The whole split button lights up a little, the part under the
            // pointer more.
            let soft = v.widgets.inactive.weak_bg_fill;
            painter.rect_filled(whole, radius, soft);
            let (part, r) = if hovered(main) {
                (rect, main)
            } else {
                (caret.map(|c| c.rect).unwrap_or(rect), caret.unwrap_or(main))
            };
            let cr = match (caret.is_some(), hovered(main)) {
                (false, _) => egui::CornerRadius::same(radius),
                (true, true) => egui::CornerRadius {
                    nw: radius,
                    sw: radius,
                    ne: 0,
                    se: 0,
                },
                (true, false) => egui::CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: radius,
                    se: radius,
                },
            };
            painter.rect_filled(part, cr, fill_for(r));
            if let Some(c) = caret {
                let x = c.rect.left();
                let line = v.widgets.noninteractive.bg_stroke.color;
                painter.vline(x, c.rect.y_range().shrink(6.0), Stroke::new(1.0, line));
            }
        }
        let fg = if !self.enabled {
            v.widgets
                .noninteractive
                .fg_stroke
                .color
                .gamma_multiply(0.45)
        } else if any_hover {
            v.widgets.hovered.fg_stroke.color
        } else {
            v.widgets.inactive.fg_stroke.color
        };
        let icon_center = if self.show_label {
            pos2(rect.center().x, rect.top() + 6.0 + ICON_SIZE / 2.0)
        } else {
            rect.center()
        };
        if self.busy {
            let r = Rect::from_center_size(icon_center, Vec2::splat(ICON_SIZE - 2.0));
            egui::Spinner::new()
                .size(ICON_SIZE - 2.0)
                .color(theme::accent(ui))
                .paint_at(ui, r);
        } else {
            let color = if self.accent && self.enabled {
                theme::accent(ui)
            } else {
                fg
            };
            painter.text(
                icon_center,
                Align2::CENTER_CENTER,
                self.icon,
                FontId::proportional(ICON_SIZE),
                color,
            );
        }
        if self.show_label {
            painter.text(
                pos2(rect.center().x, rect.bottom() - 5.0),
                Align2::CENTER_BOTTOM,
                &self.label,
                label_font(),
                fg,
            );
        }
        if let Some(c) = caret {
            painter.text(
                c.rect.center(),
                Align2::CENTER_CENTER,
                icons::CARET_DOWN,
                FontId::proportional(10.0),
                fg,
            );
        }
        if let Some(text) = &self.badge {
            badge(
                ui,
                pos2(
                    icon_center.x + ICON_SIZE / 2.0 - 2.0,
                    icon_center.y - ICON_SIZE / 2.0,
                ),
                text,
                self.enabled,
            );
        }
    }
}

/// Height of the buttons, with or without labels.
pub fn height(labels: bool) -> f32 {
    match (labels, theme::density()) {
        (false, _) => 30.0,
        (true, theme::Density::Compact) => 40.0,
        (true, theme::Density::Comfortable) => 44.0,
    }
}

fn label_font() -> FontId {
    FontId::proportional(LABEL_SIZE)
}

/// A small accent pill with a count, its left edge near `at`.
pub fn badge(ui: &egui::Ui, at: egui::Pos2, text: &str, enabled: bool) {
    let painter = ui.painter();
    let galley =
        painter.layout_no_wrap(text.to_owned(), theme::strong_font(9.5), theme::on_accent());
    let size = vec2((galley.size().x + 8.0).max(15.0), 15.0);
    let rect = Rect::from_min_size(pos2(at.x - 4.0, at.y - 4.0), size);
    let mut fill = theme::accent(ui);
    if !enabled {
        fill = fill.gamma_multiply(0.55);
    }
    // A ring in the toolbar color keeps it apart from the icon.
    painter.rect(
        rect,
        size.y / 2.0,
        fill,
        Stroke::new(1.5, chrome_fill(ui.visuals())),
        egui::StrokeKind::Outside,
    );
    painter.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme::on_accent(),
    );
}

pub struct ToolResponse {
    pub main: egui::Response,
    pub caret: Option<egui::Response>,
}

/// A thin vertical line between groups.
pub fn group_separator(ui: &mut egui::Ui) {
    let h = ui.available_height().clamp(18.0, 28.0);
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, h), Sense::hover());
    let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, color));
}

/// An icon-only toolbar button (palette, settings…).
pub fn icon_button(ui: &mut egui::Ui, icon: &str, tip: String) -> egui::Response {
    Tool::icon(icon, icon).show(ui).main.on_hover_text(tip)
}

/// The button whose operation runs, told by the job's label (see `Op::label`).
fn running(tab: &RepoTab) -> Option<Tb> {
    let job = tab.job.as_ref()?;
    button_for(&job.label)
}

/// The toolbar button of an operation label.
pub fn button_for(label: &str) -> Option<Tb> {
    // The text of a label template before its first argument.
    let starts = |template: String| {
        let prefix = template.split('\u{0}').next().unwrap_or_default();
        !prefix.is_empty() && label.starts_with(prefix)
    };
    let nul = "\u{0}";
    if label == t!("Fetch all remotes") || starts(t!("Fetch {}", nul)) {
        Some(Tb::Fetch)
    } else if label == t!("Pull") || starts(t!("Pull {}/{}", nul, nul)) {
        Some(Tb::Pull)
    } else if starts(t!("Push {} to {}", nul, nul)) {
        Some(Tb::Push)
    } else if label == t!("Stash changes") {
        Some(Tb::Stash)
    } else if starts(t!("Pop {}", nul)) {
        Some(Tb::Pop)
    } else if starts(t!("Undo: {}", nul)) {
        Some(Tb::Undo)
    } else {
        None
    }
}

/// Whether the labels fit: `reserve` is the width needed on the right.
pub fn labels_fit(ui: &egui::Ui, tab: Option<&RepoTab>, settings: &Settings, reserve: f32) -> bool {
    let Some(tab) = tab else { return true };
    let mut width = 0.0;
    for (tool, _) in tools(tab, settings, true) {
        width += tool.width(ui) + ui.spacing().item_spacing.x;
    }
    width + 4.0 * 9.0 + reserve < ui.available_width()
}

/// The buttons shown, in order.
fn tools<'a>(tab: &RepoTab, settings: &Settings, labels: bool) -> Vec<(Tool<'a>, Tb)> {
    let show = |b| settings.prefs.shows(b);
    let busy = tab.busy();
    let running = running(tab);
    let has_remote = !tab.remotes.is_empty();
    let can_sync = has_remote && tab.head.is_some();
    let (ahead, behind) = tab
        .current_branch()
        .and_then(|b| b.ahead_behind)
        .unwrap_or((0, 0));
    let mut out = Vec::new();
    let mut add = |b: Tb, tool: Tool<'a>| {
        if show(b) {
            let tool = tool.busy(running == Some(b)).label_shown(labels);
            out.push((tool, b));
        }
    };
    add(
        Tb::Fetch,
        Tool::new(icons::FETCH, "Fetch")
            .enabled(has_remote && !busy)
            .caret(),
    );
    add(
        Tb::Pull,
        Tool::new(icons::PULL, "Pull")
            .enabled(can_sync && !busy)
            .badge(behind)
            .caret(),
    );
    add(
        Tb::Push,
        Tool::new(icons::PUSH, "Push")
            .enabled(can_sync && !busy)
            .badge(ahead)
            .caret(),
    );
    add(
        Tb::Branch,
        Tool::new(icons::BRANCH, "Branch").enabled(!busy),
    );
    add(
        Tb::Stash,
        Tool::new(icons::STASH, "Stash")
            .enabled(!busy && !tab.status.is_empty())
            .caret()
            .caret_enabled(!busy),
    );
    add(
        Tb::Pop,
        Tool::new(icons::POP, "Pop").enabled(!busy && !tab.stashes.is_empty()),
    );
    add(
        Tb::Undo,
        Tool::new(icons::UNDO, t!("Undo")).enabled(!busy && !tab.undo.is_empty()),
    );
    add(Tb::Terminal, Tool::new(icons::TERMINAL, t!("Terminal")));
    add(
        Tb::FileManager,
        Tool::new(icons::FILE_MANAGER, t!("Folder")),
    );
    out
}

/// Which group a button belongs to.
fn group(b: Tb) -> u8 {
    match b {
        Tb::Fetch | Tb::Pull | Tb::Push => 0,
        Tb::Branch | Tb::Stash | Tb::Pop => 1,
        Tb::Undo => 2,
        Tb::Terminal | Tb::FileManager => 3,
    }
}

/// The repository buttons, left of the toolbar.
pub fn repo_buttons(
    ui: &mut egui::Ui,
    tab: &RepoTab,
    settings: &Settings,
    labels: bool,
    cmds: &mut Commands,
) {
    use crate::ui::sync_ui;
    let mut last_group = None;
    let (ahead, behind) = tab
        .current_branch()
        .and_then(|b| b.ahead_behind)
        .unwrap_or((0, 0));
    for (tool, b) in tools(tab, settings, labels) {
        if last_group.is_some_and(|g| g != group(b)) {
            group_separator(ui);
        }
        last_group = Some(group(b));
        let r = tool.show(ui);
        let mods = ui.input(|i| i.modifiers);
        let main = r.main;
        match b {
            Tb::Fetch | Tb::Pull | Tb::Push => {
                let more = t!("Options… (or Alt+click)");
                let tip = match b {
                    Tb::Fetch => t!("Fetch all remotes (Ctrl+Shift+F)"),
                    Tb::Pull => {
                        let mut tip = t!("Pull — {} (Ctrl+Shift+D)", settings.pull.label());
                        if behind > 0 {
                            tip = format!("{tip}\n{}", t!("{} commits to pull", behind));
                        }
                        tip
                    }
                    _ => {
                        let mut tip = t!("Push (Ctrl+Shift+U for options)");
                        if ahead > 0 {
                            tip = format!("{tip}\n{}", t!("{} commits to push", ahead));
                        }
                        tip
                    }
                };
                let main = main.on_hover_text(tip);
                let caret = r.caret.map(|c| c.on_hover_text(more));
                // A plain click runs; Alt/Ctrl+click, right-click or the ⏷ opens the dialog.
                let options = main.secondary_clicked()
                    || (main.clicked() && (mods.alt || mods.command))
                    || caret.is_some_and(|c| c.clicked());
                let run = main.clicked() && !options;
                sync_ui::toolbar_action(tab, settings, b, run, options, cmds);
            }
            Tb::Branch => {
                if main
                    .on_hover_text(t!("New branch (Ctrl+Shift+B)"))
                    .clicked()
                {
                    let head = tab.head.clone().unwrap_or_else(|| "HEAD".into());
                    cmds.push(Command::Dialog(Dialog::create_branch(&head, &head)));
                }
            }
            Tb::Stash => {
                let resp = main.on_hover_text(t!(
                    "Stash changes (Ctrl+Shift+S)\nCtrl/Alt+click: quick stash without the dialog"
                ));
                if resp.clicked() {
                    // Ctrl or Alt+click stashes everything right away.
                    let quick = mods.command || mods.alt;
                    cmds.push(if quick {
                        Command::Run(crate::ui::ref_dialogs::quick_stash())
                    } else {
                        Command::Dialog(Dialog::stash())
                    });
                }
                if let Some(caret) = r.caret {
                    // The stash list stays reachable while nothing is changed.
                    let caret = caret.on_hover_text(t!("Stashes"));
                    egui::Popup::menu(&caret)
                        .show(|ui| crate::ui::ref_dialogs::stashes_dropdown(ui, tab, cmds));
                }
            }
            Tb::Pop => {
                if let Some(s) = tab.stashes.first() {
                    let resp =
                        main.on_hover_text(t!("Apply and delete the latest stash: {}", s.message));
                    if resp.clicked() {
                        cmds.run(Op::StashApply {
                            stash: s.clone(),
                            pop: true,
                        });
                    }
                } else {
                    main.on_hover_text(t!("No stashes"));
                }
            }
            Tb::Undo => {
                if let Some(label) = tab.undo.last().map(|u| u.label.clone())
                    && main.on_hover_text(t!("Undo: {} (Ctrl+Z)", label)).clicked()
                {
                    cmds.push(Command::Undo);
                }
            }
            Tb::Terminal => {
                if main.on_hover_text(t!("Open in terminal")).clicked() {
                    cmds.push(Command::External(Launch::Terminal {
                        dir: tab.path().into(),
                    }));
                }
            }
            Tb::FileManager => {
                if main.on_hover_text(t!("Show in file manager")).clicked() {
                    cmds.push(Command::External(Launch::Reveal {
                        path: tab.path().into(),
                    }));
                }
            }
        }
    }
}
