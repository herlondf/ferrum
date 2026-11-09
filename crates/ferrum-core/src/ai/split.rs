//! Splitting the local changes into several commits: the changes are cut
//! into items (hunks of tracked text files; whole files for new, deleted,
//! binary and untracked files), a model (or [`by_folder`]) groups them
//! with messages, the user reviews the groups and [`apply`] commits them
//! one by one.
//!
//! Only the index changes: each group is staged from the diff and
//! committed; the working copy is never touched, so whatever is left out
//! stays a local change.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::Prompt;
use super::json::text_field;
use crate::cli::Git;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A hunk of a text file; `header` is the file's diff header.
    Hunk { header: String },
    /// A whole file, staged with `git add`.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `H1`, `H2`… in the order of the diff.
    pub id: String,
    pub path: String,
    pub kind: Kind,
    /// The hunk (from its `@@` line), or a summary of the file change.
    pub text: String,
    pub added: usize,
    pub removed: usize,
}

impl Item {
    /// The `@@ … @@` line, or the file summary.
    pub fn title(&self) -> &str {
        self.text.lines().next().unwrap_or_default()
    }
}

/// The local changes cut into items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    pub items: Vec<Item>,
    /// Changes when anything in the local changes does.
    pub fingerprint: String,
}

/// A commit to make: its message and the ids of its items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    pub message: String,
    pub items: Vec<String>,
}

/// Reads the local changes (staged or not, and untracked files).
pub fn collect(git: &Git) -> Result<Changes> {
    if git.run(&["rev-parse", "-q", "--verify", "HEAD"]).is_err() {
        return Err(Error::Other(
            "the repository has no commits yet; make the first commit first".into(),
        ));
    }
    let diff = git.run(&[
        "diff",
        "HEAD",
        "--no-color",
        "--no-ext-diff",
        "--no-renames",
        "--submodule=short",
        "-U3",
    ])?;
    let untracked = git.run(&["ls-files", "--others", "--exclude-standard", "-z"])?;
    let mut items = parse_diff(&diff);
    for path in untracked.split('\0').filter(|p| !p.is_empty()) {
        let lines = std::fs::read(git.dir().join(path))
            .map(|b| bytecount_lines(&b))
            .unwrap_or(0);
        items.push(Item {
            id: String::new(),
            path: path.to_owned(),
            kind: Kind::File,
            text: format!("new file {path}"),
            added: lines,
            removed: 0,
        });
    }
    for (i, item) in items.iter_mut().enumerate() {
        item.id = format!("H{}", i + 1);
    }
    let fingerprint = format!("{:x}", md5::compute(format!("{diff}\0{untracked}")));
    Ok(Changes { items, fingerprint })
}

fn bytecount_lines(b: &[u8]) -> usize {
    if b.contains(&0) {
        return 0;
    }
    b.iter().filter(|&&c| c == b'\n').count()
}

/// Cuts `git diff` output into items.
pub fn parse_diff(diff: &str) -> Vec<Item> {
    let mut items = Vec::new();
    for file in split_files(diff) {
        let (header, hunks) = match file.find("\n@@") {
            Some(i) => (&file[..=i], &file[i + 1..]),
            None => (file, ""),
        };
        let path = file_path(header);
        let whole = header.contains("\nnew file mode")
            || header.contains("\ndeleted file mode")
            || header.contains("\nold mode")
            || header.contains("Binary files")
            || header.contains("GIT binary patch")
            || hunks.contains("+Subproject commit")
            || hunks.is_empty();
        if whole {
            let (added, removed) = count(hunks);
            let what = if header.contains("\nnew file mode") {
                "new file"
            } else if header.contains("\ndeleted file mode") {
                "deleted file"
            } else if header.contains("Binary files") {
                "binary file"
            } else {
                "changed file"
            };
            items.push(Item {
                id: String::new(),
                path: path.clone(),
                kind: Kind::File,
                text: format!("{what} {path}\n{hunks}"),
                added,
                removed,
            });
            continue;
        }
        let starts: Vec<usize> = hunks
            .match_indices("@@ -")
            .map(|(i, _)| i)
            .filter(|&i| i == 0 || hunks.as_bytes()[i - 1] == b'\n')
            .collect();
        for (k, &start) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(hunks.len());
            let text = &hunks[start..end];
            let (added, removed) = count(text);
            items.push(Item {
                id: String::new(),
                path: path.clone(),
                kind: Kind::Hunk {
                    header: header.to_owned(),
                },
                text: text.to_owned(),
                added,
                removed,
            });
        }
    }
    items
}

fn split_files(diff: &str) -> Vec<&str> {
    let starts: Vec<usize> = diff
        .match_indices("diff --git ")
        .map(|(i, _)| i)
        .filter(|&i| i == 0 || diff.as_bytes()[i - 1] == b'\n')
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| &diff[s..starts.get(k + 1).copied().unwrap_or(diff.len())])
        .collect()
}

fn file_path(header: &str) -> String {
    for line in header.lines() {
        if let Some(p) = line.strip_prefix("+++ b/") {
            return unquote(p);
        }
    }
    for line in header.lines() {
        if let Some(p) = line.strip_prefix("--- a/") {
            return unquote(p);
        }
    }
    // diff --git a/x b/x
    let first = header.lines().next().unwrap_or_default();
    let rest = first.strip_prefix("diff --git ").unwrap_or(first);
    let half = rest.len() / 2;
    unquote(rest[half..].trim().trim_start_matches("b/"))
}

fn unquote(p: &str) -> String {
    p.trim().trim_matches('"').to_owned()
}

fn count(text: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for l in text.lines() {
        if l.starts_with('+') && !l.starts_with("+++") {
            added += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            removed += 1;
        }
    }
    (added, removed)
}

/// Asks a model to group the items; at most `max` bytes of hunks are shown.
pub fn prompt(changes: &Changes, max: usize) -> Prompt {
    let system = "You split a set of local changes into a few focused git commits. \
Each change has an id (H1, H2…). Group changes that belong together (one feature, one fix, \
one refactor), in the order they should be committed. Every id goes in exactly one commit. \
Write each commit message in the imperative mood: a summary line of at most 50 characters, \
then optionally a blank line and a short body.\n\
Reply with one JSON object and nothing else:\n\
{\"commits\": [{\"message\": \"Add login form\", \"changes\": [\"H1\", \"H3\"]}]}";
    let mut user = String::from("The changes:\n\n");
    let per_item = (max / changes.items.len().max(1)).max(400);
    for item in &changes.items {
        let body = &item.text;
        let (body, _) = super::cap(body, per_item);
        user.push_str(&format!(
            "### {} {} (+{} -{})\n{}\n",
            item.id,
            item.path,
            item.added,
            item.removed,
            body.trim_end()
        ));
        if user.len() > max {
            user.push_str("\n[… more changes left out]\n");
            break;
        }
    }
    Prompt::new(system, user)
}

/// Reads the model's groups: unknown ids are dropped, an id in several
/// groups stays in the first, empty groups go, and items in no group get
/// a last group of their own.
pub fn parse_groups(v: &Value, changes: &Changes) -> Vec<Group> {
    let list = match v {
        Value::Array(a) => a.clone(),
        _ => ["commits", "groups", "split"]
            .iter()
            .find_map(|k| v.get(*k).and_then(Value::as_array).cloned())
            .unwrap_or_default(),
    };
    let known: HashSet<&str> = changes.items.iter().map(|i| i.id.as_str()).collect();
    let mut used: HashSet<String> = HashSet::new();
    let mut groups = Vec::new();
    for g in list {
        let message = text_field(&g, "message")
            .or_else(|| text_field(&g, "title"))
            .unwrap_or_default();
        let ids = ["changes", "hunks", "items", "ids"]
            .iter()
            .find_map(|k| g.get(*k).and_then(Value::as_array).cloned())
            .unwrap_or_default();
        let mut items = Vec::new();
        for id in ids {
            let id = match id {
                Value::String(s) => s.trim().to_uppercase(),
                Value::Number(n) => format!("H{n}"),
                _ => continue,
            };
            let id = if id.starts_with('H') {
                id
            } else {
                format!("H{id}")
            };
            if known.contains(id.as_str()) && used.insert(id.clone()) {
                items.push(id);
            }
        }
        if !items.is_empty() {
            groups.push(Group { message, items });
        }
    }
    let rest: Vec<String> = changes
        .items
        .iter()
        .filter(|i| !used.contains(&i.id))
        .map(|i| i.id.clone())
        .collect();
    if !rest.is_empty() {
        groups.push(Group {
            message: String::new(),
            items: rest,
        });
    }
    groups
}

/// A split without a model: one commit per top-level folder.
pub fn by_folder(changes: &Changes) -> Vec<Group> {
    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &changes.items {
        let folder = match item.path.split_once('/') {
            Some((dir, _)) => dir.to_owned(),
            None => String::new(),
        };
        folders.entry(folder).or_default().push(item.id.clone());
    }
    folders
        .into_iter()
        .map(|(folder, items)| Group {
            message: if folder.is_empty() {
                "Update top-level files".into()
            } else {
                format!("Update {folder}")
            },
            items,
        })
        .collect()
}

/// Makes one commit per group (groups without items or message are
/// skipped), in order. Fails without changing anything when the local
/// changes are not the ones `changes` was read from. `before_commit` runs
/// with each group staged (the secret check) and can stop the split.
pub fn apply(
    git: &Git,
    changes: &Changes,
    groups: &[Group],
    mut before_commit: impl FnMut(usize) -> Result<()>,
    mut progress: impl FnMut(&str),
) -> Result<usize> {
    let now = collect(git)?;
    if now.fingerprint != changes.fingerprint {
        return Err(Error::Other(
            "the local changes changed since the split was proposed; propose it again".into(),
        ));
    }
    let by_id: BTreeMap<&str, &Item> = changes.items.iter().map(|i| (i.id.as_str(), i)).collect();
    // Start from an index equal to HEAD.
    git.run(&["reset", "-q", "--"])?;
    let mut made = 0;
    for (n, g) in groups.iter().enumerate() {
        let message = g.message.trim();
        if g.items.is_empty() || message.is_empty() {
            continue;
        }
        progress(message.lines().next().unwrap_or_default());
        let mut patches: Vec<(&str, String)> = Vec::new();
        let mut files: Vec<String> = Vec::new();
        // Hunks keep the order of the diff within each file.
        let mut ids: Vec<&Item> = g
            .items
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .collect();
        ids.sort_by_key(|i| i.id[1..].parse::<usize>().unwrap_or(0));
        for item in ids {
            match &item.kind {
                Kind::File => files.push(item.path.clone()),
                Kind::Hunk { header } => match patches.iter_mut().find(|(p, _)| *p == item.path) {
                    Some((_, patch)) => patch.push_str(&item.text),
                    None => patches.push((&item.path, format!("{header}{}", item.text))),
                },
            }
        }
        for (_, patch) in &patches {
            crate::ops::apply_patch(git, patch, crate::ops::PatchTarget::Index, false)?;
        }
        if !files.is_empty() {
            let mut args = vec!["add", "-A", "--"];
            args.extend(files.iter().map(String::as_str));
            git.run(&args)?;
        }
        if git.run(&["diff", "--cached", "--quiet"]).is_ok() {
            continue;
        }
        before_commit(n)?;
        crate::local::commit(git, message, Default::default(), |_| {})?;
        made += 1;
    }
    Ok(made)
}
