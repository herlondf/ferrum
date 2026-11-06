//! Commit message text: subject and description, wrapping and trailers.

/// Splits a message into its subject (first line) and description (the rest,
/// without the blank line in between).
pub fn split(message: &str) -> (String, String) {
    let (subject, rest) = message.split_once('\n').unwrap_or((message, ""));
    let body = rest.strip_prefix('\n').unwrap_or(rest);
    (subject.trim_end_matches('\r').to_owned(), body.to_owned())
}

/// The reverse of [`split`].
pub fn join(subject: &str, body: &str) -> String {
    if body.is_empty() {
        subject.to_owned()
    } else {
        format!("{subject}\n\n{body}")
    }
}

/// `Signed-off-by: …`, `Co-authored-by: …` and similar lines.
pub fn is_trailer(line: &str) -> bool {
    let Some((key, value)) = line.split_once(": ") else {
        return false;
    };
    !key.is_empty()
        && !value.trim().is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn list_marker(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let indent = line.len() - t.len();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return Some(indent + 2);
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    (digits > 0 && t[digits..].starts_with(". ")).then_some(indent + digits + 2)
}

fn wrap_words(
    words: &[&str],
    first_prefix: &str,
    prefix: &str,
    width: usize,
    out: &mut Vec<String>,
) {
    let mut line = first_prefix.to_owned();
    let mut empty = true;
    for w in words {
        let len = line.chars().count();
        if !empty && len + 1 + w.chars().count() > width {
            out.push(std::mem::replace(&mut line, prefix.to_owned()));
            empty = true;
        }
        if !empty {
            line.push(' ');
        }
        line.push_str(w);
        empty = false;
    }
    out.push(line);
}

/// Re-wraps the paragraphs of a description at `width` columns. Indented
/// lines (code), quotes and trailers are kept as they are; list items wrap
/// with a hanging indent.
pub fn wrap(text: &str, width: usize) -> String {
    let width = width.max(10);
    let mut out: Vec<String> = Vec::new();
    // Words of the paragraph being collected, and its prefixes.
    let mut words: Vec<&str> = Vec::new();
    let mut first = String::new();
    let mut hang = String::new();
    let flush = |words: &mut Vec<&str>, first: &str, hang: &str, out: &mut Vec<String>| {
        if !words.is_empty() {
            wrap_words(words, first, hang, width, out);
            words.clear();
        }
    };
    for line in text.lines() {
        let verbatim = line.starts_with([' ', '\t']) && list_marker(line).is_none()
            || line.starts_with('>')
            || is_trailer(line);
        if line.trim().is_empty() || verbatim {
            flush(&mut words, &first, &hang, &mut out);
            out.push(line.trim_end().to_owned());
            continue;
        }
        if let Some(n) = list_marker(line) {
            flush(&mut words, &first, &hang, &mut out);
            first = line[..n].to_owned();
            hang = " ".repeat(n);
            words.extend(line[n..].split_whitespace());
            continue;
        }
        if words.is_empty() {
            first.clear();
            hang.clear();
        }
        words.extend(line.split_whitespace());
    }
    flush(&mut words, &first, &hang, &mut out);
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// For a `Co-authored-by:` line being typed, the text after the colon.
pub fn co_author_query(line: &str) -> Option<&str> {
    const KEY: &str = "co-authored-by:";
    let head = line.get(..KEY.len())?;
    head.eq_ignore_ascii_case(KEY)
        .then(|| line[KEY.len()..].trim())
}

/// Cleans the answer of an AI tool: drops Markdown code fences and blank
/// lines around the message.
pub fn clean_generated(text: &str) -> String {
    let lines: Vec<&str> = text
        .trim()
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .collect();
    lines.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_join_round_trip() {
        assert_eq!(split("Fix bug"), ("Fix bug".into(), String::new()));
        assert_eq!(
            split("Fix\n\nMore\n\nText"),
            ("Fix".into(), "More\n\nText".into())
        );
        assert_eq!(join("Fix", ""), "Fix");
        assert_eq!(join("Fix", "Body"), "Fix\n\nBody");
        assert_eq!(join("", "Body"), "\n\nBody");
        for m in ["a", "a\n\nb", "\n\nb", "a\n\nb\n\nc\n"] {
            let (s, b) = split(m);
            assert_eq!(join(&s, &b), m);
        }
    }

    #[test]
    fn wraps_paragraphs_and_keeps_the_rest() {
        let text = "one two three four five six\nseven\n\n- item one two three four\n    code stays as it is\nCo-authored-by: A <a@b.c>";
        let wrapped = wrap(text, 14);
        assert_eq!(
            wrapped,
            "one two three\nfour five six\nseven\n\n- item one two\n  three four\n    code stays as it is\nCo-authored-by: A <a@b.c>"
        );
        // A long word gets its own line.
        assert_eq!(
            wrap("a https://example.com/very/long b", 10),
            "a\nhttps://example.com/very/long\nb"
        );
    }

    #[test]
    fn trailers_and_co_authors() {
        assert!(is_trailer("Signed-off-by: A <a@b.c>"));
        assert!(!is_trailer("Note: "));
        assert!(!is_trailer("this is: not one"));
        assert_eq!(co_author_query("Co-authored-by: jo"), Some("jo"));
        assert_eq!(co_author_query("co-authored-by:"), Some(""));
        assert_eq!(co_author_query("Co-author"), None);
    }

    #[test]
    fn cleans_generated_messages() {
        assert_eq!(
            clean_generated("\n```text\nAdd x\n\nBecause y\n```\n"),
            "Add x\n\nBecause y"
        );
    }
}
