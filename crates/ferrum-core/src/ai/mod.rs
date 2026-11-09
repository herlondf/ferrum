//! AI providers: a command line tool (Claude Code, Codex…), a local Ollama
//! server or any OpenAI-compatible endpoint (LM Studio, llama.cpp, vLLM).
//! One entry point, [`complete`], streams the answer of any of them.
//!
//! Also the pieces the features share: where data goes ([`Destination`]),
//! masking secrets before sending ([`redact`]), reading JSON out of model
//! answers ([`json`]), natural-language history search ([`search`]) and
//! splitting local changes into commits ([`split`]).

mod command;
mod http;
pub mod json;
pub mod prompts;
pub mod redact;
pub mod search;
pub mod split;
#[cfg(test)]
mod tests;

use std::sync::atomic::AtomicBool;

/// Where requests go and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    /// A program run through the shell with the prompt on standard input.
    Command { command: String },
    /// An Ollama server (`/api/chat`, `/api/tags`).
    Ollama { url: String, model: String },
    /// An OpenAI-compatible server; `url` ends with `/v1` (LM Studio,
    /// llama.cpp server, vLLM…). The key is optional.
    OpenAi {
        url: String,
        key: String,
        model: String,
    },
}

/// A system prompt (instructions) and the user's request with its context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

impl Prompt {
    pub fn new(system: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            user: user.into(),
        }
    }

    /// Both parts in one text, for tools that take a single prompt.
    pub fn joined(&self) -> String {
        if self.system.trim().is_empty() {
            return self.user.clone();
        }
        format!("{}\n\n{}", self.system.trim_end(), self.user)
    }

    pub fn len(&self) -> usize {
        self.system.len() + self.user.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Where the data of a request ends up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// This computer or the local network (`host`).
    Local { host: String },
    /// A service on the internet: a known name ("OpenAI") or the host.
    Remote { name: String },
    /// A command line tool: `service` when Ferrum knows where it sends
    /// data (Claude Code: Anthropic), else only the program's name.
    Tool {
        program: String,
        service: Option<String>,
    },
}

impl Destination {
    pub fn is_local(&self) -> bool {
        matches!(self, Destination::Local { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AiError {
    #[error("no {0} is set")]
    Missing(&'static str),
    #[error("could not start {program}: {detail}")]
    Start { program: String, detail: String },
    #[error("{program} failed: {detail}")]
    Failed { program: String, detail: String },
    #[error("could not connect to {url}: {detail}")]
    Connect { url: String, detail: String },
    #[error("HTTP {status}: {detail}")]
    Http { status: u16, detail: String },
    #[error("unexpected answer: {0}")]
    BadReply(String),
    #[error("the answer was empty")]
    Empty,
    #[error("cancelled")]
    Cancelled,
}

impl Provider {
    /// Where its requests go.
    pub fn destination(&self) -> Destination {
        match self {
            Provider::Command { command } => {
                let program = program_name(command);
                let service = known_tool(&program).map(str::to_owned);
                if service.as_deref() == Some("") {
                    return Destination::Local {
                        host: "localhost".into(),
                    };
                }
                Destination::Tool { program, service }
            }
            Provider::Ollama { url, .. } | Provider::OpenAi { url, .. } => url_destination(url),
        }
    }

    /// The model asked, if the provider has one.
    pub fn model(&self) -> Option<&str> {
        match self {
            Provider::Command { .. } => None,
            Provider::Ollama { model, .. } | Provider::OpenAi { model, .. } => Some(model),
        }
    }
}

/// Sends `prompt` and returns the answer; `on_text` gets each piece as it
/// arrives. Stops with [`AiError::Cancelled`] once `cancel` is set.
pub fn complete(
    provider: &Provider,
    prompt: &Prompt,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
) -> Result<String, AiError> {
    let out = match provider {
        Provider::Command { command } => command::run(command, &prompt.joined(), cancel, on_text)?,
        Provider::Ollama { url, model } => http::ollama_chat(url, model, prompt, cancel, on_text)?,
        Provider::OpenAi { url, key, model } => {
            http::openai_chat(url, key, model, prompt, cancel, on_text)?
        }
    };
    if out.trim().is_empty() {
        return Err(AiError::Empty);
    }
    Ok(out)
}

/// The models a server offers (not available for command line tools).
pub fn models(provider: &Provider) -> Result<Vec<String>, AiError> {
    match provider {
        Provider::Command { .. } => Ok(Vec::new()),
        Provider::Ollama { url, .. } => http::ollama_models(url),
        Provider::OpenAi { url, key, .. } => http::openai_models(url, key),
    }
}

/// What "Test connection" found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The server answered with these models.
    Models(Vec<String>),
    /// The program was found at this path.
    Program(String),
}

/// Checks that the provider can be reached, without sending any data.
pub fn test(provider: &Provider) -> Result<Check, AiError> {
    match provider {
        Provider::Command { command } => {
            let program = command.split_whitespace().next().unwrap_or_default();
            if program.is_empty() {
                return Err(AiError::Missing("command"));
            }
            command::find_program(program)
                .map(|p| Check::Program(p.display().to_string()))
                .ok_or_else(|| AiError::Start {
                    program: program.to_owned(),
                    detail: "not found on the PATH".into(),
                })
        }
        _ => models(provider).map(Check::Models),
    }
}

/// Removes `<think>…</think>` reasoning that some local models print
/// before their answer. An unfinished block is removed too.
pub fn strip_thinking(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out.trim_start().to_owned(),
        }
    }
    out.push_str(rest);
    out.trim_start().to_owned()
}

/// Whether the model is still inside a `<think>` block.
pub fn is_thinking(text: &str) -> bool {
    text.rfind("<think>")
        .is_some_and(|i| !text[i..].contains("</think>"))
}

/// Cuts `text` to at most `max` bytes at a line end, saying so.
pub fn cap(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let end = text[..end].rfind('\n').map_or(end, |i| i + 1);
    (format!("{}[… cut: too long]\n", &text[..end]), true)
}

fn program_name(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or_default();
    let name = first.rsplit(['/', '\\']).next().unwrap_or(first);
    let name = name.to_lowercase();
    for ext in [".exe", ".cmd", ".bat"] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem.to_owned();
        }
    }
    name
}

/// The service a known tool sends data to; `""` for tools that run models
/// on this computer.
fn known_tool(program: &str) -> Option<&'static str> {
    Some(match program {
        "claude" => "Anthropic",
        "codex" | "openai" => "OpenAI",
        "gemini" => "Google",
        "copilot" | "gh" => "GitHub",
        "aichat" | "llm" | "sgpt" | "aider" => return None,
        "ollama" | "llamafile" | "llama-cli" => "",
        _ => return None,
    })
}

/// Host of an `http(s)://host:port/path` URL.
pub fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next().unwrap_or_default().to_lowercase();
    }
    authority
        .split(':')
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

/// Whether `host` is this computer or a private network address.
pub fn is_local_host(host: &str) -> bool {
    if matches!(
        host,
        "localhost" | "::1" | "0.0.0.0" | "host.docker.internal"
    ) || host.ends_with(".local")
        || host.ends_with(".localhost")
        || host.ends_with(".lan")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
        || !host.contains('.') && !host.contains(':') && !host.is_empty()
    {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        };
    }
    false
}

fn url_destination(url: &str) -> Destination {
    let host = url_host(url);
    if is_local_host(&host) {
        return Destination::Local { host };
    }
    let known = [
        ("api.openai.com", "OpenAI"),
        ("openrouter.ai", "OpenRouter"),
        ("api.anthropic.com", "Anthropic"),
        ("api.groq.com", "Groq"),
        ("api.mistral.ai", "Mistral"),
        ("api.deepseek.com", "DeepSeek"),
        ("api.together.xyz", "Together AI"),
        ("generativelanguage.googleapis.com", "Google"),
        ("api.x.ai", "xAI"),
    ];
    let name = known
        .iter()
        .find(|(h, _)| host == *h)
        .map_or(host.clone(), |(_, n)| (*n).to_owned());
    Destination::Remote { name }
}
