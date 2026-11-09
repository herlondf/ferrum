//! Pull requests for a stack of branches: each one targets the branch below
//! it, and each description lists the whole stack with links to the others.
//!
//! Requests are created bottom first, so each description can link the ones
//! below; on GitHub and GitLab the new descriptions are updated afterwards
//! to link the ones above as well. A branch that already has an open request
//! keeps it (its description is left alone).

use serde_json::json;

use super::http;
use super::*;

/// One branch of the stack, bottom first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChainEntry {
    pub branch: String,
    /// The branch below (the trunk for the first one).
    pub target: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// The request of one branch.
#[derive(Debug, Clone)]
pub struct ChainResult {
    pub branch: String,
    pub pr: PullRequest,
    /// Opened now (not one that already existed).
    pub created: bool,
}

/// Marks the stack list in descriptions.
pub const MARKER: &str = "<!-- ferrum-stack -->";

/// How a request is referred to in a description on this service.
fn reference(kind: ForgeKind, pr: &PullRequest) -> String {
    match kind {
        ForgeKind::GitHub | ForgeKind::Gitea => format!("#{}", pr.number),
        ForgeKind::GitLab => format!("!{}", pr.number),
        _ => format!("[#{}]({})", pr.number, pr.url),
    }
}

/// The stack list for the description of `entries[me]`. `known` holds the
/// requests found so far (by position).
pub fn stack_section(
    kind: ForgeKind,
    entries: &[ChainEntry],
    known: &[Option<PullRequest>],
    me: usize,
) -> String {
    let noun = kind.request_name();
    let mut out = format!(
        "{MARKER}\n---\n**Stack** ({} {noun}s, top first):\n\n",
        entries.len()
    );
    for (i, e) in entries.iter().enumerate().rev() {
        let what = match known.get(i).and_then(Option::as_ref) {
            Some(pr) => format!("{} `{}`", reference(kind, pr), e.branch),
            None => format!("`{}`", e.branch),
        };
        let here = if i == me { " ← this one" } else { "" };
        out.push_str(&format!("{}. {what}{here}\n", entries.len() - i));
    }
    let base = entries.first().map(|e| e.target.as_str()).unwrap_or("");
    out.push_str(&format!("\nBased on `{base}`.\n"));
    out
}

fn with_section(body: &str, section: &str) -> String {
    let own = match body.find(MARKER) {
        Some(i) => body[..i].trim_end(),
        None => body.trim_end(),
    };
    if own.is_empty() {
        section.to_owned()
    } else {
        format!("{own}\n\n{section}")
    }
}

/// Whether descriptions can be updated after creation here.
pub fn can_update(kind: ForgeKind) -> bool {
    matches!(kind, ForgeKind::GitHub | ForgeKind::GitLab)
}

/// Replaces the description of request `number`.
pub fn update_body(repo: &HostedRepo, creds: &Credentials, number: u64, body: &str) -> Result<()> {
    let base = repo.api_base();
    match repo.kind {
        ForgeKind::GitHub => {
            let url = format!("{base}/repos/{}/pulls/{number}", repo.path);
            http::update(&url, &github::auth(creds), &json!({ "body": body }), false)?;
        }
        ForgeKind::GitLab => {
            let url = format!(
                "{base}/projects/{}/merge_requests/{number}",
                http::encode(&repo.path)
            );
            http::update(
                &url,
                &gitlab::auth(creds),
                &json!({ "description": body }),
                true,
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// Opens the requests of the stack (reusing open ones), bottom first.
pub fn create_chain(
    repo: &HostedRepo,
    creds: &Credentials,
    entries: &[ChainEntry],
) -> Result<Vec<ChainResult>> {
    let open = list_pull_requests(repo, creds)?;
    let mut known: Vec<Option<PullRequest>> = vec![None; entries.len()];
    let mut created = vec![false; entries.len()];
    for (i, e) in entries.iter().enumerate() {
        if let Some(pr) = open.iter().find(|p| p.source_branch == e.branch) {
            known[i] = Some(pr.clone());
        }
    }
    for (i, e) in entries.iter().enumerate() {
        if known[i].is_some() {
            continue;
        }
        let body = with_section(&e.body, &stack_section(repo.kind, entries, &known, i));
        let pr = create_pull_request(
            repo,
            creds,
            &NewPullRequest {
                title: e.title.clone(),
                body,
                source_branch: e.branch.clone(),
                target_branch: e.target.clone(),
                draft: e.draft,
            },
        )
        .map_err(|err| crate::Error::Other(format!("{}: {err}", e.branch)))?;
        known[i] = Some(pr);
        created[i] = true;
    }
    if can_update(repo.kind) {
        for (i, e) in entries.iter().enumerate() {
            let Some(pr) = known[i].as_ref().filter(|_| created[i]) else {
                continue;
            };
            let body = with_section(&e.body, &stack_section(repo.kind, entries, &known, i));
            update_body(repo, creds, pr.number, &body)?;
        }
    }
    Ok(entries
        .iter()
        .zip(known)
        .zip(created)
        .filter_map(|((e, pr), created)| {
            Some(ChainResult {
                branch: e.branch.clone(),
                pr: pr?,
                created,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::Value;

    use super::super::mock;
    use super::*;

    fn entries() -> Vec<ChainEntry> {
        [("a", "main"), ("b", "a"), ("c", "b")]
            .into_iter()
            .map(|(b, t)| ChainEntry {
                branch: b.into(),
                target: t.into(),
                title: format!("Title {b}"),
                body: format!("About {b}"),
                draft: false,
            })
            .collect()
    }

    fn pr(n: u64, head: &str, base: &str) -> Value {
        json!({"number": n, "title": "T", "user": {"login": "me"},
            "head": {"ref": head}, "base": {"ref": base}, "html_url": format!("u/{n}"),
            "updated_at": "t"})
    }

    #[test]
    fn github_chain_links_every_request() {
        let next = AtomicU64::new(6);
        let m = mock::serve(move |method, path| match (method, path) {
            ("GET", p) if p.starts_with("/api/v3/repos/o/r/pulls") => (
                200,
                Vec::new(),
                json!([pr(5, "a", "main"), pr(1, "other", "main")]).to_string(),
            ),
            ("POST", "/api/v3/repos/o/r/pulls") => {
                let n = next.fetch_add(1, Ordering::SeqCst);
                (201, Vec::new(), pr(n, "x", "y").to_string())
            }
            ("PATCH", p) if p.starts_with("/api/v3/repos/o/r/pulls/") => {
                (200, Vec::new(), pr(0, "x", "y").to_string())
            }
            _ => (500, Vec::new(), "{}".into()),
        });
        let repo = HostedRepo {
            kind: ForgeKind::GitHub,
            host: m.base.clone(),
            path: "o/r".into(),
        };
        let out = create_chain(&repo, &Credentials::token("tok"), &entries()).unwrap();
        let numbers: Vec<(u64, bool)> = out.iter().map(|r| (r.pr.number, r.created)).collect();
        assert_eq!(numbers, [(5, false), (6, true), (7, true)]);

        let all = m.all();
        let posts: Vec<Value> = all
            .iter()
            .filter(|r| r.method == "POST")
            .map(|r| serde_json::from_str(&r.body).unwrap())
            .collect();
        // b targets a, c targets b; b's first description links a's #5.
        assert_eq!(
            (posts[0]["head"].as_str(), posts[0]["base"].as_str()),
            (Some("b"), Some("a"))
        );
        assert_eq!(
            (posts[1]["head"].as_str(), posts[1]["base"].as_str()),
            (Some("c"), Some("b"))
        );
        let first = posts[0]["body"].as_str().unwrap();
        assert!(first.starts_with("About b\n\n"), "{first}");
        assert!(
            first.contains("#5 `a`") && first.contains("`b` ← this one"),
            "{first}"
        );

        // Afterwards every new one links all three.
        let patches: Vec<_> = all.iter().filter(|r| r.method == "PATCH").collect();
        assert_eq!(patches.len(), 2);
        assert!(patches[0].path.ends_with("/pulls/6"));
        let body: Value = serde_json::from_str(&patches[0].body).unwrap();
        let body = body["body"].as_str().unwrap();
        for want in ["#5 `a`", "#6 `b` ← this one", "#7 `c`", "Based on `main`"] {
            assert!(body.contains(want), "{want} in {body}");
        }
        // Top first.
        assert!(body.find("#7").unwrap() < body.find("#5").unwrap());
    }

    #[test]
    fn gitlab_chain_uses_put_and_bang_references() {
        let next = AtomicU64::new(1);
        let mr = |n: u64| {
            json!({"iid": n, "title": "T", "author": {"username": "me"},
                "source_branch": "x", "target_branch": "y", "web_url": "u", "updated_at": "t"})
        };
        let m = mock::serve(move |method, _| match method {
            "GET" => (200, Vec::new(), "[]".into()),
            "POST" => (
                201,
                Vec::new(),
                mr(next.fetch_add(1, Ordering::SeqCst)).to_string(),
            ),
            "PUT" => (200, Vec::new(), mr(0).to_string()),
            _ => (500, Vec::new(), "{}".into()),
        });
        let repo = HostedRepo {
            kind: ForgeKind::GitLab,
            host: m.base.clone(),
            path: "g/app".into(),
        };
        let mut list = entries();
        list[0].draft = true;
        let out = create_chain(&repo, &Credentials::token("tok"), &list).unwrap();
        assert_eq!(out.len(), 3);
        let all = m.all();
        let puts: Vec<_> = all.iter().filter(|r| r.method == "PUT").collect();
        assert_eq!(puts.len(), 3);
        assert!(puts[2].path.ends_with("/projects/g%2Fapp/merge_requests/3"));
        let body: Value = serde_json::from_str(&puts[0].body).unwrap();
        let text = body["description"].as_str().unwrap();
        assert!(
            text.contains("!1 `a` ← this one") && text.contains("!3 `c`"),
            "{text}"
        );
        let post: Value = serde_json::from_str(&all[1].body).unwrap();
        assert_eq!(post["title"], "Draft: Title a");
    }

    #[test]
    fn a_failure_names_the_branch() {
        let m = mock::serve(|method, _| match method {
            "GET" => (200, Vec::new(), "[]".into()),
            _ => (
                422,
                Vec::new(),
                json!({"message": "Validation Failed"}).to_string(),
            ),
        });
        let repo = HostedRepo {
            kind: ForgeKind::GitHub,
            host: m.base.clone(),
            path: "o/r".into(),
        };
        let err = create_chain(&repo, &Credentials::token("tok"), &entries())
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("a: HTTP 422"), "{err}");
    }

    #[test]
    fn section_replaces_an_old_one() {
        let s = stack_section(ForgeKind::Bitbucket, &entries(), &[], 1);
        assert!(s.contains("`b` ← this one"));
        let body = with_section(&format!("Mine\n\n{s}"), "NEW");
        assert_eq!(body, "Mine\n\nNEW");
        assert_eq!(with_section("", "NEW"), "NEW");
    }
}
