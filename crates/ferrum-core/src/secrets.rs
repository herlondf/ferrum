//! Secret detection in what is about to be committed or pushed: tokens of
//! common providers, private keys, `.env` files and high-entropy strings.
//!
//! Only added lines are scanned. A finding is skipped when its line contains
//! `ferrum:allow` (or `gitleaks:allow`), when its path matches an allowlist
//! glob, or when its fingerprint (a hash of the secret) is allowlisted.

use std::collections::HashSet;
use std::sync::LazyLock;

use git2::{Delta, DiffOptions, Oid};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{Repo, Result, lossy};

/// Lines longer than this are skipped (minified files, data).
const MAX_LINE: usize = 4000;
/// At most this many findings are returned.
const MAX_FINDINGS: usize = 200;
/// At most this many commits are scanned before a push.
const MAX_COMMITS: usize = 200;
/// Files with more added bytes than this are skipped.
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Rule id, e.g. `aws-access-key`.
    pub rule: String,
    /// Rule name for people, e.g. "AWS access key".
    pub name: String,
    pub path: String,
    /// Line in the new file (0 for a whole-file finding).
    pub line: u32,
    /// The line, with the secret masked.
    pub text: String,
    /// Allowlist key of the secret (see [`fingerprint`]).
    pub fingerprint: String,
    /// The commit it was found in (scans before a push).
    pub commit: Option<Oid>,
}

/// A user pattern from the settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomPattern {
    pub name: String,
    pub regex: String,
}

/// Which rules run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Ids of built-in rules turned off.
    pub disabled: Vec<String>,
    pub custom: Vec<CustomPattern>,
    /// Look for long random-looking strings.
    pub entropy: bool,
    /// Report `.env` files, key files and the like.
    pub sensitive_files: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            disabled: Vec::new(),
            custom: Vec::new(),
            entropy: true,
            sensitive_files: true,
        }
    }
}

/// What a repository accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Allowlist {
    /// Fingerprints of accepted secrets.
    pub secrets: Vec<String>,
    /// Path globs (`*` within a folder, `**` across folders).
    pub paths: Vec<String>,
}

impl Allowlist {
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty() && self.paths.is_empty()
    }

    pub fn allows_path(&self, path: &str) -> bool {
        self.paths.iter().any(|g| glob_match(g.trim(), path))
    }

    pub fn add(&mut self, f: &Finding) {
        if !self.secrets.contains(&f.fingerprint) {
            self.secrets.push(f.fingerprint.clone());
        }
    }
}

/// The allowlist key of a secret: a hash, so the secret is not stored.
pub fn fingerprint(rule: &str, secret: &str) -> String {
    format!("{rule}:{:x}", md5::compute(secret.as_bytes()))
}

#[derive(Clone, Copy)]
enum Check {
    /// The match is the secret.
    None,
    /// The value must not look like a placeholder and must be varied enough.
    Value,
}

struct Rule {
    id: String,
    name: String,
    re: Regex,
    /// Lowercase strings, one of which the line must contain (fast filter).
    keywords: Vec<&'static str>,
    check: Check,
}

/// Built-in rules: (id, name, keywords, regex, check). The secret is the
/// first capture group, or the whole match.
#[rustfmt::skip]
const BUILTIN: &[(&str, &str, &[&str], &str, Check)] = &[
    ("aws-access-key", "AWS access key", &["akia", "asia", "abia", "acca"],
        r"\b((?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16})\b", Check::None),
    ("aws-secret-key", "AWS secret key", &["aws"],
        r#"(?i)aws.{0,20}?secret.{0,20}?["'\s:=]+([A-Za-z0-9/+=]{40})(?:[^A-Za-z0-9/+=]|$)"#, Check::Value),
    ("github-token", "GitHub token", &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
        r"\b((?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,255})\b", Check::None),
    ("github-pat", "GitHub fine-grained token", &["github_pat_"],
        r"\b(github_pat_[A-Za-z0-9_]{60,255})\b", Check::None),
    ("gitlab-token", "GitLab token", &["glpat-", "gldt-", "glrt-", "glptt-", "glsoat-"],
        r"\b((?:glpat|gldt|glrt|glptt|glsoat)-[A-Za-z0-9_\-]{20,})", Check::None),
    ("slack-token", "Slack token", &["xox"],
        r"\b(xox[abposr]-[A-Za-z0-9-]{10,})", Check::None),
    ("slack-webhook", "Slack webhook", &["hooks.slack.com"],
        r"(https://hooks\.slack\.com/(?:services|workflows)/[A-Za-z0-9+/_-]{20,})", Check::None),
    ("stripe-key", "Stripe key", &["sk_live_", "rk_live_", "sk_test_", "rk_test_"],
        r"\b((?:sk|rk)_(?:live|test)_[A-Za-z0-9]{16,})\b", Check::None),
    ("google-api-key", "Google API key", &["aiza"],
        r"\b(AIza[0-9A-Za-z_\-]{35})(?:[^0-9A-Za-z_\-]|$)", Check::None),
    ("google-oauth-secret", "Google OAuth client secret", &["gocspx-"],
        r"\b(GOCSPX-[A-Za-z0-9_\-]{28})", Check::None),
    ("openai-key", "OpenAI API key", &["sk-", "t3blbkfj"],
        r"\b(sk-(?:(?:proj|svcacct|admin)-[A-Za-z0-9_\-]{40,}|[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20}))", Check::None),
    ("anthropic-key", "Anthropic API key", &["sk-ant-"],
        r"\b(sk-ant-(?:api|admin)\d{2}-[A-Za-z0-9_\-]{80,})", Check::None),
    ("private-key", "Private key", &["private key"],
        r"(-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----)", Check::None),
    ("jwt", "JSON Web Token", &["eyj"],
        r"\b(eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})", Check::None),
    ("npm-token", "npm token", &["npm_"],
        r"\b(npm_[A-Za-z0-9]{36})\b", Check::None),
    ("pypi-token", "PyPI token", &["pypi-"],
        r"\b(pypi-AgEIcHlwaS5vcmc[A-Za-z0-9_\-]{50,})", Check::None),
    ("sendgrid-key", "SendGrid key", &["sg."],
        r"\b(SG\.[A-Za-z0-9_\-]{22}\.[A-Za-z0-9_\-]{43})\b", Check::None),
    ("huggingface-token", "Hugging Face token", &["hf_"],
        r"\b(hf_[A-Za-z0-9]{34,})\b", Check::None),
    ("digitalocean-token", "DigitalOcean token", &["dop_v1_", "doo_v1_"],
        r"\b((?:dop|doo)_v1_[a-f0-9]{64})\b", Check::None),
    ("azure-storage-key", "Azure storage key", &["accountkey="],
        r"(?i)AccountKey=([A-Za-z0-9+/=]{86,88})", Check::None),
    ("url-password", "Password in a URL", &["://"],
        r"\b[a-zA-Z][a-zA-Z0-9+.\-]{1,20}://[^/\s:@'\x22]{1,64}:([^/\s:@'\x22]{3,128})@[A-Za-z0-9.\-]+", Check::Value),
    ("secret-assignment", "Hard-coded secret", &["secret", "token", "passw", "pwd", "api_key", "apikey", "api-key", "access_key", "auth_key"],
        r#"(?i)(?:secret|token|passw(?:or)?d|pwd|api[_\-]?key|access[_\-]?key|auth[_\-]?key)[a-z0-9_.\-]*["']?\s*(?::|=|:=|=>)\s*["']([^"'\s]{8,})["']"#, Check::Value),
];

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    BUILTIN
        .iter()
        .map(|(id, name, keywords, re, check)| Rule {
            id: (*id).to_owned(),
            name: (*name).to_owned(),
            re: Regex::new(re).expect("built-in secret rule"),
            keywords: keywords.to_vec(),
            check: *check,
        })
        .collect()
});

/// Ids and names of the built-in rules.
pub fn builtin_rules() -> Vec<(&'static str, &'static str)> {
    BUILTIN.iter().map(|r| (r.0, r.1)).collect()
}

static ENTROPY_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    // Quoted, or the value of `key = …` / `key: …` (not a `path::name`).
    Regex::new(r#"(?:["'`]|[^:=!<>]=\s*|[^:]:\s+)([A-Za-z0-9+/_\-]{32,}={0,2})(?:["'`\s),;]|$)"#)
        .unwrap()
});

/// Shannon entropy in bits per character.
pub fn entropy(s: &str) -> f64 {
    let mut counts = [0u32; 256];
    let mut n = 0u32;
    for b in s.bytes() {
        counts[b as usize] += 1;
        n += 1;
    }
    if n == 0 {
        return 0.0;
    }
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / f64::from(n);
            -p * p.log2()
        })
        .sum()
}

/// Whether a value looks like an example rather than a real secret.
fn placeholder(v: &str) -> bool {
    let l = v.to_lowercase();
    const WORDS: &[&str] = &[
        "example",
        "changeme",
        "change_me",
        "placeholder",
        "your",
        "dummy",
        "sample",
        "redacted",
        "xxxx",
        "****",
        "....",
        "password",
        "secret",
        "token",
        "foobar",
        "<",
        ">",
        "${",
        "{{",
        "%s",
        "%(",
        "$(",
        "env(",
        "process.env",
        "getenv",
        "none",
        "null",
        "undefined",
        "todo",
    ];
    WORDS.iter().any(|w| l.contains(w))
}

/// A value has enough variety to be a password or key (not a word or a name).
fn varied(v: &str) -> bool {
    let classes = [
        v.chars().any(|c| c.is_ascii_lowercase()),
        v.chars().any(|c| c.is_ascii_uppercase()),
        v.chars().any(|c| c.is_ascii_digit()),
        v.chars().any(|c| !c.is_ascii_alphanumeric()),
    ];
    classes.iter().filter(|c| **c).count() >= 2 && entropy(v) >= 3.0
}

/// A random-looking token: mixed case and digits, high entropy, not a hash.
fn random_token(t: &str) -> bool {
    let t = t.trim_end_matches('=');
    if t.len() < 32 {
        return false;
    }
    let hex = t.chars().all(|c| c.is_ascii_hexdigit());
    let upper = t.chars().filter(|c| c.is_ascii_uppercase()).count();
    let lower = t.chars().filter(|c| c.is_ascii_lowercase()).count();
    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    // Words joined by separators (identifiers, paths, slugs).
    let longest_word = t
        .split(|c: char| !c.is_ascii_alphabetic())
        .map(str::len)
        .max()
        .unwrap_or(0);
    // Random text changes between lowercase, uppercase and digits about
    // two times in three; CamelCase names and paths much less often.
    let class = |c: char| match c {
        'a'..='z' => 0,
        'A'..='Z' => 1,
        '0'..='9' => 2,
        _ => 3,
    };
    let chars: Vec<char> = t.chars().collect();
    let switches = chars
        .windows(2)
        .filter(|w| class(w[0]) != class(w[1]))
        .count();
    let rate = switches as f64 / (chars.len() - 1) as f64;
    let integrity = ["sha256-", "sha384-", "sha512-"]
        .iter()
        .any(|p| t.starts_with(p));
    !hex && !integrity
        && t.matches('_').count() < 3
        && upper >= 3
        && lower >= 3
        && digits >= 2
        && longest_word < 16
        && rate >= 0.45
        && entropy(t) >= 4.5
}

/// Files that should not be committed whatever their content.
pub fn sensitive_file(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    if name == ".env" || (name.starts_with(".env.") && !example_env(&name)) {
        return Some("Environment file");
    }
    if matches!(
        name.as_str(),
        "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519" | ".git-credentials" | ".netrc" | ".pgpass"
    ) {
        return Some("Credentials file");
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if matches!(
        ext,
        "pem" | "key" | "p12" | "pfx" | "jks" | "keystore" | "ppk"
    ) {
        return Some("Key file");
    }
    None
}

fn example_env(name: &str) -> bool {
    ["example", "sample", "template", "dist", "defaults", "test"]
        .iter()
        .any(|w| name.ends_with(w))
}

/// Files not worth scanning for random strings (hashes and generated data).
fn skip_entropy(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    name.ends_with(".lock")
        || name.ends_with("-lock.json")
        || name.ends_with("lock.yaml")
        || name == "go.sum"
        || name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.ends_with(".map")
        || name.ends_with(".svg")
        || name.ends_with(".snap")
}

/// `*` matches within a folder, `**` across folders, `?` one character.
pub fn glob_match(glob: &str, path: &str) -> bool {
    fn go(g: &[u8], p: &[u8]) -> bool {
        match g.split_first() {
            None => p.is_empty(),
            Some((b'*', rest)) if rest.first() == Some(&b'*') => {
                let rest = rest[1..].strip_prefix(b"/").unwrap_or(&rest[1..]);
                (0..=p.len()).any(|i| go(rest, &p[i..]))
            }
            Some((b'*', rest)) => (0..=p.len())
                .take_while(|&i| i == 0 || p[i - 1] != b'/')
                .any(|i| go(rest, &p[i..])),
            Some((b'?', rest)) => !p.is_empty() && p[0] != b'/' && go(rest, &p[1..]),
            Some((c, rest)) => p.first() == Some(c) && go(rest, &p[1..]),
        }
    }
    if glob.is_empty() {
        return false;
    }
    // A glob without a slash matches the file name anywhere.
    let path = if glob.contains('/') {
        path
    } else {
        path.rsplit('/').next().unwrap_or(path)
    };
    go(glob.as_bytes(), path.as_bytes())
}

/// Masks the middle of `secret` in `line`.
fn mask(line: &str, secret: &str) -> String {
    let n = secret.chars().count();
    let masked = if n <= 8 {
        "•".repeat(n)
    } else {
        let head: String = secret.chars().take(4).collect();
        let tail: String = secret.chars().skip(n - 2).collect();
        format!("{head}{}{tail}", "•".repeat((n - 6).min(16)))
    };
    let text = line.replace(secret, &masked);
    let text = text.trim();
    if text.chars().count() > 160 {
        format!("{}…", text.chars().take(159).collect::<String>())
    } else {
        text.to_owned()
    }
}

pub struct Scanner {
    custom: Vec<Rule>,
    disabled: HashSet<String>,
    entropy: bool,
    sensitive_files: bool,
    allow: Allowlist,
}

impl Scanner {
    /// A scanner for `config`; invalid custom patterns are reported and skipped.
    pub fn new(config: &Config, allow: Allowlist) -> (Self, Vec<String>) {
        let mut errors = Vec::new();
        let mut custom = Vec::new();
        for (i, p) in config.custom.iter().enumerate() {
            if p.regex.trim().is_empty() {
                continue;
            }
            match Regex::new(&p.regex) {
                Ok(re) => custom.push(Rule {
                    id: format!("custom-{}", i + 1),
                    name: if p.name.trim().is_empty() {
                        p.regex.clone()
                    } else {
                        p.name.clone()
                    },
                    re,
                    keywords: Vec::new(),
                    check: Check::None,
                }),
                Err(e) => errors.push(format!("{}: {e}", p.regex)),
            }
        }
        let scanner = Self {
            custom,
            disabled: config.disabled.iter().cloned().collect(),
            entropy: config.entropy,
            sensitive_files: config.sensitive_files,
            allow,
        };
        (scanner, errors)
    }

    fn allowed(&self, f: &Finding) -> bool {
        self.allow.secrets.contains(&f.fingerprint)
    }

    /// Findings in one line of `path`.
    pub fn scan_line(&self, path: &str, line_no: u32, line: &str) -> Vec<Finding> {
        let mut out = Vec::new();
        if line.len() > MAX_LINE || line.contains("ferrum:allow") || line.contains("gitleaks:allow")
        {
            return out;
        }
        let lower = line.to_lowercase();
        let mut seen: HashSet<String> = HashSet::new();
        let builtin = RULES.iter().filter(|r| !self.disabled.contains(&r.id));
        for rule in builtin.chain(&self.custom) {
            if !rule.keywords.is_empty() && !rule.keywords.iter().any(|k| lower.contains(k)) {
                continue;
            }
            for caps in rule.re.captures_iter(line) {
                let Some(m) = caps.get(1).or_else(|| caps.get(0)) else {
                    continue;
                };
                let secret = m.as_str();
                if matches!(rule.check, Check::Value) && (placeholder(secret) || !varied(secret)) {
                    continue;
                }
                if !seen.insert(secret.to_owned()) {
                    continue;
                }
                out.push(Finding {
                    rule: rule.id.clone(),
                    name: rule.name.clone(),
                    path: path.to_owned(),
                    line: line_no,
                    text: mask(line, secret),
                    fingerprint: fingerprint(&rule.id, secret),
                    commit: None,
                });
            }
        }
        if self.entropy && !self.disabled.contains("entropy") && !skip_entropy(path) {
            for caps in ENTROPY_TOKEN.captures_iter(line) {
                let t = &caps[1];
                if !random_token(t)
                    || seen.iter().any(|s| s.contains(t) || t.contains(s.as_str()))
                    || line.contains(&format!("base64,{t}"))
                {
                    continue;
                }
                seen.insert(t.to_owned());
                out.push(Finding {
                    rule: "entropy".into(),
                    name: "High-entropy string".into(),
                    path: path.to_owned(),
                    line: line_no,
                    text: mask(line, t),
                    fingerprint: fingerprint("entropy", t),
                    commit: None,
                });
            }
        }
        out.retain(|f| !self.allowed(f));
        out
    }

    /// A finding for a sensitive file name, if any.
    pub fn scan_path(&self, path: &str) -> Option<Finding> {
        if !self.sensitive_files || self.disabled.contains("sensitive-file") {
            return None;
        }
        let name = sensitive_file(path)?;
        let f = Finding {
            rule: "sensitive-file".into(),
            name: name.into(),
            path: path.to_owned(),
            line: 0,
            text: String::new(),
            fingerprint: fingerprint("sensitive-file", path),
            commit: None,
        };
        (!self.allowed(&f)).then_some(f)
    }

    /// Findings in a whole text (every line counts as added).
    pub fn scan_text(&self, path: &str, text: &str) -> Vec<Finding> {
        if self.allow.allows_path(path) {
            return Vec::new();
        }
        let mut out: Vec<Finding> = self.scan_path(path).into_iter().collect();
        for (i, line) in text.lines().enumerate() {
            out.extend(self.scan_line(path, i as u32 + 1, line));
        }
        out
    }

    /// Added lines of a diff.
    fn scan_diff(&self, diff: &git2::Diff, commit: Option<Oid>, out: &mut Vec<Finding>) {
        let mut skip: HashSet<String> = HashSet::new();
        let mut bytes: std::collections::HashMap<String, usize> = Default::default();
        for delta in diff.deltas() {
            if matches!(delta.status(), Delta::Deleted) {
                continue;
            }
            let Some(path) = delta.new_file().path_bytes().map(lossy) else {
                continue;
            };
            if self.allow.allows_path(&path) {
                skip.insert(path);
                continue;
            }
            if delta.new_file().is_binary() {
                skip.insert(path.clone());
            }
            if let Some(mut f) = self.scan_path(&path) {
                f.commit = commit;
                out.push(f);
            }
        }
        let _ = diff.foreach(
            &mut |_, _| true,
            None,
            None,
            Some(&mut |delta, _hunk, line| {
                if out.len() >= MAX_FINDINGS {
                    return false;
                }
                if line.origin() != '+' {
                    return true;
                }
                let Some(path) = delta.new_file().path_bytes().map(lossy) else {
                    return true;
                };
                if skip.contains(&path) {
                    return true;
                }
                let n = bytes.entry(path.clone()).or_default();
                *n += line.content().len();
                if *n > MAX_FILE_BYTES {
                    skip.insert(path);
                    return true;
                }
                let text = String::from_utf8_lossy(line.content());
                let text = text.trim_end_matches(['\n', '\r']);
                let no = line.new_lineno().unwrap_or(0);
                for mut f in self.scan_line(&path, no, text) {
                    f.commit = commit;
                    out.push(f);
                }
                true
            }),
        );
        out.truncate(MAX_FINDINGS);
    }

    /// Findings in the staged changes (what the next commit adds).
    pub fn scan_staged(&self, repo: &Repo) -> Result<Vec<Finding>> {
        let raw = repo.raw();
        let head = raw.head().ok().and_then(|h| h.peel_to_tree().ok());
        let mut opts = DiffOptions::new();
        opts.context_lines(0);
        let index = raw.index()?;
        let diff = raw.diff_tree_to_index(head.as_ref(), Some(&index), Some(&mut opts))?;
        let mut out = Vec::new();
        self.scan_diff(&diff, None, &mut out);
        Ok(out)
    }

    /// Findings in the staged changes of an amend: everything since HEAD's parent.
    pub fn scan_amend(&self, repo: &Repo) -> Result<Vec<Finding>> {
        let raw = repo.raw();
        let parent = raw
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .and_then(|c| c.parent(0).ok())
            .and_then(|p| p.tree().ok());
        let mut opts = DiffOptions::new();
        opts.context_lines(0);
        let index = raw.index()?;
        let diff = raw.diff_tree_to_index(parent.as_ref(), Some(&index), Some(&mut opts))?;
        let mut out = Vec::new();
        self.scan_diff(&diff, None, &mut out);
        Ok(out)
    }

    /// Findings in the commits of `local` that `remote` does not have yet:
    /// those after `remote/to` when it exists, else those on no branch of
    /// the remote.
    pub fn scan_unpushed(
        &self,
        repo: &Repo,
        local: &str,
        remote: &str,
        to: &str,
    ) -> Result<Vec<Finding>> {
        let raw = repo.raw();
        let tip = raw
            .revparse_single(&format!("refs/heads/{local}"))?
            .peel_to_commit()?;
        let mut walk = raw.revwalk()?;
        walk.push(tip.id())?;
        match raw.find_reference(&format!("refs/remotes/{remote}/{to}")) {
            Ok(r) => {
                if let Some(t) = r.target() {
                    walk.hide(t)?;
                }
            }
            Err(_) => {
                let _ = walk.hide_glob(&format!("refs/remotes/{remote}/*"));
            }
        }
        let mut out = Vec::new();
        for id in walk.take(MAX_COMMITS) {
            let commit = raw.find_commit(id?)?;
            let tree = commit.tree()?;
            let parent = commit.parent(0).ok().and_then(|p| p.tree().ok());
            let mut opts = DiffOptions::new();
            opts.context_lines(0);
            let diff = raw.diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut opts))?;
            self.scan_diff(&diff, Some(commit.id()), &mut out);
            if out.len() >= MAX_FINDINGS {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
