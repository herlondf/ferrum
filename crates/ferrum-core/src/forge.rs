//! Pull requests on GitHub and merge requests on GitLab.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ForgeKind {
    GitHub,
    GitLab,
}

impl ForgeKind {
    pub fn name(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "GitHub",
            ForgeKind::GitLab => "GitLab",
        }
    }

    /// "pull request" or "merge request".
    pub fn request_name(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "pull request",
            ForgeKind::GitLab => "merge request",
        }
    }
}

/// A repository hosted on a forge, parsed from a remote URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedRepo {
    pub kind: ForgeKind,
    pub host: String,
    /// `owner/repo`, or `group/subgroup/repo` on GitLab.
    pub path: String,
}

impl HostedRepo {
    fn api_base(&self) -> String {
        match (self.kind, self.host.as_str()) {
            (ForgeKind::GitHub, "github.com") => "https://api.github.com".into(),
            (ForgeKind::GitHub, host) => format!("https://{host}/api/v3"),
            (ForgeKind::GitLab, host) => format!("https://{host}/api/v4"),
        }
    }

    pub fn web_url(&self) -> String {
        format!("https://{}/{}", self.host, self.path)
    }

    /// Browser page to open a new pull request from `branch`.
    pub fn new_request_url(&self, branch: &str, target: &str) -> String {
        match self.kind {
            ForgeKind::GitHub => format!("{}/compare/{target}...{branch}?expand=1", self.web_url()),
            ForgeKind::GitLab => format!(
                "{}/-/merge_requests/new?merge_request[source_branch]={branch}&merge_request[target_branch]={target}",
                self.web_url()
            ),
        }
    }
}

/// Recognizes GitHub and GitLab remotes in https, ssh and scp-like form.
pub fn parse_remote_url(url: &str) -> Option<HostedRepo> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
        // https://host/owner/repo, ssh://git@host:22/owner/repo
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next()?, path)
    } else {
        // git@host:owner/repo
        let rest = url.rsplit_once('@').map_or(url, |(_, r)| r);
        rest.split_once(':')?
    };
    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    if path.split('/').count() < 2 {
        return None;
    }
    let lower = host.to_ascii_lowercase();
    let kind = if lower.contains("github") {
        ForgeKind::GitHub
    } else if lower.contains("gitlab") {
        ForgeKind::GitLab
    } else {
        return None;
    };
    Some(HostedRepo {
        kind,
        host: lower,
        path: path.to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub source_branch: String,
    pub target_branch: String,
    pub url: String,
    pub draft: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct NewPullRequest {
    pub title: String,
    pub body: String,
    pub source_branch: String,
    pub target_branch: String,
    pub draft: bool,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent("ferrum-git-client")
        .build()
        .into()
}

fn auth_header(kind: ForgeKind, token: &str) -> (&'static str, String) {
    match kind {
        ForgeKind::GitHub => ("Authorization", format!("Bearer {token}")),
        ForgeKind::GitLab => ("PRIVATE-TOKEN", token.to_owned()),
    }
}

fn check(mut resp: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let status = resp.status().as_u16();
    let body: Value = resp.body_mut().read_json().unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(body);
    }
    let detail = error_detail(&body);
    let hint = match status {
        401 => " (the token is invalid or expired)",
        403 => " (the token lacks permission)",
        404 => " (repository not found, or the token cannot see it)",
        _ => "",
    };
    Err(Error::Other(format!("HTTP {status}{hint}: {detail}")))
}

/// The readable part of an API error: GitHub's `message` plus its `errors`
/// list (e.g. "A pull request already exists"), GitLab's `message` (a string,
/// list or field map) or `error`.
fn error_detail(body: &Value) -> String {
    fn text(v: &Value) -> Vec<String> {
        match v {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a.iter().flat_map(text).collect(),
            Value::Object(o) => match o.get("message") {
                Some(m) => text(m),
                // GitLab validation errors: {"field": ["is invalid"]}.
                None => o
                    .iter()
                    .flat_map(|(k, v)| text(v).into_iter().map(move |t| format!("{k} {t}")))
                    .collect(),
            },
            Value::Null => Vec::new(),
            other => vec![other.to_string()],
        }
    }
    let mut parts = text(body.get("message").unwrap_or(&Value::Null));
    if let Some(errors) = body.get("errors") {
        parts.extend(text(errors));
    }
    if parts.is_empty() {
        parts = text(body.get("error").unwrap_or(&Value::Null));
    }
    parts.join(": ")
}

fn net(e: ureq::Error) -> Error {
    Error::Other(format!("network error: {e}"))
}

fn gitlab_project(repo: &HostedRepo) -> String {
    repo.path.replace('/', "%2F")
}

/// Checks a token and returns the account name.
pub fn verify_token(kind: ForgeKind, host: &str, token: &str) -> Result<String> {
    let repo = HostedRepo {
        kind,
        host: host.to_owned(),
        path: String::new(),
    };
    let (k, v) = auth_header(kind, token);
    let resp = agent()
        .get(&format!("{}/user", repo.api_base()))
        .header(k, &v)
        .call()
        .map_err(net)?;
    let body = check(resp)?;
    let field = if kind == ForgeKind::GitHub {
        "login"
    } else {
        "username"
    };
    Ok(body
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned())
}

pub fn list_pull_requests(repo: &HostedRepo, token: &str) -> Result<Vec<PullRequest>> {
    let (k, v) = auth_header(repo.kind, token);
    let url = match repo.kind {
        ForgeKind::GitHub => format!(
            "{}/repos/{}/pulls?state=open&per_page=100",
            repo.api_base(),
            repo.path
        ),
        ForgeKind::GitLab => format!(
            "{}/projects/{}/merge_requests?state=opened&per_page=100",
            repo.api_base(),
            gitlab_project(repo)
        ),
    };
    let body = check(agent().get(&url).header(k, &v).call().map_err(net)?)?;
    parse_pull_requests(repo.kind, &body)
}

pub fn create_pull_request(
    repo: &HostedRepo,
    token: &str,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let (k, v) = auth_header(repo.kind, token);
    let (url, payload) = match repo.kind {
        ForgeKind::GitHub => (
            format!("{}/repos/{}/pulls", repo.api_base(), repo.path),
            json!({
                "title": pr.title, "body": pr.body, "head": pr.source_branch,
                "base": pr.target_branch, "draft": pr.draft,
            }),
        ),
        ForgeKind::GitLab => (
            format!(
                "{}/projects/{}/merge_requests",
                repo.api_base(),
                gitlab_project(repo)
            ),
            json!({
                "title": if pr.draft { format!("Draft: {}", pr.title) } else { pr.title.clone() },
                "description": pr.body, "source_branch": pr.source_branch,
                "target_branch": pr.target_branch,
            }),
        ),
    };
    let body = check(
        agent()
            .post(&url)
            .header(k, &v)
            .send_json(&payload)
            .map_err(net)?,
    )?;
    parse_one(repo.kind, &body)
        .ok_or_else(|| Error::Other("unexpected response from the server".into()))
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}
#[derive(Deserialize)]
struct GhRef {
    #[serde(rename = "ref")]
    name: String,
}
#[derive(Deserialize)]
struct GhPull {
    number: u64,
    title: String,
    user: GhUser,
    head: GhRef,
    base: GhRef,
    html_url: String,
    #[serde(default)]
    draft: bool,
    updated_at: String,
}
#[derive(Deserialize)]
struct GlUser {
    username: String,
}
#[derive(Deserialize)]
struct GlMerge {
    iid: u64,
    title: String,
    author: GlUser,
    source_branch: String,
    target_branch: String,
    web_url: String,
    #[serde(default)]
    draft: bool,
    updated_at: String,
}

fn parse_one(kind: ForgeKind, v: &Value) -> Option<PullRequest> {
    match kind {
        ForgeKind::GitHub => {
            let p: GhPull = serde_json::from_value(v.clone()).ok()?;
            Some(PullRequest {
                number: p.number,
                title: p.title,
                author: p.user.login,
                source_branch: p.head.name,
                target_branch: p.base.name,
                url: p.html_url,
                draft: p.draft,
                updated_at: p.updated_at,
            })
        }
        ForgeKind::GitLab => {
            let m: GlMerge = serde_json::from_value(v.clone()).ok()?;
            Some(PullRequest {
                number: m.iid,
                title: m.title,
                author: m.author.username,
                source_branch: m.source_branch,
                target_branch: m.target_branch,
                url: m.web_url,
                draft: m.draft,
                updated_at: m.updated_at,
            })
        }
    }
}

fn parse_pull_requests(kind: ForgeKind, body: &Value) -> Result<Vec<PullRequest>> {
    let list = body
        .as_array()
        .ok_or_else(|| Error::Other("unexpected response from the server".into()))?;
    Ok(list.iter().filter_map(|v| parse_one(kind, v)).collect())
}

/// Token from the GitHub CLI (`gh auth token`), when it is installed and logged in.
pub fn gh_cli_token(host: &str) -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token", "--hostname", host])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let token = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_urls() {
        let gh = |path: &str| HostedRepo {
            kind: ForgeKind::GitHub,
            host: "github.com".into(),
            path: path.into(),
        };
        assert_eq!(
            parse_remote_url("https://github.com/herlondf/ferrum.git"),
            Some(gh("herlondf/ferrum"))
        );
        assert_eq!(
            parse_remote_url("git@github.com:herlondf/ferrum.git"),
            Some(gh("herlondf/ferrum"))
        );
        assert_eq!(
            parse_remote_url("ssh://git@github.com/herlondf/ferrum"),
            Some(gh("herlondf/ferrum"))
        );
        assert_eq!(
            parse_remote_url("https://user:tok@github.com/a/b/"),
            Some(gh("a/b"))
        );
        let gl = parse_remote_url("git@gitlab.example.com:group/sub/repo.git").unwrap();
        assert_eq!(
            (gl.kind, gl.host.as_str(), gl.path.as_str()),
            (ForgeKind::GitLab, "gitlab.example.com", "group/sub/repo")
        );
        assert_eq!(gl.api_base(), "https://gitlab.example.com/api/v4");
        assert!(parse_remote_url("https://bitbucket.org/a/b").is_none());
        assert!(parse_remote_url("/local/path/repo").is_none());
    }

    #[test]
    fn explains_api_errors() {
        let gh = json!({
            "message": "Validation Failed",
            "errors": [{"resource": "PullRequest", "code": "custom",
                        "message": "A pull request already exists for a:feature."}]
        });
        assert_eq!(
            error_detail(&gh),
            "Validation Failed: A pull request already exists for a:feature."
        );
        let gl = json!({"message": ["Another open merge request already exists for this source branch: !3"]});
        assert_eq!(
            error_detail(&gl),
            "Another open merge request already exists for this source branch: !3"
        );
        let fields = json!({"message": {"target_branch": ["is invalid"]}});
        assert_eq!(error_detail(&fields), "target_branch is invalid");
        assert_eq!(
            error_detail(&json!({"error": "invalid_token"})),
            "invalid_token"
        );
        assert_eq!(error_detail(&Value::Null), "");
    }

    #[test]
    fn parses_api_responses() {
        let gh = json!([{
            "number": 7, "title": "Add graph", "user": {"login": "ana"},
            "head": {"ref": "feature"}, "base": {"ref": "main"},
            "html_url": "https://github.com/a/b/pull/7", "draft": true,
            "updated_at": "2026-11-01T10:00:00Z"
        }]);
        let prs = parse_pull_requests(ForgeKind::GitHub, &gh).unwrap();
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].source_branch, "feature");
        assert!(prs[0].draft);

        let gl = json!([{
            "iid": 3, "title": "Fix", "author": {"username": "bo"},
            "source_branch": "fix", "target_branch": "main",
            "web_url": "https://gitlab.com/a/b/-/merge_requests/3",
            "updated_at": "2026-11-01T10:00:00Z"
        }]);
        let mrs = parse_pull_requests(ForgeKind::GitLab, &gl).unwrap();
        assert_eq!(
            (mrs[0].number, mrs[0].author.as_str(), mrs[0].draft),
            (3, "bo", false)
        );
    }
}
