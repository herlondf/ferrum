//! Semantic merge of a conflicted file: base, ours and theirs are parsed and
//! merged declaration by declaration (functions, classes, struct fields,
//! imports, JSON/YAML/TOML keys…), in the spirit of mergiraf. Two sides that
//! add or change different declarations merge cleanly even when the lines
//! touch; a declaration both sides changed is merged line by line on its
//! own, or recursively when it is a class or `impl` body. Anything else is a
//! real conflict and is left alone.
//!
//! The result is only a suggestion: the caller shows it for review. The
//! `mergiraf` program, when installed, can be tried as well (see [`mergiraf`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Tree};

use crate::syntax::{self, Lang};

/// A merged file and what was decided to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    pub decisions: Vec<Decision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The declaration, like "fn parse(text: &str)" or "impl Repo › fn open".
    pub item: String,
    pub how: How,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    AddedOurs,
    AddedTheirs,
    ChangedOurs,
    ChangedTheirs,
    DeletedOurs,
    DeletedTheirs,
    /// Both sides made the same change.
    SameChange,
    /// Both sides changed it; their changes merged line by line.
    MergedLines,
}

/// Why a file could not be merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// No grammar, or a language without declarations to merge.
    Unsupported,
    TooLarge,
    /// A version does not parse cleanly (or the result would not).
    Syntax,
    /// A version still has conflict markers.
    Markers,
    /// Both sides changed this declaration in ways that do not merge.
    Conflict(String),
    /// Both sides inserted code at the same place (inside this container,
    /// empty at the top level) and its order matters.
    SamePlace(String),
}

/// Merges the three versions of `path`. `base` is `None` when both sides
/// added the file.
pub fn merge(path: &str, base: Option<&str>, ours: &str, theirs: &str) -> Result<Merged, Failure> {
    let lang = Lang::from_path(path).ok_or(Failure::Unsupported)?;
    let base = base.unwrap_or("");
    if [base, ours, theirs].iter().any(|t| has_markers(t)) {
        return Err(Failure::Markers);
    }
    let parse = |text: &str| -> Result<Tree, Failure> {
        let tree = syntax::parse(lang, text).ok_or(Failure::TooLarge)?;
        if tree.root_node().has_error() {
            return Err(Failure::Syntax);
        }
        Ok(tree)
    };
    let (tb, to, tt) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let mut m = Merger {
        lang,
        src: [base, ours, theirs],
        decisions: Vec::new(),
    };
    let roots = [tb.root_node(), to.root_node(), tt.root_node()];
    let text = m.outside(roots, |m| m.descend(roots))?;
    // The result must parse as well as the inputs did.
    let check = syntax::parse(lang, &text).ok_or(Failure::TooLarge)?;
    if check.root_node().has_error() {
        return Err(Failure::Syntax);
    }
    Ok(Merged {
        text,
        decisions: m.decisions,
    })
}

fn has_markers(text: &str) -> bool {
    text.lines().any(|l| {
        l.starts_with("<<<<<<< ") || l == "<<<<<<<" || l.starts_with(">>>>>>> ") || l == "======="
    })
}

/// A declaration inside a container, with its leading comments and the
/// whitespace (or separator) before it.
#[derive(Clone)]
struct Member<'t> {
    key: String,
    /// Named declaration or import: its place among siblings does not matter.
    commutes: bool,
    label: String,
    /// Text between the previous member (or the header) and this one.
    ws: String,
    body: String,
    /// Where `body` starts and ends in the side's text.
    start: usize,
    end: usize,
    node: Node<'t>,
}

struct Split<'t> {
    head: String,
    members: Vec<Member<'t>>,
    tail: String,
}

struct Merger<'s> {
    lang: Lang,
    /// Base, ours, theirs.
    src: [&'s str; 3],
    decisions: Vec<Decision>,
}

/// Picks the side that changed: ours when both agree or only ours changed.
fn pick3<T: PartialEq + Clone>(b: &T, o: &T, t: &T) -> Option<T> {
    if o == t || b == t {
        Some(o.clone())
    } else if b == o {
        Some(t.clone())
    } else {
        None
    }
}

impl<'s> Merger<'s> {
    fn text(&self, side: usize, range: std::ops::Range<usize>) -> &'s str {
        &self.src[side][range]
    }

    /// Containers whose members can be merged one by one.
    fn commutative(&self, node: &Node) -> bool {
        let k = node.kind();
        match self.lang {
            Lang::Rust => matches!(
                k,
                "source_file" | "declaration_list" | "field_declaration_list"
            ),
            Lang::JavaScript | Lang::TypeScript | Lang::Tsx => matches!(
                k,
                "program" | "class_body" | "interface_body" | "object_type"
            ),
            Lang::Python => {
                k == "module"
                    || (k == "block"
                        && node
                            .parent()
                            .is_some_and(|p| p.kind() == "class_definition"))
            }
            Lang::Go => matches!(
                k,
                "source_file" | "field_declaration_list" | "import_spec_list"
            ),
            Lang::Java => matches!(
                k,
                "program" | "class_body" | "interface_body" | "enum_body_declarations"
            ),
            Lang::C | Lang::Cpp => matches!(
                k,
                "translation_unit" | "field_declaration_list" | "declaration_list"
            ),
            Lang::CSharp => matches!(k, "compilation_unit" | "declaration_list"),
            Lang::Json => k == "object",
            Lang::Yaml => matches!(k, "block_mapping" | "flow_mapping"),
            Lang::Toml => matches!(k, "document" | "table" | "table_array_element"),
            Lang::Html | Lang::Css | Lang::Markdown => false,
        }
    }

    /// Children that are not members: the header of a TOML table.
    fn is_header(&self, node: &Node) -> bool {
        self.lang == Lang::Toml && node.kind().ends_with("_key")
    }

    /// Comments and attributes belong to the declaration after them.
    fn is_prefix(&self, node: &Node) -> bool {
        syntax::is_comment(node) || (self.lang == Lang::Rust && node.kind() == "attribute_item")
    }

    fn split<'t>(&self, side: usize, node: Node<'t>) -> Split<'t> {
        let src = self.src[side];
        let mut cursor = node.walk();
        let children: Vec<Node<'t>> = node
            .named_children(&mut cursor)
            .filter(|c| !self.is_header(c))
            .collect();
        // Group prefixes (comments, attributes) with the member after them; a
        // comment on the same line as a member's end stays with that member.
        let mut members: Vec<(usize, usize, Node<'t>)> = Vec::new();
        let mut lead: Option<usize> = None;
        for c in &children {
            let r = c.byte_range();
            if self.is_prefix(c) {
                let same_line = members.last().is_some_and(|(_, end, _)| {
                    lead.is_none() && !src[*end..r.start].contains('\n')
                });
                if same_line && syntax::is_comment(c) {
                    if let Some(last) = members.last_mut() {
                        last.1 = r.end;
                    }
                } else {
                    lead.get_or_insert(r.start);
                }
                continue;
            }
            members.push((lead.take().unwrap_or(r.start), r.end, *c));
        }
        let start = node.start_byte();
        let end = node.end_byte();
        let head_end = members.first().map_or(end, |m| m.0);
        let tail_start = members.last().map_or(end, |m| m.1);
        let mut out = Vec::new();
        let mut prev = head_end;
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (lead, end, n) in members {
            let (key, commutes) = self.key(side, &n);
            let count = seen.entry(key.clone()).or_default();
            *count += 1;
            let key = if *count > 1 {
                format!("{key}#{count}")
            } else {
                key
            };
            out.push(Member {
                key,
                commutes,
                label: label(&src[n.byte_range()]),
                ws: src[prev..lead].to_owned(),
                body: src[lead..end].to_owned(),
                start: lead,
                end,
                node: n,
            });
            prev = end;
        }
        Split {
            head: src[start..head_end].to_owned(),
            members: out,
            tail: src[tail_start.max(head_end)..end].to_owned(),
        }
    }

    /// The identity of a declaration across versions, and whether it commutes.
    fn key(&self, side: usize, node: &Node) -> (String, bool) {
        let kind = node.kind();
        if is_import(kind) {
            return (
                format!("{kind}:{}", squash(self.text(side, node.byte_range()))),
                true,
            );
        }
        match self.name(side, node) {
            Some(name) => (format!("{kind}:{name}"), true),
            None => (
                format!("{kind}:{}", squash(self.text(side, node.byte_range()))),
                false,
            ),
        }
    }

    fn name(&self, side: usize, node: &Node) -> Option<String> {
        let text = |n: Node| squash(self.text(side, n.byte_range()));
        let field = |f: &str| node.child_by_field_name(f);
        let name = match node.kind() {
            "impl_item" => {
                let ty = field("type").map(text)?;
                match field("trait") {
                    Some(t) => format!("{} for {ty}", text(t)),
                    None => ty,
                }
            }
            "decorated_definition" | "export_statement" | "template_declaration" => {
                let inner = field("definition").or_else(|| field("declaration"))?;
                return self.name(side, &inner);
            }
            "lexical_declaration" | "variable_declaration" => {
                let d = first_named(node, "variable_declarator")?;
                text(d.child_by_field_name("name")?)
            }
            "type_declaration" => {
                text(first_named(node, "type_spec")?.child_by_field_name("name")?)
            }
            "const_declaration" | "var_declaration" => {
                let spec =
                    first_named(node, "const_spec").or_else(|| first_named(node, "var_spec"))?;
                text(spec.child_by_field_name("name")?)
            }
            "table" | "table_array_element" | "pair" if self.lang == Lang::Toml => {
                let mut c = node.walk();
                let key = node
                    .named_children(&mut c)
                    .find(|x| x.kind().ends_with("_key"))?;
                text(key)
            }
            _ => {
                if let Some(n) = field("name").or_else(|| field("key")) {
                    text(n)
                } else {
                    // C and C++: the name is at the end of the declarator chain.
                    let mut d = field("declarator")?;
                    while let Some(inner) = d.child_by_field_name("declarator") {
                        d = inner;
                    }
                    text(d)
                }
            }
        };
        // Overloads differ by their parameters.
        let overloads = matches!(
            self.lang,
            Lang::Java | Lang::CSharp | Lang::Cpp | Lang::TypeScript | Lang::Tsx
        );
        match field("parameters") {
            Some(p) if overloads => Some(format!("{name}{}", text(p))),
            _ => Some(name),
        }
    }

    /// Merges the text around three nodes (which must merge as is) and,
    /// with `inner`, the nodes themselves.
    fn outside(
        &mut self,
        nodes: [Node; 3],
        inner: impl FnOnce(&mut Self) -> Result<String, Failure>,
    ) -> Result<String, Failure> {
        let parts = |i: usize| {
            let src = self.src[i];
            (
                src[..nodes[i].start_byte()].to_owned(),
                src[nodes[i].end_byte()..].to_owned(),
            )
        };
        let [b, o, t] = [0, 1, 2].map(parts);
        let before = pick3(&b.0, &o.0, &t.0).ok_or(Failure::Unsupported)?;
        let after = pick3(&b.1, &o.1, &t.1).ok_or(Failure::Unsupported)?;
        Ok(format!("{before}{}{after}", inner(self)?))
    }

    /// Goes down single-child wrappers (a JSON document holds one object, a
    /// YAML stream one document…) to the first container of declarations.
    fn descend(&mut self, nodes: [Node; 3]) -> Result<String, Failure> {
        if nodes.iter().all(|n| self.commutative(n)) {
            return self.container(nodes, "");
        }
        let [Some(b), Some(o), Some(t)] = nodes.map(|n| only_child(&n)) else {
            return Err(Failure::Unsupported);
        };
        if b.kind() != o.kind() || o.kind() != t.kind() {
            return Err(Failure::Unsupported);
        }
        let parts = |i: usize, outer: Node, inner: Node| {
            let src = self.src[i];
            (
                src[outer.start_byte()..inner.start_byte()].to_owned(),
                src[inner.end_byte()..outer.end_byte()].to_owned(),
            )
        };
        let (pb, po, pt) = (
            parts(0, nodes[0], b),
            parts(1, nodes[1], o),
            parts(2, nodes[2], t),
        );
        let before = pick3(&pb.0, &po.0, &pt.0).ok_or(Failure::Unsupported)?;
        let after = pick3(&pb.1, &po.1, &pt.1).ok_or(Failure::Unsupported)?;
        Ok(format!("{before}{}{after}", self.descend([b, o, t])?))
    }

    /// Merges three versions of a container node.
    fn container(&mut self, nodes: [Node; 3], path: &str) -> Result<String, Failure> {
        let [b, o, t] = [0, 1, 2].map(|i| self.split(i, nodes[i]));
        let conflict = |what: &str| Failure::Conflict(join(path, what));
        let head = pick3(&b.head, &o.head, &t.head).ok_or_else(|| conflict(&label(&o.head)))?;
        let tail = pick3(&b.tail, &o.tail, &t.tail).ok_or_else(|| conflict(&label(&o.tail)))?;
        let index = |s: &Split| -> HashMap<String, usize> {
            s.members
                .iter()
                .enumerate()
                .map(|(i, m)| (m.key.clone(), i))
                .collect()
        };
        let (bi, oi, ti) = (index(&b), index(&o), index(&t));

        // Insertions by both sides at the same place must all commute.
        let anchors = |s: &Split| -> HashMap<Option<String>, Vec<bool>> {
            let mut out: HashMap<Option<String>, Vec<bool>> = HashMap::new();
            let mut anchor = None;
            for m in &s.members {
                if bi.contains_key(&m.key) {
                    anchor = Some(m.key.clone());
                } else {
                    out.entry(anchor.clone()).or_default().push(m.commutes);
                }
            }
            out
        };
        let (ao, at) = (anchors(&o), anchors(&t));
        for (place, flags) in &ao {
            if let Some(other) = at.get(place)
                && !(flags.iter().chain(other).all(|c| *c))
            {
                return Err(Failure::SamePlace(path.to_owned()));
            }
        }

        // Each member: its merged text, or None when deleted.
        let mut merged: HashMap<String, Option<(String, String)>> = HashMap::new();
        let keys: Vec<String> = {
            let mut seen = HashSet::new();
            o.members
                .iter()
                .chain(&t.members)
                .chain(&b.members)
                .map(|m| m.key.clone())
                .filter(|k| seen.insert(k.clone()))
                .collect()
        };
        for key in &keys {
            let mb = bi.get(key).map(|&n| b.members[n].clone());
            let mo = oi.get(key).map(|&n| o.members[n].clone());
            let mt = ti.get(key).map(|&n| t.members[n].clone());
            let result = self.member(path, mb, mo, mt)?;
            merged.insert(key.clone(), result);
        }

        // Ours' order, with theirs' additions after their anchor (and after
        // ours' additions there).
        let mut order: Vec<String> = o
            .members
            .iter()
            .map(|m| m.key.clone())
            .filter(|k| matches!(merged.get(k), Some(Some(_))))
            .collect();
        let ours_added = |k: &String| !bi.contains_key(k) && !ti.contains_key(k);
        let mut prev: Option<String> = None;
        for m in &t.members {
            let k = &m.key;
            if order.contains(k) {
                prev = Some(k.clone());
                continue;
            }
            if !matches!(merged.get(k), Some(Some(_))) {
                continue;
            }
            let mut at = match &prev {
                Some(p) => order
                    .iter()
                    .position(|x| x == p)
                    .map_or(order.len(), |i| i + 1),
                None => 0,
            };
            while at < order.len() && ours_added(&order[at]) {
                at += 1;
            }
            order.insert(at, k.clone());
            prev = Some(k.clone());
        }

        // Separators: the first member follows the header directly; a member
        // that was first on its side borrows the separator of its next sibling.
        let sep_after = |k: &String| -> Option<String> {
            for s in [&o, &t, &b] {
                if let Some(i) = s.members.iter().position(|m| &m.key == k) {
                    return s.members.get(i + 1).map(|m| m.ws.clone());
                }
            }
            None
        };
        let mut out = head;
        let mut last_ws = String::from("\n");
        for (i, k) in order.iter().enumerate() {
            let Some(Some((ws, body))) = merged.get(k) else {
                continue;
            };
            if i > 0 {
                let ws = if ws.is_empty() {
                    sep_after(k)
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| last_ws.clone())
                } else {
                    ws.clone()
                };
                out.push_str(&ws);
                last_ws = ws;
            }
            out.push_str(body);
        }
        out.push_str(&tail);
        Ok(out)
    }

    /// Merges one member; `None` when it is deleted.
    fn member(
        &mut self,
        path: &str,
        b: Option<Member>,
        o: Option<Member>,
        t: Option<Member>,
    ) -> Result<Option<(String, String)>, Failure> {
        let name = o
            .as_ref()
            .or(t.as_ref())
            .or(b.as_ref())
            .map(|m| join(path, &m.label))
            .unwrap_or_default();
        let mut decide = |how| {
            self.decisions.push(Decision {
                item: name.clone(),
                how,
            })
        };
        let keep = |m: &Member| Some((m.ws.clone(), m.body.clone()));
        Ok(match (b, o, t) {
            (_, None, None) => None,
            (None, Some(o), None) => {
                decide(How::AddedOurs);
                keep(&o)
            }
            (None, None, Some(t)) => {
                decide(How::AddedTheirs);
                keep(&t)
            }
            (None, Some(o), Some(t)) => {
                if o.body != t.body {
                    return Err(Failure::Conflict(name));
                }
                decide(How::SameChange);
                keep(&o)
            }
            (Some(b), Some(o), None) => {
                if b.body != o.body {
                    return Err(Failure::Conflict(name));
                }
                decide(How::DeletedTheirs);
                None
            }
            (Some(b), None, Some(t)) => {
                if b.body != t.body {
                    return Err(Failure::Conflict(name));
                }
                decide(How::DeletedOurs);
                None
            }
            (Some(b), Some(o), Some(t)) => {
                let ws = pick3(&b.ws, &o.ws, &t.ws).unwrap_or_else(|| o.ws.clone());
                if o.body == t.body {
                    if b.body != o.body {
                        decide(How::SameChange);
                    }
                    Some((ws, o.body))
                } else if b.body == o.body {
                    decide(How::ChangedTheirs);
                    Some((ws, t.body))
                } else if b.body == t.body {
                    decide(How::ChangedOurs);
                    Some((ws, o.body))
                } else {
                    let body = self.both_changed(&name, [&b, &o, &t])?;
                    Some((ws, body))
                }
            }
        })
    }

    /// A member both sides changed: merge its body recursively when it holds
    /// declarations, else line by line.
    fn both_changed(&mut self, name: &str, m: [&Member; 3]) -> Result<String, Failure> {
        let inner = m.map(|x| self.inner_container(x.node));
        if let [Some(cb), Some(co), Some(ct)] = inner {
            // Text before and after the body (header, closing brace) must merge too.
            let parts = |i: usize, c: Node| {
                (
                    self.src[i][m[i].start..c.start_byte()].to_owned(),
                    self.src[i][c.end_byte()..m[i].end].to_owned(),
                )
            };
            let (pb, po, pt) = (parts(0, cb), parts(1, co), parts(2, ct));
            if let (Some(before), Some(after)) =
                (pick3(&pb.0, &po.0, &pt.0), pick3(&pb.1, &po.1, &pt.1))
            {
                let body = self.container([cb, co, ct], name)?;
                return Ok(format!("{before}{body}{after}"));
            }
        }
        match merge_lines(&m[0].body, &m[1].body, &m[2].body) {
            Some(text) => {
                self.decisions.push(Decision {
                    item: name.to_owned(),
                    how: How::MergedLines,
                });
                Ok(text)
            }
            None => Err(Failure::Conflict(name.to_owned())),
        }
    }

    /// The declarations container of a class, `impl`, struct… (a child or grandchild).
    fn inner_container<'t>(&self, node: Node<'t>) -> Option<Node<'t>> {
        let mut c = node.walk();
        let kids: Vec<Node<'t>> = node.named_children(&mut c).collect();
        if let Some(k) = kids.iter().find(|k| self.commutative(k)) {
            return Some(*k);
        }
        kids.iter().find_map(|k| {
            let mut c = k.walk();
            k.named_children(&mut c).find(|g| self.commutative(g))
        })
    }
}

/// The only named child that is not a comment.
fn only_child<'t>(n: &Node<'t>) -> Option<Node<'t>> {
    let mut c = n.walk();
    let kids: Vec<Node<'t>> = n
        .named_children(&mut c)
        .filter(|k| !syntax::is_comment(k))
        .collect();
    (kids.len() == 1).then(|| kids[0])
}

fn first_named<'t>(n: &Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c).find(|x| x.kind() == kind)
}

fn is_import(kind: &str) -> bool {
    matches!(
        kind,
        "use_declaration"
            | "extern_crate_declaration"
            | "import_statement"
            | "import_from_statement"
            | "future_import_statement"
            | "import_declaration"
            | "import_spec"
            | "using_directive"
            | "preproc_include"
    )
}

/// Text with whitespace runs collapsed, for keys.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first line of a declaration, for people.
fn label(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or("").trim();
    let line = line.trim_end_matches(['{', ':', ' ']);
    if line.chars().count() > 80 {
        let cut: String = line.chars().take(79).collect();
        format!("{cut}…")
    } else {
        line.to_owned()
    }
}

fn join(path: &str, item: &str) -> String {
    if path.is_empty() {
        item.to_owned()
    } else {
        format!("{path} › {item}")
    }
}

/// A three-way line merge (as `git merge-file`), `None` on conflict.
pub fn merge_lines(base: &str, ours: &str, theirs: &str) -> Option<String> {
    let (mut b, mut o, mut t) = (
        git2::MergeFileInput::new(),
        git2::MergeFileInput::new(),
        git2::MergeFileInput::new(),
    );
    b.content(base.as_bytes())
        .path("file")
        .mode(Some(git2::FileMode::Blob));
    o.content(ours.as_bytes())
        .path("file")
        .mode(Some(git2::FileMode::Blob));
    t.content(theirs.as_bytes())
        .path("file")
        .mode(Some(git2::FileMode::Blob));
    // merge_file does not initialize libgit2 itself; this does.
    let _ = git2::Config::new();
    let out = git2::merge_file(&b, &o, &t, None).ok()?;
    out.is_automergeable()
        .then(|| String::from_utf8_lossy(out.content()).into_owned())
}

// ----- mergiraf -----

/// The `mergiraf` program (<https://mergiraf.org>), an external structured
/// merge tool. It is GPL licensed, so Ferrum runs it rather than linking it.
pub mod mergiraf {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// Time mergiraf may take on one file.
    const TIMEOUT: Duration = Duration::from_secs(15);

    /// Where mergiraf is installed, if it is on the PATH.
    pub fn find() -> Option<PathBuf> {
        let names: &[&str] = if cfg!(windows) {
            &["mergiraf.exe"]
        } else {
            &["mergiraf"]
        };
        std::env::split_paths(&std::env::var_os("PATH")?)
            .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
    }

    /// Runs `mergiraf merge` on the three versions. `Ok(None)` when conflicts remain.
    pub fn merge(
        exe: &Path,
        path: &str,
        base: &str,
        ours: &str,
        theirs: &str,
    ) -> std::io::Result<Option<String>> {
        let dir = tempfile_dir()?;
        let ext = Path::new(path)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let file = |name: &str, text: &str| -> std::io::Result<PathBuf> {
            let p = dir.join(format!("{name}{ext}"));
            std::fs::write(&p, text)?;
            Ok(p)
        };
        let (b, o, t) = (
            file("base", base)?,
            file("ours", ours)?,
            file("theirs", theirs)?,
        );
        let out = dir.join(format!("merged{ext}"));
        let mut cmd = Command::new(exe);
        cmd.arg("merge")
            .arg(&b)
            .arg(&o)
            .arg(&t)
            .arg("-o")
            .arg(&out)
            .arg("-p")
            .arg(path)
            .args(["-s", "base", "-x", "ours", "-y", "theirs", "-t", "10000"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn()?;
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break Some(s);
            }
            if start.elapsed() > TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let result = match status {
            Some(s) if s.success() => std::fs::read_to_string(&out)
                .ok()
                .filter(|t| !has_markers(t)),
            _ => None,
        };
        let _ = std::fs::remove_dir_all(&dir);
        Ok(result)
    }

    fn tempfile_dir() -> std::io::Result<PathBuf> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ferrum-mergiraf-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

#[cfg(all(
    test,
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-json"
))]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    const BASE: &str = "use std::fmt;\n\nfn main() {\n    println!(\"hi\");\n}\n";

    #[test]
    fn merges_functions_added_on_both_sides() {
        let ours = format!("{BASE}\nfn alpha() -> u32 {{\n    1\n}}\n");
        let theirs = format!("{BASE}\nfn beta() -> u32 {{\n    2\n}}\n");
        let m = merge("src/lib.rs", Some(BASE), &ours, &theirs).unwrap();
        assert_eq!(
            m.text,
            format!("{BASE}\nfn alpha() -> u32 {{\n    1\n}}\n\nfn beta() -> u32 {{\n    2\n}}\n")
        );
        assert_eq!(
            m.decisions,
            [
                Decision {
                    item: "fn alpha() -> u32".into(),
                    how: How::AddedOurs
                },
                Decision {
                    item: "fn beta() -> u32".into(),
                    how: How::AddedTheirs
                },
            ]
        );
    }

    #[test]
    fn merges_imports_members_and_separate_changes() {
        let base = "use a::A;\n\nstruct S {\n    x: u32,\n}\n\nimpl S {\n    fn one(&self) -> u32 {\n        1\n    }\n}\n";
        let ours = "use a::A;\nuse b::B;\n\nstruct S {\n    x: u32,\n    y: u32,\n}\n\nimpl S {\n    fn one(&self) -> u32 {\n        1\n    }\n\n    fn two(&self) -> u32 {\n        2\n    }\n}\n";
        let theirs = "use a::A;\nuse c::C;\n\nstruct S {\n    x: u32,\n    z: u32,\n}\n\nimpl S {\n    fn one(&self) -> u32 {\n        1\n    }\n\n    fn three(&self) -> u32 {\n        3\n    }\n}\n";
        let m = merge("s.rs", Some(base), ours, theirs).unwrap();
        assert_eq!(
            m.text,
            "use a::A;\nuse b::B;\nuse c::C;\n\nstruct S {\n    x: u32,\n    y: u32,\n    z: u32,\n}\n\nimpl S {\n    fn one(&self) -> u32 {\n        1\n    }\n\n    fn two(&self) -> u32 {\n        2\n    }\n\n    fn three(&self) -> u32 {\n        3\n    }\n}\n"
        );
        assert!(
            m.decisions
                .iter()
                .any(|d| d.item == "impl S › fn three(&self) -> u32")
        );
    }

    #[test]
    fn leaves_real_conflicts_alone() {
        let base = "fn f() -> u32 {\n    1\n}\n";
        let ours = "fn f() -> u32 {\n    2\n}\n";
        let theirs = "fn f() -> u32 {\n    3\n}\n";
        assert_eq!(
            merge("a.rs", Some(base), ours, theirs),
            Err(Failure::Conflict("fn f() -> u32".into()))
        );
        // Both sides add statements at the same place: order matters.
        let base = "x = 1\n";
        assert_eq!(
            merge("a.py", Some(base), "x = 1\nprint(1)\n", "x = 1\nprint(2)\n"),
            Err(Failure::SamePlace(String::new()))
        );
        // Deleted on one side, changed on the other.
        assert!(matches!(
            merge(
                "a.rs",
                Some("fn f() {}\nfn g() {}\n"),
                "fn g() {}\n",
                "fn f() { 1; }\nfn g() {}\n"
            ),
            Err(Failure::Conflict(_))
        ));
        assert_eq!(
            merge("a.txt", Some(""), "a", "b"),
            Err(Failure::Unsupported)
        );
        assert_eq!(
            merge("a.rs", Some(""), "fn (", "fn b() {}"),
            Err(Failure::Syntax)
        );
    }

    #[test]
    fn merges_data_files_and_other_languages() {
        let base = "{\n  \"name\": \"x\",\n  \"version\": 1\n}\n";
        let ours = "{\n  \"name\": \"x\",\n  \"version\": 1,\n  \"a\": true\n}\n";
        let theirs = "{\n  \"name\": \"y\",\n  \"version\": 1,\n  \"b\": false\n}\n";
        let m = merge("p.json", Some(base), ours, theirs).unwrap();
        assert_eq!(
            m.text,
            "{\n  \"name\": \"y\",\n  \"version\": 1,\n  \"a\": true,\n  \"b\": false\n}\n"
        );

        let base = "class A:\n    def f(self):\n        return 1\n";
        let ours =
            "class A:\n    def f(self):\n        return 1\n\n    def g(self):\n        return 2\n";
        let theirs = "import os\n\n\nclass A:\n    def f(self):\n        return 1\n\n    def h(self):\n        return 3\n";
        let m = merge("a.py", Some(base), ours, theirs).unwrap();
        assert_eq!(
            m.text,
            "import os\n\n\nclass A:\n    def f(self):\n        return 1\n\n    def g(self):\n        return 2\n\n    def h(self):\n        return 3\n"
        );
    }

    fn versions(t: &TestRepo, path: &str) -> (Option<String>, String, String) {
        let [base, ours, theirs] = t.repo().conflict_versions(path).unwrap();
        (base, ours.unwrap(), theirs.unwrap())
    }

    /// Two branches that both add a function at the end of the same file
    /// conflict in Git; the semantic merge resolves it. A function both
    /// changed differently stays a conflict.
    #[test]
    fn resolves_a_real_git_conflict() {
        let t = TestRepo::new();
        t.commit("src/lib.rs", BASE, "base");
        t.git.run(&["checkout", "-q", "-b", "feature"]).unwrap();
        let beta = format!("{BASE}\nfn beta() -> u32 {{\n    2\n}}\n");
        t.commit("src/lib.rs", &beta, "beta");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        let alpha = format!("{BASE}\nfn alpha() -> u32 {{\n    1\n}}\n");
        t.commit("src/lib.rs", &alpha, "alpha");
        assert!(t.git.run(&["merge", "feature"]).is_err());
        let (base, ours, theirs) = versions(&t, "src/lib.rs");
        let m = merge("src/lib.rs", base.as_deref(), &ours, &theirs).unwrap();
        assert!(m.text.contains("fn alpha()") && m.text.contains("fn beta()"));
        assert!(!has_markers(&m.text));
        // The working copy is untouched: still conflicted.
        assert!(has_markers(&t.read("src/lib.rs")));
        t.git.run(&["merge", "--abort"]).unwrap();

        t.git
            .run(&["checkout", "-q", "-b", "other", "HEAD~1"])
            .unwrap();
        t.commit("src/lib.rs", &BASE.replace("hi", "hello"), "hello");
        t.git.run(&["checkout", "-q", "main"]).unwrap();
        t.commit("src/lib.rs", &alpha.replace("hi", "hey"), "hey");
        assert!(t.git.run(&["merge", "other"]).is_err());
        let (base, ours, theirs) = versions(&t, "src/lib.rs");
        let r = merge("src/lib.rs", base.as_deref(), &ours, &theirs);
        assert_eq!(r, Err(Failure::Conflict("fn main()".into())));
    }

    /// The mergiraf command line: `merge BASE OURS THEIRS -o OUT ...`, exit 0
    /// when clean. A stand-in script checks it.
    #[cfg(unix)]
    #[test]
    fn runs_mergiraf() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = |name: &str, status: u8| {
            let p = dir.path().join(name);
            let script = format!(
                "#!/bin/sh\n[ \"$1\" = merge ] && [ \"$5\" = -o ] || exit 9\ncat \"$3\" \"$4\" > \"$6\"\nexit {status}\n"
            );
            std::fs::write(&p, script).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let clean = fake("clean", 0);
        let out = mergiraf::merge(&clean, "src/a.rs", "b\n", "o\n", "t\n").unwrap();
        assert_eq!(out.as_deref(), Some("o\nt\n"));
        let conflicted = fake("conflicted", 1);
        assert_eq!(
            mergiraf::merge(&conflicted, "a.rs", "", "o\n", "t\n").unwrap(),
            None
        );
    }

    #[test]
    fn merges_lines() {
        assert_eq!(
            merge_lines("a\nb\nc\nd\ne\n", "A\nb\nc\nd\ne\n", "a\nb\nc\nd\nE\n").as_deref(),
            Some("A\nb\nc\nd\nE\n")
        );
        assert_eq!(merge_lines("a\n", "b\n", "c\n"), None);
    }
}
