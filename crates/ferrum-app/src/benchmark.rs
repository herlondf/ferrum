//! `ferrum --benchmark <repo> [runs]`: times each step of loading a
//! repository, to find what makes a big repository slow.

use std::path::Path;
use std::time::{Duration, Instant};

use ferrum_core::{Repo, graph};

use crate::tab::{COMMIT_LIMIT, RepoTab};

/// One timed step: name, time, and a count of what it loaded.
type Step = (&'static str, Duration, String);

fn time<T>(
    steps: &mut Vec<Step>,
    name: &'static str,
    f: impl FnOnce() -> T,
    count: impl Fn(&T) -> String,
) -> T {
    let started = Instant::now();
    let out = f();
    steps.push((name, started.elapsed(), count(&out)));
    out
}

fn len<T, E>(r: &Result<Vec<T>, E>) -> String {
    match r {
        Ok(v) => v.len().to_string(),
        Err(_) => "error".into(),
    }
}

/// Runs every reload step once.
pub fn run_once(path: &Path) -> Result<Vec<Step>, String> {
    let mut s = Vec::new();
    let repo =
        time(&mut s, "open", || Repo::open(path), |_| String::new()).map_err(|e| e.to_string())?;
    let _ = time(
        &mut s,
        "head",
        || (repo.head_branch(), repo.head_oid(), repo.state()),
        |_| String::new(),
    );
    let _ = time(&mut s, "branches", || repo.branches(), len);
    let _ = time(&mut s, "tags", || repo.tags(), len);
    let _ = time(&mut s, "remotes", || repo.remotes(), len);
    let _ = time(
        &mut s,
        "ref labels",
        || repo.ref_labels(),
        |r| r.as_ref().map(|m| m.len().to_string()).unwrap_or_default(),
    );
    let commits = time(&mut s, "commits", || repo.commits(COMMIT_LIMIT), len);
    let commits = commits.unwrap_or_default();
    let _ = time(
        &mut s,
        "graph layout",
        || graph::layout(&commits),
        |g| format!("{} lanes", g.width),
    );
    let _ = time(&mut s, "stashes", || repo.stashes(), len);
    let _ = time(&mut s, "submodules", || repo.submodules(), len);
    let _ = time(&mut s, "worktrees", || repo.worktrees(), len);
    let _ = time(&mut s, "lfs check", || repo.uses_lfs(), |b| b.to_string());
    let _ = time(&mut s, "identity", || repo.identity(), |_| String::new());
    let _ = time(
        &mut s,
        "status",
        || repo.status(),
        |r| {
            r.as_ref()
                .map(|w| format!("{} staged, {} unstaged", w.staged.len(), w.unstaged.len()))
                .unwrap_or_else(|_| "error".into())
        },
    );
    // What the app does on open and after operations, all steps together.
    let _ = time(
        &mut s,
        "full reload (app)",
        || RepoTab::open(path.to_path_buf()).is_ok(),
        |_| String::new(),
    );
    Ok(s)
}

/// Prints the time of each step over `runs` runs (best and average).
pub fn main(path: &Path, runs: usize) -> i32 {
    let runs = runs.max(1);
    let mut all: Vec<Vec<Step>> = Vec::new();
    for i in 0..runs {
        match run_once(path) {
            Ok(steps) => all.push(steps),
            Err(e) => {
                eprintln!("ferrum --benchmark: {e}");
                return 1;
            }
        }
        eprint!("\rrun {}/{runs}", i + 1);
    }
    eprintln!();
    println!("Repository: {}", path.display());
    println!("{:<20} {:>10} {:>10}  loaded", "step", "best", "average");
    for (i, (name, _, count)) in all[0].iter().enumerate() {
        let times: Vec<Duration> = all.iter().map(|r| r[i].1).collect();
        let best = times.iter().min().copied().unwrap_or_default();
        let avg = times.iter().sum::<Duration>() / times.len() as u32;
        println!(
            "{name:<20} {:>10} {:>10}  {count}",
            format!("{:.1} ms", best.as_secs_f64() * 1000.0),
            format!("{:.1} ms", avg.as_secs_f64() * 1000.0),
        );
    }
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn times_every_step() {
        let dir = tempfile::tempdir().unwrap();
        let git = ferrum_core::cli::Git::new(dir.path());
        git.run(&["init", "-q", "-b", "main"]).unwrap();
        let steps = super::run_once(dir.path()).unwrap();
        let names: Vec<_> = steps.iter().map(|s| s.0).collect();
        assert!(names.contains(&"commits"));
        assert!(names.contains(&"status"));
        assert_eq!(names.last(), Some(&"full reload (app)"));
    }
}
