//! The "Ask" mode of the history search: a request in plain words ("by Ana
//! last month in src/") becomes git filters, shown as editable chips; the
//! search runs when the user confirms. The model reads the request when
//! AI is on; simple phrases are read offline otherwise (or when the
//! model's answer is unusable).

use eframe::egui::{self, RichText};
use ferrum_core::ai::search::{self, Day, Field, Filters};

use crate::ai::Stream;
use crate::icons;
use crate::tab::RepoTab;
use crate::theme;
use crate::ui::ai_common;

#[derive(Default)]
pub struct AskState {
    /// The filters, once the request was read.
    pub filters: Option<Filters>,
    /// The request they were read from.
    pub query: String,
    stream: Option<Stream>,
    /// How the request was read, and what was dropped.
    pub note: Option<String>,
}

fn label(f: Field) -> String {
    match f {
        Field::Author => t!("Author"),
        Field::Since => t!("Since"),
        Field::Until => t!("Until"),
        Field::Path => t!("Path"),
        Field::Message => t!("Message"),
        Field::Content => t!("Code"),
    }
}

fn icon(f: Field) -> &'static str {
    match f {
        Field::Author => icons::USER,
        Field::Since | Field::Until => icons::CALENDAR,
        Field::Path => icons::FOLDER,
        Field::Message => icons::CHAT_TEXT,
        Field::Content => icons::CODE,
    }
}

fn hint(f: Field) -> &'static str {
    match f {
        Field::Since | Field::Until => "YYYY-MM-DD",
        Field::Path => "src/ *login*",
        _ => "",
    }
}

pub fn search_hint() -> String {
    t!("Describe the commits, e.g. \"by Ana last month in src/\", press Enter")
}

/// Reads the request: with the model when AI is on, else offline.
pub fn interpret(tab: &mut RepoTab, ctx: &egui::Context) {
    let query = tab.history.search.trim().to_owned();
    let ask = &mut tab.history.ext.ask;
    ask.query = query.clone();
    ask.filters = None;
    let cfg = ai_common::config(ctx);
    if !cfg.enabled {
        ask.filters = Some(search::parse(&query, Day::today()));
        ask.note = Some(t!("Read offline (AI is off): simple phrases only."));
        return;
    }
    ask.note = None;
    ask.stream = Some(Stream::start(ctx, &cfg, move || {
        Ok(search::prompt(&query, Day::today()))
    }));
}

fn poll(ask: &mut AskState) {
    let Some(done) = ask.stream.as_ref().and_then(Stream::finished) else {
        return;
    };
    ask.stream = None;
    let query = ask.query.clone();
    let offline = || search::parse(&query, Day::today());
    match done.map(|t| ferrum_core::ai::json::extract(&t)) {
        Ok(Some(v)) => {
            let (f, dropped) = search::from_json(&v);
            if f.is_empty() {
                ask.filters = Some(offline());
                ask.note = Some(t!("The model found no filters; read offline instead."));
            } else {
                ask.filters = Some(f);
                ask.note = Some(if dropped.is_empty() {
                    t!("Read by the model. Check the filters, then search.")
                } else {
                    t!(
                        "Read by the model; left out as invalid: {}",
                        dropped.join(", ")
                    )
                });
            }
        }
        Ok(None) => {
            ask.filters = Some(offline());
            ask.note = Some(t!(
                "The model's answer could not be read; read offline instead."
            ));
        }
        Err(e) => {
            ask.filters = Some(offline());
            ask.note = Some(t!("{} Read offline instead.", e));
        }
    }
}

/// Runs `git log` with the filters as a job.
pub fn run(tab: &mut RepoTab, ctx: &egui::Context) {
    let Some(filters) = tab
        .history
        .ext
        .ask
        .filters
        .clone()
        .filter(|f| !f.is_empty())
    else {
        return;
    };
    let path = tab.path().to_path_buf();
    let all = !tab.history.ext.scope_branch;
    tab.history.ext.searched = Some(tab.history.search.trim().to_owned());
    tab.history.content_hits = None;
    tab.spawn_job(&t!("Search the history"), ctx, move |_| {
        let git = ferrum_core::cli::Git::new(&path);
        let hits = search::run(&git, &filters, all).map_err(|e| e.to_string())?;
        Ok(crate::jobs::JobOutput::SearchHits(
            hits.into_iter().collect(),
        ))
    });
}

/// In the search bar: Enter or "Interpret" reads the request (again when it
/// changed); with the filters read, Enter runs the search.
pub fn bar(ui: &mut egui::Ui, tab: &mut RepoTab, enter: bool) {
    poll(&mut tab.history.ext.ask);
    let term = tab.history.search.trim().to_owned();
    let ask = &tab.history.ext.ask;
    let fresh = ask.query != term || ask.filters.is_none();
    if ask.stream.is_some() {
        ui.spinner();
    } else {
        let clicked = ui
            .add_enabled(
                !term.is_empty(),
                egui::Button::new(icons::text(icons::AI, t!("Interpret"))),
            )
            .on_hover_text(t!(
                "Turn the request into filters you can check before searching"
            ))
            .clicked();
        if (clicked || enter && fresh) && !term.is_empty() {
            interpret(tab, ui.ctx());
        } else if enter && !fresh {
            run(tab, ui.ctx());
        }
    }
    ai_common::badge(ui);
}

/// The filters as chips under the search bar, with the search button.
pub fn chips(ui: &mut egui::Ui, tab: &mut RepoTab) {
    let busy = tab.busy();
    let ask = &mut tab.history.ext.ask;
    if ask.stream.is_some() {
        ui.horizontal(|ui| {
            ui.weak(t!("Reading the request…"));
            if let Some(s) = &ask.stream
                && ui.small_button(t!("Cancel")).clicked()
            {
                s.cancel();
            }
        });
        return;
    }
    let Some(filters) = &mut ask.filters else {
        return;
    };
    let mut search = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(theme::SPACE_S, theme::SPACE_S);
        let mut shown = 0;
        for f in Field::ALL {
            if filters.get(f).is_empty()
                && !ui.memory(|m| m.data.get_temp::<bool>(chip_id(f)).unwrap_or(false))
            {
                continue;
            }
            shown += 1;
            if chip(ui, f, filters.get_mut(f)) {
                filters.get_mut(f).clear();
                ui.memory_mut(|m| m.data.remove::<bool>(chip_id(f)));
            }
        }
        if shown == 0 {
            ui.weak(t!("No filters were found in the request."));
        }
        ui.menu_button(icons::text(icons::PLUS, t!("Filter")), |ui| {
            for f in Field::ALL {
                if filters.get(f).is_empty() && ui.button(icons::text(icon(f), label(f))).clicked()
                {
                    ui.memory_mut(|m| m.data.insert_temp(chip_id(f), true));
                    ui.close();
                }
            }
        });
        let ready = !filters.is_empty() && !busy;
        if ui
            .add_enabled(
                ready,
                theme::primary(icons::text(icons::SEARCH, t!("Search"))),
            )
            .on_hover_text(t!("Run git log with these filters"))
            .clicked()
        {
            search = true;
        }
    });
    if let Some(n) = &ask.note {
        ui.label(RichText::new(n).weak().size(11.0));
    }
    if search {
        run(tab, ui.ctx());
    }
}

fn chip_id(f: Field) -> egui::Id {
    egui::Id::unique(("ask-chip", f as u8))
}

/// One filter: icon, name, editable value, remove button. Returns `true`
/// when removed.
fn chip(ui: &mut egui::Ui, f: Field, value: &mut String) -> bool {
    let accent = theme::accent(ui);
    let mut removed = false;
    egui::Frame::new()
        .fill(accent.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, accent.gamma_multiply(0.35)))
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_S;
            ui.label(RichText::new(icon(f)).color(accent));
            ui.label(
                RichText::new(label(f))
                    .color(ui.visuals().weak_text_color())
                    .size(12.0),
            );
            let shown = if value.is_empty() { hint(f) } else { value.as_str() };
            let text = egui::WidgetText::from(shown).into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            );
            let width = (text.size().x + 10.0).clamp(40.0, 260.0);
            ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(hint(f))
                    .frame(egui::Frame::NONE)
                    .desired_width(width),
            );
            if ui
                .add(
                    egui::Button::new(RichText::new(icons::X).size(11.0))
                        .frame_when_inactive(false),
                )
                .on_hover_text(t!("Remove this filter"))
                .clicked()
            {
                removed = true;
            }
        });
    removed
}
