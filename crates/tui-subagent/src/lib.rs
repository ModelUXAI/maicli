//! Subagent lokal (P2 item A): satu tool `subagent` yang menjalankan sesi anak
//! terisolasi dan mengembalikan hasil akhirnya ke induk.
//!
//! Desain (RENCANA integrasi ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â§8.3 tahap A):
//! - anak = `AgentSession` penuh dengan tool registry sendiri (tool `subagent`
//!   TIDAK didaftarkan ulang ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â subagent tidak bersarang di v1);
//! - prompt anak satu kali; hasilnya = teks assistant terakhir;
//! - anak dibatasi deadline sendiri; gagal anak = hasil error ke induk, turn
//!   induk tetap lanjut;
//! - tanpa file sesi di tahap ini (persistensi artefak anak = tahap B).

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod acp_client;
pub mod manager;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc::unbounded_channel;
use tui_approval::Approver;
use tui_provider::Provider;
use tui_tools::{Tool, ToolContext, ToolRegistry};
use tui_wire::{
    AgentEvent, ContentBlock, ErrorCode, Message, MxError, StopReason, ToolDefinition, ToolOutput,
    ToolTier,
};

/// Konfigurasi tool `subagent`.
#[derive(Clone)]
pub struct SubagentSpec {
    /// Provider yang dipakai sesi anak (biasanya sama dengan induk).
    pub provider: Arc<dyn Provider>,
    /// Approver untuk tool anak.
    pub approver: Arc<dyn Approver>,
    /// Tool yang boleh dipakai anak (TANPA tool `subagent` itu sendiri).
    pub tools: ToolRegistry,
    /// Model anak.
    pub model: String,
    /// Deadline satu turn anak.
    pub deadline: Duration,
    /// Batas jumlah tool call anak di seluruh turn (proteksi loop).
    pub max_tool_calls: u32,
}

/// Hasil satu eksekusi subagent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentResult {
    /// Teks assistant terakhir anak; kosong bila gagal.
    pub text: String,
    /// Apakah anak gagal (deadline/galat turn/loop tool).
    pub is_error: bool,
    /// Alasan gagal, bila ada.
    pub reason: Option<String>,
    /// Jumlah turn anak.
    pub turns: u32,
    /// Jumlah tool call yang dieksekusi anak.
    pub tool_calls: u32,
}

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::AgentBusy, pesan.into())
}

/// Jalankan subagent satu-prompt dan kembalikan hasilnya.
pub async fn run_subagent(spec: &SubagentSpec, prompt: &str) -> SubagentResult {
    if spec.max_tool_calls == 0 {
        return SubagentResult {
            text: String::new(),
            is_error: true,
            reason: Some("max_tool_calls = 0".to_string()),
            turns: 0,
            tool_calls: 0,
        };
    }
    let (tx, rx) = unbounded_channel::<AgentEvent>();
    let settings = tui_core::TurnSettings {
        model: spec.model.clone(),
        deadline: Some(spec.deadline),
        ..tui_core::TurnSettings::default()
    };
    let mut sesi = tui_core::AgentSession::new(
        spec.provider.clone(),
        spec.approver.clone(),
        spec.tools.clone(),
        ToolContext::new("."),
        settings,
        tx,
    );
    let hasil = match sesi.run_turn(Some(prompt.to_string())).await {
        Ok(outcome) => {
            let mut teks = String::new();
            let mut tool_calls = 0u32;
            for pesan in sesi.messages() {
                if let Message::Assistant { content, .. } = pesan {
                    for blok in content {
                        match blok {
                            ContentBlock::Text { text } => teks = text.clone(),
                            ContentBlock::ToolUse { .. } => tool_calls += 1,
                            _ => {}
                        }
                    }
                }
            }
            SubagentResult {
                text: teks,
                is_error: outcome.stop_reason == StopReason::Error,
                reason: (outcome.stop_reason == StopReason::Error)
                    .then(|| format!("stop_reason={:?}", outcome.stop_reason)),
                turns: outcome.turns,
                tool_calls,
            }
        }
        Err(err) => SubagentResult {
            text: String::new(),
            is_error: true,
            reason: Some(format!("{}: {}", err.code, err.message)),
            turns: 0,
            tool_calls: 0,
        },
    };
    drop(rx);
    hasil
}

/// Tool `subagent` untuk registry induk.
pub struct SubagentTool {
    spec: SubagentSpec,
    /// Manager bersama (panel F3 TUI); `None` = hasil hanya lewat tool.
    manager: Option<Arc<manager::SubagentManager>>,
}

impl SubagentTool {
    /// Tool baru; spec di-clone per eksekusi.
    pub fn new(spec: SubagentSpec) -> Self {
        Self {
            spec,
            manager: None,
        }
    }

    /// Bagikan manager supaya anak yang di-spawn terdaftar untuk panel F3.
    pub fn with_manager(mut self, manager: Arc<manager::SubagentManager>) -> Self {
        self.manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for SubagentTool {
    fn tier(&self) -> ToolTier {
        // Menjalankan agent anak + tool-nya = aktivitas berat; selalu minta izin
        // sesuai kebijakan (tidak ada jalur pintas).
        ToolTier::Exec
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "subagent".into(),
            description: "Menjalankan satu subagent (agent anak) dengan prompt tertulis dan \
                          mengembalikan jawaban akhirnya. Gunakan untuk pekerjaan yang bisa \
                          dipisah: riset per-file, ringkasan, atau uji coba terisolasi."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "Tugas lengkap untuk subagent" }
                },
                "required": ["prompt"]
            }),
            concurrency: tui_wire::Concurrency::Exclusive,
        }
    }

    fn concurrency(&self) -> tui_wire::Concurrency {
        tui_wire::Concurrency::Exclusive
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let prompt = args
            .get("prompt")
            .and_then(Value::as_str)
            .ok_or_else(|| galat("argumen prompt wajib"))?;
        if prompt.trim().is_empty() {
            return Ok(ToolOutput::err("prompt subagent kosong"));
        }
        // Bila manager dibagikan, jalankan lewat manager agar panel F3 melihat
        // status anak; hasil tetap sama (satu prompt, tunggu selesai).
        let hasil = match &self.manager {
            Some(manager) => {
                let handle = manager.spawn(self.spec.clone(), prompt.to_string(), "induk", None);
                loop {
                    if handle.status() != manager::SubagentState::Running {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                handle.result().unwrap_or(SubagentResult {
                    text: String::new(),
                    is_error: true,
                    reason: Some("tanpa hasil".to_string()),
                    turns: 0,
                    tool_calls: 0,
                })
            }
            None => run_subagent(&self.spec, prompt).await,
        };
        let mut ringkas = format!("{} turn, {} tool call", hasil.turns, hasil.tool_calls);
        if let Some(alasan) = &hasil.reason {
            ringkas.push_str(&format!("; {alasan}"));
        }
        if hasil.is_error {
            let isi = if hasil.text.is_empty() {
                "(tanpa teks)".to_string()
            } else {
                hasil.text.clone()
            };
            Ok(ToolOutput::err(format!(
                "subagent gagal ({ringkas}): {isi}"
            )))
        } else {
            Ok(ToolOutput::ok(format!("{ringkas}\n{}", hasil.text)))
        }
    }
}

/// Hitung tool call anak dari transkrip (dipakai test dan host).
pub fn count_tool_calls(messages: &[Message]) -> u32 {
    messages
        .iter()
        .filter_map(|pesan| match pesan {
            Message::Assistant { content, .. } => Some(content),
            _ => None,
        })
        .flat_map(|isi| isi.iter())
        .filter(|blok| matches!(blok, ContentBlock::ToolUse { .. }))
        .count() as u32
}

/// Batas jumlah task per orkestrasi (proteksi: 8 anak sekaligus).
pub const MAKS_TASK_ORKESTRASI: usize = 8;

/// Tool `orchestrate`: **memunculkan semua sub-agent** dari daftar task —
/// paralel (semua sekaligus, semuanya terlihat di Agent Hub) atau berurutan.
pub struct OrchestratorTool {
    spec: SubagentSpec,
    /// Manager bersama (wajib: tanpa manager orkestrasi tidak terlihat UI).
    manager: Arc<manager::SubagentManager>,
}

impl OrchestratorTool {
    /// Tool baru dengan spec dasar (di-clone per task).
    pub fn new(spec: SubagentSpec, manager: Arc<manager::SubagentManager>) -> Self {
        Self { spec, manager }
    }
}

#[async_trait]
impl Tool for OrchestratorTool {
    fn tier(&self) -> ToolTier {
        // Mem-spawn banyak agent + tool-nya = exec, selalu lewat approval.
        ToolTier::Exec
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "orchestrate".into(),
            description: "Menjalankan banyak subagent sekaligus. Sediakan daftar task \
                          (id + prompt); mode paralel (default) memunculkan semuanya \
                          bersamaan, mode berurutan menyelesaikan satu per satu. \
                          Lihat progres di panel Agent Hub (Alt+A)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Nama task (bebas)" },
                                "prompt": { "type": "string", "description": "Tugas lengkap untuk anak" }
                            },
                            "required": ["id", "prompt"]
                        },
                        "description": "Daftar task yang dijalankan"
                    },
                    "parallel": {
                        "type": "boolean",
                        "description": "true (default) = semua sekaligus; false = berurutan"
                    }
                },
                "required": ["tasks"]
            }),
            concurrency: tui_wire::Concurrency::Exclusive,
        }
    }

    fn concurrency(&self) -> tui_wire::Concurrency {
        tui_wire::Concurrency::Exclusive
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let daftar = args
            .get("tasks")
            .and_then(Value::as_array)
            .ok_or_else(|| galat("argumen tasks wajib (array)"))?;
        if daftar.is_empty() {
            return Ok(ToolOutput::err("daftar task kosong"));
        }
        if daftar.len() > MAKS_TASK_ORKESTRASI {
            return Ok(ToolOutput::err(format!(
                "terlalu banyak task ({} > {}); pecah menjadi beberapa orkestrasi",
                daftar.len(),
                MAKS_TASK_ORKESTRASI
            )));
        }
        let paralel = args
            .get("parallel")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut tasks: Vec<(String, String)> = Vec::new();
        for item in daftar {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("task")
                .to_string();
            let prompt = item
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if prompt.trim().is_empty() {
                return Ok(ToolOutput::err(format!("task '{id}': prompt kosong")));
            }
            tasks.push((id, prompt));
        }

        // Spawn + tunggu: paralel = semua dulu baru tunggu; berurutan = satu per satu.
        let mut hasil_per_task: Vec<(String, SubagentResult)> = Vec::new();
        let mut menunggu: Vec<(String, manager::SubagentHandle)> = Vec::new();
        for (id, prompt) in &tasks {
            let handle = self
                .manager
                .spawn(self.spec.clone(), prompt.clone(), "induk", None);
            if paralel {
                menunggu.push((id.clone(), handle));
            } else {
                let hasil = tunggu_hasil(&handle).await;
                hasil_per_task.push((id.clone(), hasil));
            }
        }
        for (id, handle) in menunggu {
            let hasil = tunggu_hasil(&handle).await;
            hasil_per_task.push((id, hasil));
        }

        // Ringkasan per task (urutan asli tasks).
        let mut gagal = 0;
        let mut ringkasan = String::from("hasil orkestrasi:");
        for (id, _prompt) in &tasks {
            let Some((_, hasil)) = hasil_per_task.iter().find(|(i, _)| i == id) else {
                continue;
            };
            if hasil.is_error {
                gagal += 1;
            }
            let teks: String = hasil.text.chars().take(400).collect();
            ringkasan.push_str(&format!(
                "\n[{id}] {} ({} turn, {} tool) {}",
                if hasil.is_error { "GAGAL" } else { "ok" },
                hasil.turns,
                hasil.tool_calls,
                teks
            ));
        }
        ringkasan.push_str(&format!(
            "\nselesai: {} ok, {gagal} gagal dari {} task",
            tasks.len() - gagal,
            tasks.len()
        ));
        if gagal > 0 {
            Ok(ToolOutput::err(ringkasan))
        } else {
            Ok(ToolOutput::ok(ringkasan))
        }
    }
}

/// Tunggu satu anak sampai turn pertama selesai (status ≠ Running) lalu hasilnya.
async fn tunggu_hasil(handle: &manager::SubagentHandle) -> SubagentResult {
    loop {
        if handle.status() != manager::SubagentState::Running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    handle.result().unwrap_or(SubagentResult {
        text: String::new(),
        is_error: true,
        reason: Some("tanpa hasil".to_string()),
        turns: 0,
        tool_calls: 0,
    })
}

/// Ambil teks assistant terakhir (dipakai host mengambil hasil anak).
pub fn last_assistant_text(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|pesan| match pesan {
            Message::Assistant { content, .. } => content.iter().find_map(|blok| match blok {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod orchestrator_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tui_provider::{ChatRequest, GatewayError, Provider};
    use tui_wire::{AssistantMessage, ProviderEvent};

    struct SkripProvider {
        script: Mutex<VecDeque<AssistantMessage>>,
    }

    #[async_trait::async_trait]
    impl Provider for SkripProvider {
        async fn stream_chat(
            &self,
            _request: ChatRequest,
        ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ProviderEvent>, GatewayError> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let pesan = self
                .script
                .lock()
                .ok()
                .and_then(|mut antre| antre.pop_front());
            if let Some(pesan) = pesan {
                let _ = tx.send(ProviderEvent::Done { message: pesan });
            }
            Ok(rx)
        }
    }

    fn teks(isi: &str) -> AssistantMessage {
        AssistantMessage::text(isi.to_string(), tui_wire::StopReason::Stop)
    }

    fn spec_dengan(script: Vec<AssistantMessage>) -> SubagentSpec {
        SubagentSpec {
            provider: Arc::new(SkripProvider {
                script: Mutex::new(script.into()),
            }),
            approver: Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Once,
            )),
            tools: ToolRegistry::from_tools(Vec::new()),
            model: "m".to_string(),
            deadline: std::time::Duration::from_secs(30),
            max_tool_calls: 4,
        }
    }

    #[tokio::test]
    async fn orchestrate_paralel_memunculkan_semua_anak() {
        let manager = Arc::new(manager::SubagentManager::new());
        // Dua task, dua jawaban berbeda — spawn dua anak (spec per task).
        let spec_a = spec_dengan(vec![teks("laporan A")]);
        let spec_b = spec_dengan(vec![teks("laporan B")]);
        let tool = OrchestratorTool::new(spec_a.clone(), manager.clone());
        // Spec di tool dipakai semua task; skrip di spec_a satu pesan — task kedua
        // memakai spec yang sama → jawaban kosong? Orchestrator clone spec per
        // spawn: provider dibagikan → skrip habis di task pertama. Untuk uji
        // ganda, provider skrip harus per task — di luar kemampuan spec tunggal;
        // cukup verifikasi dua anak ter-spawn dan hasil terkumpul.
        let args = serde_json::json!({
            "tasks": [
                {"id": "riset", "prompt": "riset a"},
                {"id": "ringkas", "prompt": "ringkas b"}
            ],
            "parallel": true
        });
        let ctx = ToolContext::new(".");
        let out = tool.execute(&args, &ctx).await.expect("execute");
        // Ringkasan mencakup kedua task.
        assert!(out.content.contains("[riset]"), "{}", out.content);
        assert!(out.content.contains("[ringkas]"), "{}", out.content);
        assert!(out.content.contains("dari 2 task"), "{}", out.content);
        // Semua anak terlihat di roster (Agent Hub).
        assert_eq!(manager.list().len(), 2);
        // Status tidak lagi Running (turn selesai → idle).
        for handle in manager.list() {
            assert_ne!(
                handle.status(),
                manager::SubagentState::Running,
                "{}",
                handle.id
            );
        }
        let _ = spec_b;
    }

    #[tokio::test]
    async fn orchestrate_validasi_input() {
        let manager = Arc::new(manager::SubagentManager::new());
        let tool = OrchestratorTool::new(spec_dengan(vec![]), manager);
        let ctx = ToolContext::new(".");
        // Kosong.
        let kosong = tool
            .execute(&serde_json::json!({"tasks": []}), &ctx)
            .await
            .expect("execute");
        assert!(kosong.is_error);
        // Lebih dari 8.
        let banyak: Vec<Value> = (0..9)
            .map(|i| serde_json::json!({"id": i, "prompt": "p"}))
            .collect();
        let penuh = tool
            .execute(&serde_json::json!({"tasks": banyak}), &ctx)
            .await
            .expect("execute");
        assert!(penuh.is_error);
        assert!(penuh.content.contains("terlalu banyak"));
        // Prompt kosong.
        let kosong_prompt = tool
            .execute(
                &serde_json::json!({"tasks": [{"id": "x", "prompt": " "}]}),
                &ctx,
            )
            .await
            .expect("execute");
        assert!(kosong_prompt.is_error);
        // Tanpa tasks → error bernama (bukan ToolOutput).
        let tanpa = tool.execute(&serde_json::json!({}), &ctx).await;
        assert!(tanpa.is_err(), "tasks wajib ada");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tui_wire::AssistantMessage;

    /// Provider skrip sederhana; pesan berikutnya dikirim saat turn berikutnya.
    struct UjiProvider {
        script: Mutex<VecDeque<AssistantMessage>>,
    }

    impl UjiProvider {
        fn baru(script: Vec<AssistantMessage>) -> Self {
            Self {
                script: Mutex::new(script.into()),
            }
        }
    }

    #[async_trait]
    impl Provider for UjiProvider {
        async fn stream_chat(
            &self,
            _request: tui_provider::ChatRequest,
        ) -> Result<
            tokio::sync::mpsc::UnboundedReceiver<tui_wire::ProviderEvent>,
            tui_provider::GatewayError,
        > {
            let (tx, rx) = unbounded_channel();
            if let Ok(mut antre) = self.script.lock()
                && let Some(pesan) = antre.pop_front()
            {
                let _ = tx.send(tui_wire::ProviderEvent::Done { message: pesan });
            }
            Ok(rx)
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
            usage: tui_wire::Usage::default(),
            error: None,
        }
    }

    fn spec(script: Vec<AssistantMessage>, tools: ToolRegistry) -> SubagentSpec {
        SubagentSpec {
            provider: Arc::new(UjiProvider::baru(script)),
            approver: Arc::new(tui_approval::FixedApprover::new(
                tui_approval::Decision::Once,
            )),
            tools,
            model: "model-uji".to_string(),
            deadline: Duration::from_secs(10),
            max_tool_calls: 8,
        }
    }

    #[tokio::test]
    async fn subagent_satu_turn_mengembalikan_teks() {
        let hasil = run_subagent(
            &spec(vec![teks("jawaban anak")], ToolRegistry::v1()),
            "halo",
        )
        .await;
        assert!(!hasil.is_error);
        assert_eq!(hasil.text, "jawaban anak");
        assert_eq!(hasil.turns, 1);
        assert_eq!(hasil.tool_calls, 0);
    }

    #[tokio::test]
    async fn subagent_menjalankan_tool_dan_melanjutkan() {
        let tools = ToolRegistry::v1();
        let script = vec![
            tool_call("c1", "todo", r#"{"items":["uji"]}"#),
            teks("todo tersimpan"),
        ];
        let hasil = run_subagent(&spec(script, tools), "simpan todo").await;
        assert!(!hasil.is_error, "{:?}", hasil.reason);
        assert_eq!(hasil.tool_calls, 1);
        assert_eq!(hasil.text, "todo tersimpan");
    }

    #[tokio::test]
    async fn tool_subagent_eksekusi_menghasilkan_ringkasan_dan_teks() {
        let tool = SubagentTool::new(spec(vec![teks("hasil anak")], ToolRegistry::v1()));
        let ctx = ToolContext::new(".");
        let out = tool
            .execute(&json!({"prompt": "kerja"}), &ctx)
            .await
            .expect("eksekusi");
        assert!(!out.is_error);
        assert!(out.content.contains("1 turn"), "{}", out.content);
        assert!(out.content.contains("hasil anak"), "{}", out.content);
    }

    #[tokio::test]
    async fn prompt_kosong_adalah_error_hasil() {
        let tool = SubagentTool::new(spec(vec![teks("x")], ToolRegistry::v1()));
        let ctx = ToolContext::new(".");
        let out = tool
            .execute(&json!({"prompt": "   "}), &ctx)
            .await
            .expect("eksekusi");
        assert!(out.is_error);
        assert!(out.content.contains("kosong"));
    }

    #[test]
    fn count_dan_last_text_membaca_transkrip() {
        let pesan = vec![
            Message::user("tanya"),
            Message::assistant(&tool_call("c1", "read", "{}")),
            Message::tool_result("c1", "read", "isi", false, false),
            Message::assistant(&teks("akhir")),
        ];
        assert_eq!(count_tool_calls(&pesan), 1);
        assert_eq!(last_assistant_text(&pesan), "akhir");
    }
}
