//! Structural diff, after difftastic's idea: both versions are parsed into
//! syntax trees and compared node by node, so whitespace, indentation and
//! line breaks do not count as changes, and a block that moved is shown as
//! moved rather than deleted and added.
//!
//! Unchanged subtrees are matched first (by a hash of their tokens); where
//! children differ, nodes of the same kind are compared recursively, and the
//! rest falls back to a token diff. Changed comments and strings are marked
//! word by word.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::time::{Duration, Instant};

use similar::{Algorithm, DiffTag, capture_diff_slices_deadline};
use tree_sitter::Node;

use crate::syntax::{self, Lang};

/// Time allowed for the comparison; past it the diff gets coarser, not wrong.
pub const TIME_LIMIT: Duration = Duration::from_secs(2);
/// Deeper trees are not compared (the line diff is used).
const MAX_DEPTH: usize = 600;
/// Files with more tokens than this are not compared.
const MAX_TOKENS: usize = 150_000;
/// A deleted subtree found again elsewhere counts as moved from this many tokens.
const MIN_MOVED_TOKENS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Only on this side.
    Novel,
    /// Unchanged, but somewhere else in the file.
    Moved,
}

/// One version of the file with its changes.
#[derive(Debug, Clone, Default)]
pub struct Side {
    /// Byte range of each line, without the line break.
    pub lines: Vec<Range<usize>>,
    /// Changed parts of each line, relative to the line start, in order.
    pub marks: Vec<Vec<(Range<usize>, Mark)>>,
    /// Changed tokens.
    pub changed: usize,
    /// Moved tokens.
    pub moved: usize,
}

impl Side {
    pub fn line_changed(&self, line: u32) -> bool {
        self.marks.get(line as usize).is_some_and(|m| !m.is_empty())
    }
}

/// A row of the aligned view: a line of each side (or none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// Either line has a change.
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct StructuralDiff {
    pub lang: Lang,
    pub old: Side,
    pub new: Side,
    pub rows: Vec<Row>,
    /// One side did not parse cleanly; the result may be rougher.
    pub parse_errors: bool,
}

impl StructuralDiff {
    pub fn has_changes(&self) -> bool {
        self.rows.iter().any(|r| r.changed)
    }
}

/// Why a file is shown with the line diff instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// No grammar for this kind of file.
    Language,
    TooLarge,
    /// Not UTF-8 text.
    Binary,
    /// Nested too deeply or too many tokens.
    TooComplex,
}

/// Compares two versions of `path`.
pub fn diff_path(path: &str, old: &[u8], new: &[u8]) -> Result<StructuralDiff, Unavailable> {
    let lang = Lang::from_path(path).ok_or(Unavailable::Language)?;
    if old.len() > syntax::MAX_BYTES || new.len() > syntax::MAX_BYTES {
        return Err(Unavailable::TooLarge);
    }
    let (Ok(old), Ok(new)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return Err(Unavailable::Binary);
    };
    diff(lang, old, new)
}

pub fn diff(lang: Lang, old: &str, new: &str) -> Result<StructuralDiff, Unavailable> {
    let parse = |text: &str| syntax::parse(lang, text).ok_or(Unavailable::TooLarge);
    let (ta, tb) = (parse(old)?, parse(new)?);
    let a = Tree::build(ta.root_node(), old)?;
    let b = Tree::build(tb.root_node(), new)?;
    let mut d = Differ {
        a: &a,
        b: &b,
        sa: old,
        sb: new,
        ma: vec![LeafMark::Same; a.leaves.len()],
        mb: vec![LeafMark::Same; b.leaves.len()],
        pairs: Vec::new(),
        deadline: Instant::now() + TIME_LIMIT,
    };
    if let (Some(ra), Some(rb)) = (a.root, b.root) {
        d.region(&[ra], &[rb]);
    } else {
        d.mark_all(&a, a.root, true);
        d.mark_all(&b, b.root, false);
    }
    d.find_moves();
    let pairs = std::mem::take(&mut d.pairs);
    let (ma, mb) = (std::mem::take(&mut d.ma), std::mem::take(&mut d.mb));
    let old_side = side(old, &a, &ma);
    let new_side = side(new, &b, &mb);
    let rows = align(&a, &b, &old_side, &new_side, &pairs);
    Ok(StructuralDiff {
        lang,
        old: old_side,
        new: new_side,
        rows,
        parse_errors: ta.root_node().has_error() || tb.root_node().has_error(),
    })
}

// ----- Trees -----

struct TNode {
    kind: u16,
    hash: u64,
    kids: Vec<u32>,
    /// Leaves of the subtree, as indices into `Tree::leaves`.
    leaves: Range<u32>,
    atom: bool,
}

struct Leaf {
    range: Range<usize>,
    /// Hash of the text (comments ignore whitespace).
    hash: u64,
    prose: bool,
}

struct Tree {
    nodes: Vec<TNode>,
    leaves: Vec<Leaf>,
    root: Option<u32>,
}

fn hash_of(v: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

impl Tree {
    fn build(root: Node, src: &str) -> Result<Self, Unavailable> {
        let mut t = Tree {
            nodes: Vec::new(),
            leaves: Vec::new(),
            root: None,
        };
        t.root = t.add(root, src, 0)?;
        Ok(t)
    }

    fn add(&mut self, node: Node, src: &str, depth: usize) -> Result<Option<u32>, Unavailable> {
        if depth > MAX_DEPTH || self.leaves.len() > MAX_TOKENS {
            return Err(Unavailable::TooComplex);
        }
        let range = node.byte_range();
        if node.is_missing() || range.is_empty() {
            return Ok(None);
        }
        let start = self.leaves.len() as u32;
        if syntax::is_atom(&node) {
            let text = &src[range.clone()];
            if text.trim().is_empty() {
                return Ok(None);
            }
            let hash = if syntax::is_comment(&node) {
                hash_of(text.split_whitespace().collect::<Vec<_>>())
            } else {
                hash_of(text)
            };
            self.leaves.push(Leaf {
                range,
                hash,
                prose: syntax::is_prose(&node),
            });
            self.nodes.push(TNode {
                kind: node.kind_id(),
                hash: hash_of((node.kind_id(), hash)),
                kids: Vec::new(),
                leaves: start..start + 1,
                atom: true,
            });
            return Ok(Some(self.nodes.len() as u32 - 1));
        }
        let mut kids = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(id) = self.add(child, src, depth + 1)? {
                kids.push(id);
            }
        }
        if kids.is_empty() {
            return Ok(None);
        }
        let hash = hash_of((
            node.kind_id(),
            kids.iter()
                .map(|&k| self.nodes[k as usize].hash)
                .collect::<Vec<_>>(),
        ));
        self.nodes.push(TNode {
            kind: node.kind_id(),
            hash,
            kids,
            leaves: start..self.leaves.len() as u32,
            atom: false,
        });
        Ok(Some(self.nodes.len() as u32 - 1))
    }

    fn node(&self, id: u32) -> &TNode {
        &self.nodes[id as usize]
    }

    /// Leaves of consecutive siblings.
    fn leaves_of(&self, ids: &[u32]) -> Range<u32> {
        match (ids.first(), ids.last()) {
            (Some(&f), Some(&l)) => self.node(f).leaves.start..self.node(l).leaves.end,
            _ => 0..0,
        }
    }
}

// ----- Comparison -----

#[derive(Clone, PartialEq)]
enum LeafMark {
    Same,
    Novel,
    Moved,
    /// Changed words, as absolute byte ranges.
    Words(Vec<Range<usize>>),
}

struct Differ<'a> {
    a: &'a Tree,
    b: &'a Tree,
    sa: &'a str,
    sb: &'a str,
    ma: Vec<LeafMark>,
    mb: Vec<LeafMark>,
    /// Matched leaves, in order on both sides.
    pairs: Vec<(u32, u32)>,
    deadline: Instant,
}

impl Differ<'_> {
    fn mark_all(&mut self, t: &Tree, root: Option<u32>, old: bool) {
        if let Some(r) = root {
            let range = t.node(r).leaves.clone();
            self.novel(range, old);
        }
    }

    fn novel(&mut self, leaves: Range<u32>, old: bool) {
        let marks = if old { &mut self.ma } else { &mut self.mb };
        for i in leaves {
            marks[i as usize] = LeafMark::Novel;
        }
    }

    fn same(&mut self, x: u32, y: u32) {
        let (la, lb) = (self.a.node(x).leaves.clone(), self.b.node(y).leaves.clone());
        for (i, j) in la.zip(lb) {
            self.pairs.push((i, j));
        }
    }

    /// Compares two runs of sibling nodes.
    fn siblings(&mut self, ka: &[u32], kb: &[u32]) {
        let ha: Vec<u64> = ka.iter().map(|&k| self.a.node(k).hash).collect();
        let hb: Vec<u64> = kb.iter().map(|&k| self.b.node(k).hash).collect();
        for op in capture_diff_slices_deadline(Algorithm::Myers, &ha, &hb, Some(self.deadline)) {
            let (tag, ra, rb) = op.as_tag_tuple();
            if tag == DiffTag::Equal {
                for (x, y) in ka[ra].iter().zip(&kb[rb]) {
                    self.same(*x, *y);
                }
            } else {
                self.region(&ka[ra], &kb[rb]);
            }
        }
    }

    /// Siblings that differ: recurse into pairs of the same kind, else
    /// compare their tokens.
    fn region(&mut self, ra: &[u32], rb: &[u32]) {
        let (a, b) = (self.a, self.b);
        if ra.is_empty() || rb.is_empty() {
            self.novel(a.leaves_of(ra), true);
            self.novel(b.leaves_of(rb), false);
            return;
        }
        let pairable = ra.len() == rb.len()
            && ra.iter().zip(rb).all(|(&x, &y)| {
                let (nx, ny) = (a.node(x), b.node(y));
                nx.kind == ny.kind && !nx.atom && !ny.atom
            });
        if pairable {
            for (&x, &y) in ra.iter().zip(rb) {
                if a.node(x).hash == b.node(y).hash {
                    self.same(x, y);
                } else {
                    self.siblings(&a.node(x).kids, &b.node(y).kids);
                }
            }
            return;
        }
        self.tokens(a.leaves_of(ra), b.leaves_of(rb));
    }

    fn tokens(&mut self, la: Range<u32>, lb: Range<u32>) {
        let (a, b) = (self.a, self.b);
        let ha: Vec<u64> = la.clone().map(|i| a.leaves[i as usize].hash).collect();
        let hb: Vec<u64> = lb.clone().map(|i| b.leaves[i as usize].hash).collect();
        for op in capture_diff_slices_deadline(Algorithm::Myers, &ha, &hb, Some(self.deadline)) {
            let (tag, ra, rb) = op.as_tag_tuple();
            let ra = la.start + ra.start as u32..la.start + ra.end as u32;
            let rb = lb.start + rb.start as u32..lb.start + rb.end as u32;
            match tag {
                DiffTag::Equal => self.pairs.extend(ra.zip(rb)),
                DiffTag::Replace if ra.len() == rb.len() => {
                    for (i, j) in ra.zip(rb) {
                        self.changed_atoms(i, j);
                    }
                }
                _ => {
                    self.novel(ra, true);
                    self.novel(rb, false);
                }
            }
        }
    }

    /// Two tokens in the same place that differ: prose is marked word by word.
    fn changed_atoms(&mut self, i: u32, j: u32) {
        let (x, y) = (&self.a.leaves[i as usize], &self.b.leaves[j as usize]);
        if x.prose && y.prose {
            let (ta, tb) = (&self.sa[x.range.clone()], &self.sb[y.range.clone()]);
            if let Some((wa, wb)) = word_changes(ta, tb) {
                let shift = |v: Vec<Range<usize>>, by: usize| {
                    v.into_iter().map(|r| r.start + by..r.end + by).collect()
                };
                self.ma[i as usize] = LeafMark::Words(shift(wa, x.range.start));
                self.mb[j as usize] = LeafMark::Words(shift(wb, y.range.start));
                return;
            }
        }
        self.ma[i as usize] = LeafMark::Novel;
        self.mb[j as usize] = LeafMark::Novel;
    }

    /// Subtrees deleted on one side and added unchanged on the other.
    fn find_moves(&mut self) {
        let mut gone: HashMap<u64, Vec<u32>> = HashMap::new();
        collect_novel(self.a, self.a.root, &self.ma, &mut gone);
        let mut came: HashMap<u64, Vec<u32>> = HashMap::new();
        collect_novel(self.b, self.b.root, &self.mb, &mut came);
        for (hash, olds) in gone {
            let Some(news) = came.get(&hash) else {
                continue;
            };
            for (&x, &y) in olds.iter().zip(news) {
                for i in self.a.node(x).leaves.clone() {
                    self.ma[i as usize] = LeafMark::Moved;
                }
                for j in self.b.node(y).leaves.clone() {
                    self.mb[j as usize] = LeafMark::Moved;
                }
            }
        }
    }
}

/// The largest subtrees whose tokens are all novel, by hash.
fn collect_novel(t: &Tree, id: Option<u32>, marks: &[LeafMark], out: &mut HashMap<u64, Vec<u32>>) {
    let Some(id) = id else { return };
    let n = t.node(id);
    let all = n
        .leaves
        .clone()
        .all(|i| marks[i as usize] == LeafMark::Novel);
    if all && n.leaves.len() >= MIN_MOVED_TOKENS {
        out.entry(n.hash).or_default().push(id);
        return;
    }
    // Only subtrees with some novel token can hold a moved one.
    let any = n
        .leaves
        .clone()
        .any(|i| marks[i as usize] == LeafMark::Novel);
    if any {
        for &k in &n.kids {
            collect_novel(t, Some(k), marks, out);
        }
    }
}

/// Words of two texts that differ; `None` when they share too little.
type Ranges = Vec<Range<usize>>;

fn word_changes(a: &str, b: &str) -> Option<(Ranges, Ranges)> {
    let (wa, wb) = (words(a), words(b));
    let ta: Vec<&str> = wa.iter().map(|r| &a[r.clone()]).collect();
    let tb: Vec<&str> = wb.iter().map(|r| &b[r.clone()]).collect();
    let ops = similar::capture_diff_slices(Algorithm::Myers, &ta, &tb);
    let (mut out_a, mut out_b, mut same) = (Vec::new(), Vec::new(), 0);
    for op in ops {
        let (tag, ra, rb) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            same += ra.len();
            continue;
        }
        if !ra.is_empty() {
            out_a.push(wa[ra.start].start..wa[ra.end - 1].end);
        }
        if !rb.is_empty() {
            out_b.push(wb[rb.start].start..wb[rb.end - 1].end);
        }
    }
    (same * 3 >= wa.len().max(wb.len())).then_some((out_a, out_b))
}

/// Words and single punctuation characters (whitespace skipped).
fn words(s: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut in_word = false;
    for (i, c) in s.char_indices() {
        let end = i + c.len_utf8();
        if c.is_alphanumeric() || c == '_' {
            match out.last_mut() {
                Some(last) if in_word => last.end = end,
                _ => out.push(i..end),
            }
            in_word = true;
        } else {
            if !c.is_whitespace() {
                out.push(i..end);
            }
            in_word = false;
        }
    }
    out
}

// ----- Lines -----

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, _) in text.match_indices('\n') {
        let end = if text[..i].ends_with('\r') { i - 1 } else { i };
        out.push(start..end);
        start = i + 1;
    }
    if start < text.len() {
        out.push(start..text.len());
    }
    out
}

fn line_of(lines: &[Range<usize>], byte: usize) -> u32 {
    lines
        .partition_point(|r| r.end < byte)
        .min(lines.len().saturating_sub(1)) as u32
}

fn side(text: &str, t: &Tree, marks: &[LeafMark]) -> Side {
    let lines = line_ranges(text);
    let mut out = Side {
        marks: vec![Vec::new(); lines.len()],
        ..Default::default()
    };
    let add = |range: Range<usize>, mark: Mark, marks: &mut Vec<Vec<(Range<usize>, Mark)>>| {
        // Split across lines.
        let mut l = line_of(&lines, range.start) as usize;
        while l < lines.len() && lines[l].start < range.end {
            let lr = &lines[l];
            let (s, e) = (range.start.max(lr.start), range.end.min(lr.end));
            if s < e {
                marks[l].push((s - lr.start..e - lr.start, mark));
            }
            l += 1;
        }
    };
    for (leaf, mark) in t.leaves.iter().zip(marks) {
        match mark {
            LeafMark::Same => {}
            LeafMark::Novel => {
                out.changed += 1;
                add(leaf.range.clone(), Mark::Novel, &mut out.marks);
            }
            LeafMark::Moved => {
                out.moved += 1;
                add(leaf.range.clone(), Mark::Moved, &mut out.marks);
            }
            LeafMark::Words(ws) => {
                out.changed += 1;
                for r in ws {
                    add(r.clone(), Mark::Novel, &mut out.marks);
                }
            }
        }
    }
    for m in &mut out.marks {
        m.sort_by_key(|(r, _)| r.start);
    }
    out.lines = lines;
    out
}

/// Lines of matched tokens become rows; the lines between two such rows are
/// put next to each other.
fn align(a: &Tree, b: &Tree, old: &Side, new: &Side, pairs: &[(u32, u32)]) -> Vec<Row> {
    let mut anchors: Vec<(u32, u32)> = Vec::new();
    for &(i, j) in pairs {
        let la = line_of(&old.lines, a.leaves[i as usize].range.start);
        let lb = line_of(&new.lines, b.leaves[j as usize].range.start);
        match anchors.last() {
            Some(&(pa, pb)) if la <= pa || lb <= pb => {}
            _ => anchors.push((la, lb)),
        }
    }
    let (na, nb) = (old.lines.len() as u32, new.lines.len() as u32);
    let mut rows = Vec::with_capacity(na.max(nb) as usize);
    let row = |o: Option<u32>, n: Option<u32>| Row {
        old: o,
        new: n,
        changed: o.is_some_and(|l| old.line_changed(l)) || n.is_some_and(|l| new.line_changed(l)),
    };
    let (mut pa, mut pb) = (0u32, 0u32);
    for (la, lb) in anchors.into_iter().chain(std::iter::once((na, nb))) {
        let (ga, gb) = (la - pa, lb - pb);
        for k in 0..ga.max(gb) {
            rows.push(row((k < ga).then_some(pa + k), (k < gb).then_some(pb + k)));
        }
        if la < na && lb < nb {
            rows.push(row(Some(la), Some(lb)));
        }
        (pa, pb) = (la + 1, lb + 1);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust(old: &str, new: &str) -> StructuralDiff {
        diff(Lang::Rust, old, new).unwrap()
    }

    fn marked<'a>(text: &'a str, s: &Side, mark: Mark) -> Vec<&'a str> {
        let mut out = Vec::new();
        for (l, marks) in s.lines.iter().zip(&s.marks) {
            for (r, m) in marks {
                if *m == mark {
                    out.push(&text[l.start + r.start..l.start + r.end]);
                }
            }
        }
        out
    }

    #[test]
    fn ignores_formatting() {
        let old = "fn main() {\n    let x = foo(1, 2);\n}\n";
        let new =
            "fn main()\n{\n        let x = foo(\n            1,\n            2,\n        );\n}\n";
        let d = rust(old, new);
        // Only the trailing comma is new.
        assert_eq!(marked(new, &d.new, Mark::Novel), [","]);
        assert!(marked(old, &d.old, Mark::Novel).is_empty());
        let d = rust("fn a() {}\n", "fn  a( )  {   }\n");
        assert!(!d.has_changes());
    }

    #[test]
    fn marks_changed_expressions() {
        let old = "fn f() -> u32 {\n    let a = 1;\n    a + 2\n}\n";
        let new = "fn f() -> u32 {\n    let a = 1;\n    a * 3\n}\n";
        let d = rust(old, new);
        assert_eq!(marked(old, &d.old, Mark::Novel), ["+", "2"]);
        assert_eq!(marked(new, &d.new, Mark::Novel), ["*", "3"]);
        // Rows: the changed line is paired with its new version.
        let changed: Vec<_> = d.rows.iter().filter(|r| r.changed).collect();
        assert_eq!(changed.len(), 1);
        assert_eq!((changed[0].old, changed[0].new), (Some(2), Some(2)));
    }

    #[test]
    fn detects_moves_and_comment_words() {
        let old = "fn a() { one(); }\nfn b() { two(); }\nfn c() { three(); }\n";
        let new = "fn b() { two(); }\nfn c() { three(); }\nfn a() { one(); }\n";
        let d = rust(old, new);
        assert!(d.old.moved > 0 && d.new.moved > 0);
        assert_eq!(d.old.changed + d.new.changed, 0);

        let old = "// the quick brown fox\nfn a() {}\n";
        let new = "// the quick red fox\nfn a() {}\n";
        let d = rust(old, new);
        assert_eq!(marked(old, &d.old, Mark::Novel), ["brown"]);
        assert_eq!(marked(new, &d.new, Mark::Novel), ["red"]);
    }

    #[test]
    fn added_function_and_other_languages() {
        let old = "def a():\n    return 1\n";
        let new = "def a():\n    return 1\n\n\ndef b():\n    return 2\n";
        let d = diff(Lang::Python, old, new).unwrap();
        assert_eq!(d.old.changed, 0);
        assert!(d.new.changed > 0);
        assert!(d.rows.iter().filter(|r| r.changed).all(|r| r.old.is_none()));

        let d = diff_path(
            "a.json",
            b"{\"a\": 1, \"b\": [1,2]}",
            b"{\n  \"a\": 1,\n  \"b\": [1, 2]\n}",
        )
        .unwrap();
        assert!(!d.has_changes());
        assert_eq!(
            diff_path("a.txt", b"", b"").unwrap_err(),
            Unavailable::Language
        );
        assert_eq!(
            diff_path("a.rs", b"\xff", b"").unwrap_err(),
            Unavailable::Binary
        );
    }
}
