use super::*;
use crate::testutil::TestRepo;

// Sample secrets are joined at run time so this file holds none itself.
fn join(parts: &[&str]) -> String {
    parts.concat()
}

fn scanner() -> Scanner {
    Scanner::new(&Config::default(), Allowlist::default()).0
}

fn rules_in(line: &str) -> Vec<String> {
    scanner()
        .scan_line("src/x.rs", 1, line)
        .into_iter()
        .map(|f| f.rule)
        .collect()
}

#[test]
fn provider_tokens_are_found() {
    let cases = [
        (
            join(&["key = \"AKIA", "IOSFODNN7EXAMPLE\""]),
            "aws-access-key",
        ),
        (
            join(&[
                "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/",
                "bPxRfiCYzQ1w2E3r4T",
            ]),
            "aws-secret-key",
        ),
        (
            join(&["ghp_", "1234567890abcdefghijABCDEFGHIJ123456"]),
            "github-token",
        ),
        (
            join(&[
                "github_pat_",
                "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123",
            ]),
            "github-pat",
        ),
        (join(&["glpat-", "xY12abCD34efGH56ijKL"]), "gitlab-token"),
        (join(&["xoxb-", "123456789012-abcdefghijkl"]), "slack-token"),
        (
            join(&[
                "https://hooks.slack.com/services/",
                "T00000000/B00000000/XXXXXXXXXXXXXXXXXXXXXXXX",
            ]),
            "slack-webhook",
        ),
        (
            join(&["sk_live_", "4eC39HqLyjWDarjtT1zdp7dc"]),
            "stripe-key",
        ),
        (
            join(&["AIza", "SyD-1234567890abcdefghijklmnopqrstu"]),
            "google-api-key",
        ),
        (
            join(&[
                "sk-proj-",
                "abcdEFGH1234ijklMNOP5678qrstUVWX9012yzabCDEF3456",
            ]),
            "openai-key",
        ),
        (
            join(&["sk-ant-api03-", &"aB3dE6gH9jK2mN5pQ8sT1vW4yZ7".repeat(4)]),
            "anthropic-key",
        ),
        (
            join(&["-----BEGIN RSA ", "PRIVATE KEY-----"]),
            "private-key",
        ),
        (
            join(&["-----BEGIN OPENSSH ", "PRIVATE KEY-----"]),
            "private-key",
        ),
        (
            join(&[
                "token: eyJhbGciOiJIUzI1NiJ9",
                ".eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            ]),
            "jwt",
        ),
        (
            join(&[
                "//registry.npmjs.org/:_authToken=npm_",
                "abcdefghijklmnopqrstuvwxyz0123456789",
            ]),
            "npm-token",
        ),
        (
            join(&["postgres://admin:", "Xk9mQ2vLp7@db.internal:5432/app"]),
            "url-password",
        ),
        (
            join(&["DB_PASSWORD = \"Tr0ub4dor&", "3x\""]),
            "secret-assignment",
        ),
    ];
    for (line, rule) in cases {
        let found = rules_in(&line);
        assert!(
            found.iter().any(|r| r == rule),
            "{rule} not found in {line}: {found:?}"
        );
    }
}

#[test]
fn random_strings_are_found_but_hashes_and_names_are_not() {
    let random = join(&["let k = \"Zx8Qm2Lp9Vt4Rb7Nc1Hs", "6Kd3Wf5Jg0Ye8Ua2Io\";"]);
    assert_eq!(rules_in(&random), vec!["entropy".to_owned()]);
    for line in [
        // A commit hash, a checksum, identifiers, a path, a base64 image.
        "let id = \"9fceb02d0ae598e95dc970b74767f19372d61af8\";",
        "checksum = \"2d0ae598e95dc970b74767f19372d61af89fceb02d0ae598e95dc970b74767f1\"",
        "use crate::ui::sidebar_model::RepoPrefsWithLongNamesAndMore;",
        "const P: &str = \"/usr/share/icons/hicolor/256x256/apps/ferrum.png\";",
        "src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk\"",
        "let token = std::env::var(\"GITHUB_TOKEN\")?;",
        "password: \"changeme123\",",
        "api_key = \"${API_KEY}\"",
        "secret = \"<your secret here>\"",
        "https://user:password@example.com/repo.git",
        "let url = format!(\"https://{user}:{token}@{host}\");",
        "AKIAEXAMPLE",
    ] {
        assert!(
            rules_in(line).is_empty(),
            "false positive: {line}: {:?}",
            rules_in(line)
        );
    }
}

#[test]
fn allow_comments_globs_and_fingerprints() {
    let line = join(&["k = \"ghp_", "1234567890abcdefghijABCDEFGHIJ123456\""]);
    assert!(rules_in(&format!("{line} // ferrum:allow")).is_empty());

    let found = scanner().scan_line("a.rs", 1, &line);
    let mut allow = Allowlist::default();
    allow.add(&found[0]);
    let s = Scanner::new(&Config::default(), allow).0;
    assert!(s.scan_line("a.rs", 1, &line).is_empty());

    let allow = Allowlist {
        paths: vec!["tests/fixtures/**".into(), "*.sample".into()],
        ..Default::default()
    };
    let s = Scanner::new(&Config::default(), allow).0;
    assert!(s.scan_text("tests/fixtures/a/b.txt", &line).is_empty());
    assert!(s.scan_text("conf/keys.sample", &line).is_empty());
    assert!(!s.scan_text("src/main.rs", &line).is_empty());
    // The secret itself is masked in the report.
    let f = &scanner().scan_line("a.rs", 1, &line)[0];
    assert!(!f.text.contains("1234567890abcdefghij"));
    assert!(f.text.contains("ghp_"));
}

#[test]
fn custom_patterns_and_disabled_rules() {
    let config = Config {
        disabled: vec!["aws-access-key".into()],
        custom: vec![
            CustomPattern {
                name: "Internal token".into(),
                regex: r"ACME-[0-9]{6}".into(),
            },
            CustomPattern {
                name: "Broken".into(),
                regex: "([".into(),
            },
        ],
        ..Default::default()
    };
    let (s, errors) = Scanner::new(&config, Allowlist::default());
    assert_eq!(errors.len(), 1);
    let f = s.scan_line("a", 3, "id = ACME-123456");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].name, "Internal token");
    assert_eq!(f[0].line, 3);
    assert!(
        s.scan_line("a", 1, &join(&["AKIA", "IOSFODNN7EXAMPLE"]))
            .is_empty()
    );
}

#[test]
fn sensitive_files() {
    for p in [
        ".env",
        "app/.env.production",
        "deploy/server.pem",
        "home/.ssh/id_ed25519",
    ] {
        assert!(sensitive_file(p).is_some(), "{p}");
    }
    for p in [
        ".env.example",
        ".env.sample",
        "src/env.rs",
        "README.md",
        "keys.rs",
    ] {
        assert!(sensitive_file(p).is_none(), "{p}");
    }
}

#[test]
fn globs() {
    assert!(glob_match("*.lock", "a/b/Cargo.lock"));
    assert!(glob_match("docs/*.md", "docs/a.md"));
    assert!(!glob_match("docs/*.md", "docs/x/a.md"));
    assert!(glob_match("docs/**/*.md", "docs/x/y/a.md"));
    assert!(glob_match("docs/**", "docs/x/y/a.md"));
    assert!(!glob_match("", "a"));
}

#[test]
fn staged_changes_and_unpushed_commits_are_scanned() {
    let r = TestRepo::new();
    r.commit("a.txt", "hello\n", "first");
    let key = join(&["AKIA", "IOSFODNN7EXAMPLE"]);
    r.write("conf.txt", &format!("one\nkey = {key}\n"));
    r.write(".env", "PORT=3000\n");
    r.git.run(&["add", "-A"]).unwrap();
    let found = scanner().scan_staged(&r.repo()).unwrap();
    let mut rules: Vec<(&str, u32)> = found.iter().map(|f| (f.rule.as_str(), f.line)).collect();
    rules.sort();
    assert_eq!(rules, vec![("aws-access-key", 2), ("sensitive-file", 0)]);

    // Unstaged content is not part of the commit.
    r.git.run(&["reset", "-q"]).unwrap();
    assert!(scanner().scan_staged(&r.repo()).unwrap().is_empty());

    // Before a push: commits the remote does not have.
    let remote = TestRepo::new();
    remote
        .git
        .run(&["config", "receive.denyCurrentBranch", "ignore"])
        .unwrap();
    r.git
        .run(&["remote", "add", "origin", &remote.path().to_string_lossy()])
        .unwrap();
    r.git.run(&["push", "-q", "origin", "main"]).unwrap();
    r.git.run(&["fetch", "-q", "origin"]).unwrap();
    assert!(
        scanner()
            .scan_unpushed(&r.repo(), "main", "origin", "main")
            .unwrap()
            .is_empty()
    );
    r.git.run(&["add", "conf.txt"]).unwrap();
    r.git.run(&["commit", "-q", "-m", "conf"]).unwrap();
    let found = scanner()
        .scan_unpushed(&r.repo(), "main", "origin", "main")
        .unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].commit.is_some());
    // A new branch: compared with every branch of the remote.
    let found = scanner()
        .scan_unpushed(&r.repo(), "main", "origin", "other")
        .unwrap();
    assert_eq!(found.len(), 1);
}

/// No findings in Ferrum's own sources, docs and scripts.
#[test]
fn no_false_positives_in_ferrum_itself() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let s = scanner();
    let mut findings = Vec::new();
    let mut files = 0;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !matches!(
                    name.as_str(),
                    "target" | ".git" | "node_modules" | ".claude"
                ) {
                    stack.push(p);
                }
                continue;
            }
            let rel = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if rel.ends_with("secrets/tests.rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            files += 1;
            findings.extend(s.scan_text(&rel, &text));
        }
    }
    assert!(files > 100, "scanned {files} files");
    let report: Vec<String> = findings
        .iter()
        .map(|f| format!("{}:{} {} {}", f.path, f.line, f.rule, f.text))
        .collect();
    assert!(report.is_empty(), "false positives:\n{}", report.join("\n"));
}
