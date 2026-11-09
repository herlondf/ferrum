//! Ollama and OpenAI-compatible servers, with streamed answers.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::{AiError, Prompt};

fn agent(read_timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        // Local models can take minutes to load before the first word.
        .timeout_recv_response(read_timeout)
        .http_status_as_error(false)
        .user_agent("ferrum-git-client")
        .build()
        .into()
}

fn join(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim().trim_end_matches('/'), path)
}

fn connect_error(url: &str, e: ureq::Error) -> AiError {
    AiError::Connect {
        url: url.to_owned(),
        detail: e.to_string(),
    }
}

/// The readable part of an error body: `{"error": "…"}` (Ollama) or
/// `{"error": {"message": "…"}}` (OpenAI), else the text.
fn error_detail(text: &str) -> String {
    let detail = serde_json::from_str::<Value>(text).ok().and_then(|v| {
        let e = v.get("error")?;
        e.as_str()
            .or_else(|| e.get("message").and_then(Value::as_str))
            .map(str::to_owned)
    });
    let text = detail.unwrap_or_else(|| text.trim().to_owned());
    text.chars().take(400).collect()
}

fn checked(
    url: &str,
    resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<ureq::http::Response<ureq::Body>, AiError> {
    let mut resp = resp.map_err(|e| connect_error(url, e))?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(resp);
    }
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    Err(AiError::Http {
        status,
        detail: error_detail(&text),
    })
}

fn get_json(url: &str, key: &str) -> Result<Value, AiError> {
    let mut req = agent(Some(Duration::from_secs(15))).get(url);
    if !key.trim().is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", key.trim()));
    }
    let mut resp = checked(url, req.call())?;
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| AiError::BadReply(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| AiError::BadReply(e.to_string()))
}

fn messages(prompt: &Prompt) -> Value {
    let mut list = Vec::new();
    if !prompt.system.trim().is_empty() {
        list.push(json!({"role": "system", "content": prompt.system}));
    }
    list.push(json!({"role": "user", "content": prompt.user}));
    Value::Array(list)
}

/// Reads a streamed body line by line; `line` returns `Ok(true)` at the end.
fn stream_lines(
    resp: ureq::http::Response<ureq::Body>,
    cancel: &AtomicBool,
    mut line: impl FnMut(&str) -> Result<bool, AiError>,
) -> Result<(), AiError> {
    let reader = BufReader::new(resp.into_body().into_reader());
    for l in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let l = l.map_err(|e| AiError::BadReply(e.to_string()))?;
        if l.trim().is_empty() {
            continue;
        }
        if line(l.trim())? {
            break;
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError::Cancelled);
    }
    Ok(())
}

fn require_model(model: &str) -> Result<(), AiError> {
    if model.trim().is_empty() {
        return Err(AiError::Missing("model"));
    }
    Ok(())
}

pub fn ollama_chat(
    base: &str,
    model: &str,
    prompt: &Prompt,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
) -> Result<String, AiError> {
    require_model(model)?;
    let url = join(base, "api/chat");
    let body = json!({
        "model": model.trim(),
        "messages": messages(prompt),
        "stream": true,
    });
    let resp = checked(
        &url,
        agent(Some(Duration::from_secs(600)))
            .post(&url)
            .send_json(&body),
    )?;
    let mut out = String::new();
    stream_lines(resp, cancel, |l| {
        let v: Value = serde_json::from_str(l).map_err(|e| AiError::BadReply(e.to_string()))?;
        if let Some(e) = v.get("error").and_then(Value::as_str) {
            return Err(AiError::BadReply(e.to_owned()));
        }
        if let Some(t) = v.pointer("/message/content").and_then(Value::as_str)
            && !t.is_empty()
        {
            out.push_str(t);
            on_text(t);
        }
        Ok(v.get("done").and_then(Value::as_bool).unwrap_or(false))
    })?;
    Ok(out)
}

pub fn ollama_models(base: &str) -> Result<Vec<String>, AiError> {
    let v = get_json(&join(base, "api/tags"), "")?;
    let list = v
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::BadReply("no model list".into()))?;
    Ok(list
        .iter()
        .filter_map(|m| m.get("name").or_else(|| m.get("model")))
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect())
}

pub fn openai_chat(
    base: &str,
    key: &str,
    model: &str,
    prompt: &Prompt,
    cancel: &AtomicBool,
    on_text: &mut dyn FnMut(&str),
) -> Result<String, AiError> {
    require_model(model)?;
    let url = join(base, "chat/completions");
    let body = json!({
        "model": model.trim(),
        "messages": messages(prompt),
        "stream": true,
    });
    let mut req = agent(Some(Duration::from_secs(600)))
        .post(&url)
        .header("Accept", "text/event-stream");
    if !key.trim().is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", key.trim()));
    }
    let mut resp = checked(&url, req.send_json(&body))?;
    let streamed = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|t| t.contains("event-stream"));
    let mut out = String::new();
    if !streamed {
        // A server that ignored `stream`: one JSON answer.
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| AiError::BadReply(e.to_string()))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| AiError::BadReply(e.to_string()))?;
        let t = v
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| AiError::BadReply(error_detail(&text)))?;
        on_text(t);
        return Ok(t.to_owned());
    }
    stream_lines(resp, cancel, |l| {
        let Some(data) = l.strip_prefix("data:") else {
            return Ok(false);
        };
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(true);
        }
        let v: Value = serde_json::from_str(data).map_err(|e| AiError::BadReply(e.to_string()))?;
        if v.get("error").is_some() {
            return Err(AiError::BadReply(error_detail(data)));
        }
        if let Some(t) = v
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            && !t.is_empty()
        {
            out.push_str(t);
            on_text(t);
        }
        Ok(false)
    })?;
    Ok(out)
}

pub fn openai_models(base: &str, key: &str) -> Result<Vec<String>, AiError> {
    let v = get_json(&join(base, "models"), key)?;
    let list = v
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::BadReply("no model list".into()))?;
    Ok(list
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}
