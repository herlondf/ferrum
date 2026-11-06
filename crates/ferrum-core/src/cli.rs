//! Runs the `git` command line.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

use crate::{Error, Result};

/// The git program used by new `Git` instances (`None` = `git` on the PATH).
static PROGRAM: RwLock<Option<PathBuf>> = RwLock::new(None);
static VERBOSE: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<Vec<LogEntry>> = Mutex::new(Vec::new());
const LOG_LEN: usize = 200;
const LOG_OUTPUT_LEN: usize = 4000;

/// Sets the git executable used from now on (`None` or empty = `git` on the PATH).
pub fn set_program(path: Option<PathBuf>) {
    let path = path.filter(|p| !p.as_os_str().is_empty());
    if let Ok(mut p) = PROGRAM.write() {
        *p = path;
    }
}

/// The git executable new `Git` instances use.
pub fn program() -> PathBuf {
    PROGRAM
        .read()
        .ok()
        .and_then(|p| p.clone())
        .unwrap_or_else(|| PathBuf::from("git"))
}

/// Verbose mode: every command and its output is kept in `recent_log`, and
/// errors start with the command that failed.
pub fn set_verbose(on: bool) {
    VERBOSE.store(on, Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// One git command run in verbose mode.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub dir: PathBuf,
    /// The command line, e.g. `git pull --rebase`.
    pub command: String,
    pub success: bool,
    /// stdout and stderr, shortened.
    pub output: String,
}

/// The latest commands run in verbose mode, oldest first.
pub fn recent_log() -> Vec<LogEntry> {
    LOG.lock().map(|l| l.clone()).unwrap_or_default()
}

pub fn clear_log() {
    if let Ok(mut l) = LOG.lock() {
        l.clear();
    }
}

/// `git --version` of the configured program, e.g. "2.47.1".
pub fn version() -> Option<String> {
    let mut cmd = Command::new(program());
    cmd.arg("--version").stdin(Stdio::null());
    no_window(&mut cmd);
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_start_matches("git version ").to_owned())
}

#[derive(Debug, Clone)]
pub struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
    program: PathBuf,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            env: Vec::new(),
            program: program(),
        }
    }

    /// Uses another git executable for this instance.
    pub fn with_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.program = program.into();
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Adds an environment variable to every command run by this instance.
    pub fn with_env(mut self, key: &str, value: impl Into<String>) -> Self {
        self.env.push((key.to_owned(), value.into()));
        self
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.current_dir(&self.dir)
            // Never wait for a password on a terminal that does not exist.
            .env("GIT_TERMINAL_PROMPT", "0")
            // Commands that would open an editor keep the default message instead.
            .args(["-c", "core.editor=true", "-c", "core.quotepath=false"])
            .args(args)
            .stdin(Stdio::null());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        no_window(&mut cmd);
        cmd
    }

    /// Keeps the command in the verbose log.
    fn record(&self, args: &[&str], success: bool, stdout: &[u8], stderr: &[u8]) {
        if !verbose() {
            return;
        }
        let mut output = String::from_utf8_lossy(stdout).trim_end().to_owned();
        let err = String::from_utf8_lossy(stderr);
        if !err.trim().is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(err.trim_end());
        }
        if output.len() > LOG_OUTPUT_LEN {
            let mut end = LOG_OUTPUT_LEN;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
            output.push_str("\n…");
        }
        let entry = LogEntry {
            dir: self.dir.clone(),
            command: command_line(args),
            success,
            output,
        };
        if let Ok(mut l) = LOG.lock() {
            if l.len() >= LOG_LEN {
                l.remove(0);
            }
            l.push(entry);
        }
    }

    /// Runs git and returns stdout. A non-zero exit becomes an error with stderr.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.run_with_input(args, None)
    }

    pub fn run_with_input(&self, args: &[&str], input: Option<&[u8]>) -> Result<String> {
        let mut cmd = self.command(args);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = spawn(cmd, args)?;
        if let Some(input) = input {
            // Write in a thread so a large patch cannot deadlock against a full stdout pipe.
            let mut stdin = child.stdin.take().expect("stdin is piped");
            let data = input.to_vec();
            std::thread::spawn(move || stdin.write_all(&data));
        }
        let out = child.wait_with_output()?;
        self.record(args, out.status.success(), &out.stdout, &out.stderr);
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(failure(args, &out.stdout, &out.stderr))
        }
    }

    /// Runs git, calling `progress` with each line (or `\r`-separated update) of
    /// stderr, where git reports progress for fetch, pull and push.
    pub fn run_streaming(&self, args: &[&str], mut progress: impl FnMut(&str)) -> Result<String> {
        let mut cmd = self.command(args);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = spawn(cmd, args)?;

        let mut stdout = child.stdout.take().expect("stdout is piped");
        let out_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });

        let mut stderr = child.stderr.take().expect("stderr is piped");
        let mut all_err = Vec::new();
        let mut line = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stderr.read(&mut buf)?;
            if n == 0 {
                break;
            }
            for &b in &buf[..n] {
                all_err.push(b);
                if b == b'\n' || b == b'\r' {
                    if !line.is_empty() {
                        progress(&String::from_utf8_lossy(&line));
                        line.clear();
                    }
                } else {
                    line.push(b);
                }
            }
        }
        if !line.is_empty() {
            progress(&String::from_utf8_lossy(&line));
        }
        let status = child.wait()?;
        let out = out_thread.join().unwrap_or_default();
        self.record(args, status.success(), &out, &all_err);
        if status.success() {
            Ok(String::from_utf8_lossy(&out).into_owned())
        } else {
            Err(failure(args, &out, &all_err))
        }
    }
}

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// `git` and the arguments, quoting those with spaces.
fn command_line(args: &[&str]) -> String {
    let mut line = String::from("git");
    for a in args {
        line.push(' ');
        if a.is_empty() || a.contains(char::is_whitespace) {
            line.push_str(&format!("\"{a}\""));
        } else {
            line.push_str(a);
        }
    }
    line
}

fn spawn(mut cmd: Command, args: &[&str]) -> Result<std::process::Child> {
    let program = cmd.get_program().to_owned();
    cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound && program == "git" {
            Error::Cli("git was not found. Install Git and make sure it is on the PATH.".into())
        } else if e.kind() == std::io::ErrorKind::NotFound {
            let program = Path::new(&program).display().to_string();
            Error::Cli(format!(
                "{program} was not found. Check the git executable in Settings."
            ))
        } else {
            Error::Cli(format!("git {}: {e}", args.join(" ")))
        }
    })
}

fn failure(args: &[&str], stdout: &[u8], stderr: &[u8]) -> Error {
    let mut msg = String::from_utf8_lossy(stderr).trim().to_owned();
    // Some commands (merge, commit) explain the failure on stdout.
    let out = String::from_utf8_lossy(stdout);
    if !out.trim().is_empty() {
        if !msg.is_empty() {
            msg.push('\n');
        }
        msg.push_str(out.trim());
    }
    if msg.is_empty() {
        msg = format!("git {} failed", args.first().unwrap_or(&""));
    }
    if verbose() {
        msg = format!("$ {}\n{msg}", command_line(args));
    }
    Error::Cli(msg)
}

#[cfg(test)]
mod tests {
    use crate::testutil::TestRepo;

    #[test]
    fn reports_errors_and_streams_lines() {
        let t = TestRepo::new();
        let err = t.git.run(&["switch", "does-not-exist"]).unwrap_err();
        assert!(!err.to_string().is_empty());

        t.commit("a.txt", "a\n", "first");
        let mut lines = Vec::new();
        let out = t
            .git
            .run_streaming(&["log", "--oneline"], |l| lines.push(l.to_owned()))
            .unwrap();
        assert!(out.contains("first"));
    }

    #[test]
    fn uses_the_configured_program() {
        let t = TestRepo::new();
        let missing = t.git.clone().with_program("/no/such/git");
        let err = missing.run(&["status"]).unwrap_err().to_string();
        assert!(err.contains("/no/such/git"), "{err}");
        assert!(t.git.run(&["status"]).is_ok());
    }

    #[test]
    fn verbose_mode_keeps_a_log() {
        let t = TestRepo::new();
        super::set_verbose(true);
        let _ = t.git.run(&["rev-parse", "--is-inside-work-tree"]);
        let err = t.git.run(&["switch", "nope nope"]).unwrap_err().to_string();
        super::set_verbose(false);
        assert!(err.starts_with("$ git switch \"nope nope\""), "{err}");
        let log = super::recent_log();
        let entry = log
            .iter()
            .rev()
            .find(|e| e.dir == t.path() && e.command == "git rev-parse --is-inside-work-tree")
            .expect("logged");
        assert!(entry.success);
        assert_eq!(entry.output, "true");
    }

    #[test]
    fn passes_stdin() {
        let t = TestRepo::new();
        let out = t
            .git
            .run_with_input(&["hash-object", "--stdin"], Some(b"hello\n"))
            .unwrap();
        assert_eq!(out.trim(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }
}
