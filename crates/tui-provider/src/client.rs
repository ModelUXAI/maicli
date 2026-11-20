//! Klien HTTP untuk gateway OpenAI-compatible.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tui_wire::{ErrorCode, Message, ProviderEvent, ToolDefinition};

use crate::error::GatewayError;
use crate::sse::{SseDecoder, SsePayload};
use crate::wire::{StreamAssembler, to_wire_messages, to_wire_tools};

/// Kebijakan percobaan ulang untuk satu permintaan (sebelum body mengalir).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Jumlah percobaan maksimum (termasuk percobaan pertama).
    pub max_attempts: u32,
    /// Jeda dasar; digandakan tiap percobaan.
    pub base: Duration,
    /// Batas jeda.
    pub max: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base: Duration::from_millis(500),
            max: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// Jeda untuk percobaan ke-`attempt` (mulai 1), dengan jitter 75–100 %.
    pub fn delay(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(16);
        let factor = 1u32.checked_shl(shift).unwrap_or(u32::MAX);
        let capped = self.base.saturating_mul(factor).min(self.max);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let percent = 75 + (nanos % 26);
        capped.mul_f64(f64::from(percent) / 100.0)
    }
}

/// Satu permintaan chat completion.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Model yang dipakai (id persis dari gateway).
    pub model: String,
    /// Konteks percakapan.
    pub messages: Vec<Message>,
    /// Tool yang ditawarkan ke model.
    pub tools: Vec<ToolDefinition>,
    /// Batas token output.
    pub max_tokens: Option<u32>,
    /// Tingkat penalaran (`low`/`medium`/`high`); dikirim bila Some.
    pub reasoning_effort: Option<String>,
}

impl ChatRequest {
    /// Permintaan tanpa tool dan tanpa batas token.
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            tools: Vec::new(),
            max_tokens: None,
            reasoning_effort: None,
        }
    }
}

/// Satu model dari hasil discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Id model seperti dilaporkan gateway.
    pub id: String,
}

/// Sumber model untuk agent.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Mulai satu panggilan model; event mengalir lewat kanal yang dikembalikan.
    async fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError>;
}

/// Klien gateway OpenAI-compatible.
#[derive(Clone)]
pub struct OpenAiCompatClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
    retry: RetryPolicy,
}

impl OpenAiCompatClient {
    /// Bangun klien; `base_url` diakhiri `/v1` (tanpa garis miring penutup).
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Result<Self, GatewayError> {
        let base = base_url.into();
        let base = base.trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(GatewayError::malformed("base_url kosong"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| GatewayError::unreachable(e.to_string()))?;
        Ok(Self {
            base_url: base,
            api_key,
            http,
            retry: RetryPolicy::default(),
        })
    }

    /// Ganti kebijakan retry (dipakai test dan config).
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Alamat dasar efektif.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.api_key.as_deref() {
            Some(key) if !key.is_empty() => builder.bearer_auth(key),
            _ => builder,
        }
    }

    /// Ambil daftar model dari `GET /models`.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, GatewayError> {
        let mut attempt = 1u32;
        loop {
            let response = self
                .authorized(self.http.get(self.endpoint("models")))
                .send()
                .await;
            match response {
                Err(err) if is_transient(&err) && attempt < self.retry.max_attempts => {
                    tokio::time::sleep(self.retry.delay(attempt)).await;
                    attempt += 1;
                }
                Err(err) => return Err(GatewayError::unreachable(err.to_string())),
                Ok(response) => {
                    let status = response.status().as_u16();
                    if !(200..300).contains(&status) {
                        let retry_after = retry_after_of(response.headers());
                        let body = response.text().await.unwrap_or_default();
                        let error =
                            GatewayError::from_status(status, &body).with_retry_after(retry_after);
                        if error.retryable && attempt < self.retry.max_attempts {
                            let wait = error
                                .retry_after
                                .unwrap_or_else(|| self.retry.delay(attempt));
                            tokio::time::sleep(wait).await;
                            attempt += 1;
                            continue;
                        }
                        return Err(error);
                    }
                    let body = response
                        .text()
                        .await
                        .map_err(|e| GatewayError::malformed(e.to_string()))?;
                    return parse_models(&body);
                }
            }
        }
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        let mut body = json!({
            "model": request.model,
            "messages": to_wire_messages(&request.messages),
            "stream": true,
            "stream_options": { "include_usage": true }
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(to_wire_tools(&request.tools));
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(effort) = &request.reasoning_effort {
            body["reasoning_effort"] = json!(effort);
        }
        body
    }
}

fn is_transient(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn retry_after_of(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    let seconds: u64 = value.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

fn parse_models(body: &str) -> Result<Vec<ModelInfo>, GatewayError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| GatewayError::malformed(format!("JSON tidak valid: {e}")))?;
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| GatewayError::malformed("respons /models tanpa field data"))?;
    let models: Vec<ModelInfo> = items
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(|id| ModelInfo { id: id.to_string() })
        .collect();
    if models.is_empty() {
        return Err(GatewayError::malformed("daftar model kosong"));
    }
    Ok(models)
}

#[async_trait]
impl Provider for OpenAiCompatClient {
    async fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError> {
        let body = self.build_body(&request);
        let mut attempt = 1u32;
        let response = loop {
            let sent = self
                .authorized(self.http.post(self.endpoint("chat/completions")))
                .json(&body)
                .send()
                .await;
            match sent {
                Err(err) if is_transient(&err) && attempt < self.retry.max_attempts => {
                    tokio::time::sleep(self.retry.delay(attempt)).await;
                    attempt += 1;
                }
                Err(err) => return Err(GatewayError::unreachable(err.to_string())),
                Ok(response) => {
                    let status = response.status().as_u16();
                    if (200..300).contains(&status) {
                        break response;
                    }
                    let retry_after = retry_after_of(response.headers());
                    let body = response.text().await.unwrap_or_default();
                    let error =
                        GatewayError::from_status(status, &body).with_retry_after(retry_after);
                    if error.retryable && attempt < self.retry.max_attempts {
                        let wait = error
                            .retry_after
                            .unwrap_or_else(|| self.retry.delay(attempt));
                        tokio::time::sleep(wait).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(error);
                }
            }
        };

        let (tx, rx) = unbounded_channel();
        tokio::spawn(pump_stream(response, tx));
        Ok(rx)
    }
}

async fn pump_stream(response: reqwest::Response, tx: UnboundedSender<ProviderEvent>) {
    if tx.send(ProviderEvent::Start).is_err() {
        return;
    }
    let mut response = response;
    let mut decoder = SseDecoder::new();
    let mut assembler = StreamAssembler::default();
    loop {
        match response.chunk().await {
            Ok(Some(bytes)) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                for payload in decoder.push(&text) {
                    match payload {
                        SsePayload::Done => {
                            for event in assembler.finish() {
                                let _ = tx.send(event);
                            }
                            return;
                        }
                        SsePayload::Data(json) => {
                            if json.trim().is_empty() {
                                continue;
                            }
                            match assembler.push(&json) {
                                Ok(events) => {
                                    for event in events {
                                        if tx.send(event).is_err() {
                                            return;
                                        }
                                    }
                                }
                                Err(err) => {
                                    let _ = tx.send(ProviderEvent::Error {
                                        code: ErrorCode::GatewayRejected,
                                        message: format!("chunk streaming tidak valid: {err}"),
                                    });
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                for event in assembler.finish() {
                    let _ = tx.send(event);
                }
                return;
            }
            Err(err) => {
                let _ = tx.send(ProviderEvent::Error {
                    code: ErrorCode::GatewayUnreachable,
                    message: err.to_string(),
                });
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use tui_wire::{AssistantMessage, StopReason};

    /// Server HTTP sekali pakai: kirim respons berurutan untuk tiap koneksi.
    fn spawn_mock(responses: Vec<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
                let mut buffer = [0u8; 8192];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}/v1")
    }

    fn http_response(status: &str, content_type: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn client(base: &str) -> OpenAiCompatClient {
        OpenAiCompatClient::new(base, Some("kunci-uji".into()))
            .unwrap()
            .with_retry(RetryPolicy {
                max_attempts: 3,
                base: Duration::from_millis(1),
                max: Duration::from_millis(5),
            })
    }

    #[tokio::test]
    async fn discovery_membaca_daftar_model() {
        let base = spawn_mock(vec![http_response(
            "200 OK",
            "application/json",
            r#"{"data":[{"id":"deepseek-v4.1-flash"},{"id":"lain"}]}"#,
        )]);
        let models = client(&base).list_models().await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "deepseek-v4.1-flash");
    }

    #[tokio::test]
    async fn content_filter_dipetakan_ke_error_tanpa_retry() {
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"halo\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let base = spawn_mock(vec![http_response("200 OK", "text/event-stream", body)]);
        let mut rx = client(&base)
            .stream_chat(ChatRequest::new("m", vec![Message::user("halo")]))
            .await
            .unwrap();
        let mut final_message: Option<AssistantMessage> = None;
        while let Some(event) = rx.recv().await {
            if let ProviderEvent::Done { message } = event {
                final_message = Some(message);
            }
        }
        let message = final_message.expect("ada pesan akhir");
        assert_eq!(message.stop_reason, StopReason::Error);
        assert!(
            message
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("content_filter"),
            "{:?}",
            message.error
        );
    }

    #[tokio::test]
    async fn discovery_retry_setelah_429_lalu_sukses() {
        let base = spawn_mock(vec![
            "HTTP/1.1 429 Too Many Requests\r\nretry-after: 0\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                .to_string(),
            http_response("200 OK", "application/json", r#"{"data":[{"id":"m"}]}"#),
        ]);
        let models = client(&base).list_models().await.unwrap();
        assert_eq!(models.len(), 1);
    }

    #[tokio::test]
    async fn discovery_gagal_auth_tidak_diulang() {
        let base = spawn_mock(vec![http_response(
            "401 Unauthorized",
            "text/plain",
            "nope",
        )]);
        let err = client(&base).list_models().await.unwrap_err();
        assert_eq!(err.code, ErrorCode::GatewayAuth);
        assert!(!err.retryable);
    }

    #[tokio::test]
    async fn streaming_merakit_tool_call_dari_sse() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"pa\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"x\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let base = spawn_mock(vec![http_response("200 OK", "text/event-stream", body)]);
        let mut rx = client(&base)
            .stream_chat(ChatRequest::new("m", vec![Message::user("halo")]))
            .await
            .unwrap();
        let mut saw_start = false;
        let mut final_message: Option<AssistantMessage> = None;
        while let Some(event) = rx.recv().await {
            match event {
                ProviderEvent::Start => saw_start = true,
                ProviderEvent::Done { message } => final_message = Some(message),
                ProviderEvent::Error { message, .. } => panic!("error tak terduga: {message}"),
                _ => {}
            }
        }
        assert!(saw_start);
        let message = final_message.unwrap();
        assert_eq!(message.stop_reason, StopReason::ToolUse);
        assert_eq!(message.tool_calls()[0].2, "{\"path\":\"x\"}");
    }

    #[test]
    fn jeda_retry_dalam_batas() {
        let policy = RetryPolicy::default();
        for attempt in 1..=5 {
            let delay = policy.delay(attempt);
            assert!(delay >= Duration::from_millis(375));
            assert!(delay <= policy.max);
        }
    }
}
