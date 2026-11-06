//! Gitea and Forgejo (API v1, close to GitHub's).

use serde_json::{Value, json};

use super::http::{self, Auth, str_at, unexpected};
use super::*;

pub fn auth(c: &Credentials) -> Auth {
    Auth::Token(c.token.clone())
}

pub fn verify(base: &str, c: &Credentials) -> Result<String> {
    let body = http::get(&format!("{base}/user"), &auth(c))?;
    Ok(str_at(&body, "/login"))
}

pub fn pulls(base: &str, repo: &HostedRepo, c: &Credentials) -> Result<Vec<PullRequest>> {
    let url = format!("{base}/repos/{}/pulls?state=open&limit=50", repo.path);
    let body = http::get(&url, &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list.iter().filter_map(github::parse_pull).collect())
}

pub fn create_pull(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    // Gitea marks drafts with a title prefix.
    let title = if pr.draft {
        format!("WIP: {}", pr.title)
    } else {
        pr.title.clone()
    };
    let payload = json!({
        "title": title, "body": pr.body, "head": pr.source_branch, "base": pr.target_branch,
    });
    let body = http::post(
        &format!("{base}/repos/{}/pulls", repo.path),
        &auth(c),
        &payload,
    )?;
    github::parse_pull(&body).ok_or_else(unexpected)
}

fn parse_issue(v: &Value) -> Option<Issue> {
    if !v.get("pull_request").is_none_or(Value::is_null) {
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
        "{base}/repos/{}/issues?type=issues&state={state}&limit=50",
        repo.path
    );
    for (key, value) in [
        ("created_by", &f.author),
        ("assigned_by", &f.assignee),
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
    let body = http::get(&format!("{base}/user/repos?limit=50"), &auth(c))?;
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
