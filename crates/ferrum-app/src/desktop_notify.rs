//! Desktop notifications (D-Bus on Linux, Notification Center on macOS,
//! toasts on Windows) for long operations that finish in the background.

use crate::jobs::Outcome;

/// Shows "Fetch — done" or "Fetch failed: …". Blocks briefly; call it off the UI thread.
pub fn operation_finished(label: &str, outcome: &Outcome) {
    let body = match outcome {
        Outcome::Succeeded => t!("{} — done", label),
        Outcome::Failed(e) => t!("{} failed: {}", label, e.lines().next().unwrap_or_default()),
        Outcome::Cancelled => return,
    };
    show(&body);
}

#[cfg(not(test))]
fn show(body: &str) {
    let _ = notify_rust::Notification::new()
        .appname("Ferrum")
        .summary("Ferrum")
        .body(body)
        .show();
}

#[cfg(test)]
fn show(_body: &str) {}
