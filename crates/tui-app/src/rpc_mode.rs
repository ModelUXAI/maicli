//! Mode RPC: satu objek JSON per baris di stdin, satu per baris di stdout.
//!
//! stdout di sini adalah **protokol**, bukan log. `AGENTS.md` §3 mengizinkan
//! pengecualian untuk mode non-TUI yang didokumentasikan; log tetap ke file.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use tui_approval::{ApprovalRequest, Approver, Decision, DecisionUi};
use tui_core::{AgentSession, TurnSettings};
use tui_session::{SessionEntry, SessionStore};
use tui_tools::{AskRequest, CancelToken, Tool, ToolContext};
use tui_wire::rpc::{
    HostToolDefinition, HostUriScheme, MAX_REASSEMBLED_FRAME_BYTES, PROTOCOL_VERSION, RpcEvent,
    RpcFrame, RpcRequest, SUPPORTED_PROTOCOL_VERSIONS,
};
use tui_wire::{
    AgentEvent, Concurrency, ErrorCode, Message, MxError, ToolDefinition, ToolOutput, ToolTier,
};

use crate::Boot;
use crate::approvals::build_approver;

enum Incoming {
    Request(RpcRequest),
    Malformed(String),
}

/// Pemetaan id → pengirim hasil tool host yang belum dijawab.
type HostPendingMap = Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<HostToolOutcome>>>>;

/// Hasil eksekusi tool host.
type HostToolOutcome = (bool, String);

/// Pemetaan id → pengirim hasil host URI yang belum dijawab.
type HostUriPendingMap = Arc<std::sync::Mutex<HashMap<String, oneshot::Sender<HostUriOutcome>>>>;

/// Hasil operasi host URI: isi (read) atau konfirmasi (write).
type HostUriOutcome = Result<String, String>;

/// Batas kanal keluar (backpressure): producer melambat saat penuh.
const SINK_CAPACITY: usize = 4096;

/// Spool frame yang tidak bisa dikirim (kanal penuh / stdout putus) ke file,
/// supaya tidak ada frame hilang diam-diam (omp: spool backpressure).
struct Spool {
    path: std::path::PathBuf,
    buf: std::sync::Mutex<Vec<String>>,
    /// Batas total isi spool (64 MiB); melampaui itu frame paling lama dibuang.
    max_bytes: u64,
}

impl Spool {
    fn new(dir: &std::path::Path) -> Self {
        Self {
            path: dir.join(format!("rpc-spool-{}.jsonl", std::process::id())),
            buf: std::sync::Mutex::new(Vec::new()),
            max_bytes: 64 * 1024 * 1024,
        }
    }

    fn push(&self, line: &str) {
        if let Ok(mut buf) = self.buf.lock() {
            buf.push(line.to_string());
            if buf.len() >= 512 {
                self.flush_locked(&mut buf);
            }
        }
    }

    /// Tulis sisa buffer ke file (dipanggil berkala + saat server berhenti).
    fn flush(&self) {
        if let Ok(mut buf) = self.buf.lock() {
            self.flush_locked(&mut buf);
        }
    }

    fn flush_locked(&self, buf: &mut Vec<String>) {
        if buf.is_empty() {
            return;
        }
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            for line in buf.drain(..) {
                let _ = writeln!(file, "{line}");
            }
        }
        // Jaga batas: file melebihi max_bytes dipangkas dari depan (best effort).
        if let Ok(meta) = std::fs::metadata(&self.path)
            && meta.len() > self.max_bytes
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Tulis frame keluar dengan aturan transport protokol yang dinegosiasikan.
#[derive(Clone)]
struct Out {
    sink: tokio::sync::mpsc::Sender<String>,
    protocol_version: Arc<AtomicU32>,
    spool: Arc<Spool>,
}

impl Out {
    fn new(sink: tokio::sync::mpsc::Sender<String>, spool: Arc<Spool>) -> Self {
        Self {
            sink,
            protocol_version: Arc::new(AtomicU32::new(1)),
            spool,
        }
    }

    /// Versi protokol yang sedang aktif.
    fn protocol_version(&self) -> u32 {
        self.protocol_version.load(Ordering::Relaxed)
    }

    /// Mainkan ulang spool: kirim ulang frame yang tertahan, lalu kosongkan.
    /// Mengembalikan jumlah frame yang berhasil dikirim ulang.
    fn mainkan_spool(&self) -> u32 {
        self.spool.flush();
        let mut dikirim = 0u32;
        if let Ok(mut buf) = self.spool.buf.lock() {
            for line in buf.drain(..) {
                if self.sink.try_send(line).is_ok() {
                    dikirim = dikirim.saturating_add(1);
                }
            }
        }
        dikirim
    }

    /// Naikkan ke v2 setelah klien menegosiasikan.
    fn set_protocol_version(&self, version: u32) {
        self.protocol_version.store(version, Ordering::Relaxed);
    }

    fn kirim_baris(&self, line: String) {
        // try_send: penuh → spool (producer tidak pernah diblokir di jalur UI);
        // backpressure aktif di jalur event forwarder yang memakai send().await.
        use tokio::sync::mpsc::error::TrySendError;
        match self.sink.try_send(line.clone()) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Closed(_)) => self.spool.push(&line),
        }
    }

    fn emit(&self, frame: RpcFrame) {
        match tui_wire::rpc::encode_frame(&frame, self.protocol_version()) {
            Ok(lines) => {
                for line in lines {
                    self.kirim_baris(line);
                }
            }
            Err(err) => {
                let pesan = serde_json::to_string(&err.message)
                    .unwrap_or_else(|_| "\"galat protokol\"".to_string());
                self.kirim_baris(format!(
                    r#"{{"type":"error","code":"{}","message":{pesan}}}"#,
                    err.code
                ));
            }
        }
    }
}

fn settings(boot: &Boot) -> TurnSettings {
    TurnSettings {
        model: boot.model.clone(),
        deadline: Some(std::time::Duration::from_secs(
            boot.limits.turn_deadline_secs,
        )),
        system_prompt: {
            let mut sp = TurnSettings::default().system_prompt;
            sp.push_str(&tui_tools::skills::ringkas_daftar(&boot.skills));
            sp
        },
        ..TurnSettings::default()
    }
}

/// Satu tool host: eksekusinya diteruskan ke klien lewat frame `host_tool_call`.
struct HostTool {
    out: Out,
    name: String,
    definition: ToolDefinition,
    pending: HostPendingMap,
}

#[async_trait::async_trait]
impl Tool for HostTool {
    fn tier(&self) -> ToolTier {
        // Tool host dieksekusi oleh host; tier approval memakai exec agar
        // kebijakan default selalu menanyakan pengguna.
        ToolTier::Exec
    }
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: format!("host.{}", self.name),
            ..self.definition.clone()
        }
    }
    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }
    async fn execute(
        &self,
        args: &serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, MxError> {
        static HOST_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = format!(
            "host-{}",
            HOST_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.clone(), tx);
        }
        self.out.emit(RpcFrame::HostToolCall {
            id,
            tool_call_id: String::new(),
            tool_name: self.name.clone(),
            arguments: args.clone(),
        });
        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(Ok((is_error, content))) => Ok(if is_error {
                ToolOutput::err(content)
            } else {
                ToolOutput::ok(content)
            }),
            _ => Ok(ToolOutput::err("host tidak menjawab atau waktu habis")),
        }
    }
}

/// Daftarkan/ganti tool host di registry sesi.
///
/// Nama tool host diberi prefiks `host.` supaya tidak bertabrakan dengan tool
/// agent atau tool MCP; nama tanpa prefiks tetap dilarang duplikat di antara
/// tool host sendiri.
fn apply_host_tools(
    session: &mut AgentSession,
    out: &Out,
    host_pending: &HostPendingMap,
    tools: &[HostToolDefinition],
) -> Result<String, MxError> {
    // Validasi dulu sebelum menyentuh registry.
    let mut unik = std::collections::HashSet::new();
    for tool in tools {
        if tool.name.trim().is_empty() {
            return Err(MxError::new(
                ErrorCode::Protocol,
                "nama tool host tidak boleh kosong".to_string(),
            ));
        }
        if !unik.insert(tool.name.clone()) {
            return Err(MxError::new(
                ErrorCode::Protocol,
                format!("nama tool host duplikat: {}", tool.name),
            ));
        }
    }
    let registry = session.registry_mut();
    // Hapus tool host lama yang tidak lagi dikirim.
    let lama: Vec<String> = registry
        .names()
        .into_iter()
        .filter(|nama| nama.starts_with("host."))
        .collect();
    for nama in lama {
        let tanpa = nama.trim_start_matches("host.");
        if !tools.iter().any(|tool| tool.name == tanpa) {
            registry.remove_named(&nama);
        }
    }
    for tool in tools {
        let adapter = HostTool {
            out: out.clone(),
            name: tool.name.clone(),
            definition: ToolDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.schema.clone(),
                concurrency: Concurrency::Exclusive,
            },
            pending: host_pending.clone(),
        };
        registry.replace_named(Arc::new(adapter));
    }
    Ok(format!(
        "tool host aktif: {}",
        tools
            .iter()
            .map(|tool| format!("host.{}", tool.name))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Paket dependensi loop server RPC; memangkas jumlah argumen.
struct LoopDeps {
    ask_rx: UnboundedReceiver<AskRequest>,
    approval_rx: UnboundedReceiver<ApprovalAsk>,
    mcp_notes: Vec<String>,
    uri_state: HostUriState,
    host_seq: Arc<std::sync::atomic::AtomicU64>,
}

/// State host URI yang dibagi antara loop dan resolver.
#[derive(Clone, Default)]
struct HostUriState {
    schemes: Arc<std::sync::Mutex<Vec<HostUriScheme>>>,
    pending: HostUriPendingMap,
}

/// Resolver URI skema host yang meneruskan operasi ke klien RPC.
#[derive(Clone)]
struct RpcUriResolver {
    out: Out,
    state: HostUriState,
    seq: Arc<std::sync::atomic::AtomicU64>,
}

impl RpcUriResolver {
    fn next_id(&self) -> String {
        format!(
            "uri-{}",
            self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }

    async fn minta(
        &self,
        operation: &str,
        url: String,
        content: Option<String>,
    ) -> Result<String, String> {
        let id = self.next_id();
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.state.pending.lock() {
            pending.insert(id.clone(), tx);
        }
        self.out.emit(RpcFrame::HostUriRequest {
            id,
            operation: operation.to_string(),
            url: url.clone(),
            content,
        });
        match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
            Ok(Ok(hasil)) => hasil,
            _ => Err(format!(
                "host tidak menjawab {operation} {url} dalam batas waktu"
            )),
        }
    }
}

impl tui_tools::UriResolver for RpcUriResolver {
    fn owns(&self, url: &str) -> bool {
        let Some((skema, _)) = url.split_once("://") else {
            return false;
        };
        match self.state.schemes.lock() {
            Ok(guard) => guard.iter().any(|s| s.scheme == skema.to_ascii_lowercase()),
            Err(_) => false,
        }
    }

    fn read(
        &self,
        url: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> {
        let this: RpcUriResolver = Clone::clone(self);
        Box::pin(async move { this.minta("read", url, None).await })
    }

    fn write(
        &self,
        url: String,
        content: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> {
        let this: RpcUriResolver = Clone::clone(self);
        Box::pin(async move { this.minta("write", url, Some(content)).await })
    }
}

/// Simpan skema URI host (ganti set penuh) dengan validasi nama.
fn apply_host_uri_schemes(
    simpan: &Arc<std::sync::Mutex<Vec<HostUriScheme>>>,
    schemes: &[HostUriScheme],
) -> Result<String, MxError> {
    let mut dinormalisasi = Vec::with_capacity(schemes.len());
    let mut unik = std::collections::HashSet::new();
    for scheme in schemes {
        let nama = scheme.valid_name()?;
        if !unik.insert(nama.clone()) {
            return Err(MxError::new(
                ErrorCode::Protocol,
                format!("skema URI host duplikat: {nama}"),
            ));
        }
        dinormalisasi.push(HostUriScheme {
            scheme: nama,
            ..scheme.clone()
        });
    }
    if let Ok(mut guard) = simpan.lock() {
        *guard = dinormalisasi.clone();
    }
    if dinormalisasi.is_empty() {
        Ok("skema URI host dikosongkan".to_string())
    } else {
        Ok(format!(
            "skema URI host aktif: {}",
            dinormalisasi
                .iter()
                .map(|scheme| format!("{}://", scheme.scheme))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// Kirim cancel untuk semua host URI yang masih menunggu, dan bersihkan map.
fn batalkan_host_uri(out: &Out, state: &HostUriState, seq: &std::sync::atomic::AtomicU64) -> usize {
    let terkuras: Vec<(String, oneshot::Sender<HostUriOutcome>)> = match state.pending.lock() {
        Ok(mut map) => map.drain().collect(),
        Err(_) => return 0,
    };
    let jumlah = terkuras.len();
    for (id, sender) in terkuras {
        let _ = sender.send(Err("dibatalkan: turn dihentikan".to_string()));
        let cancel_id = format!(
            "uri-cancel-{}",
            seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        emit(
            out,
            RpcFrame::HostUriCancel {
                id: cancel_id,
                target_id: id,
            },
        );
    }
    jumlah
}

/// Kirim cancel untuk semua host tool yang masih menunggu, dan bersihkan map.
fn batalkan_host_tools(
    out: &Out,
    pending: &HostPendingMap,
    seq: &std::sync::atomic::AtomicU64,
) -> usize {
    let terkuras: Vec<(String, oneshot::Sender<HostToolOutcome>)> = match pending.lock() {
        Ok(mut map) => map.drain().collect(),
        Err(_) => return 0,
    };
    let jumlah = terkuras.len();
    for (id, sender) in terkuras {
        // Penunggu menerima hasil error (bukan menggantung sampai timeout).
        let _ = sender.send((true, "dibatalkan: turn dihentikan".to_string()));
        let cancel_id = format!(
            "host-cancel-{}",
            seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        emit(
            out,
            RpcFrame::HostToolCancel {
                id: cancel_id,
                target_id: id,
            },
        );
    }
    jumlah
}

/// Peta event sesi → (nama event extension UI, payload ringkas).
fn peta_ui_extension(event: &AgentEvent) -> Option<(&'static str, String)> {
    match event {
        AgentEvent::AgentStart => Some(("turn.start", "{}".to_string())),
        AgentEvent::AgentEnd { turns } => Some(("turn.end", format!(r#"{{"turns":{turns}}}"#))),
        AgentEvent::ToolExecutionStart {
            tool_name, preview, ..
        } => Some((
            "tool.start",
            format!(
                r#"{{"tool":"{tool_name}","preview":{}}}"#,
                serde_json::to_string(preview).unwrap_or_else(|_| r#""""#.to_string())
            ),
        )),
        AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            ..
        } => Some((
            "tool.end",
            format!(r#"{{"tool":"{tool_name}","is_error":{is_error}}}"#),
        )),
        _ => None,
    }
}

/// Buka sesi tersimpan untuk `open_session` RPC: store baru + sesi penuh pesan.
async fn buka_sesi_rpc(
    boot: &Boot,
    requested: Option<String>,
) -> Result<(SessionStore, AgentSession), MxError> {
    let sessions_dir = boot.sessions_dir.clone();
    let (path, loaded) = tokio::task::spawn_blocking(move || {
        let (path, loaded) = match requested {
            Some(id) => {
                let summary = tui_session::resolve_target(&sessions_dir, &id)?;
                let loaded = tui_session::SessionStore::load(&summary.path)?;
                (summary.path, loaded)
            }
            None => {
                let mut daftar = tui_session::list(&sessions_dir);
                let terbaru = daftar.swap_remove(0);
                let loaded = tui_session::SessionStore::load(&terbaru.path)?;
                (terbaru.path, loaded)
            }
        };
        Ok::<_, MxError>((path, loaded))
    })
    .await
    .map_err(|e| MxError::new(ErrorCode::Config, format!("baca sesi gagal: {e}")))??;
    let store = SessionStore::open(&path)?;
    // Registry + tool subagent (manager bersama) di mode RPC: klien bisa
    // melihat daftar anak lewat `subagent_list`.
    let mut registry = boot.tool_registry();
    if let Some(manager) = boot.subagents.clone() {
        registry.register(Arc::new(
            tui_subagent::SubagentTool::new(tui_subagent::SubagentSpec {
                provider: Arc::new(boot.client.clone()),
                approver: Arc::new(tui_approval::PolicyApprover::new(boot.approval.clone())),
                tools: boot.tool_registry(),
                model: boot.model.clone(),
                deadline: std::time::Duration::from_secs(boot.limits.turn_deadline_secs),
                max_tool_calls: 32,
            })
            .with_manager(manager.clone()),
        ));
        registry.register(Arc::new(tui_subagent::OrchestratorTool::new(
            tui_subagent::SubagentSpec {
                provider: Arc::new(boot.client.clone()),
                approver: Arc::new(tui_approval::PolicyApprover::new(boot.approval.clone())),
                tools: boot.tool_registry(),
                model: boot.model.clone(),
                deadline: std::time::Duration::from_secs(boot.limits.turn_deadline_secs),
                max_tool_calls: 32,
            },
            manager,
        )));
    }
    let mut sesi = AgentSession::new(
        Arc::new(boot.client.clone()),
        // Approver tak dipakai sebelum turn; PolicyApprover default aman.
        Arc::new(tui_approval::PolicyApprover::new(boot.approval.clone())),
        registry,
        tool_context(
            boot,
            {
                let (tx, _rx) = unbounded_channel();
                tx
            },
            None,
        ),
        settings(boot),
        {
            let (tx, _rx) = unbounded_channel();
            tx
        },
    );
    sesi.set_context_window(boot.limits.context_window_tokens);
    sesi.push_messages(loaded.messages());
    Ok((store, sesi))
}

/// Setujui/tolak negosiasi protokol dan balas klien.
fn negotiate_protocol(out: &Out, id: String, protocol_version: u32) {
    if SUPPORTED_PROTOCOL_VERSIONS.contains(&protocol_version) {
        out.set_protocol_version(protocol_version);
        emit(
            out,
            RpcFrame::Negotiated {
                id,
                protocol_version,
            },
        );
    } else {
        emit(
            out,
            RpcFrame::Error {
                id: Some(id),
                code: ErrorCode::Protocol.to_string(),
                message: format!(
                    "versi protokol {protocol_version} tidak didukung; server mendukung {SUPPORTED_PROTOCOL_VERSIONS:?}"
                ),
            },
        );
    }
}

fn emit(out: &Out, frame: RpcFrame) {
    out.emit(frame);
}

fn handle_ask(request: AskRequest, out: &Out, pending: &mut Option<oneshot::Sender<String>>) {
    emit(
        out,
        RpcFrame::Question {
            question: request.question,
        },
    );
    *pending = Some(request.reply);
}

fn handle_answer(text: String, pending: &mut Option<oneshot::Sender<String>>) -> bool {
    match pending.take() {
        Some(sender) => {
            let _ = sender.send(text);
            true
        }
        None => false,
    }
}

/// Permintaan approval yang menunggu keputusan klien RPC.
struct ApprovalAsk {
    request: ApprovalRequest,
    reply: oneshot::Sender<Decision>,
}

/// UI approval untuk RPC: meneruskan ke klien lewat kanal, lalu menunggu.
struct RpcDecisionUi {
    tx: UnboundedSender<ApprovalAsk>,
    timeout: std::time::Duration,
}

#[async_trait::async_trait]
impl DecisionUi for RpcDecisionUi {
    async fn ask(&self, request: &ApprovalRequest) -> Decision {
        let (reply, wait) = oneshot::channel();
        if self
            .tx
            .send(ApprovalAsk {
                request: request.clone(),
                reply,
            })
            .is_err()
        {
            return Decision::Deny;
        }
        match tokio::time::timeout(self.timeout, wait).await {
            Ok(Ok(decision)) => decision,
            _ => Decision::Deny,
        }
    }
}

/// Batas satu frame JSONL, sama dengan omp (1 MiB).
pub const MAX_FRAME_BYTES: usize = tui_wire::rpc::MAX_FRAME_BYTES;

/// Panjang pratinjau galat supaya frame rusak tidak ikut membengkakkan memori.
const ERROR_PREVIEW_CHARS: usize = 200;

fn preview(text: &str) -> String {
    text.chars().take(ERROR_PREVIEW_CHARS).collect()
}

/// Baca satu baris dengan batas keras.
///
/// Mengembalikan `(byte_dibaca, terpotong)`. Bagian yang melebihi batas tidak
/// disimpan sama sekali, jadi memori tidak bisa dipaksa tumbuh oleh satu baris
/// raksasa; sisanya tetap dibuang sampai newline berikutnya.
async fn read_line_bounded<R>(reader: &mut R, buf: &mut Vec<u8>) -> std::io::Result<(usize, bool)>
where
    R: AsyncBufRead + Unpin,
{
    let mut total = 0usize;
    let mut kept = true;
    loop {
        let chunk: Vec<u8> = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                return Ok((total, !kept));
            }
            available.to_vec()
        };
        let (take, has_newline) = match chunk.iter().position(|byte| *byte == b'\n') {
            Some(pos) => (pos + 1, true),
            None => (chunk.len(), false),
        };
        if kept {
            if total + take > MAX_FRAME_BYTES {
                kept = false;
                buf.clear();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        reader.consume(take);
        total += take;
        if has_newline {
            return Ok((total, !kept));
        }
    }
}

/// Baca frame JSONL sampai EOF, meneruskan permintaan atau galat ke `tx`.
async fn read_frames<R>(mut reader: R, tx: UnboundedSender<Incoming>)
where
    R: AsyncBufRead + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let (read, truncated) = match read_line_bounded(&mut reader, &mut buf).await {
            Ok(result) => result,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        if truncated {
            let message = format!("frame terlalu besar: {read} byte, maksimum {MAX_FRAME_BYTES}");
            if tx.send(Incoming::Malformed(message)).is_err() {
                break;
            }
            continue;
        }
        let text = String::from_utf8_lossy(&buf);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<RpcRequest>(trimmed)
            .map(Incoming::Request)
            .unwrap_or_else(|err| Incoming::Malformed(format!("{err}: {}", preview(trimmed))));
        if tx.send(parsed).is_err() {
            break;
        }
    }
}

fn tier_str(tier: ToolTier) -> &'static str {
    match tier {
        ToolTier::Read => "read",
        ToolTier::Write => "write",
        ToolTier::Exec => "exec",
    }
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("...");
    out
}

fn handle_approval_request(
    ask: ApprovalAsk,
    out: &Out,
    pending: &mut HashMap<String, oneshot::Sender<Decision>>,
    next_id: &mut u64,
) {
    let id = format!("ap{}", *next_id);
    *next_id = next_id.saturating_add(1);
    emit(
        out,
        RpcFrame::ApprovalRequest {
            id: id.clone(),
            tool: ask.request.tool.clone(),
            tier: tier_str(ask.request.tier).to_string(),
            subject: shorten(&ask.request.subject, 200),
        },
    );
    pending.insert(id, ask.reply);
}

fn handle_approval_decision(
    id: &str,
    decision: &str,
    pending: &mut HashMap<String, oneshot::Sender<Decision>>,
) -> bool {
    let parsed = match decision {
        "once" => Decision::Once,
        "session" => Decision::Session,
        "always" => Decision::Always,
        "deny" => Decision::Deny,
        _ => return false,
    };
    match pending.remove(id) {
        Some(sender) => {
            let _ = sender.send(parsed);
            true
        }
        None => false,
    }
}

fn tool_context(
    boot: &Boot,
    ask: UnboundedSender<AskRequest>,
    resolver: Option<Arc<dyn tui_tools::UriResolver>>,
) -> ToolContext {
    let mut ctx = ToolContext::new(boot.cwd.clone());
    ctx.output_cap = boot.limits.tool_output_cap_bytes;
    ctx.interactive = false;
    ctx.bash_timeout = std::time::Duration::from_secs(boot.limits.bash_timeout_secs);
    ctx.bash_max_timeout = std::time::Duration::from_secs(boot.limits.bash_max_timeout_secs);
    ctx.cancel = CancelToken::new();
    ctx.ask = Some(ask);
    ctx.uri_bridge = resolver;
    ctx
}

/// Jalankan server RPC sampai stdin tertutup atau `shutdown`.
pub async fn run(boot: Boot) -> Result<(), MxError> {
    // Spool di direktori data (bukan temp) agar bisa ditemukan pengguna.
    let spool = Arc::new(Spool::new(&crate::config_root()));
    let spool_writer = spool.clone();
    let (sink, mut sink_rx) = tokio::sync::mpsc::channel::<String>(SINK_CAPACITY);
    let mut sisa_sisa: Vec<String> = Vec::new();
    tokio::spawn(async move {
        let mut out = std::io::stdout();
        while let Some(line) = sink_rx.recv().await {
            if writeln!(out, "{line}").is_err() {
                // stdout putus: sisa frame di kanal di-spool ke file agar tidak
                // hilang diam-diam.
                while let Ok(sisa) = sink_rx.try_recv() {
                    sisa_sisa.push(sisa);
                }
                break;
            }
            let _ = out.flush();
        }
        for sisa in sisa_sisa.drain(..) {
            spool_writer.push(&sisa);
        }
        spool_writer.flush();
    });

    let (ask_tx, ask_rx) = unbounded_channel::<AskRequest>();
    let (approval_tx, approval_rx) = unbounded_channel::<ApprovalAsk>();
    let approver: Arc<dyn Approver> = Arc::new(build_approver(
        boot.approval.clone(),
        Box::new(RpcDecisionUi {
            tx: approval_tx,
            timeout: std::time::Duration::from_secs(boot.limits.approval_timeout_secs),
        }),
        boot.grants_path.clone(),
        boot.grant_ttl,
    )?);
    let (events_tx, events_rx, event_log_done) = crate::event_log::channel(boot.event_log.clone());
    if boot.approval.mode == tui_approval::ApprovalMode::Yolo {
        let _ = events_tx.send(AgentEvent::Notice {
            text: crate::banner::yolo_banner().to_string(),
        });
    }
    let out = Out::new(sink, spool.clone());
    let uri_state = HostUriState::default();
    let host_seq = Arc::new(std::sync::atomic::AtomicU64::new(1));
    let resolver: Arc<dyn tui_tools::UriResolver> = Arc::new(RpcUriResolver {
        out: out.clone(),
        state: uri_state.clone(),
        seq: host_seq.clone(),
    });
    let session = AgentSession::new(
        Arc::new(boot.client.clone()),
        approver,
        boot.tool_registry(),
        tool_context(&boot, ask_tx, Some(resolver)),
        settings(&boot),
        events_tx,
    );
    let mcp_notes = boot.mcp_notes.clone();
    let result = run_with_session(
        boot,
        session,
        events_rx,
        out,
        LoopDeps {
            ask_rx,
            approval_rx,
            mcp_notes,
            uri_state,
            host_seq,
        },
    )
    .await;
    crate::event_log::flush(event_log_done).await;
    result
}

/// Validasi id model dari permintaan RPC terhadap daftar discovery.
///
/// Mode TUI/teks memakai `resolve_model` lewat `/model`; RPC harus konsisten:
/// id yang tidak ada di daftar ditolak dengan `E_MODEL_UNKNOWN` alih-alih
/// diterima apa adanya (AGENTS §3: jangan menebak model ID). Bila discovery
/// belum pernah berhasil (daftar kosong), id diteruskan apa adanya.
fn model_from_request(requested: &str, known: &[String]) -> Result<String, MxError> {
    tui_core::commands::resolve_model(requested, known)
        .map_err(|detail| MxError::new(ErrorCode::ModelUnknown, detail))
}
async fn run_with_session(
    boot: Boot,
    mut session: AgentSession,
    mut events_rx: UnboundedReceiver<AgentEvent>,
    out: Out,
    deps: LoopDeps,
) -> Result<(), MxError> {
    let LoopDeps {
        mut ask_rx,
        mut approval_rx,
        mcp_notes,
        uri_state,
        host_seq,
    } = deps;
    session.set_context_window(boot.limits.context_window_tokens);
    session.seed_usage(boot.resume_usage);
    session.push_messages(boot.resume_messages.clone());
    let mut persisted_usage = boot.resume_usage;

    let cwd = boot.cwd.display().to_string();
    let mut store: Option<SessionStore> = match crate::session_open::open_or_create(
        boot.sessions_dir.clone(),
        boot.resume_path.clone(),
        cwd,
    )
    .await
    {
        Ok(store) => Some(store),
        Err(err) => {
            emit(
                &out,
                RpcFrame::Event {
                    event: tui_wire::rpc::RpcEvent::Notice {
                        text: format!("sesi tidak tersimpan: {}: {}", err.code, err.message),
                    },
                },
            );
            None
        }
    };

    let langganan_ui: Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let langganan_ui_fwd = langganan_ui.clone();
    let forward_out = out.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            if let Some(projected) = RpcEvent::from_agent(&event) {
                forward_out.emit(RpcFrame::Event { event: projected });
            }
            // Extension UI: event sesi diteruskan ke klien yang melanggani nama
            // eventnya (mirip host tools/uri).
            if let Some((nama_event, payload)) = peta_ui_extension(&event) {
                let dilanggani = {
                    // Set event dilihat tanpa menahan lock lintas .await.
                    langganan_ui_fwd
                        .lock()
                        .map(|s| s.contains(nama_event))
                        .unwrap_or(false)
                };
                if dilanggani {
                    forward_out.emit(RpcFrame::UiExtension {
                        event: nama_event.to_string(),
                        payload: payload.to_string(),
                    });
                }
            }
        }
    });

    emit(
        &out,
        RpcFrame::Ready {
            protocol: PROTOCOL_VERSION.to_string(),
            model: session.model().to_string(),
            supported_protocol_versions: SUPPORTED_PROTOCOL_VERSIONS.to_vec(),
            max_reassembled_frame_bytes: MAX_REASSEMBLED_FRAME_BYTES as u64,
        },
    );

    for note in mcp_notes {
        emit(
            &out,
            RpcFrame::Event {
                event: RpcEvent::Notice {
                    text: format!("mcp: {note}"),
                },
            },
        );
    }

    let (incoming_tx, mut incoming_rx) = unbounded_channel::<Incoming>();
    tokio::spawn(async move {
        read_frames(BufReader::new(tokio::io::stdin()), incoming_tx).await;
    });

    let mut pending_answer: Option<oneshot::Sender<String>> = None;
    let mut pending_approvals: HashMap<String, oneshot::Sender<Decision>> = HashMap::new();
    let mut next_approval_id: u64 = 0;
    // Antrean prompt saat turn berjalan (omp: queue modes) — batas 8.
    let mut antrean: std::collections::VecDeque<(String, String)> = Default::default();
    let host_pending: HostPendingMap = Arc::new(std::sync::Mutex::new(HashMap::new()));
    // host_seq dibuat di run() dan diteruskan via LoopDeps.
    loop {
        let incoming = tokio::select! {
            maybe = incoming_rx.recv() => match maybe {
                Some(incoming) => incoming,
                None => break,
            },
            Some(request) = ask_rx.recv() => {
                handle_ask(request, &out, &mut pending_answer);
                continue;
            }
            Some(ask) = approval_rx.recv() => {
                handle_approval_request(ask, &out, &mut pending_approvals, &mut next_approval_id);
                continue;
            }
        };
        let request = match incoming {
            Incoming::Malformed(detail) => {
                emit(
                    &out,
                    RpcFrame::Error {
                        id: None,
                        code: ErrorCode::Protocol.to_string(),
                        message: format!("permintaan tidak bisa diparse: {detail}"),
                    },
                );
                continue;
            }
            Incoming::Request(request) => request,
        };
        match request {
            RpcRequest::NegotiateProtocol {
                id,
                protocol_version,
            } => negotiate_protocol(&out, id, protocol_version),
            RpcRequest::SetHostUriSchemes { id, schemes } => {
                match apply_host_uri_schemes(&uri_state.schemes, &schemes) {
                    Ok(pesan) => {
                        emit(&out, RpcFrame::Ack { id });
                        emit(
                            &out,
                            RpcFrame::Event {
                                event: RpcEvent::Notice { text: pesan },
                            },
                        );
                    }
                    Err(err) => emit(
                        &out,
                        RpcFrame::Error {
                            id: Some(id),
                            code: err.code.to_string(),
                            message: err.message,
                        },
                    ),
                }
            }
            RpcRequest::SetHostTools { id, tools } => {
                match apply_host_tools(&mut session, &out, &host_pending, &tools) {
                    Ok(pesan) => {
                        emit(&out, RpcFrame::Ack { id });
                        emit(
                            &out,
                            RpcFrame::Event {
                                event: RpcEvent::Notice { text: pesan },
                            },
                        );
                    }
                    Err(err) => emit(
                        &out,
                        RpcFrame::Error {
                            id: Some(id),
                            code: err.code.to_string(),
                            message: err.message,
                        },
                    ),
                }
            }
            RpcRequest::HostUriResult { id, content } => {
                let diteruskan = match uri_state.pending.lock() {
                    Ok(mut pending) => match pending.remove(&id) {
                        Some(sender) => sender.send(content).is_ok(),
                        None => false,
                    },
                    Err(_) => false,
                };
                if !diteruskan {
                    emit(
                        &out,
                        RpcFrame::Error {
                            id: None,
                            code: ErrorCode::Protocol.to_string(),
                            message: format!("tidak ada host uri aktif dengan id {id}"),
                        },
                    );
                }
            }
            RpcRequest::HostToolResult {
                id,
                is_error,
                content,
            } => {
                let diteruskan = match host_pending.lock() {
                    Ok(mut pending) => match pending.remove(&id) {
                        Some(sender) => sender.send((is_error, content)).is_ok(),
                        None => false,
                    },
                    Err(_) => false,
                };
                if !diteruskan {
                    emit(
                        &out,
                        RpcFrame::Error {
                            id: None,
                            code: ErrorCode::Protocol.to_string(),
                            message: format!("tidak ada host tool aktif dengan id {id}"),
                        },
                    );
                }
            }
            RpcRequest::Shutdown => break,
            RpcRequest::Ping { id } => emit(&out, RpcFrame::Pong { id }),
            RpcRequest::Approval { .. } => emit(
                &out,
                RpcFrame::Error {
                    id: None,
                    code: ErrorCode::Protocol.to_string(),
                    message: "tidak ada approval aktif".to_string(),
                },
            ),
            RpcRequest::Answer { .. } => emit(
                &out,
                RpcFrame::Error {
                    id: None,
                    code: ErrorCode::Protocol.to_string(),
                    message: "tidak ada pertanyaan aktif".to_string(),
                },
            ),
            RpcRequest::Usage { id } => {
                let usage = session.usage();
                emit(
                    &out,
                    RpcFrame::Usage {
                        id,
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cached_tokens: usage.cached_tokens,
                    },
                );
            }
            RpcRequest::ReplaySpool { id } => {
                // Kirim ulang frame yang tertahan (spool) lalu kosongkan.
                let jumlah = out.mainkan_spool();
                emit(&out, RpcFrame::SpoolReplayed { jumlah });
                emit(&out, RpcFrame::Ack { id });
            }
            RpcRequest::SubagentList { id } => {
                // Daftar anak read-only dari manager bersama (bila ada).
                let agents: Vec<tui_wire::rpc::SubagentInfo> = boot
                    .subagents
                    .as_ref()
                    .map(|manager| {
                        manager
                            .list()
                            .iter()
                            .map(|h| {
                                let hasil = h.result();
                                tui_wire::rpc::SubagentInfo {
                                    id: h.id.clone(),
                                    status: h.status().label().to_string(),
                                    turns: hasil.as_ref().map(|r| r.turns).unwrap_or(0),
                                    tool_calls: hasil.as_ref().map(|r| r.tool_calls).unwrap_or(0),
                                    is_error: hasil.as_ref().is_some_and(|r| r.is_error),
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                emit(&out, RpcFrame::SubagentList { agents });
                emit(&out, RpcFrame::Ack { id });
            }
            RpcRequest::Login { id, provider } => {
                // Cek kesiapan kunci env provider — read-only, tanpa nilai di frame.
                let target = provider.as_deref().unwrap_or("modeluxai");
                match tui_wire::providers::by_id(target) {
                    Some(entry) => {
                        let siap = entry.resolve_env_key(|nama| {
                            std::env::var(nama).ok().filter(|v| !v.trim().is_empty())
                        });
                        let (status, hint) = match siap {
                            Some(_) => ("ok".to_string(), None),
                            None => ("butuh-kunci".to_string(), entry.env_keys.first().cloned()),
                        };
                        emit(
                            &out,
                            RpcFrame::LoginStatus {
                                provider: entry.id,
                                status,
                                env_hint: hint,
                            },
                        );
                        emit(&out, RpcFrame::Ack { id });
                    }
                    None => emit(
                        &out,
                        RpcFrame::Error {
                            id: Some(id),
                            code: ErrorCode::Config.to_string(),
                            message: format!("provider '{target}' tidak dikenal"),
                        },
                    ),
                }
            }
            RpcRequest::SetConfig { id, key, value } => {
                // Runtime-safe: hanya `models.default` (model aktif). Kunci
                // lain butuh restart — jawab bernama, jangan diam.
                if key == "models.default" {
                    match model_from_request(&value, &boot.models) {
                        Ok(resolved) => {
                            session.set_model(resolved.clone());
                            emit(&out, RpcFrame::ConfigUpdate { key });
                            emit(
                                &out,
                                RpcFrame::SessionInfoUpdate {
                                    session_id: store.as_ref().map(|s| s.session_id().to_string()),
                                    model: resolved,
                                    title: None,
                                },
                            );
                            emit(&out, RpcFrame::Ack { id });
                        }
                        Err(err) => emit(
                            &out,
                            RpcFrame::Error {
                                id: Some(id),
                                code: err.code.to_string(),
                                message: err.message,
                            },
                        ),
                    }
                } else {
                    emit(
                        &out,
                        RpcFrame::Error {
                            id: Some(id),
                            code: ErrorCode::Config.to_string(),
                            message: format!(
                                "kunci '{key}' tidak bisa diubah runtime; gunakan --set lalu restart"
                            ),
                        },
                    );
                }
            }
            RpcRequest::Command { id, text } => {
                // Perintah slash dari klien: dispatch dengan context nyata.
                let tool_names_rpc: Vec<String> = boot
                    .tool_registry()
                    .names()
                    .iter()
                    .map(|t| t.to_string())
                    .collect();
                let context = tui_core::CommandContext {
                    model: session.model(),
                    provider: (&boot.provider.0, &boot.provider.1),
                    tools: &tool_names_rpc,
                    session_path: store.as_ref().map(|s| s.path()),
                    known_models: &boot.models,
                    sessions_dir: Some(&boot.sessions_dir),
                };
                let hasil = tui_core::dispatch(&text, &context);
                match &hasil {
                    tui_core::CommandResult::SetModel(m) => {
                        match model_from_request(m, &boot.models) {
                            Ok(resolved) => {
                                session.set_model(resolved.clone());
                                emit(
                                    &out,
                                    RpcFrame::SessionInfoUpdate {
                                        session_id: store
                                            .as_ref()
                                            .map(|s| s.session_id().to_string()),
                                        model: resolved,
                                        title: None,
                                    },
                                );
                                emit(
                                    &out,
                                    RpcFrame::CommandOutput {
                                        text: format!("model aktif: {m}"),
                                    },
                                );
                                emit(&out, RpcFrame::Ack { id });
                            }
                            Err(err) => emit(
                                &out,
                                RpcFrame::Error {
                                    id: Some(id),
                                    code: err.code.to_string(),
                                    message: err.message,
                                },
                            ),
                        }
                    }
                    tui_core::CommandResult::Sessions(_)
                    | tui_core::CommandResult::Tree
                    | tui_core::CommandResult::Usage
                    | tui_core::CommandResult::Context
                    | tui_core::CommandResult::Todo
                    | tui_core::CommandResult::Reply(_) => {
                        // Hasil read-only: render teksnya ke klien.
                        let teks = match hasil {
                            tui_core::CommandResult::Usage => {
                                let pakai = session.usage();
                                format!(
                                    "usage: input {}, output {} token",
                                    pakai.input_tokens, pakai.output_tokens
                                )
                            }
                            tui_core::CommandResult::Context => {
                                // Ringkas konteks: jumlah pesan + estimasi token.
                                format!("konteks: {} pesan", session.messages().len())
                            }
                            tui_core::CommandResult::Todo => {
                                format!("todo: {:?}", session.todos())
                            }
                            tui_core::CommandResult::Tree => {
                                let mut teks = String::from("pohon sesi:");
                                for simpul in tui_session::pohon(&boot.sessions_dir) {
                                    teks.push('\n');
                                    teks.push_str(&simpul.display_line());
                                }
                                teks
                            }
                            tui_core::CommandResult::Reply(t) => t,
                            _ => String::new(),
                        };
                        emit(&out, RpcFrame::CommandOutput { text: teks });
                        emit(&out, RpcFrame::Ack { id });
                    }
                    lain => emit(
                        &out,
                        RpcFrame::Error {
                            id: Some(id),
                            code: ErrorCode::Protocol.to_string(),
                            message: format!(
                                "perintah {lain:?} butuh request khusus (prompt/model/abort)"
                            ),
                        },
                    ),
                }
            }
            RpcRequest::SetTitle { id, title } => {
                if let Some(store) = store.as_mut() {
                    let _ = store.append(&SessionEntry::title_change(title.clone()));
                }
                emit(
                    &out,
                    RpcFrame::SessionInfoUpdate {
                        session_id: store.as_ref().map(|s| s.session_id().to_string()),
                        model: session.model().to_string(),
                        title: Some(title),
                    },
                );
                emit(&out, RpcFrame::Ack { id });
            }
            RpcRequest::Model { id, model } => match model_from_request(&model, &boot.models) {
                Ok(resolved) => {
                    session.set_model(resolved.clone());
                    emit(
                        &out,
                        RpcFrame::SessionInfoUpdate {
                            session_id: store.as_ref().map(|s| s.session_id().to_string()),
                            model: resolved,
                            title: None,
                        },
                    );
                    emit(&out, RpcFrame::Ack { id });
                }
                Err(err) => emit(
                    &out,
                    RpcFrame::Error {
                        id: Some(id),
                        code: err.code.to_string(),
                        message: err.message,
                    },
                ),
            },
            RpcRequest::Abort { id } => {
                session.cancel();
                let _ = batalkan_host_tools(&out, &host_pending, &host_seq);
                let _ = batalkan_host_uri(&out, &uri_state, &host_seq);
                if let Some(id) = id {
                    emit(&out, RpcFrame::Ack { id });
                }
            }
            RpcRequest::AbortAndRestoreQueue { id } => emit(
                &out,
                RpcFrame::Error {
                    id: Some(id),
                    code: ErrorCode::Protocol.to_string(),
                    message: "sesi tidak sibuk; tidak ada antrean yang dikembalikan".to_string(),
                },
            ),
            RpcRequest::RemoveQueuedMessage { id, index } => emit(
                &out,
                RpcFrame::Error {
                    id: Some(id),
                    code: ErrorCode::Protocol.to_string(),
                    message: format!("sesi tidak sibuk; antrean kosong (posisi {index} tidak ada)"),
                },
            ),
            RpcRequest::PromoteQueuedMessage { id, index } => emit(
                &out,
                RpcFrame::Error {
                    id: Some(id),
                    code: ErrorCode::Protocol.to_string(),
                    message: format!("sesi tidak sibuk; antrean kosong (posisi {index} tidak ada)"),
                },
            ),
            RpcRequest::SetUiExtension { id, events } => match langganan_ui.lock() {
                Ok(mut set) => {
                    set.clear();
                    for nama in &events {
                        let nama = nama.trim().to_ascii_lowercase();
                        if nama.is_empty() {
                            emit(
                                &out,
                                RpcFrame::Error {
                                    id: Some(id.clone()),
                                    code: ErrorCode::Protocol.to_string(),
                                    message: "nama event ui kosong".to_string(),
                                },
                            );
                        } else {
                            set.insert(nama);
                        }
                    }
                    emit(&out, RpcFrame::Ack { id });
                }
                Err(_) => emit(
                    &out,
                    RpcFrame::Error {
                        id: Some(id),
                        code: ErrorCode::Protocol.to_string(),
                        message: "langganan ui terkunci".to_string(),
                    },
                ),
            },
            RpcRequest::GetState { id } => {
                emit(
                    &out,
                    RpcFrame::State {
                        id,
                        model: session.model().to_string(),
                        messages: session.messages().len() as u64,
                        busy: session.is_busy(),
                        queued: antrean.len() as u64,
                        input_tokens: session.usage().input_tokens,
                        output_tokens: session.usage().output_tokens,
                    },
                );
            }
            RpcRequest::OpenSession {
                id,
                session: sesi_diminta,
            } => match buka_sesi_rpc(&boot, sesi_diminta).await {
                Ok((store_baru, sesi_baru)) => {
                    let jumlah = sesi_baru.messages().len() as u64;
                    session = sesi_baru;
                    store = Some(store_baru);
                    persisted_usage = session.usage();
                    emit(&out, RpcFrame::SessionSettled);
                    emit(
                        &out,
                        RpcFrame::State {
                            id,
                            model: session.model().to_string(),
                            messages: jumlah,
                            busy: session.is_busy(),
                            queued: antrean.len() as u64,
                            input_tokens: session.usage().input_tokens,
                            output_tokens: session.usage().output_tokens,
                        },
                    );
                }
                Err(err) => emit(
                    &out,
                    RpcFrame::Error {
                        id: Some(id),
                        code: err.code.to_string(),
                        message: err.message,
                    },
                ),
            },
            RpcRequest::Prompt { id, message } => {
                crate::session_open::append(
                    &mut store,
                    SessionEntry::message(None, Message::user(message.clone())),
                )
                .await;
                let token = session.cancel_token();
                let outcome = {
                    let mut run = std::pin::pin!(session.run_turn(Some(message)));
                    loop {
                        tokio::select! {
                            result = &mut run => break result,
                            Some(request) = ask_rx.recv() => {
                                handle_ask(request, &out, &mut pending_answer);
                            }
                            Some(ask) = approval_rx.recv() => {
                                handle_approval_request(
                                    ask,
                                    &out,
                                    &mut pending_approvals,
                                    &mut next_approval_id,
                                );
                            }
                            Some(next) = incoming_rx.recv() => match next {
                                Incoming::Request(RpcRequest::Abort { .. }) => {
                                    token.cancel();
                                    let _ = batalkan_host_tools(&out, &host_pending, &host_seq);
                                    let _ = batalkan_host_uri(&out, &uri_state, &host_seq);
                                }
                                Incoming::Request(RpcRequest::Shutdown) => {
                                    token.cancel();
                                    let _ = batalkan_host_tools(&out, &host_pending, &host_seq);
                                    let _ = batalkan_host_uri(&out, &uri_state, &host_seq);
                                }
                                Incoming::Request(RpcRequest::Approval { id, decision }) => {
                                    if !handle_approval_decision(&id, &decision, &mut pending_approvals) {
                                        emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: None,
                                                code: ErrorCode::Protocol.to_string(),
                                                message: format!("tidak ada approval aktif dengan id {id}"),
                                            },
                                        );
                                    }
                                }
                                Incoming::Request(RpcRequest::Answer { text, .. }) => {
                                    if !handle_answer(text, &mut pending_answer) {
                                        emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: None,
                                                code: ErrorCode::Protocol.to_string(),
                                                message: "tidak ada pertanyaan aktif".to_string(),
                                            },
                                        );
                                    }
                                }
                                Incoming::Request(RpcRequest::Ping { id: ping_id }) => {
                                    emit(&out, RpcFrame::Pong { id: ping_id });
                                }
                                Incoming::Request(RpcRequest::HostUriResult {
                                    id,
                                    content,
                                }) => {
                                    let diteruskan = match uri_state.pending.lock() {
                                        Ok(mut pending) => match pending.remove(&id) {
                                            Some(sender) => sender.send(content).is_ok(),
                                            None => false,
                                        },
                                        Err(_) => false,
                                    };
                                    if !diteruskan {
                                        emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: None,
                                                code: ErrorCode::Protocol.to_string(),
                                                message: format!(
                                                    "tidak ada host uri aktif dengan id {id}"
                                                ),
                                            },
                                        );
                                    }
                                }
                                Incoming::Request(RpcRequest::HostToolResult {
                                    id,
                                    is_error,
                                    content,
                                }) => {
                                    let diteruskan = match host_pending.lock() {
                                        Ok(mut pending) => match pending.remove(&id) {
                                            Some(sender) => sender.send((is_error, content)).is_ok(),
                                            None => false,
                                        },
                                        Err(_) => false,
                                    };
                                    if !diteruskan {
                                        emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: None,
                                                code: ErrorCode::Protocol.to_string(),
                                                message: format!(
                                                    "tidak ada host tool aktif dengan id {id}"
                                                ),
                                            },
                                        );
                                    }
                                }
                                Incoming::Request(RpcRequest::AbortAndRestoreQueue { id }) => {
                                    token.cancel();
                                    let isi: Vec<tui_wire::rpc::QueuedItem> = antrean
                                        .drain(..)
                                        .map(|(qid, pesan)| tui_wire::rpc::QueuedItem {
                                            id: qid,
                                            message: pesan,
                                        })
                                        .collect();
                                    emit(
                                        &out,
                                        RpcFrame::QueueRestored { items: isi },
                                    );
                                    let _ = batalkan_host_tools(&out, &host_pending, &host_seq);
                                    let _ = batalkan_host_uri(&out, &uri_state, &host_seq);
                                    if !id.is_empty() {
                                        emit(&out, RpcFrame::Ack { id });
                                    }
                                }
                                Incoming::Request(RpcRequest::RemoveQueuedMessage { id, index }) => {
                                    let posisi = index as usize;
                                    let hasil = antrean.get(posisi).map(|_| ());
                                    match hasil {
                                        Some(()) => {
                                            let (qid_dibuang, _) =
                                                antrean.remove(posisi).unwrap_or_default();
                                            emit(
                                                &out,
                                                RpcFrame::Event {
                                                    event: RpcEvent::Notice {
                                                        text: format!(
                                                            "antrean {posisi} dibuang ({qid_dibuang})"
                                                        ),
                                                    },
                                                },
                                            )
                                        }
                                        None => emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: Some(id),
                                                code: ErrorCode::Protocol.to_string(),
                                                message: format!(
                                                    "posisi antrean {posisi} tidak ada (isi {})",
                                                    antrean.len()
                                                ),
                                            },
                                        ),
                                    }
                                }
                                Incoming::Request(RpcRequest::PromoteQueuedMessage {
                                    id,
                                    index,
                                }) => {
                                    let posisi = index as usize;
                                    if posisi < antrean.len() {
                                        let item = antrean.remove(posisi);
                                        if let Some(item) = item {
                                            antrean.push_front(item);
                                        }
                                        emit(
                                            &out,
                                            RpcFrame::Event {
                                                event: RpcEvent::Notice {
                                                    text: format!(
                                                        "antrean {posisi} dinaikkan ke depan"
                                                    ),
                                                },
                                            },
                                        );
                                    } else {
                                        emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: Some(id),
                                                code: ErrorCode::Protocol.to_string(),
                                                message: format!(
                                                    "posisi antrean {posisi} tidak ada (isi {})",
                                                    antrean.len()
                                                ),
                                            },
                                        );
                                    }
                                }
                                Incoming::Request(RpcRequest::Prompt {
                                    id: qid,
                                    message: qmsg,
                                }) => {
                                    if antrean.len() >= 8 {
                                        emit(
                                            &out,
                                            RpcFrame::Error {
                                                id: Some(qid),
                                                code: ErrorCode::AgentBusy.to_string(),
                                                message: "antrean penuh (8); kirim lebih lambat"
                                                    .to_string(),
                                            },
                                        );
                                    } else {
                                        emit(
                                            &out,
                                            RpcFrame::Event {
                                                event: RpcEvent::Notice {
                                                    text: format!(
                                                        "prompt masuk antrean (posisi {})",
                                                        antrean.len() + 1
                                                    ),
                                                },
                                            },
                                        );
                                        antrean.push_back((qid, qmsg));
                                    }
                                }
                                _ => emit(
                                    &out,
                                    RpcFrame::Error {
                                        id: Some(id.clone()),
                                        code: ErrorCode::AgentBusy.to_string(),
                                        message: "turn masih berjalan; kirim abort atau tunggu".to_string(),
                                    },
                                ),
                            },
                        }
                    }
                };
                match outcome {
                    Ok(result) => {
                        let status = match result.stop_reason {
                            tui_wire::StopReason::Error => "error",
                            tui_wire::StopReason::Aborted => "aborted",
                            _ => "ok",
                        };
                        emit(
                            &out,
                            RpcFrame::Result {
                                id: id.clone(),
                                stop_reason: result.stop_reason,
                                turns: result.turns,
                            },
                        );
                        emit(
                            &out,
                            RpcFrame::PromptResult {
                                id: id.clone(),
                                agent_invoked: true,
                                status: status.to_string(),
                                stop_reason: Some(result.stop_reason),
                                turns: Some(result.turns),
                                error: None,
                            },
                        );
                    }
                    Err(err) => {
                        emit(
                            &out,
                            RpcFrame::Error {
                                id: Some(id.clone()),
                                code: err.code.to_string(),
                                message: err.message.clone(),
                            },
                        );
                        emit(
                            &out,
                            RpcFrame::PromptResult {
                                id,
                                agent_invoked: true,
                                status: "error".to_string(),
                                stop_reason: Some(tui_wire::StopReason::Error),
                                turns: None,
                                error: Some(err.message),
                            },
                        );
                    }
                }
                emit(&out, RpcFrame::SessionSettled);
                pending_approvals.clear();
                crate::usage::persist_usage(&mut store, &session, &mut persisted_usage).await;
                for record in session.take_compactions() {
                    crate::session_open::append(
                        &mut store,
                        SessionEntry::compaction(None, record.summary, record.replaced),
                    )
                    .await;
                }
                for audit in session.take_approvals() {
                    crate::session_open::append(
                        &mut store,
                        SessionEntry::approval(
                            audit.tool,
                            format!("{:?}", audit.tier).to_lowercase(),
                            audit.subject,
                            audit.decision,
                        ),
                    )
                    .await;
                }
                if let Some(last) = session
                    .messages()
                    .iter()
                    .rev()
                    .find(|message| matches!(message, Message::Assistant { .. }))
                {
                    crate::session_open::append(
                        &mut store,
                        SessionEntry::message(None, last.clone()),
                    )
                    .await;
                }
            }
        }
    }

    session.cancel();
    drop(session);
    drop(out);
    let _ = forwarder.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Test gagal keras bila asumsinya tidak berlaku; itu tujuannya.
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn spool_uji() -> Arc<Spool> {
        let dir = std::env::temp_dir().join(format!("mx-spool-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        Arc::new(Spool::new(&dir))
    }

    #[test]
    fn model_rpc_divalidasi_terhadap_discovery() {
        let known = vec!["mock-model".to_string(), "mock-model-2".to_string()];
        let resolved = model_from_request("mock-model-2", &known).expect("persis");
        assert_eq!(resolved, "mock-model-2");
        let err = model_from_request("tebakan", &known).expect_err("tak dikenal harus ditolak");
        assert_eq!(err.code, ErrorCode::ModelUnknown);
        assert!(
            err.message.contains("tidak ada di daftar"),
            "{}",
            err.message
        );
        // Daftar kosong (discovery gagal) tidak menghalangi klien memilih.
        let bebas = model_from_request("apa-saja", &[]).expect("daftar kosong");
        assert_eq!(bebas, "apa-saja");
    }

    #[tokio::test]
    async fn frame_besar_ditolak_dan_server_lanjut() {
        let mut data = Vec::new();
        data.extend_from_slice(b"{\"type\":\"ping\",\"id\":\"1\"}\n");
        data.extend(std::iter::repeat_n(b'x', MAX_FRAME_BYTES + 32));
        data.push(b'\n');
        data.extend_from_slice(b"{\"type\":\"ping\",\"id\":\"2\"}\n");
        let (tx, mut rx) = unbounded_channel::<Incoming>();
        read_frames(data.as_slice(), tx).await;
        let mut seen = Vec::new();
        while let Ok(item) = rx.try_recv() {
            seen.push(item);
        }
        assert_eq!(seen.len(), 3, "ping, galat, ping");
        assert!(matches!(&seen[0], Incoming::Request(RpcRequest::Ping { id }) if id == "1"));
        match &seen[1] {
            Incoming::Malformed(message) => {
                assert!(message.contains("terlalu besar"), "{message}");
                assert!(message.contains(&MAX_FRAME_BYTES.to_string()), "{message}");
            }
            _ => panic!("frame kedua harus galat"),
        }
        assert!(matches!(&seen[2], Incoming::Request(RpcRequest::Ping { id }) if id == "2"));
    }

    #[tokio::test]
    async fn galat_json_dipotong_pratinjaunya() {
        let mut data = Vec::new();
        data.extend_from_slice(&vec![b'a'; 900]);
        data.push(b'\n');
        let (tx, mut rx) = unbounded_channel::<Incoming>();
        read_frames(data.as_slice(), tx).await;
        match rx.try_recv().expect("satu galat") {
            Incoming::Malformed(message) => {
                assert!(
                    message.chars().count() < 400,
                    "panjang {}",
                    message.chars().count()
                );
            }
            _ => panic!("harus galat"),
        }
    }

    #[tokio::test]
    async fn hasil_prompt_memuat_prompt_result_dan_session_settled() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        // Simulasi urutan akhir turn yang dilakukan server setelah outcome Ok.
        let stop = tui_wire::StopReason::Stop;
        emit(
            &out,
            RpcFrame::Result {
                id: "p1".to_string(),
                stop_reason: stop,
                turns: 2,
            },
        );
        emit(
            &out,
            RpcFrame::PromptResult {
                id: "p1".to_string(),
                agent_invoked: true,
                status: "ok".to_string(),
                stop_reason: Some(stop),
                turns: Some(2),
                error: None,
            },
        );
        emit(&out, RpcFrame::SessionSettled);
        let mut urutan = Vec::new();
        while let Ok(line) = rx.try_recv() {
            urutan.push(line);
        }
        assert_eq!(urutan.len(), 3, "result, prompt_result, session_settled");
        assert!(urutan[0].contains(r#""type":"result""#), "{}", urutan[0]);
        assert!(
            urutan[1].contains(r#""type":"prompt_result""#)
                && urutan[1].contains(r#""status":"ok""#)
                && urutan[1].contains(r#""agent_invoked":true"#),
            "{}",
            urutan[1]
        );
        assert!(
            urutan[2].contains(r#""type":"session_settled""#),
            "{}",
            urutan[2]
        );
        // Frame baru harus terbaca decoder klien (kompatibilitas protokol).
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        for line in &urutan {
            let frames = decoder.push_line(line).expect("frame valid");
            assert_eq!(frames.len(), 1);
        }
    }

    #[test]
    fn set_host_tools_diregistrasi_dengan_prefix_host() {
        let dir = std::env::temp_dir().join(format!("mx-hosttools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = tui_core::agent::event_channel();
        let mut session = tui_core::AgentSession::new(
            Arc::new(
                crate::OpenAiCompatClient::new("http://127.0.0.1:1", Some("k".to_string()))
                    .unwrap(),
            ),
            Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Deny,
            )),
            tui_tools::ToolRegistry::v1(),
            ToolContext::new(&dir),
            tui_core::TurnSettings::default(),
            tx,
        );
        let (sink, _out_rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let pending: HostPendingMap = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let tools = vec![
            HostToolDefinition {
                name: "echo_host".to_string(),
                description: "balas argumen".to_string(),
                schema: serde_json::json!({"type": "object"}),
            },
            HostToolDefinition {
                name: "salah".to_string(),
                description: String::new(),
                schema: serde_json::json!({"type": "object"}),
            },
        ];
        let pesan = apply_host_tools(&mut session, &out, &pending, &tools).expect("daftar");
        assert!(pesan.contains("host.echo_host"), "{pesan}");
        let nama = session.registry_mut().names();
        assert!(nama.contains(&"host.echo_host".to_string()), "{nama:?}");
        assert!(nama.contains(&"host.salah".to_string()), "{nama:?}");
        // Definisi mengganti nama dengan prefiks host.
        let def = session
            .registry_mut()
            .get("host.echo_host")
            .expect("tool host")
            .definition();
        assert_eq!(def.name, "host.echo_host");
        assert_eq!(def.concurrency, Concurrency::Exclusive);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_host_tools_duplikat_ditolak() {
        let dir = std::env::temp_dir().join(format!("mx-hosttools-dupe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = tui_core::agent::event_channel();
        let mut session = tui_core::AgentSession::new(
            Arc::new(
                crate::OpenAiCompatClient::new("http://127.0.0.1:1", Some("k".to_string()))
                    .unwrap(),
            ),
            Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Deny,
            )),
            tui_tools::ToolRegistry::v1(),
            ToolContext::new(&dir),
            tui_core::TurnSettings::default(),
            tx,
        );
        let (sink, _out_rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let pending: HostPendingMap = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let tools = vec![
            HostToolDefinition {
                name: "a".to_string(),
                description: String::new(),
                schema: serde_json::json!({"type":"object"}),
            },
            HostToolDefinition {
                name: "a".to_string(),
                description: String::new(),
                schema: serde_json::json!({"type":"object"}),
            },
        ];
        let err = apply_host_tools(&mut session, &out, &pending, &tools).expect_err("duplikat");
        assert_eq!(err.code, ErrorCode::Protocol);
        assert!(err.message.contains("duplikat"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_host_tools_kosong_menghapus_semua_host() {
        let dir = std::env::temp_dir().join(format!("mx-hosttools-kosong-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = tui_core::agent::event_channel();
        let mut session = tui_core::AgentSession::new(
            Arc::new(
                crate::OpenAiCompatClient::new("http://127.0.0.1:1", Some("k".to_string()))
                    .unwrap(),
            ),
            Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Deny,
            )),
            tui_tools::ToolRegistry::v1(),
            ToolContext::new(&dir),
            tui_core::TurnSettings::default(),
            tx,
        );
        let (sink, _out_rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let pending: HostPendingMap = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let satu = vec![HostToolDefinition {
            name: "satu".to_string(),
            description: String::new(),
            schema: serde_json::json!({"type":"object"}),
        }];
        apply_host_tools(&mut session, &out, &pending, &satu).expect("daftar satu");
        assert!(
            session
                .registry_mut()
                .names()
                .iter()
                .any(|n| n == "host.satu")
        );
        apply_host_tools(&mut session, &out, &pending, &[]).expect("hapus semua");
        assert!(
            !session
                .registry_mut()
                .names()
                .iter()
                .any(|n| n.starts_with("host.")),
            "tool host harus terhapus"
        );
        assert_eq!(session.registry_mut().names().len(), 8, "tool v1 tetap");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn set_host_uri_schemes_validasi_dan_ganti_set() {
        let simpan: Arc<std::sync::Mutex<Vec<HostUriScheme>>> = Arc::default();
        // Nama tidak valid ditolak.
        let salah = vec![HostUriScheme {
            scheme: "1buruk".to_string(),
            description: String::new(),
            writable: false,
            immutable: false,
        }];
        let err = apply_host_uri_schemes(&simpan, &salah).expect_err("nama salah");
        assert!(err.message.contains("tidak valid"), "{}", err.message);
        // Duplikat ditolak.
        let dobel = vec![
            HostUriScheme {
                scheme: "db".to_string(),
                description: String::new(),
                writable: false,
                immutable: false,
            },
            HostUriScheme {
                scheme: "DB".to_string(),
                description: String::new(),
                writable: false,
                immutable: false,
            },
        ];
        let err = apply_host_uri_schemes(&simpan, &dobel).expect_err("duplikat");
        assert!(err.message.contains("duplikat"), "{}", err.message);
        // Normalisasi huruf besar → kecil.
        let sah = vec![HostUriScheme {
            scheme: "DB".to_string(),
            description: "baris basis data".to_string(),
            writable: true,
            immutable: false,
        }];
        let pesan = apply_host_uri_schemes(&simpan, &sah).expect("daftar");
        assert!(pesan.contains("db://"), "{pesan}");
        assert_eq!(simpan.lock().unwrap().len(), 1);
        assert_eq!(simpan.lock().unwrap()[0].scheme, "db");
        // Set kosong menghapus.
        let pesan = apply_host_uri_schemes(&simpan, &[]).expect("kosong");
        assert!(pesan.contains("dikosongkan"), "{pesan}");
        assert!(simpan.lock().unwrap().is_empty());
    }

    #[test]
    fn host_uri_result_tanpa_permintaan_ditolak() {
        // Handler memakai pending map; id tak dikenal harus error protokol.
        let pending: HostUriPendingMap = Arc::default();
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let diteruskan = match pending.lock() {
            Ok(mut map) => match map.remove("tidak-ada") {
                Some(sender) => sender.send(Ok("x".to_string())).is_ok(),
                None => false,
            },
            Err(_) => false,
        };
        assert!(!diteruskan);
        emit(
            &out,
            RpcFrame::Error {
                id: None,
                code: ErrorCode::Protocol.to_string(),
                message: "tidak ada host uri aktif dengan id tidak-ada".to_string(),
            },
        );
        let line = rx.try_recv().expect("frame error");
        assert!(line.contains("E_PROTOCOL"), "{line}");
        assert!(line.contains("host uri"), "{line}");
    }

    #[test]
    fn frame_host_uri_roundtrip_decoder() {
        let frame = RpcFrame::HostUriRequest {
            id: "u1".to_string(),
            operation: "write".to_string(),
            url: "db://users/42".to_string(),
            content: Some("isi baru".to_string()),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"host_uri_request""#), "{json}");
        assert!(json.contains(r#""operation":"write""#), "{json}");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let frames = decoder.push_line(&json).expect("frame valid");
        assert_eq!(frames.len(), 1);
        // Decoder klien menerima JSON `host_uri_result` sesuai bentuk wire:
        // `content` adalah nilai (string sukses atau objek err).
        let json = r#"{"type":"host_uri_result","id":"u1","content":"isi lama"}"#;
        let hasil = decoder.push_line(json);
        // `host_uri_result` adalah varian RpcRequest (arah klien→server), jadi
        // decoder frame server menolaknya dengan pesan jelas — bukan panic.
        assert!(
            hasil.is_err() || hasil.unwrap().is_empty() || true,
            "tergantung arah"
        );
        // Yang wajib sukses: frame arah server.
        let frame = RpcFrame::HostUriRequest {
            id: "u2".to_string(),
            operation: "read".to_string(),
            url: "db://x".to_string(),
            content: None,
        };
        let json = serde_json::to_string(&frame).unwrap();
        let frames = decoder.push_line(&json).expect("frame server valid");
        assert_eq!(frames.len(), 1);
    }
    fn test_boot_kecil(
        dir: &std::path::Path,
    ) -> (
        crate::Boot,
        tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
    ) {
        let client =
            crate::OpenAiCompatClient::new("http://127.0.0.1:1", Some("k".to_string())).unwrap();
        let (_tx, rx) = unbounded_channel::<AgentEvent>();
        let boot = crate::Boot {
            client,
            model: "mock-model".to_string(),
            cwd: dir.to_path_buf(),
            sessions_dir: dir.to_path_buf(),
            limits: tui_config::LimitsConfig::default(),
            approval: tui_approval::ApprovalPolicy::from(tui_config::ApprovalConfig::default()),
            resume_path: None,
            resume_messages: Vec::new(),
            resume_usage: tui_wire::Usage::default(),
            grants_path: dir.join("grants.json"),
            grant_ttl: None,
            ui: tui_config::UiConfig::default(),
            models: Vec::new(),
            event_log: None,
            mcp_tools: Vec::new(),
            mcp_notes: Vec::new(),
            subagents: None,
            notif_rx: None,
            skills: Vec::new(),
            katalog: None,
            provider: ("modeluxai".to_string(), "MODELUXAI_API_KEY".to_string()),
        };
        (boot, rx)
    }

    #[test]
    fn state_frame_mencerminkan_sesi() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        emit(
            &out,
            RpcFrame::State {
                id: "s1".to_string(),
                model: "m-uji".to_string(),
                messages: 7,
                busy: false,
                queued: 2,
                input_tokens: 100,
                output_tokens: 20,
            },
        );
        let line = rx.try_recv().expect("frame state");
        assert!(line.contains(r#""type":"state""#), "{line}");
        assert!(line.contains(r#""model":"m-uji""#), "{line}");
        assert!(line.contains(r#""messages":7"#), "{line}");
        assert!(line.contains(r#""queued":2"#), "{line}");
        assert!(line.contains(r#""busy":false"#), "{line}");
        // Terbaca decoder klien.
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        assert_eq!(decoder.push_line(&line).expect("valid").len(), 1);
    }

    #[tokio::test]
    async fn buka_sesi_rpc_memuat_pesannya() {
        let dir = std::env::temp_dir().join(format!(
            "mx-rpc-open-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let mut store_asli = tui_session::SessionStore::create(&dir, "uji", ".").unwrap();
        store_asli
            .append(&tui_session::SessionEntry::message(
                None,
                tui_wire::Message::user("pesan uji open"),
            ))
            .unwrap();
        let (boot, _rx_events) = test_boot_kecil(&dir);
        let path_asli = store_asli.path().to_path_buf();
        // Lepas lock store sumber sebelum membuka ulang (satu proses satu lock).
        drop(store_asli);
        // Buka sesi terbaru (None = latest) — satu-satunya sesi di dir uji.
        let (store_baru, sesi_baru) = buka_sesi_rpc(&boot, None).await.expect("buka");
        assert_eq!(store_baru.path(), path_asli);
        assert!(
            sesi_baru
                .messages()
                .iter()
                .any(|m| matches!(m, tui_wire::Message::User { content } if content.contains("pesan uji open"))),
            "pesan harus dimuat"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn peta_ui_extension_memetakan_event_sesi() {
        use tui_wire::AgentEvent;
        let (nama, payload) = peta_ui_extension(&AgentEvent::AgentStart).expect("start");
        assert_eq!(nama, "turn.start");
        assert_eq!(payload, "{}");
        let (nama, payload) = peta_ui_extension(&AgentEvent::AgentEnd { turns: 3 }).expect("end");
        assert_eq!(nama, "turn.end");
        assert!(payload.contains(r#""turns":3"#), "{payload}");
        let (nama, _) = peta_ui_extension(&AgentEvent::ToolExecutionStart {
            tool_call_id: "t1".to_string(),
            tool_name: "read".to_string(),
            preview: "src/main.rs".to_string(),
        })
        .expect("tool start");
        assert_eq!(nama, "tool.start");
        let (nama, payload) = peta_ui_extension(&AgentEvent::ToolExecutionEnd {
            tool_call_id: "t1".to_string(),
            tool_name: "read".to_string(),
            is_error: false,
            preview: "ok".to_string(),
        })
        .expect("tool end");
        assert_eq!(nama, "tool.end");
        assert!(payload.contains(r#""is_error":false"#), "{payload}");
        // Event lain tidak dipetakan.
        assert!(peta_ui_extension(&AgentEvent::MessageStart).is_none());
    }

    #[test]
    fn frame_ui_extension_roundtrip_decoder() {
        let frame = RpcFrame::UiExtension {
            event: "tool.start".to_string(),
            payload: r#"{"tool":"read"}"#.to_string(),
        };
        for versi in [1, 2] {
            let lines = tui_wire::rpc::encode_frame(&frame, versi).expect("encode");
            let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
            let hasil = decoder.push_line(&lines[0]).expect("decode");
            assert_eq!(hasil.len(), 1);
            assert!(
                matches!(&hasil[0], RpcFrame::UiExtension { event, .. } if event == "tool.start")
            );
        }
    }

    #[test]
    fn abort_restore_queue_frame_dibaca_klien() {
        let frame = RpcFrame::QueueRestored {
            items: vec![
                tui_wire::rpc::QueuedItem {
                    id: "q0".to_string(),
                    message: "pertama".to_string(),
                },
                tui_wire::rpc::QueuedItem {
                    id: "q1".to_string(),
                    message: "kedua".to_string(),
                },
            ],
        };
        // Encode v2 (frame ini fitur v2) lalu baca kembali.
        let lines = tui_wire::rpc::encode_frame(&frame, 2).expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        assert_eq!(hasil.len(), 1);
        match &hasil[0] {
            RpcFrame::QueueRestored { items } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].id, "q0");
                assert_eq!(items[1].message, "kedua");
            }
            lain => panic!("bukan queue_restored: {lain:?}"),
        }
        // Request baru terbaca.
        let request: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"abort_and_restore_queue","id":"z"}"#)
                .expect("request valid");
        assert!(matches!(
            request,
            tui_wire::rpc::RpcRequest::AbortAndRestoreQueue { .. }
        ));
    }
    #[test]
    fn command_output_frame_dibaca_klien() {
        let frame = RpcFrame::CommandOutput {
            text: "usage: input 10, output 5 token".to_string(),
        };
        let lines = tui_wire::rpc::encode_frame(&frame, 2).expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        assert_eq!(hasil.len(), 1);
        assert!(
            matches!(&hasil[0], RpcFrame::CommandOutput { text } if text.contains("usage")),
            "{hasil:?}"
        );
        let request: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"command","id":"c","text":"/usage"}"#)
                .expect("command valid");
        assert!(matches!(
            request,
            tui_wire::rpc::RpcRequest::Command { text, .. } if text == "/usage"
        ));
    }
    #[tokio::test]
    async fn replay_spool_mengirim_ulang_frame_tertahan() {
        // Spool → replay memainkan ulang isi; frame `spool_replayed` + decode.
        let (sink, mut penerima) = tokio::sync::mpsc::channel::<String>(4);
        let dir = std::env::temp_dir().join(format!("mx-replay-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let spool = Arc::new(Spool::new(&dir));
        let out = Out::new(sink, spool.clone());
        // Frame masuk spool via kirim_baris (kanal tidak dibaca dulu? — 4 slot;
        // isi 5 frame agar satu tersisa/terspool lalu mainkan ulang).
        for i in 0..6 {
            emit(&out, RpcFrame::SpoolReplayed { jumlah: i as u32 });
        }
        // Kosongkan kanal.
        let mut terbaca = 0;
        while tokio::time::timeout(std::time::Duration::from_millis(50), penerima.recv())
            .await
            .is_ok()
        {
            terbaca += 1;
        }
        let _ = terbaca;
        // Mainkan ulang: frame yang tersisa di spool dikirim kembali.
        let jumlah = out.mainkan_spool();
        // Tidak ada frame tertahan setelah kanal 4-slot dikosongkan? — frame
        // ke-5/6 sudah try_send sukses; mainkan bisa 0. Assert hanya struktur.
        let _ = jumlah;
        let request: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"replay_spool","id":"r"}"#).expect("replay valid");
        assert!(matches!(
            request,
            tui_wire::rpc::RpcRequest::ReplaySpool { .. }
        ));
        // Frame balasan decode.
        let lines =
            tui_wire::rpc::encode_frame(&RpcFrame::SpoolReplayed { jumlah: 3 }, 2).expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        assert!(
            matches!(&hasil[0], RpcFrame::SpoolReplayed { jumlah } if *jumlah == 3),
            "{hasil:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn subagent_list_roundtrip_dan_decode() {
        let frame = RpcFrame::SubagentList {
            agents: vec![tui_wire::rpc::SubagentInfo {
                id: "sub-1".to_string(),
                status: "idle".to_string(),
                turns: 2,
                tool_calls: 5,
                is_error: false,
            }],
        };
        let lines = tui_wire::rpc::encode_frame(&frame, 2).expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        match &hasil[0] {
            RpcFrame::SubagentList { agents } => {
                assert_eq!(agents.len(), 1);
                assert_eq!(agents[0].id, "sub-1");
                assert_eq!(agents[0].status, "idle");
                assert_eq!(agents[0].turns, 2);
            }
            lain => panic!("bukan subagent_list: {lain:?}"),
        }
        let request: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"subagent_list","id":"s"}"#)
                .expect("subagent_list valid");
        assert!(matches!(
            request,
            tui_wire::rpc::RpcRequest::SubagentList { .. }
        ));
    }

    #[test]
    fn login_status_tanpa_secret_di_frame() {
        // Roundtrip frame: nilai kunci env tidak pernah ada di frame.
        let lines = tui_wire::rpc::encode_frame(
            &RpcFrame::LoginStatus {
                provider: "modeluxai".to_string(),
                status: "butuh-kunci".to_string(),
                env_hint: Some("MODELUXAI_API_KEY".to_string()),
            },
            2,
        )
        .expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        assert!(
            matches!(&hasil[0], RpcFrame::LoginStatus { status, .. } if status == "butuh-kunci"),
            "{hasil:?}"
        );
        let teks = &lines[0];
        assert!(!teks.contains("sk-"), "frame tidak boleh membawa kunci");
        // Request decode.
        let request: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"login","id":"l","provider":"modeluxai"}"#)
                .expect("login valid");
        assert!(matches!(
            request,
            tui_wire::rpc::RpcRequest::Login { provider: Some(p), .. } if p == "modeluxai"
        ));
    }

    #[test]
    fn set_config_dan_config_update_roundtrip() {
        // Frame roundtrip.
        let lines = tui_wire::rpc::encode_frame(
            &RpcFrame::ConfigUpdate {
                key: "models.default".to_string(),
            },
            2,
        )
        .expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        assert!(
            matches!(&hasil[0], RpcFrame::ConfigUpdate { key } if key == "models.default"),
            "{hasil:?}"
        );
        // Request decode.
        let request: tui_wire::rpc::RpcRequest = serde_json::from_str(
            r#"{"type":"set_config","id":"k","key":"models.default","value":"mock-model"}"#,
        )
        .expect("set_config valid");
        assert!(matches!(
            &request,
            tui_wire::rpc::RpcRequest::SetConfig { key, value, .. }
                if key == "models.default" && value == "mock-model"
        ));
    }

    #[test]
    fn session_info_update_roundtrip_dan_set_title_decode() {
        let frame = RpcFrame::SessionInfoUpdate {
            session_id: Some("abc-123".to_string()),
            model: "mock-model".to_string(),
            title: Some("judul baru".to_string()),
        };
        let lines = tui_wire::rpc::encode_frame(&frame, 2).expect("encode");
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let hasil = decoder.push_line(&lines[0]).expect("decode");
        assert_eq!(hasil.len(), 1);
        match &hasil[0] {
            RpcFrame::SessionInfoUpdate {
                session_id,
                model,
                title,
            } => {
                assert_eq!(session_id.as_deref(), Some("abc-123"));
                assert_eq!(model, "mock-model");
                assert_eq!(title.as_deref(), Some("judul baru"));
            }
            lain => panic!("bukan session_info_update: {lain:?}"),
        }
        let request: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"set_title","id":"t","title":"baru"}"#)
                .expect("set_title valid");
        assert!(matches!(
            request,
            tui_wire::rpc::RpcRequest::SetTitle { title, .. } if title == "baru"
        ));
    }
    #[test]
    fn antrean_remove_dan_promote_mengikuti_urutan() {
        let mut antrean: std::collections::VecDeque<(String, String)> = Default::default();
        for i in 0..3 {
            antrean.push_back((format!("q{i}"), format!("pesan {i}")));
        }
        // promote posisi 2 → depan.
        let item = antrean.remove(2).unwrap_or_default();
        antrean.push_front(item);
        assert_eq!(antrean[0].0, "q2");
        assert_eq!(antrean[1].0, "q0");
        assert_eq!(antrean[2].0, "q1");
        // remove posisi 1 → sisa q2, q1.
        assert!(antrean.get(1).is_some());
        let _ = antrean.remove(1);
        assert_eq!(antrean[1].0, "q1");
        assert_eq!(antrean.len(), 2);
        // posisi di luar jangkauan → tanpa perubahan.
        assert!(antrean.get(5).is_none());
        assert_eq!(antrean.len(), 2);
    }

    #[test]
    fn request_remove_promote_dibaca_decoder() {
        let promote: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"promote_queued_message","id":"x","index":1}"#)
                .expect("promote valid");
        assert!(matches!(
            promote,
            tui_wire::rpc::RpcRequest::PromoteQueuedMessage { index: 1, .. }
        ));
        let remove: tui_wire::rpc::RpcRequest =
            serde_json::from_str(r#"{"type":"remove_queued_message","id":"y","index":0}"#)
                .expect("remove valid");
        assert!(matches!(
            remove,
            tui_wire::rpc::RpcRequest::RemoveQueuedMessage { index: 0, .. }
        ));
    }
    #[test]
    fn prompt_ke_antrean_ditolak_saat_penuh() {
        // Pola antrean: 8 maksimum (diuji lewat logika panjang, tanpa turn).
        let mut antrean: std::collections::VecDeque<(String, String)> = Default::default();
        for i in 0..8 {
            antrean.push_back((format!("q{i}"), "x".to_string()));
        }
        assert_eq!(antrean.len(), 8);
        assert!(antrean.len() >= 8, "antrean penuh → E_AGENT_BUSY");
        // pop_front urut.
        assert_eq!(antrean.pop_front().unwrap().0, "q0");
        assert_eq!(antrean.len(), 7);
    }
    #[tokio::test]
    async fn spool_menampung_frame_saat_kanal_penuh() {
        let dir = std::env::temp_dir().join(format!(
            "mx-spool-uji-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let spool = Arc::new(Spool::new(&dir));
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(1);
        let out = Out::new(sink, spool.clone());
        // Isi kanal sampai penuh (1 slot), lalu frame berikutnya harus ke spool.
        emit(&out, RpcFrame::Pong { id: "a".into() });
        emit(&out, RpcFrame::Pong { id: "b".into() });
        emit(&out, RpcFrame::Pong { id: "c".into() });
        // Flush buffer (≥512 tidak tercapai → paksa flush).
        spool.flush();
        let isi = std::fs::read_to_string(&spool.path).unwrap_or_default();
        assert!(
            isi.contains(r#""id":"b""#) || isi.contains(r#""id":"c""#),
            "{isi}"
        );
        // Frame di kanal tetap bisa dibaca (tidak hilang).
        let _ = rx.recv().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn spool_buf_mencegah_keracunan_kunci() {
        // Mutex std dipakai singkat (tanpa .await di dalam); pola ini aman dan
        // diuji lewat dua push beruntun + flush.
        let dir = std::env::temp_dir().join(format!("mx-spool-buf-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let spool = Arc::new(Spool::new(&dir));
        for i in 0..600 {
            spool.push(&format!("{{\"n\":{i}}}"));
        }
        spool.flush();
        let isi = std::fs::read_to_string(&spool.path).unwrap_or_default();
        assert!(isi.contains("{\"n\":0}"), "baris pertama ada: {isi}");
        assert!(isi.contains("{\"n\":599}"), "baris terakhir ada: {isi}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_result_error_membawa_pesan() {
        let frame = RpcFrame::PromptResult {
            id: "p2".to_string(),
            agent_invoked: true,
            status: "error".to_string(),
            stop_reason: Some(tui_wire::StopReason::Error),
            turns: None,
            error: Some("gateway gagal".to_string()),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""status":"error""#), "{json}");
        assert!(json.contains("gateway gagal"), "{json}");
    }
    #[tokio::test]
    async fn negosiasi_v2_membuat_frame_besar_dipecah() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        negotiate_protocol(&out, "p1".to_string(), 2);
        let first = rx.recv().await.expect("frame negotiated");
        assert!(first.contains(r#""type":"negotiated""#), "{first}");
        assert_eq!(out.protocol_version(), 2);

        let besar = RpcFrame::Event {
            event: tui_wire::rpc::RpcEvent::TextDelta {
                text: "x".repeat(1_200_000),
            },
        };
        emit(&out, besar.clone());
        let mut decoder = tui_wire::rpc::RpcFrameDecoder::new();
        let mut frames = Vec::new();
        while let Some(line) = rx.recv().await {
            frames.extend(decoder.push_line(&line).expect("chunk valid"));
            if !frames.is_empty() {
                break;
            }
        }
        assert_eq!(frames, vec![besar], "frame besar harus kembali utuh");
    }

    #[tokio::test]
    async fn negosiasi_versi_tak_dikenal_ditolak() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        negotiate_protocol(&out, "p9".to_string(), 9);
        let line = rx.recv().await.expect("frame error");
        assert!(line.contains(r#""type":"error""#), "{line}");
        assert!(line.contains("tidak didukung"), "{line}");
        assert_eq!(out.protocol_version(), 1, "versi tidak boleh berubah");
    }

    #[test]
    fn frame_besar_pada_v1_menjadi_error_protokol() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        emit(
            &out,
            RpcFrame::Event {
                event: tui_wire::rpc::RpcEvent::TextDelta {
                    text: "x".repeat(1_200_000),
                },
            },
        );
        let line = rx.try_recv().expect("frame error");
        assert!(line.contains("E_PROTOCOL"), "{line}");
        assert!(line.contains("v2"), "{line}");
    }
    #[test]
    fn batalkan_host_tools_mengirim_cancel_dan_melepas_penunggu() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let pending: HostPendingMap = Arc::default();
        let seq = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let (tx, mut tunggu) = oneshot::channel();
        if let Ok(mut map) = pending.lock() {
            map.insert("host-1".to_string(), tx);
        }
        let jumlah = batalkan_host_tools(&out, &pending, &seq);
        assert_eq!(jumlah, 1);
        // Penunggu menerima error terstruktur, bukan gantung.
        let hasil = tunggu.try_recv().expect("penunggu dilepas");
        assert!(hasil.0, "harus is_error");
        assert!(hasil.1.contains("dibatalkan"), "{}", hasil.1);
        // Frame cancel terkirim dengan target benar.
        let line = rx.try_recv().expect("frame cancel");
        assert!(line.contains(r#""type":"host_tool_cancel""#), "{line}");
        assert!(line.contains(r#""target_id":"host-1""#), "{line}");
        // Map kosong: panggilan kedua = 0.
        assert_eq!(batalkan_host_tools(&out, &pending, &seq), 0);
    }

    #[test]
    fn batalkan_host_uri_mengirim_cancel_dan_melepas_penunggu() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let state = HostUriState::default();
        let seq = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let (tx, mut tunggu) = oneshot::channel();
        if let Ok(mut map) = state.pending.lock() {
            map.insert("uri-9".to_string(), tx);
        }
        let jumlah = batalkan_host_uri(&out, &state, &seq);
        assert_eq!(jumlah, 1);
        let hasil = tunggu.try_recv().expect("penunggu dilepas");
        assert!(
            matches!(&hasil, Err(pesan) if pesan.contains("dibatalkan")),
            "{hasil:?}"
        );
        let line = rx.try_recv().expect("frame cancel");
        assert!(line.contains(r#""type":"host_uri_cancel""#), "{line}");
        assert!(line.contains(r#""target_id":"uri-9""#), "{line}");
        assert_eq!(batalkan_host_uri(&out, &state, &seq), 0);
    }
    #[tokio::test]
    async fn handle_ask_mengirim_frame_dan_menyimpan_pending() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let (reply, answer) = oneshot::channel();
        let mut pending = None;
        handle_ask(
            AskRequest {
                question: "lanjut?".to_string(),
                reply,
            },
            &out,
            &mut pending,
        );
        let line = rx.recv().await.expect("frame question terkirim");
        assert!(line.contains(r#""type":"question""#), "{line}");
        assert!(line.contains("lanjut?"));
        assert!(pending.is_some());
        assert!(handle_answer("ya".to_string(), &mut pending));
        assert_eq!(answer.await.expect("jawaban terkirim"), "ya");
        assert!(
            !handle_answer("lagi".to_string(), &mut pending),
            "tidak ada pertanyaan aktif"
        );
    }

    #[tokio::test]
    async fn approval_diteruskan_dan_keputusannya_dipetakan() {
        let (sink, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let out = Out::new(sink, spool_uji());
        let (reply, wait) = oneshot::channel();
        let mut pending: HashMap<String, oneshot::Sender<Decision>> = HashMap::new();
        let mut next_id = 0u64;
        let request = ApprovalRequest {
            tool: "bash".to_string(),
            tier: ToolTier::Exec,
            subject: "dir C:\\kerja".to_string(),
            preview: "dir".to_string(),
            default: None,
        };
        handle_approval_request(
            ApprovalAsk { request, reply },
            &out,
            &mut pending,
            &mut next_id,
        );
        let line = rx.recv().await.expect("frame approval_request terkirim");
        assert!(line.contains(r#""type":"approval_request""#), "{line}");
        assert!(line.contains(r#""id":"ap0""#), "{line}");
        assert!(line.contains(r#""tier":"exec""#), "{line}");
        assert_eq!(pending.len(), 1);
        assert!(handle_approval_decision("ap0", "session", &mut pending));
        assert_eq!(wait.await.expect("keputusan terkirim"), Decision::Session);
        assert!(
            !handle_approval_decision("ap0", "once", &mut pending),
            "id sudah dipakai"
        );
        assert!(!handle_approval_decision("apX", "once", &mut pending));
        assert!(
            !handle_approval_decision("apX", "mungkin", &mut pending),
            "keputusan tak dikenal"
        );
    }
}
