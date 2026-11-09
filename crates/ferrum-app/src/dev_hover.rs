//! `FERRUM_HOVER=X,Y` (dev only): keeps the pointer at that point, so
//! screenshots can show hover effects such as tooltips and highlights.

use eframe::egui;

pub fn apply(raw: &mut egui::RawInput) {
    // Moved once (after the first frames): a still pointer opens tooltips.
    static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) != 5 {
        return;
    }
    let Some(pos) = std::env::var("FERRUM_HOVER").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
    }) else {
        return;
    };
    raw.events.push(egui::Event::PointerMoved(pos));
}
