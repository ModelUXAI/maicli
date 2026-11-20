//! Protokol RPC baris-per-baris (`mx --mode rpc`).
//!
//! Klien menulis satu objek JSON per baris ke stdin; server menulis satu objek
//! JSON per baris ke stdout. Protokol ini stabil untuk otomasi, jadi event
//! internal diproyeksikan ke `RpcEvent` alih-alih mengekspos tipe internal.

use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ErrorCode, MxError};
use crate::event::{AgentEvent, ProviderEvent};
use crate::message::StopReason;

/// Versi protokol yang diumumkan di frame `ready`.
pub const PROTOCOL_VERSION: &str = "mx-rpc/1";

/// Versi protokol yang didukung server: v1 (default) dan v2 (chunk).
pub const SUPPORTED_PROTOCOL_VERSIONS: [u32; 2] = [1, 2];

/// Batas satu frame fisik (baris JSON), byte, termasuk newline.
pub const MAX_FRAME_BYTES: usize = 1_048_576;

/// Batas satu frame logis setelah reassembly `rpc_chunk` (64 MiB).
pub const MAX_REASSEMBLED_FRAME_BYTES: usize = 67_108_864;

/// Penomoran `chunk_id` proses; cukup unik selama proses hidup.
static CHUNK_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn galat_protokol(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::Protocol, pesan.into())
}

/// Definisi tool milik host yang didaftarkan lewat `set_host_tools`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostToolDefinition {
    /// Nama unik tool host (tanpa prefiks).
    pub name: String,
    /// Deskripsi untuk model.
    #[serde(default)]
    pub description: String,
    /// Skema parameter JSON.
    #[serde(default = "default_object_schema")]
    pub schema: Value,
}

fn default_object_schema() -> Value {
    serde_json::json!({"type": "object"})
}

/// Satu pesan antrean yang dikembalikan ke klien (`queue_restored`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedItem {
    /// Id prompt asli.
    pub id: String,
    /// Teks pesan.
    pub message: String,
}

/// Satu anak subagent di daftar RPC (read-only, tanpa transkrip).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentInfo {
    /// Id anak (`sub-1`, …).
    pub id: String,
    /// Status siklus hidup (label teks: berjalan/idle/parkir/dihentikan).
    pub status: String,
    /// Jumlah turn yang sudah dijalankan.
    pub turns: u32,
    /// Jumlah tool call.
    pub tool_calls: u32,
    /// Hasil terakhir berupa galat.
    pub is_error: bool,
}

/// Definisi satu skema URI milik host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostUriScheme {
    /// Nama skema tanpa `://` (huruf kecil, `[a-z][a-z0-9+.-]*`).
    pub scheme: String,
    /// Deskripsi untuk manusia.
    #[serde(default)]
    pub description: String,
    /// Boleh ditulis (`write`); default `false`.
    #[serde(default)]
    pub writable: bool,
    /// Isi tidak pernah berubah (dipakai cache); default `false`.
    #[serde(default)]
    pub immutable: bool,
}

impl HostUriScheme {
    /// Normalisasi + validasi nama skema.
    pub fn valid_name(&self) -> Result<String, MxError> {
        let nama = self.scheme.trim().to_ascii_lowercase();
        let mut chars = nama.chars();
        let sah = matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
            && nama.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '.' | '-')
            });
        if !sah {
            return Err(galat_protokol(format!(
                "nama skema URI host tidak valid: '{}' (harus [a-z][a-z0-9+.-]*)",
                self.scheme
            )));
        }
        Ok(nama)
    }
}

/// Permintaan dari klien.
///
/// Parsing permisif terhadap field tambahan supaya klien versi lebih baru tidak
/// langsung ditolak; varian tak dikenal tetap error.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcRequest {
    /// Kirim satu prompt dan jalankan turn.
    Prompt {
        /// Id permintaan untuk mencocokkan balasan.
        id: String,
        /// Teks prompt.
        message: String,
    },
    /// Batalkan turn yang sedang berjalan.
    Abort {
        /// Id permintaan (opsional).
        id: Option<String>,
    },
    /// Ganti model sesi.
    Model {
        /// Id permintaan.
        id: String,
        /// Id model tujuan.
        model: String,
    },
    /// Jalankan perintah slash (v2) → frame `command_output`.
    Command {
        /// Id permintaan.
        id: String,
        /// Teks perintah (dengan atau tanpa `/`).
        text: String,
    },
    /// Ubah kunci config runtime (v2, kunci terbatas `models.default`).
    SetConfig {
        /// Id permintaan.
        id: String,
        /// Kunci config (mis. `models.default`).
        key: String,
        /// Nilai baru.
        value: String,
    },
    /// Cek kesiapan login/autentikasi provider (v2, tanpa secret di frame).
    Login {
        /// Id permintaan.
        id: String,
        /// Id provider; tanpa argumen = provider bawaan.
        provider: Option<String>,
    },
    /// Daftar subagent + statusnya (v2, read-only).
    SubagentList {
        /// Id permintaan.
        id: String,
    },
    /// Kirim ulang frame yang tersimpan di spool saat kanal penuh (v2).
    ReplaySpool {
        /// Id permintaan.
        id: String,
    },
    /// Ganti judul sesi (v2) → frame `session_info_update`.
    SetTitle {
        /// Id permintaan.
        id: String,
        /// Judul baru.
        title: String,
    },
    /// Cek hidup.
    Ping {
        /// Id permintaan.
        id: String,
    },
    /// Baca pemakaian token sesi.
    Usage {
        /// Id permintaan.
        id: String,
    },
    /// Status sesi (model, jumlah pesan, antrean, usage).
    GetState {
        /// Id permintaan.
        id: String,
    },
    /// Buka sesi tersimpan (id/prefix) dan jadikan sesi aktif.
    OpenSession {
        /// Id permintaan.
        id: String,
        /// Id/prefix sesi; kosong = terbaru.
        session: Option<String>,
    },
    /// Putuskan permintaan approval (`approval_request`).
    Approval {
        /// Id permintaan approval.
        id: String,
        /// `once`, `session`, `always`, atau `deny`.
        decision: String,
    },
    /// Jawab pertanyaan yang dikirim server (`type: "question"`).
    Answer {
        /// Id permintaan (opsional).
        id: Option<String>,
        /// Teks jawaban; kosong berarti dilewati.
        text: String,
    },
    /// Negosiasi protokol v2 (chunking frame besar).
    NegotiateProtocol {
        /// Id permintaan.
        id: String,
        /// Versi protokol yang diminta klien.
        protocol_version: u32,
    },
    /// Host mendaftarkan tool miliknya (v2): ganti set sebelumnya; nama harus
    /// unik dan tidak boleh bertabrakan dengan tool agent.
    SetHostTools {
        /// Id permintaan.
        id: String,
        /// Definisi tool host (nama + skema + deskripsi); kosong = hapus semua.
        tools: Vec<HostToolDefinition>,
    },
    /// Hasil eksekusi permintaan `host_tool_call` (v2).
    HostToolResult {
        /// Id permintaan `host_tool_call` yang dijawab.
        id: String,
        /// Isi hasil; `is_error` menandai kegagalan tool host.
        is_error: bool,
        /// Teks hasil (blok non-teks di-flatten).
        content: String,
    },
    /// Host mendaftarkan skema URI miliknya (v2): ganti set sebelumnya.
    SetHostUriSchemes {
        /// Id permintaan.
        id: String,
        /// Skema yang didaftarkan; kosong = hapus semua.
        schemes: Vec<HostUriScheme>,
    },
    /// Langganan event extension UI (v2): hanya nama dari daftar ini yang
    /// diteruskan sebagai frame `ui_extension`; kosong = berhenti semua.
    SetUiExtension {
        /// Id permintaan.
        id: String,
        /// Nama event (`turn.start`, `turn.end`, `tool.start`, `tool.end`).
        events: Vec<String>,
    },
    /// Hasil operasi `host_uri_request` (v2): isi baca/konfirmasi tulis atau
    /// pesan kesalahan.
    HostUriResult {
        /// Id permintaan `host_uri_request` yang dijawab.
        id: String,
        /// `Ok(isi)` untuk sukses, `Err(pesan)` untuk kegagalan host.
        content: Result<String, String>,
    },
    /// Buang pesan antrean (v2): urutan 0 = terdepan.
    RemoveQueuedMessage {
        /// Id permintaan.
        id: String,
        /// Posisi di antrean.
        index: u32,
    },
    /// Naikkan pesan antrean ke posisi terdepan (v2).
    PromoteQueuedMessage {
        /// Id permintaan.
        id: String,
        /// Posisi saat ini; dipindah ke 0.
        index: u32,
    },
    /// Batalkan turn berjalan (v2) dan kembalikan seluruh antrean ke klien
    /// (frame `queue_restored`, antrean dikosongkan).
    AbortAndRestoreQueue {
        /// Id permintaan.
        id: String,
    },
    /// Tutup server dengan rapi.
    Shutdown,
}

/// Event yang dikirim server ke klien.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RpcEvent {
    /// Sesi mulai bekerja.
    AgentStart,
    /// Turn dimulai.
    TurnStart {
        /// Nomor turn.
        index: u32,
    },
    /// Pesan assistant mulai.
    MessageStart,
    /// Potongan teks jawaban.
    TextDelta {
        /// Teks baru.
        text: String,
    },
    /// Blok teks selesai.
    TextEnd,
    /// Tool mulai berjalan.
    ToolStart {
        /// Id panggilan.
        tool_call_id: String,
        /// Nama tool.
        tool_name: String,
        /// Cuplikan argumen.
        preview: String,
    },
    /// Tool selesai.
    ToolEnd {
        /// Id panggilan.
        tool_call_id: String,
        /// Nama tool.
        tool_name: String,
        /// Apakah error.
        is_error: bool,
        /// Cuplikan hasil.
        preview: String,
    },
    /// Turn selesai.
    TurnEnd {
        /// Nomor turn.
        index: u32,
    },
    /// Sesi selesai.
    AgentEnd {
        /// Jumlah turn.
        turns: u32,
    },
    /// Pesan sistem untuk pengguna.
    Notice {
        /// Isi pesan.
        text: String,
    },
}

impl RpcEvent {
    /// Proyeksikan event internal; `None` bila event tidak perlu dikirim.
    pub fn from_agent(event: &AgentEvent) -> Option<Self> {
        match event {
            AgentEvent::AgentStart => Some(Self::AgentStart),
            AgentEvent::TurnStart { index } => Some(Self::TurnStart { index: *index }),
            AgentEvent::MessageStart => Some(Self::MessageStart),
            AgentEvent::MessageUpdate(ProviderEvent::TextDelta { text }) => {
                Some(Self::TextDelta { text: text.clone() })
            }
            AgentEvent::MessageUpdate(ProviderEvent::TextEnd) => Some(Self::TextEnd),
            AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                preview,
            } => Some(Self::ToolStart {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                preview: preview.clone(),
            }),
            AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                is_error,
                preview,
            } => Some(Self::ToolEnd {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                is_error: *is_error,
                preview: preview.clone(),
            }),
            AgentEvent::TurnEnd { index } => Some(Self::TurnEnd { index: *index }),
            AgentEvent::AgentEnd { turns } => Some(Self::AgentEnd { turns: *turns }),
            AgentEvent::Notice { text } => Some(Self::Notice { text: text.clone() }),
            // Klien RPC tidak mengganti sesi di tengah koneksi; event ini lokal UI.
            AgentEvent::SessionSwitched { .. } => None,
            AgentEvent::SessionsListed { .. } => None,
            AgentEvent::ContextUsage { .. } => None,
            _ => None,
        }
    }
}

/// Frame dari server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcFrame {
    /// Dikirim sekali saat server siap.
    Ready {
        /// Versi protokol.
        protocol: String,
        /// Model awal.
        model: String,
        /// Versi protokol yang didukung server.
        supported_protocol_versions: Vec<u32>,
        /// Batas frame logis setelah reassembly chunk.
        max_reassembled_frame_bytes: u64,
    },
    /// Balasan `negotiate_protocol`.
    Negotiated {
        /// Id permintaan.
        id: String,
        /// Versi yang disepakati.
        protocol_version: u32,
    },
    /// Potongan frame logis yang melebihi batas fisik (hanya protokol v2).
    RpcChunk {
        /// Id urutan chunk; semua potongan satu frame memakai id sama.
        chunk_id: String,
        /// Indeks potongan, mulai 0.
        index: u32,
        /// Jumlah potongan total.
        count: u32,
        /// Panjang byte frame logis sebelum dipecah.
        byte_length: u64,
        /// Potongan base64.
        data: String,
    },
    /// Balasan `ping`.
    Pong {
        /// Id permintaan.
        id: String,
    },
    /// Permintaan diterima (mis. ganti model).
    Ack {
        /// Id permintaan.
        id: String,
    },
    /// Permintaan persetujuan tool; klien menjawab dengan `approval`.
    ApprovalRequest {
        /// Id untuk dipakai di balasan.
        id: String,
        /// Nama tool.
        tool: String,
        /// Tier: `read`, `write`, atau `exec`.
        tier: String,
        /// Subjek yang diminta (path/perintah).
        subject: String,
    },
    /// Pertanyaan dari tool `ask`; klien menjawab dengan `answer`.
    Question {
        /// Pertanyaan untuk pengguna.
        question: String,
    },
    /// Pemakaian token kumulatif.
    Usage {
        /// Id permintaan.
        id: String,
        /// Token input.
        input_tokens: u64,
        /// Token output.
        output_tokens: u64,
        /// Token dari cache.
        cached_tokens: u64,
    },
    /// Event selama turn.
    Event {
        /// Event yang bersangkutan.
        event: RpcEvent,
    },
    /// Hasil satu prompt.
    Result {
        /// Id permintaan.
        id: String,
        /// Alasan berhenti.
        stop_reason: StopReason,
        /// Jumlah turn.
        turns: u32,
    },
    /// Status akhir satu prompt (v1.1): menyebut bagaimana pekerjaan berakhir.
    PromptResult {
        /// Id permintaan `prompt` yang bersangkutan.
        id: String,
        /// Apakah agent benar-benar dijalankan (bila `false`, prompt selesai di
        /// tangani lokal tanpa turn).
        agent_invoked: bool,
        /// `ok`, `aborted`, atau `error`.
        status: String,
        /// Alasan berhenti; ada bila turn pernah berjalan.
        stop_reason: Option<StopReason>,
        /// Jumlah turn; `None` bila agent tidak dipanggil.
        turns: Option<u32>,
        /// Pesan kesalahan bila `status = "error"`.
        error: Option<String>,
    },
    /// Sesi diam dan siap menerima prompt berikutnya (v1.1).
    SessionSettled,
    /// Status sesi (balasan `get_state`/`open_session`).
    State {
        /// Id permintaan.
        id: String,
        /// Model aktif.
        model: String,
        /// Jumlah pesan di transkrip.
        messages: u64,
        /// Apakah turn sedang berjalan.
        busy: bool,
        /// Jumlah pesan yang menunggu di antrean RPC.
        queued: u64,
        /// Token input kumulatif.
        input_tokens: u64,
        /// Token output kumulatif.
        output_tokens: u64,
    },
    /// Event extension UI (v2): diteruskan bila klien melanggani namanya.
    UiExtension {
        /// Nama event (`turn.start`, `turn.end`, `tool.start`, `tool.end`).
        event: String,
        /// Payload JSON ringkas.
        payload: String,
    },
    /// Seluruh antrean dikembalikan ke klien (balasan `abort_and_restore_queue`).
    QueueRestored {
        /// Isi antrean saat abort, urutan semula (0 = terdepan).
        items: Vec<QueuedItem>,
    },
    /// Output perintah slash (v2).
    CommandOutput {
        /// Teks hasil perintah.
        text: String,
    },
    /// Kunci config berubah runtime (v2, balasan `set_config`).
    ConfigUpdate {
        /// Kunci yang berubah.
        key: String,
    },
    /// Status login provider (v2): `ok` / `butuh-kunci`; **tanpa nilai kunci**.
    LoginStatus {
        /// Id provider yang diperiksa.
        provider: String,
        /// `ok` bila env var kunci ada dan tidak kosong.
        status: String,
        /// Nama env var yang harus diisi bila `butuh-kunci`.
        env_hint: Option<String>,
    },
    /// Daftar subagent (v2, balasan `subagent_list`).
    SubagentList {
        /// Satu entri per anak.
        agents: Vec<SubagentInfo>,
    },
    /// Spool dimainkan ulang (v2, balasan `replay_spool`).
    SpoolReplayed {
        /// Jumlah frame yang dikirim ulang.
        jumlah: u32,
    },
    /// Info sesi berubah (v2): setelah `set_title` / `model` / `open_session`.
    SessionInfoUpdate {
        /// Id sesi (bila tersimpan).
        session_id: Option<String>,
        /// Model aktif.
        model: String,
        /// Judul sesi (bila diketahui).
        title: Option<String>,
    },
    /// Minta host mengeksekusi tool miliknya (v2); host menjawab `host_tool_result`.
    HostToolCall {
        /// Id permintaan host tool.
        id: String,
        /// Id tool call agent (untuk korelasi timeline host).
        tool_call_id: String,
        /// Nama tool host (tanpa prefiks).
        tool_name: String,
        /// Argumen JSON dari model.
        arguments: Value,
    },
    /// Permintaan host tool dibatalkan (v2).
    HostToolCancel {
        /// Id permintaan yang dibatalkan.
        id: String,
        /// Id `host_tool_call` yang ditarget.
        target_id: String,
    },
    /// Minta host membaca/menulis URI skema miliknya (v2).
    HostUriRequest {
        /// Id permintaan host URI.
        id: String,
        /// `read` atau `write`.
        operation: String,
        /// URL lengkap, mis. `db://users/42`.
        url: String,
        /// Isi pengganti bila `operation = "write"`.
        content: Option<String>,
    },
    /// Permintaan host URI dibatalkan (v2).
    HostUriCancel {
        /// Id permintaan yang dibatalkan.
        id: String,
        /// Id `host_uri_request` yang ditarget.
        target_id: String,
    },
    /// Kesalahan.
    Error {
        /// Id permintaan bila ada.
        id: Option<String>,
        /// Kode error stabil.
        code: String,
        /// Pesan untuk manusia.
        message: String,
    },
}

/// Encode satu frame logis menjadi satu atau beberapa baris JSON.
///
/// Protokol v1 hanya boleh satu baris per frame; frame yang melebihi
/// [`MAX_FRAME_BYTES`] menjadi error protokol. Protokol v2 memecah frame besar
/// menjadi `rpc_chunk` base64 (jumlah byte logis dibatasi
/// [`MAX_REASSEMBLED_FRAME_BYTES`]); klien merekonstruksi lewat
/// [`RpcFrameDecoder`].
pub fn encode_frame(frame: &RpcFrame, protocol_version: u32) -> Result<Vec<String>, MxError> {
    let line = serde_json::to_string(frame)
        .map_err(|err| galat_protokol(format!("gagal serialisasi frame: {err}")))?;
    if line.len() <= MAX_FRAME_BYTES {
        return Ok(vec![line]);
    }
    if protocol_version < 2 {
        return Err(galat_protokol(format!(
            "frame {} byte melebihi batas fisik {MAX_FRAME_BYTES}; negosiasikan protokol v2 untuk chunking",
            line.len()
        )));
    }
    let bytes = line.as_bytes();
    if bytes.len() > MAX_REASSEMBLED_FRAME_BYTES {
        return Err(galat_protokol(format!(
            "frame {} byte melebihi batas reassembly {MAX_REASSEMBLED_FRAME_BYTES}",
            bytes.len()
        )));
    }
    // Sisa ruang untuk field lain + pembungkus JSON; base64 memperbesar 4/3.
    const OVERHEAD: usize = 160;
    let chunk_size = (MAX_FRAME_BYTES.saturating_sub(OVERHEAD) / 4) * 3;
    if chunk_size == 0 {
        return Err(galat_protokol("batas frame terlalu kecil untuk chunk"));
    }
    let count = bytes.len().div_ceil(chunk_size);
    if count == 0 || count > u32::MAX as usize {
        return Err(galat_protokol("jumlah chunk tidak valid"));
    }
    let chunk_id = format!("rpc-{}", CHUNK_SEQUENCE.fetch_add(1, Ordering::Relaxed));
    let mut lines = Vec::with_capacity(count);
    for (index, part) in bytes.chunks(chunk_size).enumerate() {
        let chunk = RpcFrame::RpcChunk {
            chunk_id: chunk_id.clone(),
            index: index as u32,
            count: count as u32,
            byte_length: bytes.len() as u64,
            data: BASE64.encode(part),
        };
        lines.push(
            serde_json::to_string(&chunk)
                .map_err(|err| galat_protokol(format!("gagal serialisasi chunk: {err}")))?,
        );
    }
    Ok(lines)
}

/// Rekonstruksi frame dari baris JSON (termasuk `rpc_chunk`) untuk klien.
#[derive(Debug, Default)]
pub struct RpcFrameDecoder {
    pending: Option<ChunkAssembly>,
}

#[derive(Debug)]
struct ChunkAssembly {
    chunk_id: String,
    count: u32,
    byte_length: u64,
    next: u32,
    buf: Vec<u8>,
}

impl RpcFrameDecoder {
    /// Decoder baru; tidak ada state antar sesi.
    pub fn new() -> Self {
        Self::default()
    }

    /// Proses satu baris dari server.
    ///
    /// Mengembalikan nol frame untuk chunk antara, satu frame untuk frame
    /// lengkap/chunk terakhir. Urutan salah, chunk terputus, `byteLength`/
    /// `count` tidak cocok, base64 tidak valid, atau UTF-8/JSON rusak menjadi
    /// `E_PROTOCOL` — tidak pernah diabaikan diam-diam.
    pub fn push_line(&mut self, line: &str) -> Result<Vec<RpcFrame>, MxError> {
        let value: serde_json::Value = serde_json::from_str(line)
            .map_err(|err| galat_protokol(format!("baris bukan JSON: {err}")))?;
        let tipe = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| galat_protokol("baris tanpa field type"))?;
        if tipe != "rpc_chunk" {
            if self.pending.is_some() {
                return Err(galat_protokol(
                    "frame biasa diterima di tengah urutan rpc_chunk",
                ));
            }
            let frame: RpcFrame = serde_json::from_value(value)
                .map_err(|err| galat_protokol(format!("frame tidak dikenal: {err}")))?;
            return Ok(vec![frame]);
        }
        let chunk: ChunkFields = serde_json::from_value(value)
            .map_err(|err| galat_protokol(format!("rpc_chunk rusak: {err}")))?;
        self.push_chunk(chunk)
    }

    fn push_chunk(&mut self, chunk: ChunkFields) -> Result<Vec<RpcFrame>, MxError> {
        if chunk.count == 0 || chunk.index >= chunk.count {
            return Err(galat_protokol("rpc_chunk: count/index tidak valid"));
        }
        if chunk.byte_length > MAX_REASSEMBLED_FRAME_BYTES as u64 {
            return Err(galat_protokol("rpc_chunk: byteLength melebihi batas"));
        }
        if self.pending.is_none() {
            if chunk.index != 0 {
                return Err(galat_protokol("rpc_chunk: urutan dimulai bukan dari 0"));
            }
            self.pending = Some(ChunkAssembly {
                chunk_id: chunk.chunk_id.clone(),
                count: chunk.count,
                byte_length: chunk.byte_length,
                next: 0,
                buf: Vec::with_capacity(chunk.byte_length as usize),
            });
        }
        let selesai = {
            let pending = self
                .pending
                .as_mut()
                .ok_or_else(|| galat_protokol("rpc_chunk: state hilang"))?;
            if pending.chunk_id != chunk.chunk_id
                || pending.count != chunk.count
                || pending.byte_length != chunk.byte_length
            {
                self.pending = None;
                return Err(galat_protokol(
                    "rpc_chunk: chunk dari urutan lain atau metadata berubah",
                ));
            }
            if pending.next != chunk.index {
                self.pending = None;
                return Err(galat_protokol("rpc_chunk: indeks tidak berurutan"));
            }
            let bytes = BASE64
                .decode(&chunk.data)
                .map_err(|err| galat_protokol(format!("rpc_chunk: base64 rusak: {err}")))?;
            if pending.buf.len() + bytes.len() > pending.byte_length as usize {
                self.pending = None;
                return Err(galat_protokol("rpc_chunk: data melebihi byteLength"));
            }
            pending.buf.extend_from_slice(&bytes);
            pending.next += 1;
            pending.next == pending.count
        };
        if !selesai {
            return Ok(Vec::new());
        }
        let pending = self
            .pending
            .take()
            .ok_or_else(|| galat_protokol("rpc_chunk: state hilang"))?;
        if pending.buf.len() != pending.byte_length as usize {
            return Err(galat_protokol(format!(
                "rpc_chunk: panjang data {} tidak sama dengan byteLength {}",
                pending.buf.len(),
                pending.byte_length
            )));
        }
        let teks = String::from_utf8(pending.buf)
            .map_err(|err| galat_protokol(format!("rpc_chunk: UTF-8 tidak valid: {err}")))?;
        let frame: RpcFrame = serde_json::from_str(&teks)
            .map_err(|err| galat_protokol(format!("rpc_chunk: frame tidak valid: {err}")))?;
        Ok(vec![frame])
    }
}

#[derive(Debug, Deserialize)]
struct ChunkFields {
    chunk_id: String,
    index: u32,
    count: u32,
    byte_length: u64,
    data: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_besar() -> RpcFrame {
        RpcFrame::Event {
            event: RpcEvent::TextDelta {
                text: "x".repeat(1_200_000),
            },
        }
    }

    #[test]
    fn negosiasi_protokol_dibaca() {
        let request: RpcRequest =
            serde_json::from_str(r#"{"type":"negotiate_protocol","id":"p1","protocol_version":2}"#)
                .unwrap();
        assert_eq!(
            request,
            RpcRequest::NegotiateProtocol {
                id: "p1".into(),
                protocol_version: 2
            }
        );
    }

    #[test]
    fn chunk_v2_bolak_balik_dengan_decoder() {
        let frame = frame_besar();
        let lines = encode_frame(&frame, 2).expect("v2 boleh chunk");
        assert!(lines.len() > 1, "frame 1,2 MB harus dipecah");
        for line in &lines {
            assert!(line.len() <= MAX_FRAME_BYTES, "baris {} byte", line.len());
        }
        let mut decoder = RpcFrameDecoder::new();
        let mut hasil = Vec::new();
        for line in &lines {
            hasil.extend(decoder.push_line(line).expect("chunk valid"));
        }
        assert_eq!(hasil.len(), 1);
        assert_eq!(hasil[0], frame);
    }

    #[test]
    fn v1_menolak_frame_besar() {
        let err = encode_frame(&frame_besar(), 1).expect_err("v1 harus menolak");
        assert_eq!(err.code, ErrorCode::Protocol);
        assert!(err.message.contains("v2"), "{}", err.message);
    }

    #[test]
    fn decoder_menolak_urutan_salah() {
        let lines = encode_frame(&frame_besar(), 2).expect("chunk");
        let mut decoder = RpcFrameDecoder::new();
        let err = decoder
            .push_line(&lines[1])
            .expect_err("chunk 1 tidak boleh memulai");
        assert_eq!(err.code, ErrorCode::Protocol);
        assert!(err.message.contains("urutan"), "{}", err.message);
    }

    #[test]
    fn decoder_menolak_frame_biasa_di_tengah_chunk() {
        let lines = encode_frame(&frame_besar(), 2).expect("chunk");
        let mut decoder = RpcFrameDecoder::new();
        assert!(decoder.push_line(&lines[0]).expect("chunk awal").is_empty());
        let err = decoder
            .push_line(r#"{"type":"pong","id":"1"}"#)
            .expect_err("frame biasa di tengah harus ditolak");
        assert!(err.message.contains("tengah"), "{}", err.message);
    }

    #[test]
    fn decoder_menolak_base64_rusak() {
        let mut decoder = RpcFrameDecoder::new();
        let err = decoder
            .push_line(r#"{"type":"rpc_chunk","chunk_id":"c","index":0,"count":1,"byte_length":2,"data":"!!!"}"#)
            .expect_err("base64 rusak harus ditolak");
        assert!(err.message.contains("base64"), "{}", err.message);
    }

    #[test]
    fn decoder_menolak_metadata_chunk_berubah() {
        let mut decoder = RpcFrameDecoder::new();
        let _ = decoder.push_line(
            r#"{"type":"rpc_chunk","chunk_id":"c","index":0,"count":2,"byte_length":10,"data":"aGk="}"#,
        );
        let err = decoder
            .push_line(
                r#"{"type":"rpc_chunk","chunk_id":"c","index":1,"count":3,"byte_length":10,"data":"aGk="}"#,
            )
            .expect_err("count berubah harus ditolak");
        assert!(err.message.contains("metadata"), "{}", err.message);
    }

    #[test]
    fn ready_v2_bisa_dibaca_klien() {
        let line = serde_json::to_string(&RpcFrame::Ready {
            protocol: PROTOCOL_VERSION.to_string(),
            model: "m".to_string(),
            supported_protocol_versions: SUPPORTED_PROTOCOL_VERSIONS.to_vec(),
            max_reassembled_frame_bytes: MAX_REASSEMBLED_FRAME_BYTES as u64,
        })
        .unwrap();
        let mut decoder = RpcFrameDecoder::new();
        let frames = decoder.push_line(&line).expect("ready valid");
        assert!(matches!(
            &frames[0],
            RpcFrame::Ready { supported_protocol_versions, .. }
                if supported_protocol_versions == &[1, 2]
        ));
    }

    #[test]
    fn prompt_dan_ping_dibaca() {
        let prompt: RpcRequest =
            serde_json::from_str(r#"{"type":"prompt","id":"1","message":"halo"}"#).unwrap();
        assert_eq!(
            prompt,
            RpcRequest::Prompt {
                id: "1".into(),
                message: "halo".into()
            }
        );
        let ping: RpcRequest = serde_json::from_str(r#"{"type":"ping","id":"9"}"#).unwrap();
        assert_eq!(ping, RpcRequest::Ping { id: "9".into() });
        let shutdown: RpcRequest = serde_json::from_str(r#"{"type":"shutdown"}"#).unwrap();
        assert_eq!(shutdown, RpcRequest::Shutdown);
    }

    #[test]
    fn approval_request_dan_jawaban() {
        let request: RpcRequest =
            serde_json::from_str(r#"{"type":"approval","id":"a1","decision":"once"}"#).unwrap();
        assert_eq!(
            request,
            RpcRequest::Approval {
                id: "a1".into(),
                decision: "once".into()
            }
        );
        let frame = RpcFrame::ApprovalRequest {
            id: "a1".into(),
            tool: "bash".into(),
            tier: "exec".into(),
            subject: "dir".into(),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"approval_request""#), "{json}");
        assert!(json.contains(r#""tier":"exec""#), "{json}");
    }

    #[test]
    fn answer_dan_question() {
        let answer: RpcRequest = serde_json::from_str(r#"{"type":"answer","text":"ya"}"#).unwrap();
        assert_eq!(
            answer,
            RpcRequest::Answer {
                id: None,
                text: "ya".into()
            }
        );
        let frame = RpcFrame::Question {
            question: "lanjut?".into(),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"question""#), "{json}");
        assert!(json.contains("lanjut?"), "{json}");
    }

    #[test]
    fn usage_request_dan_frame() {
        let request: RpcRequest = serde_json::from_str(r#"{"type":"usage","id":"7"}"#).unwrap();
        assert_eq!(request, RpcRequest::Usage { id: "7".into() });
        let frame = RpcFrame::Usage {
            id: "7".into(),
            input_tokens: 10,
            output_tokens: 4,
            cached_tokens: 2,
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"usage""#), "{json}");
        assert!(json.contains(r#""input_tokens":10"#), "{json}");
    }

    #[test]
    fn abort_dengan_dan_tanpa_id() {
        let with: RpcRequest = serde_json::from_str(r#"{"type":"abort","id":"2"}"#).unwrap();
        assert_eq!(
            with,
            RpcRequest::Abort {
                id: Some("2".into())
            }
        );
        let without: RpcRequest = serde_json::from_str(r#"{"type":"abort"}"#).unwrap();
        assert_eq!(without, RpcRequest::Abort { id: None });
    }

    #[test]
    fn varian_tak_dikenal_ditolak() {
        let result: Result<RpcRequest, _> = serde_json::from_str(r#"{"type":"teleport"}"#);
        assert!(result.is_err());
        let result: Result<RpcRequest, _> = serde_json::from_str(r#"{"id":"1"}"#);
        assert!(result.is_err(), "tanpa type harus error");
    }

    #[test]
    fn field_tambahan_diabaikan() {
        let request: RpcRequest =
            serde_json::from_str(r#"{"type":"ping","id":"1","future":true}"#).unwrap();
        assert_eq!(request, RpcRequest::Ping { id: "1".into() });
    }

    #[test]
    fn event_internal_diproyeksikan() {
        let event = AgentEvent::MessageUpdate(ProviderEvent::TextDelta { text: "hi".into() });
        assert_eq!(
            RpcEvent::from_agent(&event),
            Some(RpcEvent::TextDelta { text: "hi".into() })
        );
        let tool = AgentEvent::ToolExecutionStart {
            tool_call_id: "c1".into(),
            tool_name: "bash".into(),
            preview: "dir".into(),
        };
        assert_eq!(
            RpcEvent::from_agent(&tool),
            Some(RpcEvent::ToolStart {
                tool_call_id: "c1".into(),
                tool_name: "bash".into(),
                preview: "dir".into()
            })
        );
        let hidden = AgentEvent::MessageEnd {
            message: crate::message::AssistantMessage::text("x", StopReason::Stop),
        };
        assert_eq!(RpcEvent::from_agent(&hidden), None);
    }

    #[test]
    fn frame_terserialisasi_dengan_tag_type() {
        let ready = RpcFrame::Ready {
            protocol: PROTOCOL_VERSION.into(),
            model: "m".into(),
            supported_protocol_versions: SUPPORTED_PROTOCOL_VERSIONS.to_vec(),
            max_reassembled_frame_bytes: MAX_REASSEMBLED_FRAME_BYTES as u64,
        };
        let json = serde_json::to_string(&ready).unwrap();
        assert!(json.contains(r#""type":"ready""#), "{json}");
        let result = RpcFrame::Result {
            id: "1".into(),
            stop_reason: StopReason::Stop,
            turns: 2,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""type":"result""#), "{json}");
        assert!(json.contains(r#""stop_reason":"stop""#), "{json}");
        let event = RpcFrame::Event {
            event: RpcEvent::TextDelta { text: "a".into() },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"event""#), "{json}");
        assert!(json.contains(r#""event":"text_delta""#), "{json}");
    }
}
