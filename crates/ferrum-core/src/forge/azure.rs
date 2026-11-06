//! Azure DevOps Services (REST API 7.0). Personal access tokens go in Basic
//! authentication with an empty user name.

use serde_json::{Value, json};

use super::http::{self, Auth, str_at, unexpected};
use super::*;

const VERSION: &str = "api-version=7.0";

fn auth(c: &Credentials) -> Auth {
    Auth::Basic(String::new(), c.token.clone())
}

fn values(body: &Value) -> Result<&Vec<Value>> {
    body.get("value")
        .and_then(Value::as_array)
        .ok_or_else(unexpected)
}

fn repo_api(base: &str, repo: &HostedRepo) -> Result<String> {
    match repo.path.splitn(3, '/').collect::<Vec<_>>()[..] {
        [org, project, name] => Ok(format!(
            "{base}/{org}/{}/_apis/git/repositories/{}",
            http::encode(project),
            http::encode(name)
        )),
        _ => Err(unexpected()),
    }
}

fn organization(c: &Credentials) -> Result<&str> {
    let org = c.user.trim();
    if org.is_empty() {
        return Err(crate::Error::Other(
            "Azure DevOps needs the organization name".into(),
        ));
    }
    Ok(org)
}

pub fn verify(base: &str, c: &Credentials) -> Result<String> {
    let org = organization(c)?;
    let body = http::get(&format!("{base}/{org}/_apis/connectionData"), &auth(c))?;
    let name = str_at(&body, "/authenticatedUser/providerDisplayName");
    if name.is_empty() {
        // A sign-in page instead of JSON: the token was not accepted.
        return Err(crate::Error::Other(
            "HTTP 401 (the token is invalid or expired): anonymous access".into(),
        ));
    }
    Ok(name)
}

fn branch(r: &str) -> String {
    r.strip_prefix("refs/heads/").unwrap_or(r).to_owned()
}

fn parse_pull(v: &Value, repo: &HostedRepo) -> Option<PullRequest> {
    let number = v.get("pullRequestId")?.as_u64()?;
    let author = str_at(v, "/createdBy/uniqueName");
    Some(PullRequest {
        number,
        title: str_at(v, "/title"),
        author: if author.is_empty() {
            str_at(v, "/createdBy/displayName")
        } else {
            author
        },
        source_branch: branch(&str_at(v, "/sourceRefName")),
        target_branch: branch(&str_at(v, "/targetRefName")),
        url: repo.request_url(number),
        draft: v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        updated_at: str_at(v, "/creationDate"),
    })
}

pub fn pulls(base: &str, repo: &HostedRepo, c: &Credentials) -> Result<Vec<PullRequest>> {
    let url = format!(
        "{}/pullrequests?searchCriteria.status=active&{VERSION}",
        repo_api(base, repo)?
    );
    let body = http::get(&url, &auth(c))?;
    Ok(values(&body)?
        .iter()
        .filter_map(|v| parse_pull(v, repo))
        .collect())
}

pub fn create_pull(
    base: &str,
    repo: &HostedRepo,
    c: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let payload = json!({
        "title": pr.title, "description": pr.body, "isDraft": pr.draft,
        "sourceRefName": format!("refs/heads/{}", pr.source_branch),
        "targetRefName": format!("refs/heads/{}", pr.target_branch),
    });
    let url = format!("{}/pullrequests?{VERSION}", repo_api(base, repo)?);
    let body = http::post(&url, &auth(c), &payload)?;
    parse_pull(&body, repo).ok_or_else(unexpected)
}

pub fn repos(base: &str, c: &Credentials) -> Result<Vec<RemoteRepo>> {
    let org = organization(c)?;
    let url = format!("{base}/{org}/_apis/git/repositories?{VERSION}");
    let body = http::get(&url, &auth(c))?;
    Ok(values(&body)?
        .iter()
        .map(|r| RemoteRepo {
            name: format!("{}/{}", str_at(r, "/project/name"), str_at(r, "/name")),
            description: str_at(r, "/project/description"),
            https_url: str_at(r, "/remoteUrl"),
            ssh_url: str_at(r, "/sshUrl"),
            private: str_at(r, "/project/visibility") != "public",
        })
        .collect())
}
