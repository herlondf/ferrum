//! Masks what the secret detector flags before anything is sent to a
//! model: a flagged line is replaced as a whole, and every line of a
//! sensitive file (`.env`, private keys) too.

use crate::secrets::Scanner;

/// Lines the detector does not scan (minified code, data) are dropped.
const MAX_LINE: usize = 4000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redacted {
    pub text: String,
    /// Lines masked.
    pub masked: usize,
    /// Names of what was found, e.g. "AWS access key".
    pub kinds: Vec<String>,
}

/// Masks the secrets of `text`. Diffs are followed file by file (so a
/// `.env` file in a diff is masked); other text is scanned as `path`.
pub fn redact(scanner: &Scanner, path: &str, text: &str) -> Redacted {
    let mut out = Redacted::default();
    let mut current = path.to_owned();
    let mut sensitive = scanner.scan_path(path).map(|f| f.name);
    if let Some(name) = &sensitive {
        note(&mut out, name);
    }
    let mut kept = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let newline = &line[body.len()..];
        if let Some(p) = diff_path(body) {
            current = p.to_owned();
            sensitive = scanner.scan_path(&current).map(|f| f.name);
            if let Some(name) = &sensitive {
                note(&mut out, name);
            }
            kept.push_str(line);
            continue;
        }
        let in_diff_header = body.starts_with("diff --git ")
            || body.starts_with("--- /dev/null")
            || body.starts_with("@@")
            || body.starts_with("index ");
        if let Some(name) = sensitive
            .as_ref()
            .filter(|_| !in_diff_header && !body.is_empty())
        {
            let prefix = diff_prefix(body);
            kept.push_str(&format!("{prefix}[masked by Ferrum: {name}]{newline}"));
            out.masked += 1;
            continue;
        }
        if body.len() > MAX_LINE {
            let prefix = diff_prefix(body);
            kept.push_str(&format!("{prefix}[long line left out]{newline}"));
            continue;
        }
        let findings = scanner.scan_line(&current, 0, body);
        if let Some(f) = findings.first() {
            let prefix = diff_prefix(body);
            kept.push_str(&format!(
                "{prefix}[secret masked by Ferrum: {}]{newline}",
                f.name
            ));
            out.masked += 1;
            for f in &findings {
                note(&mut out, &f.name);
            }
            continue;
        }
        kept.push_str(line);
    }
    out.text = kept;
    out
}

fn note(out: &mut Redacted, name: &str) {
    if !out.kinds.iter().any(|k| k == name) {
        out.kinds.push(name.to_owned());
    }
}

/// The path of a `--- a/path` or `+++ b/path` line of a diff.
fn diff_path(line: &str) -> Option<&str> {
    if let Some(p) = line.strip_prefix("--- a/") {
        return Some(p);
    }
    let p = line.strip_prefix("+++ ")?;
    if p == "/dev/null" {
        return None;
    }
    Some(p.strip_prefix("b/").unwrap_or(p))
}

/// The `+`, `-` or space of a diff line, kept so the diff still reads.
fn diff_prefix(line: &str) -> &str {
    match line.chars().next() {
        Some(c @ ('+' | '-' | ' ')) => &line[..c.len_utf8()],
        _ => "",
    }
}
