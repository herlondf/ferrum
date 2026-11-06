//! Runs programs other than git (custom commands), recorded in the same
//! command log and stopped by the same cancel token as a [`Git`].

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use crate::cli::{CommandRecord, Git, Stream};
use crate::{Error, Result};

const MAX_LOG_LINES: usize = 2000;

/// What to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// A script for the system shell (`sh -c` or `cmd /C`).
    Shell(String),
    /// A program (relative paths start in the repository) with its arguments.
    Exec { program: String, args: Vec<String> },
}

impl Program {
    /// The command line shown in the activity log.
    pub fn display(&self) -> String {
        match self {
            Program::Shell(script) => script.trim().to_owned(),
            Program::Exec { program, args } => {
                let mut s = quote_display(program);
                for a in args {
                    s.push(' ');
                    s.push_str(&quote_display(a));
                }
                s
            }
        }
    }

    fn command(&self, dir: &Path) -> Command {
        match self {
            Program::Shell(script) => shell_command(script),
            Program::Exec { program, args } => {
                let p = Path::new(program);
                let mut cmd = if p.is_relative() && program.contains(['/', '\\']) {
                    Command::new(dir.join(p))
                } else {
                    Command::new(program)
                };
                cmd.args(args);
                cmd
            }
        }
    }
}

#[cfg(windows)]
fn shell_command(script: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new("cmd");
    // Passed as typed: cmd has its own quoting rules.
    cmd.raw_arg(format!("/D /S /C \"{script}\""));
    cmd
}

#[cfg(not(windows))]
fn shell_command(script: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);
    cmd
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

fn quote_display(s: &str) -> String {
    if s.is_empty() || s.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_owned()
    }
}

/// Runs `program` in the repository of `git`, with its log and cancel token.
/// `progress` gets each output line. Returns stdout.
pub fn run(git: &Git, program: &Program, mut progress: impl FnMut(&str)) -> Result<String> {
    let cancel = git.cancel_token().cloned();
    let cancelled = || cancel.as_ref().is_some_and(|c| c.is_cancelled());
    if cancelled() {
        return Err(Error::Cancelled);
    }
    let mut cmd = program.command(git.dir());
    cmd.current_dir(git.dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut cmd);
    let record = start_record(git, program);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("{}: {e}", program.display());
            update(git, record, |r| {
                r.output.push((Stream::Stderr, msg.clone()));
                r.finished = Some(SystemTime::now());
            });
            return Err(Error::Cli(msg));
        }
    };

    let (tx, rx) = mpsc::channel::<(Stream, String)>();
    let readers = [
        reader(child.stdout.take().map(|s| Box::new(s) as _), Stream::Stdout, tx.clone()),
        reader(child.stderr.take().map(|s| Box::new(s) as _), Stream::Stderr, tx),
    ];
    let (mut out, mut err) = (String::new(), String::new());
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((stream, line)) => {
                progress(&line);
                let all = if stream == Stream::Stdout {
                    &mut out
                } else {
                    &mut err
                };
                all.push_str(&line);
                all.push('\n');
                update(git, record, |r| {
                    if r.output.len() < MAX_LOG_LINES {
                        r.output.push((stream, line));
                    }
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if cancelled() {
            return Err(kill(git, record, &mut child));
        }
    }
    for r in readers {
        let _ = r.join();
    }
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if cancelled() {
            return Err(kill(git, record, &mut child));
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    update(git, record, |r| {
        r.exit = status.code();
        r.finished = Some(SystemTime::now());
    });
    if status.success() {
        return Ok(out);
    }
    let mut msg = err.trim().to_owned();
    if msg.is_empty() {
        msg = out.trim().to_owned();
    }
    let code = status
        .code()
        .map_or_else(|| "a signal".to_owned(), |c| format!("code {c}"));
    Err(Error::Cli(if msg.is_empty() {
        format!("The command exited with {code}")
    } else {
        format!("The command exited with {code}:\n{msg}")
    }))
}

type Pipe = Box<dyn Read + Send>;

fn reader(
    pipe: Option<Pipe>,
    stream: Stream,
    tx: mpsc::Sender<(Stream, String)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Some(pipe) = pipe else { return };
        let mut buf = Vec::new();
        let mut r = BufReader::new(pipe);
        while r.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
            let line = String::from_utf8_lossy(&buf)
                .trim_end_matches(['\n', '\r'])
                .to_owned();
            buf.clear();
            if tx.send((stream, line)).is_err() {
                break;
            }
        }
    })
}

fn kill(git: &Git, record: Option<usize>, child: &mut Child) -> Error {
    let _ = child.kill();
    let _ = child.wait();
    update(git, record, |r| {
        r.cancelled = true;
        r.finished = Some(SystemTime::now());
    });
    Error::Cancelled
}

fn start_record(git: &Git, program: &Program) -> Option<usize> {
    let mut log = git.log()?.lock().ok()?;
    log.push(CommandRecord {
        command: program.display(),
        dir: git.dir().to_path_buf(),
        started: SystemTime::now(),
        finished: None,
        exit: None,
        cancelled: false,
        output: Vec::new(),
    });
    Some(log.len() - 1)
}

fn update(git: &Git, record: Option<usize>, f: impl FnOnce(&mut CommandRecord)) {
    if let (Some(i), Some(log)) = (record, git.log())
        && let Ok(mut log) = log.lock()
        && let Some(r) = log.get_mut(i)
    {
        f(r);
    }
}

/// Splits a command line into words: spaces separate them, '…' and "…"
/// group, and a backslash escapes a space, a quote or a backslash.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, c) if c.is_whitespace() => {
                if has_word {
                    out.push(std::mem::take(&mut cur));
                    has_word = false;
                }
            }
            (None, '\'' | '"') => {
                quote = Some(c);
                has_word = true;
            }
            (Some(q), c) if c == q => quote = None,
            (q, '\\')
                if q != Some('\'')
                    && chars
                        .peek()
                        .is_some_and(|n| matches!(n, ' ' | '"' | '\'' | '\\')) =>
            {
                cur.push(chars.next().unwrap_or('\\'));
                has_word = true;
            }
            (_, c) => {
                cur.push(c);
                has_word = true;
            }
        }
    }
    if has_word {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{CancelToken, CommandLog};
    use crate::testutil::TestRepo;

    #[test]
    fn splits_words_and_quotes() {
        assert_eq!(split_args("a  b"), ["a", "b"]);
        assert_eq!(split_args(r#"x "a b" 'c d' e\ f"#), ["x", "a b", "c d", "e f"]);
        assert_eq!(split_args(r"C:\tools\run.exe --x"), [r"C:\tools\run.exe", "--x"]);
        assert_eq!(split_args(r#"say "" end"#), ["say", "", "end"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn runs_a_script_and_records_it() {
        let repo = TestRepo::new();
        let log = CommandLog::default();
        let git = repo.git.clone().with_log(log.clone());
        let mut lines = Vec::new();
        let out = run(&git, &Program::Shell("echo hello".into()), |l| {
            lines.push(l.to_owned())
        })
        .unwrap();
        assert_eq!(out.trim(), "hello");
        assert_eq!(lines.iter().map(|l| l.trim()).collect::<Vec<_>>(), ["hello"]);
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].command, "echo hello");
        assert_eq!(log[0].exit, Some(0));
        assert!(log[0].finished.is_some());
    }

    #[test]
    fn runs_in_the_repository() {
        let repo = TestRepo::new();
        repo.write("marker.txt", "here\n");
        let script = if cfg!(windows) {
            "type marker.txt"
        } else {
            "cat marker.txt"
        };
        let out = run(&repo.git, &Program::Shell(script.into()), |_| {}).unwrap();
        assert_eq!(out.trim(), "here");
    }

    #[test]
    fn reports_failures() {
        let repo = TestRepo::new();
        let err = run(&repo.git, &Program::Shell("exit 3".into()), |_| {}).unwrap_err();
        assert!(err.to_string().contains("code 3"), "{err}");
        let missing = Program::Exec {
            program: "ferrum-no-such-program".into(),
            args: vec![],
        };
        assert!(run(&repo.git, &missing, |_| {}).is_err());
    }

    #[test]
    fn cancelled_before_start() {
        let repo = TestRepo::new();
        let cancel = CancelToken::new();
        cancel.cancel();
        let git = repo.git.clone().with_cancel(cancel);
        let err = run(&git, &Program::Shell("echo no".into()), |_| {}).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }

    #[test]
    fn display_quotes_arguments() {
        let p = Program::Exec {
            program: "tool".into(),
            args: vec!["a b".into(), "c".into()],
        };
        assert_eq!(p.display(), "tool \"a b\" c");
    }
}
