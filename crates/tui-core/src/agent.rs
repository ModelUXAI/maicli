//! Turn loop: dua tingkat (follow-up di luar, tool + steering di dalam).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tui_approval::{ApprovalRequest, Approver, Decision};
use tui_context::{
    Budget, DEFAULT_KEEP_RECENT_TOKENS, DEFAULT_RESERVE_TOKENS, apply_compaction,
    estimate_messages, plan_compaction, plan_shake, prune_tool_results,
};
use tui_provider::{ChatRequest, Provider};
use tui_tools::{CancelToken, Tool, ToolContext, ToolRegistry, unknown_tool};
use tui_wire::{
    AgentEvent, AssistantMessage, ContentBlock, ErrorCode, Message, MxError, ProviderEvent,
    StopReason, Usage,
};
use tui_wire::{Concurrency, ToolTier};

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/system.md");
const COMPACT_INSTRUCTION: &str = include_str!("../../../prompts/compaction.md");

/// Batas compaction per turn agar sesi tidak berputar meringkas tanpa henti.
const MAX_COMPACTIONS_PER_TURN: u32 = 2;

/// Jumlah hasil tool terakhir yang isinya dipertahankan saat pruning.
const KEEP_RECENT_TOOL_RESULTS: usize = 8;

/// Hook observasi setelah satu tool selesai (sukses/gagal/ditolak).
pub type AfterToolHook = Arc<dyn Fn(&str, bool, &str) + Send + Sync>;

/// Hook observasi sebelum eksekusi tool (setelah approval lolos): (nama, preview).
pub type BeforeToolHook = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Pengaturan satu sesi agent.
#[derive(Debug, Clone)]
pub struct TurnSettings {
    /// Model yang dipakai.
    pub model: String,
    /// Instruksi sistem awal.
    pub system_prompt: String,
    /// Batas token output per panggilan model.
    pub max_tokens: Option<u32>,
    /// Deadline satu turn (bukan per panggilan).
    pub deadline: Option<Duration>,
    /// Tingkat penalaran (low/medium/high); dikirim ke gateway bila Some.
    pub effort: Option<String>,
}

impl Default for TurnSettings {
    fn default() -> Self {
        Self {
            model: "deepseek-v4.1-flash".to_string(),
            system_prompt: SYSTEM_PROMPT.to_string(),
            max_tokens: None,
            deadline: Some(Duration::from_secs(900)),
            effort: None,
        }
    }
}

/// Hasil satu pemanggilan `run_turn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnOutcome {
    /// Jumlah turn yang dijalankan (termasuk follow-up).
    pub turns: u32,
    /// Alasan berhenti turn terakhir.
    pub stop_reason: StopReason,
}

/// Tool call yang sudah lolos approval dan siap dieksekusi.
#[derive(Clone)]
struct PreparedTool {
    id: String,
    name: String,
    tool: std::sync::Arc<dyn Tool>,
    args: serde_json::Value,
    ctx: ToolContext,
    concurrency: Concurrency,
}

/// Hasil satu compaction manual: ukuran konteks sebelum/sesudah.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionReport {
    /// Perkiraan token konteks sebelum diringkas.
    pub before_tokens: u64,
    /// Perkiraan token konteks sesudah diringkas.
    pub after_tokens: u64,
    /// Jumlah pesan yang digantikan ringkasan.
    pub summarized: usize,
}

/// Batas bawah token output saat `max_tokens` menyesuaikan sisa window.
///
/// Mengikuti RENCANA C-07 (`fitOutputTokens`): minimum 1.024 token supaya model
/// tidak diminta menjawab dengan jatah yang tidak masuk akal.
pub const MIN_OUTPUT_TOKENS: u32 = 1_024;

/// Sesi agent: konteks, antrean, dan gerbang approval.
/// Hook `syncContextBeforeModelCall`: terima konteks, kembalikan pengganti.
pub type ContextSyncFn = dyn Fn(&[Message]) -> Option<Vec<Message>> + Send + Sync;

pub struct AgentSession {
    provider: Arc<dyn Provider>,
    approver: Arc<dyn Approver>,
    tools: ToolRegistry,
    tool_ctx: ToolContext,
    settings: TurnSettings,
    events: UnboundedSender<AgentEvent>,
    messages: Vec<Message>,
    steering: VecDeque<String>,
    follow_ups: VecDeque<String>,
    busy: Arc<AtomicBool>,
    budget: Budget,
    auto_compact: bool,
    compactions_this_turn: u32,
    cancel: CancelToken,
    paused: Arc<AtomicBool>,
    compactions: Vec<CompactionRecord>,
    approvals: Vec<ApprovalAudit>,
    usage: Usage,
    usage_by_model: Vec<(String, Usage)>,
    before_model_call: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    context_sync: Option<Arc<ContextSyncFn>>,
    /// Hook **sesudah** tool: (nama, is_error, cuplikan hasil) — observasi;
    /// dipanggil di `finish_tool` (semua jalur eksekusi, termasuk paralel).
    after_tool: Option<AfterToolHook>,
    /// Hook **sebelum** eksekusi tool (setelah approval lolos): observasi.
    before_tool: Option<BeforeToolHook>,
    status: crate::status::TurnStatus,
}

impl AgentSession {
    /// Bangun sesi dengan kanal event yang diberikan.
    pub fn new(
        provider: Arc<dyn Provider>,
        approver: Arc<dyn Approver>,
        tools: ToolRegistry,
        tool_ctx: ToolContext,
        settings: TurnSettings,
        events: UnboundedSender<AgentEvent>,
    ) -> Self {
        Self {
            provider,
            approver,
            tools,
            tool_ctx,
            settings,
            events,
            messages: Vec::new(),
            steering: VecDeque::new(),
            follow_ups: VecDeque::new(),
            busy: Arc::new(AtomicBool::new(false)),
            budget: Budget::default(),
            auto_compact: true,
            compactions_this_turn: 0,
            cancel: CancelToken::new(),
            paused: Arc::new(AtomicBool::new(false)),
            compactions: Vec::new(),
            approvals: Vec::new(),
            usage: Usage::default(),
            usage_by_model: Vec::new(),
            before_model_call: None,
            context_sync: None,
            after_tool: None,
            before_tool: None,
            status: crate::status::TurnStatus::Idle,
        }
    }

    /// Konteks percakapan saat ini.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Tambahkan pesan ke konteks tanpa menjalankan turn (mis. resume).
    pub fn push_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Antrekan pesan steering: diproses pada batas iterasi berikutnya.
    pub fn steer(&mut self, text: impl Into<String>) {
        self.steering.push_back(text.into());
    }

    /// Tambahkan pesan hasil resume ke konteks.
    pub fn push_messages(&mut self, messages: Vec<Message>) {
        self.messages.extend(messages);
    }

    /// Antrekan follow-up: dijalankan sebagai turn baru setelah agen berhenti.
    pub fn follow_up(&mut self, text: impl Into<String>) {
        self.follow_ups.push_back(text.into());
    }

    /// Apakah sesi sedang menjalankan turn.
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    /// Model yang dipakai turn berikutnya.
    pub fn model(&self) -> &str {
        &self.settings.model
    }

    /// Ganti model sesi (dipakai `/model`).
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.settings.model = model.into();
    }

    /// Set tingkat penalaran (`low`/`medium`/`high`); `None` = default gateway.
    pub fn set_effort(&mut self, effort: Option<String>) {
        self.settings.effort = effort;
    }

    /// Ganti approver (dipakai `/force`: yolo satu turn lalu pulih).
    pub fn set_approver(&mut self, approver: Arc<dyn Approver>) {
        self.approver = approver;
    }

    /// Tingkat penalaran aktif.
    pub fn effort(&self) -> Option<&str> {
        self.settings.effort.as_deref()
    }

    /// Minta turn yang sedang berjalan berhenti (tool + streaming).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Salinan token untuk UI atau pengawas lain.
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Jeda sesi: turn berikutnya ditolak, turn berjalan berhenti di batas iterasi.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// Lanjutkan sesi yang dijeda.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Apakah sesi sedang dijeda.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Pegangan flag jeda untuk pengawas (mis. tool yang menjeda sesi).
    pub fn pause_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.paused)
    }

    /// Pegangan jeda ber-tipe untuk tool/UI.
    pub fn pause_handle(&self) -> PauseHandle {
        PauseHandle(Arc::clone(&self.paused))
    }

    /// Pemakaian token kumulatif sesi ini.
    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// Pemakaian token per model, urut sesuai kemunculan pertama.
    pub fn usage_by_model(&self) -> &[(String, Usage)] {
        &self.usage_by_model
    }

    fn record_usage(&mut self, model: &str, usage: Usage) {
        if usage.is_zero() {
            return;
        }
        match self
            .usage_by_model
            .iter_mut()
            .find(|(name, _)| name == model)
        {
            Some((_, total)) => total.accumulate(usage),
            None => self.usage_by_model.push((model.to_string(), usage)),
        }
    }

    /// Isi pemakaian awal dari sesi yang dilanjutkan.
    pub fn seed_usage(&mut self, usage: Usage) {
        self.usage = usage;
    }

    /// Ambil audit approval yang belum dipersist (mengosongkan daftar).
    pub fn take_approvals(&mut self) -> Vec<ApprovalAudit> {
        std::mem::take(&mut self.approvals)
    }

    /// Ambil catatan compaction yang belum dipersist (mengosongkan daftar).
    pub fn take_compactions(&mut self) -> Vec<CompactionRecord> {
        std::mem::take(&mut self.compactions)
    }

    /// Registry tool yang bisa ditambah setelah sesi dibuat.
    pub fn registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    /// Anggaran konteks yang dipakai sesi.
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// Ganti anggaran konteks (dipakai config dan test).
    pub fn set_budget(&mut self, budget: Budget) {
        self.budget = budget;
    }

    /// Setel window konteks dengan reserve/keep default.
    pub fn set_context_window(&mut self, context_window: u64) {
        self.budget = Budget::new(
            context_window,
            DEFAULT_RESERVE_TOKENS,
            DEFAULT_KEEP_RECENT_TOKENS,
        );
    }

    /// Status turn saat ini (lihat [`crate::status::TurnStatus`]).
    pub fn status(&self) -> crate::status::TurnStatus {
        self.status
    }

    fn set_status(&mut self, next: crate::status::TurnStatus) {
        debug_assert!(
            self.status.can_transition_to(next),
            "transisi status tidak sah: {} -> {}",
            self.status.label(),
            next.label()
        );
        self.status = next;
    }

    /// Pasang hook `syncContextBeforeModelCall` (RENCANA C-03).
    ///
    /// Hook menerima konteks saat ini dan boleh mengembalikan konteks
    /// pengganti (mis. membuang pesan basi) sebelum compaction/panggilan model.
    /// `None` berarti tidak ada perubahan; hasil kosong diabaikan agar turn
    /// tidak mengirim permintaan tanpa pesan.
    pub fn set_context_sync(&mut self, hook: Arc<ContextSyncFn>) {
        self.context_sync = Some(hook);
    }

    /// Pasang gate yang dipanggil sebelum setiap permintaan model.
    ///
    /// Gate mengembalikan `Some(alasan)` untuk menghentikan turn sebelum
    /// panggilan model berikutnya (RENCANA C-03 `beforeModelCall`). Dipakai
    /// untuk batas anggaran eksternal atau intervensi UI.
    pub fn set_before_model_call(&mut self, gate: Arc<dyn Fn() -> Option<String> + Send + Sync>) {
        self.before_model_call = Some(gate);
    }

    /// Pasang hook **sesudah** tool (observasi: nama, is_error, cuplikan hasil).
    /// Dipanggil untuk setiap eksekusi tool, paralel maupun berurutan.
    pub fn set_after_tool(&mut self, hook: AfterToolHook) {
        self.after_tool = Some(hook);
    }

    /// Pasang hook **sebelum** eksekusi tool (setelah approval lolos):
    /// (nama tool, cuplikan argumen). Observasi, tidak mengubah eksekusi.
    pub fn set_before_tool(&mut self, hook: BeforeToolHook) {
        self.before_tool = Some(hook);
    }

    /// Atur batas token output per panggilan model.
    pub fn set_max_tokens(&mut self, value: Option<u32>) {
        self.settings.max_tokens = value;
    }

    /// Nyalakan/matikan compaction otomatis.
    pub fn set_auto_compact(&mut self, enabled: bool) {
        self.auto_compact = enabled;
    }

    /// Perkiraan token konteks saat ini.
    pub fn estimated_tokens(&self) -> u64 {
        estimate_messages(&self.messages)
    }

    /// Ringkas bagian awal konteks bila melewati anggaran.
    async fn compact(&mut self) -> Option<CompactionReport> {
        let before_tokens = self.estimated_tokens();
        let plan = plan_compaction(&self.messages, self.budget.keep_recent_tokens)?;
        let summarized = plan.to_summarize.len();
        let mut request_messages = plan.to_summarize.clone();
        request_messages.push(Message::user(COMPACT_INSTRUCTION.to_string()));
        let request = ChatRequest {
            model: self.settings.model.clone(),
            messages: request_messages,
            tools: Vec::new(),
            max_tokens: Some(2_048),
            reasoning_effort: None,
        };
        let mut receiver = match self.provider.stream_chat(request).await {
            Ok(receiver) => receiver,
            Err(err) => {
                let _ = self.events.send(AgentEvent::Notice {
                    text: format!("compaction gagal: {}", err.code),
                });
                return None;
            }
        };
        let mut summary = String::new();
        while let Some(event) = receiver.recv().await {
            if let ProviderEvent::Done { message } = event {
                summary = message.joined_text();
            }
        }
        if summary.trim().is_empty() {
            let _ = self.events.send(AgentEvent::Notice {
                text: "compaction dilewati: ringkasan kosong".to_string(),
            });
            return None;
        }
        let replaced = plan.to_summarize.len() as u64;
        self.messages = apply_compaction(&summary, &plan);
        self.compactions.push(CompactionRecord {
            summary: summary.clone(),
            replaced,
        });
        Some(CompactionReport {
            before_tokens,
            after_tokens: self.estimated_tokens(),
            summarized,
        })
    }

    /// Metode compaction `shake`: pangkas isi hasil tool lama tanpa panggilan model.
    ///
    /// Hanya dipakai bila hasilnya membawa konteks kembali ke bawah target
    /// pemulihan; kalau tidak, `false` dikembalikan agar pemanggil memakai
    /// metode `soft` yang benar-benar meringkas.
    fn shake(&mut self) -> bool {
        let target = self.budget.recovery_target();
        let Some(plan) = plan_shake(&self.messages, target, tui_context::SHAKE_KEEP_LAST) else {
            return false;
        };
        if plan.after_tokens > target {
            return false;
        }
        self.messages = plan.messages;
        let _ = self.events.send(AgentEvent::Notice {
            text: format!(
                "konteks diringkas (shake): ~{} \u{2192} ~{} token ({} hasil tool lama dihapus)",
                plan.before_tokens, plan.after_tokens, plan.pruned
            ),
        });
        true
    }

    /// Daftar todo sesi (dari tool `todo`).
    pub fn todos(&self) -> Vec<String> {
        self.tools.todos()
    }

    /// Ringkas konteks sekarang (dipakai `/compact`).
    ///
    /// Mengembalikan `None` bila tidak ada yang bisa diringkas atau ringkasan
    /// gagal dibuat.
    pub async fn compact_now(&mut self) -> Option<CompactionReport> {
        self.compact().await
    }

    /// Jalankan satu turn (plus follow-up yang antre).
    pub async fn run_turn(&mut self, prompt: Option<String>) -> Result<TurnOutcome, MxError> {
        if self.is_paused() {
            return Err(MxError::new(ErrorCode::AgentPaused, "sesi dijeda"));
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(MxError::new(
                ErrorCode::AgentBusy,
                "sesi masih memproses turn",
            ));
        }
        let _busy = BusyGuard {
            flag: Arc::clone(&self.busy),
        };
        if self.status.is_turn_terminal() {
            self.set_status(crate::status::TurnStatus::Idle);
        }
        self.set_status(crate::status::TurnStatus::Streaming);
        self.run_inner(prompt).await
    }

    async fn run_inner(&mut self, prompt: Option<String>) -> Result<TurnOutcome, MxError> {
        if let Some(text) = prompt {
            self.messages.push(Message::user(text));
        }
        let deadline = self.settings.deadline.map(|limit| Instant::now() + limit);
        let mut turns = 0u32;
        self.compactions_this_turn = 0;
        self.cancel.reset();
        // Penjaga deadline: batalkan seluruh turn (termasuk tool yang berjalan)
        // saat batas waktu lewat, bukan hanya sebelum panggilan model.
        let deadline_task = deadline.map(|instant| {
            let token = self.cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep_until(tokio::time::Instant::from_std(instant)).await;
                token.cancel();
            })
        });
        let _ = self.events.send(AgentEvent::AgentStart);

        let stop_reason = loop {
            turns += 1;
            let _ = self.events.send(AgentEvent::TurnStart { index: turns });
            let reason = loop {
                if deadline_passed(deadline) {
                    let _ = self.events.send(AgentEvent::Notice {
                        text: "deadline turn tercapai; turn dihentikan".to_string(),
                    });
                    break StopReason::Aborted;
                }
                if self.is_paused() {
                    let _ = self.events.send(AgentEvent::Notice {
                        text: "sesi dijeda; turn berhenti di batas iterasi".to_string(),
                    });
                    break StopReason::Aborted;
                }
                while let Some(steer) = self.steering.pop_front() {
                    self.messages.push(Message::user(steer));
                }
                // Pemicu memakai max(estimasi lokal, usage provider) sesuai
                // RENCANA C-07: estimasi bisa meleset, usage gateway lebih nyata.
                if self.cancel.is_cancelled() {
                    let _ = self.events.send(AgentEvent::Notice {
                        text: "turn dibatalkan".to_string(),
                    });
                    break StopReason::Aborted;
                }
                if let Some(hook) = &self.context_sync {
                    if let Some(replaced) = hook(&self.messages) {
                        if !replaced.is_empty() {
                            self.messages = replaced;
                        }
                    }
                }
                let used = self.estimated_tokens().max(self.usage.context_tokens());
                if self.auto_compact
                    && self.budget.needs_compaction(used)
                    && self.compactions_this_turn < MAX_COMPACTIONS_PER_TURN
                {
                    self.compactions_this_turn += 1;
                    // `shake` lebih murah: ia hanya membuang isi hasil tool lama.
                    // Bila setelah dipangkas konteks sudah kembali di bawah ambang
                    // pemulihan, panggilan model untuk ringkasan tidak diperlukan.
                    if !self.shake() {
                        self.compact().await;
                    }
                }
                if let Some(gate) = &self.before_model_call {
                    if let Some(reason) = gate() {
                        let _ = self.events.send(AgentEvent::Notice {
                            text: format!("turn dihentikan: {reason}"),
                        });
                        break StopReason::Aborted;
                    }
                }
                let request = self.build_request();
                let _ = self.events.send(AgentEvent::MessageStart);
                let mut receiver = match self.provider.stream_chat(request).await {
                    Ok(receiver) => receiver,
                    Err(err) => {
                        let _ = self.events.send(AgentEvent::Notice {
                            text: format!("{} — {}", err.code, err.code.user_message()),
                        });
                        break StopReason::Error;
                    }
                };
                let mut assistant: Option<AssistantMessage> = None;
                let cancel = self.cancel.clone();
                let mut interrupted = false;
                loop {
                    tokio::select! {
                        maybe = receiver.recv() => {
                            let Some(event) = maybe else { break };
                            let _ = self.events.send(AgentEvent::MessageUpdate(event.clone()));
                            match event {
                                ProviderEvent::Done { message } => {
                                    let model = self.settings.model.clone();
                                    self.record_usage(&model, message.usage);
                                    self.usage.accumulate(message.usage);
                                    assistant = Some(message);
                                }
                                ProviderEvent::Error { code, message } => {
                                    let _ = self.events.send(AgentEvent::Notice {
                                        text: format!("{code}: {message}"),
                                    });
                                }
                                _ => {}
                            }
                        }
                        () = cancel.cancelled() => {
                            interrupted = true;
                            break;
                        }
                    }
                }
                if interrupted {
                    // Stream diputus: tetapkan `message_end` sintetis agar event
                    // dua tingkat tetap berpasangan (message_start tanpa end
                    // membuat UI menggantung).
                    let synthetic = AssistantMessage {
                        content: Vec::new(),
                        stop_reason: StopReason::Aborted,
                        usage: Usage::default(),
                        error: None,
                    };
                    let _ = self.events.send(AgentEvent::MessageEnd {
                        message: synthetic.clone(),
                    });
                    self.messages.push(Message::assistant(&synthetic));
                    let _ = self.events.send(AgentEvent::Notice {
                        text: "turn dibatalkan".to_string(),
                    });
                    break StopReason::Aborted;
                }
                let Some(assistant) = assistant else {
                    // Event dua tingkat harus tetap berpasangan: `message_start`
                    // tanpa `message_end` membuat UI menggantung. Pesan sintetis
                    // menyimpan alasan error untuk transkrip.
                    let reason = "provider tidak mengirim pesan akhir".to_string();
                    let _ = self.events.send(AgentEvent::Notice {
                        text: reason.clone(),
                    });
                    let synthetic = AssistantMessage {
                        content: Vec::new(),
                        stop_reason: StopReason::Error,
                        usage: Usage::default(),
                        error: Some(reason),
                    };
                    let _ = self.events.send(AgentEvent::MessageEnd {
                        message: synthetic.clone(),
                    });
                    self.messages.push(Message::assistant(&synthetic));
                    break StopReason::Error;
                };
                self.messages.push(Message::assistant(&assistant));
                let _ = self.events.send(AgentEvent::MessageEnd {
                    message: assistant.clone(),
                });

                let calls: Vec<(String, String, String)> = assistant
                    .tool_calls()
                    .into_iter()
                    .map(|(id, name, arguments)| {
                        (id.to_string(), name.to_string(), arguments.to_string())
                    })
                    .collect();
                if calls.is_empty() {
                    break assistant.stop_reason;
                }
                if assistant.stop_reason.is_runnable() {
                    self.set_status(crate::status::TurnStatus::ExecutingTools);
                    let mut prepared: Vec<PreparedTool> = Vec::new();
                    for (id, name, arguments) in calls {
                        if let Some(tool) = self.prepare_tool(&id, &name, &arguments).await {
                            prepared.push(tool);
                        }
                    }
                    self.run_tools(prepared).await;
                    if self.auto_compact {
                        let _ = prune_tool_results(&mut self.messages, KEEP_RECENT_TOOL_RESULTS);
                    }
                    if self.cancel.is_cancelled() {
                        let _ = self.events.send(AgentEvent::Notice {
                            text: "turn dibatalkan".to_string(),
                        });
                        break StopReason::Aborted;
                    }
                    continue;
                }
                // Turn `length`/`error`/`aborted`: jangan jalankan tool, pasangkan
                // placeholder agar pasangan tool_use/tool_result tetap utuh.
                let reason = match assistant.stop_reason {
                    StopReason::Length => "turn terpotong (max_tokens)",
                    StopReason::Error => "turn gagal",
                    StopReason::Aborted => "turn dibatalkan",
                    StopReason::Stop | StopReason::ToolUse => "turn tidak runnable",
                };
                for (id, name, _) in calls {
                    self.messages.push(Message::tool_result(
                        id,
                        name,
                        format!("Tidak dijalankan: {reason}."),
                        true,
                        false,
                    ));
                }
                break assistant.stop_reason;
            };
            let _ = self.events.send(AgentEvent::TurnEnd { index: turns });
            if self.is_paused() {
                break reason;
            }
            match self.follow_ups.pop_front() {
                Some(text) => {
                    self.messages.push(Message::user(text));
                    continue;
                }
                None => break reason,
            }
        };
        let final_status = match stop_reason {
            StopReason::Aborted => crate::status::TurnStatus::Aborted,
            StopReason::Error => crate::status::TurnStatus::Error,
            _ => crate::status::TurnStatus::Idle,
        };
        if let Some(task) = deadline_task {
            task.abort();
        }
        self.set_status(final_status);
        let _ = self.events.send(AgentEvent::AgentEnd { turns });
        Ok(TurnOutcome { turns, stop_reason })
    }

    /// Siapkan satu tool call: approval, event start, dan argumen yang sudah diparse.
    ///
    /// `None` berarti call ini sudah diselesaikan di sini (tool tak dikenal atau
    /// ditolak pengguna) dan tidak boleh dieksekusi.
    async fn prepare_tool(
        &mut self,
        id: &str,
        name: &str,
        arguments: &str,
    ) -> Option<PreparedTool> {
        let subject = subject_of(arguments);
        let preview = preview_of(arguments);
        let Some(tool) = self.tools.get(name) else {
            // Tool tak dikenal tetap dilaporkan ke UI supaya kegagalan tidak
            // terlihat seperti diam (C-04 kriteria 3).
            let err = unknown_tool(name);
            let _ = self.events.send(AgentEvent::ToolExecutionEnd {
                tool_call_id: id.to_string(),
                tool_name: name.to_string(),
                is_error: true,
                preview: err.message.chars().take(200).collect(),
            });
            self.push_tool_result(id, name, err.message, true);
            return None;
        };
        let request = ApprovalRequest {
            tool: name.to_string(),
            tier: tool.tier(),
            subject: subject.clone(),
            preview: preview.clone(),
            default: None,
        };
        self.set_status(crate::status::TurnStatus::AwaitingApproval);
        // Pembatalan saat menunggu approval harus menghentikan turn, bukan
        // menggantung sampai batas waktu approval (C-03: Ctrl+C membatalkan
        // turn yang berjalan).
        let decision = {
            let cancelled = self.cancel.cancelled();
            tokio::pin!(cancelled);
            tokio::select! {
                decision = self.approver.decide(request) => decision,
                () = &mut cancelled => {
                    let _ = self.events.send(AgentEvent::Notice {
                        text: "persetujuan dibatalkan karena turn dibatalkan".to_string(),
                    });
                    Decision::Deny
                }
            }
        };
        self.set_status(crate::status::TurnStatus::ExecutingTools);
        self.approvals.push(ApprovalAudit {
            tool: name.to_string(),
            tier: tool.tier(),
            subject,
            decision: format!("{decision:?}"),
        });
        if decision == Decision::Deny {
            if let Some(hook) = &self.after_tool {
                hook(name, true, "Permintaan ditolak pengguna.");
            }
            self.push_tool_result(id, name, "Permintaan ditolak pengguna.".to_string(), true);
            let _ = self.events.send(AgentEvent::ToolExecutionEnd {
                tool_call_id: id.to_string(),
                tool_name: name.to_string(),
                is_error: true,
                preview: "ditolak".to_string(),
            });
            return None;
        }
        let _ = self.events.send(AgentEvent::ToolExecutionStart {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            preview: preview.clone(),
        });
        if let Some(hook) = &self.before_tool {
            hook(name, &preview);
        }
        let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
        let ctx = ToolContext {
            cancel: self.cancel.clone(),
            ..self.tool_ctx.clone()
        };
        let concurrency = tool.definition().concurrency;
        Some(PreparedTool {
            id: id.to_string(),
            name: name.to_string(),
            tool,
            args,
            ctx,
            concurrency,
        })
    }

    /// Jalankan tool yang sudah disiapkan sesuai konkurensinya.
    ///
    /// Tool `shared` yang berurutan dijalankan bersamaan; tool `exclusive`
    /// selalu sendirian supaya tidak tumpang tindih dengan yang lain.
    /// Jalankan satu tool tetapi berhenti bila turn dibatalkan/deadline lewat.
    async fn execute_tool_cancellable(
        task: &PreparedTool,
    ) -> Result<tui_wire::ToolOutput, MxError> {
        let cancelled = task.ctx.cancel.cancelled();
        tokio::pin!(cancelled);
        tokio::select! {
            result = task.tool.execute(&task.args, &task.ctx) => result,
            () = &mut cancelled => Err(MxError::new(
                tui_wire::ErrorCode::ToolFailed,
                "tool dibatalkan karena turn dihentikan",
            )),
        }
    }

    async fn run_tools(&mut self, prepared: Vec<PreparedTool>) {
        let mut index = 0;
        while index < prepared.len() {
            if prepared[index].concurrency == Concurrency::Shared {
                let mut end = index;
                while end < prepared.len() && prepared[end].concurrency == Concurrency::Shared {
                    end += 1;
                }
                let mut pending: Vec<PreparedTool> = prepared[index..end].to_vec();
                if pending.len() == 1 {
                    let task = pending.remove(0);
                    let id = task.id.clone();
                    let name = task.name.clone();
                    let result = Self::execute_tool_cancellable(&task).await;
                    self.finish_tool(&id, &name, result);
                } else {
                    let mut handles = Vec::new();
                    for task in pending {
                        let id = task.id.clone();
                        let name = task.name.clone();
                        let handle = tokio::spawn(async move {
                            let result = Self::execute_tool_cancellable(&task).await;
                            (id, name, result)
                        });
                        handles.push(handle);
                    }
                    for handle in handles {
                        match handle.await {
                            Ok((id, name, result)) => self.finish_tool(&id, &name, result),
                            Err(err) => {
                                let _ = self.events.send(AgentEvent::Notice {
                                    text: format!("tool paralel berhenti tak terduga: {err}"),
                                });
                            }
                        }
                    }
                }
                index = end;
                continue;
            }
            let task = prepared[index].clone();
            let id = task.id.clone();
            let name = task.name.clone();
            let result = Self::execute_tool_cancellable(&task).await;
            self.finish_tool(&id, &name, result);
            index += 1;
        }
    }

    /// Catat hasil satu tool: event end + pesan `tool_result`.
    fn finish_tool(&mut self, id: &str, name: &str, result: Result<tui_wire::ToolOutput, MxError>) {
        match result {
            Ok(output) => {
                if let Some(hook) = &self.after_tool {
                    hook(name, output.is_error, &output.content);
                }
                let _ = self.events.send(AgentEvent::ToolExecutionEnd {
                    tool_call_id: id.to_string(),
                    tool_name: name.to_string(),
                    is_error: output.is_error,
                    preview: output.content.chars().take(200).collect(),
                });
                self.push_tool_result(id, name, output.content, output.is_error);
            }
            Err(err) => {
                let text = format!("{}: {}", err.code, err.message);
                if let Some(hook) = &self.after_tool {
                    hook(name, true, &text);
                }
                let _ = self.events.send(AgentEvent::ToolExecutionEnd {
                    tool_call_id: id.to_string(),
                    tool_name: name.to_string(),
                    is_error: true,
                    preview: text.chars().take(200).collect(),
                });
                self.push_tool_result(id, name, text, true);
            }
        }
    }
    fn push_tool_result(&mut self, id: &str, name: &str, content: String, is_error: bool) {
        self.messages
            .push(Message::tool_result(id, name, content, is_error, false));
    }

    fn fit_output_tokens(&self) -> Option<u32> {
        let configured = self.settings.max_tokens?;
        let remaining = self.budget.usable().saturating_sub(self.estimated_tokens());
        let floor = u64::from(MIN_OUTPUT_TOKENS.min(configured));
        let fitted = remaining
            .min(u64::from(configured))
            .max(floor)
            .min(u64::from(configured));
        u32::try_from(fitted).ok()
    }

    fn build_request(&self) -> ChatRequest {
        let mut messages = Vec::with_capacity(self.messages.len() + 1);
        if !self.settings.system_prompt.is_empty() {
            messages.push(Message::system(self.settings.system_prompt.clone()));
        }
        messages.extend(self.messages.iter().cloned());
        ChatRequest {
            model: self.settings.model.clone(),
            messages,
            tools: self.tools.definitions(),
            max_tokens: self.fit_output_tokens(),
            reasoning_effort: self.settings.effort.clone(),
        }
    }
}

/// Audit satu keputusan approval, untuk dipersist oleh pemanggil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalAudit {
    /// Nama tool.
    pub tool: String,
    /// Tier tool.
    pub tier: ToolTier,
    /// Subjek yang diminta.
    pub subject: String,
    /// Keputusan yang diambil.
    pub decision: String,
}

/// Catatan satu compaction, untuk dipersist oleh pemanggil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionRecord {
    /// Ringkasan yang menggantikan pesan lama.
    pub summary: String,
    /// Jumlah pesan yang digantikan.
    pub replaced: u64,
}

/// Pegangan flag jeda untuk pengawas (tool, UI lain).
#[derive(Debug, Clone)]
pub struct PauseHandle(Arc<AtomicBool>);

impl PauseHandle {
    /// Jeda sesi.
    pub fn pause(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Lanjutkan sesi.
    pub fn resume(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    /// Apakah sesi sedang dijeda.
    pub fn is_paused(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Melepas penanda sibuk saat turn selesai **atau** future-nya dibatalkan.
struct BusyGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

fn deadline_passed(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|limit| Instant::now() >= limit)
}

fn subject_of(arguments: &str) -> String {
    let value: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    for key in ["command", "path", "pattern", "question"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }
    arguments.chars().take(120).collect()
}

fn preview_of(arguments: &str) -> String {
    arguments.chars().take(300).collect()
}

/// Buat kanal event untuk satu sesi.
pub fn event_channel() -> (UnboundedSender<AgentEvent>, UnboundedReceiver<AgentEvent>) {
    unbounded_channel()
}

/// Blok teks dari pesan assistant (untuk render).
pub fn assistant_text(message: &AssistantMessage) -> String {
    message.joined_text()
}

/// Apakah pesan membawa blok tool.
pub fn has_tool_use(messages: &[ContentBlock]) -> bool {
    messages.iter().any(|block| block.tool_use_id().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use tui_approval::FixedApprover;
    use tui_provider::GatewayError;
    use tui_tools::Tool;
    use tui_wire::{ToolDefinition, ToolOutput, ToolTier};

    struct ScriptedProvider {
        scripts: StdMutex<VecDeque<Vec<ProviderEvent>>>,
        requests: StdMutex<Vec<ChatRequest>>,
        hang: bool,
        keepalive: StdMutex<Option<UnboundedSender<ProviderEvent>>>,
    }

    impl ScriptedProvider {
        fn new(scripts: Vec<Vec<ProviderEvent>>) -> Self {
            Self {
                scripts: StdMutex::new(scripts.into()),
                requests: StdMutex::new(Vec::new()),
                hang: false,
                keepalive: StdMutex::new(None),
            }
        }

        /// Provider yang tidak pernah mengirim `Done` dan tidak menutup kanal;
        /// dipakai untuk menguji pembatalan turn.
        fn hanging() -> Self {
            Self {
                hang: true,
                ..Self::new(Vec::new())
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().map(|r| r.len()).unwrap_or(0)
        }

        fn last_request(&self) -> Option<ChatRequest> {
            self.requests.lock().ok().and_then(|r| r.last().cloned())
        }

        fn first_request(&self) -> Option<ChatRequest> {
            self.requests.lock().ok().and_then(|r| r.first().cloned())
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        async fn stream_chat(
            &self,
            request: ChatRequest,
        ) -> Result<UnboundedReceiver<ProviderEvent>, GatewayError> {
            if let Ok(mut requests) = self.requests.lock() {
                requests.push(request);
            }
            let (tx, rx) = unbounded_channel();
            if self.hang {
                if let Ok(mut slot) = self.keepalive.lock() {
                    *slot = Some(tx);
                }
                return Ok(rx);
            }
            let script = self
                .scripts
                .lock()
                .ok()
                .and_then(|mut scripts| scripts.pop_front())
                .unwrap_or_default();
            for event in script {
                let _ = tx.send(event);
            }
            Ok(rx)
        }
    }

    struct RecordingTool {
        calls: Arc<StdMutex<Vec<String>>>,
    }

    #[async_trait]
    impl Tool for RecordingTool {
        fn tier(&self) -> ToolTier {
            ToolTier::Read
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".into(),
                description: "mengembalikan argumen".into(),
                parameters: serde_json::json!({"type": "object"}),
                concurrency: tui_wire::Concurrency::Shared,
            }
        }

        async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(args.to_string());
            }
            Ok(ToolOutput::ok("hasil-tool"))
        }
    }

    fn settings() -> TurnSettings {
        TurnSettings {
            deadline: Some(Duration::from_secs(30)),
            ..TurnSettings::default()
        }
    }

    fn tool_call_message(name: &str, stop_reason: StopReason) -> AssistantMessage {
        AssistantMessage {
            content: vec![
                ContentBlock::Text {
                    text: "menjalankan tool".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: name.into(),
                    arguments: "{\"command\":\"dir\"}".into(),
                },
            ],
            stop_reason,
            usage: Default::default(),
            error: None,
        }
    }

    type Harness = (
        AgentSession,
        Arc<ScriptedProvider>,
        Arc<StdMutex<Vec<String>>>,
        UnboundedReceiver<AgentEvent>,
    );

    #[test]
    fn after_tool_dipanggil_sukses_dengan_nama_dan_status() {
        let panggilan = Arc::new(StdMutex::new(Vec::<(String, bool, String)>::new()));
        let pemberi = panggilan.clone();
        let (mut session, _provider, _calls, _rx) = harness(
            vec![vec![done(tool_call_message("echo", StopReason::ToolUse))]],
            Decision::Once,
        );
        session.set_after_tool(Arc::new(move |nama: &str, is_error: bool, cuplik: &str| {
            if let Ok(mut simpan) = pemberi.lock() {
                simpan.push((
                    nama.to_string(),
                    is_error,
                    cuplik.chars().take(40).collect(),
                ));
            }
        }));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(session.run_turn(Some("satu tool".into())))
            .unwrap();
        let tercatat = panggilan.lock().unwrap().clone();
        assert!(!tercatat.is_empty(), "hook harus dipanggil");
        assert_eq!(tercatat[0].0, "echo");
        assert!(!tercatat[0].1, "tool sukses: is_error=false");
        assert!(tercatat[0].2.contains("RecordingTool") || !tercatat[0].2.is_empty());
    }

    #[test]
    fn after_tool_dipanggil_saat_tool_ditolak() {
        let panggilan = Arc::new(StdMutex::new(Vec::<(String, bool)>::new()));
        let pemberi = panggilan.clone();
        let (mut session, _provider, _calls, _rx) = harness(
            vec![vec![done(tool_call_message("echo", StopReason::ToolUse))]],
            Decision::Deny,
        );
        session.set_after_tool(Arc::new(move |nama: &str, is_error: bool, _: &str| {
            if let Ok(mut simpan) = pemberi.lock() {
                simpan.push((nama.to_string(), is_error));
            }
        }));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(session.run_turn(Some("tolak tool".into())))
            .unwrap();
        let tercatat = panggilan.lock().unwrap().clone();
        assert!(!tercatat.is_empty(), "hook harus dipanggil saat ditolak");
        assert_eq!(tercatat[0].0, "echo");
        assert!(tercatat[0].1, "ditolak: is_error=true");
    }

    #[test]
    fn before_tool_dipanggil_sebelum_eksekusi() {
        let panggilan = Arc::new(StdMutex::new(Vec::<(String, String)>::new()));
        let pemberi = panggilan.clone();
        let (mut session, _provider, _calls, _rx) = harness(
            vec![vec![done(tool_call_message("echo", StopReason::ToolUse))]],
            Decision::Once,
        );
        session.set_before_tool(Arc::new(move |nama: &str, cuplik: &str| {
            if let Ok(mut simpan) = pemberi.lock() {
                simpan.push((nama.to_string(), cuplik.chars().take(40).collect()));
            }
        }));
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(session.run_turn(Some("satu tool".into())))
            .unwrap();
        let tercatat = panggilan.lock().unwrap().clone();
        assert!(!tercatat.is_empty(), "hook sebelum harus dipanggil");
        assert_eq!(tercatat[0].0, "echo");
    }

    fn harness(scripts: Vec<Vec<ProviderEvent>>, decision: Decision) -> Harness {
        let provider = Arc::new(ScriptedProvider::new(scripts));
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let tool = Arc::new(RecordingTool {
            calls: calls.clone(),
        });
        let registry = ToolRegistry::from_tools(vec![tool]);
        let (tx, rx) = event_channel();
        let session = AgentSession::new(
            provider.clone(),
            Arc::new(FixedApprover::new(decision)),
            registry,
            ToolContext::new(std::env::temp_dir()),
            settings(),
            tx,
        );
        (session, provider, calls, rx)
    }

    fn done(message: AssistantMessage) -> ProviderEvent {
        ProviderEvent::Done { message }
    }

    struct DelayedTool {
        name: &'static str,
        concurrency: tui_wire::Concurrency,
        delay_ms: u64,
        completed: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl Tool for DelayedTool {
        fn tier(&self) -> ToolTier {
            ToolTier::Read
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.into(),
                description: "tool lambat untuk uji konkurensi".into(),
                parameters: serde_json::json!({"type": "object"}),
                concurrency: self.concurrency,
            }
        }

        async fn execute(&self, _args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            self.completed
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ToolOutput::ok("selesai"))
        }
    }

    fn call(id: &str, name: &str) -> ContentBlock {
        call_with(id, name, "{}")
    }

    fn call_with(id: &str, name: &str, arguments: &str) -> ContentBlock {
        ContentBlock::ToolUse {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    fn turn_with_tools(calls: Vec<ContentBlock>) -> AssistantMessage {
        AssistantMessage {
            content: calls,
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        }
    }

    fn delayed(
        name: &'static str,
        concurrency: tui_wire::Concurrency,
        delay_ms: u64,
        completed: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> std::sync::Arc<dyn Tool> {
        std::sync::Arc::new(DelayedTool {
            name,
            concurrency,
            delay_ms,
            completed: std::sync::Arc::clone(completed),
        })
    }

    async fn run_two_tools(
        first: (&'static str, tui_wire::Concurrency, u64),
        second: (&'static str, tui_wire::Concurrency, u64),
    ) -> usize {
        let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tools: Vec<std::sync::Arc<dyn Tool>> = vec![
            delayed(first.0, first.1, first.2, &completed),
            delayed(second.0, second.1, second.2, &completed),
        ];
        let registry = ToolRegistry::from_tools(tools);
        let final_answer = AssistantMessage::text("beres", StopReason::Stop);
        let scripts = vec![
            vec![done(turn_with_tools(vec![
                call("c1", first.0),
                call("c2", second.0),
            ]))],
            vec![done(final_answer)],
        ];
        let provider = std::sync::Arc::new(ScriptedProvider::new(scripts));
        let (tx, _rx) = event_channel();
        let mut session = AgentSession::new(
            provider,
            std::sync::Arc::new(FixedApprover::new(Decision::Once)),
            registry,
            ToolContext::new("."),
            TurnSettings::default(),
            tx,
        );
        let run = tokio::spawn(async move { session.run_turn(Some("jalan".into())).await });
        tokio::time::pause();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(120)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        let done = completed.load(std::sync::atomic::Ordering::SeqCst);
        // Jangan menunggu task selesai: dengan jam dijeda, runtime akan
        // memajukan waktu saat idle sehingga tool kedua bisa ikut selesai.
        run.abort();
        done
    }

    #[tokio::test]
    async fn tool_shared_berjalan_bersamaan() {
        let completed = run_two_tools(
            ("lambat_a", tui_wire::Concurrency::Shared, 100),
            ("lambat_b", tui_wire::Concurrency::Shared, 100),
        )
        .await;
        assert_eq!(
            completed, 2,
            "dua tool shared harus selesai dalam satu jendela 120ms"
        );
    }

    #[tokio::test]
    async fn tool_exclusive_tidak_tumpang_tindih() {
        let completed = run_two_tools(
            ("eksklusif_a", tui_wire::Concurrency::Exclusive, 100),
            ("lambat_b", tui_wire::Concurrency::Shared, 100),
        )
        .await;
        assert_eq!(
            completed, 1,
            "setelah 120ms hanya tool eksklusif yang selesai; yang kedua menunggu"
        );
    }
    struct WaitingApprover {
        started: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        decision: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<Decision>>>,
    }

    #[async_trait]
    impl Approver for WaitingApprover {
        async fn decide(&self, _request: tui_approval::ApprovalRequest) -> Decision {
            self.started
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let receiver = {
                let mut guard = self.decision.lock().await;
                guard.take()
            };
            match receiver {
                Some(rx) => rx.await.unwrap_or(Decision::Deny),
                None => Decision::Deny,
            }
        }
    }

    /// Kumpulkan seluruh event dari kanal sampai tertutup/none.
    fn drain_events(mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    fn golden_lines(text: &str) -> Vec<String> {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    #[tokio::test]
    async fn compaction_dipicu_usage_provider_yang_lebih_besar() {
        // Estimasi lokal kecil, tetapi gateway melaporkan konteks besar.
        let mut summary = AssistantMessage::text("ringkasan", StopReason::Stop);
        summary.usage = Usage {
            input_tokens: 9_000,
            output_tokens: 0,
            cached_tokens: 0,
        };
        let answer = AssistantMessage::text("jawaban", StopReason::Stop);
        let (mut session, provider, _calls, _rx) = harness(
            vec![vec![done(summary)], vec![done(answer)]],
            Decision::Once,
        );
        session.set_budget(Budget::new(10_000, 1_000, 100));
        session.seed_usage(Usage {
            input_tokens: 9_500,
            output_tokens: 0,
            cached_tokens: 0,
        });
        for _ in 0..3 {
            session.push_message(Message::user("y".repeat(300)));
        }
        assert!(
            session.estimated_tokens() < session.budget().usable(),
            "estimasi lokal sengaja di bawah ambang"
        );
        session
            .run_turn(Some("pertanyaan".into()))
            .await
            .expect("turn");
        assert_eq!(
            provider.request_count(),
            2,
            "usage provider di atas ambang harus memicu ringkasan"
        );
    }

    #[tokio::test]
    async fn hysteresis_mencegah_compaction_berulang() {
        let summary = AssistantMessage::text("ringkasan pendek", StopReason::Stop);
        let answer = AssistantMessage::text("jawaban", StopReason::Stop);
        let (mut session, provider, _calls, _rx) = harness(
            vec![
                vec![done(summary)],
                vec![done(answer)],
                vec![done(AssistantMessage::text("lagi", StopReason::Stop))],
            ],
            Decision::Once,
        );
        session.set_budget(Budget::new(2_000, 500, 100));
        for _ in 0..12 {
            session.push_message(Message::user("x".repeat(600)));
        }
        session
            .run_turn(Some("pertama".into()))
            .await
            .expect("turn");
        assert_eq!(provider.request_count(), 2, "satu ringkasan + satu jawaban");
        assert!(
            session.budget().within_recovery(session.estimated_tokens()),
            "setelah compaction konteks harus di bawah target pemulihan"
        );
        // Turn berikutnya tidak boleh langsung memicu ringkasan lagi.
        session.run_turn(Some("kedua".into())).await.expect("turn");
        assert_eq!(
            provider.request_count(),
            3,
            "tanpa hysteresis akan ada ringkasan kedua"
        );
    }

    #[tokio::test]
    async fn golden_follow_up() {
        let first = AssistantMessage::text("pertama", StopReason::Stop);
        let second = AssistantMessage::text("kedua", StopReason::Stop);
        let (mut session, _provider, _calls, rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        session.follow_up("lanjut");
        session.run_turn(Some("mulai".into())).await.expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/follow-up.txt"));
        let diff = tui_wire::parity::compare("follow-up", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_length_tanpa_tool() {
        let answer = AssistantMessage::text("terpotong", StopReason::Length);
        let (mut session, _provider, _calls, rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session
            .run_turn(Some("panjang".into()))
            .await
            .expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/length.txt"));
        let diff = tui_wire::parity::compare("length", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_dua_tool_dalam_satu_pesan() {
        let tool_turn = AssistantMessage {
            content: vec![
                ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "echo".into(),
                    arguments: "{\"n\":1}".into(),
                },
                ContentBlock::ToolUse {
                    id: "c2".into(),
                    name: "echo".into(),
                    arguments: "{\"n\":2}".into(),
                },
            ],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let answer = AssistantMessage::text("selesai", StopReason::Stop);
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![done(tool_turn)], vec![done(answer)]],
            Decision::Once,
        );
        session
            .run_turn(Some("dua tool".into()))
            .await
            .expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/dua-tool.txt"));
        let diff = tui_wire::parity::compare("dua-tool", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_rantai_dua_tool() {
        let first = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{\"n\":1}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let second = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c2".into(),
                name: "echo".into(),
                arguments: "{\"n\":2}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let answer = AssistantMessage::text("selesai", StopReason::Stop);
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![done(first)], vec![done(second)], vec![done(answer)]],
            Decision::Once,
        );
        session.run_turn(Some("rantai".into())).await.expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/rantai-dua-tool.txt"));
        let diff = tui_wire::parity::compare("rantai-dua-tool", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_tool_tidak_dikenal() {
        let tool_turn = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "tidak_ada".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let answer = AssistantMessage::text("beres", StopReason::Stop);
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![done(tool_turn)], vec![done(answer)]],
            Decision::Once,
        );
        session
            .run_turn(Some("panggil asing".into()))
            .await
            .expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/tool-tidak-dikenal.txt"));
        let diff = tui_wire::parity::compare("tool-tidak-dikenal", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn status_turn_selesai_kembali_idle() {
        let answer = AssistantMessage::text("halo", StopReason::Stop);
        let (mut session, _provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        assert_eq!(session.status(), crate::TurnStatus::Idle, "awal idle");
        session.run_turn(Some("halo".into())).await.expect("turn");
        assert_eq!(
            session.status(),
            crate::TurnStatus::Idle,
            "turn normal selesai idle"
        );
    }

    #[tokio::test]
    async fn status_turn_dibatalkan_menjadi_aborted_lalu_idle() {
        let (tx, _events) = event_channel();
        let mut session = AgentSession::new(
            Arc::new(ScriptedProvider::hanging()),
            Arc::new(FixedApprover::new(Decision::Once)),
            ToolRegistry::from_tools(Vec::new()),
            ToolContext::new(std::env::temp_dir()),
            settings(),
            tx,
        );
        let token = session.cancel_token();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            token.cancel();
        });
        session.run_turn(Some("x".into())).await.expect("turn");
        assert_eq!(session.status(), crate::TurnStatus::Aborted);
    }

    #[tokio::test]
    async fn gate_sebelum_panggilan_model_bisa_menghentikan_turn() {
        let answer = AssistantMessage::text("tidak terpakai", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        session.set_before_model_call(std::sync::Arc::new(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                Some("batas eksternal tercapai".to_string())
            } else {
                None
            }
        }));
        let outcome = session.run_turn(Some("halo".into())).await.expect("turn");
        assert_eq!(outcome.stop_reason, StopReason::Aborted);
        assert_eq!(
            provider.request_count(),
            0,
            "gate harus menghentikan sebelum memanggil model"
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn gate_sebelum_panggilan_model_menunggu_bila_tidak_menghalangi() {
        let answer = AssistantMessage::text("lolos", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.set_before_model_call(std::sync::Arc::new(|| None));
        let outcome = session.run_turn(Some("halo".into())).await.expect("turn");
        assert_eq!(outcome.stop_reason, StopReason::Stop);
        assert_eq!(provider.request_count(), 1);
    }

    #[tokio::test]
    async fn golden_tiga_turn_teks_tool_follow_up() {
        let first = AssistantMessage::text("pertama", StopReason::Stop);
        let tool_turn = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{\"x\":1}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let answer = AssistantMessage::text("selesai", StopReason::Stop);
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![done(first)], vec![done(tool_turn)], vec![done(answer)]],
            Decision::Once,
        );
        session.follow_up("lanjut");
        session.run_turn(Some("mulai".into())).await.expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/tiga-turn.txt"));
        let diff = tui_wire::parity::compare("tiga-turn", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_abort() {
        let (tx, rx) = event_channel();
        let mut session = AgentSession::new(
            Arc::new(ScriptedProvider::hanging()),
            Arc::new(FixedApprover::new(Decision::Once)),
            ToolRegistry::from_tools(Vec::new()),
            ToolContext::new(std::env::temp_dir()),
            settings(),
            tx,
        );
        let token = session.cancel_token();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            token.cancel();
        });
        let outcome = session.run_turn(Some("x".into())).await.expect("turn");
        assert_eq!(outcome.stop_reason, StopReason::Aborted);
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/abort.txt"));
        let diff = tui_wire::parity::compare("abort", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_error_provider() {
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![ProviderEvent::Error {
                code: tui_wire::ErrorCode::GatewayServer,
                message: "gateway meledak".to_string(),
            }]],
            Decision::Once,
        );
        let outcome = session.run_turn(Some("halo".into())).await;
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/error-provider.txt"));
        assert!(
            outcome.is_ok(),
            "turn error tetap Ok dengan stop_reason error: {actual:?}"
        );
        let diff = tui_wire::parity::compare("error-provider", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_teks_murni() {
        let answer = AssistantMessage::text("halo juga", StopReason::Stop);
        let (mut session, _provider, _calls, rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.run_turn(Some("halo".into())).await.expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/teks-murni.txt"));
        let diff = tui_wire::parity::compare("teks-murni", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_tool_diizinkan() {
        let tool_turn = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{\"x\":1}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let answer = AssistantMessage::text("beres", StopReason::Stop);
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![done(tool_turn)], vec![done(answer)]],
            Decision::Once,
        );
        session
            .run_turn(Some("pakai tool".into()))
            .await
            .expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/tool-diizinkan.txt"));
        let diff = tui_wire::parity::compare("tool-diizinkan", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[tokio::test]
    async fn golden_tool_ditolak() {
        let tool_turn = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let answer = AssistantMessage::text("ditolak", StopReason::Stop);
        let (mut session, _provider, _calls, rx) = harness(
            vec![vec![done(tool_turn)], vec![done(answer)]],
            Decision::Deny,
        );
        session.run_turn(Some("coba".into())).await.expect("turn");
        let actual = tui_wire::parity::normalize_all(&drain_events(rx));
        let expected = golden_lines(include_str!("../../../tests/golden/tool-ditolak.txt"));
        let diff = tui_wire::parity::compare("tool-ditolak", &expected, &actual);
        assert!(diff.is_clean(), "{}", diff.report());
    }

    #[test]
    fn golden_mendeteksi_mutasi_satu_perilaku() {
        let expected = golden_lines(include_str!("../../../tests/golden/teks-murni.txt"));
        let mut mutated = expected.clone();
        // Mutasi satu perilaku: satu event message_end berubah status.
        let index = mutated
            .iter()
            .position(|event| event.starts_with("message_end:ok:"))
            .expect("fixture memuat message_end");
        mutated[index] = "message_end:MUTASI".to_string();
        let diff = tui_wire::parity::compare("teks-murni-mutasi", &expected, &mutated);
        assert!(!diff.is_clean(), "harness harus MERAH untuk mutasi");
        assert_eq!(diff.first_mismatch, Some(index));
        let report = diff.report();
        assert!(report.contains("teks-murni-mutasi"), "{report}");
        assert!(report.contains("MUTASI"), "{report}");
    }

    #[tokio::test]
    async fn tidak_ada_tool_yang_melewati_gate_approval() {
        let dir = std::env::temp_dir().join(format!("maicli-gate-{}", tui_session::new_id()));
        std::fs::create_dir_all(&dir).expect("buat dir");
        let sentinel = dir.join("ditulis.txt");
        let marker = dir.join("marker.txt");
        let calls = vec![
            call_with("c1", "read", r#"{"path":"a.txt"}"#),
            call_with(
                "c2",
                "write",
                &format!(r#"{{"path":"{}","content":"x"}}"#, sentinel.display()),
            ),
            call_with(
                "c3",
                "edit",
                r#"{"path":"a.txt","old_string":"a","new_string":"b"}"#,
            ),
            call_with("c4", "glob", r#"{"pattern":"*.rs"}"#),
            call_with("c5", "grep", r#"{"pattern":"fn main"}"#),
            call_with(
                "c6",
                "bash",
                &format!(r#"{{"command":"echo x > {}"}}"#, marker.display()),
            ),
            call_with("c7", "todo", r#"{"items":["kerja"]}"#),
            call_with("c8", "ask", r#"{"question":"lanjut?"}"#),
        ];
        let final_answer = AssistantMessage::text("beres", StopReason::Stop);
        let provider = std::sync::Arc::new(ScriptedProvider::new(vec![
            vec![done(turn_with_tools(calls))],
            vec![done(final_answer)],
        ]));
        let (tx, _rx) = event_channel();
        let mut session = AgentSession::new(
            provider,
            std::sync::Arc::new(FixedApprover::new(Decision::Deny)),
            ToolRegistry::v1(),
            ToolContext::new(&dir),
            settings(),
            tx,
        );
        let outcome = session
            .run_turn(Some("coba semua".into()))
            .await
            .expect("turn");
        assert_eq!(outcome.stop_reason, StopReason::Stop);
        let results = session
            .messages()
            .iter()
            .filter(|message| matches!(message, Message::ToolResult { .. }))
            .count();
        assert_eq!(results, 8, "setiap tool call mendapat placeholder hasil");
        for message in session.messages() {
            if let Message::ToolResult {
                content, is_error, ..
            } = message
            {
                assert!(*is_error, "hasil harus error: {content}");
                assert!(content.contains("ditolak"), "harus ditolak gate: {content}");
            }
        }
        assert!(!sentinel.exists(), "write tidak boleh berjalan");
        assert!(!marker.exists(), "bash tidak boleh berjalan");
        let todo = session.todos();
        assert!(todo.is_empty(), "todo tidak boleh berubah");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn turn_tertunda_sampai_approval_diputuskan() {
        let answer = AssistantMessage::text("beres", StopReason::Stop);
        let tool_call = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{\"x\":1}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let provider = std::sync::Arc::new(ScriptedProvider::new(vec![
            vec![done(tool_call)],
            vec![done(answer)],
        ]));
        let calls = std::sync::Arc::new(StdMutex::new(Vec::new()));
        let registry = ToolRegistry::from_tools(vec![std::sync::Arc::new(RecordingTool {
            calls: calls.clone(),
        })]);
        let (tx, _rx) = event_channel();
        let started = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (decision_tx, decision_rx) = tokio::sync::oneshot::channel();
        let approver = std::sync::Arc::new(WaitingApprover {
            started: started.clone(),
            decision: tokio::sync::Mutex::new(Some(decision_rx)),
        });
        let mut session = AgentSession::new(
            provider,
            approver,
            registry,
            ToolContext::new("."),
            TurnSettings::default(),
            tx,
        );
        let run = tokio::spawn(async move { session.run_turn(Some("jalan".into())).await });
        for _ in 0..50 {
            if started.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(started.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            calls.lock().map(|guard| guard.len()).unwrap_or(0),
            0,
            "tool tidak boleh dieksekusi sebelum diputuskan"
        );
        let _ = decision_tx.send(Decision::Once);
        let outcome = run.await.expect("turn selesai").expect("turn ok");
        assert_eq!(outcome.stop_reason, StopReason::Stop);
        assert_eq!(
            calls.lock().map(|guard| guard.len()).unwrap_or(0),
            1,
            "tool dieksekusi setelah izin"
        );
    }

    #[tokio::test]
    async fn turn_yang_dibatalkan_melepas_penanda_sibuk() {
        let provider = Arc::new(ScriptedProvider::hanging());
        let (tx, _events) = event_channel();
        let mut session = AgentSession::new(
            provider,
            Arc::new(FixedApprover::new(Decision::Once)),
            ToolRegistry::from_tools(Vec::new()),
            ToolContext::new(std::env::temp_dir()),
            settings(),
            tx,
        );
        let outcome = tokio::time::timeout(
            Duration::from_millis(50),
            session.run_turn(Some("x".into())),
        )
        .await;
        assert!(outcome.is_err(), "turn harus dibatalkan oleh timeout");
        assert!(
            !session.is_busy(),
            "penanda sibuk harus dilepas saat future dibatalkan"
        );
    }

    #[tokio::test]
    async fn cancel_menghentikan_turn_yang_menunggu_stream() {
        let (tx, _events) = event_channel();
        let mut session = AgentSession::new(
            Arc::new(ScriptedProvider::hanging()),
            Arc::new(FixedApprover::new(Decision::Once)),
            ToolRegistry::from_tools(Vec::new()),
            ToolContext::new(std::env::temp_dir()),
            settings(),
            tx,
        );
        let token = session.cancel_token();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            token.cancel();
        });
        let outcome =
            tokio::time::timeout(Duration::from_secs(2), session.run_turn(Some("x".into())))
                .await
                .expect("turn harus berhenti sendiri setelah dibatalkan")
                .expect("pembatalan bukan error");
        assert_eq!(outcome.stop_reason, StopReason::Aborted);
        assert!(!session.is_busy());
    }
    #[tokio::test]
    async fn pesan_resume_ikut_pada_permintaan_pertama() {
        let answer = AssistantMessage::text("lanjutan", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.push_messages(vec![
            Message::user("permintaan lama"),
            Message::assistant(&AssistantMessage::text("jawaban lama", StopReason::Stop)),
        ]);
        session.run_turn(Some("lanjutkan".into())).await.unwrap();
        let first = provider.first_request().expect("permintaan tercatat");
        let texts: Vec<String> = first
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::User { content } => Some(content.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["permintaan lama".to_string(), "lanjutkan".to_string()]
        );
    }

    struct PauseTool {
        handle: PauseHandle,
    }

    #[async_trait]
    impl tui_tools::Tool for PauseTool {
        fn tier(&self) -> tui_wire::ToolTier {
            tui_wire::ToolTier::Write
        }

        fn definition(&self) -> tui_wire::ToolDefinition {
            tui_wire::ToolDefinition {
                name: "pause_now".into(),
                description: "menjeda sesi dari dalam tool".into(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
                concurrency: tui_wire::Concurrency::Shared,
            }
        }

        async fn execute(
            &self,
            _args: &Value,
            _ctx: &ToolContext,
        ) -> Result<tui_wire::ToolOutput, MxError> {
            self.handle.pause();
            Ok(tui_wire::ToolOutput::ok("dijeda dari tool"))
        }
    }

    #[tokio::test]
    async fn context_sync_mengganti_konteks_sebelum_panggilan_model() {
        let answer = AssistantMessage::text("ok", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.push_message(Message::user("pesan lama yang harus dibuang"));
        session.set_context_sync(Arc::new(|messages: &[Message]| {
            // Hook contoh: buang pesan pertama, tambahkan penanda sinkronisasi.
            let mut baru: Vec<Message> = messages.iter().skip(1).cloned().collect();
            baru.push(Message::user("disinkronkan oleh hook"));
            Some(baru)
        }));
        session.run_turn(Some("halo".into())).await.expect("turn");
        let request = provider.last_request().expect("permintaan tercatat");
        let teks: Vec<&str> = request
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::User { content } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !teks.iter().any(|isi| isi.contains("pesan lama")),
            "pesan lama harus dibuang hook: {teks:?}"
        );
        assert!(
            teks.iter()
                .any(|isi| isi.contains("disinkronkan oleh hook")),
            "hasil hook harus ikut ke permintaan: {teks:?}"
        );
    }

    #[tokio::test]
    async fn context_sync_kosong_diabaikan() {
        let answer = AssistantMessage::text("ok", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.set_context_sync(Arc::new(|_messages: &[Message]| Some(Vec::new())));
        session.run_turn(Some("halo".into())).await.expect("turn");
        let request = provider.last_request().expect("permintaan tercatat");
        assert!(
            request
                .messages
                .iter()
                .any(|message| matches!(message, Message::User { content } if content == "halo")),
            "konteks kosong dari hook harus diabaikan"
        );
    }

    #[tokio::test]
    async fn pause_menolak_turn_berikutnya() {
        let (mut session, _provider, _calls, _rx) = harness(
            vec![vec![done(AssistantMessage::text("x", StopReason::Stop))]],
            Decision::Once,
        );
        session.pause();
        assert!(session.is_paused());
        let err = session.run_turn(Some("halo".into())).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::AgentPaused);
        session.resume();
        assert!(!session.is_paused());
        assert!(session.run_turn(Some("halo".into())).await.is_ok());
    }

    #[tokio::test]
    async fn deadline_membatalkan_tool_yang_sedang_berjalan() {
        let tool_turn = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "lambat".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registry = ToolRegistry::from_tools(vec![Arc::new(DelayedTool {
            name: "lambat",
            concurrency: tui_wire::Concurrency::Shared,
            delay_ms: 5_000,
            completed: completed.clone(),
        })]);
        let (tx, _events) = event_channel();
        let mut session = AgentSession::new(
            Arc::new(ScriptedProvider::new(vec![vec![done(tool_turn)]])),
            Arc::new(FixedApprover::new(Decision::Once)),
            registry,
            ToolContext::new(std::env::temp_dir()),
            TurnSettings {
                deadline: Some(Duration::from_millis(80)),
                ..settings()
            },
            tx,
        );
        let mulai = std::time::Instant::now();
        let outcome =
            tokio::time::timeout(Duration::from_secs(2), session.run_turn(Some("x".into())))
                .await
                .expect("deadline harus menghentikan tool")
                .expect("turn");
        assert_eq!(outcome.stop_reason, StopReason::Aborted);
        assert!(mulai.elapsed() < Duration::from_secs(2));
        assert_eq!(
            completed.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "tool tidak boleh selesai setelah dibatalkan"
        );
    }

    #[tokio::test]
    async fn pembatalan_saat_menunggu_approval_tidak_menggantung() {
        let tool_turn = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let (tx, _events) = event_channel();
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let registry = ToolRegistry::from_tools(vec![Arc::new(RecordingTool {
            calls: calls.clone(),
        })]);
        // Approver yang menunggu selamanya; pembatalan harus memutusnya.
        let (jawaban_tx, jawaban_rx) = tokio::sync::oneshot::channel();
        let approver = WaitingApprover {
            started: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            decision: tokio::sync::Mutex::new(Some(jawaban_rx)),
        };
        let mut session = AgentSession::new(
            Arc::new(ScriptedProvider::new(vec![vec![done(tool_turn)]])),
            Arc::new(approver),
            registry,
            ToolContext::new(std::env::temp_dir()),
            settings(),
            tx,
        );
        let token = session.cancel_token();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token.cancel();
        });
        let mulai = std::time::Instant::now();
        let outcome =
            tokio::time::timeout(Duration::from_secs(2), session.run_turn(Some("x".into())))
                .await
                .expect("tidak boleh menggantung menunggu approval")
                .expect("turn");
        assert_eq!(outcome.stop_reason, StopReason::Aborted);
        assert!(mulai.elapsed() < Duration::from_secs(2));
        assert!(
            calls.lock().map(|c| c.is_empty()).unwrap_or(false),
            "tool tidak boleh dieksekusi setelah dibatalkan"
        );
        drop(jawaban_tx);
    }

    #[tokio::test]
    async fn pause_setelah_tool_menghentikan_turn_sebelum_follow_up() {
        let with_tool = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "pause_now".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let (mut session, provider, _calls, _rx) = harness(
            vec![
                vec![done(with_tool)],
                vec![done(AssistantMessage::text(
                    "tidak boleh",
                    StopReason::Stop,
                ))],
            ],
            Decision::Once,
        );
        let handle = session.pause_handle();
        session
            .registry_mut()
            .register(Arc::new(PauseTool { handle }));
        session.follow_up("lanjut");
        let outcome = session.run_turn(Some("mulai".into())).await.expect("turn");
        assert_eq!(
            outcome.turns, 1,
            "follow-up tidak boleh diproses saat dijeda"
        );
        assert_eq!(
            provider.request_count(),
            1,
            "tidak ada panggilan model kedua"
        );
        assert!(session.is_paused());
    }

    #[tokio::test]
    async fn pause_di_tengah_turn_berhenti_sebelum_panggilan_model_berikut() {
        let with_tool = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "pause_now".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Default::default(),
            error: None,
        };
        let (mut session, provider, _calls, _rx) = harness(
            vec![
                vec![done(with_tool)],
                vec![done(AssistantMessage::text(
                    "tidak boleh",
                    StopReason::Stop,
                ))],
            ],
            Decision::Once,
        );
        let handle = session.pause_handle();
        session
            .registry_mut()
            .register(Arc::new(PauseTool { handle }));
        let outcome = session.run_turn(Some("mulai".into())).await.unwrap();
        assert_eq!(
            provider.request_count(),
            1,
            "panggilan model kedua tidak boleh terjadi"
        );
        assert_eq!(outcome.stop_reason, StopReason::Aborted);
    }

    #[tokio::test]
    async fn usage_dipisah_per_model() {
        let mut first = AssistantMessage::text("satu", StopReason::Stop);
        first.usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
            cached_tokens: 0,
        };
        let mut second = AssistantMessage::text("dua", StopReason::Stop);
        second.usage = Usage {
            input_tokens: 3,
            output_tokens: 2,
            cached_tokens: 0,
        };
        let (mut session, _provider, _calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        session.run_turn(Some("satu".into())).await.unwrap();
        session.set_model("model-b");
        session.run_turn(Some("dua".into())).await.unwrap();
        let by_model = session.usage_by_model();
        assert_eq!(by_model.len(), 2, "dua model tercatat terpisah");
        assert_eq!(by_model[0].1.input_tokens, 10);
        assert_eq!(by_model[1].0, "model-b");
        assert_eq!(by_model[1].1.input_tokens, 3);
        assert_eq!(session.usage().input_tokens, 13, "total tetap dijumlahkan");
    }

    #[tokio::test]
    async fn usage_terakumulasi_antar_turn() {
        let mut first = AssistantMessage::text("satu", StopReason::Stop);
        first.usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
            cached_tokens: 2,
        };
        let mut second = AssistantMessage::text("dua", StopReason::Stop);
        second.usage = Usage {
            input_tokens: 3,
            output_tokens: 7,
            cached_tokens: 1,
        };
        let (mut session, _provider, _calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        session.run_turn(Some("satu".into())).await.unwrap();
        session.run_turn(Some("dua".into())).await.unwrap();
        assert_eq!(
            session.usage(),
            Usage {
                input_tokens: 13,
                output_tokens: 12,
                cached_tokens: 3
            }
        );
    }

    #[tokio::test]
    async fn teks_tanpa_tool_selesai_satu_turn() {
        let text = AssistantMessage::text("halo", StopReason::Stop);
        let (mut session, provider, calls, _rx) = harness(vec![vec![done(text)]], Decision::Once);
        let outcome = session.run_turn(Some("hai".into())).await.unwrap();
        assert_eq!(outcome.turns, 1);
        assert_eq!(outcome.stop_reason, StopReason::Stop);
        assert_eq!(provider.request_count(), 1);
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(session.messages().len(), 2);
    }

    #[tokio::test]
    async fn tool_call_runnable_dieksekusi_lalu_lanjut() {
        let first = tool_call_message("echo", StopReason::ToolUse);
        let second = AssistantMessage::text("selesai", StopReason::Stop);
        let (mut session, provider, calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        let outcome = session.run_turn(Some("jalankan".into())).await.unwrap();
        assert_eq!(outcome.turns, 1);
        assert_eq!(provider.request_count(), 2);
        assert_eq!(calls.lock().unwrap().len(), 1);
        let last = provider.last_request().unwrap();
        assert!(
            last.messages.iter().any(
                |m| matches!(m, Message::ToolResult { content, .. } if content == "hasil-tool")
            )
        );
    }

    #[tokio::test]
    async fn audit_approval_tercatat_untuk_izin_dan_tolak() {
        let first = tool_call_message("echo", StopReason::ToolUse);
        let second = AssistantMessage::text("selesai", StopReason::Stop);
        let (mut session, _provider, _calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        session.run_turn(Some("jalankan".into())).await.unwrap();
        let audits = session.take_approvals();
        assert_eq!(audits.len(), 1, "satu keputusan izin tercatat");
        assert_eq!(audits[0].tool, "echo");
        assert_eq!(audits[0].tier, ToolTier::Read, "tool uji bertier baca");
        assert_eq!(audits[0].decision, "Once");
        assert!(session.take_approvals().is_empty(), "daftar dikosongkan");

        let deny = tool_call_message("echo", StopReason::ToolUse);
        let (mut denied, _provider, calls, _rx) = harness(vec![vec![done(deny)]], Decision::Deny);
        denied.run_turn(Some("jalankan".into())).await.unwrap();
        let audits = denied.take_approvals();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].decision, "Deny");
        assert!(calls.lock().unwrap().is_empty(), "tool tidak dieksekusi");
    }

    #[tokio::test]
    async fn turn_length_tidak_menjalankan_tool_dan_memasang_placeholder() {
        let message = tool_call_message("echo", StopReason::Length);
        let (mut session, provider, calls, _rx) =
            harness(vec![vec![done(message)]], Decision::Once);
        let outcome = session.run_turn(Some("x".into())).await.unwrap();
        assert_eq!(outcome.stop_reason, StopReason::Length);
        assert_eq!(provider.request_count(), 1);
        assert!(calls.lock().unwrap().is_empty());
        assert!(session.messages().iter().any(|m| matches!(
            m,
            Message::ToolResult { is_error: true, content, .. } if content.contains("max_tokens")
        )));
    }

    #[tokio::test]
    async fn follow_up_menjalankan_turn_kedua() {
        let first = AssistantMessage::text("pertama", StopReason::Stop);
        let second = AssistantMessage::text("kedua", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        session.follow_up("lanjut");
        let outcome = session.run_turn(Some("mulai".into())).await.unwrap();
        assert_eq!(outcome.turns, 2);
        assert_eq!(provider.request_count(), 2);
    }

    #[tokio::test]
    async fn steering_masuk_sebagai_pesan_pengguna_sebelum_panggilan_berikut() {
        let first = AssistantMessage::text("awal", StopReason::Stop);
        let (mut session, provider, _calls, _rx) = harness(vec![vec![done(first)]], Decision::Once);
        session.steer("catatan tambahan");
        session.run_turn(Some("mulai".into())).await.unwrap();
        let request = provider.last_request().unwrap();
        assert!(
            request
                .messages
                .iter()
                .any(|m| matches!(m, Message::User { content } if content == "catatan tambahan"))
        );
    }

    #[tokio::test]
    async fn approval_ditolak_tidak_mengeksekusi_tool() {
        let first = tool_call_message("echo", StopReason::ToolUse);
        let second = AssistantMessage::text("dibatalkan", StopReason::Stop);
        let (mut session, _provider, calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Deny);
        session.run_turn(Some("x".into())).await.unwrap();
        assert!(calls.lock().unwrap().is_empty());
        assert!(session.messages().iter().any(|m| matches!(
            m,
            Message::ToolResult { is_error: true, content, .. } if content.contains("ditolak")
        )));
    }

    #[tokio::test]
    async fn tool_tidak_dikenal_menghasilkan_error_bernama() {
        let first = tool_call_message("tidak-ada", StopReason::ToolUse);
        let second = AssistantMessage::text("selesai", StopReason::Stop);
        let (mut session, _provider, _calls, _rx) =
            harness(vec![vec![done(first)], vec![done(second)]], Decision::Once);
        session.run_turn(Some("x".into())).await.unwrap();
        assert!(session.messages().iter().any(|m| matches!(
            m,
            Message::ToolResult { is_error: true, content, .. } if content.contains("tidak dikenal")
        )));
    }

    #[tokio::test]
    async fn compact_now_mengembalikan_laporan_token() {
        let summary = AssistantMessage::text("ringkasan", StopReason::Stop);
        let (mut session, _provider, _calls, _rx) =
            harness(vec![vec![done(summary)]], Decision::Once);
        session.set_budget(Budget::new(400, 200, 100));
        for _ in 0..3 {
            session.push_message(Message::user("x".repeat(600)));
        }
        let report = session.compact_now().await.expect("ada yang diringkas");
        assert!(report.before_tokens > report.after_tokens, "{report:?}");
        assert!(report.summarized > 0, "{report:?}");
        assert_eq!(
            report.after_tokens,
            session.estimated_tokens(),
            "angka sesudah harus sama dengan kondisi nyata"
        );
    }

    #[tokio::test]
    async fn compact_now_tanpa_yang_bisa_diringkas_mengembalikan_none() {
        let (mut session, _provider, _calls, _rx) = harness(vec![], Decision::Once);
        assert!(session.compact_now().await.is_none());
    }

    #[tokio::test]
    async fn output_tokens_mengikuti_sisa_window() {
        let answer = AssistantMessage::text("ok", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.set_max_tokens(Some(8_000));
        session.set_budget(Budget::new(4_000, 0, 400));
        // Konteks ~2.400 token: sisa window 4.000 - 2.400 = ~1.600.
        for _ in 0..4 {
            session.push_message(Message::user("x".repeat(2_400)));
        }
        session.run_turn(Some("tanya".into())).await.unwrap();
        let request = provider.first_request().expect("ada permintaan");
        let fit = request.max_tokens.expect("max_tokens terisi");
        assert!(fit < 8_000, "harus menyesuaikan sisa window, dapat {fit}");
        assert!(
            fit >= MIN_OUTPUT_TOKENS,
            "tidak di bawah batas minimum: {fit}"
        );
    }

    #[tokio::test]
    async fn output_tokens_dipaksa_minimum_saat_window_penuh() {
        let answer = AssistantMessage::text("ok", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.set_max_tokens(Some(8_000));
        session.set_auto_compact(false);
        session.set_budget(Budget::new(1_000, 0, 0));
        session.push_message(Message::user("x".repeat(6_000)));
        session.run_turn(Some("tanya".into())).await.unwrap();
        let request = provider.first_request().expect("ada permintaan");
        assert_eq!(
            request.max_tokens,
            Some(MIN_OUTPUT_TOKENS),
            "sisa window hampir nol tetap memakai lantai minimum"
        );
    }

    #[tokio::test]
    async fn output_tokens_tanpa_setting_tetap_kosong() {
        let answer = AssistantMessage::text("ok", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.run_turn(Some("tanya".into())).await.unwrap();
        let request = provider.first_request().expect("ada permintaan");
        assert_eq!(request.max_tokens, None);
    }

    #[tokio::test]
    async fn shake_dipakai_tanpa_panggilan_model_saat_hasil_tool_lama_besar() {
        let answer = AssistantMessage::text("jawaban", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        // usable 6.500 (target pemulihan 5.200); 20 hasil ~10.000 token,
        // 8 hasil terbaru ~4.100 token sehingga shake cukup.
        session.set_budget(Budget::new(7_000, 500, 200));
        for _ in 0..20 {
            session.push_message(Message::tool_result(
                "call",
                "bash",
                "x".repeat(2_000),
                false,
                false,
            ));
        }
        session.run_turn(Some("pertanyaan".into())).await.unwrap();
        assert_eq!(
            provider.request_count(),
            1,
            "shake tidak boleh memanggil model untuk ringkasan"
        );
        let pruned = session
            .messages()
            .iter()
            .filter(|message| {
                matches!(message, Message::ToolResult { content, .. }
                    if content.as_str() == tui_context::PRUNED_TOOL_RESULT)
            })
            .count();
        assert!(pruned > 0, "hasil tool lama harus dipangkas");
        let utuh = session
            .messages()
            .iter()
            .filter(|message| {
                matches!(message, Message::ToolResult { content, .. }
                    if content.as_str() != tui_context::PRUNED_TOOL_RESULT)
            })
            .count();
        assert_eq!(utuh, tui_context::SHAKE_KEEP_LAST);
    }

    #[tokio::test]
    async fn shake_gagal_berarti_fallback_ke_soft() {
        let summary = AssistantMessage::text("ringkasan", StopReason::Stop);
        let answer = AssistantMessage::text("jawaban akhir", StopReason::Stop);
        let (mut session, provider, _calls, _rx) = harness(
            vec![vec![done(summary)], vec![done(answer)]],
            Decision::Once,
        );
        session.set_budget(Budget::new(400, 200, 100));
        for _ in 0..3 {
            session.push_message(Message::user("x".repeat(600)));
        }
        session.run_turn(Some("pertanyaan".into())).await.unwrap();
        assert_eq!(
            provider.request_count(),
            2,
            "tanpa hasil tool yang bisa dipangkas, soft tetap dipakai"
        );
    }

    #[tokio::test]
    async fn compaction_dijalankan_saat_konteks_melewati_anggaran() {
        let summary = AssistantMessage::text("ringkasan panjang", StopReason::Stop);
        let answer = AssistantMessage::text("jawaban akhir", StopReason::Stop);
        let (mut session, provider, _calls, _rx) = harness(
            vec![vec![done(summary)], vec![done(answer)]],
            Decision::Once,
        );
        session.set_budget(Budget::new(400, 200, 100));
        for _ in 0..3 {
            session.push_message(Message::user("x".repeat(600)));
        }
        let before = session.estimated_tokens();
        session.run_turn(Some("pertanyaan".into())).await.unwrap();
        assert_eq!(
            provider.request_count(),
            2,
            "satu permintaan ringkasan, satu jawaban"
        );
        let first = provider
            .first_request()
            .expect("permintaan pertama tercatat");
        assert!(
            first.messages.iter().any(
                |m| matches!(m, Message::User { content } if content.contains("ringkasan konteks"))
            ),
            "permintaan ringkasan harus memuat instruksi compaction"
        );
        assert!(
            session.estimated_tokens() < before,
            "konteks harus mengecil"
        );
        let records = session.take_compactions();
        assert_eq!(records.len(), 1, "satu compaction tercatat untuk dipersist");
        assert!(records[0].summary.contains("ringkasan panjang"));
        assert!(records[0].replaced > 0);
        assert!(
            session.take_compactions().is_empty(),
            "daftar dikosongkan setelah diambil"
        );
        match session.messages().first() {
            Some(Message::User { content }) => {
                assert!(content.contains("[ringkasan konteks sebelumnya]"));
                assert!(content.contains("ringkasan panjang"));
            }
            other => panic!("pesan pertama harus ringkasan, dapat {other:?}"),
        }
    }

    #[tokio::test]
    async fn tanpa_compaction_saat_anggaran_cukup() {
        let answer = AssistantMessage::text("jawaban", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.run_turn(Some("halo".into())).await.unwrap();
        assert_eq!(provider.request_count(), 1);
        assert_eq!(session.messages().len(), 2, "hanya permintaan + jawaban");
    }

    #[tokio::test]
    async fn compaction_dimatikan_tidak_memanggil_provider_ekstra() {
        let answer = AssistantMessage::text("jawaban", StopReason::Stop);
        let (mut session, provider, _calls, _rx) =
            harness(vec![vec![done(answer)]], Decision::Once);
        session.set_budget(Budget::new(400, 200, 100));
        session.set_auto_compact(false);
        for _ in 0..3 {
            session.push_message(Message::user("x".repeat(600)));
        }
        session.run_turn(Some("pertanyaan".into())).await.unwrap();
        assert_eq!(provider.request_count(), 1);
    }
    #[test]
    fn subject_diambil_dari_argumen() {
        assert_eq!(subject_of("{\"command\":\"dir /s\"}"), "dir /s");
        assert_eq!(subject_of("{\"path\":\"a.txt\"}"), "a.txt");
        assert_eq!(subject_of("bukan-json"), "bukan-json");
    }
}
