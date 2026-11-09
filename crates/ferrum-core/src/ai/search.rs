//! Natural-language history search: a request such as "commits that
//! touched login last month by Ana" becomes git log filters. A model can
//! do it ([`prompt`], then [`from_json`]); [`parse`] handles simple
//! phrases (English and Portuguese) without one.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Prompt;
use super::json::text_field;
use crate::cli::Git;
use crate::{Oid, Result};

/// Filters for `git log`; an empty field is not used.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filters {
    /// Author name or email (part of it).
    pub author: String,
    /// First day, `YYYY-MM-DD` (any date git understands also works).
    pub since: String,
    /// Last day, included.
    pub until: String,
    /// A path, folder or pathspec glob such as `*login*`.
    pub path: String,
    /// Words of the commit message (all must be there).
    pub message: String,
    /// Text the commits add or remove (`git log -S`).
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Author,
    Since,
    Until,
    Path,
    Message,
    Content,
}

impl Field {
    pub const ALL: [Field; 6] = [
        Field::Author,
        Field::Since,
        Field::Until,
        Field::Path,
        Field::Message,
        Field::Content,
    ];
}

impl Filters {
    pub fn get(&self, f: Field) -> &str {
        match f {
            Field::Author => &self.author,
            Field::Since => &self.since,
            Field::Until => &self.until,
            Field::Path => &self.path,
            Field::Message => &self.message,
            Field::Content => &self.content,
        }
    }

    pub fn get_mut(&mut self, f: Field) -> &mut String {
        match f {
            Field::Author => &mut self.author,
            Field::Since => &mut self.since,
            Field::Until => &mut self.until,
            Field::Path => &mut self.path,
            Field::Message => &mut self.message,
            Field::Content => &mut self.content,
        }
    }

    pub fn is_empty(&self) -> bool {
        Field::ALL.iter().all(|f| self.get(*f).trim().is_empty())
    }

    /// Arguments of `git log` listing the matching commits.
    pub fn log_args(&self, all: bool) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "log".into(),
            "--format=%H".into(),
            "--regexp-ignore-case".into(),
            "--fixed-strings".into(),
            if all { "--all" } else { "HEAD" }.into(),
        ];
        let author = self.author.trim();
        if !author.is_empty() {
            args.push(format!("--author={author}"));
        }
        let since = self.since.trim();
        if !since.is_empty() {
            match Day::parse(since) {
                Some(d) => args.push(format!("--since={} 00:00:00", d.format())),
                None => args.push(format!("--since={since}")),
            }
        }
        let until = self.until.trim();
        if !until.is_empty() {
            match Day::parse(until) {
                Some(d) => args.push(format!("--until={} 23:59:59", d.format())),
                None => args.push(format!("--until={until}")),
            }
        }
        let words = message_words(&self.message);
        if words.len() > 1 {
            args.push("--all-match".into());
        }
        for w in words {
            args.push(format!("--grep={w}"));
        }
        let content = self.content.trim();
        if !content.is_empty() {
            args.push(format!("-S{content}"));
        }
        let path = self.path.trim();
        if !path.is_empty() {
            args.push("--".into());
            args.push(path.trim_start_matches('/').to_owned());
        }
        args
    }
}

/// `fix "login page"` -> `fix`, `login page`.
fn message_words(message: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, part) in message.split('"').enumerate() {
        if i % 2 == 1 {
            if !part.trim().is_empty() {
                out.push(part.trim().to_owned());
            }
        } else {
            out.extend(part.split_whitespace().map(str::to_owned));
        }
    }
    out
}

/// The commits matching `filters` (all branches, or `HEAD`).
pub fn run(git: &Git, filters: &Filters, all: bool) -> Result<Vec<Oid>> {
    let args = filters.log_args(all);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git.run(&refs)?;
    Ok(out
        .lines()
        .filter_map(|l| Oid::from_str(l.trim()).ok())
        .collect())
}

// ----- Dates -----

/// A calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// Today in the local time zone.
    pub fn today() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let local = secs + i64::from(crate::local_offset_minutes()) * 60;
        Self::from_days(local.div_euclid(86_400))
    }

    /// The day `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Self {
        // Howard Hinnant's civil_from_days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }

    pub fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.to_days() + n)
    }

    pub fn add_months(self, n: i32) -> Self {
        let total = self.year * 12 + self.month as i32 - 1 + n;
        let (year, month) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
        let day = self.day.min(days_in_month(year, month));
        Self { year, month, day }
    }

    /// 0 is Monday.
    pub fn weekday(self) -> i64 {
        (self.to_days() + 3).rem_euclid(7)
    }

    pub fn format(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `YYYY-MM-DD` (or with `/`, or followed by a time), `YYYY-MM`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let date = s.split(['T', ' ']).next()?;
        let parts: Vec<&str> = date.split(['-', '/']).collect();
        let num = |p: &str| p.parse::<u32>().ok();
        let (year, month, day) = match parts.as_slice() {
            [y, m, d] if y.len() == 4 => (num(y)?, num(m)?, num(d)?),
            [y, m] if y.len() == 4 => (num(y)?, num(m)?, 1),
            _ => return None,
        };
        let year = year as i32;
        if !(1970..=2200).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
        {
            return None;
        }
        Some(Self { year, month, day })
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// ----- With a model -----

/// Asks a model to turn `query` into filters (answer read by [`from_json`]).
pub fn prompt(query: &str, today: Day) -> Prompt {
    let system = format!(
        "You turn a request to search a git history into filters. Today is {} ({}).\n\
Reply with one JSON object and nothing else. Keys, all optional:\n\
- \"author\": a name or email\n\
- \"since\", \"until\": first and last day, \"YYYY-MM-DD\", both included\n\
- \"path\": a file or folder path, or a glob such as \"*login*\" for files whose path contains a word\n\
- \"message\": words of the commit message\n\
- \"content\": code or text that the commits added or removed\n\
Leave out what the request does not ask for; never invent values.\n\
Example: \"commits that touched login last month by Ana\" -> \
{{\"author\": \"Ana\", \"since\": \"{}\", \"until\": \"{}\", \"path\": \"*login*\"}}",
        today.format(),
        WEEKDAYS[today.weekday() as usize],
        today.add_months(-1).format(),
        today.format(),
    );
    Prompt::new(system, query.trim())
}

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// What a model answered, checked: dates must be real dates, paths are
/// made relative, empty and unknown values are dropped. The second value
/// lists the fields that were dropped as invalid.
pub fn from_json(v: &Value) -> (Filters, Vec<String>) {
    let mut f = Filters::default();
    let mut dropped = Vec::new();
    let v = match v {
        // Some models wrap it: {"filters": {...}} or [{...}].
        Value::Array(a) => a.first().cloned().unwrap_or(Value::Null),
        Value::Object(o) if o.len() == 1 && o.values().all(Value::is_object) => {
            o.values().next().cloned().unwrap_or(Value::Null)
        }
        other => other.clone(),
    };
    let field = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| match v.get(*k) {
            Some(Value::Array(a)) => a.iter().find_map(|x| x.as_str().map(str::to_owned)),
            Some(_) => text_field(&v, k),
            None => None,
        })
    };
    let clean = |s: String| -> String {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        s.trim().chars().take(200).collect()
    };
    if let Some(a) = field(&["author", "by", "committer"]) {
        f.author = clean(a);
    }
    for (keys, slot, name) in [
        (
            &["since", "after", "from", "start"][..],
            &mut f.since,
            "since",
        ),
        (&["until", "before", "to", "end"][..], &mut f.until, "until"),
    ] {
        if let Some(d) = field(keys) {
            match Day::parse(&d) {
                Some(day) => *slot = day.format(),
                None => dropped.push(format!("{name}: {d}")),
            }
        }
    }
    if let (Some(a), Some(b)) = (Day::parse(&f.since), Day::parse(&f.until))
        && a > b
    {
        std::mem::swap(&mut f.since, &mut f.until);
    }
    if let Some(p) = field(&["path", "paths", "file", "files", "folder"]) {
        let p = clean(p);
        if p.contains("..") {
            dropped.push(format!("path: {p}"));
        } else {
            f.path = pathspec(p.trim_start_matches(['/', '\\']).trim_start_matches("./"));
        }
    }
    if let Some(m) = field(&["message", "grep", "subject", "keywords"]) {
        f.message = clean(m);
    }
    if let Some(c) = field(&["content", "code", "text", "pickaxe", "diff"]) {
        f.content = clean(c);
    }
    (f, dropped)
}

/// A bare word (no folder, extension or wildcard) matches every path
/// containing it.
fn pathspec(p: &str) -> String {
    let p = p.trim();
    if p.is_empty() || p.contains(['/', '.', '*', '?', '[', ':', '\\']) {
        return p.replace('\\', "/");
    }
    format!("*{p}*")
}

// ----- Without a model -----

struct Token {
    text: String,
    lower: String,
    quoted: bool,
}

fn tokens(query: &str) -> Vec<Token> {
    let mut out = Vec::new();
    for (i, part) in query.split('"').enumerate() {
        if i % 2 == 1 {
            if !part.trim().is_empty() {
                out.push(Token {
                    text: part.trim().to_owned(),
                    lower: part.trim().to_lowercase(),
                    quoted: true,
                });
            }
            continue;
        }
        for w in part.split_whitespace() {
            let w = w.trim_matches(|c: char| matches!(c, ',' | ';' | '?' | '!'));
            if w.is_empty() {
                continue;
            }
            out.push(Token {
                text: w.to_owned(),
                lower: w.to_lowercase(),
                quoted: false,
            });
        }
    }
    out
}

const STOPWORDS: &[&str] = &[
    // English
    "commit",
    "commits",
    "show",
    "me",
    "find",
    "list",
    "all",
    "the",
    "a",
    "an",
    "that",
    "which",
    "who",
    "with",
    "of",
    "for",
    "and",
    "please",
    "my",
    "were",
    "was",
    "made",
    "done",
    "on",
    "at",
    "to",
    "in",
    "changes",
    "where",
    "any",
    "some",
    "files",
    "file",
    // Portuguese
    "mostre",
    "mostrar",
    "encontre",
    "liste",
    "listar",
    "todos",
    "todas",
    "os",
    "as",
    "o",
    "que",
    "do",
    "da",
    "dos",
    "das",
    "com",
    "e",
    "feitos",
    "feitas",
    "feito",
    "um",
    "uma",
    "no",
    "na",
    "nos",
    "nas",
    "alterações",
    "mudanças",
    "arquivo",
    "arquivos",
    "em",
    "de",
];

fn is_unit(w: &str) -> Option<&'static str> {
    Some(match w {
        "day" | "days" | "dia" | "dias" => "day",
        "week" | "weeks" | "semana" | "semanas" => "week",
        "month" | "months" | "mês" | "mes" | "meses" => "month",
        "year" | "years" | "ano" | "anos" => "year",
        _ => return None,
    })
}

fn back(today: Day, n: i64, unit: &str) -> Day {
    match unit {
        "day" => today.add_days(-n),
        "week" => today.add_days(-7 * n),
        "month" => today.add_months(-(n as i32)),
        _ => today.add_months(-12 * n as i32),
    }
}

fn number(w: &str) -> Option<i64> {
    w.parse().ok().or(match w {
        "one" | "a" | "um" | "uma" => Some(1),
        "two" | "dois" | "duas" => Some(2),
        "three" | "três" | "tres" => Some(3),
        "four" | "quatro" => Some(4),
        "six" | "seis" => Some(6),
        _ => None,
    })
}

/// A date word at `t[i..]`: the day and how many tokens it used.
fn date_at(t: &[Token], i: usize, today: Day) -> Option<(Day, usize)> {
    let w = |k: usize| t.get(i + k).map_or("", |x| x.lower.as_str());
    if let Some(d) = Day::parse(w(0)) {
        return Some((d, 1));
    }
    if w(0).len() == 4
        && let Ok(y) = w(0).parse::<i32>()
        && (1970..=2200).contains(&y)
    {
        return Some((Day::new(y, 1, 1), 1));
    }
    match w(0) {
        "today" | "hoje" => Some((today, 1)),
        "yesterday" | "ontem" => Some((today.add_days(-1), 1)),
        _ => {
            // "3 weeks ago", "3 semanas atrás"
            let n = number(w(0))?;
            let unit = is_unit(w(1))?;
            let used = if matches!(w(2), "ago" | "atrás" | "atras") {
                3
            } else {
                2
            };
            Some((back(today, n, unit), used))
        }
    }
}

/// Reads simple phrases: dates ("last week", "yesterday", "in 2024",
/// "since 2024-03-01", "last 3 days", "semana passada"), the author ("by
/// Ana", "por Ana"), paths ("in src/", "touched login"), message words
/// ("about fix", quoted text) and content ("containing TODO"). Words it
/// does not understand become message words when nothing else matched.
pub fn parse(query: &str, today: Day) -> Filters {
    let t = tokens(query);
    let mut f = Filters::default();
    let mut leftover: Vec<String> = Vec::new();
    let w = |k: usize| t.get(k).map_or("", |x| x.lower.as_str());
    let mut i = 0;
    while i < t.len() {
        let (a, b, c) = (w(i), w(i + 1), w(i + 2));
        // Quoted text is message words unless a keyword took it.
        if t[i].quoted {
            push_words(&mut f.message, &format!("\"{}\"", t[i].text));
            i += 1;
            continue;
        }
        // Relative periods.
        let period = match (a, b) {
            ("this" | "esta" | "nesta" | "essa" | "nessa", "week" | "semana") => {
                Some((today.add_days(-today.weekday()), 2))
            }
            ("this" | "este" | "neste" | "esse" | "nesse", "month" | "mês" | "mes") => {
                Some((Day::new(today.year, today.month, 1), 2))
            }
            ("this" | "este" | "neste" | "esse" | "nesse", "year" | "ano") => {
                Some((Day::new(today.year, 1, 1), 2))
            }
            ("last" | "past", u) | ("último" | "ultimo" | "última" | "ultima", u)
                if is_unit(u).is_some() =>
            {
                Some((back(today, 1, is_unit(u).unwrap_or("day")), 2))
            }
            (u, "passado" | "passada") if is_unit(u).is_some() => {
                Some((back(today, 1, is_unit(u).unwrap_or("day")), 2))
            }
            ("last" | "past" | "últimos" | "ultimos" | "últimas" | "ultimas", n)
                if number(n).is_some() && is_unit(c).is_some() =>
            {
                let n = number(n).unwrap_or(1);
                Some((back(today, n, is_unit(c).unwrap_or("day")), 3))
            }
            _ => None,
        };
        if let Some((day, used)) = period {
            f.since = day.format();
            i += used;
            continue;
        }
        if matches!(a, "today" | "hoje") {
            f.since = today.format();
            i += 1;
            continue;
        }
        if matches!(a, "yesterday" | "ontem") {
            f.since = today.add_days(-1).format();
            f.until = today.add_days(-1).format();
            i += 1;
            continue;
        }
        // "3 days ago" alone: since then.
        if number(a).is_some()
            && is_unit(b).is_some()
            && matches!(c, "ago" | "atrás" | "atras")
            && let Some((day, used)) = date_at(&t, i, today)
        {
            f.since = day.format();
            i += used;
            continue;
        }
        // since / after / before / until DATE
        let skip_de = usize::from(matches!(b, "de" | "of"));
        let bound = match a {
            "since" | "after" | "desde" | "from" => Some(false),
            "depois" if skip_de == 1 => Some(false),
            "before" | "until" | "till" | "antes" | "até" | "ate" => Some(true),
            _ => None,
        };
        if let Some(until) = bound {
            let at = i
                + 1
                + if a == "depois" || a == "antes" {
                    skip_de
                } else {
                    0
                };
            if let Some((day, used)) = date_at(&t, at, today) {
                let year_only = t[at].lower.len() == 4;
                if until {
                    let day = if a == "before" || a == "antes" {
                        day.add_days(-1)
                    } else if year_only {
                        Day::new(day.year, 12, 31)
                    } else {
                        day
                    };
                    f.until = day.format();
                } else {
                    let day = if (a == "after" || a == "depois") && !year_only {
                        day.add_days(1)
                    } else if a == "after" || a == "depois" {
                        Day::new(day.year + 1, 1, 1)
                    } else {
                        day
                    };
                    f.since = day.format();
                }
                i = at + used;
                continue;
            }
        }
        // "in 2024" / "em 2024"
        if matches!(a, "in" | "em" | "during" | "durante")
            && b.len() == 4
            && let Ok(y) = b.parse::<i32>()
            && (1970..=2200).contains(&y)
        {
            f.since = Day::new(y, 1, 1).format();
            f.until = Day::new(y, 12, 31).format();
            i += 2;
            continue;
        }
        // Author
        let author_kw = match (a, b) {
            ("by" | "por" | "author" | "autor" | "autora" | "from", _) => 1,
            ("written" | "escritos" | "feitos", "by" | "por") => 2,
            _ if a.starts_with("author:") || a.starts_with("autor:") => 0,
            // "commits da Ana": a capitalized name after de/da/do.
            ("de" | "da" | "do", _)
                if t.get(i + 1)
                    .is_some_and(|x| x.text.chars().next().is_some_and(char::is_uppercase)) =>
            {
                1
            }
            _ => usize::MAX,
        };
        if author_kw != usize::MAX {
            if author_kw == 0 {
                f.author = t[i].text.split_once(':').map_or("", |(_, v)| v).to_owned();
                i += 1;
                continue;
            }
            let at = i + author_kw;
            if let Some(tok) = t.get(at)
                && !STOPWORDS.contains(&tok.lower.as_str())
                && date_at(&t, at, today).is_none()
            {
                let mut name = vec![tok.text.clone()];
                let mut j = at + 1;
                // "by Ana Silva": following capitalized words.
                while let Some(next) = t.get(j) {
                    let capital = next.text.chars().next().is_some_and(char::is_uppercase);
                    if tok.quoted || !capital || next.quoted {
                        break;
                    }
                    name.push(next.text.clone());
                    j += 1;
                }
                f.author = name.join(" ");
                i = j;
                continue;
            }
        }
        // Paths
        let path_kw = match (a, b) {
            (
                "touched" | "touching" | "touch" | "changed" | "changing" | "modified"
                | "modifying" | "affecting" | "affected" | "path" | "alteraram" | "alterou"
                | "mudaram" | "mudou" | "modificaram" | "tocaram" | "caminho",
                _,
            ) => Some(1),
            ("mexeram" | "mexem" | "mexeu", "em" | "no" | "na" | "nos" | "nas") => Some(2),
            ("mexeram" | "mexem" | "mexeu", _) => Some(1),
            _ if a.starts_with("path:") => Some(0),
            _ => None,
        };
        if let Some(n) = path_kw {
            if n == 0 {
                f.path = pathspec(t[i].text.split_once(':').map_or("", |(_, v)| v));
                i += 1;
                continue;
            }
            let mut at = i + n;
            // "touched the file x", "alteraram o arquivo x"
            while t.get(at).is_some_and(|x| {
                matches!(
                    x.lower.as_str(),
                    "the"
                        | "file"
                        | "files"
                        | "folder"
                        | "o"
                        | "a"
                        | "os"
                        | "arquivo"
                        | "arquivos"
                        | "pasta"
                        | "em"
                        | "no"
                        | "na"
                )
            }) {
                at += 1;
            }
            if let Some(tok) = t.get(at)
                && !STOPWORDS.contains(&tok.lower.as_str())
            {
                f.path = pathspec(&tok.text);
                i = at + 1;
                continue;
            }
        }
        if matches!(
            a,
            "in" | "under" | "inside" | "within" | "em" | "no" | "na" | "dentro" | "sob"
        ) {
            let at = i + 1 + usize::from(a == "dentro" && b == "de");
            if let Some(tok) = t.get(at)
                && looks_like_path(&tok.text)
            {
                f.path = pathspec(&tok.text);
                i = at + 1;
                continue;
            }
        }
        // Content
        if matches!(
            a,
            "containing"
                | "contains"
                | "contain"
                | "code"
                | "content"
                | "contendo"
                | "contém"
                | "contem"
                | "código"
                | "codigo"
                | "adding"
                | "removing"
        ) && let Some(tok) = t.get(i + 1)
        {
            f.content = tok.text.clone();
            i += 2;
            continue;
        }
        // Message
        if matches!(
            a,
            "about" | "mentioning" | "message" | "titled" | "sobre" | "mencionando" | "mensagem"
        ) && let Some(tok) = t.get(i + 1)
            && !STOPWORDS.contains(&tok.lower.as_str())
        {
            let word = if tok.quoted {
                format!("\"{}\"", tok.text)
            } else {
                tok.text.clone()
            };
            push_words(&mut f.message, &word);
            i += 2;
            continue;
        }
        if looks_like_path(&t[i].text) {
            f.path = pathspec(&t[i].text);
            i += 1;
            continue;
        }
        if !STOPWORDS.contains(&a) && !matches!(a, "last" | "past" | "since" | "before") {
            leftover.push(t[i].text.clone());
        }
        i += 1;
    }
    if !leftover.is_empty() && f.message.is_empty() {
        f.message = leftover.join(" ");
    }
    f
}

fn push_words(message: &mut String, words: &str) {
    if !message.is_empty() {
        message.push(' ');
    }
    message.push_str(words);
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/')
        || s.contains('*')
        || s.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && !stem.chars().all(|c| c.is_ascii_digit())
                && (1..=5).contains(&ext.len())
                && ext.chars().all(char::is_alphanumeric)
        })
}
