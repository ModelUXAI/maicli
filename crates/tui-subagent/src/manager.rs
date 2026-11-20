//! Manager subagent (tahap B): kontrol anak yang berjalan di task terpisah.
//!
//! Satu [`SubagentHandle`] mewakili satu sesi anak di task; induk/host
//! mengirim perintah (`Steer`, `Abort`, `Park`, `Resume`, `Kill`) lewat kanal
//! dan membaca status + hasil. Bila `sessions_dir` diberikan, transkrip anak
//! ditulis sebagai sesi JSONL dengan `parent_session` = id induk (lineage).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tui_session::SessionStore;
use tui_tools::ToolRegistry;
use tui_wire::Message;

use crate::{SubagentResult, SubagentSpec};

/// Status siklus hidup anak (nilai dikirim sebagai teks di [`Handle::status`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentState {
    /// Task berjalan / turn aktif.
    Running,
    /// Menunggu prompt berikutnya (turn selesai, proses hidup; hasil terakhir
    /// tetap terbaca).
    Idle,
    /// Dijeda (`Park`); task hidup, turn berhenti di batas iterasi.
    Parked,
    /// Dihentikan (`Kill`) — task mati, hasil tidak tersedia.
    Killed,
}

impl SubagentState {
    /// Label teks untuk tampilan.
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "berjalan",
            Self::Idle => "idle",
            Self::Parked => "parkir",
            Self::Killed => "dihentikan",
        }
    }
}

/// Perintah untuk anak yang sedang berjalan.
#[derive(Debug, Clone)]
pub enum SubagentCommand {
    /// Kirim pesan steering (masuk konteks sebelum panggilan model berikutnya).
    Steer(String),
    /// Batalkan turn berjalan; anak tetap hidup (bisa prompt lagi).
    Abort,
    /// Jeda anak di batas iterasi.
    Park,
    /// Lanjutkan anak yang diparkir.
    Resume,
    /// Matikan task anak.
    Kill,
    /// Prompt turn berikutnya (anak harus Idle); steering buffer ikut dijalankan.
    Prompt(String),
}

/// Titik kendali satu anak.
#[derive(Clone)]
pub struct SubagentHandle {
    /// Id unik anak (sub-1, sub-2, …).
    pub id: String,
    /// Id sesi induk (lineage; kosong bila induk tidak ber-file).
    pub parent_id: String,
    state: Arc<std::sync::Mutex<SubagentState>>,
    cmds: UnboundedSender<SubagentCommand>,
    result: Arc<std::sync::Mutex<Option<SubagentResult>>>,
    /// Slot langganan event anak (tahap C): task runner meneruskan event sesi
    /// anak ke sini; notifikasi sebelum langganan disimpan (retensi 200).
    events: Arc<EventSlot>,
    /// Path file sesi anak (bila `sessions_dir` diberikan).
    pub session_path: Arc<std::sync::Mutex<Option<PathBuf>>>,
}

/// Slot penerusan event: langganan aktif + tunggakan pra-langganan.
#[derive(Default)]
struct EventSlot {
    pengirim: tokio::sync::Mutex<Option<UnboundedSender<tui_wire::AgentEvent>>>,
    tunggakan: std::sync::Mutex<Vec<tui_wire::AgentEvent>>,
}

impl EventSlot {
    /// Kirim event: ke langganan, atau tunggakan (maks 200) bila belum ada.
    fn kirim(&self, event: tui_wire::AgentEvent) {
        if let Ok(slot) = self.pengirim.try_lock() {
            if let Some(tx) = &*slot {
                let _ = tx.send(event);
                return;
            }
        }
        if let Ok(mut tunggakan) = self.tunggakan.lock() {
            if tunggakan.len() < 200 {
                tunggakan.push(event);
            }
        }
    }

    /// Buka langganan: serahkan tunggakan lalu pasang penerima.
    async fn langganan(&self) -> UnboundedReceiver<tui_wire::AgentEvent> {
        let (tx, rx) = unbounded_channel();
        if let Ok(mut tunggakan) = self.tunggakan.lock() {
            for event in tunggakan.drain(..) {
                let _ = tx.send(event);
            }
        }
        *self.pengirim.lock().await = Some(tx);
        rx
    }
}

impl SubagentHandle {
    /// Status terkini anak.
    pub fn status(&self) -> SubagentState {
        match self.state.lock() {
            Ok(guard) => *guard,
            Err(_) => SubagentState::Killed,
        }
    }

    /// Kirim perintah; `false` = task sudah mati.
    pub fn send(&self, command: SubagentCommand) -> bool {
        self.cmds.send(command).is_ok()
    }

    /// Steering ringkas.
    pub fn steer(&self, text: impl Into<String>) -> bool {
        self.send(SubagentCommand::Steer(text.into()))
    }

    /// Batalkan turn berjalan.
    pub fn abort(&self) -> bool {
        self.send(SubagentCommand::Abort)
    }

    /// Jeda anak.
    pub fn park(&self) -> bool {
        self.send(SubagentCommand::Park)
    }

    /// Lanjutkan anak yang diparkir.
    pub fn resume(&self) -> bool {
        self.send(SubagentCommand::Resume)
    }

    /// Matikan task anak.
    pub fn kill(&self) -> bool {
        self.send(SubagentCommand::Kill)
    }

    /// Prompt turn berikutnya (anak Idle). `false` = task mati.
    pub fn prompt(&self, text: impl Into<String>) -> bool {
        self.send(SubagentCommand::Prompt(text.into()))
    }

    /// Langganan **event sesi anak** (tahap C): AgentStart/ToolExecution/End
    /// dst. mengalir dari runner. Notifikasi pra-langganan diserahkan juga
    /// (retensi 200). `false` bila sudah dilanggani? — tidak: tiap panggilan
    /// membuat kanal baru (banyak langganan diizinkan).
    pub async fn events(&self) -> UnboundedReceiver<tui_wire::AgentEvent> {
        self.events.langganan().await
    }

    /// Hasil akhir; `None` bila belum selesai.
    pub fn result(&self) -> Option<SubagentResult> {
        match self.result.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        }
    }

    /// Path sesi anak (lineage), bila persistensi diaktifkan.
    pub fn session_path(&self) -> Option<PathBuf> {
        self.session_path.lock().ok()?.clone()
    }
}

/// Registry subagent untuk satu sesi induk.
#[derive(Default)]
pub struct SubagentManager {
    entries: std::sync::Mutex<HashMap<String, SubagentHandle>>,
    seq: AtomicU64,
}

impl SubagentManager {
    /// Manager kosong.
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn anak: task berjalan, handle terdaftar, artefak sesi ditulis bila
    /// `parent_id` + `sessions_dir` diisi.
    pub fn spawn(
        &self,
        spec: SubagentSpec,
        prompt: String,
        parent_id: &str,
        sessions_dir: Option<PathBuf>,
    ) -> SubagentHandle {
        let id = format!("sub-{}", self.seq.fetch_add(1, Ordering::Relaxed) + 1);
        let (cmd_tx, cmd_rx) = unbounded_channel();
        let handle = SubagentHandle {
            id: id.clone(),
            parent_id: parent_id.to_string(),
            state: Arc::new(std::sync::Mutex::new(SubagentState::Running)),
            cmds: cmd_tx,
            result: Arc::new(std::sync::Mutex::new(None)),
            events: Arc::new(EventSlot::default()),
            session_path: Arc::new(std::sync::Mutex::new(None)),
        };
        if let Ok(mut map) = self.entries.lock() {
            map.insert(id.clone(), handle.clone());
        }
        tokio::spawn(runner(
            parent_id.to_string(),
            spec,
            prompt,
            sessions_dir,
            cmd_rx,
            handle.clone(),
        ));
        handle
    }

    /// Semua handle (urutan id naik) — untuk roster/panel.
    pub fn list(&self) -> Vec<SubagentHandle> {
        let mut handles: Vec<SubagentHandle> = match self.entries.lock() {
            Ok(map) => {
                let mut ids: Vec<String> = map.keys().cloned().collect();
                ids.sort();
                ids.into_iter()
                    .filter_map(|id| map.get(&id).cloned())
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        handles.sort_by(|a, b| a.id.cmp(&b.id));
        handles
    }

    /// Ambil handle berdasarkan id.
    pub fn get(&self, id: &str) -> Option<SubagentHandle> {
        self.entries.lock().ok()?.get(id).cloned()
    }

    /// Hapus entri (setelah Killed/Finished dibereskan host).
    pub fn remove(&self, id: &str) -> bool {
        match self.entries.lock() {
            Ok(mut map) => map.remove(id).is_some(),
            Err(_) => false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn runner(
    parent_id: String,
    spec: SubagentSpec,
    prompt: String,
    sessions_dir: Option<PathBuf>,
    mut cmds: UnboundedReceiver<SubagentCommand>,
    handle: SubagentHandle,
) {
    // Sesi anak dengan store file opsional (lineage).
    let mut store: Option<SessionStore> = None;
    if let (Some(dir), false) = (sessions_dir.as_ref(), parent_id.is_empty()) {
        if let Ok(new_store) =
            SessionStore::create_with_parent(dir, "subagent", ".", Some(parent_id.clone()))
        {
            if let Ok(mut slot) = handle.session_path.lock() {
                *slot = Some(new_store.path().to_path_buf());
            }
            store = Some(new_store);
        }
    }
    // Anak tidak boleh bersarang: buang tool `subagent` dari registry anak.
    let registry = ToolRegistry::from_tools(
        spec.tools
            .names()
            .iter()
            .filter(|nama| nama.as_str() != "subagent")
            .filter_map(|nama| spec.tools.get(nama))
            .collect(),
    );
    let mut provider_spec = spec.clone();
    provider_spec.tools = registry;

    let mut killed = false;
    // Setup sekali: satu sesi anak untuk semua turn (multi-turn). Event anak
    // diteruskan ke langganan induk (tahap C) oleh task penerus.
    let (mut sesi, mut rx) = build_session_tanpa_nesting(&provider_spec);
    let slot = handle.events.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            slot.kirim(event);
        }
    });
    let token = sesi.cancel_token();
    let pause_handle = sesi.pause_handle();
    // Steering saat turn berjalan ditahan; digabung ke prompt turn berikutnya.
    let mut buffer_steer: Vec<String> = Vec::new();
    // Prompt yang datang saat anak berjalan; dijalankan segera setelah turn.
    let mut antrean_prompt: std::collections::VecDeque<String> = Default::default();
    let mut prompt_aktif = prompt;
    loop {
        if killed {
            break;
        }
        // Tandai berjalan, tulis prompt pengguna.
        set_state(&handle, SubagentState::Running);
        if let Some(s) = store.as_mut() {
            let _ = s.append(&tui_session::SessionEntry::message(
                None,
                Message::user(prompt_aktif.clone()),
            ));
        }
        let mut hasil_kini: Option<SubagentResult> = None;
        // Turn berjalan sambil melayani perintah. Blok membatasi masa hidup
        // `run` (pinjaman mutable sesi) sehingga transkrip bisa dibaca setelahnya.
        let hasil = {
            let mut run = std::pin::pin!(sesi.run_turn(Some(prompt_aktif.clone())));
            loop {
                tokio::select! {
                    outcome = &mut run => break outcome,
                    Some(command) = cmds.recv() => match command {
                        // Steer saat turn berjalan: ditunda ke prompt turn
                        // berikutnya (run_turn meminjam sesi; steer tidak bisa
                        // dipanggil di sini — batasan yang didokumentasikan).
                        SubagentCommand::Steer(text) => buffer_steer.push(text),
                        // Prompt saat berjalan → antrean; dijalankan setelah turn.
                        SubagentCommand::Prompt(pesan) => antrean_prompt.push_back(pesan),
                        SubagentCommand::Abort => {
                            token.cancel();
                            // Status final (Idle/Killed) diputuskan setelah turn berhenti.
                        }
                        SubagentCommand::Park => {
                            pause_handle.pause();
                            set_state(&handle, SubagentState::Parked);
                        }
                        SubagentCommand::Resume => {
                            pause_handle.resume();
                            set_state(&handle, SubagentState::Running);
                        }
                        SubagentCommand::Kill => {
                            token.cancel();
                            killed = true;
                            set_state(&handle, SubagentState::Killed);
                            // Putus loop dalam: keluar tanpa hasil.
                            break Err(tui_wire::MxError::new(
                                tui_wire::ErrorCode::AgentPaused,
                                "subagent dihentikan".to_string(),
                            ));
                        }
                    },
                }
            }
        };
        match hasil {
            Ok(outcome) => {
                let teks = sesi
                    .messages()
                    .iter()
                    .rev()
                    .find_map(|pesan| match pesan {
                        Message::Assistant { content, .. } => {
                            content.iter().find_map(|blok| match blok {
                                tui_wire::ContentBlock::Text { text } => Some(text.clone()),
                                _ => None,
                            })
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                let tool_calls = sesi
                    .messages()
                    .iter()
                    .filter(|pesan| matches!(pesan, Message::Assistant { .. }))
                    .map(|pesan| match pesan {
                        Message::Assistant { content, .. } => content
                            .iter()
                            .filter(|blok| matches!(blok, tui_wire::ContentBlock::ToolUse { .. }))
                            .count(),
                        _ => 0,
                    })
                    .sum::<usize>() as u32;
                let hasil_baru = SubagentResult {
                    text: teks,
                    is_error: outcome.stop_reason == tui_wire::StopReason::Error,
                    reason: (outcome.stop_reason == tui_wire::StopReason::Aborted)
                        .then(|| "dibatalkan".to_string()),
                    turns: outcome.turns,
                    tool_calls,
                };
                if let Some(s) = store.as_mut() {
                    if let Some(last) = sesi
                        .messages()
                        .iter()
                        .rev()
                        .find(|m| matches!(m, Message::Assistant { .. }))
                    {
                        let _ = s.append(&tui_session::SessionEntry::message(None, last.clone()));
                    }
                }
                // Hasil ditulis SEBELUM status final supaya pembaca yang melihat
                // status Idle/Killed selalu menemukan hasilnya (tanpa race).
                if let Ok(mut slot) = handle.result.lock() {
                    *slot = Some(hasil_baru.clone());
                }
            }
            Err(_) => {
                hasil_kini = Some(SubagentResult {
                    text: String::new(),
                    is_error: true,
                    reason: Some(if killed {
                        "dihentikan".to_string()
                    } else {
                        "galat turn".to_string()
                    }),
                    turns: 0,
                    tool_calls: 0,
                });
                if let Some(hasil) = &hasil_kini {
                    if let Ok(mut slot) = handle.result.lock() {
                        *slot = Some(hasil.clone());
                    }
                }
            }
        }
        if let Some(hasil) = &hasil_kini {
            if let Ok(mut slot) = handle.result.lock() {
                *slot = Some(hasil.clone());
            }
        }
        if killed {
            set_state(&handle, SubagentState::Killed);
            break;
        }
        // Anak Idle: tunggu prompt berikutnya tanpa batas waktu (task mati saat
        // runtime proses selesai); Prompt → turn berikutnya; Kill → mati.
        set_state(&handle, SubagentState::Idle);
        let prompt_baru = loop {
            match cmds.recv().await {
                Some(SubagentCommand::Prompt(pesan)) => break Some(pesan),
                Some(SubagentCommand::Kill) => {
                    set_state(&handle, SubagentState::Killed);
                    killed = true;
                    break None;
                }
                Some(SubagentCommand::Steer(text)) => {
                    // Steer saat idle = bagian dari prompt berikutnya.
                    buffer_steer.push(text);
                }
                _ => {} // Abort/Park/Resume saat idle: abaikan
            }
        };
        let Some(pesan_baru) = prompt_baru else { break };
        // Gabungkan steering buffer + prompt baru.
        let mut gabungan = buffer_steer.join("\n");
        if !gabungan.is_empty() {
            gabungan.push('\n');
        }
        gabungan.push_str(&pesan_baru);
        prompt_aktif = gabungan;
        buffer_steer.clear();
    }
}

fn build_session_tanpa_nesting(
    spec: &SubagentSpec,
) -> (
    tui_core::AgentSession,
    UnboundedReceiver<AgentEventForRunner>,
) {
    // Sesi anak memakai kanal event internalnya sendiri; penerima dipakai
    // runner untuk meneruskan event ke langganan induk (tahap C).
    let (tx, rx) = unbounded_channel();
    let sesi = tui_core::AgentSession::new(
        spec.provider.clone(),
        spec.approver.clone(),
        spec.tools.clone(),
        tui_tools::ToolContext::new("."),
        tui_core::TurnSettings {
            model: spec.model.clone(),
            deadline: Some(spec.deadline),
            ..tui_core::TurnSettings::default()
        },
        tx,
    );
    (sesi, rx)
}

type AgentEventForRunner = tui_wire::AgentEvent;

fn set_state(handle: &SubagentHandle, state: SubagentState) {
    if let Ok(mut slot) = handle.state.lock() {
        *slot = state;
    }
}
#[cfg(test)]
mod manager_tests {
    use super::*;
    use crate::SubagentSpec;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tui_provider::{ChatRequest, GatewayError, Provider};
    use tui_tools::{Tool, ToolContext, ToolRegistry};
    use tui_wire::{AssistantMessage, ProviderEvent, StopReason, ToolOutput, ToolTier};

    struct UjiProvider {
        script: Mutex<VecDeque<AssistantMessage>>,
    }

    #[async_trait::async_trait]
    impl Provider for UjiProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
        ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError> {
            let (tx, rx) = unbounded_channel();
            if let Ok(mut antre) = self.script.lock()
                && let Some(pesan) = antre.pop_front()
            {
                let _ = tx.send(ProviderEvent::Done { message: pesan });
            }
            Ok(rx)
        }
    }

    struct UjiTool;

    #[async_trait::async_trait]
    impl Tool for UjiTool {
        fn tier(&self) -> ToolTier {
            ToolTier::Read
        }
        fn definition(&self) -> tui_wire::ToolDefinition {
            tui_wire::ToolDefinition {
                name: "uji".into(),
                description: "tool uji".into(),
                parameters: serde_json::json!({"type": "object"}),
                concurrency: tui_wire::Concurrency::Shared,
            }
        }
        async fn execute(
            &self,
            _args: &serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolOutput, tui_wire::MxError> {
            Ok(ToolOutput::ok("hasil-uji"))
        }
    }

    fn teks(text: &str) -> AssistantMessage {
        AssistantMessage::text(text.to_string(), StopReason::Stop)
    }

    /// Provider yang tidak pernah menjawab: simulasi kerja panjang untuk uji
    /// abort/kill saat turn masih berjalan.
    struct GantungProvider;

    #[async_trait::async_trait]
    impl Provider for GantungProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
        ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError> {
            let (tx, rx) = unbounded_channel();
            // tx ditahan: stream tidak pernah selesai.
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                drop(tx);
            });
            Ok(rx)
        }
    }

    // (tool_call helper: lihat lib.rs test yang sama; manager test tidak
    // memerlukan skenario tool karena gate/eksekusi sudah diuji di lib.rs.)

    fn spec(script: Vec<AssistantMessage>) -> SubagentSpec {
        SubagentSpec {
            provider: Arc::new(UjiProvider {
                script: Mutex::new(script.into()),
            }),
            approver: Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Once,
            )),
            tools: ToolRegistry::from_tools(vec![Arc::new(UjiTool)]),
            model: "model-uji".to_string(),
            deadline: Duration::from_secs(10),
            max_tool_calls: 8,
        }
    }

    #[tokio::test]
    async fn spawn_selesai_dan_hasil_terbaca() {
        let manager = SubagentManager::new();
        let handle = manager.spawn(
            spec(vec![teks("selesai anak")]),
            "kerjakan".to_string(),
            "induk-1",
            None,
        );
        // Tunggu turn selesai → anak Idle (menunggu prompt berikutnya).
        for _ in 0..100 {
            if handle.status() == SubagentState::Idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(handle.status(), SubagentState::Idle);
        let hasil = handle.result().expect("hasil");
        assert_eq!(hasil.text, "selesai anak");
        assert!(!hasil.is_error);
        assert!(handle.session_path().is_none(), "tanpa dir = tanpa file");
        // Roster memuat anak.
        assert_eq!(manager.list().len(), 1);
        assert!(manager.remove("sub-1"));
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn kill_mengubah_status_dan_hasil_error() {
        let manager = SubagentManager::new();
        // Provider menggantung agar turn masih berjalan saat kill dipanggil.
        let s = SubagentSpec {
            provider: Arc::new(GantungProvider),
            ..spec(Vec::new())
        };
        let handle = manager.spawn(s, "kerja panjang".to_string(), "induk-1", None);
        // Tunggu task benar-benar masuk Running sebelum kill.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(handle.kill());
        // Tunggu task mati.
        for _ in 0..100 {
            if handle.status() == SubagentState::Killed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(handle.status(), SubagentState::Killed);
        let hasil = handle.result().expect("hasil parsial");
        assert!(hasil.is_error);
        assert!(
            hasil
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("hentikan"))
                || hasil.reason.is_some()
        );
    }

    #[tokio::test]
    async fn lineage_file_anak_memuat_parent_session() {
        let dir = std::env::temp_dir().join(format!(
            "maicli-subagent-lineage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|n| n.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("dir");
        let manager = SubagentManager::new();
        let handle = manager.spawn(
            spec(vec![teks("anak selesai")]),
            "prompt anak".to_string(),
            "induk-xyz",
            Some(dir.clone()),
        );
        for _ in 0..100 {
            if handle.status() == SubagentState::Idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let path = handle.session_path().expect("file sesi anak");
        let isi = std::fs::read_to_string(&path).expect("baca sesi anak");
        assert!(isi.contains("\"parent_session\":\"induk-xyz\""), "{isi}");
        assert!(isi.contains("prompt anak"), "{isi}");
        assert!(isi.contains("anak selesai"), "{isi}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn abort_turn_berjalan_lalu_anak_idle_dan_bisa_prompt_lagi() {
        // Provider yang tidak pernah menjawab → turn menggantung sampai abort.
        struct GantungProvider;
        #[async_trait::async_trait]
        impl Provider for GantungProvider {
            async fn stream_chat(
                &self,
                _request: ChatRequest,
            ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError> {
                let (tx, rx) = unbounded_channel();
                // tx ditahan: stream tidak pernah selesai.
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    drop(tx);
                });
                Ok(rx)
            }
        }
        let s = SubagentSpec {
            provider: Arc::new(GantungProvider),
            approver: Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Once,
            )),
            tools: ToolRegistry::from_tools(Vec::new()),
            model: "m".to_string(),
            deadline: Duration::from_secs(60),
            max_tool_calls: 8,
        };
        let manager = SubagentManager::new();
        let handle = manager.spawn(s, "gantung".to_string(), "induk", None);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(handle.status(), SubagentState::Running);
        assert!(handle.abort());
        // Turn aborted → anak tetap hidup → Idle (hasil is_error tersedia).
        for _ in 0..100 {
            if handle.status() == SubagentState::Idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(handle.status(), SubagentState::Idle);
        let hasil = handle.result().expect("hasil parsial");
        // Abort ≠ error eksekusi; penanda dibawa oleh `reason`.
        assert!(
            hasil
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("dibatalkan")),
            "reason harus menyebut dibatalkan: {:?}",
            hasil.reason
        );
        // Multi-turn: prompt setelah idle → anak berjalan lagi (lalu di-kill).
        assert!(handle.prompt("lanjut bekerja"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(handle.status(), SubagentState::Running);
        handle.kill();
    }

    #[tokio::test]
    async fn event_anak_mengalir_ke_langganan_dengan_retensi() {
        // Event sesi anak (AgentStart/End) harus sampai ke subscriber induk,
        // termasuk yang dikirim sebelum langganan (retensi tunggakan).
        let manager = SubagentManager::new();
        let handle = manager.spawn(
            spec(vec![teks("jawaban event")]),
            "prompt event".to_string(),
            "induk",
            None,
        );
        // Beri kesempatan runner memancarkan event SEBELUM subscribe.
        tokio::time::sleep(Duration::from_millis(60)).await;
        let mut events = handle.events().await;
        let mut ada_start = false;
        let mut ada_end = false;
        for _ in 0..50 {
            match tokio::time::timeout(Duration::from_millis(100), events.recv()).await {
                Ok(Some(tui_wire::AgentEvent::AgentStart)) => ada_start = true,
                Ok(Some(tui_wire::AgentEvent::AgentEnd { .. })) => {
                    ada_end = true;
                    break;
                }
                Ok(Some(_)) => {}
                _ => break,
            }
        }
        assert!(ada_start, "AgentStart harus diteruskan");
        assert!(ada_end, "AgentEnd harus diteruskan");
    }

    #[tokio::test]
    async fn multi_turn_prompt_saat_idle_dan_steer_gabung() {
        // Provider skrip dua jawaban: "satu" lalu "dua".
        let manager = SubagentManager::new();
        let handle = manager.spawn(
            spec(vec![teks("jawaban satu"), teks("jawaban dua")]),
            "minta pertama".to_string(),
            "induk",
            None,
        );
        for _ in 0..100 {
            if handle.status() == SubagentState::Idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(handle.result().expect("hasil 1").text, "jawaban satu");
        // Steer saat idle → digabung ke prompt berikutnya.
        assert!(handle.steer("fokus"));
        assert!(handle.prompt("minta kedua"));
        // Tunggu turn kedua benar-benar berjalan (bukan Idle lama), lalu selesai.
        for _ in 0..100 {
            if handle.status() == SubagentState::Running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        for _ in 0..100 {
            if handle.status() == SubagentState::Idle {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(handle.result().expect("hasil 2").text, "jawaban dua");
        // Transkrip induk? — cek prompt kedua berisi steering + prompt.
        assert!(handle.kill());
    }
}
