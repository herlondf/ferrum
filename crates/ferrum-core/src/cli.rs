//! Runs the `git` command line.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, mpsc};
use std::time::{Duration, SystemTime};

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
    log: Option<CommandLog>,
    cancel: Option<CancelToken>,
    program: PathBuf,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            env: Vec::new(),
            log: None,
            cancel: None,
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

    /// Records every command run by this instance in `log`.
    pub fn with_log(mut self, log: CommandLog) -> Self {
        self.log = Some(log);
        self
    }

    /// Kills running commands, and refuses to start new ones, once `cancel` is set.
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// The cancel token set with [`Git::with_cancel`].
    pub fn cancel_token(&self) -> Option<&CancelToken> {
        self.cancel.as_ref()
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
        self.exec(args, None, None)
    }

    pub fn run_with_input(&self, args: &[&str], input: Option<&[u8]>) -> Result<String> {
        self.exec(args, input, None)
    }

    /// Runs git, calling `progress` with each line (or `\r`-separated update) of
    /// stderr, where git reports progress for fetch, pull and push.
    pub fn run_streaming(&self, args: &[&str], mut progress: impl FnMut(&str)) -> Result<String> {
        self.exec(args, None, Some(&mut progress))
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(CancelToken::is_cancelled)
    }

    fn exec(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
        mut progress: Option<&mut dyn FnMut(&str)>,
    ) -> Result<String> {
        if self.cancelled() {
            return Err(Error::Cancelled);
        }
        let mut cmd = self.command(args);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let record = self.log_start(args);
        let mut child = match spawn(cmd, args) {
            Ok(c) => c,
            Err(e) => {
                self.log_update(record, |r| {
                    r.output.push((Stream::Stderr, e.to_string()));
                    r.finished = Some(SystemTime::now());
                });
                return Err(e);
            }
        };
        if let Some(input) = input {
            // Write in a thread so a large patch cannot deadlock against a full stdout pipe.
            let mut stdin = child.stdin.take().expect("stdin is piped");
            let data = input.to_vec();
            std::thread::spawn(move || stdin.write_all(&data));
        }

        let mut stdout = child.stdout.take().expect("stdout is piped");
        let out_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });

        // Stderr is read in a thread and sent line by line, so this thread can
        // watch for cancellation while git runs.
        let mut stderr = child.stderr.take().expect("stderr is piped");
        let (tx, rx) = mpsc::channel::<(String, bool)>();
        let err_thread = std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut line = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    all.push(b);
                    if b == b'\n' || b == b'\r' {
                        if !line.is_empty() {
                            let text = String::from_utf8_lossy(&line).into_owned();
                            let _ = tx.send((text, b == b'\r'));
                            line.clear();
                        }
                    } else {
                        line.push(b);
                    }
                }
            }
            if !line.is_empty() {
                let _ = tx.send((String::from_utf8_lossy(&line).into_owned(), false));
            }
            all
        });

        // A `\r` line is a progress update that the next line replaces.
        let mut replace_last = false;
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok((line, cr)) => {
                    if let Some(p) = progress.as_mut() {
                        p(&line);
                    }
                    self.log_update(record, |r| {
                        if replace_last
                            && let Some(last) = r.output.last_mut()
                            && last.0 == Stream::Stderr
                        {
                            last.1 = line;
                        } else if r.output.len() < MAX_LOG_LINES {
                            r.output.push((Stream::Stderr, line));
                        }
                    });
                    replace_last = cr;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if self.cancelled() {
                return Err(self.kill(&mut child, record));
            }
        }
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if self.cancelled() {
                return Err(self.kill(&mut child, record));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let out = out_thread.join().unwrap_or_default();
        let err = err_thread.join().unwrap_or_default();
        self.log_update(record, |r| {
            let text = String::from_utf8_lossy(&out);
            let text = text.trim_end();
            if !text.is_empty() {
                r.output
                    .push((Stream::Stdout, truncate(text, MAX_LOG_STDOUT)));
            }
            r.exit = status.code();
            r.finished = Some(SystemTime::now());
        });
        self.record(args, status.success(), &out, &err);
        if status.success() {
            Ok(String::from_utf8_lossy(&out).into_owned())
        } else {
            Err(failure(args, &out, &err))
        }
    }

    /// Stops a cancelled command. Its reader threads end when the pipes close.
    fn kill(&self, child: &mut std::process::Child, record: Option<usize>) -> Error {
        let _ = child.kill();
        let _ = child.wait();
        self.log_update(record, |r| {
            r.cancelled = true;
            r.finished = Some(SystemTime::now());
        });
        Error::Cancelled
    }

    fn log_start(&self, args: &[&str]) -> Option<usize> {
        let mut log = self.log.as_ref()?.lock().ok()?;
        log.push(CommandRecord {
            command: command_line(args),
            dir: self.dir.clone(),
            started: SystemTime::now(),
            finished: None,
            exit: None,
            cancelled: false,
            output: Vec::new(),
        });
        Some(log.len() - 1)
    }

    fn log_update(&self, record: Option<usize>, f: impl FnOnce(&mut CommandRecord)) {
        if let (Some(i), Some(log)) = (record, &self.log)
            && let Ok(mut log) = log.lock()
            && let Some(r) = log.get_mut(i)
        {
            f(r);
        }
    }
}

const MAX_LOG_LINES: usize = 2000;
const MAX_LOG_STDOUT: usize = 64 * 1024;

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… ({} more bytes)", &text[..cut], text.len() - cut)
}

/// Set it to stop the commands of a `Git` made `with_cancel`.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// One git command, as recorded in a `CommandLog`.
#[derive(Debug, Clone)]
pub struct CommandRecord {
    /// The command line, e.g. `git fetch --prune origin`.
    pub command: String,
    pub dir: PathBuf,
    pub started: SystemTime,
    /// `None` while it runs.
    pub finished: Option<SystemTime>,
    /// Exit code (`None` while running, when killed or when git did not start).
    pub exit: Option<i32>,
    pub cancelled: bool,
    /// Stderr lines as they came, then stdout.
    pub output: Vec<(Stream, String)>,
}

/// Commands recorded by a `Git` made `with_log`; updated while they run.
pub type CommandLog = Arc<Mutex<Vec<CommandRecord>>>;

/// `git` and the arguments, quoted where needed.
pub fn command_line(args: &[&str]) -> String {
    let mut s = String::from("git");
    for a in args {
        s.push(' ');
        if a.is_empty() || a.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
            s.push('"');
            s.push_str(&a.replace('"', "\\\""));
            s.push('"');
        } else {
            s.push_str(a);
        }
    }
    s
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
    use super::*;
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

    #[test]
    fn records_commands_in_the_log() {
        let t = TestRepo::new();
        t.commit("a.txt", "a\n", "first");
        let log = CommandLog::default();
        let git = t.git.clone().with_log(log.clone());
        git.run(&["log", "--format=%s"]).unwrap();
        git.run(&["switch", "nope"]).unwrap_err();
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].command, "git log --format=%s");
        assert_eq!(log[0].exit, Some(0));
        assert!(log[0].finished.is_some());
        assert_eq!(log[0].output, vec![(Stream::Stdout, "first".to_owned())]);
        assert_ne!(log[1].exit, Some(0));
        assert!(log[1].output.iter().any(|(s, _)| *s == Stream::Stderr));
    }

    #[test]
    fn quotes_arguments_with_spaces() {
        assert_eq!(
            command_line(&["commit", "-m", "two words"]),
            "git commit -m \"two words\""
        );
    }

    #[test]
    fn cancel_kills_the_running_command() {
        let t = TestRepo::new();
        // An alias that runs a shell command which waits long enough to be killed.
        t.git.run(&["config", "alias.wait", "!sleep 30"]).unwrap();
        let cancel = CancelToken::new();
        let git = t.git.clone().with_cancel(cancel.clone());
        let started = std::time::Instant::now();
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c.cancel();
        });
        let err = git.run_streaming(&["wait"], |_| {}).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(10));
        // A cancelled instance does not start new commands.
        assert!(matches!(git.run(&["status"]), Err(Error::Cancelled)));
    }
}
