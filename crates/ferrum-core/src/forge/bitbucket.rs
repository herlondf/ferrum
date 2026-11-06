//! Bitbucket Cloud (API 2.0) and Bitbucket Server / Data Center (REST 1.0).

use serde_json::{Value, json};

use super::http::{self, Auth, str_at, unexpected};
use super::*;

/// App passwords and API tokens go with the username; access tokens alone.
fn auth(c: &Credentials) -> Auth {
    if c.user.trim().is_empty() {
        Auth::Bearer(c.token.clone())
    } else {
        Auth::Basic(c.user.trim().to_owned(), c.token.clone())
    }
}

fn values(body: &Value) -> Result<&Vec<Value>> {
    body.get("values")
        .and_then(Value::as_array)
        .ok_or_else(unexpected)
}

/// The clone link named `name` (`https`, `http` or `ssh`).
fn clone_link(r: &Value, names: &[&str]) -> String {
    r.pointer("/links/clone")
        .and_then(Value::as_array)
        .and_then(|l| {
            l.iter()
                .find(|x| names.contains(&str_at(x, "/name").as_str()))
        })
        .map(|x| str_at(x, "/href"))
        .unwrap_or_default()
}

fn first_of(v: &Value, pointers: &[&str]) -> String {
    pointers
        .iter()
        .map(|p| str_at(v, p))
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

// ----- Cloud -----

pub fn verify(base: &str, c: &Credentials) -> Result<String> {
    let body = http::get(&format!("{base}/user"), &auth(c))?;
    Ok(first_of(
        &body,
        &["/username", "/nickname", "/display_name"],
    ))
}

fn parse_pull(v: &Value) -> Option<PullRequest> {
    Some(PullRequest {
        number: v.get("id")?.as_u64()?,
        title: str_at(v, "/title"),
        author: first_of(v, &["/author/nickname", "/author/display_name"]),
        source_branch: str_at(v, "/source/branch/name"),
        target_branch: str_at(v, "/destination/branch/name"),
        url: str_at(v, "/links/html/href"),
        draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
        updated_at: str_at(v, "/updated_on"),
    })
}

pub fn pulls(base: &str, repo: &HostedRepo, c: &Credentials) -> Result<Vec<PullRequest>> {
    let url = format!(
        "{base}/repositories/{}/pullrequests?state=OPEN&pagelen=50",
        repo.path
    );
    let body = http::get(&url, &auth(c))?;
    Ok(values(&body)?.iter().filter_map(parse_pull).collect())
}

pub fn create_pull(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let payload = json!({
        "title": pr.title, "description": pr.body, "draft": pr.draft,
        "source": {"branch": {"name": pr.source_branch}},
        "destination": {"branch": {"name": pr.target_branch}},
    });
    let url = format!("{base}/repositories/{}/pullrequests", repo.path);
    let body = http::post(&url, &auth(c), &payload)?;
    parse_pull(&body).ok_or_else(unexpected)
}

fn parse_issue(v: &Value) -> Option<Issue> {
    let assignee = first_of(v, &["/assignee/nickname", "/assignee/display_name"]);
    Some(Issue {
        number: v.get("id")?.as_u64()?,
        title: str_at(v, "/title"),
        author: first_of(v, &["/reporter/nickname", "/reporter/display_name"]),
        assignees: if assignee.is_empty() {
            Vec::new()
        } else {
            vec![assignee]
        },
        labels: [str_at(v, "/kind"), str_at(v, "/component/name")]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
        open: matches!(str_at(v, "/state").as_str(), "new" | "open"),
        url: str_at(v, "/links/html/href"),
        updated_at: str_at(v, "/updated_on"),
        comments: 0,
    })
}

pub fn issues(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    f: &IssueFilter,
) -> Result<Vec<Issue>> {
    let query = match f.state {
        IssueState::Open => "&q=".to_owned() + &http::encode("(state=\"new\" OR state=\"open\")"),
        IssueState::Closed => {
            "&q=".to_owned() + &http::encode("state!=\"new\" AND state!=\"open\"")
        }
        IssueState::All => String::new(),
    };
    let url = format!(
        "{base}/repositories/{}/issues?pagelen=50&sort=-updated_on{query}",
        repo.path
    );
    let body = http::get(&url, &auth(c))?;
    Ok(values(&body)?.iter().filter_map(parse_issue).collect())
}

pub fn create_issue(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    issue: &NewIssue,
) -> Result<Issue> {
    let payload = json!({"title": issue.title, "content": {"raw": issue.body}});
    let url = format!("{base}/repositories/{}/issues", repo.path);
    let body = http::post(&url, &auth(c), &payload)?;
    parse_issue(&body).ok_or_else(unexpected)
}

pub fn repos(base: &str, c: &Credentials) -> Result<Vec<RemoteRepo>> {
    let url = format!("{base}/repositories?role=member&pagelen=100&sort=-updated_on");
    let body = http::get(&url, &auth(c))?;
    Ok(values(&body)?
        .iter()
        .map(|r| RemoteRepo {
            name: str_at(r, "/full_name"),
            description: str_at(r, "/description"),
            https_url: clone_link(r, &["https"]),
            ssh_url: clone_link(r, &["ssh"]),
            private: r
                .get("is_private")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect())
}

// ----- Server / Data Center -----

fn server_repo(repo: &HostedRepo) -> String {
    let (project, name) = repo.owner_and_name();
    format!("projects/{project}/repos/{name}")
}

pub fn server_verify(base: &str, c: &Credentials) -> Result<String> {
    // Needs a logged-in user; the name comes back in a header.
    let reply = http::get_reply(&format!("{base}/profile/recent/repos?limit=1"), &auth(c))?;
    Ok(reply.user.unwrap_or_else(|| c.user.clone()))
}

fn server_parse_pull(v: &Value) -> Option<PullRequest> {
    let updated = v
        .get("updatedDate")
        .and_then(Value::as_i64)
        .map(|ms| iso_time(ms / 1000))
        .unwrap_or_default();
    Some(PullRequest {
        number: v.get("id")?.as_u64()?,
        title: str_at(v, "/title"),
        author: first_of(v, &["/author/user/name", "/author/user/displayName"]),
        source_branch: str_at(v, "/fromRef/displayId"),
        target_branch: str_at(v, "/toRef/displayId"),
        url: str_at(v, "/links/self/0/href"),
        draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
        updated_at: updated,
    })
}

pub fn server_pulls(base: &str, repo: &HostedRepo, c: &Credentials) -> Result<Vec<PullRequest>> {
    let url = format!(
        "{base}/{}/pull-requests?state=OPEN&limit=100",
        server_repo(repo)
    );
    let body = http::get(&url, &auth(c))?;
    Ok(values(&body)?
        .iter()
        .filter_map(server_parse_pull)
        .collect())
}

pub fn server_create_pull(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let payload = json!({
        "title": pr.title, "description": pr.body, "draft": pr.draft,
        "fromRef": {"id": format!("refs/heads/{}", pr.source_branch)},
        "toRef": {"id": format!("refs/heads/{}", pr.target_branch)},
    });
    let url = format!("{base}/{}/pull-requests", server_repo(repo));
    let body = http::post(&url, &auth(c), &payload)?;
    server_parse_pull(&body).ok_or_else(unexpected)
}

pub fn server_repos(base: &str, c: &Credentials) -> Result<Vec<RemoteRepo>> {
    let body = http::get(&format!("{base}/repos?limit=100"), &auth(c))?;
    Ok(values(&body)?
        .iter()
        .map(|r| RemoteRepo {
            name: format!("{}/{}", str_at(r, "/project/key"), str_at(r, "/slug")),
            description: str_at(r, "/description"),
            https_url: clone_link(r, &["http", "https"]),
            ssh_url: clone_link(r, &["ssh"]),
            private: !r.get("public").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect())
}
