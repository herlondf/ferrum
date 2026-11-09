//! The context and instructions sent for each AI feature. Context is read
//! with git and cut to `max` bytes; secrets are masked later, by the
//! caller, right before sending ([`super::redact`]).

use super::{Prompt, cap};
use crate::cli::Git;
use crate::{Oid, Result};

fn explain_system(lang: &str) -> String {
    format!(
        "You are an experienced software engineer explaining git changes to a colleague. \
Answer in {lang}, in Markdown: first a short paragraph saying what the change does and why, \
then the important details as a short list (files, behaviour, risks, anything surprising). \
Be concise and concrete; do not repeat the diff."
    )
}

/// "Explain this commit": its message, stats and patch.
pub fn explain_commit(git: &Git, id: Oid, lang: &str, max: usize) -> Result<Prompt> {
    let sha = id.to_string();
    let show = |extra: &[&str]| {
        let mut args = vec![
            "show",
            "--no-color",
            "--no-ext-diff",
            "--format=fuller",
            "--stat",
            "--patch",
        ];
        args.extend_from_slice(extra);
        args.push(&sha);
        git.run(&args)
    };
    let text = show(&["--diff-merges=first-parent"]).or_else(|_| show(&[]))?;
    let (text, _) = cap(&text, max);
    Ok(Prompt::new(
        explain_system(lang),
        format!("Explain this commit.\n\n{text}"),
    ))
}

/// What "Explain this diff" is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOf {
    /// Local changes of a file: staged or not.
    Local { path: String, staged: bool },
    /// A file in a commit.
    Commit { id: Oid, path: String },
}

pub fn explain_diff(git: &Git, of: &DiffOf, lang: &str, max: usize) -> Result<Prompt> {
    let text = match of {
        DiffOf::Local { path, staged } => {
            let mut args = vec!["diff", "--no-color", "--no-ext-diff"];
            if *staged {
                args.push("--cached");
            }
            args.extend(["--", path.as_str()]);
            let diff = git.run(&args)?;
            if diff.trim().is_empty() && !staged {
                // Untracked: the whole file is new.
                let bytes = std::fs::read(git.dir().join(path))?;
                let content = String::from_utf8_lossy(&bytes);
                let mut out = format!("New file {path}:\n");
                for l in content.lines() {
                    out.push('+');
                    out.push_str(l);
                    out.push('\n');
                }
                out
            } else {
                diff
            }
        }
        DiffOf::Commit { id, path } => {
            let sha = id.to_string();
            git.run(&[
                "show",
                "--no-color",
                "--no-ext-diff",
                "--format=Commit %h: %s%n%n%b",
                "--patch",
                &sha,
                "--",
                path,
            ])?
        }
    };
    let (text, _) = cap(&text, max);
    Ok(Prompt::new(
        explain_system(lang),
        format!("Explain this diff.\n\n{text}"),
    ))
}

/// "Explain this file's history": the last commits that changed it, with
/// the patches of the most recent ones.
pub fn explain_history(git: &Git, path: &str, lang: &str, max: usize) -> Result<Prompt> {
    let log = git.run(&[
        "log",
        "--follow",
        "-n",
        "40",
        "--no-color",
        "--date=short",
        "--format=- %h %ad %an: %s",
        "--",
        path,
    ])?;
    let patches = git
        .run(&[
            "log",
            "--follow",
            "-n",
            "8",
            "--no-color",
            "--no-ext-diff",
            "--date=short",
            "--format=commit %h %ad %an%n%s%n%b",
            "-p",
            "-U2",
            "--",
            path,
        ])
        .unwrap_or_default();
    let (patches, _) = cap(&patches, max.saturating_sub(log.len()).max(max / 3));
    let system = format!(
        "You are an experienced software engineer explaining how a file evolved. Answer in {lang}, \
in Markdown: a short paragraph about what the file is for, then its history as a few periods \
or themes (who changed what and why, with commit hashes), then anything worth knowing before \
changing it. Be concise."
    );
    let (log, _) = cap(&log, max / 3);
    Ok(Prompt::new(
        system,
        format!(
            "Explain the history of {path}.\n\nCommits, newest first:\n{log}\nThe latest changes:\n{patches}"
        ),
    ))
}

/// Base, ours and theirs of a conflicted file, with the side names.
pub struct ConflictSides<'a> {
    pub path: &'a str,
    pub base: Option<&'a str>,
    pub ours: &'a str,
    pub theirs: &'a str,
    pub ours_name: &'a str,
    pub theirs_name: &'a str,
}

pub fn resolve_conflict(s: &ConflictSides, lang: &str, max: usize) -> Prompt {
    let third = max / 3;
    let part = |t: &str| {
        let mut t = cap(t, third).0;
        if !t.ends_with('\n') {
            t.push('\n');
        }
        t
    };
    let system = format!(
        "You resolve git merge conflicts. You get the common base and the two versions of a file. \
Merge them so that the intent of both sides is kept; when they truly contradict, prefer the \
smallest sensible result and say so. First explain in {lang}, in a few short sentences or bullet \
points, what each side changed and how you merged it. Then give the complete merged file in one \
fenced code block, with no conflict markers and nothing left out."
    );
    let base = s
        .base
        .map(|b| format!("Base (common ancestor):\n```\n{}```\n\n", part(b)))
        .unwrap_or_else(|| "There is no common base: both sides added the file.\n\n".into());
    Prompt::new(
        system,
        format!(
            "File: {}\n\n{base}Ours ({}):\n```\n{}```\n\nTheirs ({}):\n```\n{}```\n",
            s.path,
            s.ours_name,
            part(s.ours),
            s.theirs_name,
            part(s.theirs)
        ),
    )
}

/// A proposed resolution: the explanation and the merged file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub explanation: String,
    pub text: String,
    /// The answer still has conflict markers.
    pub has_markers: bool,
}

/// Reads [`resolve_conflict`]'s answer: the longest fenced block is the
/// file, the text around it the explanation.
pub fn parse_resolution(answer: &str, ends_with_newline: bool) -> Option<Resolution> {
    let answer = super::strip_thinking(answer);
    let blocks = super::json::fenced_blocks(&answer);
    let mut text = blocks.into_iter().max_by_key(String::len)?;
    let first_line = answer
        .lines()
        .position(|l| l.trim_start().starts_with("```"))
        .unwrap_or(0);
    let explanation = answer
        .lines()
        .take(first_line)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    if !ends_with_newline {
        while text.ends_with('\n') {
            text.pop();
        }
    }
    let has_markers = text
        .lines()
        .any(|l| l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> ") || l == "=======");
    Some(Resolution {
        explanation,
        text,
        has_markers,
    })
}

/// A pull request description from the commits and the diff of `head`
/// since it left `base`.
pub fn pull_request(git: &Git, base: &str, head: &str, lang: &str, max: usize) -> Result<Prompt> {
    let range = format!("{base}..{head}");
    let log = git.run(&[
        "log",
        "--no-merges",
        "--reverse",
        "--no-color",
        "--format=- %s%n%w(0,2,2)%b",
        &range,
    ])?;
    let three = format!("{base}...{head}");
    let stat = git.run(&["diff", "--no-color", "--stat", &three])?;
    let diff = git.run(&["diff", "--no-color", "--no-ext-diff", &three])?;
    let (log, _) = cap(&log, max / 4);
    let (diff, _) = cap(&diff, max.saturating_sub(log.len() + stat.len()));
    let system = format!(
        "You write pull request descriptions. Write in {lang}, in Markdown, without a title: a \
short paragraph saying what the change does and why, then a \"Changes\" section with a short \
bullet list, then a \"Testing\" section only if the changes show how it was tested. Be concise; \
do not invent anything that is not in the commits or the diff."
    );
    Ok(Prompt::new(
        system,
        format!("Commits:\n{log}\n\nFiles:\n{stat}\n\nDiff:\n{diff}"),
    ))
}
