//! SDK embedding Rust untuk sesi agent `maicli`.
//!
//! Crate ini adalah permukaan publik yang dijanjikan stabil (lihat
//! `docs/sdk-rust-v1.md`): embedder tidak perlu menyentuh `tui-core` langsung,
//! tetapi tetap mendapat akses ke event dua tingkat, hasil turn, dan kontrol
//! sesi (abort, pause, model).
//!
//! Prinsipnya sama dengan SDK omp: **provide to override, omit to discover** —
//! embedder wajib memberikan `Provider` dan `Approver`; tool default adalah
//! registry v1, dan tool set boleh diganti.
//!
//! ```
//! # async fn contoh() -> Result<(), tui_wire::MxError> {
//! use std::sync::Arc;
//! use tui_sdk::SessionBuilder;
//!
//! // Provider dan approver disediakan embedder; di sini hanya bentuk API-nya.
//! // let (mut session, mut events) = SessionBuilder::new(".")
//! //     .provider(provider)
//! //     .approver(approver)
//! //     .build()?;
//! // let hasil = session.prompt("halo").await?;
//! // while let Some(event) = events.next().await { /* ... */ }
//! # Ok(())
//! # }
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tui_approval::Approver;
use tui_core::{AgentSession, TurnOutcome, TurnSettings};
use tui_provider::Provider;
pub use tui_provider::{ChatRequest, GatewayError};
use tui_tools::{CancelToken, ToolContext, ToolRegistry};
use tui_wire::{AgentEvent, ErrorCode, Message, MxError, Usage};

pub use tui_approval::{ApprovalRequest, Decision};
pub use tui_wire::ProviderEvent;

/// Kanal event sesi.
///
/// Satu sesi satu kanal; event berurutan sesuai yang dikirim agent. Event
/// `AgentStart`/`AgentEnd` membingkai satu `prompt`.
pub struct EventStream {
    rx: UnboundedReceiver<AgentEvent>,
}

impl EventStream {
    /// Tunggu event berikutnya; `None` berarti sesi sudah tidak mengirim event.
    pub async fn next(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }

    /// Ambil event yang sudah antre tanpa menunggu.
    pub fn try_next(&mut self) -> Option<AgentEvent> {
        self.rx.try_recv().ok()
    }
}

/// Builder sesi.
///
/// `provider` dan `approver` wajib diisi; sisanya default aman:
/// tool registry v1, `TurnSettings::default()`, dan `interactive = false`
/// (tool `ask` tidak akan meminta UI).
#[derive(Clone)]
pub struct SessionBuilder {
    cwd: PathBuf,
    provider: Option<Arc<dyn Provider>>,
    approver: Option<Arc<dyn Approver>>,
    tools: Option<ToolRegistry>,
    settings: TurnSettings,
    interactive: bool,
}

impl SessionBuilder {
    /// Builder dengan direktori kerja `cwd`.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            provider: None,
            approver: None,
            tools: None,
            settings: TurnSettings::default(),
            interactive: false,
        }
    }

    /// Provider gateway (wajib).
    pub fn provider(mut self, provider: Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Approver gate tool (wajib).
    pub fn approver(mut self, approver: Arc<dyn Approver>) -> Self {
        self.approver = Some(approver);
        self
    }

    /// Registry tool; default `ToolRegistry::v1()`.
    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Setelan turn (model, system prompt, batas token, deadline).
    pub fn settings(mut self, settings: TurnSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Model untuk turn berikutnya.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.settings.model = model.into();
        self
    }

    /// Apakah sesi punya antarmuka untuk menjawab tool `ask`.
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    /// Bangun sesi dan kanal eventnya.
    ///
    /// # Errors
    ///
    /// `E_CONFIG` bila `provider` atau `approver` tidak diisi.
    pub fn build(self) -> Result<(Session, EventStream), MxError> {
        let provider = self
            .provider
            .ok_or_else(|| galat_config("provider wajib diisi: panggil .provider(...)"))?;
        let approver = self
            .approver
            .ok_or_else(|| galat_config("approver wajib diisi: panggil .approver(...)"))?;
        let tools = self.tools.unwrap_or_else(ToolRegistry::v1);
        let mut tool_ctx = ToolContext::new(&self.cwd);
        tool_ctx.interactive = self.interactive;
        let (tx, rx) = unbounded_channel();
        let inner = AgentSession::new(provider, approver, tools, tool_ctx, self.settings, tx);
        Ok((Session { inner }, EventStream { rx }))
    }
}

fn galat_config(pesan: &str) -> MxError {
    MxError::new(ErrorCode::Config, pesan.to_string())
}

/// Registry sesi paralel dalam satu proses (adopsi omp `AgentRegistry`).
///
/// Setiap sesi dibangun dari builder **sendiri** (provider/approver per sesi);
/// registry hanya mengelola id + akses. Sesi di-drop oleh embedder lewat
/// [`remove`](Self::remove).
///
/// ```
/// use std::sync::Arc;
/// use tui_sdk::AgentRegistry;
///
/// let mut registry = AgentRegistry::baru();
/// registry.daftarkan(
///     "utama",
///     tui_sdk::SessionBuilder::new(".")
///         // .provider(...).approver(...) wajib sebelum build
/// );
/// assert!(registry.sesi("utama"), "id terdaftar");
/// assert!(registry.hapus("utama"));
/// ```
pub struct AgentRegistry {
    builders: std::collections::BTreeMap<String, SessionBuilder>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::baru()
    }
}

impl AgentRegistry {
    /// Registry kosong.
    pub fn baru() -> Self {
        Self {
            builders: std::collections::BTreeMap::new(),
        }
    }

    /// Daftarkan builder di bawah id (menimpa bila id sama).
    pub fn daftarkan(&mut self, id: impl Into<String>, builder: SessionBuilder) {
        self.builders.insert(id.into(), builder);
    }

    /// Bangun sesi ber-id: mengembalikan sesi + event stream; builder
    /// **tetap** tersimpan sehingga sesi bisa dibangun ulang.
    pub fn bangun(&mut self, id: &str) -> Option<Result<(Session, EventStream), MxError>> {
        self.builders
            .get_mut(id)
            .map(|builder| builder.clone().build())
    }

    /// Cek id terdaftar.
    pub fn sesi(&self, id: &str) -> bool {
        self.builders.contains_key(id)
    }

    /// Daftar id (urut).
    pub fn ids(&self) -> Vec<String> {
        self.builders.keys().cloned().collect()
    }

    /// Hapus pendaftaran id.
    pub fn hapus(&mut self, id: &str) -> bool {
        self.builders.remove(id).is_some()
    }
}

/// Sesi agent yang bisa di-embed.
///
/// Satu `prompt` = satu turn (termasuk tool dan follow-up yang dipicu turn itu).
/// Setelah `AgentEnd` diterima dari [`EventStream`], sesi siap menerima prompt
/// berikutnya.
pub struct Session {
    inner: AgentSession,
}

impl Session {
    /// Jalankan satu turn dengan prompt pengguna.
    ///
    /// # Errors
    ///
    /// Mengembalikan `MxError` turn (mis. gateway gagal, budget habis) —
    /// kegagalan tool tidak membatalkan turn; hasil tool menjadi isi konteks.
    pub async fn prompt(&mut self, text: impl Into<String>) -> Result<TurnOutcome, MxError> {
        self.inner.run_turn(Some(text.into())).await
    }

    /// Batalkan turn yang sedang berjalan (streaming dan tool).
    pub fn abort(&self) {
        self.inner.cancel();
    }

    /// Token pembatalan untuk dipakai pengawas (host tetap memegang kendali).
    pub fn cancel_token(&self) -> CancelToken {
        self.inner.cancel_token()
    }

    /// Model yang dipakai turn berikutnya.
    pub fn model(&self) -> &str {
        self.inner.model()
    }

    /// Ganti model sesi.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.inner.set_model(model);
    }

    /// Transkrip pesan saat ini (tanpa entry sesi yang tidak masuk konteks).
    pub fn messages(&self) -> &[Message] {
        self.inner.messages()
    }

    /// Pemakaian token kumulatif sesi.
    pub fn usage(&self) -> Usage {
        self.inner.usage()
    }

    /// Estimasi token konteks saat ini (heuristik lokal, bukan angka pasti).
    pub fn estimated_tokens(&self) -> u64 {
        self.inner.estimated_tokens()
    }

    /// Jeda sesi: turn berikutnya berhenti di batas iterasi.
    pub fn pause(&self) {
        self.inner.pause();
    }

    /// Lanjutkan sesi yang dijeda.
    pub fn resume(&self) {
        self.inner.resume();
    }

    /// Isi ulang transkrip (dipakai fitur resume/host yang menyimpan sendiri).
    pub fn push_messages(&mut self, messages: Vec<Message>) {
        self.inner.push_messages(messages);
    }

    /// Seed pemakaian token (dipakai resume dari sesi tersimpan).
    pub fn seed_usage(&mut self, usage: Usage) {
        self.inner.seed_usage(usage);
    }

    /// Atur jendela konteks untuk compaction otomatis.
    pub fn set_context_window(&mut self, window: u64) {
        self.inner.set_context_window(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tui_approval::ApprovalRequest;
    use tui_wire::{
        AssistantMessage, ContentBlock, ProviderEvent, StopReason, ToolOutput as WireToolOutput,
    };

    /// Provider skrip: tiap panggilan mengambil satu pesan; tanpa skrip
    /// kanal tetap terbuka supaya turn menggantung (untuk uji abort).
    struct UjiProvider {
        script: Mutex<VecDeque<AssistantMessage>>,
        held: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<ProviderEvent>>>,
    }

    impl UjiProvider {
        fn baru(script: Vec<AssistantMessage>) -> Self {
            Self {
                script: Mutex::new(script.into()),
                held: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for UjiProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
        ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError> {
            let (tx, rx) = unbounded_channel();
            let pesan = self
                .script
                .lock()
                .ok()
                .and_then(|mut antre| antre.pop_front());
            match pesan {
                Some(pesan) => {
                    let _ = tx.send(ProviderEvent::Done { message: pesan });
                }
                None => {
                    if let Ok(mut held) = self.held.lock() {
                        held.push(tx);
                    }
                }
            }
            Ok(rx)
        }
    }

    struct UjiApprover {
        dipanggil: AtomicBool,
    }

    impl UjiApprover {
        fn baru() -> Self {
            Self {
                dipanggil: AtomicBool::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl Approver for UjiApprover {
        async fn decide(&self, _request: ApprovalRequest) -> Decision {
            self.dipanggil.store(true, Ordering::SeqCst);
            Decision::Once
        }
    }

    fn teks(text: &str) -> AssistantMessage {
        AssistantMessage::text(text.to_string(), StopReason::Stop)
    }

    fn tool_call(id: &str, nama: &str, args: &str) -> AssistantMessage {
        AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: nama.to_string(),
                arguments: args.to_string(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
            error: None,
        }
    }

    #[tokio::test]
    async fn prompt_menghasilkan_turn_dan_event_berpasangan() {
        let dir = std::env::temp_dir();
        let provider = Arc::new(UjiProvider::baru(vec![teks("halo dari sdk")]));
        let approver = Arc::new(UjiApprover::baru());
        let (mut sesi, mut events) = SessionBuilder::new(&dir)
            .provider(provider)
            .approver(approver)
            .model("model-uji")
            .build()
            .expect("builder lengkap");

        let hasil = sesi.prompt("hai").await.expect("turn");
        assert_eq!(hasil.stop_reason, StopReason::Stop);
        assert_eq!(hasil.turns, 1);

        let mut urutan = Vec::new();
        while let Some(event) = events.try_next() {
            urutan.push(event);
        }
        assert!(matches!(urutan.first(), Some(AgentEvent::AgentStart)));
        assert!(matches!(urutan.last(), Some(AgentEvent::AgentEnd { .. })));
        assert!(
            urutan
                .iter()
                .any(|event| matches!(event, AgentEvent::MessageEnd { .. })),
            "harus ada message_end"
        );
        assert!(
            sesi.messages()
                .iter()
                .any(|m| matches!(m, Message::Assistant { .. })),
            "transkrip harus memuat jawaban assistant"
        );
    }

    #[tokio::test]
    async fn tool_call_dieksekusi_lewat_gate_dan_turn_selesai() {
        let dir = std::env::temp_dir();
        let provider = Arc::new(UjiProvider::baru(vec![
            tool_call("c1", "todo", "{\"items\":[\"uji sdk\"]}"),
            teks("selesai"),
        ]));
        let approver = Arc::new(UjiApprover::baru());
        let (mut sesi, _events) = SessionBuilder::new(&dir)
            .provider(provider.clone())
            .approver(approver.clone())
            .build()
            .expect("builder lengkap");

        let hasil = sesi.prompt("buat todo").await.expect("turn");
        assert_eq!(hasil.stop_reason, StopReason::Stop);
        assert!(
            approver.dipanggil.load(Ordering::SeqCst),
            "tool harus melewati gate approval"
        );
        assert!(
            sesi.messages()
                .iter()
                .any(|m| matches!(m, Message::ToolResult { .. })),
            "transkrip harus memuat hasil tool"
        );
        // Kanal event tetap hidup setelah turn selesai.
        drop(provider);
        let _ = _events;
    }

    #[tokio::test]
    async fn builder_tanpa_provider_adalah_error_config() {
        let err = match SessionBuilder::new(".")
            .approver(Arc::new(UjiApprover::baru()))
            .build()
        {
            Ok(_) => panic!("harus error"),
            Err(err) => err,
        };
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("provider"), "{}", err.message);
    }

    #[tokio::test]
    async fn registry_sesi_paralel_dan_bangun_ulang() {
        // Dua sesi terdaftar, dibangun, berjalan paralel, lalu satu dihapus.
        let builder_a = SessionBuilder::new(".")
            .provider(Arc::new(UjiProvider::baru(vec![teks("jawaban-a")])))
            .approver(Arc::new(tui_approval::FixedApprover::new(Decision::Once)));
        let builder_b = SessionBuilder::new(".")
            .provider(Arc::new(UjiProvider::baru(vec![teks("jawaban-b")])))
            .approver(Arc::new(tui_approval::FixedApprover::new(Decision::Once)));
        let mut registry = AgentRegistry::baru();
        registry.daftarkan("a", builder_a);
        registry.daftarkan("b", builder_b);
        assert!(registry.sesi("a") && registry.sesi("b"));
        assert_eq!(registry.ids(), vec!["a".to_string(), "b".to_string()]);
        let (mut sesi_a, mut event_a) = registry.bangun("a").expect("terdaftar").expect("build");
        let (mut sesi_b, _event_b) = registry.bangun("b").expect("terdaftar").expect("build");
        // Paralel: dua prompt dijalankan bersamaan.
        let (hasil_a, hasil_b) = tokio::join!(sesi_a.prompt("tanya a"), sesi_b.prompt("tanya b"));
        assert_eq!(hasil_a.unwrap().stop_reason, StopReason::Stop);
        assert_eq!(hasil_b.unwrap().stop_reason, StopReason::Stop);
        let _ = event_a.next().await;
        // Hapus satu; satu tersisa.
        assert!(registry.hapus("a"));
        assert!(!registry.sesi("a"));
        assert!(registry.sesi("b"));
    }
    #[tokio::test]
    async fn abort_menghentikan_turn_yang_menunggu_stream() {
        let dir = std::env::temp_dir();
        let provider = Arc::new(UjiProvider::baru(Vec::new()));
        let approver = Arc::new(UjiApprover::baru());
        let (mut sesi, _events) = SessionBuilder::new(&dir)
            .provider(provider)
            .approver(approver)
            .build()
            .expect("builder lengkap");
        let token = sesi.cancel_token();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            token.cancel();
        });
        let hasil = tokio::time::timeout(std::time::Duration::from_secs(2), sesi.prompt("gantung"))
            .await
            .expect("abort tidak boleh menggantung")
            .expect("turn");
        assert_eq!(hasil.stop_reason, StopReason::Aborted);
    }

    #[test]
    fn tool_output_wire_bisa_dipakai_embedder() {
        // Menjaga re-export tetap tersedia untuk embedder yang menulis tool sendiri.
        let out = WireToolOutput::ok("halo");
        assert!(!out.is_error);
    }
}
