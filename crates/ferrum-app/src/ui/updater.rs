//! The "Update Ferrum" window, opened from the toolbar's Update button.
//! Downloads and installs the new release for the formats that can update
//! themselves (AppImage, Windows installer, macOS app); for the others it
//! says how to update.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use ferrum_core::self_update::{self, Install, Progress, Restart};
use ferrum_core::update;

use crate::theme;

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Working(Receiver<Result<Restart, String>>),
    Ready(Restart),
    Failed(String),
}

pub struct Updater {
    pub open: bool,
    install: Install,
    phase: Phase,
    progress: Progress,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            open: false,
            install: Install::current(),
            phase: Phase::Idle,
            progress: Progress::default(),
        }
    }
}

fn open_id() -> egui::Id {
    egui::Id::unique("ferrum-updater-open")
}

/// Asks for the window to open on the next frame.
pub fn request_open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(open_id(), true));
}

impl Updater {
    /// Dev aid (`FERRUM_START=update` or `update:KIND`): pretend this copy was
    /// installed as KIND (appimage, windows, mac, script, flatpak).
    pub fn dev(&mut self, kind: &str) {
        self.open = true;
        self.install = match kind {
            "appimage" => Install::AppImage("/opt/Ferrum.AppImage".into()),
            "windows" => Install::WindowsInstaller("C:/Ferrum".into()),
            "mac" => Install::MacApp("/Applications/Ferrum.app".into()),
            "script" => Install::Script,
            "flatpak" => Install::Managed("Flatpak"),
            _ => self.install.clone(),
        };
    }

    fn start(&mut self, ctx: &egui::Context, repo: &'static str) {
        let (tx, rx) = mpsc::channel();
        let install = self.install.clone();
        self.progress = Progress::default();
        let progress = self.progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let release = update::latest_release(repo)?;
                let file = self_update::download(&install, &release, &progress)?;
                self_update::install(&install, &file)
            })();
            let _ = tx.send(result.map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.phase = Phase::Working(rx);
    }
}

/// Draws the window. `available` is the (version, release page) found by
/// the daily check.
pub fn show(
    ctx: &egui::Context,
    st: &mut Updater,
    available: Option<&(String, String)>,
    repo: &'static str,
) {
    if ctx.data_mut(|d| d.remove_temp::<bool>(open_id())).is_some() {
        st.open = true;
    }
    if let Phase::Working(rx) = &st.phase {
        match rx.try_recv() {
            Ok(Ok(restart)) => st.phase = Phase::Ready(restart),
            Ok(Err(e)) => st.phase = Phase::Failed(e),
            Err(mpsc::TryRecvError::Disconnected) => {
                st.phase = Phase::Failed(t!("The update stopped unexpectedly."))
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
        }
    }
    if !st.open {
        return;
    }
    let mut open = st.open;
    let mut close = false;
    egui::Window::new(t!("Update Ferrum"))
        .id(egui::Id::unique("ferrum-updater"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .frame(theme::dialog_frame(&ctx.global_style()))
        .show(ctx, |ui| {
            ui.set_width(380.0);
            close = body(ui, st, available, repo);
        });
    st.open = open && !close;
}

/// The window's content; true when it should close.
fn body(
    ui: &mut egui::Ui,
    st: &mut Updater,
    available: Option<&(String, String)>,
    repo: &'static str,
) -> bool {
    let current = env!("CARGO_PKG_VERSION");
    let Some((version, url)) = available else {
        ui.label(t!("Ferrum {} is up to date.", current));
        return ui.add(theme::secondary(t!("Close"))).clicked();
    };
    ui.label(theme::strong(t!("Version {} is available", version)).size(16.0));
    ui.label(t!("You have version {}.", current));
    ui.add_space(theme::SPACE_M);

    let mut close = false;
    match (&st.install, &st.phase) {
        (Install::Script, _) => {
            ui.label(t!(
                "Ferrum was installed with the install command. Run it again to update:"
            ));
            let cmd = self_update::script_command();
            ui.add(egui::Label::new(egui::RichText::new(cmd).monospace()).wrap());
            if ui.button(t!("Copy command")).clicked() {
                ui.ctx().copy_text(cmd.into());
            }
        }
        (Install::Managed(by), _) => {
            let by = match *by {
                "your package manager" => t!("your package manager"),
                "manual download" => t!("a manual download"),
                other => other.to_owned(),
            };
            ui.label(t!("This copy is updated by {}.", by));
        }
        (_, Phase::Idle) => {
            ui.label(t!(
                "Ferrum downloads the new version, checks its checksum and restarts."
            ));
        }
        (_, Phase::Working(_)) => {
            let bar = match st.progress.fraction() {
                Some(f) => egui::ProgressBar::new(f).show_percentage(),
                None => egui::ProgressBar::new(0.0).animate(true),
            };
            ui.add(bar.text(t!("Downloading…")));
        }
        (_, Phase::Ready(_)) => {
            ui.label(t!("Version {} is installed. Restart to use it.", version));
        }
        (_, Phase::Failed(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, t!("The update failed: {}", e));
        }
    }
    ui.add_space(theme::SPACE_L);
    ui.horizontal(|ui| {
        let can = st.install.can_update();
        match &st.phase {
            Phase::Idle | Phase::Failed(_) if can => {
                let label = match st.phase {
                    Phase::Failed(_) => t!("Try again"),
                    _ => t!("Install and restart"),
                };
                if ui.add(theme::primary(label)).clicked() {
                    st.start(ui.ctx(), repo);
                }
            }
            Phase::Working(_) => {
                if ui.add(theme::secondary(t!("Cancel"))).clicked() {
                    st.progress.cancel.store(true, Ordering::Relaxed);
                }
            }
            Phase::Ready(restart) if ui.add(theme::primary(t!("Restart now"))).clicked() => {
                match self_update::restart(restart) {
                    Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(e) => st.phase = Phase::Failed(e.to_string()),
                }
            }
            _ => {}
        }
        if ui.button(t!("Release notes")).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        if !matches!(st.phase, Phase::Working(_)) && ui.button(t!("Close")).clicked() {
            close = true;
        }
    });
    // The Windows installer closes and restarts Ferrum by itself.
    if matches!(st.phase, Phase::Ready(Restart::Quit)) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
    close
}
