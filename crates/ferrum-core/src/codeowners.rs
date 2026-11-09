//! CODEOWNERS files (GitHub, GitLab): who owns a path.
//!
//! Patterns follow gitignore rules as GitHub documents them: a pattern
//! without a slash matches at any depth, one with a slash is anchored at the
//! root, a trailing slash matches a folder's contents, `*` stays within a
//! folder and `**` crosses folders. The last matching line wins.

use std::path::Path;

/// Where the file is looked for, in GitHub's order.
pub const LOCATIONS: [&str; 4] = [
    ".github/CODEOWNERS",
    "CODEOWNERS",
    "docs/CODEOWNERS",
    ".gitlab/CODEOWNERS",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub pattern: String,
    pub owners: Vec<String>,
    /// 1-based line in the file.
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeOwners {
    /// The file it was read from (relative to the repository).
    pub file: String,
    pub rules: Vec<Rule>,
}

impl CodeOwners {
    pub fn parse(file: &str, text: &str) -> Self {
        let mut rules = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            // GitLab sections: "[Section]" or "^[Optional]".
            if line.is_empty() || line.starts_with('[') || line.starts_with("^[") {
                continue;
            }
            let mut parts = split_unescaped(line).into_iter();
            let Some(pattern) = parts.next() else {
                continue;
            };
            rules.push(Rule {
                pattern,
                owners: parts.collect(),
                line: i + 1,
            });
        }
        Self {
            file: file.to_owned(),
            rules,
        }
    }

    /// Reads the first CODEOWNERS file found in the working copy at `root`.
    pub fn load(root: &Path) -> Option<Self> {
        LOCATIONS.iter().find_map(|rel| {
            let text = std::fs::read_to_string(root.join(rel)).ok()?;
            Some(Self::parse(rel, &text))
        })
    }

    /// The rule that decides who owns `path` (the last match), if any. A
    /// rule with no owners means nobody owns the path.
    pub fn rule_for(&self, path: &str) -> Option<&Rule> {
        let path = path.trim_start_matches('/');
        self.rules.iter().rev().find(|r| matches(&r.pattern, path))
    }

    pub fn owners(&self, path: &str) -> &[String] {
        self.rule_for(path).map_or(&[], |r| r.owners.as_slice())
    }
}

fn strip_comment(line: &str) -> &str {
    let b = line.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'#' && (i == 0 || b[i - 1] != b'\\') {
            return &line[..i];
        }
    }
    line
}

/// Splits on whitespace, keeping `\ ` and `\#` escapes as the plain character.
fn split_unescaped(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if matches!(chars.peek(), Some(' ') | Some('#')) => {
                cur.push(chars.next().unwrap_or(' '));
            }
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether CODEOWNERS `pattern` matches the file `path` (no leading slash).
pub fn matches(pattern: &str, path: &str) -> bool {
    let dir_only = pattern.ends_with('/');
    let pat = pattern.trim_end_matches('/');
    if pat.is_empty() || pat == "*" && !dir_only {
        return !path.is_empty();
    }
    let anchored = pat.starts_with('/') || pat.trim_start_matches('/').contains('/');
    let pat = pat.trim_start_matches('/');
    // GitHub: "docs/*" covers the files in docs, not those in its folders.
    let files_only = pat.ends_with("/*");
    let parts: Vec<&str> = path.split('/').collect();
    // Candidates: the file itself and each folder above it (a folder match
    // covers everything inside). Unanchored patterns may start at any depth.
    let starts: Vec<usize> = if anchored {
        vec![0]
    } else {
        (0..parts.len()).collect()
    };
    for s in starts {
        for end in s + 1..=parts.len() {
            if dir_only && end == parts.len() || files_only && end < parts.len() {
                continue;
            }
            let candidate = parts[s..end].join("/");
            if glob(pat.as_bytes(), candidate.as_bytes()) {
                return true;
            }
        }
    }
    false
}

/// `*` and `?` within a path part, `**` across parts.
fn glob(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if p.starts_with(b"**") {
        let rest = &p[2..];
        if rest.is_empty() {
            return true;
        }
        if let Some(rest) = rest.strip_prefix(b"/") {
            // "**/x": x here, or after any slash.
            if glob(rest, s) {
                return true;
            }
            return (0..s.len()).any(|i| s[i] == b'/' && glob(rest, &s[i + 1..]));
        }
        return (0..=s.len()).any(|i| glob(rest, &s[i..]));
    }
    match p[0] {
        b'*' => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != b'/')
            .any(|i| glob(&p[1..], &s[i..])),
        b'?' => !s.is_empty() && s[0] != b'/' && glob(&p[1..], &s[1..]),
        c => !s.is_empty() && s[0] == c && glob(&p[1..], &s[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_follow_gitignore_rules() {
        assert!(matches("*", "a/b.rs"));
        assert!(matches("*.js", "src/app/x.js"));
        assert!(!matches("*.js", "src/app/x.jsx"));
        assert!(matches("/build/logs/", "build/logs/a/b.log"));
        assert!(!matches("/build/logs/", "x/build/logs/a.log"));
        assert!(matches("docs/*", "docs/getting-started.md"));
        assert!(!matches("docs/*", "docs/build-app/troubleshooting.md"));
        assert!(matches("apps/", "foo/apps/x.rs"));
        assert!(!matches("apps/", "apps"));
        assert!(matches("/docs/", "docs/a/b.md"));
        assert!(matches("**/logs", "deep/down/logs/x.txt"));
        assert!(matches("/scripts", "scripts/x.sh"));
        assert!(matches("src/**/test.rs", "src/a/b/test.rs"));
        assert!(matches("src/**/test.rs", "src/test.rs"));
        assert!(matches("Makefile", "sub/Makefile"));
        assert!(matches("a?c.txt", "abc.txt"));
        assert!(!matches("a?c.txt", "a/c.txt"));
    }

    #[test]
    fn last_matching_rule_wins() {
        let text = "\
# Default owners
*       @org/everyone
*.js    @js-owner  # trailing comment
[Docs section]
/docs/  @docs-team docs@example.com
/docs/generated/
my\\ file.txt @spaces
";
        let co = CodeOwners::parse("CODEOWNERS", text);
        assert_eq!(co.rules.len(), 5);
        assert_eq!(co.owners("src/main.rs"), ["@org/everyone"]);
        assert_eq!(co.owners("web/app.js"), ["@js-owner"]);
        assert_eq!(co.owners("docs/a.md"), ["@docs-team", "docs@example.com"]);
        let r = co.rule_for("docs/generated/x.md").unwrap();
        assert!(r.owners.is_empty());
        assert_eq!(r.line, 6);
        assert_eq!(co.owners("my file.txt"), ["@spaces"]);
    }

    #[test]
    fn loads_from_known_locations() {
        let dir = tempfile::tempdir().unwrap();
        assert!(CodeOwners::load(dir.path()).is_none());
        std::fs::create_dir_all(dir.path().join(".github")).unwrap();
        std::fs::write(dir.path().join(".github/CODEOWNERS"), "* @a\n").unwrap();
        std::fs::write(dir.path().join("CODEOWNERS"), "* @b\n").unwrap();
        let co = CodeOwners::load(dir.path()).unwrap();
        assert_eq!(co.file, ".github/CODEOWNERS");
        assert_eq!(co.owners("x"), ["@a"]);
    }
}
