//! Repository statistics: when and by whom commits were made, and where.

use std::collections::HashMap;

use crate::Result;
use crate::cli::Git;
use crate::repo::{FS, RS};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorStats {
    pub name: String,
    pub email: String,
    pub commits: usize,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub commits: usize,
    /// Commits per weekday, Monday first, in each commit's own time zone.
    pub weekday: [usize; 7],
    pub hour: [usize; 24],
    /// Most commits first.
    pub authors: Vec<AuthorStats>,
    /// Lines changed per top-level folder (`""` for files at the root), largest first.
    pub folders: Vec<(String, usize)>,
}

/// Statistics of HEAD's history (or every branch with `all`), from `since`
/// (Unix seconds) on.
pub fn collect(git: &Git, since: Option<i64>, all: bool) -> Result<Stats> {
    let fmt = format!("--format={RS}%at{FS}%ai{FS}%aN{FS}%aE");
    let mut args = vec![
        "log".to_owned(),
        if all { "--all" } else { "HEAD" }.to_owned(),
        fmt,
        "--numstat".into(),
        "--no-renames".into(),
    ];
    if let Some(s) = since {
        args.push(format!("--since=@{s}"));
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git.run(&args)?;
    Ok(parse(&out))
}

pub(crate) fn parse(out: &str) -> Stats {
    let mut stats = Stats::default();
    let mut authors: HashMap<String, AuthorStats> = HashMap::new();
    let mut folders: HashMap<String, usize> = HashMap::new();
    for rec in out.split(RS).filter(|r| !r.trim().is_empty()) {
        let mut lines = rec.lines();
        let header = lines.next().unwrap_or_default();
        let f: Vec<&str> = header.splitn(4, FS).collect();
        let [time, date, name, email] = f.as_slice() else {
            continue;
        };
        let Ok(time) = time.parse::<i64>() else {
            continue;
        };
        let local = time + i64::from(crate::repo::parse_offset(date)) * 60;
        let days = local.div_euclid(86_400);
        // 1970-01-01 was a Thursday.
        stats.weekday[(days + 3).rem_euclid(7) as usize] += 1;
        stats.hour[(local.rem_euclid(86_400) / 3600) as usize] += 1;
        stats.commits += 1;
        let a = authors
            .entry(email.to_lowercase())
            .or_insert_with(|| AuthorStats {
                name: (*name).to_owned(),
                email: (*email).to_owned(),
                ..Default::default()
            });
        a.commits += 1;
        for l in lines {
            let mut parts = l.splitn(3, '\t');
            let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            // Binary files show `-`; count them as one changed line.
            let add = add.parse::<usize>().unwrap_or(0);
            let del = del.parse::<usize>().unwrap_or(0);
            a.added += add;
            a.removed += del;
            let folder = path.split_once('/').map_or("", |(f, _)| f);
            *folders.entry(folder.to_owned()).or_default() += (add + del).max(1);
        }
    }
    stats.authors = authors.into_values().collect();
    stats
        .authors
        .sort_by(|a, b| b.commits.cmp(&a.commits).then(a.name.cmp(&b.name)));
    stats.folders = folders.into_iter().collect();
    stats
        .folders
        .sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn parses_log_records() {
        // 2024-01-01 was a Monday; 23:30 at -03:00 is Tuesday 02:30 UTC.
        let out = format!(
            "{RS}1704162600{FS}2024-01-01 23:30:00 -0300{FS}Ann{FS}ann@x\n\n3\t1\tsrc/a.rs\n-\t-\timg.png\n\
             {RS}1704162600{FS}2024-01-02 02:30:00 +0000{FS}Bob{FS}bob@x\n\n1\t0\tsrc/b.rs\n\
             {RS}1704162600{FS}2024-01-02 02:30:00 +0000{FS}Ann{FS}ANN@x\n"
        );
        let s = parse(&out);
        assert_eq!(s.commits, 3);
        assert_eq!(s.weekday[0], 1, "Monday in Ann's zone");
        assert_eq!(s.weekday[1], 2);
        assert_eq!(s.hour[23], 1);
        assert_eq!(s.hour[2], 2);
        assert_eq!(s.authors[0].name, "Ann");
        assert_eq!(s.authors[0].commits, 2);
        assert_eq!(s.authors[0].added, 3);
        assert_eq!(s.folders, [("src".to_owned(), 5), (String::new(), 1)]);
    }

    #[test]
    fn collects_from_a_repository() {
        let t = TestRepo::new();
        t.commit("a/x.txt", "1\n2\n", "one");
        t.commit("b.txt", "1\n", "two");
        let s = collect(&t.git, None, false).unwrap();
        assert_eq!(s.commits, 2);
        assert_eq!(s.authors.len(), 1);
        assert_eq!(s.authors[0].added, 3);
        assert_eq!(s.folders[0], ("a".to_owned(), 2));
        let none = collect(&t.git, Some(i64::from(i32::MAX)), true).unwrap();
        assert_eq!(none.commits, 0);
    }
}
