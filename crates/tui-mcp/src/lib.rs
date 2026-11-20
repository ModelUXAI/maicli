//! Klien MCP (Model Context Protocol) transport **stdio** dan bridge tool untuk
//! agent `mx`.
//!
//! Status v1: `initialize` → `notifications/initialized` → `tools/list` →
//! `tools/call`, ditambah penanganan request server (`ping`, `roots/list`).
//! Transport HTTP/SSE, OAuth, dan lifecycle/reconnect belum ada (lihat
//! `docs/platform-dan-integrasi.md` §7).
//!
//! Tool MCP didaftarkan ke registry dengan nama `mcp.<server>.<tool>` dan tier
//! **exec** secara default supaya selalu melewati gate approval; embedder yang
//! lebih tahu bisa mengganti tier lewat [`McpConnection::into_tools_with_tier`].

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod http;
pub mod sse;

pub use http::resolve_header;

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tui_tools::{Tool, ToolContext, ToolRegistry};
use tui_wire::{Concurrency, ErrorCode, MxError, ToolDefinition, ToolOutput, ToolTier};

/// Versi protokol MCP yang diminta klien pada `initialize`.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Batas baris jawaban server MCP (mencegah skrip rusak menghabiskan memori).
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Konfigurasi satu server MCP stdio.
#[derive(Debug, Clone)]
pub struct ServerSpec {
    /// Nama logis server; dipakai sebagai prefiks nama tool.
    pub name: String,
    /// Perintah yang dijalankan.
    pub command: String,
    /// Argumen perintah.
    pub args: Vec<String>,
    /// Environment tambahan/penimpa untuk server.
    pub env: Vec<(String, String)>,
    /// Batas waktu satu request MCP.
    pub timeout: Duration,
}

impl ServerSpec {
    /// Spesifikasi minimal: nama + perintah, timeout 30 detik.
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Tambah satu argumen.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Tambah satu variabel environment.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Ganti batas waktu request.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Satu tool yang diumumkan server MCP.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolInfo {
    /// Nama tool di server.
    pub name: String,
    /// Deskripsi untuk model.
    pub description: String,
    /// Skema parameter JSON.
    pub input_schema: Value,
}

/// Hasil `tools/call`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolResult {
    /// Teks gabungan dari blok konten.
    pub text: String,
    /// Apakah server menandai hasil sebagai error.
    pub is_error: bool,
}

fn galat(kode: ErrorCode, pesan: impl Into<String>) -> MxError {
    MxError::new(kode, pesan.into())
}

fn nama_env_rahasia(nama: &str) -> bool {
    let atas = nama.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD"]
        .iter()
        .any(|pola| atas.contains(pola))
}

/// Parse jawaban `tools/list` (dipakai stdio dan HTTP).
fn parse_tools(result: &Value, server: &str) -> Result<Vec<McpToolInfo>, MxError> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            galat(
                ErrorCode::Protocol,
                format!("jawaban tools/list server '{server}' tanpa array tools"),
            )
        })?;
    let mut daftar = Vec::with_capacity(tools.len());
    for tool in tools {
        let nama = tool
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                galat(
                    ErrorCode::Protocol,
                    format!("tool MCP dari '{server}' tanpa name"),
                )
            })?
            .to_string();
        let deskripsi = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let skema = tool
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object"}));
        daftar.push(McpToolInfo {
            name: nama,
            description: deskripsi,
            input_schema: skema,
        });
    }
    Ok(daftar)
}

/// Parse jawaban `tools/call` (dipakai stdio dan HTTP).
fn parse_call(result: &Value) -> Result<McpToolResult, MxError> {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut teks = String::new();
    if let Some(blok) = result.get("content").and_then(Value::as_array) {
        for item in blok {
            let tipe = item.get("type").and_then(Value::as_str).unwrap_or("?");
            if tipe == "text" {
                if let Some(isi) = item.get("text").and_then(Value::as_str) {
                    if !teks.is_empty() {
                        teks.push('\n');
                    }
                    teks.push_str(isi);
                }
            } else {
                if !teks.is_empty() {
                    teks.push('\n');
                }
                teks.push_str(&format!("[konten non-teks: {tipe}]"));
            }
        }
    }
    Ok(McpToolResult {
        text: teks,
        is_error,
    })
}

/// Klien satu server MCP via stdin/stdout anak proses.
pub struct McpClient {
    name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
    timeout: Duration,
    /// Penerima notifikasi server (progress/log); `None` = dibuang.
    notifications: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<McpNotification>>>>,
}

/// Notifikasi dari server MCP (bukan balasan request).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpNotification {
    /// Nama metode notifikasi, mis. `notifications/progress`.
    pub method: String,
    /// Isi `params` bila ada.
    pub params: Value,
}

impl McpClient {
    /// Jalankan server (tanpa handshake); panggil [`Self::initialize`] setelahnya.
    pub async fn start(spec: &ServerSpec) -> Result<Self, MxError> {
        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (kunci, nilai) in &spec.env {
            command.env(kunci, nilai);
        }
        // Server MCP tidak perlu mewarisi kredensial gateway/host.
        for (nama, _) in std::env::vars() {
            if nama_env_rahasia(&nama) {
                command.env_remove(&nama);
            }
        }
        let mut child = command.spawn().map_err(|err| {
            galat(
                ErrorCode::ToolFailed,
                format!(
                    "server MCP '{}' tidak bisa dijalankan ({}): {err}",
                    spec.name, spec.command
                ),
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            galat(
                ErrorCode::Protocol,
                format!("stdin server MCP '{}' tidak tersedia", spec.name),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            galat(
                ErrorCode::Protocol,
                format!("stdout server MCP '{}' tidak tersedia", spec.name),
            )
        })?;
        let stderr_tail = Arc::new(Mutex::new(VecDeque::new()));
        if let Some(stderr) = child.stderr.take() {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if let Ok(mut guard) = tail.try_lock() {
                        if guard.len() >= 20 {
                            guard.pop_front();
                        }
                        guard.push_back(line);
                    }
                }
            });
        }
        Ok(Self {
            name: spec.name.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_tail,
            next_id: 1,
            timeout: spec.timeout,
            notifications: Arc::new(Mutex::new(None)),
        })
    }

    /// Handshake MCP: `initialize` lalu `notifications/initialized`.
    pub async fn initialize(&mut self) -> Result<(), MxError> {
        let _ = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {"name": "mx", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
            .await?;
        self.notify("notifications/initialized", Value::Null).await
    }

    /// Daftar tool yang diumumkan server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, MxError> {
        let result = self.request("tools/list", json!({})).await?;
        parse_tools(&result, &self.name)
    }

    /// Panggil satu tool di server.
    pub async fn call_tool(
        &mut self,
        nama: &str,
        arguments: Value,
    ) -> Result<McpToolResult, MxError> {
        let result = self
            .request("tools/call", json!({"name": nama, "arguments": arguments}))
            .await?;
        parse_call(&result)
    }

    /// Langgan notifikasi server; mengembalikan kanal penerima.
    pub async fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpNotification> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *self.notifications.lock().await = Some(tx);
        rx
    }

    /// Hentikan server (SIGKILL anak; `kill_on_drop` juga berlaku).
    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), MxError> {
        let msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.tulis(&msg).await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, MxError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.tulis(&msg).await?;
        let deadline = tokio::time::Instant::now() + self.timeout;
        let mut buf = String::new();
        loop {
            let sisa = deadline.saturating_duration_since(tokio::time::Instant::now());
            if sisa.is_zero() {
                return Err(galat(
                    ErrorCode::ToolTimeout,
                    format!(
                        "server MCP '{}' tidak menjawab {method} dalam {:?}",
                        self.name, self.timeout
                    ),
                ));
            }
            buf.clear();
            let dibaca = tokio::time::timeout(sisa, self.stdout.read_line(&mut buf))
                .await
                .map_err(|_| {
                    galat(
                        ErrorCode::ToolTimeout,
                        format!(
                            "server MCP '{}' tidak menjawab {method} dalam {:?}",
                            self.name, self.timeout
                        ),
                    )
                })?
                .map_err(|err| {
                    galat(
                        ErrorCode::ToolFailed,
                        format!("gagal membaca stdout server MCP '{}': {err}", self.name),
                    )
                })?;
            if dibaca == 0 {
                return Err(galat(
                    ErrorCode::ToolFailed,
                    format!(
                        "server MCP '{}' menutup stdout saat menunggu {method}{}",
                        self.name,
                        self.tail_stderr().await
                    ),
                ));
            }
            if buf.len() > MAX_LINE_BYTES {
                return Err(galat(
                    ErrorCode::Protocol,
                    format!("jawaban MCP melebihi {MAX_LINE_BYTES} byte"),
                ));
            }
            let baris = buf.trim();
            if baris.is_empty() {
                continue;
            }
            let pesan: Value = serde_json::from_str(baris).map_err(|err| {
                galat(
                    ErrorCode::Protocol,
                    format!("jawaban MCP bukan JSON: {err}"),
                )
            })?;
            if pesan.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = pesan.get("error") {
                    return Err(galat(
                        ErrorCode::ToolFailed,
                        format!("server MCP '{}' menolak {method}: {err}", self.name),
                    ));
                }
                return Ok(pesan.get("result").cloned().unwrap_or(Value::Null));
            }
            if let Some(metode_server) = pesan.get("method").and_then(Value::as_str) {
                if let Some(id_server) = pesan.get("id").cloned() {
                    self.jawab_request_server(metode_server, id_server).await?;
                } else if let Ok(guard) = self.notifications.try_lock() {
                    if let Some(tx) = guard.as_ref() {
                        let _ = tx.send(McpNotification {
                            method: metode_server.to_string(),
                            params: pesan.get("params").cloned().unwrap_or(Value::Null),
                        });
                    }
                }
            }
        }
    }

    async fn jawab_request_server(&mut self, method: &str, id: Value) -> Result<(), MxError> {
        let jawaban = match method {
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            "roots/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"roots": []}}),
            lain => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("method tidak didukung: {lain}")}
            }),
        };
        self.tulis(&jawaban).await
    }

    async fn tulis(&mut self, value: &Value) -> Result<(), MxError> {
        let mut baris = serde_json::to_string(value)
            .map_err(|err| galat(ErrorCode::Protocol, format!("gagal serialisasi MCP: {err}")))?;
        baris.push('\n');
        self.stdin
            .write_all(baris.as_bytes())
            .await
            .map_err(|err| {
                galat(
                    ErrorCode::ToolFailed,
                    format!("gagal menulis ke server MCP '{}': {err}", self.name),
                )
            })?;
        self.stdin.flush().await.map_err(|err| {
            galat(
                ErrorCode::ToolFailed,
                format!("gagal flush ke server MCP '{}': {err}", self.name),
            )
        })
    }

    async fn tail_stderr(&self) -> String {
        let guard = self.stderr_tail.lock().await;
        if guard.is_empty() {
            return String::new();
        }
        let teks: Vec<&str> = guard.iter().map(String::as_str).collect();
        format!("; stderr: {}", teks.join(" | "))
    }
}

/// Koneksi MCP yang sudah handshake dan sudah membaca daftar tool.
pub struct McpConnection {
    server: String,
    client: Arc<Mutex<McpBackend>>,
    tools: Vec<McpToolInfo>,
}

/// Backend koneksi: stdio (anak proses), HTTP streamable, atau legacy SSE.
enum McpBackend {
    Stdio(Box<McpClient>),
    Http(HttpMcpClient),
    Sse(SseMcpClient),
}

impl McpConnection {
    /// Nama server.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Tool yang diumumkan server.
    pub fn tools(&self) -> &[McpToolInfo] {
        &self.tools
    }

    /// Langgan notifikasi server (progress/log) lewat klien.
    ///
    /// HTTP: GET stream dengan **reconnect otomatis** (500 ms); SSE: aliran GET
    /// legacy; stdio: pembaca proses anak.
    pub async fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpNotification> {
        let mut backend = self.client.lock().await;
        match &mut *backend {
            McpBackend::Stdio(klien) => klien.subscribe().await,
            McpBackend::Http(klien) => {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                // Tugas aliran hidup selama rx (dan klien) dipakai; reconnect
                // ditangani di dalam stream_notifikasi.
                let transport = klien.transport_cloner();
                tokio::spawn(async move { transport.stream_notifikasi(tx).await });
                rx
            }
            McpBackend::Sse(klien) => {
                // SSE legacy: notifikasi juga mengalir di stream GET yang sama.
                klien.subscribe().await
            }
        }
    }

    /// Panggil tool langsung (tanpa registry), berguna untuk host/CLI.
    pub async fn call_tool(&self, nama: &str, arguments: Value) -> Result<McpToolResult, MxError> {
        let mut backend = self.client.lock().await;
        match &mut *backend {
            McpBackend::Stdio(klien) => klien.call_tool(nama, arguments).await,
            McpBackend::Http(klien) => klien.call_tool(nama, arguments).await,
            McpBackend::Sse(klien) => klien.call_tool(nama, arguments).await,
        }
    }

    /// Tutup koneksi (HTTP: DELETE sesi; stdio: tidak melakukan apa-apa).
    pub async fn close(&self) {
        let mut backend = self.client.lock().await;
        if let McpBackend::Http(klien) = &mut *backend {
            klien.transport.close().await;
        }
    }

    /// Ubah semua tool server menjadi `Arc<dyn Tool>` bertier `exec`.
    pub fn into_tools(self) -> Vec<Arc<dyn Tool>> {
        self.into_tools_with_tier(ToolTier::Exec)
    }

    /// Seperti [`Self::into_tools`] tetapi tier bisa ditentukan embedder.
    pub fn into_tools_with_tier(self, tier: ToolTier) -> Vec<Arc<dyn Tool>> {
        self.tools
            .into_iter()
            .map(|info| {
                Arc::new(McpToolAdapter {
                    server: self.server.clone(),
                    client: self.client.clone(),
                    info,
                    tier,
                }) as Arc<dyn Tool>
            })
            .collect()
    }

    /// Daftarkan semua tool ke registry yang sudah ada.
    pub fn register_into(self, registry: &mut ToolRegistry) -> Result<(), MxError> {
        for tool in self.into_tools() {
            let nama = tool.definition().name;
            if registry.names().iter().any(|ada| ada == &nama) {
                return Err(galat(
                    ErrorCode::Config,
                    format!("nama tool MCP bertabrakan dengan tool yang sudah ada: {nama}"),
                ));
            }
            registry.register(tool);
        }
        Ok(())
    }
}

/// Jalankan server MCP stdio, handshake, lalu kembalikan koneksi siap pakai.
pub async fn connect_stdio(spec: ServerSpec) -> Result<McpConnection, MxError> {
    let mut client = McpClient::start(&spec).await?;
    client.initialize().await?;
    let tools = client.list_tools().await?;
    Ok(McpConnection {
        server: spec.name,
        client: Arc::new(Mutex::new(McpBackend::Stdio(Box::new(client)))),
        tools,
    })
}

/// Jalankan koneksi MCP Streamable HTTP, handshake, lalu kembalikan koneksi.
pub async fn connect_http(
    url: impl Into<String>,
    name: impl Into<String>,
    timeout: Duration,
) -> Result<McpConnection, MxError> {
    connect_http_with(
        url,
        name,
        timeout,
        Arc::new(std::collections::HashMap::new()),
    )
    .await
}

/// Sama seperti [`connect_http`] dengan header kustom (mis. Bearer token).
pub async fn connect_http_with(
    url: impl Into<String>,
    name: impl Into<String>,
    timeout: Duration,
    headers: Arc<std::collections::HashMap<String, String>>,
) -> Result<McpConnection, MxError> {
    let nama = name.into();
    let transport = http::HttpTransport::new(url, timeout, headers)?;
    let mut klien = HttpMcpClient {
        nama: nama.clone(),
        transport,
        next_id: 1,
    };
    klien.initialize().await?;
    let tools = klien.list_tools().await?;
    Ok(McpConnection {
        server: nama,
        client: Arc::new(Mutex::new(McpBackend::Http(klien))),
        tools,
    })
}

/// Klien MCP di atas `HttpTransport` dengan API sama seperti `McpClient`.
struct HttpMcpClient {
    nama: String,
    transport: http::HttpTransport,
    next_id: u64,
}

impl HttpMcpClient {
    /// Salinan transport untuk tugas aliran notifikasi (GET stream).
    fn transport_cloner(&self) -> http::HttpTransport {
        self.transport.clone()
    }
}

/// Klien MCP di atas `SseTransport` (legacy 2024-11-05).
struct SseMcpClient {
    nama: String,
    transport: sse::SseTransport,
    next_id: u64,
}

impl SseMcpClient {
    /// Langgan notifikasi lewat transport.
    async fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<McpNotification> {
        self.transport.subscribe().await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, MxError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.transport
            .request(method, params, id, Duration::from_secs(15))
            .await
    }

    async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, MxError> {
        let result = self.request("tools/list", json!({})).await?;
        parse_tools(&result, &self.nama)
    }

    async fn call_tool(&mut self, nama: &str, arguments: Value) -> Result<McpToolResult, MxError> {
        let result = self
            .request("tools/call", json!({"name": nama, "arguments": arguments}))
            .await?;
        parse_call(&result)
    }
}

/// Jalankan koneksi MCP legacy SSE (2024-11-05), tunggu `endpoint`, ambil tool.
pub async fn connect_sse(
    url: impl Into<String>,
    name: impl Into<String>,
    timeout: Duration,
) -> Result<McpConnection, MxError> {
    connect_sse_with(
        url,
        name,
        timeout,
        Arc::new(std::collections::HashMap::new()),
    )
    .await
}

/// Sama seperti [`connect_sse`] dengan header kustom (mis. Bearer token).
pub async fn connect_sse_with(
    url: impl Into<String>,
    name: impl Into<String>,
    timeout: Duration,
    headers: Arc<std::collections::HashMap<String, String>>,
) -> Result<McpConnection, MxError> {
    let nama = name.into();
    let transport = sse::SseTransport::connect(url, timeout, headers).await?;
    let mut klien = SseMcpClient {
        nama: nama.clone(),
        transport,
        next_id: 1,
    };
    let tools = klien.list_tools().await?;
    Ok(McpConnection {
        server: nama,
        client: Arc::new(Mutex::new(McpBackend::Sse(klien))),
        tools,
    })
}

impl HttpMcpClient {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, MxError> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.transport.request(method, params, id).await
    }

    async fn initialize(&mut self) -> Result<(), MxError> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "mx", "version": env!("CARGO_PKG_VERSION")}
            }),
        )
        .await?;
        self.transport
            .notify("notifications/initialized", Value::Null)
            .await
    }

    async fn list_tools(&mut self) -> Result<Vec<McpToolInfo>, MxError> {
        let result = self.request("tools/list", json!({})).await?;
        parse_tools(&result, &self.nama)
    }

    async fn call_tool(&mut self, nama: &str, arguments: Value) -> Result<McpToolResult, MxError> {
        let result = self
            .request("tools/call", json!({"name": nama, "arguments": arguments}))
            .await?;
        parse_call(&result)
    }
}
/// Adapter satu tool MCP sebagai [`Tool`] agent.
struct McpToolAdapter {
    server: String,
    client: Arc<Mutex<McpBackend>>,
    info: McpToolInfo,
    tier: ToolTier,
}

#[async_trait::async_trait]
impl Tool for McpToolAdapter {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: format!("mcp.{}.{}", self.server, self.info.name),
            description: if self.info.description.is_empty() {
                format!(
                    "tool MCP '{}' dari server '{}'",
                    self.info.name, self.server
                )
            } else {
                self.info.description.clone()
            },
            parameters: self.info.input_schema.clone(),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        self.tier
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let mut backend = self.client.lock().await;
        let hasil = match &mut *backend {
            McpBackend::Stdio(klien) => klien.call_tool(&self.info.name, args.clone()).await?,
            McpBackend::Http(klien) => klien.call_tool(&self.info.name, args.clone()).await?,
            McpBackend::Sse(klien) => klien.call_tool(&self.info.name, args.clone()).await?,
        };
        Ok(if hasil.is_error {
            ToolOutput::err(hasil.text)
        } else {
            ToolOutput::ok(hasil.text)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::sync::atomic::{AtomicU64, Ordering};

    // Helper hanya dipakai test transport proses (mock Windows PowerShell);
    // test Linux tidak memakainya — cfg agar tidak dead-code lint.
    #[cfg(windows)]
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    #[cfg(windows)]
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mx-mcp-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn server_tidak_ada_dilaporkan() {
        let spec =
            ServerSpec::new("hilang", "perintah-tidak-ada-xyz").timeout(Duration::from_secs(2));
        let err = match connect_stdio(spec).await {
            Ok(_) => panic!("harus gagal"),
            Err(err) => err,
        };
        assert_eq!(err.code, ErrorCode::ToolFailed);
        assert!(
            err.message.contains("tidak bisa dijalankan"),
            "{}",
            err.message
        );
    }

    #[cfg(windows)]
    const MOCK_SERVER: &str = r#"
$ErrorActionPreference='Stop'
function Kirim($obj) {
  [Console]::Out.WriteLine(($obj | ConvertTo-Json -Compress -Depth 12))
  [Console]::Out.Flush()
}
while ($null -ne ($line = [Console]::In.ReadLine())) {
  if ($line.Trim() -eq '') { continue }
  $msg = $line | ConvertFrom-Json
  $id = $msg.id
  $method = $msg.method
  if ($method -eq 'initialize') {
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{
      protocolVersion='2025-06-18'; capabilities=@{}; serverInfo=@{ name='mock'; version='1.0' } } }
  } elseif ($method -eq 'notifications/initialized') {
  } elseif ($method -eq 'tools/list') {
    $alat = @(
      @{ name='echo'; description='kembalikan argumen';
         inputSchema=@{ type='object'; properties=@{ echo=@{ type='string' } }; required=@('echo') } },
      @{ name='gagal'; description='selalu gagal'; inputSchema=@{ type='object' } }
    )
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{ tools=$alat } }
  } elseif ($method -eq 'tools/call') {
    if ($msg.params.name -eq 'gagal') {
      Kirim @{ jsonrpc='2.0'; id=$id; result=@{ content=@(@{ type='text'; text='gagal dari mock' }); isError=$true } }
    } else {
      $teks = [string]$msg.params.arguments.echo
      Kirim @{ jsonrpc='2.0'; id=$id; result=@{ content=@(@{ type='text'; text=$teks }) } }
    }
  } elseif ($id) {
    Kirim @{ jsonrpc='2.0'; id=$id; error=@{ code=-32601; message='tidak didukung' } }
  }
}
"#;

    #[cfg(windows)]
    fn tulis_mock(dir: &std::path::Path) -> std::path::PathBuf {
        let skrip = dir.join("mock_mcp.ps1");
        std::fs::write(&skrip, MOCK_SERVER).expect("tulis skrip mock");
        skrip
    }

    #[cfg(windows)]
    fn spec_mock(dir: &std::path::Path) -> ServerSpec {
        let skrip = tulis_mock(dir);
        let mut spec = ServerSpec::new("mock", "powershell").timeout(Duration::from_secs(15));
        spec = spec
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(skrip.display().to_string());
        spec
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn notifikasi_progress_diteruskan_ke_langganan() {
        let dir = temp_dir("notif");
        let skrip = dir.join("mock_notif.ps1");
        std::fs::write(
            &skrip,
            r#"
$ErrorActionPreference='Stop'
function Kirim($obj) {
  [Console]::Out.WriteLine(($obj | ConvertTo-Json -Compress -Depth 12))
  [Console]::Out.Flush()
}
while ($null -ne ($line = [Console]::In.ReadLine())) {
  if ($line.Trim() -eq '') { continue }
  $msg = $line | ConvertFrom-Json
  $id = $msg.id
  if ($msg.method -eq 'initialize') {
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{ protocolVersion='2025-06-18'; capabilities=@{}; serverInfo=@{ name='n'; version='1' } } }
  } elseif ($msg.method -eq 'tools/list') {
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{ tools=@( @{ name='apa'; description='uji'; inputSchema=@{ type='object' } } ) } }
  } elseif ($msg.method -eq 'tools/call') {
    Kirim @{ jsonrpc='2.0'; method='notifications/progress'; params=@{ progressToken=1; progress=50; total=100 } }
    Kirim @{ jsonrpc='2.0'; id=$id; result=@{ content=@( @{ type='text'; text='selesai' } ) } }
  } elseif ($msg.method -eq 'notifications/initialized') {
  }
}
"#,
        )
        .expect("tulis");
        let spec = ServerSpec::new("notif", "powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(skrip.display().to_string())
            .timeout(Duration::from_secs(10));
        let koneksi = connect_stdio(spec).await.expect("handshake");
        let mut notif = koneksi.subscribe().await;
        // Panggil tool: server mengirim progress SEBELUM menjawab.
        let hasil = koneksi
            .call_tool("apa", serde_json::json!({}))
            .await
            .expect("call");
        assert_eq!(hasil.text, "selesai");
        let diterima = tokio::time::timeout(Duration::from_millis(500), notif.recv()).await;
        match diterima {
            Ok(Some(n)) => {
                assert!(n.method.contains("progress"), "{:?}", n.method);
                assert_eq!(n.params.get("progress").and_then(Value::as_u64), Some(50));
            }
            other => panic!("notifikasi progress harus diterima, dapat {other:?}"),
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn handshake_list_dan_call_tool() {
        let dir = temp_dir("handshake");
        let koneksi = connect_stdio(spec_mock(&dir)).await.expect("handshake");
        assert_eq!(koneksi.server(), "mock");
        let nama: Vec<&str> = koneksi.tools().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(nama, vec!["echo", "gagal"]);

        let hasil = koneksi
            .call_tool("echo", serde_json::json!({"echo": "halo mcp"}))
            .await
            .expect("call");
        assert!(!hasil.is_error);
        assert_eq!(hasil.text, "halo mcp");

        let gagal = koneksi
            .call_tool("gagal", serde_json::json!({}))
            .await
            .expect("call gagal");
        assert!(gagal.is_error);
        assert!(gagal.text.contains("gagal dari mock"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn adapter_tool_berprefix_dan_bertier_exec() {
        let dir = temp_dir("adapter");
        let koneksi = connect_stdio(spec_mock(&dir)).await.expect("handshake");
        let tools = koneksi.into_tools();
        assert_eq!(tools.len(), 2);
        let echo = tools
            .iter()
            .find(|t| t.definition().name == "mcp.mock.echo")
            .expect("adapter echo");
        assert_eq!(echo.tier(), ToolTier::Exec);
        assert_eq!(echo.definition().concurrency, Concurrency::Exclusive);
        let ctx = ToolContext::new(&dir);
        let out = echo
            .execute(&serde_json::json!({"echo": "lewat adapter"}), &ctx)
            .await
            .expect("eksekusi");
        assert!(!out.is_error);
        assert_eq!(out.content, "lewat adapter");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    const MOCK_HTTP_SERVER: &str = r#"
$ErrorActionPreference='Stop'
$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add('http://127.0.0.1:18491/')
$listener.Start()
try {
  while ($listener.IsListening) {
    $ctx = $listener.GetContext()
    $req = $ctx.Request
    $body = (New-Object IO.StreamReader($req.InputStream)).ReadToEnd()
    if ($req.HttpMethod -eq 'DELETE') { $ctx.Response.Close(); break }
    $msg = $body | ConvertFrom-Json
    $resp = $ctx.Response
    $id = $msg.id
    if ($null -eq $id) { $resp.StatusCode=202; $resp.Close(); continue }
    $method = $msg.method
    $hasil = $null
    if ($method -eq 'initialize') {
      $hasil = @{ protocolVersion='2025-06-18'; capabilities=@{}; serverInfo=@{ name='mock-http'; version='1.0' } }
      $resp.Headers.Add('Mcp-Session-Id','sesi-uji-1')
    } elseif ($method -eq 'tools/list') {
      $hasil = @{ tools=@( @{ name='echo'; description='balas'; inputSchema=@{ type='object' } } ) }
    } elseif ($method -eq 'tools/call') {
      $hasil = @{ content=@( @{ type='text'; text=[string]$msg.params.arguments.echo } ) }
    } else {
      $out = @{ jsonrpc='2.0'; id=$id; error=@{ code=-32601; message='tidak didukung' } }
      $json = $out | ConvertTo-Json -Compress
      $buf = [Text.Encoding]::UTF8.GetBytes($json)
      $resp.ContentType='application/json'; $resp.ContentLength64=$buf.Length
      $resp.OutputStream.Write($buf,0,$buf.Length); $resp.Close(); continue
    }
    $out = @{ jsonrpc='2.0'; id=$id; result=$hasil }
    $json = $out | ConvertTo-Json -Compress -Depth 12
    $buf = [Text.Encoding]::UTF8.GetBytes($json)
    $resp.ContentType='application/json'; $resp.ContentLength64=$buf.Length
    $resp.OutputStream.Write($buf,0,$buf.Length)
    $resp.Close()
  }
} finally { $listener.Stop() }
"#;

    #[cfg(windows)]
    #[tokio::test]
    async fn http_handshake_list_dan_call() {
        let dir = temp_dir("http");
        let skrip = dir.join("mock_http.ps1");
        std::fs::write(&skrip, MOCK_HTTP_SERVER).expect("tulis mock");
        let mut server = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&skrip)
            .spawn()
            .expect("jalankan mock HTTP");
        // beri waktu listener naik: tunggu port siap (bukan tidur tetap yang
        // rapuh saat beban kompilasi paralel tinggi).
        let mut siap = false;
        for _ in 0..50 {
            if std::net::TcpStream::connect("127.0.0.1:18491").is_ok() {
                siap = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(siap, "mock HTTP tidak siap");
        let koneksi = connect_http(
            "http://127.0.0.1:18491/",
            "mock-http",
            Duration::from_secs(10),
        )
        .await
        .expect("handshake HTTP");
        assert_eq!(koneksi.server(), "mock-http");
        let nama: Vec<&str> = koneksi.tools().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(nama, vec!["echo"]);
        let hasil = koneksi
            .call_tool("echo", serde_json::json!({"echo": "halo http"}))
            .await
            .expect("call");
        assert_eq!(hasil.text, "halo http");
        koneksi.close().await;
        let _ = server.kill().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn register_into_menolak_nama_bentrok() {
        let dir = temp_dir("bentrok");
        let koneksi = connect_stdio(spec_mock(&dir)).await.expect("handshake");
        let mut registry = ToolRegistry::from_tools(Vec::new());
        let tools = koneksi.into_tools();
        for tool in &tools {
            registry.register(tool.clone());
        }
        let lagi = connect_stdio(spec_mock(&dir)).await.expect("handshake");
        let err = lagi
            .register_into(&mut registry)
            .expect_err("nama bentrok harus ditolak");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("bertabrakan"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn resolve_header_meneruskan_literal_dan_env_tidak_ada() {
        // crate melarang unsafe (set_var); env var terbaca diuji via PATH
        // (variabel proses yang ada) + env yang tidak ada.
        assert_eq!(crate::resolve_header("Bearer x"), "Bearer x");
        assert_eq!(crate::resolve_header("env:TIDAK_ADA_XYZ"), "");
        assert!(
            !crate::resolve_header("env:PATH").is_empty(),
            "env:PATH harus terbaca"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn connect_http_with_mengirim_header_auth_dan_retry_503() {
        // Mock: wajib Authorization persis; 2 request pertama balas 503 (retry),
        // setelah itu 200. Counter lewat file.
        let dir = temp_dir("auth");
        let counter = dir.join("counter.txt");
        let counter_ps = counter.display().to_string().replace('\\', "/");
        let skrip = dir.join("mock_auth.ps1");
        std::fs::write(
            &skrip,
            format!(
                r#"
$ErrorActionPreference='Stop'
$counterFile = '{counter_ps}'
$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add('http://127.0.0.1:18494/')
$listener.Start()
try {{
  while ($listener.IsListening) {{
    $ctx = $listener.GetContext()
    $req = $ctx.Request
    $resp = $ctx.Response
    $auth = $req.Headers['Authorization']
    if ($auth -ne 'Bearer rahasia-uji') {{
      $resp.StatusCode = 401; $resp.Close(); continue
    }}
    $n = 0
    if (Test-Path $counterFile) {{ $n = [int][IO.File]::ReadAllText($counterFile) }}
    [IO.File]::WriteAllText($counterFile, [string]($n+1))
    if ($n -lt 1) {{
      $resp.StatusCode = 503; $resp.Close(); continue
    }}
    $body = (New-Object IO.StreamReader($req.InputStream)).ReadToEnd()
    $msg = $body | ConvertFrom-Json
    $id = $msg.id
    $hasil = @{{ tools=@( @{{ name='echo'; description='balas'; inputSchema=@{{ type='object' }} }} ) }}
    $balasan = @{{ jsonrpc='2.0'; id=$id; result=$hasil }} | ConvertTo-Json -Compress -Depth 10
    $buf = [Text.Encoding]::UTF8.GetBytes($balasan)
    $resp.ContentType = 'application/json'
    $resp.OutputStream.Write($buf,0,$buf.Length)
    $resp.Close()
  }}
}} finally {{ $listener.Stop() }}
"#
            ),
        )
        .expect("tulis");
        let mut server = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&skrip)
            .spawn()
            .expect("jalankan mock auth");
        let mut siap = false;
        for _ in 0..50 {
            if std::net::TcpStream::connect("127.0.0.1:18494").is_ok() {
                siap = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(siap, "mock auth tidak siap");
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            "Bearer rahasia-uji".to_string(),
        );
        // Handshake: initialize (503,503,ok) + tools/list (ok) → retry bekerja.
        let koneksi = connect_http_with(
            "http://127.0.0.1:18494/",
            "auth-mock",
            Duration::from_secs(10),
            Arc::new(headers),
        )
        .await
        .expect("handshake dengan header");
        assert_eq!(koneksi.tools().len(), 1);
        // Tanpa header → 401 → gagal.
        let gagal = connect_http(
            "http://127.0.0.1:18494/",
            "tanpa-auth",
            Duration::from_secs(5),
        )
        .await;
        assert!(gagal.is_err(), "tanpa header harus ditolak");
        koneksi.close().await;
        let _ = server.kill().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn http_stream_notifikasi_reconnect_setelah_eof() {
        // Mock streamable HTTP: setiap GET stream mengirim **satu** notifikasi
        // lalu menutup koneksi — klien harus GET ulang (reconnect) hingga
        // penerima di-drop. Tiga notifikasi harus diterima dari dua+ koneksi.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("alamat").port();
        let jumlah = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        std::thread::spawn(move || {
            for koneksi in listener.incoming() {
                let Ok(mut stream) = koneksi else { continue };
                let hitung = jumlah.clone();
                std::thread::spawn(move || {
                    use std::io::{Read as _, Write as _};
                    let mut buffer = [0u8; 8192];
                    let _ = stream.read(&mut buffer);
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
                    let _ = stream.write_all(
                        b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":1}}\n\n",
                    );
                    let _ = stream.flush();
                    hitung.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    // Drop stream = EOF → klien harus reconnect.
                });
            }
        });
        let klien = http::HttpTransport::new(
            format!("http://127.0.0.1:{port}"),
            Duration::from_secs(5),
            std::sync::Arc::new(std::collections::HashMap::new()),
        )
        .expect("new");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        // Aliran memakai salinan transport; klien lokal boleh selesai.
        let transport = klien.clone();
        tokio::spawn(async move { transport.stream_notifikasi(tx).await });
        // Dua notifikasi (dua koneksi berbeda) = reconnect bekerja.
        let mut diterima = 0;
        for _ in 0..40 {
            if tokio::time::timeout(Duration::from_millis(250), rx.recv())
                .await
                .is_ok()
            {
                diterima += 1;
                if diterima >= 2 {
                    break;
                }
            }
        }
        assert!(
            diterima >= 2,
            "reconnect harus memberi notifikasi berulang (dapat {diterima})"
        );
        // Bila penerima di-drop, stream berhenti (jumlah koneksi berhenti naik).
        drop(rx);
        tokio::time::sleep(Duration::from_millis(900)).await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn sse_legacy_handshake_list_dan_call() {
        // Mock legacy SSE (2024-11-05) dalam Rust murni: thread GET mengirim
        // event `endpoint` lalu meneruskan balasan dari channel; thread POST
        // menghitung jawaban dan mengirimnya ke channel GET (korelasi id).
        let (tx_balasan, rx_bagikan) = {
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            (tx, std::sync::Arc::new(std::sync::Mutex::new(rx)))
        };
        // Port dinamis: hindari bentrok saat test paralel.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("alamat").port();
        std::thread::spawn(move || {
            for koneksi in listener.incoming() {
                let Ok(mut stream) = koneksi else { continue };
                let rx = rx_bagikan.clone();
                let tx = tx_balasan.clone();
                std::thread::spawn(move || {
                    let mut buffer = [0u8; 8192];
                    use std::io::Read as _;
                    let baca = stream.read(&mut buffer).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..baca]).to_string();
                    let is_get = request.starts_with("GET");
                    if is_get {
                        let head = "HTTP/1.1 200 OK\r
Content-Type: text/event-stream\r
\r
";
                        std::io::Write::write_all(&mut stream, head.as_bytes()).ok();
                        // endpoint event menunjuk path POST (relatif → origin).
                        std::io::Write::write_all(
                            &mut stream,
                            b"event: endpoint
data: /messages

",
                        )
                        .ok();
                        // Satu notifikasi server (tanpa id) lewat stream yang sama.
                        std::io::Write::write_all(
                            &mut stream,
                            b"event: message
data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":7}}

",
                        )
                        .ok();
                        // Teruskan balasan dari channel selagi koneksi hidup.
                        while let Some(balasan) = rx.lock().ok().and_then(|penerima| {
                            penerima
                                .recv_timeout(std::time::Duration::from_secs(20))
                                .ok()
                        }) {
                            let frame = format!("event: message\ndata: {balasan}\n\n");
                            if std::io::Write::write_all(&mut stream, frame.as_bytes()).is_err() {
                                break;
                            }
                        }
                    } else if request.starts_with("POST") {
                        let body = request
                            .split(
                                "\r
\r
",
                            )
                            .nth(1)
                            .unwrap_or("")
                            .to_string();
                        let pesan: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                        let id = pesan.get("id").cloned().unwrap_or(serde_json::json!(0));
                        let method = pesan.get("method").and_then(|m| m.as_str()).unwrap_or("");
                        let hasil = match method {
                            "tools/list" => serde_json::json!({
                                "tools": [{"name": "echo", "description": "balas",
                                    "inputSchema": {"type": "object"}}]
                            }),
                            "tools/call" => serde_json::json!({
                                "content": [{"type": "text",
                                    "text": pesan["params"]["arguments"]["echo"]}]
                            }),
                            _ => serde_json::json!({ "ok": true }),
                        };
                        let jawaban = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": hasil
                        });
                        let _ = tx.send(jawaban.to_string());
                        let head = "HTTP/1.1 202 Accepted\r
Content-Length: 0\r
\r
";
                        std::io::Write::write_all(&mut stream, head.as_bytes()).ok();
                    }
                });
            }
        });
        let url = format!("http://127.0.0.1:{port}/");
        let koneksi = connect_sse(url, "sse-mock", Duration::from_secs(15))
            .await
            .expect("handshake sse");
        // Langgan notifikasi sebelum call (endpoint + notif progress sudah
        // mengalir di stream GET yang sama).
        let mut notif = koneksi.subscribe().await;
        let tools = koneksi.tools();
        assert_eq!(tools.len(), 1, "{}", tools.len());
        assert_eq!(tools[0].name, "echo");
        let hasil = koneksi
            .call_tool("echo", serde_json::json!({"echo": "lewat-sse"}))
            .await
            .expect("call");
        assert!(!hasil.is_error);
        assert_eq!(hasil.text, "lewat-sse");
        // Notifikasi dari stream GET harus diterima (progress=7).
        let diterima = tokio::time::timeout(Duration::from_secs(5), notif.recv()).await;
        let n = diterima.expect("notif tepat waktu").expect("notif ada");
        assert!(n.method.contains("progress"), "{:?}", n.method);
        assert_eq!(n.params.get("progress").and_then(Value::as_u64), Some(7));
    }
}
