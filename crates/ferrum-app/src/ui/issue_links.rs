//! Links from commit messages to issue trackers: per-repository regex rules
//! (Jira, Redmine…) and `#123` references on GitHub / GitLab remotes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use eframe::egui;
use ferrum_core::forge::{ForgeKind, HostedRepo};
use regex::Regex;

use crate::tab::RepoTab;
use crate::ui::history_model::IssueRule;

/// A link found in a text: the matched byte range and the URL it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<usize>,
    pub url: String,
}

thread_local! {
    static COMPILED: RefCell<HashMap<String, Option<Regex>>> = RefCell::new(HashMap::new());
}

fn with_regex<T>(pattern: &str, f: impl FnOnce(&Regex) -> T) -> Option<T> {
    COMPILED.with(|c| {
        let mut c = c.borrow_mut();
        let re = c
            .entry(pattern.to_owned())
            .or_insert_with(|| Regex::new(pattern).ok());
        re.as_ref().map(f)
    })
}

/// Fills `$0`, `$1`… in `template` with the groups of a match.
fn expand(template: &str, caps: &regex::Captures) -> String {
    let mut out = template.to_owned();
    // Highest group first so `$1` does not eat the start of `$10`.
    for i in (0..caps.len()).rev() {
        let value = caps.get(i).map_or("", |m| m.as_str());
        out = out.replace(&format!("${i}"), value);
    }
    out
}

/// Links in `text`, from the rules and the forge's own `#123` references.
pub fn find(text: &str, rules: &[IssueRule], hosted: Option<&HostedRepo>) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    for rule in rules.iter().filter(|r| !r.pattern.trim().is_empty()) {
        with_regex(&rule.pattern, |re| {
            for caps in re.captures_iter(text) {
                let Some(m) = caps.get(0).filter(|m| !m.is_empty()) else {
                    continue;
                };
                links.push(Link {
                    range: m.range(),
                    url: expand(&rule.url, &caps),
                });
            }
        });
    }
    if let Some(h) = hosted {
        let pattern = match h.kind {
            ForgeKind::GitLab => r"(?:^|[\s(\[])([#!](\d+))\b",
            _ => r"(?:^|[\s(\[])(#(\d+))\b",
        };
        with_regex(pattern, |re| {
            for caps in re.captures_iter(text) {
                let (Some(m), Some(n)) = (caps.get(1), caps.get(2)) else {
                    continue;
                };
                let n = n.as_str();
                let n: u64 = n.parse().unwrap_or_default();
                let url = if m.as_str().starts_with('!') || !h.kind.has_issues() {
                    h.request_url(n)
                } else {
                    h.issue_url(n)
                };
                links.push(Link {
                    range: m.range(),
                    url,
                });
            }
        });
    }
    // Earlier rules win where matches overlap.
    let mut kept: Vec<Link> = Vec::new();
    for l in links {
        if !kept
            .iter()
            .any(|k| k.range.start < l.range.end && l.range.start < k.range.end)
        {
            kept.push(l);
        }
    }
    kept.sort_by_key(|l| l.range.start);
    kept
}

/// Checks a rule's pattern; returns the error to show.
pub fn check(pattern: &str) -> Option<String> {
    Regex::new(pattern).err().map(|e| e.to_string())
}

/// Window to edit the repository's issue link rules.
pub fn window(ctx: &egui::Context, tab: &mut RepoTab) {
    if !tab.history.ext.issue_window {
        return;
    }
    let mut open = true;
    let hosted = tab.hosted.clone();
    let rules = &mut tab.history.ext.prefs.issue_rules;
    egui::Window::new(t!("Issue links"))
        .open(&mut open)
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.label(t!("Commit summaries matching a pattern link to the URL. $1, $2… are the pattern's groups; $0 is the whole match."));
            if let Some(h) = &hosted {
                ui.weak(t!("References like #123 already link to {}.", h.web_url()));
            }
            ui.add_space(6.0);
            let mut remove = None;
            egui::Grid::new("issue-rules")
                .num_columns(3)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.weak(t!("Pattern (regex)"));
                    ui.weak(t!("Link"));
                    ui.end_row();
                    for (i, r) in rules.iter_mut().enumerate() {
                        ui.vertical(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut r.pattern)
                                    .hint_text("([A-Z]+-\\d+)")
                                    .desired_width(180.0),
                            );
                            if let Some(e) = check(&r.pattern) {
                                let first = e.lines().last().unwrap_or_default().to_owned();
                                ui.colored_label(ui.visuals().error_fg_color, first);
                            }
                        });
                        ui.add(
                            egui::TextEdit::singleline(&mut r.url)
                                .hint_text("https://jira.example.com/browse/$1")
                                .desired_width(280.0),
                        );
                        if ui.small_button(crate::icons::CLOSE).on_hover_text(t!("Remove")).clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            if let Some(i) = remove {
                rules.remove(i);
            }
            if ui.button(t!("+ Add rule")).clicked() {
                rules.push(IssueRule::default());
            }
        });
    if !open {
        tab.history.ext.issue_window = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_rule_and_forge_links() {
        let rules = [IssueRule {
            pattern: r"([A-Z]+)-(\d+)".into(),
            url: "https://jira.example.com/browse/$1-$2".into(),
        }];
        let gh = HostedRepo {
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            path: "o/r".into(),
        };
        let text = "Fix ABC-42 crash (#7), not a#8";
        let links = find(text, &rules, Some(&gh));
        assert_eq!(links.len(), 2);
        assert_eq!(&text[links[0].range.clone()], "ABC-42");
        assert_eq!(links[0].url, "https://jira.example.com/browse/ABC-42");
        assert_eq!(&text[links[1].range.clone()], "#7");
        assert_eq!(links[1].url, "https://github.com/o/r/issues/7");

        let gl = HostedRepo {
            kind: ForgeKind::GitLab,
            host: "gitlab.com".into(),
            path: "g/r".into(),
        };
        let links = find("See !12 and #3", &[], Some(&gl));
        assert_eq!(links[0].url, "https://gitlab.com/g/r/-/merge_requests/12");
        assert_eq!(links[1].url, "https://gitlab.com/g/r/-/issues/3");
        // A broken pattern is skipped, not fatal.
        let bad = [IssueRule {
            pattern: "(".into(),
            url: "x".into(),
        }];
        assert!(find("text", &bad, None).is_empty());
        assert!(check("(").is_some());
    }
}
