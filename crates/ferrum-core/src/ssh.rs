//! SSH keys and known hosts, through the OpenSSH tools (`ssh-keygen`,
//! `ssh-add`, `ssh-keyscan`) that come with git.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKey {
    pub private: PathBuf,
    pub public: PathBuf,
    /// `ssh-ed25519`, `ssh-rsa`…
    pub kind: String,
    /// `SHA256:…`, empty when `ssh-keygen` is not available.
    pub fingerprint: String,
    pub comment: String,
    /// The `.pub` file's text, to paste into a service.
    pub public_key: String,
}

fn tool(name: &str) -> Command {
    let mut cmd = Command::new(name);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn output(mut cmd: Command, what: &str) -> Result<String> {
    let out = cmd
        .output()
        .map_err(|e| Error::Other(format!("could not run {what}: {e}")))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = if err.trim().is_empty() {
            text
        } else {
            err.into_owned()
        };
        Err(Error::Other(format!("{what} failed: {}", msg.trim())))
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `~/.ssh`.
pub fn ssh_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".ssh"))
}

/// Reads a key pair from its private key path (the `.pub` must be beside it).
pub fn read_key(private: &Path) -> Result<SshKey> {
    let public = PathBuf::from(format!("{}.pub", private.display()));
    let text = std::fs::read_to_string(&public)
        .map_err(|e| Error::Other(format!("{}: {e}", public.display())))?;
    let line = text.lines().next().unwrap_or_default().trim().to_owned();
    let mut parts = line.splitn(3, ' ');
    let kind = parts.next().unwrap_or_default().to_owned();
    let _blob = parts.next();
    let comment = parts.next().unwrap_or_default().to_owned();
    if !kind.starts_with("ssh-") && !kind.starts_with("ecdsa-") && !kind.starts_with("sk-") {
        return Err(Error::Other(format!(
            "{} is not an SSH public key",
            public.display()
        )));
    }
    let fingerprint = fingerprint(&public).unwrap_or_default();
    Ok(SshKey {
        private: private.to_path_buf(),
        public,
        kind,
        fingerprint,
        comment,
        public_key: line,
    })
}

/// `SHA256:…` of a public key file (or of `-`'s stdin lines).
fn fingerprint(public: &Path) -> Result<String> {
    let mut cmd = tool("ssh-keygen");
    cmd.arg("-lf").arg(public);
    let out = output(cmd, "ssh-keygen")?;
    Ok(out.split_whitespace().nth(1).unwrap_or_default().to_owned())
}

/// Key pairs in `dir` (private keys with a `.pub` beside them), sorted by name.
pub fn list_keys(dir: &Path) -> Vec<SshKey> {
    let mut keys: Vec<SshKey> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "pub"))
        .filter_map(|p| {
            let private = p.with_extension("");
            private.is_file().then(|| read_key(&private).ok()).flatten()
        })
        .collect();
    keys.sort_by(|a, b| a.private.cmp(&b.private));
    keys
}

/// Creates an ED25519 key pair at `path` (and `path.pub`).
pub fn generate(path: &Path, passphrase: &str, comment: &str) -> Result<SshKey> {
    if path.exists() {
        return Err(Error::Other(format!("{} already exists", path.display())));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        if dir.file_name().is_some_and(|n| n == ".ssh") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut cmd = tool("ssh-keygen");
    cmd.args(["-q", "-t", "ed25519", "-f"])
        .arg(path)
        .args(["-N", passphrase, "-C", comment]);
    output(cmd, "ssh-keygen")?;
    read_key(path)
}

/// Adds a key to the running ssh-agent, which then remembers its passphrase
/// until logout. `env` should carry the askpass variables (the passphrase
/// prompt goes through Ferrum's window).
pub fn add_to_agent(private: &Path, env: &[(String, String)]) -> Result<()> {
    let mut cmd = tool("ssh-add");
    cmd.arg(private);
    for (k, v) in env {
        cmd.env(k, v);
    }
    output(cmd, "ssh-add").map(drop)
}

/// Whether an ssh-agent is reachable (`ssh-add -l` exits 0 or 1).
pub fn agent_running() -> bool {
    let mut cmd = tool("ssh-add");
    cmd.arg("-l").stdout(Stdio::null()).stderr(Stdio::null());
    cmd.status().is_ok_and(|s| matches!(s.code(), Some(0 | 1)))
}

// ----- Known hosts -----

/// The host and port of an ssh remote URL (`git@host:a/b`, `ssh://git@host:2222/a/b`).
pub fn ssh_host(url: &str) -> Option<(String, Option<u16>)> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("ssh://") {
        let auth_host = rest.split('/').next()?;
        let host = auth_host.rsplit_once('@').map_or(auth_host, |(_, h)| h);
        let (host, port) = match host.rsplit_once(':') {
            Some((h, p)) => (h, p.parse().ok()),
            None => (host, None),
        };
        return Some((host.to_owned(), port));
    }
    if url.contains("://") {
        return None;
    }
    let (left, _) = url.split_once(':')?;
    let host = left.rsplit_once('@').map_or(left, |(_, h)| h);
    (!host.contains('/') && host.len() > 1).then(|| (host.to_owned(), None))
}

/// Whether git failed because the server's host key is not trusted yet.
pub fn is_unknown_host_error(text: &str) -> bool {
    text.contains("Host key verification failed")
        || text.contains("authenticity of host")
        || text.contains("No ED25519 host key is known")
        || (text.contains("host key is known") && text.contains("strict checking"))
}

/// The host named in an ssh error ("authenticity of host 'h (1.2.3.4)'",
/// "host key is known for h and").
pub fn host_in_error(text: &str) -> Option<String> {
    if let Some(rest) = text.split("authenticity of host '").nth(1) {
        let host = rest.split(['\'', ' ']).next()?;
        return Some(strip_brackets(host));
    }
    let rest = text.split(" host key is known for ").nth(1)?;
    Some(strip_brackets(rest.split_whitespace().next()?))
}

/// `[host]:2222` → `host`.
fn strip_brackets(h: &str) -> String {
    let h = h.trim_start_matches('[');
    h.split(']').next().unwrap_or(h).to_owned()
}

/// Keys a server offers, read with `ssh-keyscan`.
#[derive(Debug, Clone)]
pub struct HostKeys {
    pub host: String,
    /// Lines for `known_hosts`.
    pub lines: String,
    /// (key type, `SHA256:…`) for each line, to compare with what the service publishes.
    pub fingerprints: Vec<(String, String)>,
}

pub fn scan_host(host: &str, port: Option<u16>) -> Result<HostKeys> {
    let mut cmd = tool("ssh-keyscan");
    if let Some(p) = port {
        cmd.args(["-p", &p.to_string()]);
    }
    cmd.args(["-T", "10", host]);
    let lines = output(cmd, "ssh-keyscan")?;
    let lines: String = lines
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| format!("{l}\n"))
        .collect();
    if lines.is_empty() {
        return Err(Error::Other(format!("{host} sent no host keys")));
    }
    let fingerprints = fingerprints_of(&lines)?;
    Ok(HostKeys {
        host: host.to_owned(),
        lines,
        fingerprints,
    })
}

/// Fingerprints of `known_hosts` lines (`ssh-keygen -lf -`).
pub fn fingerprints_of(lines: &str) -> Result<Vec<(String, String)>> {
    let mut cmd = tool("ssh-keygen");
    cmd.args(["-lf", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Other(format!("could not run ssh-keygen: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(lines.as_bytes());
    }
    let out = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // "256 SHA256:abc host (ED25519)"
    Ok(text
        .lines()
        .filter_map(|l| {
            let fp = l.split_whitespace().nth(1)?.to_owned();
            let kind = l.rsplit_once('(')?.1.trim_end_matches(')').to_owned();
            Some((kind, fp))
        })
        .collect())
}

/// Appends host keys to a `known_hosts` file.
pub fn trust_host(known_hosts: &Path, lines: &str) -> Result<()> {
    if let Some(dir) = known_hosts.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existing = std::fs::read_to_string(known_hosts).unwrap_or_default();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(known_hosts)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        f.write_all(b"\n")?;
    }
    for line in lines.lines().filter(|l| !existing.contains(l.trim())) {
        writeln!(f, "{line}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_keygen() -> bool {
        tool("ssh-keygen")
            .arg("-?")
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .is_ok()
    }

    #[test]
    fn generates_and_lists_keys() {
        if !has_keygen() {
            eprintln!("ssh-keygen not found; skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys").join("id_test");
        let key = generate(&path, "secret phrase", "me@laptop").unwrap();
        assert_eq!(key.kind, "ssh-ed25519");
        assert_eq!(key.comment, "me@laptop");
        assert!(
            key.fingerprint.starts_with("SHA256:"),
            "{}",
            key.fingerprint
        );
        assert!(generate(&path, "", "x").is_err(), "never overwrites");
        generate(&dir.path().join("keys").join("id_other"), "", "").unwrap();
        // A lone .pub without its private key is not listed.
        std::fs::write(dir.path().join("keys/lonely.pub"), &key.public_key).unwrap();
        let names: Vec<_> = list_keys(&dir.path().join("keys"))
            .into_iter()
            .map(|k| {
                k.private
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["id_other", "id_test"]);

        // Fingerprints of known_hosts lines.
        let line = format!("example.com {}\n", key.public_key);
        let fps = fingerprints_of(&line).unwrap();
        assert_eq!(fps, [("ED25519".to_owned(), key.fingerprint.clone())]);
        let kh = dir.path().join("known_hosts");
        trust_host(&kh, &line).unwrap();
        trust_host(&kh, &line).unwrap();
        assert_eq!(std::fs::read_to_string(&kh).unwrap(), line, "added once");
    }

    #[test]
    fn reads_hosts() {
        assert_eq!(
            ssh_host("git@github.com:a/b.git"),
            Some(("github.com".into(), None))
        );
        assert_eq!(
            ssh_host("ssh://git@bb.corp:7999/p/r.git"),
            Some(("bb.corp".into(), Some(7999)))
        );
        assert_eq!(ssh_host("https://github.com/a/b"), None);
        assert_eq!(ssh_host("/local/path"), None);
        let err = "The authenticity of host 'gitlab.com (172.65.251.78)' can't be established.";
        assert!(is_unknown_host_error(err));
        assert_eq!(host_in_error(err).as_deref(), Some("gitlab.com"));
        let err = "No ED25519 host key is known for [bb.corp]:7999 and you have requested strict checking.\nHost key verification failed.";
        assert!(is_unknown_host_error(err));
        assert_eq!(host_in_error(err).as_deref(), Some("bb.corp"));
        assert!(!is_unknown_host_error("Permission denied (publickey)."));
    }
}
