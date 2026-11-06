//! In the clone dialog: the repositories of the logged-in accounts, with a
//! search box and the choice of SSH or HTTPS URLs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui;
use ferrum_core::forge::{self, RemoteRepo};

use crate::hosted::{Pending, take_done};
use crate::settings::Settings;

#[derive(Default)]
struct PickerState {
    account: usize,
    search: String,
    ssh: bool,
    /// Loaded lists by account (kind, host, username).
    repos: HashMap<String, Result<Vec<RemoteRepo>, String>>,
    pending: Option<(String, Pending<Vec<RemoteRepo>>)>,
}

type Shared = Arc<Mutex<PickerState>>;

fn account_key(a: &crate::settings::ForgeAccount) -> String {
    format!("{:?}|{}|{}", a.kind, a.host, a.username)
}

/// Loads the repositories of `account` off the UI thread.
pub fn start_load(
    ctx: &egui::Context,
    a: &crate::settings::ForgeAccount,
) -> Pending<Vec<RemoteRepo>> {
    let (kind, host, creds) = (a.kind, a.host.clone(), a.credentials());
    Pending::spawn(ctx, move || {
        forge::list_repositories(kind, &host, &creds).map_err(|e| e.to_string())
    })
}

/// Repositories whose name or description has every word of `search`.
pub fn matching<'a>(repos: &'a [RemoteRepo], search: &str) -> Vec<&'a RemoteRepo> {
    let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
    repos
        .iter()
        .filter(|r| {
            let text = format!("{} {}", r.name, r.description).to_lowercase();
            words.iter().all(|w| text.contains(w))
        })
        .collect()
}

/// Shows the picker. Returns true when a repository was picked into `url`.
pub fn picker(ui: &mut egui::Ui, settings: &Settings, url: &mut String) -> bool {
    if settings.accounts.is_empty() {
        return false;
    }
    let id = egui::Id::unique("clone-repo-picker");
    let shared: Shared = ui.data_mut(|d| d.get_temp_mut_or_default::<Shared>(id).clone());
    let mut st = shared.lock().unwrap_or_else(|e| e.into_inner());
    let ctx = ui.ctx().clone();
    if let Some((key, pending)) = st.pending.take() {
        let mut slot = Some(pending);
        match take_done(&mut slot) {
            Some(r) => {
                st.repos.insert(key, r);
            }
            None => st.pending = slot.map(|p| (key, p)),
        }
    }
    st.account = st.account.min(settings.accounts.len() - 1);
    let account = &settings.accounts[st.account];
    let key = account_key(account);
    if !st.repos.contains_key(&key) && st.pending.is_none() {
        st.pending = Some((key.clone(), start_load(&ctx, account)));
    }
    let mut picked = false;
    egui::CollapsingHeader::new(t!("From your accounts"))
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let label = |a: &crate::settings::ForgeAccount| {
                    format!("{} · {} · {}", a.kind.name(), a.host, a.username)
                };
                egui::ComboBox::from_id_salt("clone-account")
                    .selected_text(label(account))
                    .show_ui(ui, |ui| {
                        for (i, a) in settings.accounts.iter().enumerate() {
                            ui.selectable_value(&mut st.account, i, label(a));
                        }
                    });
                ui.radio_value(&mut st.ssh, false, "HTTPS");
                ui.radio_value(&mut st.ssh, true, "SSH");
                if ui.small_button("⟳").on_hover_text(t!("Reload")).clicked() {
                    st.repos.remove(&key);
                }
            });
            ui.add(
                egui::TextEdit::singleline(&mut st.search)
                    .hint_text(t!("Search repositories"))
                    .desired_width(f32::INFINITY),
            );
            match st.repos.get(&key) {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.weak(t!("Loading repositories…"));
                    });
                }
                Some(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, format!("⚠ {e}"));
                }
                Some(Ok(repos)) => {
                    let list = matching(repos, &st.search);
                    let ssh = st.ssh;
                    egui::ScrollArea::vertical()
                        .id_salt("clone-repos")
                        .max_height(170.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if list.is_empty() {
                                ui.weak(t!("No repositories match"));
                            }
                            for r in list {
                                let link = if ssh && !r.ssh_url.is_empty() {
                                    &r.ssh_url
                                } else {
                                    &r.https_url
                                };
                                let mut text = r.name.clone();
                                if r.private {
                                    text.push_str("  🔒");
                                }
                                let on = url.trim() == link;
                                let resp = ui.selectable_label(on, text);
                                let resp = if r.description.is_empty() {
                                    resp.on_hover_text(link)
                                } else {
                                    resp.on_hover_text(format!("{}\n{link}", r.description))
                                };
                                if resp.clicked() {
                                    *url = link.clone();
                                    picked = true;
                                }
                            }
                        });
                }
            }
        });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_repositories() {
        let r = |name: &str, d: &str| RemoteRepo {
            name: name.into(),
            description: d.into(),
            https_url: String::new(),
            ssh_url: String::new(),
            private: false,
        };
        let repos = [
            r("ana/ferrum", "Git client"),
            r("ana/notes", ""),
            r("team/api", "REST server"),
        ];
        let names = |s: &str| {
            matching(&repos, s)
                .iter()
                .map(|r| r.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("").len(), 3);
        assert_eq!(names("GIT"), ["ana/ferrum"]);
        assert_eq!(names("ana no"), ["ana/notes"]);
        assert!(names("zzz").is_empty());
    }
}
