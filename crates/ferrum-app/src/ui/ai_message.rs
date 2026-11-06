//! Commit messages written by an AI command line tool (Claude Code, Codex...)
//! from the staged diff. Runs on its own thread and can be cancelled.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use eframe::egui;
use ferrum_core::cli::Git;

use crate::commit_prefs::AiPrefs;

/// Larger diffs are cut, so the prompt stays a reasonable size.
const MAX_DIFF: usize = 60_000;

pub struct AiTask {
    rx: Receiver<Result<String, String>>,
    cancel: Arc<AtomicBool>,
}

impl AiTask {
    /// Starts the tool with the prompt built from the staged diff. With
    /// `amend`, the diff includes the changes of the last commit.
    pub fn start(dir: PathBuf, prefs: &AiPrefs, amend: bool, ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (command, template) = (prefs.command.clone(), prefs.prompt.clone());
        let (flag, ctx) = (cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = run(&dir, &command, &template, amend, &flag);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        Self { rx, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The message (or an error), once the tool has finished.
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err(t!("The AI tool stopped unexpectedly.")))
            }
        }
    }
}

/// Fills the prompt template: `{diff}` is replaced, or the diff is appended.
pub fn build_prompt(template: &str, diff: &str) -> String {
    if template.contains("{diff}") {
        template.replace("{diff}", diff)
    } else {
        format!("{}\n\n{diff}", template.trim_end())
    }
}

fn staged_diff(dir: &std::path::Path, amend: bool) -> Result<String, String> {
    let git = Git::new(dir);
    let mut args = vec!["diff", "--cached", "--no-color", "--no-ext-diff"];
    if amend && git.run(&["rev-parse", "-q", "--verify", "HEAD~1"]).is_ok() {
        args.push("HEAD~1");
    }
    let mut diff = git.run(&args).map_err(|e| e.to_string())?;
    if diff.trim().is_empty() {
        return Err(t!("Nothing is staged."));
    }
    if diff.len() > MAX_DIFF {
        let mut end = MAX_DIFF;
        while !diff.is_char_boundary(end) {
            end -= 1;
        }
        diff.truncate(end);
        diff.push_str("\n[diff truncated]\n");
    }
    Ok(diff)
}

fn shell(command: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = Command::new("cmd");
        c.args(["/C", command]).creation_flags(CREATE_NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
}

fn run(
    dir: &std::path::Path,
    command: &str,
    template: &str,
    amend: bool,
    cancel: &AtomicBool,
) -> Result<String, String> {
    if command.trim().is_empty() {
        return Err(t!("Set the AI command in Settings first."));
    }
    let prompt = build_prompt(template, &staged_diff(dir, amend)?);
    let mut child = shell(command)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| t!("Could not start {}: {}", command, e))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let read = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = read(Box::new(child.stdout.take().expect("stdout is piped")));
    let err = read(Box::new(child.stderr.take().expect("stderr is piped")));
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(t!("Cancelled."));
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if !status.success() {
        let detail = if err.trim().is_empty() { out } else { err };
        return Err(t!("{} failed: {}", command, detail.trim()));
    }
    let message = ferrum_core::message::clean_generated(&out);
    if message.is_empty() {
        return Err(t!("The AI tool returned an empty message."));
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_gets_the_diff() {
        assert_eq!(build_prompt("Write it:\n{diff}", "D"), "Write it:\nD");
        assert_eq!(build_prompt("Write it.\n", "D"), "Write it.\n\nD");
    }

    #[cfg(unix)]
    #[test]
    fn runs_the_command_with_the_prompt_on_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(&["init", "-q"]).unwrap();
        git.run(&["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let cancel = AtomicBool::new(false);
        // Nothing staged yet.
        assert!(run(dir.path(), "cat", "{diff}", false, &cancel).is_err());
        git.run(&["add", "a.txt"]).unwrap();
        // `grep` echoes the matching line of the prompt back.
        let msg = run(dir.path(), "grep '^+hello'", "{diff}", false, &cancel).unwrap();
        assert_eq!(msg, "+hello");
        assert!(run(dir.path(), "exit 3", "{diff}", false, &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            run(dir.path(), "sleep 5", "{diff}", false, &cancel),
            Err(t!("Cancelled."))
        );
    }
}
