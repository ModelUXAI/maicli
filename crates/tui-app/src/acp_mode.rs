//! Mode ACP (Agent Client Protocol, subagent D) — agen `maicli` sebagai
//! **server ACP** di stdin/stdout: host eksternal (Zed dsb.) **dan** lokal
//! (pipe) mengendalikan agen.
//!
//! Protokol: JSON-RPC 2.0 **line-delimited** (satu objek per baris, tanpa
//! header):
//! - `initialize` → `{protocolVersion: 1, agentCapabilities}`,
//! - `session/new` → `{sessionId}`,
//! - `session/prompt` (params `{sessionId, prompt:[{type:"text",text}]}`) →
//!   selama turn: notifikasi **`session/update`** (`agent_message_chunk`,
//!   `tool_call`, `tool_call_update`), selesai → result `{stopReason}`,
//! - `session/cancel` (params `{sessionId}`) → batalkan turn berjalan.
//!
//! Sesi yang sedang berjalan **dipindahkan ke task turn** (ownership); cancel
//! bekerja lewat cancel token terpisah sehingga tidak perlu menunggu lock.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tui_wire::{AgentEvent, ErrorCode, MxError};

/// Protokol ACP yang diimplementasi.
const PROTOCOLE_VERSION: u32 = 1;

/// Slot satu sesi: bila `hidup` = None, sesi sedang dipakai task turn.
struct SlotSesi {
    hidup: tokio::sync::Mutex<Option<(tui_sdk::Session, tui_sdk::EventStream)>>,
    /// Cancel token sesi (tetap terpasang; bekerja saat turn jalan).
    token: tui_tools::CancelToken,
}

/// Konteks bersama semua sesi.
struct AcpCtx {
    client: Arc<dyn tui_provider::Provider>,
    approver: Arc<dyn tui_approval::Approver>,
    tools: Arc<tui_tools::ToolRegistry>,
    cwd: PathBuf,
    model: String,
    skills: Vec<tui_tools::skills::Skill>,
    sesi: tokio::sync::Mutex<HashMap<String, Arc<SlotSesi>>>,
}

/// Jalankan mode ACP sampai stdin ditutup.
pub async fn run(config: tui_config::Config, cwd: PathBuf, api_key: String) -> Result<(), MxError> {
    let client: Arc<dyn tui_provider::Provider> = Arc::new(
        tui_provider::OpenAiCompatClient::new(&config.gateway.base_url, Some(api_key))
            .map_err(|e| MxError::new(ErrorCode::GatewayUnreachable, e.message))?,
    );
    let approver: Arc<dyn tui_approval::Approver> = Arc::new(tui_approval::PolicyApprover::new(
        tui_approval::ApprovalPolicy::from(config.approval.clone()),
    ));
    let tools = Arc::new(tui_tools::ToolRegistry::v1());
    let ctx = Arc::new(AcpCtx {
        client,
        approver,
        tools,
        cwd: cwd.clone(),
        model: config.models.default.clone(),
        skills: tui_tools::skills::temukan(&cwd),
        sesi: tokio::sync::Mutex::new(HashMap::new()),
    });
    // Task penulis tunggal: balasan + update keluar berurutan tanpa interleave.
    let (keluar_tx, mut keluar_rx) = unbounded_channel::<String>();
    let penulis = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(baris) = keluar_rx.recv().await {
            let _ = stdout.write_all(baris.as_bytes()).await;
            let _ = stdout.write_all(b"\n").await;
            let _ = stdout.flush().await;
        }
    });
    let mut baris = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(teks)) = baris.next_line().await {
        if teks.trim().is_empty() {
            continue;
        }
        let Ok(pesan) = serde_json::from_str::<Value>(&teks) else {
            kirim_galat(&keluar_tx, Value::Null, -32700, "bukan JSON valid");
            continue;
        };
        tangani(&ctx, &keluar_tx, pesan).await;
    }
    penulis.abort();
    Ok(())
}

/// Kirim balasan JSON-RPC (result).
fn kirim_balas(tx: &UnboundedSender<String>, id: Value, result: Value) {
    let _ = tx.send(json!({"jsonrpc":"2.0","id":id,"result":result}).to_string());
}

/// Kirim galat JSON-RPC.
fn kirim_galat(tx: &UnboundedSender<String>, id: Value, kode: i64, pesan: &str) {
    let _ =
        tx.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":kode,"message":pesan}}).to_string());
}

/// Kirim notifikasi `session/update`.
fn kirim_update(tx: &UnboundedSender<String>, sesi_id: &str, update: Value) {
    let _ = tx.send(
        json!({
            "jsonrpc":"2.0",
            "method":"session/update",
            "params":{"sessionId":sesi_id,"update":update}
        })
        .to_string(),
    );
}

/// Tangani satu pesan JSON-RPC masuk.
async fn tangani(ctx: &AcpCtx, tx: &UnboundedSender<String>, pesan: Value) {
    let metode = pesan.get("method").and_then(Value::as_str).unwrap_or("");
    let id = pesan.get("id").cloned().unwrap_or(Value::Null);
    let params = pesan.get("params").cloned().unwrap_or(Value::Null);
    match metode {
        "initialize" => {
            kirim_balas(
                tx,
                id,
                json!({
                    "protocolVersion": PROTOCOLE_VERSION,
                    "agentCapabilities": {"loadSession": false},
                }),
            );
        }
        "session/new" => {
            let sid = format!(
                "ses-{:x}{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|n| n.as_nanos())
                    .unwrap_or(0)
            );
            let builder = tui_sdk::SessionBuilder::new(&ctx.cwd)
                .provider(ctx.client.clone())
                .approver(ctx.approver.clone())
                .tools((*ctx.tools).clone())
                .model(ctx.model.clone())
                .settings(tui_core::TurnSettings {
                    model: ctx.model.clone(),
                    system_prompt: {
                        let mut sp = tui_core::TurnSettings::default().system_prompt;
                        sp.push_str(&tui_tools::skills::ringkas_daftar(&ctx.skills));
                        sp
                    },
                    ..tui_core::TurnSettings::default()
                })
                .interactive(false);
            match builder.build() {
                Ok((sesi, events)) => {
                    let token = sesi.cancel_token();
                    ctx.sesi.lock().await.insert(
                        sid.clone(),
                        Arc::new(SlotSesi {
                            hidup: tokio::sync::Mutex::new(Some((sesi, events))),
                            token,
                        }),
                    );
                    kirim_balas(tx, id, json!({"sessionId": sid}));
                }
                Err(err) => kirim_galat(tx, id, -32000, &err.message),
            }
        }
        "session/prompt" => {
            let sesi_id = params
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let teks = params
                .get("prompt")
                .and_then(Value::as_array)
                .map(|blok| {
                    blok.iter()
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect::<Vec<&str>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if sesi_id.is_empty() || teks.is_empty() {
                kirim_galat(tx, id, -32602, "sessionId dan prompt (blok teks) wajib");
                return;
            }
            let slot = ctx.sesi.lock().await.get(&sesi_id).cloned();
            let Some(slot) = slot else {
                kirim_galat(tx, id, -32001, "sessionId tidak dikenal");
                return;
            };
            let mut hidup = slot.hidup.lock().await;
            let Some((mut sesi, mut events)) = hidup.take() else {
                kirim_galat(tx, id, -32002, "sesi sedang menjalankan prompt");
                return;
            };
            drop(hidup);
            let tx2 = tx.clone();
            let sid2 = sesi_id.clone();
            tokio::spawn(async move {
                // Scope run: pinjaman sesi berakhir di sini → sesi bebas dipindah.
                let selesai: Result<tui_core::TurnOutcome, MxError> = {
                    let mut run = std::pin::pin!(sesi.prompt(teks));
                    loop {
                        tokio::select! {
                            outcome = &mut run => break outcome,
                            event = events.next() => {
                                match event {
                                    Some(event) => {
                                        if let Some(update) = event_ke_update(&event) {
                                            kirim_update(&tx2, &sid2, update);
                                        }
                                    }
                                    None => {
                                        break Err(MxError::new(
                                            ErrorCode::ToolFailed,
                                            "kanal event sesi tertutup",
                                        ));
                                    }
                                }
                            }
                        }
                    }
                };
                // Kembalikan sesi (konteks utuh) ke slot untuk prompt berikutnya.
                if let Ok(mut hidup) = slot.hidup.try_lock() {
                    *hidup = Some((sesi, events));
                }
                match selesai {
                    Ok(outcome) => {
                        kirim_balas(
                            &tx2,
                            id,
                            json!({"stopReason": if outcome.stop_reason == tui_wire::StopReason::Aborted { "aborted" } else { "end_turn" }}),
                        );
                    }
                    Err(err) => kirim_galat(&tx2, id, -32003, &err.message),
                }
            });
        }
        "session/cancel" => {
            let sesi_id = params
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let slot = ctx.sesi.lock().await.get(sesi_id).cloned();
            if let Some(slot) = slot {
                slot.token.cancel();
            }
            if id != Value::Null {
                kirim_balas(tx, id, json!({}));
            }
        }
        lain => {
            kirim_galat(tx, id, -32601, &format!("metode tidak dikenal: {lain}"));
        }
    }
}

/// Petakan event sesi ke update ACP.
fn event_ke_update(event: &AgentEvent) -> Option<Value> {
    match event {
        AgentEvent::AgentStart => Some(json!({"sessionUpdate":"started"})),
        AgentEvent::AgentEnd { turns } => Some(json!({"sessionUpdate":"finished","turns":turns})),
        AgentEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            preview,
        } => Some(json!({
            "sessionUpdate":"tool_call",
            "toolCallId": tool_call_id,
            "title": tool_name,
            "rawInput": preview,
        })),
        AgentEvent::ToolExecutionEnd {
            tool_call_id,
            is_error,
            preview,
            ..
        } => Some(json!({
            "sessionUpdate":"tool_call_update",
            "toolCallId": tool_call_id,
            "status": if *is_error { "failed" } else { "completed" },
            "rawOutput": preview,
        })),
        AgentEvent::Notice { text } => Some(json!({
            "sessionUpdate":"agent_thought_chunk",
            "content": {"type":"text","text":text}
        })),
        _ => None,
    }
}
