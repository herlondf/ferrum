//! Translations. English text is the key: `t!("Check out {}", name)` looks the
//! template up in the current language's table and fills `{}` in order.
//! Missing translations fall back to English.
//!
//! To translate a new string, wrap it in `t!` and add it to a table in
//! `pt_br/`; the `every_key_is_translated` test lists what is missing.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

mod pt_br;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    /// Follow the operating system.
    #[default]
    System,
    English,
    PortugueseBrazil,
}

impl Language {
    pub const ALL: [Language; 3] = [
        Language::System,
        Language::English,
        Language::PortugueseBrazil,
    ];

    /// Name shown in the language picker, always in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::System => "System / Sistema",
            Language::English => "English",
            Language::PortugueseBrazil => "Português (Brasil)",
        }
    }

    /// The concrete language to use, resolving `System` from the OS locale.
    fn resolve(self) -> Language {
        match self {
            Language::System => match sys_locale::get_locale() {
                Some(l) if l.to_lowercase().starts_with("pt") => Language::PortugueseBrazil,
                _ => Language::English,
            },
            other => other,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Sets the language used by `t!`.
pub fn set_language(lang: Language) {
    let code = match lang.resolve() {
        Language::PortugueseBrazil => 1,
        _ => 0,
    };
    CURRENT.store(code, Ordering::Relaxed);
}

/// Whether `t!` translates to Brazilian Portuguese.
pub fn portuguese() -> bool {
    CURRENT.load(Ordering::Relaxed) == 1
}

static PT_BR: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    pt_br::TABLES
        .iter()
        .flat_map(|t| t.iter().copied())
        .collect()
});

/// Translates an English template.
pub fn tr(key: &'static str) -> &'static str {
    match CURRENT.load(Ordering::Relaxed) {
        1 => PT_BR.get(key).copied().unwrap_or(key),
        _ => key,
    }
}

/// Replaces each `{}` in `template` with the next argument.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        match args.next() {
            Some(a) => out.push_str(&a.to_string()),
            None => out.push_str("{}"),
        }
        rest = &rest[pos + 2..];
    }
    out.push_str(rest);
    out
}

/// `t!("Text")` or `t!("Check out {}", name)`: translated text as a `String`.
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::tr($key).to_owned()
    };
    ($key:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($key), &[$(&$arg as &dyn std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders_in_order() {
        assert_eq!(fill("Merge {} into {}", &[&"a", &"b"]), "Merge a into b");
        assert_eq!(fill("{} files", &[&3]), "3 files");
        assert_eq!(fill("no args", &[]), "no args");
    }

    /// Every `t!("…")` key in the sources, with how many `{}` it has.
    fn keys_in_sources() -> Vec<(String, usize)> {
        let mut keys = Vec::new();
        for src in crate::ui_tests::SOURCES {
            // Comments may mention `t!` examples; only code counts.
            let code: String = src
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            let mut rest = code.as_str();
            // Built with concat! so this line does not match itself.
            while let Some(pos) = rest.find(concat!("t", "!(")) {
                // Skip identifiers ending in t (e.g. `print!`) by checking the previous char.
                let before = &rest[..pos];
                rest = &rest[pos + 3..];
                if before
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                // rustfmt may put the string on the next line.
                let trimmed = rest.trim_start();
                let Some(after_quote) = trimmed.strip_prefix('"') else {
                    continue;
                };
                rest = after_quote;
                let Some(end) = find_string_end(rest) else {
                    continue;
                };
                let key = unescape(&rest[..end]);
                let holes = key.matches("{}").count();
                keys.push((key, holes));
                rest = &rest[end..];
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }

    fn find_string_end(s: &str) -> Option<usize> {
        let mut escaped = false;
        for (i, c) in s.char_indices() {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => return Some(i),
                _ => {}
            }
        }
        None
    }

    fn unescape(s: &str) -> String {
        s.replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    }

    #[test]
    fn every_key_is_translated() {
        let keys = keys_in_sources();
        assert!(!keys.is_empty());
        let missing: Vec<&str> = keys
            .iter()
            .filter(|(k, _)| !PT_BR.contains_key(k.as_str()))
            .map(|(k, _)| k.as_str())
            .collect();
        assert!(
            missing.is_empty(),
            "{} keys without a pt-BR translation:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    #[test]
    fn repeated_keys_agree() {
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for table in pt_br::TABLES {
            for (k, v) in table.iter() {
                if let Some(prev) = seen.insert(k, v) {
                    assert_eq!(prev, *v, "different translations for {k:?}");
                }
            }
        }
    }

    #[test]
    fn translations_keep_placeholders() {
        for (key, value) in PT_BR.iter() {
            assert_eq!(
                key.matches("{}").count(),
                value.matches("{}").count(),
                "placeholders differ: {key:?} -> {value:?}"
            );
        }
    }

    #[test]
    fn no_unused_translations() {
        let keys: std::collections::HashSet<String> =
            keys_in_sources().into_iter().map(|(k, _)| k).collect();
        let unused: Vec<&&str> = PT_BR.keys().filter(|k| !keys.contains(**k)).collect();
        assert!(
            unused.is_empty(),
            "translations no longer used in code: {unused:?}"
        );
    }
}
