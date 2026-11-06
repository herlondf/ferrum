//! Password and passphrase prompts from git and ssh, answered in Ferrum's
//! window.
//!
//! Git and ssh run `GIT_ASKPASS` / `SSH_ASKPASS` with the prompt as the only
//! argument and read the answer from its output. Ferrum points both at its
//! own executable with `FERRUM_ASKPASS=port:secret`; that process (see
//! [`client`]) connects to the [`Server`] running in the app on localhost,
//! sends the prompt, waits for the user and prints the answer. The secret
//! keeps other local programs from asking for the user's passwords.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};

use serde_json::{Value, json};

/// Environment variable that puts the executable in askpass mode.
pub const ENV: &str = "FERRUM_ASKPASS";

/// A prompt waiting for the user.
pub struct Request {
    pub prompt: String,
    reply: Sender<Option<String>>,
}

impl Request {
    /// A request not tied to a connection, and where its answer arrives
    /// (for tests and previews).
    pub fn new(prompt: &str) -> (Request, Receiver<Option<String>>) {
        let (reply, rx) = mpsc::channel();
        let prompt = prompt.to_owned();
        (Request { prompt, reply }, rx)
    }

    /// Sends the answer back (`None` cancels; git then fails).
    pub fn answer(self, answer: Option<String>) {
        let _ = self.reply.send(answer);
    }
}

pub struct Server {
    port: u16,
    secret: String,
    rx: Receiver<Request>,
}

/// A random hex string, from the standard library's OS-seeded hasher keys.
fn random_secret() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

impl Server {
    /// Listens on a free localhost port. `notify` runs when a prompt arrives
    /// (to wake the UI).
    pub fn start(notify: impl Fn() + Send + Sync + 'static) -> std::io::Result<Server> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let secret = random_secret();
        let (tx, rx) = mpsc::channel();
        let expected = secret.clone();
        let notify = std::sync::Arc::new(notify);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (tx, expected, notify) = (tx.clone(), expected.clone(), notify.clone());
                std::thread::spawn(move || {
                    let _ = serve(stream, &expected, &tx, &*notify);
                });
            }
        });
        Ok(Server { port, secret, rx })
    }

    /// Variables for git commands so their prompts come to this server.
    pub fn env(&self, exe: &Path) -> Vec<(String, String)> {
        let exe = exe.to_string_lossy().into_owned();
        vec![
            ("GIT_ASKPASS".into(), exe.clone()),
            ("SSH_ASKPASS".into(), exe),
            // OpenSSH 8.4+: use askpass even without a display or with a terminal.
            ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
            (ENV.into(), format!("{}:{}", self.port, self.secret)),
        ]
    }

    pub fn try_recv(&self) -> Option<Request> {
        self.rx.try_recv().ok()
    }
}

fn serve(
    stream: TcpStream,
    secret: &str,
    tx: &Sender<Request>,
    notify: &dyn Fn(),
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let msg: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    if msg.get("secret").and_then(Value::as_str) != Some(secret) {
        return Ok(());
    }
    let prompt = msg
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let (reply, wait) = mpsc::channel();
    if tx.send(Request { prompt, reply }).is_err() {
        return Ok(());
    }
    notify();
    // Dropped without an answer counts as cancel.
    let answer = wait.recv().ok().flatten();
    let out = match answer {
        Some(a) => json!({"answer": a}),
        None => json!({"cancel": true}),
    };
    let mut stream = stream;
    writeln!(stream, "{out}")
}

/// Sends `prompt` to the app at `addr` and waits for the answer.
pub fn ask(addr: &str, secret: &str, prompt: &str) -> std::io::Result<Option<String>> {
    let mut stream = TcpStream::connect(addr)?;
    writeln!(stream, "{}", json!({"secret": secret, "prompt": prompt}))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let reply: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    Ok(reply
        .get("answer")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// The askpass process: asks the app, prints the answer, returns the exit
/// code (1 when cancelled or the app cannot be reached).
pub fn client(prompt: &str) -> i32 {
    let Some(spec) = std::env::var(ENV).ok() else {
        return 1;
    };
    let Some((port, secret)) = spec.split_once(':') else {
        return 1;
    };
    match ask(&format!("127.0.0.1:{port}"), secret, prompt) {
        Ok(Some(answer)) => {
            println!("{answer}");
            0
        }
        _ => 1,
    }
}

/// What kind of answer a prompt wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// User names: shown while typing.
    Visible,
    /// Passwords and passphrases.
    Secret,
    /// ssh's "Are you sure you want to continue connecting (yes/no)?".
    Confirm,
}

pub fn prompt_kind(prompt: &str) -> PromptKind {
    let p = prompt.to_lowercase();
    if p.contains("(yes/no") || p.contains("continue connecting") {
        PromptKind::Confirm
    } else if p.starts_with("username") || p.contains("username for") {
        PromptKind::Visible
    } else {
        PromptKind::Secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let server = Server::start(|| {}).unwrap();
        let env = server.env(Path::new("/bin/ferrum"));
        let spec = &env.iter().find(|(k, _)| k == ENV).unwrap().1;
        let (port, secret) = spec.split_once(':').unwrap();
        let addr = format!("127.0.0.1:{port}");

        let (a, s) = (addr.clone(), secret.to_owned());
        let asked = std::thread::spawn(move || ask(&a, &s, "Password for 'https://h': "));
        let req = loop {
            if let Some(r) = server.try_recv() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(req.prompt, "Password for 'https://h': ");
        req.answer(Some("hunter2".into()));
        assert_eq!(asked.join().unwrap().unwrap().as_deref(), Some("hunter2"));

        // Cancelled.
        let (a, s) = (addr.clone(), secret.to_owned());
        let asked = std::thread::spawn(move || ask(&a, &s, "Username: "));
        let req = loop {
            if let Some(r) = server.try_recv() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        drop(req);
        assert_eq!(asked.join().unwrap().unwrap(), None);

        // A wrong secret gets nothing, and no prompt reaches the app.
        assert_eq!(ask(&addr, "wrong", "Password: ").unwrap(), None);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(server.try_recv().is_none());
    }

    /// Git really asks through the helper: a script stands in for Ferrum's
    /// executable and answers with a fixed password.
    #[cfg(unix)]
    #[test]
    fn git_uses_the_helper() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("askpass.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho \"$1\" >> \"$(dirname \"$0\")/asked\"\necho secret\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let git = crate::cli::Git::new(dir.path())
            .with_env("GIT_ASKPASS", script.to_string_lossy())
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("HOME", dir.path().to_string_lossy());
        let out = git
            .run_with_input(
                &["credential", "fill"],
                Some(b"protocol=https\nhost=example.com\n\n"),
            )
            .unwrap();
        assert!(out.contains("password=secret"), "{out}");
        let asked = std::fs::read_to_string(dir.path().join("asked")).unwrap();
        assert!(
            asked.contains("Username for 'https://example.com'"),
            "{asked}"
        );
        assert_eq!(
            prompt_kind(asked.lines().next().unwrap()),
            PromptKind::Visible
        );
    }

    #[test]
    fn classifies_prompts() {
        assert_eq!(
            prompt_kind("Enter passphrase for key '/home/a/.ssh/id_ed25519': "),
            PromptKind::Secret
        );
        assert_eq!(
            prompt_kind("Are you sure you want to continue connecting (yes/no/[fingerprint])? "),
            PromptKind::Confirm
        );
        assert_eq!(
            prompt_kind("Password for 'https://a@h': "),
            PromptKind::Secret
        );
    }
}
