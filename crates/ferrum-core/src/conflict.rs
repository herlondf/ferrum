//! Parses conflict markers so each conflict can be resolved on its own.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    /// Text both sides agree on.
    Common(String),
    Conflict(Conflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub ours: String,
    pub base: Option<String>,
    pub theirs: String,
    pub ours_label: String,
    pub theirs_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Ours,
    Theirs,
    OursThenTheirs,
    TheirsThenOurs,
}

#[derive(PartialEq)]
enum State {
    Common,
    Ours,
    Base,
    Theirs,
}

fn marker<'a>(line: &'a str, m: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(m)?;
    let rest = rest.trim_end_matches(['\n', '\r']);
    (rest.is_empty() || rest.starts_with(' ')).then(|| rest.trim_start())
}

/// Splits a file with conflict markers into common text and conflicts.
pub fn parse(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut common = String::new();
    let mut cur = Conflict {
        ours: String::new(),
        base: None,
        theirs: String::new(),
        ours_label: String::new(),
        theirs_label: String::new(),
    };
    let mut state = State::Common;
    for line in text.split_inclusive('\n') {
        match state {
            State::Common => {
                if let Some(label) = marker(line, "<<<<<<<") {
                    if !common.is_empty() {
                        chunks.push(Chunk::Common(std::mem::take(&mut common)));
                    }
                    cur.ours_label = label.to_owned();
                    state = State::Ours;
                } else {
                    common.push_str(line);
                }
            }
            State::Ours | State::Base => {
                if marker(line, "|||||||").is_some() {
                    cur.base = Some(String::new());
                    state = State::Base;
                } else if marker(line, "=======").is_some() {
                    state = State::Theirs;
                } else if state == State::Ours {
                    cur.ours.push_str(line);
                } else if let Some(b) = cur.base.as_mut() {
                    b.push_str(line);
                }
            }
            State::Theirs => {
                if let Some(label) = marker(line, ">>>>>>>") {
                    cur.theirs_label = label.to_owned();
                    chunks.push(Chunk::Conflict(std::mem::replace(
                        &mut cur,
                        Conflict {
                            ours: String::new(),
                            base: None,
                            theirs: String::new(),
                            ours_label: String::new(),
                            theirs_label: String::new(),
                        },
                    )));
                    state = State::Common;
                } else {
                    cur.theirs.push_str(line);
                }
            }
        }
    }
    if !common.is_empty() {
        chunks.push(Chunk::Common(common));
    }
    chunks
}

/// Builds the file from chunks. Conflicts without a choice keep their markers.
pub fn render(chunks: &[Chunk], choices: &[Option<Choice>]) -> String {
    let mut out = String::new();
    let mut n = 0;
    for chunk in chunks {
        match chunk {
            Chunk::Common(t) => out.push_str(t),
            Chunk::Conflict(c) => {
                match choices.get(n).copied().flatten() {
                    Some(Choice::Ours) => out.push_str(&c.ours),
                    Some(Choice::Theirs) => out.push_str(&c.theirs),
                    Some(Choice::OursThenTheirs) => {
                        out.push_str(&c.ours);
                        out.push_str(&c.theirs);
                    }
                    Some(Choice::TheirsThenOurs) => {
                        out.push_str(&c.theirs);
                        out.push_str(&c.ours);
                    }
                    None => {
                        out.push_str(&format!("<<<<<<< {}\n", c.ours_label));
                        out.push_str(&c.ours);
                        if let Some(b) = &c.base {
                            out.push_str("||||||| base\n");
                            out.push_str(b);
                        }
                        out.push_str("=======\n");
                        out.push_str(&c.theirs);
                        out.push_str(&format!(">>>>>>> {}\n", c.theirs_label));
                    }
                }
                n += 1;
            }
        }
    }
    out
}

pub fn conflict_count(chunks: &[Chunk]) -> usize {
    chunks
        .iter()
        .filter(|c| matches!(c, Chunk::Conflict(_)))
        .count()
}

pub fn has_markers(text: &str) -> bool {
    conflict_count(&parse(text)) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "top\n<<<<<<< HEAD\nours 1\nours 2\n=======\ntheirs\n>>>>>>> feature\nmiddle\n<<<<<<< HEAD\nA\n||||||| base\nB\n=======\nC\n>>>>>>> feature\nend\n";

    #[test]
    fn parses_and_renders() {
        let chunks = parse(TEXT);
        assert_eq!(conflict_count(&chunks), 2);
        let Chunk::Conflict(c) = &chunks[1] else {
            panic!()
        };
        assert_eq!(c.ours, "ours 1\nours 2\n");
        assert_eq!(c.theirs_label, "feature");
        let Chunk::Conflict(c2) = &chunks[3] else {
            panic!()
        };
        assert_eq!(c2.base.as_deref(), Some("B\n"));

        let resolved = render(
            &chunks,
            &[Some(Choice::Theirs), Some(Choice::OursThenTheirs)],
        );
        assert_eq!(resolved, "top\ntheirs\nmiddle\nA\nC\nend\n");
        assert!(!has_markers(&resolved));

        // An unresolved conflict keeps its markers.
        let partial = render(&chunks, &[Some(Choice::Ours), None]);
        assert!(has_markers(&partial));
        assert!(partial.starts_with("top\nours 1\nours 2\nmiddle\n<<<<<<< HEAD\n"));
    }

    #[test]
    fn plain_text_has_no_conflicts() {
        let chunks = parse("a\n=======\nb\n");
        assert_eq!(conflict_count(&chunks), 0);
        assert_eq!(render(&chunks, &[]), "a\n=======\nb\n");
    }
}
