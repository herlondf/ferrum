//! Windows for prompts from git and ssh (passwords, passphrases, host keys)
//! and for trusting a server's SSH host key.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use eframe::egui;
use ferrum_core::askpass::{PromptKind, Request, prompt_kind};
use ferrum_core::ssh::{self, HostKeys};

use crate::hosted::{ForgePrefs, Pending};

/// An answer asked again this soon was probably wrong; ask the user.
const RETRY: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct Prompts {
    queue: VecDeque<Request>,
    current: Option<Current>,
    /// Answers remembered until Ferrum closes: prompt -> (answer, last used).
    cache: HashMap<String, (String, Instant)>,
}

struct Current {
    req: Request,
    answer: String,
    focus: bool,
}

impl Prompts {
    /// A new prompt: answered from the remembered answers, or queued.
    pub fn push(&mut self, req: Request) {
        if let Some((answer, used)) = self.cache.get_mut(&req.prompt) {
            if used.elapsed() > RETRY {
                *used = Instant::now();
                let answer = answer.clone();
                req.answer(Some(answer));
                return;
            }
            self.cache.remove(&req.prompt);
        }
        self.queue.push_back(req);
    }

    /// Forgets remembered answers (e.g. after an authentication error).
    pub fn forget(&mut self) {
        self.cache.clear();
    }

    /// The prompt shown now (for tests).
    #[cfg(test)]
    pub fn shown(&self) -> Option<&str> {
        self.current.as_ref().map(|c| c.req.prompt.as_str())
    }
}

/// Shows the oldest waiting prompt.
pub fn show(ctx: &egui::Context, p: &mut Prompts, prefs: &mut ForgePrefs) {
    if p.current.is_none() {
        let Some(req) = p.queue.pop_front() else {
            return;
        };
        p.current = Some(Current {
            req,
            answer: String::new(),
            focus: true,
        });
    }
    let Some(cur) = p.current.as_mut() else {
        return;
    };
    let kind = prompt_kind(&cur.req.prompt);
    let mut done: Option<Option<String>> = None;
    egui::Modal::new(egui::Id::unique("askpass-prompt"))
        .frame(crate::theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading(match kind {
                PromptKind::Confirm => t!("Unknown SSH host"),
                _ => t!("Authentication"),
            });
            ui.add_space(4.0);
            ui.label(cur.req.prompt.trim());
            ui.add_space(6.0);
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            match kind {
                PromptKind::Confirm => {
                    ui.weak(t!("Check the fingerprint against the one the service publishes before trusting it."));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(crate::theme::primary(t!("Trust and connect"))).clicked() {
                            done = Some(Some("yes".into()));
                        }
                        if ui.add(crate::theme::secondary(t!("Cancel"))).clicked() {
                            done = Some(Some("no".into()));
                        }
                    });
                }
                PromptKind::Visible | PromptKind::Secret => {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut cur.answer)
                            .password(kind == PromptKind::Secret)
                            .hint_text(if kind == PromptKind::Secret {
                                t!("Password")
                            } else {
                                t!("Username")
                            })
                            .desired_width(f32::INFINITY),
                    );
                    if cur.focus {
                        resp.request_focus();
                        cur.focus = false;
                    }
                    ui.checkbox(
                        &mut prefs.remember_answers,
                        t!("Remember until Ferrum closes"),
                    );
                    if cur.req.prompt.contains("://") {
                        ui.weak(t!("A git credential helper, when set up, saves it for next time."));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(crate::theme::primary(t!("OK"))).clicked() || (enter && resp.lost_focus()) {
                            done = Some(Some(cur.answer.clone()));
                        }
                        if ui.add(crate::theme::secondary(t!("Cancel"))).clicked() {
                            done = Some(None);
                        }
                    });
                }
            }
        });
    if let Some(answer) = done
        && let Some(cur) = p.current.take()
    {
        if prefs.remember_answers
            && kind != PromptKind::Confirm
            && let Some(a) = &answer
        {
            p.cache
                .insert(cur.req.prompt.clone(), (a.clone(), Instant::now()));
        }
        cur.req.answer(answer);
    }
}

// ----- Trusting a host -----

pub struct TrustWindow {
    host: String,
    port: Option<u16>,
    scan: Option<Pending<HostKeys>>,
    keys: Option<Result<HostKeys, String>>,
    result: Option<Result<(), String>>,
}

impl TrustWindow {
    pub fn new(ctx: &egui::Context, host: String, port: Option<u16>) -> Self {
        let (h, p) = (host.clone(), port);
        let scan = Pending::spawn(ctx, move || {
            ssh::scan_host(&h, p).map_err(|e| e.to_string())
        });
        Self {
            host,
            port,
            scan: Some(scan),
            keys: None,
            result: None,
        }
    }
}

/// Returns `false` when the window is closed.
pub fn trust_window(ctx: &egui::Context, w: &mut TrustWindow) -> bool {
    if let Some(r) = crate::hosted::take_done(&mut w.scan) {
        w.keys = Some(r);
    }
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::unique("ssh-trust"));
    let modal = modal.frame(crate::theme::dialog_frame(&ctx.global_style()));
    modal.show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(t!("Trust SSH host"));
        ui.add_space(4.0);
        let target = match w.port {
            Some(p) => format!("{}:{p}", w.host),
            None => w.host.clone(),
        };
        ui.label(t!("{} is not in your known hosts, so ssh refused to connect.", target));
        ui.add_space(6.0);
        match &w.keys {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t!("Reading the host keys…"));
                });
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
            }
            Some(Ok(keys)) => {
                ui.label(t!("The server sent these key fingerprints:"));
                egui::Grid::new("ssh-trust-keys")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for (kind, fp) in &keys.fingerprints {
                            ui.label(kind);
                            ui.label(egui::RichText::new(fp).monospace());
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
                ui.weak(t!("Compare them with the fingerprints the service publishes (for example in its SSH documentation) before trusting the host."));
            }
        }
        match &w.result {
            Some(Ok(())) => {
                ui.add_space(4.0);
                ui.label(t!("Added to known_hosts. Try the operation again."));
            }
            Some(Err(e)) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
            }
            None => {}
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let trusted = matches!(w.result, Some(Ok(())));
            if let Some(Ok(keys)) = &w.keys
                && !trusted
                && ui.button(t!("Trust host")).clicked()
            {
                let file = ssh::ssh_dir().map(|d| d.join("known_hosts"));
                w.result = Some(match file {
                    Some(f) => ssh::trust_host(&f, &keys.lines).map_err(|e| e.to_string()),
                    None => Err(t!("The home folder was not found.")),
                });
            }
            let close = if trusted { t!("Close") } else { t!("Cancel") };
            if ui.button(close).clicked() {
                keep = false;
            }
        });
    });
    keep
}
