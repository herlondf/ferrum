//! HTTP requests to forge APIs: authentication headers, JSON bodies and
//! readable errors.

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

/// How a request proves who it is.
#[derive(Debug, Clone)]
pub enum Auth {
    Bearer(String),
    /// A header with the raw token, e.g. GitLab's `PRIVATE-TOKEN`.
    Header(&'static str, String),
    /// `Authorization: token X` (Gitea, Forgejo).
    Token(String),
    Basic(String, String),
}

impl Auth {
    fn header(&self) -> (&'static str, String) {
        match self {
            Auth::Bearer(t) => ("Authorization", format!("Bearer {t}")),
            Auth::Header(name, t) => (name, t.clone()),
            Auth::Token(t) => ("Authorization", format!("token {t}")),
            Auth::Basic(u, p) => (
                "Authorization",
                format!("Basic {}", base64(format!("{u}:{p}").as_bytes())),
            ),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent("ferrum-git-client")
        .build()
        .into()
}

/// A successful response: its JSON body and the headers callers look at.
pub struct Reply {
    pub body: Value,
    /// `X-OAuth-Scopes` (GitHub classic tokens).
    pub scopes: Option<String>,
    /// `X-AUSERNAME` (Bitbucket Server).
    pub user: Option<String>,
}

pub fn get(url: &str, auth: &Auth) -> Result<Value> {
    get_reply(url, auth).map(|r| r.body)
}

pub fn get_reply(url: &str, auth: &Auth) -> Result<Reply> {
    let (k, v) = auth.header();
    let resp = agent()
        .get(url)
        .header(k, &v)
        .header("Accept", "application/json")
        .call()
        .map_err(net)?;
    check(resp)
}

pub fn post(url: &str, auth: &Auth, payload: &Value) -> Result<Value> {
    let (k, v) = auth.header();
    let resp = agent()
        .post(url)
        .header(k, &v)
        .header("Accept", "application/json")
        .send_json(payload)
        .map_err(net)?;
    check(resp).map(|r| r.body)
}

pub fn put_empty(url: &str, auth: &Auth) -> Result<()> {
    let (k, v) = auth.header();
    let resp = agent()
        .put(url)
        .header(k, &v)
        .header("Accept", "application/json")
        .send_empty()
        .map_err(net)?;
    check(resp).map(drop)
}

fn check(mut resp: ureq::http::Response<ureq::Body>) -> Result<Reply> {
    let status = resp.status().as_u16();
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let (scopes, user) = (header("x-oauth-scopes"), header("x-ausername"));
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(Reply { body, scopes, user });
    }
    let detail = error_detail(&body);
    let hint = match status {
        401 => " (the token is invalid or expired)",
        403 => " (the token lacks permission)",
        404 => " (not found, or the token cannot see it)",
        _ => "",
    };
    Err(Error::Other(format!("HTTP {status}{hint}: {detail}")))
}

/// The readable part of an API error: GitHub's `message` plus its `errors`
/// list (e.g. "A pull request already exists"), GitLab's `message` (a string,
/// list or field map), Bitbucket's `error.message`, or `error`.
pub fn error_detail(body: &Value) -> String {
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

/// Unexpected JSON from the server.
pub fn unexpected() -> Error {
    Error::Other("unexpected response from the server".into())
}

/// Percent-encodes a URL query or path component.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn base64(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(CHARS[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Unix seconds as `2026-11-01T10:00:00Z`.
pub fn iso_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Reads a string at a JSON pointer, or "".
pub fn str_at(v: &Value, pointer: &str) -> String {
    v.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes() {
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b":t"), "OnQ=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b""), "");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
        assert_eq!(iso_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_time(1_700_000_000), "2023-11-14T22:13:20Z");
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
        let bb = json!({"type": "error", "error": {"message": "Repository not found"}});
        assert_eq!(error_detail(&bb), "Repository not found");
        // Bitbucket Server: {"errors": [{"message": "..."}]}.
        let bbs = json!({"errors": [{"context": null, "message": "Already exists"}]});
        assert_eq!(error_detail(&bbs), "Already exists");
        assert_eq!(error_detail(&Value::Null), "");
    }
}
