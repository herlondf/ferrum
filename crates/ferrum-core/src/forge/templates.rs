//! Pull request templates kept in the repository.

use std::path::Path;

/// A template: its name (file name without `.md`) and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub body: String,
}

/// Single-file templates, in the order GitHub, GitLab, Gitea and Azure
/// DevOps look for them.
const FILES: &[&str] = &[
    ".github/pull_request_template.md",
    "pull_request_template.md",
    "docs/pull_request_template.md",
    ".gitea/pull_request_template.md",
    ".gitlab/merge_request_templates/default.md",
    ".azuredevops/pull_request_template.md",
    ".vsts/pull_request_template.md",
];

/// Folders with several templates.
const DIRS: &[&str] = &[
    ".github/PULL_REQUEST_TEMPLATE",
    "PULL_REQUEST_TEMPLATE",
    "docs/PULL_REQUEST_TEMPLATE",
    ".gitlab/merge_request_templates",
    ".gitea/PULL_REQUEST_TEMPLATE",
    ".azuredevops/pull_request_template/branches",
];

/// Finds the file named `rel` under `root`, ignoring the case of each part.
fn find_ci(root: &Path, rel: &str) -> Option<std::path::PathBuf> {
    let mut dir = root.to_path_buf();
    for part in rel.split('/') {
        let exact = dir.join(part);
        if exact.exists() {
            dir = exact;
            continue;
        }
        let entry = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))?;
        dir = entry.path();
    }
    Some(dir)
}

/// The templates of the repository at `workdir`; the default one first.
pub fn find(workdir: &Path) -> Vec<Template> {
    let mut out: Vec<Template> = Vec::new();
    let mut seen = Vec::new();
    let mut add = |path: std::path::PathBuf, out: &mut Vec<Template>| {
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            return;
        }
        if let Ok(body) = std::fs::read_to_string(&path) {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            seen.push(key);
            out.push(Template { name, body });
        }
    };
    for f in FILES {
        if let Some(p) = find_ci(workdir, f).filter(|p| p.is_file()) {
            add(p, &mut out);
        }
    }
    for d in DIRS {
        let Some(dir) = find_ci(workdir, d).filter(|p| p.is_dir()) else {
            continue;
        };
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("txt")
                    })
            })
            .collect();
        files.sort();
        for p in files {
            add(p, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_templates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert!(find(p).is_empty());
        std::fs::create_dir_all(p.join(".github/PULL_REQUEST_TEMPLATE")).unwrap();
        std::fs::write(p.join(".github/PULL_REQUEST_TEMPLATE.md"), "upper").unwrap();
        std::fs::write(p.join(".github/PULL_REQUEST_TEMPLATE/bug.md"), "bug").unwrap();
        std::fs::write(p.join(".github/PULL_REQUEST_TEMPLATE/feature.md"), "feat").unwrap();
        std::fs::write(p.join(".github/PULL_REQUEST_TEMPLATE/notes.png"), "x").unwrap();
        std::fs::create_dir_all(p.join(".gitlab/merge_request_templates")).unwrap();
        std::fs::write(p.join(".gitlab/merge_request_templates/Default.md"), "gl").unwrap();
        let names: Vec<String> = find(p).into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            ["PULL_REQUEST_TEMPLATE", "Default", "bug", "feature"]
        );
        assert_eq!(find(p)[0].body, "upper");
    }
}
