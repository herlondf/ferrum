//! Author avatars: where to find a picture for an email address, and a small
//! disk cache so each one is downloaded once.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{Error, Result};

/// Picture size requested, in pixels.
pub const SIZE: u32 = 64;

/// URLs to try for an email, best first: the GitHub account for GitHub
/// `noreply` addresses, then Gravatar (which answers 404 when it has none).
pub fn urls(email: &str) -> Vec<String> {
    let email = email.trim().to_lowercase();
    let mut out = Vec::new();
    if let Some(user) = email.strip_suffix("@users.noreply.github.com") {
        // `12345+name@…` (newer) or `name@…` (older addresses).
        match user.split_once('+') {
            Some((id, _)) if id.chars().all(|c| c.is_ascii_digit()) => out.push(format!(
                "https://avatars.githubusercontent.com/u/{id}?s={SIZE}"
            )),
            _ => out.push(format!("https://github.com/{user}.png?size={SIZE}")),
        }
    }
    out.push(format!(
        "https://www.gravatar.com/avatar/{:x}?s={SIZE}&d=404",
        md5::compute(email.as_bytes())
    ));
    out
}

/// Cache file of an email's picture in `dir`. An empty file means "no picture".
pub fn cache_file(dir: &Path, email: &str) -> PathBuf {
    let key = md5::compute(email.trim().to_lowercase().as_bytes());
    dir.join(format!("{key:x}"))
}

/// The picture for `email` (PNG or JPEG bytes), from the cache or the network.
/// `None` when no service has one.
pub fn fetch(dir: &Path, email: &str) -> Result<Option<Vec<u8>>> {
    let file = cache_file(dir, email);
    if let Ok(bytes) = std::fs::read(&file) {
        return Ok(Some(bytes).filter(|b| !b.is_empty()));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .user_agent("ferrum-git-client")
        .build()
        .into();
    let mut found = None;
    for url in urls(email) {
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| Error::Other(format!("network error: {e}")))?;
        if resp.status().is_success() {
            let mut bytes = Vec::new();
            resp.body_mut()
                .as_reader()
                .take(2 << 20)
                .read_to_end(&mut bytes)?;
            found = Some(bytes);
            break;
        }
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&file, found.as_deref().unwrap_or_default())?;
    Ok(found)
}

/// One or two letters for an avatar without a picture: "Ada Lovelace" -> "AL".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    let first = |w: &str| {
        w.chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
    };
    match words.as_slice() {
        [] => "?".into(),
        [one] => first(one).unwrap_or_default(),
        [a, .., b] => format!(
            "{}{}",
            first(a).unwrap_or_default(),
            first(b).unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_urls() {
        let u = urls(" Test@Example.com");
        // md5("test@example.com")
        assert_eq!(
            u,
            ["https://www.gravatar.com/avatar/55502f40dc8b7c769880b10874abc9d0?s=64&d=404"]
        );
        let gh = urls("1234+octo@users.noreply.github.com");
        assert_eq!(gh[0], "https://avatars.githubusercontent.com/u/1234?s=64");
        assert_eq!(gh.len(), 2);
        assert_eq!(
            urls("octo@users.noreply.github.com")[0],
            "https://github.com/octo.png?size=64"
        );
    }

    #[test]
    fn cached_answers_skip_the_network() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(cache_file(dir.path(), "a@x"), b"png").unwrap();
        std::fs::write(cache_file(dir.path(), "none@x"), b"").unwrap();
        assert_eq!(fetch(dir.path(), "A@x ").unwrap(), Some(b"png".to_vec()));
        assert_eq!(fetch(dir.path(), "none@x").unwrap(), None);
    }

    #[test]
    fn makes_initials() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("linus"), "L");
        assert_eq!(initials("john.q.public"), "JP");
        assert_eq!(initials(""), "?");
    }
}
