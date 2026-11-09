//! Commit messages written by the AI provider of Settings > AI from the
//! staged diff. Runs on its own thread and can be cancelled.

use std::path::PathBuf;

use eframe::egui;
use ferrum_core::ai::Prompt;
use ferrum_core::cli::Git;

use crate::ai::{Config, Stream};
use crate::commit_prefs::AiPrefs;

/// Larger diffs are cut, so the prompt stays a reasonable size.
const MAX_DIFF: usize = 60_000;

pub struct AiTask {
    stream: Stream,
}

impl AiTask {
    /// Starts the request with the prompt built from the staged diff. With
    /// `amend`, the diff includes the changes of the last commit.
    pub fn start(dir: PathBuf, prefs: &AiPrefs, amend: bool, ctx: &egui::Context) -> Self {
        let cfg = crate::ui::ai_common::config(ctx);
        Self::start_with(dir, &cfg, prefs.prompt.clone(), amend, ctx)
    }

    fn start_with(
        dir: PathBuf,
        cfg: &Config,
        template: String,
        amend: bool,
        ctx: &egui::Context,
    ) -> Self {
        let stream = Stream::start(ctx, cfg, move || {
            let diff = staged_diff(&dir, amend)?;
            Ok(Prompt::new("", build_prompt(&template, &diff)))
        });
        Self { stream }
    }

    pub fn cancel(&self) {
        self.stream.cancel();
    }

    /// The message (or an error), once the model has finished.
    pub fn poll(&self) -> Option<Result<String, String>> {
        let text = self.stream.finished()?;
        Some(text.and_then(|t| {
            let message =
                ferrum_core::message::clean_generated(&ferrum_core::ai::strip_thinking(&t));
            if message.is_empty() {
                Err(t!("The AI tool returned an empty message."))
            } else {
                Ok(message)
            }
        }))
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
    let diff = git.run(&args).map_err(|e| e.to_string())?;
    if diff.trim().is_empty() {
        return Err(t!("Nothing is staged."));
    }
    Ok(truncate(diff, MAX_DIFF))
}

/// Cuts `text` to about `max` bytes, saying so.
pub fn truncate(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[diff truncated]\n");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrum_core::ai::Provider;

    #[test]
    fn prompt_gets_the_diff() {
        assert_eq!(build_prompt("Write it:\n{diff}", "D"), "Write it:\nD");
        assert_eq!(build_prompt("Write it.\n", "D"), "Write it.\n\nD");
    }

    pub(crate) fn config(command: &str, enabled: bool) -> Config {
        Config {
            enabled,
            provider: Provider::Command {
                command: command.into(),
            },
            max: 100_000,
            secrets: Default::default(),
        }
    }

    fn wait(task: &AiTask) -> Result<String, String> {
        for _ in 0..500 {
            if let Some(r) = task.poll() {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the task did not finish");
    }

    #[cfg(unix)]
    #[test]
    fn writes_the_message_from_the_staged_diff() {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(&["init", "-q"]).unwrap();
        git.run(&["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let ctx = egui::Context::default();
        let start = |command: &str, enabled: bool| {
            AiTask::start_with(
                dir.path().into(),
                &config(command, enabled),
                "{diff}".into(),
                false,
                &ctx,
            )
        };
        // Nothing staged yet.
        assert_eq!(wait(&start("cat", true)), Err(t!("Nothing is staged.")));
        git.run(&["add", "a.txt"]).unwrap();
        // `grep` echoes the matching line of the prompt back.
        assert_eq!(wait(&start("grep '^+hello'", true)), Ok("+hello".into()));
        assert!(wait(&start("exit 3", true)).is_err());
        // Nothing is run while AI is off.
        assert_eq!(
            wait(&start("grep '^+hello'", false)),
            Err(t!("AI is off. Turn it on in Settings > AI."))
        );
        let slow = start("sleep 5", true);
        slow.cancel();
        assert_eq!(wait(&slow), Err(t!("Cancelled.")));
    }
}
