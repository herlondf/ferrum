//! Custom commands: shell scripts, programs or URLs the user adds to the
//! context menus and the palette, with variables like `$SHA` and `$branch`.
//! They come from Settings and from `.ferrum/custom-commands.json` in the
//! repository (shared with the team; each one is approved before its first run).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ferrum_core::cli::Git;
use ferrum_core::shell::{self, Program};
use ferrum_core::{BranchInfo, Oid};
use serde::{Deserialize, Serialize};

use crate::command::{Command, Op};
use crate::jobs::Refresh;
use crate::settings::Settings;
use crate::tab::RepoTab;

/// The shared file, relative to the repository.
pub const SHARED_FILE: &str = ".ferrum/custom-commands.json";

/// What a command is shown for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Repository,
    Commit,
    Branch,
    File,
    Submodule,
}

impl Scope {
    pub const ALL: [Scope; 5] = [
        Scope::Repository,
        Scope::Commit,
        Scope::Branch,
        Scope::File,
        Scope::Submodule,
    ];

    pub fn label(self) -> String {
        match self {
            Scope::Repository => t!("Repository"),
            Scope::Commit => t!("Commit"),
            Scope::Branch => t!("Branch"),
            Scope::File => t!("File"),
            Scope::Submodule => t!("Submodule"),
        }
    }
}

/// How the command text is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A script for `sh -c` (`cmd /C` on Windows).
    #[default]
    Shell,
    /// A program or script file and its arguments, without a shell.
    Script,
    /// A web address opened in the browser.
    Url,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Shell, Kind::Script, Kind::Url];

    pub fn label(self) -> String {
        match self {
            Kind::Shell => t!("Shell command"),
            Kind::Script => t!("Program and arguments"),
            Kind::Url => t!("Open URL"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    #[default]
    Text,
    Branch,
    File,
    Checkbox,
    Choice,
}

impl FieldKind {
    pub const ALL: [FieldKind; 5] = [
        FieldKind::Text,
        FieldKind::Branch,
        FieldKind::File,
        FieldKind::Checkbox,
        FieldKind::Choice,
    ];

    pub fn label(self) -> String {
        match self {
            FieldKind::Text => t!("Text"),
            FieldKind::Branch => t!("Branch"),
            FieldKind::File => t!("File"),
            FieldKind::Checkbox => t!("Checkbox"),
            FieldKind::Choice => t!("Choice"),
        }
    }
}

/// A field of the form shown before running; its value becomes `$name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub kind: FieldKind,
    /// Initial value (for a checkbox: "true" to start checked).
    pub default: String,
    /// Options of a choice.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// Checkbox values when checked / not checked (inserted as typed).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub on: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub off: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CustomCommand {
    /// Shown in menus; `/` makes submenus ("Deploy/Staging").
    pub name: String,
    pub scope: Scope,
    pub kind: Kind,
    pub command: String,
    /// Replaces `command` on that system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    /// Reload the repository when it finishes.
    pub refresh: bool,
    /// Ask before running (shows the command).
    pub confirm: bool,
    /// Open the output window while it runs.
    pub show_output: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub form: Vec<FormField>,
}

impl CustomCommand {
    pub fn new(name: &str, scope: Scope) -> Self {
        Self {
            name: name.to_owned(),
            scope,
            ..Default::default()
        }
    }

    /// The command text for this system.
    pub fn text(&self) -> &str {
        let os = if cfg!(windows) {
            &self.windows
        } else if cfg!(target_os = "macos") {
            &self.macos
        } else {
            &self.linux
        };
        os.as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.command)
    }

    /// The last part of the name (after the submenus).
    pub fn leaf(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name).trim()
    }
}

/// Custom commands in Settings, and the shared ones already approved.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomSettings {
    pub commands: Vec<CustomCommand>,
    /// `repo path|hash` of shared commands the user allowed to run.
    pub approved: BTreeSet<String>,
}

/// The contents of `.ferrum/custom-commands.json`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SharedFile {
    List(Vec<CustomCommand>),
    Object { commands: Vec<CustomCommand> },
}

pub fn parse_shared(text: &str) -> Result<Vec<CustomCommand>, String> {
    match serde_json::from_str::<SharedFile>(text) {
        Ok(SharedFile::List(l) | SharedFile::Object { commands: l }) => Ok(l),
        Err(e) => Err(e.to_string()),
    }
}

/// The JSON written for sharing, as in the repository file.
pub fn to_json(list: &[CustomCommand]) -> String {
    #[derive(Serialize)]
    struct Out<'a> {
        commands: &'a [CustomCommand],
    }
    serde_json::to_string_pretty(&Out { commands: list }).unwrap_or_default()
}

type SharedCache = HashMap<PathBuf, (Option<SystemTime>, Result<Vec<CustomCommand>, String>)>;

thread_local! {
    static SHARED: RefCell<SharedCache> = RefCell::new(HashMap::new());
    /// The commands of Settings, copied every frame (menus get no settings).
    static USER: RefCell<Vec<CustomCommand>> = const { RefCell::new(Vec::new()) };
}

/// Called every frame with the commands of Settings.
pub fn set_user_commands(list: &[CustomCommand]) {
    USER.with(|u| {
        if u.borrow().as_slice() != list {
            *u.borrow_mut() = list.to_vec();
        }
    });
}

/// The repository's shared commands (reloaded when the file changes), or the
/// error reading it.
pub fn shared(repo: &Path) -> Result<Vec<CustomCommand>, String> {
    let file = repo.join(SHARED_FILE);
    let mtime = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
    SHARED.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((t, list)) = c.get(repo)
            && *t == mtime
        {
            return list.clone();
        }
        let list = match mtime {
            None => Ok(Vec::new()),
            Some(_) => std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|t| parse_shared(&t)),
        };
        c.insert(repo.to_path_buf(), (mtime, list.clone()));
        list
    })
}

/// Commands for a scope: Settings first, then the repository's (`true`).
pub fn for_scope(repo: &Path, scope: Scope) -> Vec<(CustomCommand, bool)> {
    let mut out: Vec<(CustomCommand, bool)> = USER.with(|u| {
        u.borrow()
            .iter()
            .filter(|c| c.scope == scope && !c.name.trim().is_empty())
            .map(|c| (c.clone(), false))
            .collect()
    });
    if let Ok(list) = shared(repo) {
        out.extend(
            list.into_iter()
                .filter(|c| c.scope == scope && !c.name.trim().is_empty())
                .map(|c| (c, true)),
        );
    }
    out
}

/// What a menu was opened on. Missing values are filled from the repository
/// when the command runs.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub repo: PathBuf,
    pub scope: Scope,
    pub sha: Option<String>,
    pub branch: Option<String>,
    pub remote: Option<String>,
    pub files: Vec<String>,
    pub submodule: Option<String>,
    pub selected: Vec<String>,
}

impl Target {
    pub fn repository(tab: &RepoTab) -> Self {
        Self {
            repo: tab.path().to_path_buf(),
            ..Default::default()
        }
    }

    pub fn commit(tab: &RepoTab, id: Oid) -> Self {
        let many = &tab.history.ext.selected_many;
        let selected = if many.contains(&id) && many.len() > 1 {
            many.iter().map(Oid::to_string).collect()
        } else {
            vec![id.to_string()]
        };
        Self {
            scope: Scope::Commit,
            sha: Some(id.to_string()),
            selected,
            ..Self::repository(tab)
        }
    }

    pub fn branch(tab: &RepoTab, b: &BranchInfo) -> Self {
        let (remote, branch) = match b.split_remote() {
            Some((r, _)) => (Some(r.to_owned()), b.name.clone()),
            None => (
                b.upstream
                    .as_deref()
                    .and_then(|u| u.split_once('/'))
                    .map(|(r, _)| r.to_owned()),
                b.name.clone(),
            ),
        };
        Self {
            scope: Scope::Branch,
            sha: b.target.map(|t| t.to_string()),
            branch: Some(branch),
            remote,
            ..Self::repository(tab)
        }
    }

    pub fn files(repo: &Path, files: &[String], sha: Option<Oid>) -> Self {
        Self {
            repo: repo.to_path_buf(),
            scope: Scope::File,
            sha: sha.map(|s| s.to_string()),
            files: files.to_vec(),
            ..Default::default()
        }
    }

    pub fn submodule(tab: &RepoTab, path: &str) -> Self {
        Self {
            scope: Scope::Submodule,
            submodule: Some(path.to_owned()),
            ..Self::repository(tab)
        }
    }
}

/// A command picked in a menu, resolved by the app (which has the settings).
#[derive(Debug, Clone)]
pub struct Invocation {
    pub cmd: CustomCommand,
    pub shared: bool,
    pub target: Target,
}

/// A variable value and how to insert it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Quoted for the shell when needed.
    Text(String),
    /// Several words, each quoted (e.g. `$selected_shas`, several files).
    List(Vec<String>),
    /// Inserted as typed (checkbox values).
    Raw(String),
}

pub type Vars = Vec<(String, Value)>;

/// The variables of a target, with what is missing taken from the repository.
pub fn variables(target: &Target, tab: Option<&RepoTab>) -> Vars {
    let head_sha = tab.and_then(|t| t.head_oid).map(|o| o.to_string());
    let sha = target.sha.clone().or(head_sha).unwrap_or_default();
    let branch = target
        .branch
        .clone()
        .or_else(|| tab.and_then(|t| t.head.clone()))
        .unwrap_or_default();
    let remote = target
        .remote
        .clone()
        .or_else(|| {
            let t = tab?;
            let up = t.current_branch().and_then(|b| b.upstream.as_deref());
            up.and_then(|u| u.split_once('/').map(|(r, _)| r.to_owned()))
                .or_else(|| t.remotes.first().map(|r| r.name.clone()))
        })
        .unwrap_or_default();
    let selected = if target.selected.is_empty() {
        vec![sha.clone()]
    } else {
        target.selected.clone()
    };
    let short = sha.get(..7).unwrap_or(&sha).to_owned();
    let file = match target.files.as_slice() {
        [one] => Value::Text(one.clone()),
        list => Value::List(list.to_vec()),
    };
    vec![
        ("SHA".into(), Value::Text(sha)),
        ("SHORT_SHA".into(), Value::Text(short)),
        ("branch".into(), Value::Text(branch)),
        ("file".into(), file),
        (
            "path".into(),
            Value::Text(target.repo.display().to_string()),
        ),
        ("remote".into(), Value::Text(remote)),
        (
            "submodule".into(),
            Value::Text(target.submodule.clone().unwrap_or_default()),
        ),
        ("selected_shas".into(), Value::List(selected)),
    ]
}

/// The form's values as variables (`values` in the order of `form`).
pub fn form_variables(form: &[FormField], values: &[String]) -> Vars {
    form.iter()
        .zip(values)
        .filter(|(f, _)| !f.name.trim().is_empty())
        .map(|(f, v)| {
            let value = match f.kind {
                FieldKind::Checkbox => {
                    let on = v == "true";
                    let (yes, no) = (
                        if f.on.is_empty() { "true" } else { &f.on },
                        if f.off.is_empty() && f.on.is_empty() {
                            "false"
                        } else {
                            &f.off
                        },
                    );
                    Value::Raw(if on { yes } else { no }.to_owned())
                }
                _ => Value::Text(v.clone()),
            };
            (f.name.trim().to_owned(), value)
        })
        .collect()
}

/// How values are inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    Shell,
    /// One argument of a program: as is.
    Plain,
    Url,
}

/// Replaces `$name` and `${name}` with known variables; others stay (so
/// `$HOME` still reaches the shell).
pub fn expand(text: &str, vars: &Vars, quote: Quote) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let (name, len) = if let Some(inner) = after.strip_prefix('{') {
            match inner.find('}') {
                Some(end) => (&inner[..end], end + 2),
                None => ("", 0),
            }
        } else {
            let end = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            (&after[..end], end)
        };
        match vars.iter().find(|(n, _)| n == name).filter(|_| len > 0) {
            Some((_, v)) => {
                out.push_str(&insert(v, quote));
                rest = &after[len..];
            }
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn insert(v: &Value, quote: Quote) -> String {
    let one = |s: &str| match quote {
        Quote::Shell => shell_quote(s),
        Quote::Plain => s.to_owned(),
        Quote::Url => url_encode(s),
    };
    match v {
        Value::Raw(s) => s.clone(),
        Value::Text(s) => one(s),
        Value::List(l) => {
            let sep = if quote == Quote::Url { "," } else { " " };
            l.iter().map(|s| one(s)).collect::<Vec<_>>().join(sep)
        }
    }
}

/// Quotes a value for the shell unless it is made of safe characters only.
pub fn shell_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        return s.to_owned();
    }
    if cfg!(windows) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// What is run, with the variables filled in.
pub fn program(cmd: &CustomCommand, vars: &Vars) -> Result<Program, String> {
    let text = cmd.text().trim();
    if text.is_empty() {
        return Err(t!("The command {} is empty.", cmd.name));
    }
    Ok(match cmd.kind {
        Kind::Shell | Kind::Url => Program::Shell(expand(text, vars, Quote::Shell)),
        Kind::Script => {
            let mut words = shell::split_args(text)
                .into_iter()
                .map(|w| expand(&w, vars, Quote::Plain));
            let program = words.next().unwrap_or_default();
            Program::Exec {
                program,
                args: words.collect(),
            }
        }
    })
}

/// FNV-1a, stable between versions (approvals are saved).
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Key of an approval: the repository and what the command runs.
pub fn approval_key(repo: &Path, cmd: &CustomCommand) -> String {
    let what = format!("{:?}\n{}", cmd.kind, cmd.text());
    format!("{}|{:016x}", repo.display(), hash(&what))
}

/// Runs a custom command (as an `Op`).
#[derive(Debug, Clone)]
pub struct Run {
    pub label: String,
    pub program: Program,
    pub refresh: bool,
}

impl Run {
    pub fn refresh(&self) -> Refresh {
        if self.refresh {
            Refresh::All
        } else {
            Refresh::None
        }
    }

    pub fn run(&self, git: &Git, progress: &dyn Fn(&str)) -> ferrum_core::Result<()> {
        shell::run(git, &self.program, |l| progress(l)).map(drop)
    }
}

/// Commands that run `cmd` now.
pub fn start(cmd: &CustomCommand, vars: &Vars) -> Vec<Command> {
    if cmd.kind == Kind::Url {
        let url = expand(cmd.text().trim(), vars, Quote::Url);
        return vec![Command::OpenUrl(url)];
    }
    match program(cmd, vars) {
        Ok(program) => {
            let label = cmd.leaf().to_owned();
            if cmd.show_output {
                crate::ui::custom_commands::watch_output(&label);
            }
            vec![Command::Run(Op::Custom(Run {
                label,
                program,
                refresh: cmd.refresh,
            }))]
        }
        Err(e) => vec![Command::Error(e)],
    }
}

/// What to do when a command is picked: run it, or first show its form,
/// the confirmation, or the approval of a shared command.
pub fn resolve(inv: Invocation, settings: &Settings, tab: Option<&RepoTab>) -> Vec<Command> {
    let vars = variables(&inv.target, tab);
    let key = approval_key(&inv.target.repo, &inv.cmd);
    let approve = inv.shared && !settings.custom.approved.contains(&key);
    if approve || inv.cmd.confirm || !inv.cmd.form.is_empty() {
        let d = crate::ui::custom_commands::CustomDialog::new(inv, vars, approve.then_some(key));
        return vec![Command::Dialog(crate::ui::dialogs::Dialog::Custom(d))];
    }
    start(&inv.cmd, &vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        let target = Target {
            repo: "/r".into(),
            scope: Scope::Commit,
            sha: Some("0123456789abcdef".into()),
            branch: Some("feature/x".into()),
            files: vec!["a b.txt".into()],
            selected: vec!["aaa".into(), "bbb".into()],
            ..Default::default()
        };
        variables(&target, None)
    }

    #[test]
    fn expands_known_variables() {
        let v = vars();
        assert_eq!(
            expand("$SHORT_SHA $SHA", &v, Quote::Plain),
            "0123456 0123456789abcdef"
        );
        assert_eq!(expand("${branch}!", &v, Quote::Plain), "feature/x!");
        assert_eq!(expand("$HOME and $", &v, Quote::Plain), "$HOME and $");
        assert_eq!(expand("$selected_shas", &v, Quote::Shell), "aaa bbb");
        assert_eq!(expand("cost $5", &v, Quote::Plain), "cost $5");
    }

    #[test]
    fn quotes_for_the_shell() {
        let v = vars();
        let quoted = if cfg!(windows) {
            "\"a b.txt\""
        } else {
            "'a b.txt'"
        };
        assert_eq!(
            expand("cat $file", &v, Quote::Shell),
            format!("cat {quoted}")
        );
        assert_eq!(shell_quote("feature/x"), "feature/x");
        if !cfg!(windows) {
            assert_eq!(shell_quote("it's"), "'it'\\''s'");
            assert_eq!(shell_quote(""), "''");
        }
        assert_eq!(
            expand("https://x/$file?b=$branch", &v, Quote::Url),
            "https://x/a%20b.txt?b=feature/x"
        );
    }

    #[test]
    fn form_values_become_variables() {
        let form = vec![
            FormField {
                name: "msg".into(),
                ..Default::default()
            },
            FormField {
                name: "force".into(),
                kind: FieldKind::Checkbox,
                on: "--force".into(),
                ..Default::default()
            },
        ];
        let mut v = form_variables(&form, &["hi there".into(), "true".into()]);
        assert_eq!(expand("$msg $force", &v, Quote::Plain), "hi there --force");
        v = form_variables(&form, &["x".into(), String::new()]);
        assert_eq!(expand("[$force]", &v, Quote::Shell), "[]");
    }

    #[test]
    fn script_kind_splits_before_expanding() {
        let cmd = CustomCommand {
            name: "Run".into(),
            kind: Kind::Script,
            command: "./tool --file $file".into(),
            ..Default::default()
        };
        assert_eq!(
            program(&cmd, &vars()).unwrap(),
            Program::Exec {
                program: "./tool".into(),
                args: vec!["--file".into(), "a b.txt".into()],
            }
        );
        let empty = CustomCommand::default();
        assert!(program(&empty, &vars()).is_err());
    }

    #[test]
    fn per_os_command_wins() {
        let mut cmd = CustomCommand {
            command: "generic".into(),
            ..Default::default()
        };
        cmd.linux = Some("linux".into());
        cmd.macos = Some("mac".into());
        cmd.windows = Some("win".into());
        let want = if cfg!(windows) {
            "win"
        } else if cfg!(target_os = "macos") {
            "mac"
        } else {
            "linux"
        };
        assert_eq!(cmd.text(), want);
        cmd.linux = Some("  ".into());
        cmd.macos = None;
        cmd.windows = None;
        assert_eq!(cmd.text(), "generic");
    }

    #[test]
    fn reads_the_shared_file() {
        let list = parse_shared(
            r#"{"commands": [{"name": "Open/CI", "scope": "commit", "kind": "url",
                "command": "https://ci/$SHA"},
                {"name": "Lint", "command": "make lint", "windows": "lint.cmd",
                 "form": [{"name": "fix", "kind": "checkbox", "on": "--fix"}]}]}"#,
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].scope, Scope::Commit);
        assert_eq!(list[0].kind, Kind::Url);
        assert_eq!(list[0].leaf(), "CI");
        assert_eq!(list[1].scope, Scope::Repository);
        assert_eq!(list[1].form[0].kind, FieldKind::Checkbox);
        assert_eq!(parse_shared("[]").unwrap().len(), 0);
        assert!(parse_shared("{nope").is_err());
        // What the editor exports reads back.
        assert_eq!(parse_shared(&to_json(&list)).unwrap(), list);
    }

    #[test]
    fn approval_changes_with_the_command() {
        let mut cmd = CustomCommand {
            command: "echo 1".into(),
            ..Default::default()
        };
        let a = approval_key(Path::new("/r"), &cmd);
        assert_eq!(a, approval_key(Path::new("/r"), &cmd));
        cmd.name = "renamed".into();
        assert_eq!(a, approval_key(Path::new("/r"), &cmd));
        cmd.command = "echo 2".into();
        assert_ne!(a, approval_key(Path::new("/r"), &cmd));
        assert_ne!(
            a,
            approval_key(
                Path::new("/other"),
                &CustomCommand {
                    command: "echo 1".into(),
                    ..Default::default()
                }
            )
        );
    }
}
