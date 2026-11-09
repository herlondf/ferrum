//! Reading JSON out of model answers, which often wrap it in code fences,
//! put prose around it or think aloud first.

use serde_json::Value;

/// The first JSON object (or array) in `text`: the whole text, else a
/// fenced block, else the first balanced `{…}` / `[…]` that parses.
pub fn extract(text: &str) -> Option<Value> {
    let text = super::strip_thinking(text);
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed)
        && (v.is_object() || v.is_array())
    {
        return Some(v);
    }
    for block in fenced_blocks(trimmed) {
        if let Some(v) = first_balanced(&block) {
            return Some(v);
        }
    }
    first_balanced(trimmed)
}

/// Contents of the ``` blocks of `text`, in order (an unclosed last block
/// counts).
pub fn fenced_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            match current.take() {
                Some(block) => out.push(block),
                None => current = Some(String::new()),
            }
            continue;
        }
        if let Some(block) = &mut current {
            block.push_str(line);
            block.push('\n');
        }
    }
    if let Some(block) = current
        && !block.trim().is_empty()
    {
        out.push(block);
    }
    out
}

/// Tries every `{` or `[` from the left until a balanced value parses.
fn first_balanced(text: &str) -> Option<Value> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(off) = text[from..].find(['{', '[']) {
        let start = from + off;
        if let Some(end) = balanced_end(bytes, start)
            && let Ok(v) = serde_json::from_str::<Value>(&text[start..=end])
        {
            return Some(v);
        }
        from = start + 1;
    }
    None
}

/// Index of the bracket closing the one at `start`, skipping strings.
fn balanced_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A string field, trimmed; `None` when missing, not a string or empty
/// (numbers are turned into text).
pub fn text_field(v: &Value, key: &str) -> Option<String> {
    let s = match v.get(key)? {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let lower = s.to_lowercase();
    (!s.is_empty() && !matches!(lower.as_str(), "null" | "none" | "n/a" | "-")).then_some(s)
}
