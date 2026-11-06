//! Unified diff of one file, with optional line selection for staging.

use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, Sense, pos2, vec2};
use ferrum_core::{FileDiff, LineKind};

use crate::tab::LineSelection;
use crate::util::{added_color, removed_color};

const ROW_HEIGHT: f32 = 18.0;
const NUMBER_WIDTH: f32 = 44.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// A commit: nothing to stage.
    ReadOnly,
    /// Working copy changes: stage or discard.
    Unstaged,
    /// Index changes: unstage.
    Staged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Stage,
    Unstage,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffAction {
    /// Act on a whole hunk.
    Hunk(Action, usize),
    /// Act on the selected lines.
    Lines(Action),
}

enum Row {
    Hunk(usize),
    Line(usize, usize),
}

/// Draws the diff. `selection` enables picking lines (only in staging modes).
pub fn show(
    ui: &mut egui::Ui,
    id: &str,
    file: &FileDiff,
    mode: DiffMode,
    line_staging: bool,
    mut selection: Option<&mut LineSelection>,
) -> Option<DiffAction> {
    let mut action = None;

    // Header: counts and actions for the selected lines.
    ui.horizontal(|ui| {
        ui.colored_label(added_color(ui), format!("+{}", file.additions));
        ui.colored_label(removed_color(ui), format!("−{}", file.deletions));
        let selected = selection.as_ref().map_or(0, |s| s.lines.len());
        if selected > 0 && line_staging {
            ui.separator();
            ui.label(if selected == 1 {
                t!("1 line selected")
            } else {
                t!("{} lines selected", selected)
            });
            match mode {
                DiffMode::Unstaged => {
                    if ui.button(t!("Stage lines")).clicked() {
                        action = Some(DiffAction::Lines(Action::Stage));
                    }
                    if ui.button(t!("Discard lines")).clicked() {
                        action = Some(DiffAction::Lines(Action::Discard));
                    }
                }
                DiffMode::Staged => {
                    if ui.button(t!("Unstage lines")).clicked() {
                        action = Some(DiffAction::Lines(Action::Unstage));
                    }
                }
                DiffMode::ReadOnly => {}
            }
            if ui.small_button(t!("Clear")).clicked()
                && let Some(s) = selection.as_deref_mut()
            {
                s.clear();
            }
        } else if line_staging && mode != DiffMode::ReadOnly {
            ui.weak(t!("Click lines to select them, Shift+click for a range."));
        }
    });
    ui.separator();

    if file.hunks.is_empty() {
        ui.weak(if file.binary {
            t!("Binary file.")
        } else {
            t!("No changes to show (empty file or mode change).")
        });
        return action;
    }

    let rows: Vec<Row> = file
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(h, hunk)| {
            std::iter::once(Row::Hunk(h)).chain((0..hunk.lines.len()).map(move |l| Row::Line(h, l)))
        })
        .collect();

    let font = crate::util::code_font(12.0);
    let row_h = crate::util::code_row_height(ROW_HEIGHT);
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'm'));
    let longest = file
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .map(|l| l.text.chars().count() + 4)
        .max()
        .unwrap_or(0);
    let content_w = NUMBER_WIDTH * 2.0 + 16.0 + longest as f32 * char_w;

    let dark = ui.visuals().dark_mode;
    let (add_bg, del_bg) = if dark {
        (
            Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 40),
            Color32::from_rgba_unmultiplied(0xf8, 0x51, 0x49, 40),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(0x2e, 0xa0, 0x43, 45),
            Color32::from_rgba_unmultiplied(0xf8, 0x51, 0x49, 40),
        )
    };
    let sel_bg = ui.visuals().selection.bg_fill.gamma_multiply(0.7);

    egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink(false)
        .show_rows(ui, row_h, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width().max(content_w);
            for r in range {
                match rows[r] {
                    Row::Hunk(h) => {
                        let hunk = &file.hunks[h];
                        ui.allocate_ui_with_layout(
                            vec2(width, row_h),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                let rect = ui.max_rect();
                                ui.painter()
                                    .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(&hunk.header)
                                        .monospace()
                                        .size(11.5)
                                        .weak(),
                                );
                                // Buttons stay visible on the right edge of the viewport.
                                let visible_right = ui.clip_rect().right();
                                let mut btn = |ui: &mut egui::Ui, label: &str, a: Action| {
                                    if ui.small_button(label).clicked() {
                                        action = Some(DiffAction::Hunk(a, h));
                                    }
                                };
                                let area = Rect::from_x_y_ranges(
                                    (visible_right - 240.0)..=visible_right - 6.0,
                                    rect.y_range(),
                                );
                                ui.scope_builder(
                                    egui::UiBuilder::new()
                                        .max_rect(area)
                                        .layout(Layout::right_to_left(Align::Center)),
                                    |ui| match mode {
                                        DiffMode::Unstaged => {
                                            btn(ui, &t!("Discard hunk"), Action::Discard);
                                            btn(ui, &t!("Stage hunk"), Action::Stage);
                                        }
                                        DiffMode::Staged => {
                                            btn(ui, &t!("Unstage hunk"), Action::Unstage)
                                        }
                                        DiffMode::ReadOnly => {}
                                    },
                                );
                            },
                        );
                    }
                    Row::Line(h, l) => {
                        let line = &file.hunks[h].lines[l];
                        let selectable =
                            selection.is_some() && line_staging && line.kind != LineKind::Context;
                        let sense = if selectable {
                            Sense::click()
                        } else {
                            Sense::hover()
                        };
                        let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), sense);
                        let is_sel = selection
                            .as_ref()
                            .is_some_and(|s| s.lines.contains(&(h, l)));
                        let painter = ui.painter();
                        let bg = match line.kind {
                            LineKind::Added => add_bg,
                            LineKind::Removed => del_bg,
                            LineKind::Context => Color32::TRANSPARENT,
                        };
                        painter.rect_filled(rect, 0.0, if is_sel { sel_bg } else { bg });
                        if selectable && resp.hovered() {
                            painter.rect_stroke(
                                rect,
                                0.0,
                                (1.0, ui.visuals().selection.stroke.color),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let weak = ui.visuals().weak_text_color();
                        let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                        let y = rect.center().y;
                        painter.text(
                            pos2(rect.left() + NUMBER_WIDTH - 6.0, y),
                            Align2::RIGHT_CENTER,
                            num(line.old_no),
                            crate::util::code_font(11.0),
                            weak,
                        );
                        painter.text(
                            pos2(rect.left() + NUMBER_WIDTH * 2.0 - 6.0, y),
                            Align2::RIGHT_CENTER,
                            num(line.new_no),
                            crate::util::code_font(11.0),
                            weak,
                        );
                        let (prefix, color) = match line.kind {
                            LineKind::Added => ("+", added_color(ui)),
                            LineKind::Removed => ("-", removed_color(ui)),
                            LineKind::Context => (" ", ui.visuals().text_color()),
                        };
                        let x = rect.left() + NUMBER_WIDTH * 2.0 + 4.0;
                        painter.text(pos2(x, y), Align2::LEFT_CENTER, prefix, font.clone(), color);
                        let mut text = crate::util::expand_tabs(&line.text);
                        if line.no_newline {
                            text.push_str(&format!("  {}", t!("(no newline at end)")));
                        }
                        painter.text(
                            pos2(x + char_w * 2.0, y),
                            Align2::LEFT_CENTER,
                            text,
                            font.clone(),
                            ui.visuals().text_color(),
                        );

                        if resp.clicked()
                            && let Some(sel) = selection.as_deref_mut()
                        {
                            let shift = ui.input(|i| i.modifiers.shift);
                            match (shift, sel.anchor) {
                                (true, Some(anchor)) => select_range(file, sel, anchor, (h, l)),
                                _ => {
                                    if !sel.lines.remove(&(h, l)) {
                                        sel.lines.insert((h, l));
                                    }
                                    sel.anchor = Some((h, l));
                                }
                            }
                        }
                    }
                }
            }
        });
    action
}

/// Selects every changed line between two positions.
fn select_range(file: &FileDiff, sel: &mut LineSelection, a: (usize, usize), b: (usize, usize)) {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    for (h, hunk) in file.hunks.iter().enumerate() {
        for (l, line) in hunk.lines.iter().enumerate() {
            if (h, l) >= start && (h, l) <= end && line.kind != LineKind::Context {
                sel.lines.insert((h, l));
            }
        }
    }
}
