//! A command line AI tool: the prompt goes to its standard input and its
//! standard output, read as it comes, is the answer. It runs in the temp
//! folder, not in the repository.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::AiError;

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

/// Where `program` is: a path, or found on the PATH.
pub fn find_program(program: &str) -> Option<PathBuf> {
    let p = PathBuf::from(program);
    if p.components().count() > 1 {
        return p.is_file().then_some(p);
    }
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|e| dir.join(format!("{program}{e}")))
            .find(|p| p.is_file())
    })
}

pub fn run(
    command: &str,
    prompt: &str,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
) -> Result<String, AiError> {
    if command.trim().is_empty() {
        return Err(AiError::Missing("command"));
    }
    let program = command
        .split_whitespace()
        .next()
        .unwrap_or(command)
        .to_owned();
    // Run outside the repository: the tool gets only the prompt, whose
    // secrets are masked, and cannot read the files around it.
    let mut child = shell(command)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AiError::Start {
            program: program.clone(),
            detail: e.to_string(),
        })?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input = prompt.as_bytes().to_vec();
    std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut take = |bytes: Vec<u8>, out: &mut String, on_text: &mut dyn FnMut(&str)| {
        pending.extend(bytes);
        let valid = match std::str::from_utf8(&pending) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        if valid > 0 {
            let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
            pending.drain(..valid);
            out.push_str(&text);
            on_text(&text);
        }
    };
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AiError::Cancelled);
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => take(bytes, &mut out, on_text),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = reader.join();
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AiError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                return Err(AiError::Failed {
                    program,
                    detail: e.to_string(),
                });
            }
        }
    };
    let err = errors.join().unwrap_or_default();
    if !status.success() {
        let detail = if err.trim().is_empty() { &out } else { &err };
        return Err(AiError::Failed {
            program,
            detail: detail.trim().to_owned(),
        });
    }
    Ok(out)
}
