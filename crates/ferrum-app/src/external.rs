//! Launching external programs: the editor or IDE, a terminal, the file
//! manager, and the external diff and merge tools.
//!
//! Views ask for a launch with `Command::External(Launch::…)`; the app runs
//! it with the tools chosen in Settings > Tools (`Settings::external`):
//!
//! ```ignore
//! cmds.push(Command::External(Launch::Editor { path: tab.path().join(file), line: Some(42) }));
//! cmds.push(Command::External(Launch::Terminal { dir: tab.path().to_owned() }));
//! cmds.push(Command::External(Launch::Reveal { path: tab.path().join(file) }));
//! // Two versions of a file, e.g. written with `temp_file` from blobs:
//! cmds.push(Command::External(Launch::Diff { left, right, cwd: tab.path().to_owned() }));
//! cmds.push(Command::External(Launch::Merge { repo: tab.path().to_owned(), path: file.into() }));
//! ```
//!
//! Every preset is a program (with fallbacks, e.g. `zed` then `zeditor`) and
//! an argument template. A custom command is split like a shell would
//! (quotes group words) and these placeholders are replaced in each word:
//! `$FILE`, `$LINE` (1 when there is none) and `$DIR` for editors, `$DIR` for
//! terminals, `$LOCAL` and `$REMOTE` for diff tools, plus `$BASE` and
//! `$MERGED` for merge tools. `${NAME}` works too.
//!
//! Launched programs are not waited for; their exit is collected on a
//! background thread.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};

/// Something to open in another program.
#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// Open a file (or folder) in the editor, optionally at a 1-based line.
    Editor { path: PathBuf, line: Option<u32> },
    /// Open a terminal in a folder.
    Terminal { dir: PathBuf },
    /// Show a file or folder in the file manager (selecting the file where possible).
    Reveal { path: PathBuf },
    /// Compare two files in the diff tool. `cwd` is where the tool starts (the repository).
    #[allow(dead_code)] // for the diff views (external diff)
    Diff {
        left: PathBuf,
        right: PathBuf,
        cwd: PathBuf,
    },
    /// Resolve a conflicted file (relative to `repo`) in the merge tool.
    Merge { repo: PathBuf, path: String },
}

/// The programs chosen in Settings > Tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExternalTools {
    pub editor: Editor,
    /// Used when `editor` is `Custom`.
    pub editor_command: String,
    pub terminal: Terminal,
    pub terminal_command: String,
    pub diff_tool: DiffTool,
    pub diff_command: String,
    pub merge_tool: DiffTool,
    pub merge_command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Editor {
    /// The application the system opens the file type with.
    #[default]
    System,
    VsCode,
    Zed,
    Sublime,
    JetBrains,
    /// Neovim inside the chosen terminal.
    Neovim,
    Cursor,
    Custom,
}

impl Editor {
    pub const ALL: [Editor; 8] = [
        Editor::System,
        Editor::VsCode,
        Editor::Zed,
        Editor::Sublime,
        Editor::JetBrains,
        Editor::Neovim,
        Editor::Cursor,
        Editor::Custom,
    ];

    pub fn label(self) -> String {
        match self {
            Editor::System => t!("System default"),
            Editor::VsCode => "Visual Studio Code".into(),
            Editor::Zed => "Zed".into(),
            Editor::Sublime => "Sublime Text".into(),
            Editor::JetBrains => t!("JetBrains IDE (IntelliJ, RustRover, PyCharm…)"),
            Editor::Neovim => t!("Neovim (in the terminal)"),
            Editor::Cursor => "Cursor".into(),
            Editor::Custom => t!("Custom command"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[allow(clippy::enum_variant_names)] // product names
pub enum Terminal {
    /// The first terminal found for this system.
    #[default]
    Auto,
    Gnome,
    Ptyxis,
    Konsole,
    Kitty,
    Alacritty,
    WezTerm,
    Ghostty,
    WindowsTerminal,
    Cmd,
    PowerShell,
    MacTerminal,
    ITerm,
    Custom,
}

impl Terminal {
    /// The choices that make sense on this system.
    pub fn choices() -> Vec<Terminal> {
        let mut list = vec![Terminal::Auto];
        if cfg!(windows) {
            list.extend([
                Terminal::WindowsTerminal,
                Terminal::Cmd,
                Terminal::PowerShell,
            ]);
        } else if cfg!(target_os = "macos") {
            list.extend([Terminal::MacTerminal, Terminal::ITerm]);
        } else {
            list.extend([Terminal::Gnome, Terminal::Ptyxis, Terminal::Konsole]);
        }
        list.extend([
            Terminal::Kitty,
            Terminal::Alacritty,
            Terminal::WezTerm,
            Terminal::Ghostty,
            Terminal::Custom,
        ]);
        list
    }

    pub fn label(self) -> String {
        match self {
            Terminal::Auto => t!("Automatic"),
            Terminal::Gnome => "GNOME Terminal".into(),
            Terminal::Ptyxis => "Ptyxis".into(),
            Terminal::Konsole => "Konsole".into(),
            Terminal::Kitty => "Kitty".into(),
            Terminal::Alacritty => "Alacritty".into(),
            Terminal::WezTerm => "WezTerm".into(),
            Terminal::Ghostty => "Ghostty".into(),
            Terminal::WindowsTerminal => "Windows Terminal".into(),
            Terminal::Cmd => t!("Command Prompt (cmd)"),
            Terminal::PowerShell => "PowerShell".into(),
            Terminal::MacTerminal => t!("Terminal (macOS)"),
            Terminal::ITerm => "iTerm2".into(),
            Terminal::Custom => t!("Custom command"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DiffTool {
    /// `git difftool` / `git mergetool`, as set up in git config.
    #[default]
    GitConfig,
    Meld,
    KDiff3,
    BeyondCompare,
    P4Merge,
    VsCode,
    Vimdiff,
    Custom,
}

impl DiffTool {
    pub const ALL: [DiffTool; 8] = [
        DiffTool::GitConfig,
        DiffTool::Meld,
        DiffTool::KDiff3,
        DiffTool::BeyondCompare,
        DiffTool::P4Merge,
        DiffTool::VsCode,
        DiffTool::Vimdiff,
        DiffTool::Custom,
    ];

    pub fn label(self) -> String {
        match self {
            DiffTool::GitConfig => t!("From git config (difftool / mergetool)"),
            DiffTool::Meld => "Meld".into(),
            DiffTool::KDiff3 => "KDiff3".into(),
            DiffTool::BeyondCompare => "Beyond Compare".into(),
            DiffTool::P4Merge => "P4Merge".into(),
            DiffTool::VsCode => "Visual Studio Code".into(),
            DiffTool::Vimdiff => t!("vimdiff (in the terminal)"),
            DiffTool::Custom => t!("Custom command"),
        }
    }
}

/// Example custom commands, shown as hints in Settings.
pub const EDITOR_HINT: &str = "code --goto $FILE:$LINE";
pub const TERMINAL_HINT: &str = "foot --working-directory $DIR";
pub const DIFF_HINT: &str = "meld $LOCAL $REMOTE";
pub const MERGE_HINT: &str = "meld --output $MERGED $LOCAL $BASE $REMOTE";

/// A program to start. `programs` are tried in order until one exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub programs: Vec<String>,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl Invocation {
    fn new(programs: &[&str], args: Vec<String>, cwd: Option<&Path>) -> Self {
        Self {
            programs: programs.iter().map(|p| (*p).to_owned()).collect(),
            args,
            cwd: cwd.map(Path::to_path_buf),
        }
    }

    /// The program and its arguments.
    #[cfg(test)]
    pub fn argv(&self) -> Vec<String> {
        let mut v = vec![self.programs.first().cloned().unwrap_or_default()];
        v.extend(self.args.iter().cloned());
        v
    }
}

/// Starts `what` with the configured tools.
pub fn launch(tools: &ExternalTools, what: &Launch) -> Result<(), String> {
    let inv = match what {
        Launch::Merge { repo, path } if tools.merge_tool != DiffTool::GitConfig => {
            let dir = scratch_dir("merge")?;
            let files = ferrum_core::config::write_conflict_files(repo, path, &dir)
                .map_err(|e| e.to_string())?;
            merge(tools, repo, &files)?
        }
        _ => plan(tools, what)?,
    };
    spawn(&inv)
}

/// What `launch` would run (merges with a preset get the conflict files from
/// `write_conflict_files` first; this plans `git mergetool`-style launches only).
pub fn plan(tools: &ExternalTools, what: &Launch) -> Result<Invocation, String> {
    match what {
        Launch::Editor { path, line } => editor(tools, path, *line),
        Launch::Terminal { dir } => terminal(tools, dir, &[]),
        Launch::Reveal { path } => Ok(reveal(path)),
        Launch::Diff { left, right, cwd } => diff(tools, left, right, cwd),
        Launch::Merge { repo, path } => {
            if tools.merge_tool == DiffTool::GitConfig {
                let git = ferrum_core::cli::program().display().to_string();
                let args = strings(&["mergetool", "--no-prompt", "--", path]);
                Ok(Invocation::new(&[&git], args, Some(repo)))
            } else {
                Err(t!("Write the conflict files first."))
            }
        }
    }
}

/// Values for the placeholders of a template.
#[derive(Default)]
struct Vars<'a> {
    file: Option<&'a Path>,
    line: Option<u32>,
    dir: Option<&'a Path>,
    local: Option<&'a Path>,
    remote: Option<&'a Path>,
    base: Option<&'a Path>,
    merged: Option<&'a Path>,
}

impl Vars<'_> {
    fn value(&self, name: &str) -> Option<String> {
        let p = |p: Option<&Path>| p.map(|p| p.display().to_string());
        match name {
            "FILE" => p(self.file),
            "LINE" => Some(self.line.unwrap_or(1).to_string()),
            "DIR" => p(self.dir),
            "LOCAL" => p(self.local),
            "REMOTE" => p(self.remote),
            "BASE" => p(self.base),
            "MERGED" => p(self.merged),
            _ => None,
        }
    }

    /// Replaces `$NAME` and `${NAME}` in one word; unknown names stay.
    fn fill(&self, word: &str) -> String {
        let mut out = String::new();
        let mut rest = word;
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
                    .find(|c: char| !c.is_ascii_uppercase())
                    .unwrap_or(after.len());
                (&after[..end], end)
            };
            match self.value(name) {
                Some(v) if len > 0 => {
                    out.push_str(&v);
                    rest = &after[len..];
                }
                _ => {
                    out.push('$');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

/// Splits a command line into words: whitespace separates, quotes group,
/// a backslash escapes the next character inside double quotes.
pub fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                cur.extend(chars.next());
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            (None, c) => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    words
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// A preset: fallback programs and an argument template.
fn preset(programs: &[&str], args: &[&str], vars: &Vars, cwd: Option<&Path>) -> Invocation {
    Invocation::new(programs, args.iter().map(|a| vars.fill(a)).collect(), cwd)
}

/// A custom command from Settings.
fn custom(command: &str, vars: &Vars, cwd: Option<&Path>) -> Result<Invocation, String> {
    let mut words = split_command(command).into_iter().map(|w| vars.fill(&w));
    let program = words
        .next()
        .ok_or_else(|| t!("Enter a custom command in Settings > Tools first."))?;
    Ok(Invocation {
        programs: vec![program],
        args: words.collect(),
        cwd: cwd.map(Path::to_path_buf),
    })
}

fn editor(tools: &ExternalTools, path: &Path, line: Option<u32>) -> Result<Invocation, String> {
    let dir = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    let vars = Vars {
        file: Some(path),
        line,
        dir,
        ..Default::default()
    };
    let at_line = line.is_some();
    let goto = |programs: &[&str], flag: Option<&str>| {
        let mut args: Vec<&str> = Vec::new();
        if at_line {
            args.extend(flag);
            args.push("$FILE:$LINE");
        } else {
            args.push("$FILE");
        }
        preset(programs, &args, &vars, dir)
    };
    Ok(match tools.editor {
        Editor::System => open_with_system(path),
        Editor::VsCode => goto(&["code"], Some("--goto")),
        Editor::Cursor => goto(&["cursor"], Some("--goto")),
        Editor::Zed => goto(&["zed", "zeditor", "zedit"], None),
        Editor::Sublime => goto(&["subl", "sublime_text"], None),
        Editor::JetBrains => {
            let args: &[&str] = if at_line {
                &["--line", "$LINE", "$FILE"]
            } else {
                &["$FILE"]
            };
            preset(JETBRAINS, args, &vars, dir)
        }
        Editor::Neovim => {
            let mut cmd = vec!["nvim".to_owned()];
            if let Some(l) = line {
                cmd.push(format!("+{l}"));
            }
            cmd.push(path.display().to_string());
            return terminal(tools, dir.unwrap_or(path), &cmd);
        }
        Editor::Custom => return custom(&tools.editor_command, &vars, dir),
    })
}

/// JetBrains launcher scripts (Toolbox and standalone installs).
const JETBRAINS: &[&str] = &[
    "idea",
    "rustrover",
    "pycharm",
    "webstorm",
    "clion",
    "goland",
    "phpstorm",
    "rider",
    "rubymine",
    "idea64",
    "pycharm64",
];

fn open_with_system(path: &Path) -> Invocation {
    let p = path.display().to_string();
    if cfg!(windows) {
        Invocation::new(&["explorer"], vec![p], None)
    } else if cfg!(target_os = "macos") {
        Invocation::new(&["open"], vec![p], None)
    } else {
        Invocation::new(&["xdg-open", "gio"], vec![p], None)
    }
}

/// A terminal in `dir`, running `cmd` when it is not empty.
fn terminal(tools: &ExternalTools, dir: &Path, cmd: &[String]) -> Result<Invocation, String> {
    let vars = Vars {
        dir: Some(dir),
        ..Default::default()
    };
    let kind = match tools.terminal {
        Terminal::Auto => auto_terminal(),
        Terminal::Custom => {
            let mut inv = custom(&tools.terminal_command, &vars, Some(dir))?;
            if !cmd.is_empty() {
                inv.args.push("-e".into());
                inv.args.extend(cmd.iter().cloned());
            }
            return Ok(inv);
        }
        t => t,
    };
    let d = dir.display().to_string();
    let run = !cmd.is_empty();
    // (programs, arguments for the folder, the flag that comes before a command)
    let (programs, mut args, exec): (&[&str], Vec<String>, Option<&str>) = match kind {
        Terminal::Gnome => (
            &["gnome-terminal"],
            vec![format!("--working-directory={d}")],
            Some("--"),
        ),
        Terminal::Ptyxis => (
            &["ptyxis"],
            vec!["--new-window".into(), format!("--working-directory={d}")],
            Some("--"),
        ),
        Terminal::Konsole => (&["konsole"], strings(&["--workdir", &d]), Some("-e")),
        Terminal::Kitty => (&["kitty"], strings(&["--directory", &d]), None),
        Terminal::Alacritty => (
            &["alacritty"],
            strings(&["--working-directory", &d]),
            Some("-e"),
        ),
        Terminal::WezTerm => (&["wezterm"], strings(&["start", "--cwd", &d]), Some("--")),
        Terminal::Ghostty => (
            &["ghostty"],
            vec![format!("--working-directory={d}")],
            Some("-e"),
        ),
        Terminal::WindowsTerminal => (&["wt"], strings(&["-d", &d]), None),
        Terminal::Cmd => {
            let mut args = strings(&["/C", "start", "", "cmd"]);
            if run {
                args.push("/K".into());
                args.extend(cmd.iter().cloned());
            }
            return Ok(Invocation::new(&["cmd"], args, Some(dir)));
        }
        Terminal::PowerShell => {
            let mut args = strings(&["/C", "start", "", "powershell", "-NoExit"]);
            if run {
                args.push("-Command".into());
                args.extend(cmd.iter().cloned());
            }
            return Ok(Invocation::new(&["cmd"], args, Some(dir)));
        }
        Terminal::MacTerminal | Terminal::ITerm => {
            let app = if kind == Terminal::ITerm {
                "iTerm"
            } else {
                "Terminal"
            };
            if !run {
                return Ok(Invocation::new(&["open"], strings(&["-a", app, &d]), None));
            }
            let line = format!(
                "cd {} && {}",
                sh_quote(&d),
                cmd.iter()
                    .map(|w| sh_quote(w))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let script = if kind == Terminal::ITerm {
                format!(
                    "tell application \"iTerm\" to create window with default profile command \"/bin/sh -c {}\"",
                    applescript_quote(&sh_quote(&line))
                )
            } else {
                format!(
                    "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
                    applescript_quote(&line)
                )
            };
            return Ok(Invocation::new(
                &["osascript"],
                strings(&["-e", &script]),
                None,
            ));
        }
        // Linux fallback when nothing better is installed.
        _ => (&["xterm"], Vec::new(), Some("-e")),
    };
    if run {
        args.extend(exec.map(str::to_owned));
        args.extend(cmd.iter().cloned());
    }
    Ok(Invocation::new(programs, args, Some(dir)))
}

/// The terminal to use when the setting is Automatic.
fn auto_terminal() -> Terminal {
    if cfg!(windows) {
        if find_program("wt").is_some() {
            Terminal::WindowsTerminal
        } else {
            Terminal::Cmd
        }
    } else if cfg!(target_os = "macos") {
        Terminal::MacTerminal
    } else {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();
        let mut order = vec![
            ("ptyxis", Terminal::Ptyxis),
            ("gnome-terminal", Terminal::Gnome),
            ("konsole", Terminal::Konsole),
        ];
        if desktop.contains("kde") {
            order.rotate_left(2);
        }
        order.extend([
            ("ghostty", Terminal::Ghostty),
            ("kitty", Terminal::Kitty),
            ("wezterm", Terminal::WezTerm),
            ("alacritty", Terminal::Alacritty),
        ]);
        order
            .into_iter()
            .find(|(p, _)| find_program(p).is_some())
            .map(|(_, t)| t)
            .unwrap_or(Terminal::Custom)
    }
}

fn reveal(path: &Path) -> Invocation {
    let p = path.display().to_string();
    if path.is_dir() {
        return open_with_system(path);
    }
    if cfg!(windows) {
        Invocation::new(&["explorer"], vec![format!("/select,{p}")], None)
    } else if cfg!(target_os = "macos") {
        Invocation::new(&["open"], strings(&["-R", &p]), None)
    } else {
        // The FileManager1 D-Bus interface selects the file (Nautilus, Dolphin, Nemo...).
        let uri = file_uri(path);
        Invocation::new(
            &["dbus-send", "gdbus"],
            strings(&[
                "--session",
                "--type=method_call",
                "--dest=org.freedesktop.FileManager1",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ]),
            None,
        )
    }
}

/// `file://` URI with the unsafe characters escaped.
fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.display().to_string().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn diff(
    tools: &ExternalTools,
    left: &Path,
    right: &Path,
    cwd: &Path,
) -> Result<Invocation, String> {
    let vars = Vars {
        local: Some(left),
        remote: Some(right),
        dir: Some(cwd),
        ..Default::default()
    };
    let lr = ["$LOCAL", "$REMOTE"];
    Ok(match tools.diff_tool {
        DiffTool::GitConfig => {
            let git = ferrum_core::cli::program().display().to_string();
            preset(
                &[&git],
                &[
                    "difftool",
                    "--no-prompt",
                    "--no-index",
                    "--",
                    "$LOCAL",
                    "$REMOTE",
                ],
                &vars,
                Some(cwd),
            )
        }
        DiffTool::Meld => preset(MELD, &lr, &vars, Some(cwd)),
        DiffTool::KDiff3 => preset(KDIFF3, &lr, &vars, Some(cwd)),
        DiffTool::BeyondCompare => preset(BCOMPARE, &lr, &vars, Some(cwd)),
        DiffTool::P4Merge => preset(P4MERGE, &lr, &vars, Some(cwd)),
        DiffTool::VsCode => preset(
            &["code"],
            &["--wait", "--diff", "$LOCAL", "$REMOTE"],
            &vars,
            Some(cwd),
        ),
        DiffTool::Vimdiff => {
            let cmd = vec!["vimdiff".into(), vars.fill("$LOCAL"), vars.fill("$REMOTE")];
            return terminal(tools, cwd, &cmd);
        }
        DiffTool::Custom => return custom(&tools.diff_command, &vars, Some(cwd)),
    })
}

fn merge(
    tools: &ExternalTools,
    repo: &Path,
    files: &ferrum_core::config::ConflictFiles,
) -> Result<Invocation, String> {
    let vars = Vars {
        local: Some(&files.local),
        remote: Some(&files.remote),
        base: Some(&files.base),
        merged: Some(&files.merged),
        dir: Some(repo),
        ..Default::default()
    };
    let cwd = Some(repo);
    Ok(match tools.merge_tool {
        DiffTool::GitConfig => {
            let rel = files.merged.strip_prefix(repo).unwrap_or(&files.merged);
            let path = rel.to_string_lossy().replace('\\', "/");
            plan(
                tools,
                &Launch::Merge {
                    repo: repo.into(),
                    path,
                },
            )?
        }
        DiffTool::Meld => preset(
            MELD,
            &["--output", "$MERGED", "$LOCAL", "$BASE", "$REMOTE"],
            &vars,
            cwd,
        ),
        DiffTool::KDiff3 => preset(
            KDIFF3,
            &["--auto", "$BASE", "$LOCAL", "$REMOTE", "-o", "$MERGED"],
            &vars,
            cwd,
        ),
        DiffTool::BeyondCompare => preset(
            BCOMPARE,
            &["$LOCAL", "$REMOTE", "$BASE", "$MERGED"],
            &vars,
            cwd,
        ),
        DiffTool::P4Merge => preset(
            P4MERGE,
            &["$BASE", "$LOCAL", "$REMOTE", "$MERGED"],
            &vars,
            cwd,
        ),
        DiffTool::VsCode => preset(
            &["code"],
            &["--wait", "--merge", "$REMOTE", "$LOCAL", "$BASE", "$MERGED"],
            &vars,
            cwd,
        ),
        DiffTool::Vimdiff => {
            let cmd: Vec<String> = [
                "vimdiff",
                "-c",
                "4wincmd w | wincmd J",
                "$LOCAL",
                "$BASE",
                "$REMOTE",
                "$MERGED",
            ]
            .iter()
            .map(|a| vars.fill(a))
            .collect();
            return terminal(tools, repo, &cmd);
        }
        DiffTool::Custom => return custom(&tools.merge_command, &vars, cwd),
    })
}

// Where the tools are installed when they are not on the PATH.
const MELD: &[&str] = &[
    "meld",
    r"C:\Program Files\Meld\Meld.exe",
    r"C:\Program Files (x86)\Meld\Meld.exe",
    "/Applications/Meld.app/Contents/MacOS/Meld",
];
const KDIFF3: &[&str] = &[
    "kdiff3",
    r"C:\Program Files\KDiff3\kdiff3.exe",
    r"C:\Program Files\KDiff3\bin\kdiff3.exe",
    "/Applications/kdiff3.app/Contents/MacOS/kdiff3",
];
const BCOMPARE: &[&str] = &[
    "bcompare",
    "BComp",
    r"C:\Program Files\Beyond Compare 5\BComp.exe",
    r"C:\Program Files\Beyond Compare 4\BComp.exe",
];
const P4MERGE: &[&str] = &[
    "p4merge",
    r"C:\Program Files\Perforce\p4merge.exe",
    "/Applications/p4merge.app/Contents/Resources/launchp4merge",
];

/// Quotes a word for `/bin/sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn applescript_quote(s: &str) -> String {
    s.replace('\\', r"\\").replace('"', "\\\"")
}

/// A fresh folder under the temp dir for files handed to a tool.
fn scratch_dir(kind: &str) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("ferrum")
        .join(format!("{kind}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Writes `data` to a new temporary file named `name` (e.g. "main.rs" or
/// "main.abc1234.rs"), for handing a version of a file to a diff tool.
#[allow(dead_code)] // for the diff views (external diff of a commit)
pub fn temp_file(name: &str, data: &[u8]) -> Result<PathBuf, String> {
    let name = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let path = scratch_dir("diff")?.join(name);
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Finds a program: an existing path, or a name on the PATH (with the
/// Windows extensions from PATHEXT).
pub fn find_program(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() > 1 {
        return p.is_file().then(|| p.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        let plain = dir.join(name);
        if (!cfg!(windows) || p.extension().is_some()) && plain.is_file() {
            return Some(plain);
        }
        exts.iter()
            .map(|e| dir.join(format!("{name}{e}")))
            .find(|f| f.is_file())
    })
}

/// Starts the first program of `inv` that exists and collects its exit in the background.
pub fn spawn(inv: &Invocation) -> Result<(), String> {
    let found = inv.programs.iter().find_map(|p| find_program(p));
    let Some(program) = found else {
        let name = inv.programs.first().cloned().unwrap_or_default();
        return Err(t!(
            "{} was not found. Install it or choose another program in Settings > Tools.",
            name
        ));
    };
    let mut cmd = std::process::Command::new(&program);
    cmd.args(&inv.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = &inv.cwd
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        // Batch launchers (code.cmd) would flash a console window.
        use std::os::windows::process::CommandExt;
        let ext = program
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext == "cmd" || ext == "bat" {
            cmd.creation_flags(0x0800_0000);
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> ExternalTools {
        ExternalTools::default()
    }

    #[test]
    fn splits_commands_like_a_shell() {
        assert_eq!(
            split_command("code  --goto $FILE"),
            ["code", "--goto", "$FILE"]
        );
        assert_eq!(
            split_command(r#""C:\Program Files\x.exe" -a 'b c' "d \"e\"" """#),
            [r"C:\Program Files\x.exe", "-a", "b c", "d \"e\"", ""]
        );
        assert!(split_command("   ").is_empty());
    }

    #[test]
    fn fills_placeholders() {
        let f = PathBuf::from("/r/my file.rs");
        let v = Vars {
            file: Some(&f),
            line: Some(7),
            ..Default::default()
        };
        assert_eq!(v.fill("$FILE:$LINE"), "/r/my file.rs:7");
        assert_eq!(v.fill("${LINE}x"), "7x");
        assert_eq!(v.fill("$HOME $1 $"), "$HOME $1 $");
        assert_eq!(Vars::default().fill("+$LINE"), "+1");
    }

    #[test]
    fn plans_editor_presets() {
        let mut t = tools();
        let file = PathBuf::from("/repo/src/main.rs");
        let at = |t: &ExternalTools, line| {
            plan(
                t,
                &Launch::Editor {
                    path: file.clone(),
                    line,
                },
            )
            .unwrap()
            .argv()
        };
        t.editor = Editor::VsCode;
        assert_eq!(at(&t, Some(3)), ["code", "--goto", "/repo/src/main.rs:3"]);
        assert_eq!(at(&t, None), ["code", "/repo/src/main.rs"]);
        t.editor = Editor::Zed;
        assert_eq!(at(&t, Some(3)), ["zed", "/repo/src/main.rs:3"]);
        t.editor = Editor::JetBrains;
        assert_eq!(
            at(&t, Some(3)),
            ["idea", "--line", "3", "/repo/src/main.rs"]
        );
        t.editor = Editor::Neovim;
        t.terminal = Terminal::Kitty;
        assert_eq!(
            at(&t, Some(3)),
            [
                "kitty",
                "--directory",
                "/repo/src",
                "nvim",
                "+3",
                "/repo/src/main.rs"
            ]
        );
        t.editor = Editor::Custom;
        t.editor_command = "my-ed -l $LINE \"$FILE\"".into();
        assert_eq!(at(&t, None), ["my-ed", "-l", "1", "/repo/src/main.rs"]);
        t.editor_command.clear();
        assert!(
            plan(
                &t,
                &Launch::Editor {
                    path: file,
                    line: None
                }
            )
            .is_err()
        );
    }

    #[test]
    fn plans_terminals() {
        let mut t = tools();
        let dir = PathBuf::from("/my repo");
        let open = |t: &ExternalTools| plan(t, &Launch::Terminal { dir: dir.clone() }).unwrap();
        t.terminal = Terminal::Gnome;
        assert_eq!(
            open(&t).argv(),
            ["gnome-terminal", "--working-directory=/my repo"]
        );
        assert_eq!(open(&t).cwd.as_deref(), Some(dir.as_path()));
        t.terminal = Terminal::WezTerm;
        assert_eq!(open(&t).argv(), ["wezterm", "start", "--cwd", "/my repo"]);
        t.terminal = Terminal::Custom;
        t.terminal_command = "foot -D $DIR".into();
        assert_eq!(open(&t).argv(), ["foot", "-D", "/my repo"]);
        // Commands run through the terminal's exec flag.
        t.terminal = Terminal::Alacritty;
        let inv = terminal(&t, &dir, &strings(&["vimdiff", "a", "b"])).unwrap();
        assert_eq!(
            inv.argv(),
            [
                "alacritty",
                "--working-directory",
                "/my repo",
                "-e",
                "vimdiff",
                "a",
                "b"
            ]
        );
    }

    #[test]
    fn plans_diff_and_merge_tools() {
        let mut t = tools();
        let (l, r, cwd) = (
            PathBuf::from("/t/a.LOCAL.rs"),
            PathBuf::from("/t/a.REMOTE.rs"),
            PathBuf::from("/repo"),
        );
        let d = |t: &ExternalTools| {
            plan(
                t,
                &Launch::Diff {
                    left: l.clone(),
                    right: r.clone(),
                    cwd: cwd.clone(),
                },
            )
            .unwrap()
            .argv()
        };
        assert_eq!(
            d(&t)[1..],
            [
                "difftool",
                "--no-prompt",
                "--no-index",
                "--",
                "/t/a.LOCAL.rs",
                "/t/a.REMOTE.rs"
            ]
        );
        t.diff_tool = DiffTool::Meld;
        assert_eq!(d(&t), ["meld", "/t/a.LOCAL.rs", "/t/a.REMOTE.rs"]);
        t.diff_tool = DiffTool::VsCode;
        assert_eq!(
            d(&t),
            [
                "code",
                "--wait",
                "--diff",
                "/t/a.LOCAL.rs",
                "/t/a.REMOTE.rs"
            ]
        );
        t.diff_tool = DiffTool::Custom;
        t.diff_command = "difft $LOCAL $REMOTE".into();
        assert_eq!(d(&t), ["difft", "/t/a.LOCAL.rs", "/t/a.REMOTE.rs"]);

        let files = ferrum_core::config::ConflictFiles {
            base: "/t/B".into(),
            local: "/t/L".into(),
            remote: "/t/R".into(),
            merged: "/repo/a.rs".into(),
        };
        let m = |t: &ExternalTools| merge(t, &cwd, &files).unwrap().argv();
        t.merge_tool = DiffTool::Meld;
        assert_eq!(
            m(&t),
            ["meld", "--output", "/repo/a.rs", "/t/L", "/t/B", "/t/R"]
        );
        t.merge_tool = DiffTool::KDiff3;
        assert_eq!(
            m(&t),
            [
                "kdiff3",
                "--auto",
                "/t/B",
                "/t/L",
                "/t/R",
                "-o",
                "/repo/a.rs"
            ]
        );
        t.merge_tool = DiffTool::P4Merge;
        assert_eq!(m(&t), ["p4merge", "/t/B", "/t/L", "/t/R", "/repo/a.rs"]);
        t.merge_tool = DiffTool::BeyondCompare;
        assert_eq!(m(&t), ["bcompare", "/t/L", "/t/R", "/t/B", "/repo/a.rs"]);
        t.merge_tool = DiffTool::Custom;
        t.merge_command = "tool $BASE $MERGED".into();
        assert_eq!(m(&t), ["tool", "/t/B", "/repo/a.rs"]);
        t.merge_tool = DiffTool::GitConfig;
        let inv = plan(
            &t,
            &Launch::Merge {
                repo: cwd.clone(),
                path: "a.rs".into(),
            },
        )
        .unwrap();
        assert_eq!(inv.argv()[1..], ["mergetool", "--no-prompt", "--", "a.rs"]);
    }

    #[test]
    fn reveals_files() {
        let inv = reveal(Path::new("/no such/dir/a b.txt"));
        if cfg!(all(unix, not(target_os = "macos"))) {
            assert!(
                inv.args
                    .contains(&"array:string:file:///no%20such/dir/a%20b.txt".to_owned())
            );
        }
    }

    #[test]
    fn reports_missing_programs() {
        let inv = Invocation::new(&["ferrum-no-such-program"], Vec::new(), None);
        let err = spawn(&inv).unwrap_err();
        assert!(err.contains("ferrum-no-such-program"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn runs_custom_commands() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let file = dir.path().join("my file.txt");
        std::fs::write(&file, "x").unwrap();
        let t = ExternalTools {
            editor: Editor::Custom,
            editor_command: format!(
                "sh -c 'printf \"%s:%s\" \"$1\" \"$2\" > {}' sh $FILE $LINE",
                out.display()
            ),
            ..Default::default()
        };
        launch(
            &t,
            &Launch::Editor {
                path: file.clone(),
                line: Some(9),
            },
        )
        .unwrap();
        for _ in 0..200 {
            if let Ok(s) = std::fs::read_to_string(&out)
                && !s.is_empty()
            {
                assert_eq!(s, format!("{}:9", file.display()));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the command did not run");
    }

    #[test]
    fn writes_temp_files() {
        let p = temp_file("src/lib.rs", b"hi").unwrap();
        assert!(p.ends_with("lib.rs"));
        assert_eq!(std::fs::read(&p).unwrap(), b"hi");
    }
}
