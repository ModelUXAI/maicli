//! Transport HTTP untuk `tui-mcp`: Streamable HTTP (protokol 2025-06-18).
//!
//! Semantik mengikuti omp (`docs/mcp-protocol-transports.md` §Streamable HTTP):
//! satu request JSON-RPC = satu POST; respons JSON langsung, atau SSE stream
//! yang berisi balasan ber-id sama; sesi server dilacak lewat header
//! `Mcp-Session-Id` bila server mengirimkannya.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

/// Endpoint + klien HTTP satu server MCP Streamable HTTP.
#[derive(Clone)]
pub struct HttpTransport {
    url: String,
    http: reqwest::Client,
    session_id: Arc<Mutex<Option<String>>>,
    /// Header kustom per request (mis. `Authorization: Bearer …`); nilai sudah
    /// di-resolve dari `env:` saat konstruksi dan tidak pernah dicatat log.
    headers: Arc<std::collections::HashMap<String, String>>,
    #[allow(dead_code)]
    timeout: Duration,
}

pub(crate) fn galat(kode: tui_wire::ErrorCode, pesan: impl Into<String>) -> tui_wire::MxError {
    tui_wire::MxError::new(kode, pesan.into())
}

/// Nilai header `env:NAMA` → isi env var; nilai lain diteruskan apa adanya.
pub fn resolve_header(nilai: &str) -> String {
    if let Some(nama) = nilai.strip_prefix("env:") {
        std::env::var(nama).unwrap_or_default()
    } else {
        nilai.to_string()
    }
}

impl HttpTransport {
    /// Transport baru; `timeout` dipakai per request HTTP.
    pub fn new(
        url: impl Into<String>,
        timeout: Duration,
        headers: Arc<std::collections::HashMap<String, String>>,
    ) -> Result<Self, tui_wire::MxError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| {
                galat(
                    tui_wire::ErrorCode::GatewayUnreachable,
                    format!("klien HTTP MCP gagal dibuat: {err}"),
                )
            })?;
        Ok(Self {
            url: url.into(),
            http,
            session_id: Arc::new(Mutex::new(None)),
            headers,
            timeout,
        })
    }

    /// URL endpoint.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Kirim satu pesan JSON-RPC dan tunggu balasannya (satu POST).
    ///
    /// Respons bisa berupa objek JSON langsung, atau SSE stream
    /// (`text/event-stream`) yang berisi balasan ber-id sama.
    /// Status 502/503/504 diulang **satu kali** (reconnect ringan).
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        id: u64,
    ) -> Result<Value, tui_wire::MxError> {
        let mut pesan = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if !params.is_null() {
            pesan["params"] = params;
        }
        let mut response = self.post(&pesan).await?;
        // Satu kali retry untuk kegagalan server sementara (502/503/504).
        if matches!(response.status().as_u16(), 502..=504) {
            tokio::time::sleep(Duration::from_millis(200)).await;
            response = self.post(&pesan).await?;
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        if content_type.contains("text/event-stream") {
            return Self::kumpulkan_sse(response, id).await;
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(galat(
                tui_wire::ErrorCode::GatewayServer,
                format!("server MCP {method} balas {status}: {}", potong(&body)),
            ));
        }
        let parsed: Value = serde_json::from_str(&body).map_err(|err| {
            galat(
                tui_wire::ErrorCode::Protocol,
                format!("jawaban MCP bukan JSON: {err}"),
            )
        })?;
        ekstrak_result(&parsed, method)
    }

    /// Kirim notifikasi (tanpa id) — satu POST; respons diabaikan kecuali galat.
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), tui_wire::MxError> {
        let mut pesan = json!({"jsonrpc": "2.0", "method": method});
        if !params.is_null() {
            pesan["params"] = params;
        }
        let response = self.post(&pesan).await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(galat(
                tui_wire::ErrorCode::GatewayServer,
                format!("notifikasi {method} ditolak {status}: {}", potong(&body)),
            ));
        }
        Ok(())
    }

    /// Tutup sesi (DELETE + Mcp-Session-Id bila ada).
    pub async fn close(&self) {
        if let Some(sid) = self.session_id.lock().await.clone() {
            let _ = self
                .http
                .delete(&self.url)
                .header("Mcp-Session-Id", sid)
                .send()
                .await;
        }
    }

    /// Alirkan notifikasi server lewat **GET stream** (streamable HTTP), dengan
    /// reconnect: stream berakhir/terputus → GET lagi (jeda 500 ms) sampai semua
    /// penerima di-drop. Bila server menolak GET (mis. 405), aliran berhenti
    /// diam — notifikasi tetap bisa lewat respons POST sesuai spesifikasi.
    pub async fn stream_notifikasi(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::McpNotification>,
    ) {
        loop {
            let mut request = self
                .http
                .get(&self.url)
                .header("Accept", "text/event-stream");
            if let Some(sid) = self.session_id.lock().await.clone() {
                request = request.header("Mcp-Session-Id", sid);
            }
            for (kunci, nilai) in self.headers.iter() {
                request = request.header(kunci.as_str(), nilai.as_str());
            }
            let Ok(response) = request.send().await else {
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            };
            if !response.status().is_success() {
                // 405 dsb. = server tidak menyediakan stream; berhenti, bukan
                // menghantam server tiap 500 ms.
                return;
            }
            let mut response = response;
            let mut buffer: Vec<u8> = Vec::new();
            loop {
                let chunk = match response.chunk().await {
                    Ok(Some(bytes)) => bytes,
                    _ => break,
                };
                buffer.extend_from_slice(&chunk);
                while let Some(posisi) = buffer.iter().position(|b| *b == b'\n') {
                    let baris: Vec<u8> = buffer.drain(..=posisi).collect();
                    let baris = String::from_utf8_lossy(&baris);
                    let Some(data) = baris.trim().strip_prefix("data:") else {
                        continue;
                    };
                    let Ok(pesan) = serde_json::from_str::<Value>(data.trim()) else {
                        continue;
                    };
                    // Notifikasi = tanpa id; balasan request diabaikan di sini.
                    if pesan.get("id").is_none()
                        && let Some(method) = pesan.get("method").and_then(Value::as_str)
                    {
                        let _ = tx.send(crate::McpNotification {
                            method: method.to_string(),
                            params: pesan.get("params").cloned().unwrap_or(Value::Null),
                        });
                    }
                }
            }
            // EOF: tunggu lalu sambung ulang.
            tokio::time::sleep(Duration::from_millis(500)).await;
            if tx.is_closed() {
                return;
            }
        }
    }

    async fn post(&self, pesan: &Value) -> Result<reqwest::Response, tui_wire::MxError> {
        let mut request = self
            .http
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .json(pesan);
        if let Some(sid) = self.session_id.lock().await.clone() {
            request = request.header("Mcp-Session-Id", sid);
        }
        for (kunci, nilai) in self.headers.iter() {
            request = request.header(kunci.as_str(), nilai.as_str());
        }
        let response = request.send().await.map_err(|err| {
            galat(
                tui_wire::ErrorCode::GatewayUnreachable,
                format!("POST ke {} gagal: {err}", self.url),
            )
        })?;
        if let Some(sid) = response.headers().get("mcp-session-id")
            && let Ok(sid) = sid.to_str()
        {
            *self.session_id.lock().await = Some(sid.to_string());
        }
        Ok(response)
    }

    async fn kumpulkan_sse(
        response: reqwest::Response,
        id: u64,
    ) -> Result<Value, tui_wire::MxError> {
        let mut response = response;
        let mut buffer: Vec<u8> = Vec::new();
        loop {
            // Ambil chunk berikutnya (Response::chunk = Result<Option<Bytes>>).
            let chunk = match response.chunk().await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    return Err(galat(
                        tui_wire::ErrorCode::GatewayUnreachable,
                        "stream SSE MCP ditutup tanpa balasan",
                    ));
                }
                Err(err) => {
                    return Err(galat(
                        tui_wire::ErrorCode::GatewayUnreachable,
                        format!("stream SSE MCP terputus: {err}"),
                    ));
                }
            };
            buffer.extend_from_slice(&chunk);
            // Proses semua baris lengkap di buffer.
            while let Some(posisi) = buffer.iter().position(|b| *b == b'\n') {
                let baris: Vec<u8> = buffer.drain(..=posisi).collect();
                let baris = String::from_utf8_lossy(&baris);
                let Some(data) = baris.trim().strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let pesan: Value = serde_json::from_str(data).map_err(|err| {
                    galat(
                        tui_wire::ErrorCode::Protocol,
                        format!("data SSE MCP bukan JSON: {err}"),
                    )
                })?;
                if pesan.get("id").and_then(Value::as_u64) == Some(id) {
                    return ekstrak_result(&pesan, "request");
                }
            }
        }
    }
}

pub(crate) fn potong(text: &str) -> String {
    text.chars().take(200).collect()
}

pub(crate) fn ekstrak_result(pesan: &Value, method: &str) -> Result<Value, tui_wire::MxError> {
    if let Some(err) = pesan.get("error") {
        return Err(galat(
            tui_wire::ErrorCode::ToolFailed,
            format!("server MCP menolak {method}: {err}"),
        ));
    }
    Ok(pesan.get("result").cloned().unwrap_or(Value::Null))
}
