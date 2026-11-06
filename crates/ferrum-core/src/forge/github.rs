//! GitHub and GitHub Enterprise Server (REST API v3).

use serde_json::{Value, json};

use super::http::{self, Auth, str_at, unexpected};
use super::*;

pub fn auth(c: &Credentials) -> Auth {
    Auth::Bearer(c.token.clone())
}

pub fn verify(base: &str, c: &Credentials) -> Result<String> {
    let body = http::get(&format!("{base}/user"), &auth(c))?;
    Ok(str_at(&body, "/login"))
}

pub fn scopes(base: &str, c: &Credentials) -> Result<Option<Vec<String>>> {
    let reply = http::get_reply(&format!("{base}/user"), &auth(c))?;
    Ok(reply.scopes.map(|s| {
        s.split(',')
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty())
            .collect()
    }))
}

pub fn parse_pull(v: &Value) -> Option<PullRequest> {
    Some(PullRequest {
        number: v.get("number")?.as_u64()?,
        title: str_at(v, "/title"),
        author: str_at(v, "/user/login"),
        source_branch: str_at(v, "/head/ref"),
        target_branch: str_at(v, "/base/ref"),
        url: str_at(v, "/html_url"),
        draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
        updated_at: str_at(v, "/updated_at"),
    })
}

pub fn pulls(base: &str, repo: &HostedRepo, c: &Credentials) -> Result<Vec<PullRequest>> {
    let url = format!("{base}/repos/{}/pulls?state=open&per_page=100", repo.path);
    let body = http::get(&url, &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list.iter().filter_map(parse_pull).collect())
}

pub fn create_pull(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let payload = json!({
        "title": pr.title, "body": pr.body, "head": pr.source_branch,
        "base": pr.target_branch, "draft": pr.draft,
    });
    let body = http::post(
        &format!("{base}/repos/{}/pulls", repo.path),
        &auth(c),
        &payload,
    )?;
    parse_pull(&body).ok_or_else(unexpected)
}

fn parse_issue(v: &Value) -> Option<Issue> {
    // The issues API lists pull requests too.
    if v.get("pull_request").is_some() {
        return None;
    }
    let names = |key: &str, field: &str| -> Vec<String> {
        v.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| str_at(x, field)).collect())
            .unwrap_or_default()
    };
    Some(Issue {
        number: v.get("number")?.as_u64()?,
        title: str_at(v, "/title"),
        author: str_at(v, "/user/login"),
        assignees: names("assignees", "/login"),
        labels: names("labels", "/name"),
        open: str_at(v, "/state") == "open",
        url: str_at(v, "/html_url"),
        updated_at: str_at(v, "/updated_at"),
        comments: v.get("comments").and_then(Value::as_u64).unwrap_or(0),
    })
}

pub fn issues(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    f: &IssueFilter,
) -> Result<Vec<Issue>> {
    let state = match f.state {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
        IssueState::All => "all",
    };
    let mut url = format!(
        "{base}/repos/{}/issues?state={state}&per_page=100&sort=updated",
        repo.path
    );
    for (key, value) in [
        ("creator", &f.author),
        ("assignee", &f.assignee),
        ("labels", &f.label),
    ] {
        if !value.trim().is_empty() {
            url.push_str(&format!("&{key}={}", http::encode(value.trim())));
        }
    }
    let body = http::get(&url, &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list.iter().filter_map(parse_issue).collect())
}

pub fn create_issue(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    issue: &NewIssue,
) -> Result<Issue> {
    let payload = json!({"title": issue.title, "body": issue.body});
    let body = http::post(
        &format!("{base}/repos/{}/issues", repo.path),
        &auth(c),
        &payload,
    )?;
    parse_issue(&body).ok_or_else(unexpected)
}

pub fn repos(base: &str, c: &Credentials) -> Result<Vec<RemoteRepo>> {
    let url = format!("{base}/user/repos?per_page=100&sort=updated");
    let body = http::get(&url, &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list
        .iter()
        .map(|r| RemoteRepo {
            name: str_at(r, "/full_name"),
            description: str_at(r, "/description"),
            https_url: str_at(r, "/clone_url"),
            ssh_url: str_at(r, "/ssh_url"),
            private: r.get("private").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect())
}

/// The page of an API URL like `https://api.github.com/repos/o/r/pulls/3`.
fn html_url(api: &str, host: &str) -> String {
    let web = web_base(host);
    let path = api
        .split_once("/repos/")
        .map(|(_, p)| p)
        .unwrap_or_default();
    if path.is_empty() {
        return format!("{web}/notifications");
    }
    let path = path.replacen("/pulls/", "/pull/", 1);
    let path = path.replacen("/commits/", "/commit/", 1);
    format!("{web}/{path}")
}

pub fn notifications(base: &str, host: &str, c: &Credentials) -> Result<Vec<Notification>> {
    let body = http::get(&format!("{base}/notifications?per_page=50"), &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list
        .iter()
        .map(|n| {
            let repo = str_at(n, "/repository/full_name");
            let api = str_at(n, "/subject/url");
            let url = if api.is_empty() {
                format!("{}/{repo}", web_base(host))
            } else {
                html_url(&api, host)
            };
            Notification {
                id: str_at(n, "/id"),
                title: str_at(n, "/subject/title"),
                kind: str_at(n, "/subject/type"),
                repo,
                url,
                reason: str_at(n, "/reason"),
                updated_at: str_at(n, "/updated_at"),
            }
        })
        .collect())
}

pub fn mark_read(base: &str, c: &Credentials) -> Result<()> {
    http::put_empty(&format!("{base}/notifications"), &auth(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_pages() {
        assert_eq!(
            html_url("https://api.github.com/repos/a/b/pulls/3", "github.com"),
            "https://github.com/a/b/pull/3"
        );
        assert_eq!(
            html_url("https://ghe.corp/api/v3/repos/a/b/issues/9", "ghe.corp"),
            "https://ghe.corp/a/b/issues/9"
        );
    }
}
