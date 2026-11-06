//! Editing `.gitignore`: patterns for a file, its folder or extension, and a
//! few small built-in templates.

use std::path::Path;

/// Built-in templates: (name, patterns).
pub const TEMPLATES: &[(&str, &str)] = &[
    ("Rust", "/target/\n**/*.rs.bk\n*.pdb\n"),
    (
        "Node",
        "node_modules/\nnpm-debug.log*\nyarn-debug.log*\nyarn-error.log*\n.pnpm-debug.log*\ndist/\ncoverage/\n.env\n.env.local\n",
    ),
    (
        "Python",
        "__pycache__/\n*.py[cod]\n*.egg-info/\n.eggs/\nbuild/\ndist/\n.venv/\nvenv/\n.pytest_cache/\n.mypy_cache/\n.coverage\n",
    ),
    (
        "Java",
        "*.class\n*.jar\n*.war\n*.ear\n*.log\ntarget/\nbuild/\n.gradle/\nout/\nhs_err_pid*\n",
    ),
    (
        "Go",
        "*.exe\n*.exe~\n*.dll\n*.so\n*.dylib\n*.test\n*.out\nvendor/\ngo.work\n",
    ),
    (
        ".NET",
        "[Bb]in/\n[Oo]bj/\n*.user\n*.suo\n*.userprefs\n.vs/\n[Dd]ebug/\n[Rr]elease/\nTestResults/\n*.nupkg\n",
    ),
    (
        "macOS",
        ".DS_Store\n.AppleDouble\n.LSOverride\n._*\n.Spotlight-V100\n.Trashes\n",
    ),
    (
        "Windows",
        "Thumbs.db\nehthumbs.db\nDesktop.ini\n$RECYCLE.BIN/\n*.lnk\n",
    ),
    ("Linux", "*~\n.fuse_hidden*\n.directory\n.Trash-*\n.nfs*\n"),
];

/// Pattern that ignores exactly this file.
pub fn file_pattern(path: &str) -> String {
    format!("/{path}")
}

/// Pattern for the folder that contains `path`, if it is not at the root.
pub fn folder_pattern(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    let (dir, _) = path.rsplit_once('/')?;
    Some(format!("/{dir}/"))
}

/// Pattern for every file with the extension of `path`.
pub fn extension_pattern(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| format!("*.{ext}"))
}

/// Adds patterns to the `.gitignore` at the repository root, skipping those
/// already there. Returns how many were added.
pub fn append(root: &Path, patterns: &str) -> std::io::Result<usize> {
    let file = root.join(".gitignore");
    let mut content = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let existing: Vec<String> = content.lines().map(|l| l.trim().to_owned()).collect();
    let new: Vec<&str> = patterns
        .lines()
        .map(str::trim)
        .filter(|p| !p.is_empty() && !existing.iter().any(|e| e == p))
        .collect();
    if new.is_empty() {
        return Ok(0);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for p in &new {
        content.push_str(p);
        content.push('\n');
    }
    std::fs::write(file, content)?;
    Ok(new.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_for_a_path() {
        assert_eq!(file_pattern("src/a.rs"), "/src/a.rs");
        assert_eq!(folder_pattern("src/gen/a.rs").as_deref(), Some("/src/gen/"));
        assert_eq!(folder_pattern("a.rs"), None);
        assert_eq!(extension_pattern("src/a.log").as_deref(), Some("*.log"));
        assert_eq!(extension_pattern(".env"), None);
        assert_eq!(extension_pattern("Makefile"), None);
    }

    #[test]
    fn appends_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "*.log").unwrap();
        assert_eq!(append(dir.path(), "*.log\n/target/\n").unwrap(), 1);
        assert_eq!(append(dir.path(), TEMPLATES[0].1).unwrap(), 2);
        let text = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(text, "*.log\n/target/\n**/*.rs.bk\n*.pdb\n");
    }
}
