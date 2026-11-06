//! Looks for a newer Ferrum release on GitHub. Only tells; never downloads.

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.2.0`.
    pub version: String,
    /// The release page.
    pub url: String,
}

/// The latest release of `owner/name` on GitHub.
pub fn latest_release(repo: &str) -> Result<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent("ferrum-git-client")
        .build()
        .into();
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let body: Value = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| Error::Other(format!("network error: {e}")))?
        .body_mut()
        .read_json()
        .map_err(|e| Error::Other(format!("network error: {e}")))?;
    parse_release(&body).ok_or_else(|| Error::Other("unexpected answer from GitHub".into()))
}

fn parse_release(body: &Value) -> Option<Release> {
    let tag = body.get("tag_name")?.as_str()?;
    let url = body.get("html_url")?.as_str()?;
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_owned(),
        url: url.to_owned(),
    })
}

/// Whether version `a` is newer than `b` (dotted numbers; a pre-release
/// suffix such as `-rc.1` counts as older than the release itself).
pub fn is_newer(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let (main, pre) = match v.split_once(['-', '+']) {
            Some((m, _)) => (m, true),
            None => (v, false),
        };
        let nums = main.split('.').map(|n| n.parse().unwrap_or(0)).collect();
        (nums, pre)
    }
    let (mut na, pa) = parse(a);
    let (mut nb, pb) = parse(b);
    let len = na.len().max(nb.len());
    na.resize(len, 0);
    nb.resize(len, 0);
    match na.cmp(&nb) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => !pa && pb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer("0.2.0-rc.1", "0.2.0"));
    }

    #[test]
    fn reads_the_release_answer() {
        let body = serde_json::json!({
            "tag_name": "v0.3.1",
            "html_url": "https://github.com/herlondf/ferrum/releases/tag/v0.3.1",
        });
        let r = parse_release(&body).unwrap();
        assert_eq!(r.version, "0.3.1");
        assert!(r.url.ends_with("v0.3.1"));
        assert!(parse_release(&serde_json::json!({"message": "Not Found"})).is_none());
    }
}
