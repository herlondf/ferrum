//! Languages Ferrum parses with tree-sitter, for the structural diff and the
//! semantic merge. Each grammar is a cargo feature (`lang-*`, all on by
//! default); a language whose feature is off is simply not supported.

use tree_sitter::{Language, Node, Parser, Tree};

/// Files larger than this are not parsed (the line diff is used instead).
pub const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Json,
    Yaml,
    Toml,
    Html,
    Css,
    Markdown,
}

impl Lang {
    pub const ALL: [Lang; 16] = [
        Lang::Rust,
        Lang::JavaScript,
        Lang::TypeScript,
        Lang::Tsx,
        Lang::Python,
        Lang::Go,
        Lang::Java,
        Lang::C,
        Lang::Cpp,
        Lang::CSharp,
        Lang::Json,
        Lang::Yaml,
        Lang::Toml,
        Lang::Html,
        Lang::Css,
        Lang::Markdown,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::JavaScript => "JavaScript",
            Lang::TypeScript => "TypeScript",
            Lang::Tsx => "TSX",
            Lang::Python => "Python",
            Lang::Go => "Go",
            Lang::Java => "Java",
            Lang::C => "C",
            Lang::Cpp => "C++",
            Lang::CSharp => "C#",
            Lang::Json => "JSON",
            Lang::Yaml => "YAML",
            Lang::Toml => "TOML",
            Lang::Html => "HTML",
            Lang::Css => "CSS",
            Lang::Markdown => "Markdown",
        }
    }

    /// The language of a file, by its extension, if its grammar is built in.
    pub fn from_path(path: &str) -> Option<Lang> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        let lang = match ext.as_str() {
            "rs" => Lang::Rust,
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "py" | "pyi" | "pyw" => Lang::Python,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "c" | "h" => Lang::C,
            "cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ipp" => Lang::Cpp,
            "cs" => Lang::CSharp,
            "json" => Lang::Json,
            "yaml" | "yml" => Lang::Yaml,
            "toml" => Lang::Toml,
            "html" | "htm" | "xhtml" => Lang::Html,
            "css" => Lang::Css,
            "md" | "markdown" => Lang::Markdown,
            _ => return None,
        };
        lang.grammar().is_some().then_some(lang)
    }

    /// The tree-sitter grammar, when its feature is built in.
    pub fn grammar(self) -> Option<Language> {
        let f: Option<tree_sitter_language::LanguageFn> = match self {
            #[cfg(feature = "lang-rust")]
            Lang::Rust => Some(tree_sitter_rust::LANGUAGE),
            #[cfg(feature = "lang-js")]
            Lang::JavaScript => Some(tree_sitter_javascript::LANGUAGE),
            #[cfg(feature = "lang-ts")]
            Lang::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT),
            #[cfg(feature = "lang-ts")]
            Lang::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX),
            #[cfg(feature = "lang-python")]
            Lang::Python => Some(tree_sitter_python::LANGUAGE),
            #[cfg(feature = "lang-go")]
            Lang::Go => Some(tree_sitter_go::LANGUAGE),
            #[cfg(feature = "lang-java")]
            Lang::Java => Some(tree_sitter_java::LANGUAGE),
            #[cfg(feature = "lang-c")]
            Lang::C => Some(tree_sitter_c::LANGUAGE),
            #[cfg(feature = "lang-cpp")]
            Lang::Cpp => Some(tree_sitter_cpp::LANGUAGE),
            #[cfg(feature = "lang-csharp")]
            Lang::CSharp => Some(tree_sitter_c_sharp::LANGUAGE),
            #[cfg(feature = "lang-json")]
            Lang::Json => Some(tree_sitter_json::LANGUAGE),
            #[cfg(feature = "lang-yaml")]
            Lang::Yaml => Some(tree_sitter_yaml::LANGUAGE),
            #[cfg(feature = "lang-toml")]
            Lang::Toml => Some(tree_sitter_toml_ng::LANGUAGE),
            #[cfg(feature = "lang-html")]
            Lang::Html => Some(tree_sitter_html::LANGUAGE),
            #[cfg(feature = "lang-css")]
            Lang::Css => Some(tree_sitter_css::LANGUAGE),
            #[cfg(feature = "lang-markdown")]
            Lang::Markdown => Some(tree_sitter_md::LANGUAGE),
            #[allow(unreachable_patterns)]
            _ => None,
        };
        f.map(Language::new)
    }
}

/// The languages built into this binary.
pub fn available() -> Vec<Lang> {
    Lang::ALL
        .into_iter()
        .filter(|l| l.grammar().is_some())
        .collect()
}

/// Parses `text`; `None` when the language is not built in or the file is too large.
pub fn parse(lang: Lang, text: &str) -> Option<Tree> {
    if text.len() > MAX_BYTES {
        return None;
    }
    let mut parser = Parser::new();
    parser.set_language(&lang.grammar()?).ok()?;
    parser.parse(text, None)
}

pub(crate) fn is_comment(node: &Node) -> bool {
    node.kind().contains("comment")
}

/// Nodes kept whole by the diff: strings and comments are compared as text.
pub(crate) fn is_atom(node: &Node) -> bool {
    if node.child_count() == 0 {
        return true;
    }
    let k = node.kind();
    is_comment(node)
        || k.contains("string")
        || k == "char_literal"
        || k == "character_literal"
        || k == "heredoc_body"
}

/// Atoms whose text is prose (comments, strings, Markdown and HTML text):
/// changes inside them are shown word by word.
pub(crate) fn is_prose(node: &Node) -> bool {
    let k = node.kind();
    is_comment(node) || k.contains("string") || k == "inline" || k == "text"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_languages_and_parses() {
        assert_eq!(Lang::from_path("src/main.rs"), Some(Lang::Rust));
        assert_eq!(Lang::from_path("a/b/App.TSX"), Some(Lang::Tsx));
        assert_eq!(Lang::from_path("Makefile"), None);
        assert_eq!(Lang::from_path("notes.txt"), None);
        assert_eq!(available().len(), Lang::ALL.len());
        for lang in available() {
            assert!(parse(lang, "").is_some(), "{}", lang.name());
        }
        let tree = parse(Lang::Rust, "fn main() { let x = \"a b\"; }").unwrap();
        assert!(!tree.root_node().has_error());
    }
}
