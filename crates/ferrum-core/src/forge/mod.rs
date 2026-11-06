//! Hosted services: pull requests, issues, repositories, notifications and
//! SSH keys on GitHub (and Enterprise), GitLab (and self-hosted), Bitbucket
//! Cloud and Server/Data Center, Azure DevOps and Gitea/Forgejo.

use std::sync::RwLock;

mod azure;
mod bitbucket;
mod gitea;
mod github;
mod gitlab;
mod http;
#[cfg(test)]
mod mock;
pub mod templates;
#[cfg(test)]
mod tests;

pub use http::iso_time;

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ForgeKind {
    GitHub,
    GitLab,
    /// Bitbucket Cloud (bitbucket.org).
    Bitbucket,
    /// Bitbucket Server and Data Center.
    BitbucketServer,
    Azure,
    /// Gitea and Forgejo (Codeberg).
    Gitea,
}

impl ForgeKind {
    pub const ALL: [ForgeKind; 6] = [
        ForgeKind::GitHub,
        ForgeKind::GitLab,
        ForgeKind::Bitbucket,
        ForgeKind::BitbucketServer,
        ForgeKind::Azure,
        ForgeKind::Gitea,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "GitHub",
            ForgeKind::GitLab => "GitLab",
            ForgeKind::Bitbucket => "Bitbucket",
            ForgeKind::BitbucketServer => "Bitbucket Server",
            ForgeKind::Azure => "Azure DevOps",
            ForgeKind::Gitea => "Gitea",
        }
    }

    /// "pull request" or "merge request".
    pub fn request_name(self) -> &'static str {
        match self {
            ForgeKind::GitLab => "merge request",
            _ => "pull request",
        }
    }

    /// The public host, for services that have one.
    pub fn default_host(self) -> Option<&'static str> {
        match self {
            ForgeKind::GitHub => Some("github.com"),
            ForgeKind::GitLab => Some("gitlab.com"),
            ForgeKind::Bitbucket => Some("bitbucket.org"),
            ForgeKind::Azure => Some("dev.azure.com"),
            ForgeKind::BitbucketServer | ForgeKind::Gitea => None,
        }
    }

    /// Whether the API has issues Ferrum can list (Bitbucket Server and Azure
    /// DevOps use Jira and work items instead).
    pub fn has_issues(self) -> bool {
        !matches!(self, ForgeKind::BitbucketServer | ForgeKind::Azure)
    }

    /// What `Credentials::user` means for this service, if it is needed.
    pub fn user_label(self) -> Option<&'static str> {
        match self {
            ForgeKind::Bitbucket => Some("Username"),
            ForgeKind::Azure => Some("Organization"),
            _ => None,
        }
    }

    /// Page where a token with the right scopes is created.
    pub fn token_url(self, host: &str) -> String {
        let web = web_base(host);
        match self {
            ForgeKind::GitHub => {
                format!(
                    "{web}/settings/tokens/new?scopes=repo,notifications,write:public_key&description=Ferrum"
                )
            }
            ForgeKind::GitLab => {
                format!("{web}/-/user_settings/personal_access_tokens?name=Ferrum&scopes=api")
            }
            ForgeKind::Bitbucket => "https://bitbucket.org/account/settings/app-passwords/".into(),
            ForgeKind::BitbucketServer => format!("{web}/plugins/servlet/access-tokens/manage"),
            ForgeKind::Azure => format!("{web}/_usersSettings/tokens"),
            ForgeKind::Gitea => format!("{web}/user/settings/applications"),
        }
    }

    /// Scopes the token needs, as the service names them.
    pub fn token_scopes(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "repo, notifications, write:public_key",
            ForgeKind::GitLab => "api",
            ForgeKind::Bitbucket => "Repositories: Read, Pull requests: Write, Issues: Write",
            ForgeKind::BitbucketServer => "Repository: Write",
            ForgeKind::Azure => "Code: Read & write",
            ForgeKind::Gitea => "repository, issue, user: read and write",
        }
    }

    /// Whether Ferrum can add an SSH key to the account through the API.
    pub fn can_upload_ssh_key(self) -> bool {
        matches!(
            self,
            ForgeKind::GitHub | ForgeKind::GitLab | ForgeKind::Gitea
        )
    }
}

/// A token, and the user name or organization some services need with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    pub token: String,
    /// Bitbucket Cloud: the username for an app password (empty for access
    /// tokens). Azure DevOps: the organization.
    pub user: String,
}

impl Credentials {
    pub fn token(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            user: String::new(),
        }
    }
}

/// `https://host`, or `host` itself when it has a scheme (tests, plain http).
fn web_base(host: &str) -> String {
    if host.contains("://") {
        host.trim_end_matches('/').to_owned()
    } else {
        format!("https://{host}")
    }
}

/// A repository hosted on a forge, parsed from a remote URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedRepo {
    pub kind: ForgeKind,
    pub host: String,
    /// `owner/repo`, `group/subgroup/repo` on GitLab, `PROJECT/repo` on
    /// Bitbucket Server, `organization/project/repo` on Azure DevOps.
    pub path: String,
}

impl HostedRepo {
    fn api_base(&self) -> String {
        api_base(self.kind, &self.host)
    }

    fn owner_and_name(&self) -> (&str, &str) {
        self.path.rsplit_once('/').unwrap_or(("", &self.path))
    }

    pub fn web_url(&self) -> String {
        let web = web_base(&self.host);
        match self.kind {
            ForgeKind::BitbucketServer => {
                let (project, repo) = self.owner_and_name();
                format!("{web}/projects/{project}/repos/{repo}")
            }
            ForgeKind::Azure => match self.path.splitn(3, '/').collect::<Vec<_>>()[..] {
                [org, project, repo] => format!("{web}/{org}/{project}/_git/{repo}"),
                _ => format!("{web}/{}", self.path),
            },
            _ => format!("{web}/{}", self.path),
        }
    }

    /// Browser page to open a new pull request from `branch`.
    pub fn new_request_url(&self, branch: &str, target: &str) -> String {
        let web = self.web_url();
        match self.kind {
            ForgeKind::GitHub => format!("{web}/compare/{target}...{branch}?expand=1"),
            ForgeKind::GitLab => format!(
                "{web}/-/merge_requests/new?merge_request[source_branch]={branch}&merge_request[target_branch]={target}"
            ),
            ForgeKind::Bitbucket => {
                format!("{web}/pull-requests/new?source={branch}&dest={target}")
            }
            ForgeKind::BitbucketServer => format!(
                "{web}/pull-requests?create&sourceBranch=refs/heads/{branch}&targetBranch=refs/heads/{target}"
            ),
            ForgeKind::Azure => {
                format!("{web}/pullrequestcreate?sourceRef={branch}&targetRef={target}")
            }
            ForgeKind::Gitea => format!("{web}/compare/{target}...{branch}"),
        }
    }

    pub fn commit_url(&self, sha: &str) -> String {
        let web = self.web_url();
        match self.kind {
            ForgeKind::GitLab => format!("{web}/-/commit/{sha}"),
            ForgeKind::Bitbucket | ForgeKind::BitbucketServer => format!("{web}/commits/{sha}"),
            _ => format!("{web}/commit/{sha}"),
        }
    }

    pub fn issue_url(&self, number: u64) -> String {
        let web = self.web_url();
        match self.kind {
            ForgeKind::GitLab => format!("{web}/-/issues/{number}"),
            _ => format!("{web}/issues/{number}"),
        }
    }

    pub fn request_url(&self, number: u64) -> String {
        let web = self.web_url();
        match self.kind {
            ForgeKind::GitHub => format!("{web}/pull/{number}"),
            ForgeKind::GitLab => format!("{web}/-/merge_requests/{number}"),
            ForgeKind::Bitbucket | ForgeKind::BitbucketServer => {
                format!("{web}/pull-requests/{number}")
            }
            ForgeKind::Azure => format!("{web}/pullrequest/{number}"),
            ForgeKind::Gitea => format!("{web}/pulls/{number}"),
        }
    }

    /// Issues page of the repository (`None` where the service has none).
    pub fn issues_url(&self) -> Option<String> {
        let web = self.web_url();
        match self.kind {
            ForgeKind::GitLab => Some(format!("{web}/-/issues")),
            k if k.has_issues() => Some(format!("{web}/issues")),
            _ => None,
        }
    }
}

fn api_base(kind: ForgeKind, host: &str) -> String {
    let web = web_base(host);
    match (kind, host) {
        (ForgeKind::GitHub, "github.com") => "https://api.github.com".into(),
        (ForgeKind::GitHub, _) => format!("{web}/api/v3"),
        (ForgeKind::GitLab, _) => format!("{web}/api/v4"),
        (ForgeKind::Bitbucket, "bitbucket.org") => "https://api.bitbucket.org/2.0".into(),
        (ForgeKind::Bitbucket, _) => format!("{web}/2.0"),
        (ForgeKind::BitbucketServer, _) => format!("{web}/rest/api/1.0"),
        (ForgeKind::Azure, _) => web,
        (ForgeKind::Gitea, _) => format!("{web}/api/v1"),
    }
}

// ----- Remote URLs -----

/// Self-hosted servers Ferrum knows from the saved accounts: (host, kind).
static KNOWN_HOSTS: RwLock<Vec<(String, ForgeKind)>> = RwLock::new(Vec::new());

/// Tells `parse_remote_url` which hosts run which service (from the accounts),
/// so self-hosted servers with any name are recognized.
pub fn set_known_hosts(hosts: Vec<(String, ForgeKind)>) {
    if let Ok(mut k) = KNOWN_HOSTS.write() {
        *k = hosts;
    }
}

pub fn known_hosts() -> Vec<(String, ForgeKind)> {
    KNOWN_HOSTS.read().map(|k| k.clone()).unwrap_or_default()
}

/// Recognizes hosted remotes in https, ssh and scp-like form.
pub fn parse_remote_url(url: &str) -> Option<HostedRepo> {
    parse_remote_url_with(url, &known_hosts())
}

/// The host name without a scheme or port.
fn host_name(host: &str) -> &str {
    let host = host.split_once("://").map_or(host, |(_, h)| h);
    let host = host.split('/').next().unwrap_or(host);
    host.split(':').next().unwrap_or(host)
}

pub fn parse_remote_url_with(url: &str, known: &[(String, ForgeKind)]) -> Option<HostedRepo> {
    let url = url.trim();
    let (host, path, http) = if let Some((scheme, rest)) = url.split_once("://") {
        // https://host/owner/repo, ssh://git@host:22/owner/repo
        let rest = rest
            .split_once('/')
            .map_or(rest, |(auth_host, _)| auth_host);
        let host_part = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
        let path = url.split_once("://")?.1.split_once('/')?.1;
        let http = scheme.starts_with("http");
        // Keep the port of web URLs (the API is on the same port).
        let host = if http {
            host_part
        } else {
            host_part.split(':').next()?
        };
        (host, path, http)
    } else {
        // git@host:owner/repo
        let rest = url.rsplit_once('@').map_or(url, |(_, r)| r);
        let (host, path) = rest.split_once(':')?;
        if host.contains('/') || host.len() < 2 {
            // A local path such as C:/x or ./a:b.
            return None;
        }
        (host, path, false)
    };
    let path = path
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    let lower = host.to_ascii_lowercase();
    let name = host_name(&lower).to_owned();

    let account = known
        .iter()
        .find(|(h, _)| host_name(&h.to_ascii_lowercase()) == name);
    let kind = if let Some((_, kind)) = account {
        *kind
    } else if name == "dev.azure.com"
        || name.ends_with(".visualstudio.com")
        || name == "ssh.dev.azure.com"
    {
        ForgeKind::Azure
    } else if name == "bitbucket.org" {
        ForgeKind::Bitbucket
    } else if name.contains("github") {
        ForgeKind::GitHub
    } else if name.contains("gitlab") {
        ForgeKind::GitLab
    } else if name.contains("bitbucket") || (http && path.starts_with("scm/")) {
        ForgeKind::BitbucketServer
    } else if name.contains("gitea") || name.contains("forgejo") || name == "codeberg.org" {
        ForgeKind::Gitea
    } else {
        return None;
    };

    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let path = match kind {
        ForgeKind::Azure => {
            // dev.azure.com/org/project/_git/repo, ssh.dev.azure.com:v3/org/project/repo,
            // org.visualstudio.com/[DefaultCollection/]project/_git/repo
            let parts: Vec<&str> = parts
                .iter()
                .copied()
                .filter(|p| *p != "_git" && *p != "v3" && *p != "DefaultCollection")
                .collect();
            match (name.strip_suffix(".visualstudio.com"), parts.as_slice()) {
                (Some(org), [project, repo]) if org != "vs-ssh" => {
                    format!("{org}/{project}/{repo}")
                }
                (_, [org, project, repo]) => format!("{org}/{project}/{repo}"),
                _ => return None,
            }
        }
        ForgeKind::BitbucketServer => {
            let parts = if parts.first() == Some(&"scm") {
                &parts[1..]
            } else {
                &parts[..]
            };
            match parts {
                [project, repo] => format!("{project}/{repo}"),
                _ => return None,
            }
        }
        _ => {
            if parts.len() < 2 {
                return None;
            }
            parts.join("/")
        }
    };
    let host = match (kind, account) {
        (ForgeKind::Azure, _) => "dev.azure.com".to_owned(),
        // The account's host may carry a port or scheme the remote lacks.
        (_, Some((h, _))) => h.to_ascii_lowercase(),
        _ => lower,
    };
    Some(HostedRepo { kind, host, path })
}

// ----- Data -----

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub open: bool,
    pub url: String,
    pub updated_at: String,
    pub comments: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum IssueState {
    #[default]
    Open,
    Closed,
    All,
}

/// Which issues to list. Empty fields do not filter.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct IssueFilter {
    pub state: IssueState,
    pub author: String,
    pub assignee: String,
    pub label: String,
    /// Words in the title (matched by Ferrum after loading).
    pub text: String,
}

impl IssueFilter {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Keeps the issues whose title has every word of `text`, and applies
    /// the filters the service could not.
    pub fn matches(&self, issue: &Issue) -> bool {
        let title = issue.title.to_lowercase();
        let eq = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        self.text
            .to_lowercase()
            .split_whitespace()
            .all(|w| title.contains(w) || issue.number.to_string() == w.trim_start_matches('#'))
            && (self.author.trim().is_empty() || eq(&self.author, &issue.author))
            && (self.assignee.trim().is_empty()
                || issue.assignees.iter().any(|a| eq(a, &self.assignee)))
            && (self.label.trim().is_empty() || issue.labels.iter().any(|l| eq(l, &self.label)))
            && match self.state {
                IssueState::Open => issue.open,
                IssueState::Closed => !issue.open,
                IssueState::All => true,
            }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NewIssue {
    pub title: String,
    pub body: String,
}

/// A repository of the logged-in account, for cloning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepo {
    /// `owner/name` (or the service's equivalent).
    pub name: String,
    pub description: String,
    pub https_url: String,
    pub ssh_url: String,
    pub private: bool,
}

/// A GitHub notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: String,
    pub title: String,
    /// `PullRequest`, `Issue`, `Release`…
    pub kind: String,
    pub repo: String,
    /// Page in the browser.
    pub url: String,
    pub reason: String,
    pub updated_at: String,
}

// ----- Requests -----

/// Checks a token and returns the account name.
pub fn verify_token(kind: ForgeKind, host: &str, creds: &Credentials) -> Result<String> {
    let base = api_base(kind, host);
    match kind {
        ForgeKind::GitHub => github::verify(&base, creds),
        ForgeKind::GitLab => gitlab::verify(&base, creds),
        ForgeKind::Bitbucket => bitbucket::verify(&base, creds),
        ForgeKind::BitbucketServer => bitbucket::server_verify(&base, creds),
        ForgeKind::Azure => azure::verify(&base, creds),
        ForgeKind::Gitea => gitea::verify(&base, creds),
    }
}

/// The scopes of a GitHub classic token (`None` when the service does not
/// say, e.g. fine-grained tokens and other services).
pub fn token_scopes(
    kind: ForgeKind,
    host: &str,
    creds: &Credentials,
) -> Result<Option<Vec<String>>> {
    match kind {
        ForgeKind::GitHub => github::scopes(&api_base(kind, host), creds),
        _ => Ok(None),
    }
}

pub fn list_pull_requests(repo: &HostedRepo, creds: &Credentials) -> Result<Vec<PullRequest>> {
    let base = repo.api_base();
    match repo.kind {
        ForgeKind::GitHub => github::pulls(&base, repo, creds),
        ForgeKind::GitLab => gitlab::merges(&base, repo, creds),
        ForgeKind::Bitbucket => bitbucket::pulls(&base, repo, creds),
        ForgeKind::BitbucketServer => bitbucket::server_pulls(&base, repo, creds),
        ForgeKind::Azure => azure::pulls(&base, repo, creds),
        ForgeKind::Gitea => gitea::pulls(&base, repo, creds),
    }
}

pub fn create_pull_request(
    repo: &HostedRepo,
    creds: &Credentials,
    pr: &NewPullRequest,
) -> Result<PullRequest> {
    let base = repo.api_base();
    match repo.kind {
        ForgeKind::GitHub => github::create_pull(&base, repo, creds, pr),
        ForgeKind::GitLab => gitlab::create_merge(&base, repo, creds, pr),
        ForgeKind::Bitbucket => bitbucket::create_pull(&base, repo, creds, pr),
        ForgeKind::BitbucketServer => bitbucket::server_create_pull(&base, repo, creds, pr),
        ForgeKind::Azure => azure::create_pull(&base, repo, creds, pr),
        ForgeKind::Gitea => gitea::create_pull(&base, repo, creds, pr),
    }
}

/// Issues matching `filter` (newest activity first).
pub fn list_issues(
    repo: &HostedRepo,
    creds: &Credentials,
    filter: &IssueFilter,
) -> Result<Vec<Issue>> {
    let base = repo.api_base();
    let list = match repo.kind {
        ForgeKind::GitHub => github::issues(&base, repo, creds, filter),
        ForgeKind::GitLab => gitlab::issues(&base, repo, creds, filter),
        ForgeKind::Bitbucket => bitbucket::issues(&base, repo, creds, filter),
        ForgeKind::Gitea => gitea::issues(&base, repo, creds, filter),
        ForgeKind::BitbucketServer | ForgeKind::Azure => Err(crate::Error::Other(format!(
            "{} has no issues Ferrum can list",
            repo.kind.name()
        ))),
    }?;
    Ok(list.into_iter().filter(|i| filter.matches(i)).collect())
}

pub fn create_issue(repo: &HostedRepo, creds: &Credentials, issue: &NewIssue) -> Result<Issue> {
    let base = repo.api_base();
    match repo.kind {
        ForgeKind::GitHub => github::create_issue(&base, repo, creds, issue),
        ForgeKind::GitLab => gitlab::create_issue(&base, repo, creds, issue),
        ForgeKind::Bitbucket => bitbucket::create_issue(&base, repo, creds, issue),
        ForgeKind::Gitea => gitea::create_issue(&base, repo, creds, issue),
        ForgeKind::BitbucketServer | ForgeKind::Azure => Err(crate::Error::Other(format!(
            "{} has no issues Ferrum can create",
            repo.kind.name()
        ))),
    }
}

/// Repositories the account can see, most recently updated first.
pub fn list_repositories(
    kind: ForgeKind,
    host: &str,
    creds: &Credentials,
) -> Result<Vec<RemoteRepo>> {
    let base = api_base(kind, host);
    match kind {
        ForgeKind::GitHub => github::repos(&base, creds),
        ForgeKind::GitLab => gitlab::repos(&base, creds),
        ForgeKind::Bitbucket => bitbucket::repos(&base, creds),
        ForgeKind::BitbucketServer => bitbucket::server_repos(&base, creds),
        ForgeKind::Azure => azure::repos(&base, creds),
        ForgeKind::Gitea => gitea::repos(&base, creds),
    }
}

/// Unread GitHub notifications.
pub fn list_notifications(host: &str, creds: &Credentials) -> Result<Vec<Notification>> {
    github::notifications(&api_base(ForgeKind::GitHub, host), host, creds)
}

/// Marks every GitHub notification as read.
pub fn mark_notifications_read(host: &str, creds: &Credentials) -> Result<()> {
    github::mark_read(&api_base(ForgeKind::GitHub, host), creds)
}

/// Adds a public SSH key to the account.
pub fn upload_ssh_key(
    kind: ForgeKind,
    host: &str,
    creds: &Credentials,
    title: &str,
    key: &str,
) -> Result<()> {
    let base = api_base(kind, host);
    let payload = serde_json::json!({"title": title, "key": key.trim()});
    let auth = match kind {
        ForgeKind::GitHub => github::auth(creds),
        ForgeKind::GitLab => gitlab::auth(creds),
        ForgeKind::Gitea => gitea::auth(creds),
        other => {
            return Err(crate::Error::Other(format!(
                "Adding SSH keys to {} is not supported; add it on the website",
                other.name()
            )));
        }
    };
    let url = match kind {
        ForgeKind::GitHub | ForgeKind::GitLab | ForgeKind::Gitea => format!("{base}/user/keys"),
        _ => unreachable!(),
    };
    http::post(&url, &auth, &payload).map(drop).map_err(|e| {
        let text = e.to_string();
        if text.contains("HTTP 403") || text.contains("HTTP 404") || text.contains("HTTP 401") {
            let scope = match kind {
                ForgeKind::GitHub => "write:public_key",
                ForgeKind::GitLab => "api",
                _ => "write:user",
            };
            crate::Error::Other(format!(
                "{text}\nThe token needs the \"{scope}\" scope to add SSH keys. Create a token with it, or add the key on the website."
            ))
        } else {
            e
        }
    })
}

/// Token from the GitHub CLI (`gh auth token`), when it is installed and logged in.
pub fn gh_cli_token(host: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token", "--hostname", host])
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    let token = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !token.is_empty()).then_some(token)
}
