//! Text helpers for the diff view: syntax highlighting, word-level changes,
//! whitespace display and the layout of one line.

use std::ops::Range;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontId, text::LayoutJob, text::TextFormat};
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Lines longer than this are not highlighted (minified files).
const MAX_HIGHLIGHT_LEN: usize = 2000;
/// Time spent highlighting per frame; the rest continues on the next frames.
const FRAME_BUDGET: Duration = Duration::from_millis(12);

fn theme(dark: bool) -> &'static Theme {
    let name = if dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    };
    &THEMES.themes[name]
}

/// The syntax for a file name, if one is known.
pub fn syntax_for(path: &str) -> Option<&'static SyntaxReference> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map_or(name, |(_, e)| e);
    let lower = ext.to_ascii_lowercase();
    // Close relatives for languages the default set lacks.
    let alias = match lower.as_str() {
        "ts" | "tsx" | "mts" | "cts" | "jsx" | "mjs" | "cjs" => "js",
        "kt" | "kts" => "java",
        "vue" | "svelte" => "html",
        "h" | "hpp" | "hh" | "hxx" => "cpp",
        other => other,
    };
    let ss = &*SYNTAXES;
    ss.find_syntax_by_extension(name)
        .or_else(|| ss.find_syntax_by_extension(alias))
        .filter(|s| s.name != "Plain Text")
}

/// Colored ranges of one line: (byte range, color).
pub type Spans = Vec<(Range<usize>, Color32)>;

/// Highlights the lines of one side of a file in order, a few at a time.
pub struct SideHighlighter {
    syntax: &'static SyntaxReference,
    highlighter: Highlighter<'static>,
    parse: ParseState,
    state: HighlightState,
    /// Colors of the lines highlighted so far.
    pub done: Vec<Spans>,
}

impl SideHighlighter {
    pub fn new(syntax: &'static SyntaxReference, dark: bool) -> Self {
        let highlighter = Highlighter::new(theme(dark));
        let state = HighlightState::new(&highlighter, ScopeStack::new());
        Self {
            syntax,
            highlighter,
            parse: ParseState::new(syntax),
            state,
            done: Vec::new(),
        }
    }

    /// Highlights `lines` up to `upto` (exclusive) within the frame budget.
    /// Returns false when it ran out of time.
    pub fn advance<'a>(&mut self, lines: impl Fn(usize) -> &'a str, upto: usize) -> bool {
        let start = Instant::now();
        while self.done.len() < upto {
            if start.elapsed() > FRAME_BUDGET {
                return false;
            }
            let text = lines(self.done.len());
            if text.len() > MAX_HIGHLIGHT_LEN {
                self.done.push(Vec::new());
                continue;
            }
            let line = format!("{text}\n");
            let Ok(ops) = self.parse.parse_line(&line, &SYNTAXES) else {
                // A broken grammar state: start over for the rest.
                self.parse = ParseState::new(self.syntax);
                self.done.push(Vec::new());
                continue;
            };
            let mut spans = Vec::new();
            let mut pos = 0;
            for (style, piece) in
                HighlightIterator::new(&mut self.state, &ops, &line, &self.highlighter)
            {
                let end = (pos + piece.len()).min(text.len());
                if end > pos {
                    let c = style.foreground;
                    spans.push((pos..end, Color32::from_rgb(c.r, c.g, c.b)));
                }
                pos += piece.len();
            }
            self.done.push(spans);
        }
        true
    }
}

/// Byte ranges that differ between two versions of a line, token by token.
/// Empty when the lines have too little in common to be worth marking.
pub fn word_changes(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if old.len() > MAX_HIGHLIGHT_LEN || new.len() > MAX_HIGHLIGHT_LEN {
        return (Vec::new(), Vec::new());
    }
    let (a, b) = (tokens(old), tokens(new));
    let strs = |t: &[Range<usize>], s: &'_ str| -> Vec<String> {
        t.iter().map(|r| s[r.clone()].to_owned()).collect()
    };
    let (sa, sb) = (strs(&a, old), strs(&b, new));
    let ops = similar::capture_diff_slices(similar::Algorithm::Myers, &sa, &sb);
    let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
    let mut same = 0;
    for op in ops {
        let (tag, ra, rb) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            same += a[ra].iter().map(|r| r.len()).sum::<usize>();
            continue;
        }
        if !ra.is_empty() {
            push_merged(&mut out_a, a[ra.start].start..a[ra.end - 1].end);
        }
        if !rb.is_empty() {
            push_merged(&mut out_b, b[rb.start].start..b[rb.end - 1].end);
        }
    }
    // Mostly rewritten lines: marking nearly everything only adds noise.
    let longest = old.trim().len().max(new.trim().len()).max(1);
    if same * 4 < longest {
        return (Vec::new(), Vec::new());
    }
    (out_a, out_b)
}

fn push_merged(out: &mut Vec<Range<usize>>, r: Range<usize>) {
    match out.last_mut() {
        Some(last) if last.end == r.start => last.end = r.end,
        _ => out.push(r),
    }
}

/// Splits a line into words, single punctuation characters and whitespace runs.
fn tokens(s: &str) -> Vec<Range<usize>> {
    #[derive(PartialEq)]
    enum K {
        Word,
        Space,
        Other,
    }
    let kind = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            K::Word
        } else if c.is_whitespace() {
            K::Space
        } else {
            K::Other
        }
    };
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut prev: Option<K> = None;
    for (i, c) in s.char_indices() {
        let k = kind(c);
        let joins = prev.as_ref() == Some(&k) && k != K::Other;
        match out.last_mut() {
            Some(last) if joins => last.end = i + c.len_utf8(),
            _ => out.push(i..i + c.len_utf8()),
        }
        prev = Some(k);
    }
    out
}

/// Byte ranges of `query` in `text`, ignoring ASCII case.
pub fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() || query.len() > text.len() {
        return Vec::new();
    }
    let (t, q) = (text.to_lowercase(), query.to_lowercase());
    // Lowercasing can change byte lengths outside ASCII; only trust equal lengths.
    if t.len() != text.len() {
        return text
            .match_indices(query)
            .map(|(i, m)| i..i + m.len())
            .collect();
    }
    t.match_indices(&q).map(|(i, m)| i..i + m.len()).collect()
}

/// How one line is drawn.
pub struct LineStyle<'a> {
    pub font: FontId,
    pub color: Color32,
    pub weak: Color32,
    pub syntax: Option<&'a Spans>,
    pub words: &'a [Range<usize>],
    pub word_bg: Color32,
    pub found: &'a [Range<usize>],
    pub found_bg: Color32,
    pub current: Option<Range<usize>>,
    pub current_bg: Color32,
    pub tab_width: usize,
    pub show_whitespace: bool,
}

/// Number of columns `text` takes once tabs are expanded.
pub fn display_width(text: &str, tab_width: usize) -> usize {
    let mut col = 0;
    for c in text.chars() {
        col += if c == '\t' {
            tab_width - col % tab_width
        } else {
            1
        };
    }
    col
}

/// Lays out one line: tabs expanded, whitespace optionally shown, syntax
/// colors and backgrounds for changed words and search matches.
pub fn layout_line(text: &str, s: &LineStyle) -> LayoutJob {
    let mut job = LayoutJob::default();
    let in_any = |ranges: &[Range<usize>], i: usize| ranges.iter().any(|r| r.contains(&i));
    let mut syntax_i = 0;
    let mut col = 0;
    let mut buf = String::new();
    let mut fmt: Option<TextFormat> = None;
    for (i, c) in text.char_indices() {
        let mut color = s.color;
        if let Some(spans) = s.syntax {
            while syntax_i < spans.len() && spans[syntax_i].0.end <= i {
                syntax_i += 1;
            }
            if let Some((r, c)) = spans.get(syntax_i)
                && r.contains(&i)
            {
                color = *c;
            }
        }
        let background = if s.current.as_ref().is_some_and(|r| r.contains(&i)) {
            s.current_bg
        } else if in_any(s.found, i) {
            s.found_bg
        } else if in_any(s.words, i) {
            s.word_bg
        } else {
            Color32::TRANSPARENT
        };
        let (piece, color) = match c {
            '\t' => {
                let n = s.tab_width - col % s.tab_width;
                col += n;
                if s.show_whitespace {
                    (format!("»{}", " ".repeat(n - 1)), s.weak)
                } else {
                    (" ".repeat(n), color)
                }
            }
            ' ' if s.show_whitespace => {
                col += 1;
                ("·".to_owned(), s.weak)
            }
            _ => {
                col += 1;
                (c.to_string(), color)
            }
        };
        let f = TextFormat {
            font_id: s.font.clone(),
            color,
            background,
            ..Default::default()
        };
        if fmt.as_ref() != Some(&f) {
            if let Some(prev) = fmt.take() {
                job.append(&std::mem::take(&mut buf), 0.0, prev);
            }
            fmt = Some(f);
        }
        buf.push_str(&piece);
    }
    if let Some(prev) = fmt {
        job.append(&buf, 0.0, prev);
    }
    job
}

/// Adds a weak note (like "no newline at end") after a line.
pub fn append_note(job: &mut LayoutJob, note: &str, s: &LineStyle) {
    job.append(
        note,
        0.0,
        TextFormat {
            font_id: s.font.clone(),
            color: s.weak,
            ..Default::default()
        },
    );
}

/// Wraps or clips a job at `width`.
pub fn set_wrap(job: &mut LayoutJob, width: Option<f32>) {
    match width {
        Some(w) => {
            job.wrap = egui::text::TextWrapping {
                max_width: w.max(20.0),
                break_anywhere: true,
                ..Default::default()
            }
        }
        None => job.wrap = egui::text::TextWrapping::no_max_width(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_changed_tokens() {
        let (a, b) = word_changes("let x = foo(bar);", "let x = foo(baz);");
        assert_eq!(a, vec![12..15]);
        assert_eq!(b, vec![12..15]);
        let (a, b) = word_changes("call(a, b)", "call(a, b, c)");
        assert!(a.is_empty());
        assert_eq!(b, vec![9..12]);
        // Unrelated lines are not marked word by word.
        let (a, b) = word_changes("completely different", "nothing alike here!");
        assert!(a.is_empty() && b.is_empty());
    }

    #[test]
    fn finds_matches_ignoring_case() {
        assert_eq!(find_all("Foo foo FOO", "foo"), [0..3, 4..7, 8..11]);
        assert!(find_all("abc", "").is_empty());
        assert_eq!(find_all("ação Ação", "ação").len(), 2);
        // Lowercase changes the length of "İ": fall back to exact matches.
        assert_eq!(find_all("İx ix", "ix"), vec![4..6]);
    }

    #[test]
    fn expands_tabs_and_shows_whitespace() {
        assert_eq!(display_width("\tab", 4), 6);
        assert_eq!(display_width("a\tb", 4), 5);
        let style = LineStyle {
            font: FontId::monospace(12.0),
            color: Color32::WHITE,
            weak: Color32::GRAY,
            syntax: None,
            words: &[],
            word_bg: Color32::RED,
            found: &[],
            found_bg: Color32::YELLOW,
            current: None,
            current_bg: Color32::YELLOW,
            tab_width: 4,
            show_whitespace: true,
        };
        assert_eq!(layout_line("a b\tc", &style).text, "a·b»c");
        let plain = LineStyle {
            show_whitespace: false,
            ..style
        };
        assert_eq!(layout_line("\tx", &plain).text, "    x");
    }

    #[test]
    fn highlights_known_languages() {
        assert!(syntax_for("src/main.rs").is_some());
        assert!(syntax_for("web/app.tsx").is_some());
        assert!(syntax_for("notes.unknownext").is_none());
        let syntax = syntax_for("a.rs").unwrap();
        let mut h = SideHighlighter::new(syntax, true);
        let lines = ["fn main() {", "    let s = \"x\";", "}"];
        // The first lines compile the grammar, which may take a few frames.
        while !h.advance(|i| lines[i], 3) {}
        assert_eq!(h.done.len(), 3);
        // More than one color on the line with a keyword and a string.
        let colors: std::collections::HashSet<_> = h.done[1].iter().map(|(_, c)| *c).collect();
        assert!(colors.len() > 1);
    }
}
