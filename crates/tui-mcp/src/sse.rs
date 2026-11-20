//! Transport legacy SSE untuk `tui-mcp` (protokol 2024-11-05).
//!
//! Semantik mengikuti omp (`docs/mcp-protocol-transports.md` §HTTP+SSE):
//! satu koneksi **GET** `text/event-stream` untuk semua balasan server; event
//! `endpoint` pertama menyebut URL POST tempat request dikirim; setiap POST
//! dijawab **asinkron** lewat stream SSE tersebut (korelasi per id).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{Mutex, oneshot};

use crate::http::{ekstrak_result, galat, potong};

/// Transport SSE satu server MCP legacy.
#[derive(Clone)]
pub struct SseTransport {
    /// URL POST tempat request dikirim (dari event `endpoint`).
    endpoint: Arc<std::sync::OnceLock<String>>,
    url_get: String,
    http: reqwest::Client,
    headers: Arc<std::collections::HashMap<String, String>>,
    /// Penerima balasan per id (diisi task pembaca stream).
    tunggu: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    /// Slot penerima notifikasi server (bila ada yang berlangganan).
    notifications: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<crate::McpNotification>>>>,
    /// Notifikasi yang datang sebelum ada langganan (retensi maks 100).
    tertunda: Arc<Mutex<Vec<crate::McpNotification>>>,
}

impl SseTransport {
    /// Buka koneksi GET SSE dan tunggu event `endpoint`.
    pub async fn connect(
        url: impl Into<String>,
        timeout: Duration,
        headers: Arc<std::collections::HashMap<String, String>>,
    ) -> Result<Self, tui_wire::MxError> {
        let url_get = url.into();
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| {
                galat(
                    tui_wire::ErrorCode::GatewayUnreachable,
                    format!("klien HTTP MCP gagal dibuat: {err}"),
                )
            })?;
        let response = http
            .get(&url_get)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|err| {
                galat(
                    tui_wire::ErrorCode::GatewayUnreachable,
                    format!("GET {url_get} gagal: {err}"),
                )
            })?;
        if !response.status().is_success() {
            return Err(galat(
                tui_wire::ErrorCode::GatewayServer,
                format!("GET {url_get} balas {}", response.status()),
            ));
        }
        let transport = Self {
            endpoint: Arc::new(std::sync::OnceLock::new()),
            url_get,
            http,
            headers,
            tunggu: Arc::new(Mutex::new(HashMap::new())),
            notifications: Arc::new(Mutex::new(None)),
            tertunda: Arc::new(Mutex::new(Vec::new())),
        };
        transport.jalankan_pembaca(response);
        // Tunggu event endpoint sampai batas waktu.
        let batas = tokio::time::Instant::now() + timeout;
        loop {
            if transport.endpoint.get().is_some() {
                return Ok(transport);
            }
            if tokio::time::Instant::now() >= batas {
                return Err(galat(
                    tui_wire::ErrorCode::ToolTimeout,
                    format!(
                        "server MCP SSE {} tidak mengirim endpoint",
                        transport.url_get
                    ),
                ));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// URL POST endpoint (hasil event `endpoint`).
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.get().map(String::as_str)
    }

    /// Kirim satu request JSON-RPC dan tunggu balasannya dari stream SSE.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        id: u64,
        timeout: Duration,
    ) -> Result<Value, tui_wire::MxError> {
        let endpoint = self.endpoint.get().ok_or_else(|| {
            galat(
                tui_wire::ErrorCode::Protocol,
                "endpoint POST SSE MCP belum diketahui".to_string(),
            )
        })?;
        let mut pesan = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if !params.is_null() {
            pesan["params"] = params;
        }
        let (tx, rx) = oneshot::channel();
        self.tunggu.lock().await.insert(id, tx);
        let mut permintaan = self
            .http
            .post(endpoint)
            .header("Content-Type", "application/json");
        for (kunci, nilai) in self.headers.iter() {
            permintaan = permintaan.header(kunci.as_str(), nilai.as_str());
        }
        // POST diulang satu kali bila koneksi gagal (reconnect ringan saat
        // server MCP baru siap / beban tinggi). Builder dibangun ulang tiap
        // percobaan (RequestBuilder tidak Clone di reqwest 0.12).
        let kirim_post = || {
            let mut builder = self
                .http
                .post(endpoint)
                .header("Content-Type", "application/json");
            for (kunci, nilai) in self.headers.iter() {
                builder = builder.header(kunci.as_str(), nilai.as_str());
            }
            builder.json(&pesan).send()
        };
        let response = match kirim_post().await {
            Ok(jawaban) => jawaban,
            Err(_) => tokio::time::timeout(timeout, kirim_post())
                .await
                .map_err(|_| {
                    galat(
                        tui_wire::ErrorCode::GatewayUnreachable,
                        format!("POST ke {endpoint} gagal dua kali"),
                    )
                })?
                .map_err(|err| {
                    galat(
                        tui_wire::ErrorCode::GatewayUnreachable,
                        format!("POST ke {endpoint} gagal: {err}"),
                    )
                })?,
        };
        let status = response.status();
        if !status.is_success() && status.as_u16() != 202 {
            let body = response.text().await.unwrap_or_default();
            self.tunggu.lock().await.remove(&id);
            return Err(galat(
                tui_wire::ErrorCode::GatewayServer,
                format!("server MCP {method} balas {status}: {}", potong(&body)),
            ));
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => ekstrak_result(&result, method),
            Ok(Err(_)) => Err(galat(
                tui_wire::ErrorCode::GatewayUnreachable,
                "stream SSE MCP ditutup sebelum balasan".to_string(),
            )),
            Err(_) => {
                self.tunggu.lock().await.remove(&id);
                Err(galat(
                    tui_wire::ErrorCode::ToolTimeout,
                    format!("server MCP SSE tidak menjawab {method} dalam batas waktu"),
                ))
            }
        }
    }

    /// Notifikasi (tanpa id) — POST dan abaikan balasan.
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), tui_wire::MxError> {
        let endpoint = self.endpoint.get().ok_or_else(|| {
            galat(
                tui_wire::ErrorCode::Protocol,
                "endpoint POST SSE MCP belum diketahui".to_string(),
            )
        })?;
        let mut pesan = json!({"jsonrpc": "2.0", "method": method});
        if !params.is_null() {
            pesan["params"] = params;
        }
        let mut permintaan = self
            .http
            .post(endpoint)
            .header("Content-Type", "application/json");
        for (kunci, nilai) in self.headers.iter() {
            permintaan = permintaan.header(kunci.as_str(), nilai.as_str());
        }
        let response = permintaan.json(&pesan).send().await.map_err(|err| {
            galat(
                tui_wire::ErrorCode::GatewayUnreachable,
                format!("POST notifikasi ke {endpoint} gagal: {err}"),
            )
        })?;
        let status = response.status();
        if !status.is_success() && status.as_u16() != 202 {
            return Err(galat(
                tui_wire::ErrorCode::GatewayServer,
                format!("notifikasi {method} ditolak {status}"),
            ));
        }
        Ok(())
    }

    /// Langgan notifikasi server lewat stream GET yang sudah berjalan.
    pub async fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<crate::McpNotification> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // Tunggakan dulu (notifikasi bisa datang sebelum subscribe dipanggil).
        {
            let mut tunggakan = self.tertunda.lock().await;
            for n in tunggakan.drain(..) {
                let _ = tx.send(n);
            }
        }
        *self.notifications.lock().await = Some(tx);
        rx
    }

    /// Task pembaca: proses stream SSE dan sebar balasan ke penerima per id.
    fn jalankan_pembaca(&self, mut response: reqwest::Response) {
        let tunggu = self.tunggu.clone();
        let endpoint_slot = self.endpoint.clone();
        let notifikasi = self.notifications.clone();
        let tertunda = self.tertunda.clone();
        let url_get = self.url_get.clone();
        tokio::spawn(async move {
            let mut data_line: Option<String> = None;
            let mut event_name = String::new();
            let mut buffer: Vec<u8> = Vec::new();
            loop {
                let chunk = match response.chunk().await {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) | Err(_) => break,
                };
                buffer.extend_from_slice(&chunk);
                while let Some(posisi) = buffer.iter().position(|b| *b == b'\n') {
                    let baris: Vec<u8> = buffer.drain(..=posisi).collect();
                    let baris = String::from_utf8_lossy(&baris).trim().to_string();
                    if baris.is_empty() {
                        // Akhir event: proses data yang terkumpul.
                        if let Some(data) = data_line.take() {
                            if event_name == "endpoint" {
                                // Endpoint boleh path relatif → resolusi ke
                                // origin URL GET (omp: resolve terhadap base).
                                let absolut = if data.starts_with("http://")
                                    || data.starts_with("https://")
                                {
                                    data.clone()
                                } else if let Some(origin) = url_get
                                    .split_once("://")
                                    .and_then(|(_, sisanya)| sisanya.split('/').next())
                                {
                                    format!(
                                        "{}://{}{}",
                                        url_get.split("://").next().unwrap_or("http"),
                                        origin,
                                        data
                                    )
                                } else {
                                    data.clone()
                                };
                                let _ = endpoint_slot.set(absolut);
                            } else if let Ok(pesan) = serde_json::from_str::<Value>(&data) {
                                if let Some(id) = pesan.get("id").and_then(Value::as_u64)
                                    && let Some(sender) = tunggu.lock().await.remove(&id)
                                {
                                    let _ = sender.send(pesan);
                                } else if let Some(method) =
                                    pesan.get("method").and_then(Value::as_str)
                                {
                                    // Notifikasi (tanpa id): kirim ke langganan
                                    // bila ada; belum ada → tunggakan (retensi 100).
                                    let notif = crate::McpNotification {
                                        method: method.to_string(),
                                        params: pesan.get("params").cloned().unwrap_or(Value::Null),
                                    };
                                    let slot = notifikasi.lock().await;
                                    match &*slot {
                                        Some(tx) => {
                                            let _ = tx.send(notif);
                                        }
                                        None => {
                                            drop(slot);
                                            let mut tunggakan = tertunda.lock().await;
                                            if tunggakan.len() < 100 {
                                                tunggakan.push(notif);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        event_name.clear();
                        continue;
                    }
                    if let Some(nilai) = baris.strip_prefix("event:") {
                        event_name = nilai.trim().to_string();
                    } else if let Some(nilai) = baris.strip_prefix("data:") {
                        data_line = Some(nilai.trim().to_string());
                    }
                }
            }
            // Stream ditutup: bebaskan semua penerima agar tidak menggantung.
            tunggu.lock().await.clear();
            let _ = url_get;
        });
    }
}
