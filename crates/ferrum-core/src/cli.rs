//! Runs the `git` command line.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Git {
    dir: PathBuf,
    env: Vec<(String, String)>,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            env: Vec::new(),
        }
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
        let mut cmd = Command::new("git");
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
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
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
        if status.success() {
            Ok(String::from_utf8_lossy(&out).into_owned())
        } else {
            Err(failure(args, &out, &all_err))
        }
    }
}

fn spawn(mut cmd: Command, args: &[&str]) -> Result<std::process::Child> {
    cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Cli("git was not found. Install Git and make sure it is on the PATH.".into())
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
    fn passes_stdin() {
        let t = TestRepo::new();
        let out = t
            .git
            .run_with_input(&["hash-object", "--stdin"], Some(b"hello\n"))
            .unwrap();
        assert_eq!(out.trim(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }
}
