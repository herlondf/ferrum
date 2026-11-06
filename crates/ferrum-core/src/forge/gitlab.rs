//! GitLab.com and self-hosted GitLab (REST API v4).

use serde_json::{Value, json};

use super::http::{self, Auth, str_at, unexpected};
use super::*;

pub fn auth(c: &Credentials) -> Auth {
    Auth::Header("PRIVATE-TOKEN", c.token.clone())
}

fn project(repo: &HostedRepo) -> String {
    http::encode(&repo.path)
}

pub fn verify(base: &str, c: &Credentials) -> Result<String> {
    let body = http::get(&format!("{base}/user"), &auth(c))?;
    Ok(str_at(&body, "/username"))
}

pub fn parse_merge(v: &Value) -> Option<PullRequest> {
    Some(PullRequest {
        number: v.get("iid")?.as_u64()?,
        title: str_at(v, "/title"),
        author: str_at(v, "/author/username"),
        source_branch: str_at(v, "/source_branch"),
        target_branch: str_at(v, "/target_branch"),
        url: str_at(v, "/web_url"),
        draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
        updated_at: str_at(v, "/updated_at"),
    })
}

pub fn merges(base: &str, repo: &HostedRepo, c: &Credentials) -> Result<Vec<PullRequest>> {
    let url = format!(
        "{base}/projects/{}/merge_requests?state=opened&per_page=100",
        project(repo)
    );
    let body = http::get(&url, &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list.iter().filter_map(parse_merge).collect())
}

pub fn create_merge(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let title = if pr.draft {
        format!("Draft: {}", pr.title)
    } else {
        pr.title.clone()
    };
    let payload = json!({
        "title": title, "description": pr.body, "source_branch": pr.source_branch,
        "target_branch": pr.target_branch,
    });
    let url = format!("{base}/projects/{}/merge_requests", project(repo));
    let body = http::post(&url, &auth(c), &payload)?;
    parse_merge(&body).ok_or_else(unexpected)
}

fn parse_issue(v: &Value) -> Option<Issue> {
    Some(Issue {
        number: v.get("iid")?.as_u64()?,
        title: str_at(v, "/title"),
        author: str_at(v, "/author/username"),
        assignees: v
            .get("assignees")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| str_at(x, "/username")).collect())
            .unwrap_or_default(),
        labels: v
            .get("labels")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        open: str_at(v, "/state") == "opened",
        url: str_at(v, "/web_url"),
        updated_at: str_at(v, "/updated_at"),
        comments: v
            .get("user_notes_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

pub fn issues(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    f: &IssueFilter,
) -> Result<Vec<Issue>> {
    let state = match f.state {
        IssueState::Open => "&state=opened",
        IssueState::Closed => "&state=closed",
        IssueState::All => "",
    };
    let mut url = format!(
        "{base}/projects/{}/issues?per_page=100&order_by=updated_at{state}",
        project(repo)
    );
    for (key, value) in [
        ("author_username", &f.author),
        ("assignee_username", &f.assignee),
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
    let payload = json!({"title": issue.title, "description": issue.body});
    let url = format!("{base}/projects/{}/issues", project(repo));
    let body = http::post(&url, &auth(c), &payload)?;
    parse_issue(&body).ok_or_else(unexpected)
}

pub fn repos(base: &str, c: &Credentials) -> Result<Vec<RemoteRepo>> {
    let url = format!("{base}/projects?membership=true&per_page=100&order_by=last_activity_at");
    let body = http::get(&url, &auth(c))?;
    let list = body.as_array().ok_or_else(unexpected)?;
    Ok(list
        .iter()
        .map(|r| RemoteRepo {
            name: str_at(r, "/path_with_namespace"),
            description: str_at(r, "/description"),
            https_url: str_at(r, "/http_url_to_repo"),
            ssh_url: str_at(r, "/ssh_url_to_repo"),
            private: str_at(r, "/visibility") != "public",
        })
        .collect())
}
