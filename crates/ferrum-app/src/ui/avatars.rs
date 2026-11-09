//! Author avatars in the commit list. Pictures are downloaded on background
//! threads (Gravatar, GitHub) and cached on disk; until one arrives, or when
//! downloads are off, a colored circle with the author's initials is shown.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Rect};

const WORKERS: usize = 4;

enum Slot {
    Loading,
    Ready(egui::TextureHandle),
    Missing,
}

type Results = Arc<Mutex<Vec<(String, Option<egui::ColorImage>)>>>;

struct Avatars {
    slots: HashMap<String, Slot>,
    requests: Option<Sender<String>>,
    results: Results,
}

thread_local! {
    static AVATARS: RefCell<Avatars> = RefCell::new(Avatars {
        slots: HashMap::new(),
        requests: None,
        results: Arc::default(),
    });
}

/// Where downloaded pictures are kept.
fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("FERRUM_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("FERRUM_SCREENSHOT")
                .map(|_| std::env::temp_dir().join("ferrum-screenshot"))
        })
        .or_else(|| eframe::storage_dir("ferrum"))?;
    Some(base.join("avatars"))
}

fn start_workers(ctx: &egui::Context, results: Results) -> Option<Sender<String>> {
    let dir = cache_dir()?;
    let (tx, rx) = mpsc::channel::<String>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..WORKERS {
        let (rx, results, ctx, dir): (Arc<Mutex<Receiver<String>>>, _, _, _) =
            (rx.clone(), results.clone(), ctx.clone(), dir.clone());
        std::thread::spawn(move || {
            loop {
                let Ok(email) = rx
                    .lock()
                    .map_err(|_| ())
                    .and_then(|r| r.recv().map_err(|_| ()))
                else {
                    return;
                };
                let image = ferrum_core::avatar::fetch(&dir, &email)
                    .ok()
                    .flatten()
                    .and_then(|bytes| decode(&bytes));
                if let Ok(mut r) = results.lock() {
                    r.push((email, image));
                }
                ctx.request_repaint();
            }
        });
    }
    Some(tx)
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// The picture for `email`, starting a download the first time it is asked for.
fn texture(ctx: &egui::Context, email: &str) -> Option<egui::TextureHandle> {
    // Tests never touch the network.
    if cfg!(test) {
        return None;
    }
    let key = email.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    AVATARS.with(|a| {
        let mut a = a.borrow_mut();
        let done: Vec<_> = a
            .results
            .lock()
            .map(|mut r| std::mem::take(&mut *r))
            .unwrap_or_default();
        for (email, image) in done {
            let slot = match image {
                Some(img) => Slot::Ready(ctx.load_texture(
                    format!("avatar-{email}"),
                    img,
                    egui::TextureOptions::LINEAR,
                )),
                None => Slot::Missing,
            };
            a.slots.insert(email, slot);
        }
        match a.slots.get(&key) {
            Some(Slot::Ready(t)) => return Some(t.clone()),
            Some(_) => return None,
            None => {}
        }
        if a.requests.is_none() {
            let results = a.results.clone();
            a.requests = start_workers(ctx, results);
        }
        let sent = a
            .requests
            .as_ref()
            .is_some_and(|tx| tx.send(key.clone()).is_ok());
        a.slots
            .insert(key, if sent { Slot::Loading } else { Slot::Missing });
        None
    })
}

static NETWORK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Remembers the "Download avatars" setting for views that do not get the
/// settings (the commit details). Set by the commit list every frame.
pub fn set_network(on: bool) {
    NETWORK.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn network() -> bool {
    NETWORK.load(std::sync::atomic::Ordering::Relaxed)
}

/// A stable color for an author.
pub fn author_color(email: &str) -> Color32 {
    let h = email
        .trim()
        .to_lowercase()
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    let hue = (h % 360) as f32 / 360.0;
    // Deep enough for white initials to stay readable.
    egui::ecolor::Hsva::new(hue, 0.55, 0.62, 1.0).into()
}

/// Draws the avatar of an author in `rect` (a square).
pub fn paint(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    email: &str,
    network: bool,
) {
    let r = rect.width().min(rect.height()) / 2.0;
    let center = rect.center();
    if network && let Some(tex) = texture(ui.ctx(), email) {
        let img = egui::Image::new((tex.id(), rect.size()))
            .corner_radius(r)
            .fit_to_exact_size(rect.size());
        img.paint_at(
            ui,
            Rect::from_center_size(center, egui::vec2(2.0 * r, 2.0 * r)),
        );
        return;
    }
    painter.circle_filled(center, r, author_color(email));
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        ferrum_core::avatar::initials(name),
        crate::theme::strong_font(r * 0.85),
        Color32::WHITE,
    );
}
