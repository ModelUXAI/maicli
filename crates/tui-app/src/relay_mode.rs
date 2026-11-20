//! Relay WebSocket collab (adopsi omp ws relay, v1).
//!
//! `maicli --mode relay --port N` menjalankan **satu room** di loopback:
//! - klien pertama per room = **control** (boleh prompt/abort);
//! - klien berikutnya = **guest** (view saja — menerima siaran event);
//! - control dapat mengangkat guest via `{"promote":true}`;
//! - kunci room: env `MAICLI_RELAY_KEY` menang, bila kosong kunci acak ditulis ke
//!   `<config_root>/relay-key` (izin pengguna saja) — room TIDAK pernah
//!   terbuka tanpa kunci.
//!
//! Protokol (JSON teks):
//! - klien → server: `{"join":"<kunci>"}`, `{"prompt":"..."}` (control),
//!   `{"abort":true}` (control), `{"promote":true}` (control),
//! - server → klien: `{"event":"...","detail":{...}}` (siaran), jawaban turn
//!   `{"event":"jawab","detail":{...}}`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tui_wire::MxError;

/// Galat konfigurasi.
fn galat_config(pesan: String) -> MxError {
    MxError::new(tui_wire::ErrorCode::Config, pesan)
}

/// Jalankan server relay sampai proses dihentikan.
pub async fn run(
    config: tui_config::Config,
    port: u16,
    cwd: PathBuf,
    api_key: String,
    skills_dimuat: Vec<tui_tools::skills::Skill>,
) -> Result<(), MxError> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| galat_config(format!("tidak bisa bind {addr}: {e}")))?;
    // Kunci room: env menang; bila kosong, acak ditulis ke berkas relay-key.
    let mut kunci = std::env::var("MAICLI_RELAY_KEY").unwrap_or_default();
    let jalur_kunci = crate::config_root().join("relay-key");
    if kunci.is_empty() {
        match std::fs::read_to_string(&jalur_kunci) {
            Ok(ada) => kunci = ada.trim().to_string(),
            Err(_) => {
                let baru = format!(
                    "maicli-relay-{:x}{:x}{:x}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|n| n.as_nanos())
                        .unwrap_or(0),
                    port
                );
                if let Some(induk) = jalur_kunci.parent() {
                    let _ = std::fs::create_dir_all(induk);
                }
                let _ = std::fs::write(&jalur_kunci, &baru);
                let _ = tui_wire::secure_file::restrict_to_current_user(&jalur_kunci);
                kunci = baru;
            }
        }
    }
    eprintln!(
        "maicli relay mendengarkan ws://{addr} (model {})",
        config.models.default
    );
    if std::env::var("MAICLI_RELAY_KEY").is_ok() {
        eprintln!("kunci room: dari env MAICLI_RELAY_KEY");
    } else {
        eprintln!("kunci room disimpan: {}", jalur_kunci.display());
    }
    let tools = Arc::new(tui_tools::ToolRegistry::v1());
    let approver = Arc::new(tui_approval::PolicyApprover::new(
        tui_approval::ApprovalPolicy::from(config.approval.clone()),
    ));
    let client: Arc<dyn tui_provider::Provider> = Arc::new(
        tui_provider::OpenAiCompatClient::new(&config.gateway.base_url, Some(api_key))
            .map_err(|e| MxError::new(tui_wire::ErrorCode::GatewayUnreachable, e.message))?,
    );
    let room = Arc::new(Room {
        kunci,
        model: config.models.default.clone(),
        client,
        tools,
        approver,
        cwd,
        skills: skills_dimuat,
        pesan: std::sync::Mutex::new(Vec::new()),
        usage: std::sync::Mutex::new(tui_wire::Usage::default()),
        aktif: std::sync::Mutex::new(None),
        klien: std::sync::Mutex::new(HashMap::new()),
        seq: std::sync::atomic::AtomicU64::new(1),
    });
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let room = room.clone();
        tokio::spawn(async move {
            let _ = layani_klien(stream, room).await;
        });
    }
}

/// Satu room relay: sesi stateful + daftar klien + kunci.
struct Room {
    kunci: String,
    model: String,
    client: Arc<dyn tui_provider::Provider>,
    tools: Arc<tui_tools::ToolRegistry>,
    approver: Arc<dyn tui_approval::Approver>,
    cwd: PathBuf,
    skills: Vec<tui_tools::skills::Skill>,
    /// Transkrip panjang (konten sesi antar turn).
    pesan: std::sync::Mutex<Vec<tui_wire::Message>>,
    usage: std::sync::Mutex<tui_wire::Usage>,
    /// Cancel token turn berjalan.
    aktif: std::sync::Mutex<Option<tui_tools::CancelToken>>,
    /// Klien terhubung: id → pengirim WS + peran.
    klien: std::sync::Mutex<HashMap<String, (UnboundedSender<WsMessage>, Peran)>>,
    /// Penomoran koneksi.
    seq: std::sync::atomic::AtomicU64,
}

/// Peran klien di room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Peran {
    /// View + control (klien pertama).
    Control,
    /// Hanya melihat siaran.
    Guest,
}

/// Siarkan pesan JSON ke semua klien terhubung.
fn siarkan(room: &Room, isi: &str) {
    if let Ok(klien) = room.klien.lock() {
        for (tx, _) in klien.values() {
            let _ = tx.send(WsMessage::text(isi.to_string()));
        }
    }
}

/// Layani satu koneksi WebSocket: handshake join → loop pesan.
async fn layani_klien(stream: TcpStream, room: Arc<Room>) -> Result<(), MxError> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| galat_config(format!("handshake WS gagal: {e}")))?;
    let (mut penulis, mut pembaca) = ws.split();
    // Handshake: pesan pertama = join + kunci.
    let joined = tokio::time::timeout(Duration::from_secs(10), pembaca.next()).await;
    let Some(Ok(WsMessage::Text(teks))) = joined.ok().flatten() else {
        return Ok(());
    };
    let join: Value = serde_json::from_str(&teks).unwrap_or(Value::Null);
    if join.get("join").and_then(Value::as_str) != Some(room.kunci.as_str()) {
        let _ = penulis
            .send(WsMessage::text(r#"{"error":"kunci room salah"}"#))
            .await;
        return Ok(());
    }
    let id = format!(
        "klien-{}",
        room.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let peran = {
        let mut klien = room
            .klien
            .lock()
            .map_err(|_| galat_config("room terkunci".into()))?;
        let peran = if klien.values().any(|(_, p)| *p == Peran::Control) {
            Peran::Guest
        } else {
            Peran::Control
        };
        let (tx, mut rx) = unbounded_channel::<WsMessage>();
        klien.insert(id.clone(), (tx, peran));
        // Task penulis: kirim siaran dari kanal.
        tokio::spawn(async move {
            while let Some(pesan) = rx.recv().await {
                if penulis.send(pesan).await.is_err() {
                    break;
                }
            }
        });
        peran
    };
    siarkan(
        &room,
        &json!({"event":"join","detail":{"id":id,"peran":format!("{peran:?}")}}).to_string(),
    );
    // Loop baca pesan klien.
    while let Some(Ok(WsMessage::Text(teks))) = pembaca.next().await {
        let pesan: Value = serde_json::from_str(&teks).unwrap_or(Value::Null);
        let peran_kini = room
            .klien
            .lock()
            .ok()
            .and_then(|k| k.get(&id).map(|(_, p)| *p))
            .unwrap_or(Peran::Guest);
        if let Some(prompt) = pesan.get("prompt").and_then(Value::as_str) {
            if peran_kini != Peran::Control {
                let _ = room.klien.lock().ok().and_then(|k| {
                    k.get(&id).map(|(tx, _)| {
                        tx.send(WsMessage::text(r#"{"error":"guest tidak boleh prompt"}"#))
                    })
                });
                continue;
            }
            jalankan_turn(&room, prompt.to_string()).await;
        } else if pesan.get("abort").and_then(Value::as_bool) == Some(true) {
            if peran_kini == Peran::Control
                && let Ok(guard) = room.aktif.lock()
                && let Some(token) = &*guard
            {
                token.cancel();
            }
        } else if pesan.get("promote").and_then(Value::as_bool) == Some(true)
            && peran_kini == Peran::Control
        {
            // v1: control lama turun jadi guest; guest pertama lain naik.
            if let Ok(mut klien) = room.klien.lock() {
                if let Some((_, p)) = klien.get_mut(&id) {
                    *p = Peran::Guest;
                }
                let mut dinaikkan = false;
                for (cid, (_, p)) in klien.iter_mut() {
                    if !dinaikkan && cid != &id && *p == Peran::Guest {
                        *p = Peran::Control;
                        dinaikkan = true;
                    }
                }
            }
            siarkan(
                &room,
                &json!({"event":"promote","detail":{"oleh":id}}).to_string(),
            );
        }
    }
    // Klien keluar: hapus dari room.
    if let Ok(mut klien) = room.klien.lock() {
        klien.remove(&id);
    }
    siarkan(
        &room,
        &json!({"event":"keluar","detail":{"id":id}}).to_string(),
    );
    Ok(())
}

/// Jalankan satu turn (control meminta prompt): sesi stateful, siarkan event.
async fn jalankan_turn(room: &Arc<Room>, pesan: String) {
    let transkrip = room.pesan.lock().map(|g| g.clone()).unwrap_or_default();
    let usage_awal = room.usage.lock().map(|g| *g).unwrap_or_default();
    let builder = tui_sdk::SessionBuilder::new(&room.cwd)
        .provider(room.client.clone())
        .approver(room.approver.clone())
        .tools((*room.tools).clone())
        .model(room.model.clone())
        .settings(tui_core::TurnSettings {
            model: room.model.clone(),
            system_prompt: {
                let mut sp = tui_core::TurnSettings::default().system_prompt;
                sp.push_str(&tui_tools::skills::ringkas_daftar(&room.skills));
                sp
            },
            ..tui_core::TurnSettings::default()
        })
        .interactive(false);
    let Ok((mut sesi, mut stream_event)) = builder.build() else {
        siarkan(room, r#"{"event":"galat","detail":"sesi gagal dibuat"}"#);
        return;
    };
    sesi.push_messages(transkrip);
    sesi.seed_usage(usage_awal);
    let token = sesi.cancel_token();
    if let Ok(mut g) = room.aktif.lock() {
        *g = Some(token);
    }
    siarkan(room, &json!({"event":"prompt","message":pesan}).to_string());
    let hasil_jalan = sesi.prompt(pesan.clone()).await;
    // Kumpulkan event sesi → siarkan.
    while let Some(event) = stream_event.try_next()
        && let Some((nama, detail)) = ringkas_event(&event)
    {
        siarkan(room, &json!({"event":nama,"detail":detail}).to_string());
    }
    // Simpan konten + bersihkan penanda aktif + siarkan jawaban.
    if let Ok(mut p) = room.pesan.lock() {
        *p = sesi.messages().to_vec();
    }
    if let Ok(mut u) = room.usage.lock() {
        *u = sesi.usage();
    }
    if let Ok(mut g) = room.aktif.lock() {
        *g = None;
    }
    match hasil_jalan {
        Ok(outcome) => {
            let teks = sesi
                .messages()
                .iter()
                .rev()
                .find_map(|m| match m {
                    tui_wire::Message::Assistant { content, .. } => {
                        content.iter().find_map(|b| match b {
                            tui_wire::ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .unwrap_or_default();
            siarkan(
                room,
                &json!({
                    "event":"jawab",
                    "detail":{"status":format!("{:?}", outcome.stop_reason),"turns":outcome.turns,"text":teks}
                })
                .to_string(),
            );
        }
        Err(err) => {
            siarkan(
                room,
                &json!({"event":"galat","detail":{"code":err.code.to_string(),"pesan":err.message}})
                    .to_string(),
            );
        }
    }
}

/// Ringkas satu event sesi menjadi (nama, detail JSON) untuk siaran.
fn ringkas_event(event: &tui_wire::AgentEvent) -> Option<(String, Value)> {
    match event {
        tui_wire::AgentEvent::AgentStart => Some(("agent_start".into(), json!({}))),
        tui_wire::AgentEvent::AgentEnd { turns } => {
            Some(("agent_end".into(), json!({"turns": turns})))
        }
        tui_wire::AgentEvent::ToolExecutionStart {
            tool_name, preview, ..
        } => Some((
            "tool_start".into(),
            json!({"tool": tool_name, "preview": preview}),
        )),
        tui_wire::AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            preview,
            ..
        } => Some((
            "tool_end".into(),
            json!({"tool": tool_name, "is_error": is_error, "preview": preview}),
        )),
        tui_wire::AgentEvent::Notice { text } => Some(("notice".into(), json!(text))),
        _ => None,
    }
}
