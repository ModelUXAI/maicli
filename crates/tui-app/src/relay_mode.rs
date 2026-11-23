//! Relay WebSocket collab (adopsi omp ws relay, v1).
//!
//! `maicli --mode relay --port N` menjalankan **multi-room** di loopback
//! (room dipilih lewat `{"join":"<room>:<kunci>"}`):
//! - klien pertama per room = **control** (boleh prompt/abort);
//! - klien berikutnya = **guest** (view saja — menerima siaran event);
//! - control dapat mengangkat guest via `{"promote":true}`;
//! - kunci room: env `MAICLI_RELAY_KEY` mendukung `room=kunci,room2=kunci2`;
//!   format lama (satu kunci) = room `default`; tanpa env, kunci acak ditulis ke
//!   `<config_root>/relay-key` (izin pengguna saja) — room TIDAK pernah
//!   terbuka tanpa kunci.
//!
//! Protokol (JSON teks):
//! - klien → server: `{"join":"<room>:<kunci>"}` (atau kunci saja = `default`),
//!   `{"prompt":"..."}` (control), `{"abort":true}` (control),
//!   `{"promote":true}` (control), `{"view":true}` (guest murni),
//! - server → klien: `{"event":"...","detail":{...}}` (siaran), jawaban turn
//!   `{"event":"jawab","detail":{...}}`, `{"event":"join",...}`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tui_wire::MxError;

/// Galat konfigurasi.
fn galat_config(pesan: String) -> MxError {
    MxError::new(tui_wire::ErrorCode::Config, pesan)
}

/// QR SVG untuk URL relay (None bila encoding gagal).
pub fn qr_svg(teks: &str) -> Option<String> {
    let kode = qrcode::QrCode::new(teks.as_bytes()).ok()?;
    Some(
        kode.render::<qrcode::render::svg::Color>()
            .min_dimensions(180, 180)
            .build(),
    )
}

/// JSON info relay: URL, daftar room (tanpa kunci), dan QR SVG.
fn info_relay(addr: &str, public: &str, rooms: &HashMap<String, Arc<Room>>) -> String {
    let url = if public.trim().is_empty() {
        format!("http://{addr}/")
    } else {
        format!("{}/", public.trim().trim_end_matches('/'))
    };
    let nama: Vec<String> = rooms.keys().cloned().collect();
    let qr = qr_svg(&url).unwrap_or_default();
    json!({"url": url, "rooms": nama, "qr_svg": qr}).to_string()
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
    // Kunci room: env `MAICLI_RELAY_KEY` mendukung `room=kunci,room2=kunci2`.
    // Format lama (satu kunci) = room `default`. Bila kosong, kunci acak untuk
    // `default` ditulis ke `<data>/relay-key` (0600).
    let jalur_kunci = crate::config_root().join("relay-key");
    let mut rooms: HashMap<String, String> = HashMap::new();
    let env_kunci = std::env::var("MAICLI_RELAY_KEY").unwrap_or_default();
    let sumber = if env_kunci.is_empty() {
        std::fs::read_to_string(&jalur_kunci).unwrap_or_default()
    } else {
        env_kunci.clone()
    };
    for bagian in sumber.split([',', '\n']) {
        let bagian = bagian.trim();
        if bagian.is_empty() {
            continue;
        }
        match bagian.split_once('=') {
            Some((room, kunci)) if !room.trim().is_empty() => {
                rooms.insert(room.trim().to_string(), kunci.trim().to_string());
            }
            _ => {
                rooms.insert("default".to_string(), bagian.to_string());
            }
        }
    }
    if rooms.is_empty() {
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
        rooms.insert("default".to_string(), baru);
    }
    eprintln!(
        "maicli relay mendengarkan ws://{addr} (model {})",
        config.models.default
    );
    if std::env::var("MAICLI_RELAY_KEY").is_ok() {
        eprintln!(
            "kunci room: dari env MAICLI_RELAY_KEY ({} room)",
            rooms.len()
        );
    } else {
        eprintln!(
            "kunci room disimpan: {} ({} room)",
            jalur_kunci.display(),
            rooms.len()
        );
    }
    let tools = Arc::new(tui_tools::ToolRegistry::v1());
    let approver = Arc::new(tui_approval::PolicyApprover::new(
        tui_approval::ApprovalPolicy::from(config.approval.clone()),
    ));
    let client: Arc<dyn tui_provider::Provider> = Arc::new(
        tui_provider::OpenAiCompatClient::new(&config.gateway.base_url, Some(api_key))
            .map_err(|e| MxError::new(tui_wire::ErrorCode::GatewayUnreachable, e.message))?,
    );
    let rooms: Arc<HashMap<String, Arc<Room>>> = Arc::new(
        rooms
            .into_iter()
            .map(|(nama, kunci)| {
                let nama_room = nama.clone();
                (
                    nama,
                    Arc::new(Room {
                        nama: nama_room,
                        kunci: std::sync::Mutex::new(kunci),
                        model: config.models.default.clone(),
                        client: client.clone(),
                        tools: tools.clone(),
                        approver: approver.clone(),
                        cwd: cwd.clone(),
                        skills: skills_dimuat.clone(),
                        pesan: std::sync::Mutex::new(Vec::new()),
                        usage: std::sync::Mutex::new(tui_wire::Usage::default()),
                        aktif: std::sync::Mutex::new(None),
                        klien: std::sync::Mutex::new(HashMap::new()),
                        seq: std::sync::atomic::AtomicU64::new(1),
                    }),
                )
            })
            .collect(),
    );
    let info = Arc::new(info_relay(&addr, &config.collab.remote_url, &rooms));
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let rooms = rooms.clone();
        let info = info.clone();
        // HTTP GET tanpa upgrade = halaman klien web; sisanya WS relay.
        let mut intip = [0u8; 256];
        let n = stream.peek(&mut intip).await.unwrap_or(0);
        let awal = &intip[..n];
        let upgrade = awal
            .windows(9)
            .any(|w| w.eq_ignore_ascii_case(b"websocket"));
        let http = (awal.starts_with(b"GET ") || awal.starts_with(b"POST ")) && !upgrade;
        if http {
            tokio::spawn(async move {
                let _ = layani_http(stream, &info, rooms.clone()).await;
            });
            continue;
        }
        tokio::spawn(async move {
            let _ = layani_klien(stream, rooms).await;
        });
    }
}

/// Satu room relay: sesi stateful + daftar klien + kunci.
struct Room {
    nama: String,
    kunci: std::sync::Mutex<String>,
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
    /// View + control (klien pertama non-view).
    Control,
    /// Hanya melihat siaran; bisa dinaikkan control.
    Guest,
    /// Read-only murni: tidak pernah dinaikkan menjadi control.
    View,
}

/// Siarkan pesan JSON ke semua klien terhubung.
fn siarkan(room: &Room, isi: &str) {
    if let Ok(klien) = room.klien.lock() {
        for (tx, _) in klien.values() {
            let _ = tx.send(WsMessage::text(isi.to_string()));
        }
    }
}

/// Layani satu koneksi WebSocket: pilih room dari frame join, lalu loop pesan.
/// Halaman klien browser relay (lokal; tanpa aset eksternal).
pub fn halaman_web() -> &'static str {
    r#"<!doctype html><html lang="id"><meta charset="utf-8">
<title>maicli relay</title>
<style>body{font:14px system-ui;margin:2rem;max-width:70ch}input,button{font:inherit;margin:.2rem}
pre{background:#111;color:#eee;padding:1rem;white-space:pre-wrap;max-height:70vh;overflow:auto}</style>
<h2>maicli relay</h2><p>Masukkan room dan kunci dari host relay.</p>
<label>room <input id="room" value="default"></label>
<label>kunci <input id="key" type="password"></label>
<button id="join">gabung</button>
<p><a id="link" href="/">buka link relay</a></p><div id="qr"></div>
<pre id="log"></pre>
<script>
const log = (x) => document.getElementById('log').textContent += x + '
';
let ws;
fetch('/api/relay-info').then(r => r.json()).then(info => {
  document.getElementById('link').href = info.url;
  document.getElementById('link').textContent = info.url;
  document.getElementById('qr').innerHTML = info.qr_svg;
}).catch(() => {});
document.getElementById('join').onclick = () => {
  if (ws) ws.close();
  const room = document.getElementById('room').value;
  const key = document.getElementById('key').value;
  ws = new WebSocket((location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + '/');
  ws.onopen = () => ws.send(JSON.stringify({join: room + ':' + key, view: true}));
  ws.onmessage = (e) => log(e.data);
  ws.onclose = () => log('[terputus]');
};
</script></html>"#
}

/// Kunci room baru (cukup acak untuk satu mesin).
fn kunci_baru() -> String {
    format!(
        "maicli-relay-{:x}{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// Layani halaman web relay pada koneksi HTTP biasa.
async fn layani_http(
    mut stream: TcpStream,
    info: &str,
    rooms: Arc<HashMap<String, Arc<Room>>>,
) -> Result<(), MxError> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    let permintaan = String::from_utf8_lossy(&buf[..n]).to_string();
    if permintaan.starts_with("POST /api/relay/revoke") {
        let header = |nama: &str| {
            permintaan.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case(nama)
                    .then(|| v.trim().to_string())
            })
        };
        let room_nama = header("x-relay-room").unwrap_or_else(|| "default".to_string());
        let kunci = header("x-relay-key").unwrap_or_default();
        let hasil = rooms.get(&room_nama).and_then(|room| {
            let mut g = room.kunci.lock().ok()?;
            if *g != kunci {
                return None;
            }
            let baru = kunci_baru();
            *g = baru.clone();
            Some(baru)
        });
        let (status, body) = match hasil {
            Some(kunci) => (
                "200 OK",
                json!({"room": room_nama, "key": kunci}).to_string(),
            ),
            None => (
                "403 Forbidden",
                r#"{"error":"kunci room salah atau room tidak ada"}"#.to_string(),
            ),
        };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(body.as_bytes()).await;
        return Ok(());
    }
    let (tipe, body) = if permintaan.contains("GET /api/relay-info") {
        ("application/json", info.to_string())
    } else {
        ("text/html; charset=utf-8", halaman_web().to_string())
    };
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {tipe}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    Ok(())
}

async fn layani_klien(
    stream: TcpStream,
    rooms: Arc<HashMap<String, Arc<Room>>>,
) -> Result<(), MxError> {
    let ws = tokio_tungstenite::accept_async(stream)
        .await
        .map_err(|e| galat_config(format!("handshake WS gagal: {e}")))?;
    let (mut penulis, mut pembaca) = ws.split();
    // Handshake: `{"join":"room:kunci"}` (format lama tanpa ':' = room default).
    let joined = tokio::time::timeout(Duration::from_secs(10), pembaca.next()).await;
    let Some(Ok(WsMessage::Text(teks))) = joined.ok().flatten() else {
        return Ok(());
    };
    let join: Value = serde_json::from_str(&teks).unwrap_or(Value::Null);
    let Some(join_value) = join.get("join").and_then(Value::as_str) else {
        let _ = penulis
            .send(WsMessage::text(r#"{"error":"frame join wajib"}"#))
            .await;
        return Ok(());
    };
    let (room_nama, kunci) = match join_value.split_once(':') {
        Some((room, kunci)) => (room.to_string(), kunci.to_string()),
        None => ("default".to_string(), join_value.to_string()),
    };
    let Some(room) = rooms.get(&room_nama).cloned() else {
        let _ = penulis
            .send(WsMessage::text(format!(
                r#"{{"error":"room '{room_nama}' tidak ada"}}"#
            )))
            .await;
        return Ok(());
    };
    let kunci_sah = room.kunci.lock().map(|k| *k == kunci).unwrap_or(false);
    if !kunci_sah {
        let _ = penulis
            .send(WsMessage::text(r#"{"error":"kunci room salah"}"#))
            .await;
        return Ok(());
    }
    let minta_view = join.get("view").and_then(Value::as_bool).unwrap_or(false);
    let id = format!(
        "klien-{}",
        room.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let peran = {
        let mut klien = room
            .klien
            .lock()
            .map_err(|_| galat_config("room terkunci".into()))?;
        let ada_control = klien.values().any(|(_, p)| *p == Peran::Control);
        let peran = if minta_view {
            Peran::View
        } else if ada_control {
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
        &json!({
            "event":"join",
            "detail":{"id":id,"peran":format!("{peran:?}"),"room":room.nama}
        })
        .to_string(),
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
    // Klien keluar: hapus dari room. Bila control pergi, naikkan guest
    // pertama (bukan View) agar room tidak kehilangan kendali.
    let peran_keluar = {
        let mut klien = room
            .klien
            .lock()
            .map_err(|_| galat_config("room terkunci".into()))?;
        let peran = klien.remove(&id).map(|(_, peran)| peran);
        if peran == Some(Peran::Control) {
            for (cid, (_, p)) in klien.iter_mut() {
                if *p == Peran::Guest {
                    *p = Peran::Control;
                    siarkan(
                        &room,
                        &json!({"event":"promote","detail":{"oleh":"sistem","id":cid}}).to_string(),
                    );
                    break;
                }
            }
        }
        peran
    };
    siarkan(
        &room,
        &json!({
            "event":"keluar",
            "detail":{"id":id,"peran":format!("{peran_keluar:?}"),"room":room.nama}
        })
        .to_string(),
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

#[cfg(test)]
mod web_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn halaman_memuat_klien_websocket() {
        let html = halaman_web();
        assert!(html.contains("WebSocket"), "tanpa WebSocket");
        assert!(html.contains("join"), "tanpa join");
        assert!(html.contains("wss://"), "tanpa wss");
        assert!(html.contains("api/relay-info"), "tanpa info");
        assert_ne!(kunci_baru(), kunci_baru());
    }

    #[test]
    fn qr_dan_info_relay() {
        let svg = qr_svg("http://127.0.0.1:1/").expect("qr");
        assert!(svg.contains("<svg"), "{svg}");
        let rooms = HashMap::new();
        let info = info_relay("127.0.0.1:1", "", &rooms);
        assert!(info.contains("http://127.0.0.1:1/"), "{info}");
        let publik = info_relay("127.0.0.1:1", "https://relay.example.com", &rooms);
        assert!(publik.contains("https://relay.example.com/"), "{publik}");
    }
}
