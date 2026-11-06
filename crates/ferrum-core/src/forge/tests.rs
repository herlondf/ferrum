use serde_json::{Value, json};

use super::mock::{self, Mock};
use super::*;

fn repo(kind: ForgeKind, host: &str, path: &str) -> HostedRepo {
    HostedRepo {
        kind,
        host: host.into(),
        path: path.into(),
    }
}

#[test]
fn parses_remote_urls() {
    let p = |u: &str| parse_remote_url_with(u, &[]).map(|r| (r.kind, r.host, r.path));
    let gh = |path: &str| Some((ForgeKind::GitHub, "github.com".to_owned(), path.to_owned()));
    assert_eq!(
        p("https://github.com/herlondf/ferrum.git"),
        gh("herlondf/ferrum")
    );
    assert_eq!(
        p("git@github.com:herlondf/ferrum.git"),
        gh("herlondf/ferrum")
    );
    assert_eq!(
        p("ssh://git@github.com/herlondf/ferrum"),
        gh("herlondf/ferrum")
    );
    assert_eq!(p("https://user:tok@github.com/a/b/"), gh("a/b"));
    let gl = parse_remote_url_with("git@gitlab.example.com:group/sub/repo.git", &[]).unwrap();
    assert_eq!(
        (gl.kind, gl.host.as_str(), gl.path.as_str()),
        (ForgeKind::GitLab, "gitlab.example.com", "group/sub/repo")
    );
    assert_eq!(gl.api_base(), "https://gitlab.example.com/api/v4");
    assert_eq!(
        p("https://bitbucket.org/team/app.git"),
        Some((
            ForgeKind::Bitbucket,
            "bitbucket.org".into(),
            "team/app".into()
        ))
    );
    assert_eq!(
        p("git@bitbucket.org:team/app.git"),
        Some((
            ForgeKind::Bitbucket,
            "bitbucket.org".into(),
            "team/app".into()
        ))
    );
    // Bitbucket Server: /scm/ in web URLs, a port in ssh ones.
    assert_eq!(
        p("https://bitbucket.corp.com:7990/scm/PROJ/app.git"),
        Some((
            ForgeKind::BitbucketServer,
            "bitbucket.corp.com:7990".into(),
            "PROJ/app".into()
        ))
    );
    assert_eq!(
        p("ssh://git@bitbucket.corp.com:7999/proj/app.git"),
        Some((
            ForgeKind::BitbucketServer,
            "bitbucket.corp.com".into(),
            "proj/app".into()
        ))
    );
    let az = |path: &str| {
        Some((
            ForgeKind::Azure,
            "dev.azure.com".to_owned(),
            path.to_owned(),
        ))
    };
    assert_eq!(
        p("https://me@dev.azure.com/org/proj/_git/app"),
        az("org/proj/app")
    );
    assert_eq!(
        p("git@ssh.dev.azure.com:v3/org/proj/app"),
        az("org/proj/app")
    );
    assert_eq!(
        p("https://org.visualstudio.com/proj/_git/app"),
        az("org/proj/app")
    );
    assert_eq!(
        p("https://org.visualstudio.com/DefaultCollection/proj/_git/app"),
        az("org/proj/app")
    );
    assert_eq!(
        p("https://codeberg.org/a/b.git"),
        Some((ForgeKind::Gitea, "codeberg.org".into(), "a/b".into()))
    );
    assert!(p("https://git.corp.com/a/b").is_none());
    assert!(p("/local/path/repo").is_none());
    assert!(p("C:/work/repo").is_none());

    // Self-hosted servers known from the accounts.
    let known = vec![
        ("git.corp.com".to_owned(), ForgeKind::GitHub),
        ("code.corp.com:8443".to_owned(), ForgeKind::Gitea),
    ];
    let k = |u: &str| parse_remote_url_with(u, &known).map(|r| (r.kind, r.host, r.path));
    assert_eq!(
        k("git@git.corp.com:a/b.git"),
        Some((ForgeKind::GitHub, "git.corp.com".into(), "a/b".into()))
    );
    assert_eq!(
        k("ssh://git@code.corp.com:2222/a/b.git"),
        Some((ForgeKind::Gitea, "code.corp.com:8443".into(), "a/b".into()))
    );
}

#[test]
fn builds_web_urls() {
    let bbs = repo(ForgeKind::BitbucketServer, "bb.corp", "PROJ/app");
    assert_eq!(bbs.web_url(), "https://bb.corp/projects/PROJ/repos/app");
    assert_eq!(
        bbs.request_url(4),
        "https://bb.corp/projects/PROJ/repos/app/pull-requests/4"
    );
    let az = repo(ForgeKind::Azure, "dev.azure.com", "org/proj/app");
    assert_eq!(az.web_url(), "https://dev.azure.com/org/proj/_git/app");
    assert_eq!(
        az.commit_url("abc"),
        "https://dev.azure.com/org/proj/_git/app/commit/abc"
    );
    assert!(az.issues_url().is_none());
    let gl = repo(ForgeKind::GitLab, "gitlab.com", "g/r");
    assert_eq!(gl.issue_url(2), "https://gitlab.com/g/r/-/issues/2");
    assert_eq!(gl.commit_url("abc"), "https://gitlab.com/g/r/-/commit/abc");
    let ghe = repo(ForgeKind::GitHub, "ghe.corp", "a/b");
    assert_eq!(ghe.api_base(), "https://ghe.corp/api/v3");
    assert_eq!(
        repo(ForgeKind::Bitbucket, "bitbucket.org", "a/b").api_base(),
        "https://api.bitbucket.org/2.0"
    );
}

/// A server answering every request with `body`.
fn answer(body: Value) -> Mock {
    let text = body.to_string();
    mock::serve(move |_, _| (200, Vec::new(), text.clone()))
}

fn creds() -> Credentials {
    Credentials::token("tok")
}

#[test]
fn github_requests() {
    let m = answer(json!([{
        "number": 7, "title": "Add graph", "user": {"login": "ana"},
        "head": {"ref": "feature"}, "base": {"ref": "main"},
        "html_url": "https://github.com/a/b/pull/7", "draft": true,
        "updated_at": "2026-11-01T10:00:00Z"
    }]));
    let r = repo(ForgeKind::GitHub, &m.base, "a/b");
    let prs = list_pull_requests(&r, &creds()).unwrap();
    assert_eq!(
        (prs[0].number, prs[0].source_branch.as_str(), prs[0].draft),
        (7, "feature", true)
    );
    let got = m.last();
    assert_eq!(got.path, "/api/v3/repos/a/b/pulls?state=open&per_page=100");
    assert_eq!(got.header("authorization"), Some("Bearer tok"));

    // Issues skip pull requests and send the filters.
    let m = answer(json!([
        {"number": 1, "title": "Crash on start", "user": {"login": "bo"}, "state": "open",
         "assignees": [{"login": "ana"}], "labels": [{"name": "bug"}],
         "html_url": "u", "updated_at": "t", "comments": 3},
        {"number": 2, "title": "A PR", "user": {"login": "bo"}, "state": "open",
         "pull_request": {}, "html_url": "u", "updated_at": "t"}
    ]));
    let r = repo(ForgeKind::GitHub, &m.base, "a/b");
    let filter = IssueFilter {
        assignee: "ana".into(),
        label: "bug".into(),
        ..Default::default()
    };
    let issues = list_issues(&r, &creds(), &filter).unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(
        (issues[0].comments, issues[0].labels.as_slice()),
        (3, &["bug".to_owned()][..])
    );
    assert!(
        m.last().path.contains("&assignee=ana&labels=bug"),
        "{}",
        m.last().path
    );
    // Text is matched locally.
    let none = IssueFilter {
        text: "freeze".into(),
        ..Default::default()
    };
    assert!(list_issues(&r, &creds(), &none).unwrap().is_empty());

    // Creating sends the fields.
    let m = answer(json!({"number": 9, "title": "T", "user": {"login": "me"},
        "head": {"ref": "f"}, "base": {"ref": "main"}, "html_url": "u", "updated_at": "t"}));
    let r = repo(ForgeKind::GitHub, &m.base, "a/b");
    let pr = NewPullRequest {
        title: "T".into(),
        body: "B".into(),
        source_branch: "f".into(),
        target_branch: "main".into(),
        draft: false,
    };
    assert_eq!(create_pull_request(&r, &creds(), &pr).unwrap().number, 9);
    assert_eq!(m.last().method, "POST");
    let sent: Value = serde_json::from_str(&m.last().body).unwrap();
    assert_eq!(
        (sent["head"].as_str(), sent["base"].as_str()),
        (Some("f"), Some("main"))
    );
}

#[test]
fn github_scopes_notifications_and_keys() {
    let m = mock::serve(|method, path| match (method, path) {
        ("GET", "/api/v3/user") => (
            200,
            vec![("X-OAuth-Scopes", "repo, notifications".into())],
            json!({"login": "ana"}).to_string(),
        ),
        ("GET", p) if p.starts_with("/api/v3/notifications") => (
            200,
            Vec::new(),
            json!([{"id": "1", "reason": "mention", "updated_at": "t",
                "repository": {"full_name": "a/b"},
                "subject": {"title": "Fix", "type": "PullRequest",
                            "url": "https://x/api/v3/repos/a/b/pulls/3"}}])
            .to_string(),
        ),
        ("POST", "/api/v3/user/keys") => {
            (404, Vec::new(), json!({"message": "Not Found"}).to_string())
        }
        _ => (500, Vec::new(), "{}".into()),
    });
    assert_eq!(
        verify_token(ForgeKind::GitHub, &m.base, &creds()).unwrap(),
        "ana"
    );
    let scopes = token_scopes(ForgeKind::GitHub, &m.base, &creds())
        .unwrap()
        .unwrap();
    assert_eq!(scopes, ["repo", "notifications"]);
    let n = list_notifications(&m.base, &creds()).unwrap();
    assert_eq!(n[0].url, format!("{}/a/b/pull/3", m.base));
    let err = upload_ssh_key(
        ForgeKind::GitHub,
        &m.base,
        &creds(),
        "laptop",
        "ssh-ed25519 AAA",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("write:public_key"), "{err}");
    let sent: Value = serde_json::from_str(&m.last().body).unwrap();
    assert_eq!(sent["key"], "ssh-ed25519 AAA");
}

#[test]
fn gitlab_requests() {
    let m = answer(json!([{
        "iid": 3, "title": "Fix", "author": {"username": "bo"},
        "source_branch": "fix", "target_branch": "main",
        "web_url": "https://gitlab.com/a/b/-/merge_requests/3",
        "updated_at": "2026-11-01T10:00:00Z"
    }]));
    let r = repo(ForgeKind::GitLab, &m.base, "group/sub/app");
    let mrs = list_pull_requests(&r, &creds()).unwrap();
    assert_eq!((mrs[0].number, mrs[0].author.as_str()), (3, "bo"));
    let got = m.last();
    assert_eq!(
        got.path,
        "/api/v4/projects/group%2Fsub%2Fapp/merge_requests?state=opened&per_page=100"
    );
    assert_eq!(got.header("private-token"), Some("tok"));

    let m = answer(
        json!({"iid": 4, "title": "Draft: T", "author": {"username": "me"},
        "source_branch": "f", "target_branch": "main", "web_url": "u", "updated_at": "t"}),
    );
    let r = repo(ForgeKind::GitLab, &m.base, "g/app");
    let pr = NewPullRequest {
        title: "T".into(),
        draft: true,
        source_branch: "f".into(),
        target_branch: "main".into(),
        ..Default::default()
    };
    create_pull_request(&r, &creds(), &pr).unwrap();
    let sent: Value = serde_json::from_str(&m.last().body).unwrap();
    assert_eq!(sent["title"], "Draft: T");

    let m = answer(
        json!([{"path_with_namespace": "g/app", "http_url_to_repo": "https://h/g/app.git",
        "ssh_url_to_repo": "git@h:g/app.git", "visibility": "private"}]),
    );
    let repos = list_repositories(ForgeKind::GitLab, &m.base, &creds()).unwrap();
    assert_eq!(
        (repos[0].ssh_url.as_str(), repos[0].private),
        ("git@h:g/app.git", true)
    );
}

#[test]
fn bitbucket_cloud_requests() {
    let m = answer(json!({"values": [{
        "id": 5, "title": "Feature", "author": {"nickname": "ana"},
        "source": {"branch": {"name": "feat"}}, "destination": {"branch": {"name": "main"}},
        "links": {"html": {"href": "https://bitbucket.org/t/a/pull-requests/5"}},
        "updated_on": "2026-11-01T10:00:00Z"
    }]}));
    let r = repo(ForgeKind::Bitbucket, &m.base, "team/app");
    let c = Credentials {
        token: "secret".into(),
        user: "ana".into(),
    };
    let prs = list_pull_requests(&r, &c).unwrap();
    assert_eq!((prs[0].number, prs[0].target_branch.as_str()), (5, "main"));
    let got = m.last();
    assert_eq!(
        got.path,
        "/2.0/repositories/team/app/pullrequests?state=OPEN&pagelen=50"
    );
    assert_eq!(got.header("authorization"), Some("Basic YW5hOnNlY3JldA=="));

    create_pull_request(
        &r,
        &c,
        &NewPullRequest {
            title: "T".into(),
            source_branch: "feat".into(),
            target_branch: "main".into(),
            ..Default::default()
        },
    )
    .unwrap_err(); // The mock answers with a list, not a pull request.
    let sent: Value = serde_json::from_str(&m.last().body).unwrap();
    assert_eq!(sent["source"]["branch"]["name"], "feat");

    let m = answer(json!({"values": [{"id": 2, "title": "Bug", "state": "new",
        "reporter": {"nickname": "bo"}, "links": {"html": {"href": "u"}}, "updated_on": "t"}]}));
    let r = repo(ForgeKind::Bitbucket, &m.base, "team/app");
    let issues = list_issues(&r, &c, &IssueFilter::default()).unwrap();
    assert!(issues[0].open);

    let m = answer(
        json!({"values": [{"full_name": "team/app", "is_private": true,
        "links": {"clone": [{"name": "https", "href": "https://ana@bitbucket.org/team/app.git"},
                            {"name": "ssh", "href": "git@bitbucket.org:team/app.git"}]}}]}),
    );
    let repos = list_repositories(ForgeKind::Bitbucket, &m.base, &c).unwrap();
    assert_eq!(repos[0].ssh_url, "git@bitbucket.org:team/app.git");
}

#[test]
fn bitbucket_server_requests() {
    let m = mock::serve(|_, path| {
        if path.starts_with("/rest/api/1.0/profile") {
            (
                200,
                vec![("X-AUSERNAME", "ana".into())],
                json!({"values": []}).to_string(),
            )
        } else {
            let pr = json!({"id": 8, "title": "Change", "author": {"user": {"name": "ana"}},
                "fromRef": {"displayId": "feat"}, "toRef": {"displayId": "main"},
                "links": {"self": [{"href": "https://bb/projects/P/repos/a/pull-requests/8"}]},
                "updatedDate": 1_700_000_000_000_i64});
            (200, Vec::new(), json!({"values": [pr]}).to_string())
        }
    });
    assert_eq!(
        verify_token(ForgeKind::BitbucketServer, &m.base, &creds()).unwrap(),
        "ana"
    );
    let r = repo(ForgeKind::BitbucketServer, &m.base, "PROJ/app");
    let prs = list_pull_requests(&r, &creds()).unwrap();
    assert_eq!(prs[0].updated_at, "2023-11-14T22:13:20Z");
    assert_eq!(
        m.last().path,
        "/rest/api/1.0/projects/PROJ/repos/app/pull-requests?state=OPEN&limit=100"
    );
    let _ = create_pull_request(
        &r,
        &creds(),
        &NewPullRequest {
            title: "T".into(),
            source_branch: "feat".into(),
            target_branch: "main".into(),
            ..Default::default()
        },
    );
    let sent: Value = serde_json::from_str(&m.last().body).unwrap();
    assert_eq!(sent["fromRef"]["id"], "refs/heads/feat");
    assert!(list_issues(&r, &creds(), &IssueFilter::default()).is_err());
}

#[test]
fn azure_requests() {
    let m = mock::serve(|_, path| {
        if path.contains("connectionData") {
            let user = json!({"authenticatedUser": {"providerDisplayName": "Ana"}});
            (200, Vec::new(), user.to_string())
        } else {
            let pr = json!({"pullRequestId": 12, "title": "T", "createdBy": {"uniqueName": "ana@x"},
                "sourceRefName": "refs/heads/feat", "targetRefName": "refs/heads/main",
                "isDraft": true, "creationDate": "2026-01-01T00:00:00Z"});
            (200, Vec::new(), json!({"value": [pr]}).to_string())
        }
    });
    let c = Credentials {
        token: "pat".into(),
        user: "org".into(),
    };
    assert_eq!(verify_token(ForgeKind::Azure, &m.base, &c).unwrap(), "Ana");
    assert_eq!(m.last().path, "/org/_apis/connectionData");
    assert_eq!(m.last().header("authorization"), Some("Basic OnBhdA=="));
    let r = repo(ForgeKind::Azure, &m.base, "org/My Project/app");
    let prs = list_pull_requests(&r, &c).unwrap();
    assert_eq!(
        (prs[0].source_branch.as_str(), prs[0].draft),
        ("feat", true)
    );
    assert!(
        prs[0]
            .url
            .ends_with("/org/My Project/_git/app/pullrequest/12")
    );
    assert!(
        m.last()
            .path
            .starts_with("/org/My%20Project/_apis/git/repositories/app/pullrequests?")
    );
    let no_org = Credentials::token("pat");
    assert!(verify_token(ForgeKind::Azure, &m.base, &no_org).is_err());
}

#[test]
fn gitea_requests() {
    let m = answer(json!([{
        "number": 2, "title": "Pull", "user": {"login": "ana"},
        "head": {"ref": "feat"}, "base": {"ref": "main"}, "html_url": "u", "updated_at": "t",
        "state": "open"
    }]));
    let r = repo(ForgeKind::Gitea, &m.base, "a/b");
    assert_eq!(list_pull_requests(&r, &creds()).unwrap()[0].number, 2);
    let got = m.last();
    assert_eq!(got.path, "/api/v1/repos/a/b/pulls?state=open&limit=50");
    assert_eq!(got.header("authorization"), Some("token tok"));
    let issues = list_issues(&r, &creds(), &IssueFilter::default()).unwrap();
    assert_eq!(issues.len(), 1, "an entry without pull_request is an issue");
}

#[test]
fn reports_http_errors() {
    let m = mock::serve(|_, _| {
        (
            401,
            Vec::new(),
            json!({"message": "Bad credentials"}).to_string(),
        )
    });
    let err = verify_token(ForgeKind::GitHub, &m.base, &creds())
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "HTTP 401 (the token is invalid or expired): Bad credentials"
    );
    assert_eq!(m.all().len(), 1);
}
