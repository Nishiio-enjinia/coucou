// Claude API client — the same integration as ClaudeService.swift: multi-turn
// chat with web search, and files sent as document/image/text blocks.
//
// Everything happens here rather than in the island: the API key never leaves
// the Credential Manager, and file bytes never cross the IPC boundary.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::secrets;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Server-side fallback: on a policy decline the API retries the same request on
/// a fallback model inside the same call, so the island never shows a dead end.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 4096;
/// Text and code files are inlined; anything larger is skipped, as on macOS.
const MAX_INLINE_TEXT: u64 = 200_000;

pub const DEFAULT_MODEL: &str = "claude-opus-5";

const SYSTEM_PROMPT: &str = "You are Mochi, a personal AI assistant living at the top of the user's screen. \
You have web search access and can help with absolutely anything — research, coding, finding places, recommendations, tasks, questions. \
Respond in the user's language. Be thorough and complete — use as much detail as the task requires. \
No markdown formatting (no **, no ##, no bullet dashes). Use plain text with line breaks.";

#[derive(Default)]
pub struct Chat {
    /// Full multi-turn history. Claude stores content blocks; Ollama stores strings.
    messages: Mutex<Vec<Value>>,
    /// Which backend last wrote the history. A switch clears it.
    provider: Mutex<String>,
}

impl Chat {
    pub fn reset(&self) {
        self.messages.lock().unwrap().clear();
    }

    fn use_provider(&self, provider: &str) {
        let mut current = self.provider.lock().unwrap();
        if *current != provider {
            self.messages.lock().unwrap().clear();
            *current = provider.to_string();
        }
    }

    fn is_empty(&self) -> bool {
        self.messages.lock().unwrap().is_empty()
    }

    fn push(&self, message: Value) {
        self.messages.lock().unwrap().push(message);
    }

    fn pop(&self) {
        self.messages.lock().unwrap().pop();
    }

    fn snapshot(&self) -> Vec<Value> {
        self.messages.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatContext {
    File { name: String, path: String },
    Window { app_name: String, title: String, url: Option<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub text: String,
}

/// One chat turn. Returns the assistant's text, or a message the island shows
/// in the note view.
pub async fn send(
    chat: &Chat,
    provider: &str,
    ollama_url: &str,
    model: &str,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    if provider == "ollama" {
        return send_ollama(chat, ollama_url, model, query, context).await;
    }
    chat.use_provider("claude");
    let key = secrets::get("anthropic-api-key")
        .ok_or_else(|| "API key missing. Open settings.".to_string())?;

    let mut content: Vec<Value> = Vec::new();

    // File / window context rides along with the first message only, exactly
    // like ClaudeService.chat().
    if chat.is_empty() {
        match &context {
            Some(ChatContext::File { name, path }) => {
                if let Some(block) = file_block(path) {
                    content.push(block);
                }
                content.push(json!({ "type": "text", "text": format!("File: {name}") }));
            }
            Some(ChatContext::Window { app_name, title, url }) => {
                let mut text = format!("Context — App: {app_name}, Window: {title}");
                if let Some(url) = url {
                    text.push_str(&format!(", URL: {url}"));
                }
                content.push(json!({ "type": "text", "text": text }));
            }
            None => {}
        }
    }
    content.push(json!({ "type": "text", "text": query }));

    chat.push(json!({ "role": "user", "content": content }));

    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": SYSTEM_PROMPT,
        "tools": [{ "type": "web_search_20260209", "name": "web_search", "max_uses": 5 }],
        "fallbacks": "default",
        "messages": chat.snapshot(),
    });

    let response = match call(&key, &body).await {
        Ok(v) => v,
        Err(err) => {
            chat.pop(); // keep the history consistent with what the model saw
            return Err(err);
        }
    };

    // A policy decline comes back as HTTP 200 with stop_reason "refusal".
    if response.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        chat.pop();
        let why = response
            .get("stop_details")
            .and_then(|d| d.get("explanation"))
            .and_then(Value::as_str)
            .unwrap_or("Claude declined this one.");
        return Err(why.to_string());
    }

    let Some(blocks) = response.get("content").and_then(Value::as_array).cloned() else {
        chat.pop();
        return Err("Unexpected API response.".into());
    };

    // Store the whole content — tool_use / tool_result blocks included — so the
    // next turn has the right context.
    chat.push(json!({ "role": "assistant", "content": blocks.clone() }));

    let text = blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if text.is_empty() {
        return Err("No response text.".into());
    }
    Ok(ChatReply { text })
}

const OLLAMA_TEXT_CAP: usize = 24_000;

pub fn normalise_ollama_url(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if s.is_empty() {
        s = "http://127.0.0.1:11434".into();
    }
    for suffix in ["/api", "/v1"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.trim_end_matches('/').to_string();
        }
    }
    s
}

/// Models installed on the local Ollama server.
/// Native `GET /api/tags` is what `ollama list` uses; `/v1/models` is the fallback.
/// Synchronous on purpose: awaiting a spawned task from a Tauri command never
/// came back, and the settings list stayed on "Loading models…".
pub fn ollama_models(url: &str) -> Result<Vec<String>, String> {
    let base = normalise_ollama_url(url);
    require_http(&base)?;
    let tags = model_ids(&format!("{base}/api/tags"), "models", "name");
    if let Ok(models) = &tags {
        if !models.is_empty() {
            return Ok(models.clone());
        }
    }
    match model_ids(&format!("{base}/v1/models"), "data", "id") {
        Ok(models) if !models.is_empty() => Ok(models),
        Ok(models) => tags.or(Ok(models)),
        Err(err) => tags.or(Err(err)),
    }
}

fn model_ids(url: &str, array_key: &str, id_key: &str) -> Result<Vec<String>, String> {
    let (status, raw) = exchange_blocking("GET", url, None, std::time::Duration::from_secs(30))?;
    if status != 200 {
        crate::log::line(format!("ollama models HTTP {status} {url}"));
        return Err(http_failure(status, &raw));
    }
    let body: Value = serde_json::from_str(&raw).map_err(|err| ollama_fail(&err.to_string()))?;
    let Some(items) = body.get(array_key).and_then(Value::as_array) else {
        return Err(ollama_fail(&clip(&raw, 240)));
    };
    let mut models = Vec::new();
    for item in items {
        let Some(id) = item.get(id_key).and_then(Value::as_str) else { continue };
        if !is_chat_model(id) {
            continue;
        }
        models.push(id.to_string());
    }
    Ok(models)
}

fn is_chat_model(id: &str) -> bool {
    let lower = id.to_lowercase();
    !["embed", "bge-", "all-minilm", "clip", "rerank"]
        .iter()
        .any(|skip| lower.contains(skip))
}

async fn send_ollama(
    chat: &Chat,
    url: &str,
    model: &str,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    if model.trim().is_empty() || model.starts_with("claude-") {
        return Err(crate::i18n::t("chat.pickModel"));
    }
    let base = normalise_ollama_url(url);
    require_http(&base)?;
    chat.use_provider("ollama");

    let user_text = if chat.is_empty() {
        match &context {
            Some(ChatContext::File { name, path }) => match file_text(path) {
                Some(text) => format!("File: {name}\n\n{text}\n\n{query}"),
                None => format!("File: {name}\n\n{query}"),
            },
            Some(ChatContext::Window { app_name, title, url }) => {
                let mut prefix = format!("Context — App: {app_name}, Window: {title}");
                if let Some(url) = url {
                    prefix.push_str(&format!(", URL: {url}"));
                }
                format!("{prefix}\n\n{query}")
            }
            None => query,
        }
    } else {
        query
    };
    chat.push(json!({ "role": "user", "content": user_text }));

    let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    messages.extend(chat.snapshot());
    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        // DeepSeek-R1 spends the whole budget inside <think> otherwise, and the
        // island would show an empty reply. Ollama 0.35 honours this flag.
        "think": false,
        "messages": messages,
    });

    let payload = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    // First turn loads the weights. A 7B file can take a couple of minutes.
    let (status, raw) = match local_exchange(
        "POST",
        &format!("{base}/v1/chat/completions"),
        Some(payload),
        300,
    )
    .await
    {
        Ok(v) => v,
        Err(err) => {
            chat.pop();
            return Err(err);
        }
    };
    if !(200..300).contains(&status) {
        chat.pop();
        let detail = serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message").and_then(Value::as_str).or_else(|| e.as_str()))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| raw.chars().take(200).collect());
        let lower = detail.to_lowercase();
        if status == 404 || lower.contains("not found") {
            return Err(crate::i18n::t("chat.modelMissing").replace("{name}", model));
        }
        return Err(detail);
    }
    let parsed: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let text = ollama_visible_text(&parsed);
    if text.is_empty() {
        chat.pop();
        return Err(crate::i18n::t("chat.emptyReply"));
    }
    chat.push(json!({ "role": "assistant", "content": text.clone() }));
    Ok(ChatReply { text })
}

fn require_http(url: &str) -> Result<(), String> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(crate::i18n::t("chat.ollamaDown"))
    }
}

/// Chat still runs on the async runtime, so the socket lives on its own thread.
/// `spawn_blocking` from a Tauri command never completed.
async fn local_exchange(
    method: &str,
    url: &str,
    body: Option<Vec<u8>>,
    read_timeout_secs: u64,
) -> Result<(u16, String), String> {
    let method = method.to_string();
    let url = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(exchange_blocking(
            &method,
            &url,
            body,
            std::time::Duration::from_secs(read_timeout_secs),
        ));
    });
    loop {
        match rx.try_recv() {
            Ok(result) => return result,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(crate::i18n::t("chat.ollamaDown"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
        }
    }
}

fn exchange_blocking(
    method: &str,
    url: &str,
    body: Option<Vec<u8>>,
    read_timeout: std::time::Duration,
) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    let (host, port, path) = http_target(url)?;
    let mut addrs: Vec<std::net::SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|err| {
            crate::log::line(format!("ollama dns {host}: {err}"));
            ollama_fail(&err.to_string())
        })?
        .collect();
    addrs.sort_by_key(|addr| !addr.is_ipv4());
    let mut stream = None;
    let mut last = String::new();
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, std::time::Duration::from_secs(30)) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(err) => last = err.to_string(),
        }
    }
    let mut stream = match stream {
        Some(s) => s,
        None => {
            crate::log::line(format!("ollama connect {url}: {last}"));
            return Err(ollama_fail(&last));
        }
    };
    stream.set_read_timeout(Some(read_timeout)).ok();
    stream.set_write_timeout(Some(std::time::Duration::from_secs(30))).ok();

    let host_header = if port == 80 { host } else { format!("{host}:{port}") };
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\nAccept: application/json\r\nAuthorization: Bearer ollama\r\n"
    );
    if body.is_some() {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body.as_ref().map(|b| b.len()).unwrap_or(0)));
    }
    req.push_str("\r\n");
    if let Err(err) = stream.write_all(req.as_bytes()) {
        return Err(ollama_fail(&err.to_string()));
    }
    if let Some(bytes) = &body {
        if let Err(err) = stream.write_all(bytes) {
            return Err(ollama_fail(&err.to_string()));
        }
    }

    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.len() > 8_000_000 {
                    break;
                }
                if http_body_complete(&buf) {
                    break;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                crate::log::line(format!("ollama timeout {url}: {err}"));
                return Err(crate::i18n::t("chat.ollamaTimeout"));
            }
            Err(err) => {
                crate::log::line(format!("ollama read {url}: {err}"));
                return Err(ollama_fail(&err.to_string()));
            }
        }
    }
    decode_http(&buf)
}

fn http_target(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| crate::i18n::t("chat.ollamaDown"))?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return Err(crate::i18n::t("chat.ollamaDown"));
    }
    let (host, port) = if let Some(inside) = authority.strip_prefix('[') {
        let (host, port) = inside.split_once("]:").unwrap_or((inside.trim_end_matches(']'), "80"));
        (host.to_string(), port.parse::<u16>().unwrap_or(80))
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port.parse::<u16>().unwrap_or(80);
        (host.to_string(), port)
    } else {
        (authority.to_string(), 80)
    };
    Ok((host, port, path))
}

fn http_body_complete(buf: &[u8]) -> bool {
    let Some(header_end) = find_subsequence(buf, b"\r\n\r\n") else {
        return false;
    };
    let headers = &buf[..header_end];
    let body_len = buf.len() - header_end - 4;
    let Some(expected) = content_length(headers) else {
        return false;
    };
    body_len >= expected
}

fn decode_http(buf: &[u8]) -> Result<(u16, String), String> {
    let header_end = find_subsequence(buf, b"\r\n\r\n").ok_or_else(|| ollama_fail("empty response"))?;
    let header = String::from_utf8_lossy(&buf[..header_end]);
    let status = header
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| ollama_fail(&clip(&header, 200)))?;
    let body = String::from_utf8_lossy(&buf[header_end + 4..]).into_owned();
    Ok((status, body))
}

/// The sentence from the catalog, then the real cause on the next line.
fn ollama_fail(detail: &str) -> String {
    let head = crate::i18n::t("chat.ollamaDown");
    let detail = detail.trim();
    if detail.is_empty() || head.contains(detail) {
        head
    } else {
        format!("{head}\n{detail}")
    }
}

fn http_failure(status: u16, body: &str) -> String {
    let snippet = clip(body, 240);
    if snippet.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}\n{snippet}")
    }
}

fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn content_length(headers: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(headers);
    for line in text.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn file_text(path: &str) -> Option<String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(ext.as_str(), "pdf" | "jpg" | "jpeg" | "png" | "gif" | "webp") {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let mut chars = text.chars();
    let clipped: String = chars.by_ref().take(OLLAMA_TEXT_CAP).collect();
    if chars.next().is_some() {
        Some(format!("{clipped}\n[truncated]"))
    } else {
        Some(clipped)
    }
}

/// Answer text from an OpenAI-compatible chat completion.
/// Thinking models put the reply in `content` after a think block, or only in
/// `reasoning` / `thinking` when `content` stays empty.
fn ollama_visible_text(parsed: &Value) -> String {
    let content = parsed
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("");
    let visible = strip_think(content);
    if !visible.is_empty() {
        return visible;
    }
    for key in ["reasoning", "reasoning_content", "thinking"] {
        let Some(text) = parsed
            .pointer(&format!("/choices/0/message/{key}"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let text = strip_think(text);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

fn strip_think(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        rest = &rest[start + "<think>".len()..];
        if let Some(end) = rest.find("</think>") {
            rest = &rest[end + "</think>".len()..];
        } else {
            rest = "";
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

async fn call(key: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .post(ENDPOINT)
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", FALLBACK_BETA)
        .header("content-type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        // Surface the API's own message, which is what makes a bad key obvious.
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        return Err(format!("Claude API {status}: {detail}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Bad API response: {e}"))
}

/// PDF → document block, image → image block, text/code → inline text.
/// Mirrors readFileAsBlock() in ClaudeService.swift.
fn file_block(path: &str) -> Option<Value> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let media_type = match ext.as_str() {
        "pdf" => Some(("document", "application/pdf")),
        "jpg" | "jpeg" => Some(("image", "image/jpeg")),
        "png" => Some(("image", "image/png")),
        "gif" => Some(("image", "image/gif")),
        "webp" => Some(("image", "image/webp")),
        _ => None,
    };

    if let Some((block_type, media)) = media_type {
        let bytes = std::fs::read(path).ok()?;
        return Some(json!({
            "type": block_type,
            "source": { "type": "base64", "media_type": media, "data": base64(&bytes) },
        }));
    }

    let len = std::fs::metadata(path).ok()?.len();
    if len > MAX_INLINE_TEXT {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some(json!({ "type": "text", "text": format!("File contents:\n{text}") }))
}

/// Small standalone base64 encoder — not worth another dependency.
/// Also used for Stripe's basic auth.
pub(crate) fn base64_for(bytes: &[u8]) -> String {
    base64(bytes)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64, is_chat_model, normalise_ollama_url, ollama_visible_text, strip_think};
    use serde_json::json;

    #[test]
    fn ollama_url_drops_a_trailing_api_path() {
        assert_eq!(normalise_ollama_url("http://127.0.0.1:11434/"), "http://127.0.0.1:11434");
        assert_eq!(normalise_ollama_url("http://127.0.0.1:11434/v1"), "http://127.0.0.1:11434");
        assert_eq!(normalise_ollama_url(""), "http://127.0.0.1:11434");
    }

    #[test]
    fn local_http_target_keeps_host_port_and_path() {
        assert_eq!(
            super::http_target("http://127.0.0.1:11434/api/tags").unwrap(),
            ("127.0.0.1".into(), 11434, "/api/tags".into())
        );
    }

    #[test]
    fn local_http_response_splits_status_and_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        assert!(super::http_body_complete(raw));
        assert_eq!(super::decode_http(raw).unwrap(), (200, "ok".into()));
    }

    #[test]
    fn think_blocks_are_hidden() {
        assert_eq!(strip_think("Hi <think>secret</think> there"), "Hi  there");
        assert_eq!(strip_think("<think>still open"), "");
    }

    #[test]
    fn huggingface_chat_ids_stay_listed() {
        assert!(is_chat_model(
            "hf.co/bartowski/DeepSeek-R1-Distill-Qwen-7B-GGUF:Q4_K_M"
        ));
        assert!(!is_chat_model("nomic-embed-text"));
    }

    #[test]
    fn thinking_models_still_yield_an_answer() {
        let after = json!({"choices":[{"message":{"content":"<think>secret</think> Bonjour"}}]});
        assert_eq!(ollama_visible_text(&after), "Bonjour");
        let reasoning_only = json!({"choices":[{"message":{"content":"","reasoning":"La réponse"}}]});
        assert_eq!(ollama_visible_text(&reasoning_only), "La réponse");
        let still_open = json!({"choices":[{"message":{"content":"<think>still open"}}]});
        assert_eq!(ollama_visible_text(&still_open), "");
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
