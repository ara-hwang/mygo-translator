//! Client for llama.cpp's OpenAI-compatible server.
//!
//! The application never embeds llama.cpp; it drives a `llama-server` process and talks to it
//! over loopback HTTP. This module contains the two calls that matter: a readiness probe for
//! `/health` and a streaming `POST /v1/chat/completions` that feeds deltas to a callback as
//! they arrive.

use std::fmt;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::http::{self, CancelToken, HttpError, Request};
use crate::settings::GenerationSettings;
use crate::sse::{SseDecoder, SseEvent};

/// Default overall timeout for one translation request.
pub const TRANSLATE_TIMEOUT: Duration = Duration::from_secs(300);
/// Default timeout for a health probe.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// The loopback location of a llama-server instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Hostname or IP, e.g. `127.0.0.1`.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl Endpoint {
    /// Builds an endpoint from a host and port.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    /// `127.0.0.1:<port>`.
    pub fn localhost(port: u16) -> Self {
        Self::new("127.0.0.1", port)
    }

    /// Parses `host:port`, `http://host:port` or `host` (defaults to port 8788).
    pub fn parse(text: &str) -> Result<Self, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("서버 주소가 비어 있습니다.".into());
        }
        let without_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(trimmed);
        let host_port = without_scheme.split('/').next().unwrap_or_default();
        match host_port.rsplit_once(':') {
            Some((host, port)) => {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| format!("포트 번호가 올바르지 않습니다: {port:?}"))?;
                if host.is_empty() {
                    return Err("호스트가 비어 있습니다.".into());
                }
                Ok(Self::new(host, port))
            }
            None => Ok(Self::new(host_port, 8788)),
        }
    }

    /// Resolves the endpoint to a socket address.
    pub fn socket_addr(&self) -> Result<SocketAddr, String> {
        let host = match self.host.as_str() {
            "localhost" => "127.0.0.1",
            other => other,
        };
        format!("{host}:{}", self.port)
            .parse()
            .map_err(|_| format!("주소를 해석할 수 없습니다: {}:{}", self.host, self.port))
    }

    /// Human-readable base URL.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// Sampling options sent with each request.
#[derive(Clone, Debug, PartialEq)]
pub struct GenerationParams {
    /// Sampling temperature.
    pub temperature: f64,
    /// Nucleus sampling probability.
    pub top_p: f64,
    /// Top-k cutoff (`-1` disables).
    pub top_k: i32,
    /// Repetition penalty.
    pub repeat_penalty: f64,
    /// Maximum number of generated tokens.
    pub max_tokens: u32,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self::from(&GenerationSettings::default())
    }
}

impl From<&GenerationSettings> for GenerationParams {
    fn from(settings: &GenerationSettings) -> Self {
        Self {
            temperature: settings.temperature,
            top_p: settings.top_p,
            top_k: settings.top_k,
            repeat_penalty: settings.repeat_penalty,
            max_tokens: settings.max_tokens,
        }
    }
}

/// Result of a health probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    /// True when the server reports it can accept requests.
    pub ready: bool,
    /// Raw detail for the UI while the model is still loading.
    pub detail: String,
}

/// Everything learned from one translation request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslationResult {
    /// Cleaned translation text (possibly partial when cancelled).
    pub text: String,
    /// True when the caller cancelled the request.
    pub cancelled: bool,
    /// Why generation stopped, as reported by the server.
    pub finish_reason: Option<String>,
    /// Prompt tokens, when the server reports usage.
    pub prompt_tokens: Option<u64>,
    /// Completion tokens, when the server reports usage.
    pub completion_tokens: Option<u64>,
    /// Milliseconds until the first token arrived.
    pub first_token_ms: Option<u128>,
    /// Total wall-clock milliseconds.
    pub total_ms: u128,
    /// Server-reported generation speed, if present.
    pub tokens_per_second: Option<f64>,
}

/// Errors surfaced by a translation request.
#[derive(Debug)]
pub enum ChatError {
    /// Transport-level failure.
    Http(HttpError),
    /// The server rejected or failed the request.
    Server(String),
    /// The stream contained something that is not a chat completion chunk.
    Protocol(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatError::Http(err) => write!(f, "{err}"),
            ChatError::Server(message) => write!(f, "모델 서버 오류: {message}"),
            ChatError::Protocol(message) => write!(f, "스트림 해석 오류: {message}"),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<HttpError> for ChatError {
    fn from(err: HttpError) -> Self {
        ChatError::Http(err)
    }
}

/// Asks the server whether it is ready to translate.
///
/// A 503 while the model loads is a normal answer, not an error, so this only fails when the
/// connection itself fails.
pub fn probe_health(
    endpoint: &Endpoint,
    cancel: CancelToken,
    timeout: Duration,
) -> Result<Health, HttpError> {
    let addr = endpoint.socket_addr().map_err(HttpError::Connect)?;
    let mut response = http::open(addr, &Request::get("/health"), cancel, timeout)?;
    let body = response.read_text(8192).unwrap_or_default();
    let ready = response.status == 200 && body.contains("\"ok\"");
    let detail = if ready {
        "ok".to_string()
    } else if response.status == 503 {
        body.trim().to_string()
    } else if body.trim().is_empty() {
        format!("HTTP {}", response.status)
    } else {
        body.trim().to_string()
    };
    Ok(Health { ready, detail })
}

/// Streams a translation from the server.
///
/// `on_delta` is called for every content delta as it arrives, which lets the UI paint text
/// while the GPU is still generating. The returned result contains the fully cleaned text.
pub fn translate_stream(
    endpoint: &Endpoint,
    model: &str,
    params: &GenerationParams,
    prompt: &str,
    cancel: CancelToken,
    on_delta: &mut dyn FnMut(&str),
) -> Result<TranslationResult, ChatError> {
    let started = Instant::now();
    let addr = endpoint
        .socket_addr()
        .map_err(|err| ChatError::Http(HttpError::Connect(err)))?;

    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": params.temperature,
        "top_p": params.top_p,
        "top_k": params.top_k,
        "repeat_penalty": params.repeat_penalty,
        "max_tokens": params.max_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
        "cache_prompt": true,
    })
    .to_string();

    let mut response = http::open(
        addr,
        &Request::post_json("/v1/chat/completions", body),
        cancel.clone(),
        TRANSLATE_TIMEOUT,
    )?;

    if !response.is_success() {
        let detail = response.read_text(8192).unwrap_or_default();
        return Err(ChatError::Http(HttpError::Status {
            code: response.status,
            body: compact_error_body(&detail),
        }));
    }

    let mut decoder = SseDecoder::default();
    let mut raw = String::new();
    let mut result = TranslationResult::default();
    let mut first_delta_at: Option<Instant> = None;
    let mut done = false;

    loop {
        let chunk = match response.read_chunk() {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(HttpError::Cancelled) => {
                result.cancelled = true;
                break;
            }
            Err(err) => return Err(ChatError::Http(err)),
        };
        for event in decoder.push_bytes(&chunk) {
            match event {
                SseEvent::Done => {
                    done = true;
                }
                SseEvent::Data(payload) => {
                    let value: Value = serde_json::from_str(&payload).map_err(|err| {
                        ChatError::Protocol(format!("{err}: {}", truncate(&payload, 200)))
                    })?;
                    if let Some(message) = server_error(&value) {
                        return Err(ChatError::Server(message));
                    }
                    if let Some(delta) = value
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(Value::as_str)
                    {
                        if !delta.is_empty() {
                            if first_delta_at.is_none() {
                                first_delta_at = Some(Instant::now());
                                result.first_token_ms = Some(started.elapsed().as_millis().max(1));
                            }
                            raw.push_str(delta);
                            on_delta(delta);
                        }
                    }
                    if let Some(choice) = value.get("choices").and_then(|choices| choices.get(0)) {
                        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                            result.finish_reason = Some(reason.to_string());
                        }
                    }
                    if let Some(usage) = value.get("usage") {
                        if let Some(tokens) = usage.get("prompt_tokens").and_then(Value::as_u64) {
                            result.prompt_tokens = Some(tokens);
                        }
                        if let Some(tokens) = usage.get("completion_tokens").and_then(Value::as_u64)
                        {
                            result.completion_tokens = Some(tokens);
                        }
                    }
                    if let Some(timings) = value.get("timings") {
                        if let Some(speed) =
                            timings.get("predicted_per_second").and_then(Value::as_f64)
                        {
                            result.tokens_per_second = Some(speed);
                        }
                    }
                }
            }
        }
        if done {
            break;
        }
    }

    for event in decoder.finish() {
        if let SseEvent::Data(payload) = event {
            if let Ok(value) = serde_json::from_str::<Value>(&payload) {
                if let Some(delta) = value
                    .get("choices")
                    .and_then(|choices| choices.get(0))
                    .and_then(|choice| choice.get("delta"))
                    .and_then(|delta| delta.get("content"))
                    .and_then(Value::as_str)
                {
                    if !delta.is_empty() {
                        raw.push_str(delta);
                        on_delta(delta);
                    }
                }
            }
        }
    }

    result.text = crate::normalize_output(&raw);
    result.total_ms = started.elapsed().as_millis();
    if result.cancelled && result.text.is_empty() {
        return Ok(result);
    }
    Ok(result)
}

fn server_error(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    if let Some(message) = error.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    Some(error.to_string())
}

fn compact_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = server_error(&value) {
            return message;
        }
    }
    truncate(trimmed, 300)
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::cancel_token;
    use crate::testutil::mock::{delta_chunk, MockServer, MockSpec};
    use std::sync::atomic::Ordering;

    #[test]
    fn parses_endpoint_forms() {
        assert_eq!(
            Endpoint::parse("http://127.0.0.1:8788/health").unwrap(),
            Endpoint::new("127.0.0.1", 8788)
        );
        assert_eq!(
            Endpoint::parse("localhost:9000").unwrap(),
            Endpoint::new("localhost", 9000)
        );
        assert_eq!(
            Endpoint::parse("localhost").unwrap(),
            Endpoint::new("localhost", 8788)
        );
        assert!(Endpoint::parse("").is_err());
        assert!(Endpoint::parse("host:notaport").is_err());
    }

    #[test]
    fn resolves_localhost_to_loopback() {
        let addr = Endpoint::new("localhost", 8080).socket_addr().unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn health_reports_busy_while_the_model_loads() {
        let server = MockServer::start(MockSpec::Sequence(vec![
            MockSpec::status(503, "Loading model"),
            MockSpec::json(r#"{"status":"ok"}"#),
        ]));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        let first = probe_health(&endpoint, cancel_token(), HEALTH_TIMEOUT).unwrap();
        assert!(!first.ready);
        let second = probe_health(&endpoint, cancel_token(), HEALTH_TIMEOUT).unwrap();
        assert!(second.ready);
    }

    #[test]
    fn streams_deltas_and_reports_usage() {
        let events = vec![
            delta_chunk("안녕"),
            delta_chunk("하세요"),
            // A final chunk carrying the finish reason, usage and timings.
            "{\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":2},\"timings\":{\"predicted_per_second\":42.5}}".to_string(),
        ];
        let refs: Vec<&str> = events.iter().map(String::as_str).collect();
        let server = MockServer::start(MockSpec::sse(refs));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        let mut deltas = Vec::new();
        let result = translate_stream(
            &endpoint,
            "hymt",
            &GenerationParams::default(),
            "Translate",
            cancel_token(),
            &mut |delta| deltas.push(delta.to_string()),
        )
        .unwrap();

        assert_eq!(deltas, vec!["안녕".to_string(), "하세요".to_string()]);
        assert_eq!(result.text, "안녕하세요");
        assert_eq!(result.prompt_tokens, Some(11));
        assert_eq!(result.completion_tokens, Some(2));
        assert_eq!(result.tokens_per_second, Some(42.5));
        assert_eq!(result.finish_reason.as_deref(), Some("stop"));
        assert!(result.first_token_ms.is_some());
        assert!(!result.cancelled);
    }

    #[test]
    fn surfaces_server_side_errors_from_a_stream() {
        let server = MockServer::start(MockSpec::sse(vec![
            r#"{"error":{"message":"failed to load model"}}"#,
        ]));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        let error = translate_stream(
            &endpoint,
            "hymt",
            &GenerationParams::default(),
            "Translate",
            cancel_token(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, ChatError::Server(_)));
        assert!(error.to_string().contains("failed to load model"));
    }

    #[test]
    fn surfaces_http_errors_with_a_compact_body() {
        let server = MockServer::start(MockSpec::status(
            500,
            r#"{"error":{"message":"context size exceeded"}}"#,
        ));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        let error = translate_stream(
            &endpoint,
            "hymt",
            &GenerationParams::default(),
            "Translate",
            cancel_token(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("context size exceeded"));
    }

    #[test]
    fn cancelling_returns_the_partial_translation() {
        let chunks: Vec<String> = (0..40).map(|_| delta_chunk("가")).collect();
        let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let server = MockServer::start(MockSpec::sse(refs));
        let endpoint = Endpoint::new("127.0.0.1", server.addr().port());
        let token = cancel_token();
        let token_for_cb = token.clone();
        let result = translate_stream(
            &endpoint,
            "hymt",
            &GenerationParams::default(),
            "Translate",
            token.clone(),
            &mut |_| {
                token_for_cb.store(true, Ordering::Relaxed);
            },
        )
        .unwrap();
        assert!(result.cancelled);
        assert!(!result.text.is_empty());
    }
}
