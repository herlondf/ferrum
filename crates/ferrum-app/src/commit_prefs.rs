//! Saved options of the local changes view and the commit box.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const MAX_RECENT_MESSAGES: usize = 20;

/// How the changed files are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ListMode {
    /// Unstaged and staged lists of paths.
    #[default]
    Flat,
    /// Unstaged and staged lists grouped by folder.
    Tree,
    /// One list with a checkbox to stage each file.
    Combined,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitPrefs {
    pub list_mode: ListMode,
    pub hide_untracked: bool,
    pub show_ignored: bool,
    /// Subject length shown as a warning.
    pub subject_soft_limit: usize,
    /// Subject length shown as an error; the extra characters are marked.
    pub subject_hard_limit: usize,
    /// Column of the ruler in the description (0 hides it).
    pub ruler_column: usize,
    /// Files bigger than this many megabytes and not in LFS get a warning (0 = off).
    pub large_file_mb: u64,
    pub signoff: bool,
    pub push_after_commit: bool,
    /// Recent commit messages, newest first.
    pub recent_messages: Vec<String>,
    /// Unfinished messages, by worktree path.
    pub drafts: HashMap<String, String>,
}

impl Default for CommitPrefs {
    fn default() -> Self {
        Self {
            list_mode: ListMode::Flat,
            hide_untracked: false,
            show_ignored: false,
            subject_soft_limit: 50,
            subject_hard_limit: 72,
            ruler_column: 72,
            large_file_mb: 10,
            signoff: false,
            push_after_commit: false,
            recent_messages: Vec::new(),
            drafts: HashMap::new(),
        }
    }
}

impl CommitPrefs {
    pub fn add_recent_message(&mut self, message: &str) {
        let message = message.trim();
        if message.is_empty() {
            return;
        }
        self.recent_messages.retain(|m| m != message);
        self.recent_messages.insert(0, message.to_owned());
        self.recent_messages.truncate(MAX_RECENT_MESSAGES);
    }

    pub fn large_file_threshold(&self) -> Option<u64> {
        (self.large_file_mb > 0).then(|| self.large_file_mb * 1024 * 1024)
    }
}

pub const DEFAULT_AI_COMMAND: &str = "claude -p";
pub const DEFAULT_AI_PROMPT: &str = "Write a git commit message for the staged changes below. \
Use the imperative mood. First line: a summary of at most 50 characters. Then a blank line \
and a short description of what changed and why, wrapped at 72 columns. Reply with the \
message only, no quotes or Markdown.\n\n{diff}";

/// Which kind of AI provider is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AiProvider {
    /// A local Ollama server.
    #[default]
    Ollama,
    /// An OpenAI-compatible server (LM Studio, llama.cpp, vLLM…).
    OpenAi,
    /// A command line tool (Claude Code, Codex…).
    Command,
}

pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
pub const DEFAULT_OPENAI_URL: &str = "http://localhost:1234/v1";

/// AI settings (Settings > AI): the provider and the commit message prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiPrefs {
    /// Command run through the shell; it gets the prompt on standard input.
    pub command: String,
    /// Prompt template; `{diff}` is replaced by the staged diff.
    pub prompt: String,
    /// Nothing is sent to any provider until this is on.
    pub enabled: bool,
    pub provider: AiProvider,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_url: String,
    /// Optional; most local servers need none.
    pub openai_key: String,
    pub openai_model: String,
    /// Most context sent with one request, in KB.
    pub max_kb: u32,
}

impl Default for AiPrefs {
    fn default() -> Self {
        Self {
            command: DEFAULT_AI_COMMAND.into(),
            prompt: DEFAULT_AI_PROMPT.into(),
            enabled: false,
            provider: AiProvider::default(),
            ollama_url: DEFAULT_OLLAMA_URL.into(),
            ollama_model: String::new(),
            openai_url: DEFAULT_OPENAI_URL.into(),
            openai_key: String::new(),
            openai_model: String::new(),
            max_kb: 48,
        }
    }
}
