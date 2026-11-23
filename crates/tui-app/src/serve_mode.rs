//! API server lokal (`maicli serve`): HTTP/1.1 minimal di loopback saja.
//!
//! Endpoint:
//! - `GET /api/health` — status server (model, jumlah sesi hidup).
//! - `POST /api/prompt` — `{"message": "…"}`: satu turn penuh; jawaban JSON.
//! - `GET /api/sessions` — daftar sesi tersimpan.
//!
//! Keamanan: bind **127.0.0.1 saja**; bila env `MAICLI_SERVE_TOKEN` diset, semua
//! request wajib `Authorization: Bearer <token>`. Tool yang butuh persetujuan
//! manusia ditolak otomatis (server tanpa UI — kebijakan approval dari config
//! tetap berlaku untuk tier yang diizinkan mode).

use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use tui_config::Config;
use tui_provider::OpenAiCompatClient;
use tui_sdk::SessionBuilder;
use tui_tools::ToolRegistry;
use tui_wire::{ErrorCode, MxError};

/// State PKCE yang menunggu callback: state → (provider, verifier, redirect).
type OAuthPending =
    Arc<std::sync::Mutex<std::collections::HashMap<String, (String, String, String)>>>;

/// Hasil satu request terlayani (status untuk diagnostik pemanggil).
struct ServeResult {
    #[allow(dead_code)] // dibaca pemanggil log di masa depan; sekarang hanya kontrak balas()
    status: u16,
    #[allow(dead_code)]
    body: String,
}

/// Ambil satu parameter dari query string (dekode `%XX` dan `+`).
pub fn query_param(path: &str, kunci: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    for pasangan in query.split('&') {
        let (k, v) = pasangan.split_once('=')?;
        if k == kunci {
            let v = v.replace('+', " ");
            let bytes = v.as_bytes();
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%'
                    && i + 2 < bytes.len()
                    && let (Some(a), Some(b)) = (
                        (bytes[i + 1] as char).to_digit(16),
                        (bytes[i + 2] as char).to_digit(16),
                    )
                {
                    out.push((a * 16 + b) as u8);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            return String::from_utf8(out).ok();
        }
    }
    None
}

/// Tambah parameter ke URL (encoding minimal untuk karakter URL).
pub fn tambah_query(url: &str, params: &[(&str, &str)]) -> String {
    let encode = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    let tambahan: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect();
    if tambahan.is_empty() {
        url.to_string()
    } else {
        format!(
            "{}{}{}",
            url,
            if url.contains('?') { '&' } else { '?' },
            tambahan.join("&")
        )
    }
}

/// PKCE `code_challenge` S256.
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// String acak sederhana untuk state/verifier (bukan kunci kriptografis).
pub fn acak() -> String {
    format!(
        "{:x}{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
        std::process::id()
    )
}

/// Riwayat event server (collab v1: tamu memantau lewat `GET /api/events`;
/// WebSocket relay = tahap lanjut yang butuh dependensi WS).
#[derive(Default)]
struct Riwayat {
    buf: std::sync::Mutex<std::collections::VecDeque<String>>,
}

impl Riwayat {
    fn push(&self, baris: String) {
        if let Ok(mut buf) = self.buf.lock() {
            buf.push_back(baris);
            while buf.len() > 1000 {
                buf.pop_front();
            }
        }
    }

    fn json(&self) -> String {
        let baris: Vec<String> = match self.buf.lock() {
            Ok(buf) => buf.iter().cloned().collect(),
            Err(_) => Vec::new(),
        };
        format!("{{\"events\":[{}]}}", baris.join(","))
    }

    /// Potongan mulai `sejak` (indeks absolut riwayat).
    fn sejak(&self, sejak: usize) -> Vec<String> {
        match self.buf.lock() {
            Ok(buf) => buf.iter().skip(sejak).cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Jumlah event tercatat.
    fn total(&self) -> usize {
        self.buf.lock().map(|buf| buf.len()).unwrap_or(0)
    }

    /// SSE push: kirim event baru sampai klien putus (poll 300 ms).
    /// Header `Last-Event-ID` menentukan mulai (0 = seluruh riwayat).
    fn stream_sse(&self, mut stream: TcpStream, sejak_awal: usize) {
        use std::io::Write;
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
        if stream.write_all(head.as_bytes()).is_err() {
            return;
        }
        let _ = stream.flush();
        let mut sejak = sejak_awal.min(self.total());
        loop {
            for baris in self.sejak(sejak) {
                let frame = format!("data: {baris}\n\n");
                if stream.write_all(frame.as_bytes()).is_err() {
                    return;
                }
                sejak += 1;
            }
            let _ = stream.flush();
            if stream.peek(&mut [0u8; 1]).is_ok_and(|n| n == 0) {
                // Klien menutup koneksi.
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
}

/// Jalankan server HTTP sampai proses dihentikan.
pub async fn run(
    config: Config,
    port: u16,
    cwd: PathBuf,
    api_key: String,
    skills_dimuat: Vec<tui_tools::skills::Skill>,
) -> Result<(), MxError> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr).map_err(|e| {
        MxError::new(
            ErrorCode::Config,
            format!("tidak bisa bind {addr}: {e} — port mungkin dipakai proses lain"),
        )
    })?;
    // Token: env `MAICLI_SERVE_TOKEN` menang; bila kosong, token dibaca/dibuat di
    // berkas `serve-token` (0600) di direktori data — sehingga server TIDAK
    // PERNAH berjalan tanpa otentikasi kecuali file dihapus.
    let mut token = std::env::var("MAICLI_SERVE_TOKEN").unwrap_or_default();
    let jalur_token = crate::config_root().join("serve-token");
    if token.is_empty() {
        match std::fs::read_to_string(&jalur_token) {
            Ok(ada) => token = ada.trim().to_string(),
            Err(_) => {
                // Buat token acak sederhana (waktu + pid + alamat listen).
                let baru = format!(
                    "maicli-{:x}{:x}{:x}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|n| n.as_nanos())
                        .unwrap_or(0),
                    std::process::id() as u128,
                    listener.local_addr().map(|a| a.port() as u128).unwrap_or(0),
                );
                std::fs::write(&jalur_token, &baru).map_err(|e| {
                    MxError::new(
                        ErrorCode::Config,
                        format!("tulis {}: {e}", jalur_token.display()),
                    )
                })?;
                tui_wire::secure_file::restrict_to_current_user(&jalur_token).map_err(|e| {
                    MxError::new(
                        ErrorCode::Config,
                        format!("izin {}: {e}", jalur_token.display()),
                    )
                })?;
                token = baru;
                eprintln!(
                    "token serve dibuat: {} (beri ke klien: header Authorization: Bearer)",
                    jalur_token.display()
                );
            }
        }
    }
    let client: Arc<dyn tui_provider::Provider> = Arc::new(
        OpenAiCompatClient::new(&config.gateway.base_url, Some(api_key))
            .map_err(|e| MxError::new(ErrorCode::GatewayUnreachable, e.message))?,
    );
    let tools: Arc<ToolRegistry> = Arc::new(ToolRegistry::v1());
    let approver: Arc<dyn tui_approval::Approver> = Arc::new(tui_approval::PolicyApprover::new(
        tui_approval::ApprovalPolicy::from(config.approval.clone()),
    ));
    eprintln!(
        "maicli serve mendengarkan http://{addr} (model {})",
        config.models.default
    );
    if token.is_empty() {
        eprintln!(
            "PERINGATAN: MAICLI_SERVE_TOKEN tidak diset — API tanpa autentikasi (loopback saja)"
        );
    }
    let riwayat = Arc::new(Riwayat::default());
    let ctx = Arc::new(ServeCtx {
        client,
        tools,
        approver,
        cwd,
        model: config.models.default.clone(),
        riwayat,
        sessions: Arc::new(SessionRegistry::new()),
        skills: skills_dimuat,
        // Subagent diaktifkan di serve juga (daftar lewat /api/subagents).
        subagents: Some(Arc::new(tui_subagent::manager::SubagentManager::new())),
        oauth: Arc::new(config.oauth_broker.clone()),
        oauth_pending: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        oauth_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        hooks: config.hooks.clone(),
    });
    loop {
        let (stream, _peer) = match listener.accept() {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let token = token.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = layani(stream, &token, &ctx);
        });
    }
}

/// State satu sesi live (dipakai ulang antar turn HTTP).
struct LiveSession {
    pesan: std::sync::Mutex<Vec<tui_wire::Message>>,
    usage: std::sync::Mutex<tui_wire::Usage>,
    aktif: std::sync::Mutex<Option<tui_tools::CancelToken>>,
}

impl Default for LiveSession {
    fn default() -> Self {
        Self {
            pesan: std::sync::Mutex::new(Vec::new()),
            usage: std::sync::Mutex::new(tui_wire::Usage::default()),
            aktif: std::sync::Mutex::new(None),
        }
    }
}

/// Daftar sesi live di server; maksimal 16 sesi (bebas memori & API calls).
struct SessionRegistry {
    sesi: std::sync::Mutex<std::collections::BTreeMap<String, Arc<LiveSession>>>,
    next: std::sync::atomic::AtomicUsize,
}

const MAX_SESI_LIVE: usize = 16;

impl SessionRegistry {
    fn new() -> Self {
        let mut sesi = std::collections::BTreeMap::new();
        sesi.insert("default".to_string(), Arc::new(LiveSession::default()));
        Self {
            sesi: std::sync::Mutex::new(sesi),
            next: std::sync::atomic::AtomicUsize::new(1),
        }
    }

    /// Ambil sesi yang diminta; belum ada → buat id baru (atau tolak bila penuh).
    fn ambil_atau_buat(&self, id: Option<&str>) -> Result<(String, Arc<LiveSession>), String> {
        if let Some(id) = id
            && let Some(sesi) = self.sesi.lock().ok().and_then(|g| g.get(id).cloned())
        {
            return Ok((id.to_string(), sesi));
        }
        let mut g = self
            .sesi
            .lock()
            .map_err(|_| "lock sesi gagal".to_string())?;
        if g.len() >= MAX_SESI_LIVE {
            return Err(format!(
                "batas {MAX_SESI_LIVE} sesi live tercapai; hapus sesi dulu"
            ));
        }
        let id = id
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 64
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .map(str::to_string)
            .unwrap_or_else(|| {
                format!(
                    "sesi-{}",
                    self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                )
            });
        if g.contains_key(&id) {
            return Ok((id.clone(), g[&id].clone()));
        }
        let sesi = Arc::new(LiveSession::default());
        g.insert(id.clone(), sesi.clone());
        Ok((id, sesi))
    }

    /// Ringkasan semua sesi live untuk GET /api/sessions.
    fn ringkasan(&self) -> Vec<serde_json::Value> {
        self.sesi
            .lock()
            .map(|g| {
                g.iter()
                    .map(|(id, sesi)| {
                        serde_json::json!({
                            "id": id,
                            "messages": sesi.pesan.lock().map(|p| p.len()).unwrap_or(0),
                            "input_tokens": sesi.usage.lock().map(|u| u.input_tokens).unwrap_or(0),
                            "output_tokens": sesi.usage.lock().map(|u| u.output_tokens).unwrap_or(0),
                            "busy": sesi.aktif.lock().map(|a| a.is_some()).unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Hapus satu sesi; sesi `default` tidak boleh dihapus.
    fn hapus(&self, id: &str) -> bool {
        if id == "default" {
            return false;
        }
        self.sesi
            .lock()
            .map(|mut g| g.remove(id).is_some())
            .unwrap_or(false)
    }
}

/// Konteks server yang dibagikan ke tiap request.
#[derive(Clone)]
struct ServeCtx {
    client: Arc<dyn tui_provider::Provider>,
    tools: Arc<ToolRegistry>,
    approver: Arc<dyn tui_approval::Approver>,
    cwd: PathBuf,
    model: String,
    riwayat: Arc<Riwayat>,
    /// Konten sesi panjang (transkrip + usage); sesi objek dibangun ulang per
    /// request dari konten ini (Session tidak dikirim antar-thread).
    /// Registry sesi live (multi-sesi, maks. 16).
    sessions: Arc<SessionRegistry>,
    /// Skill dimuat saat boot (dipakai system prompt).
    skills: Vec<tui_tools::skills::Skill>,
    /// Manager subagent bila fitur aktif (daftar anak lewat /api/subagents).
    subagents: Option<Arc<tui_subagent::manager::SubagentManager>>,
    /// Konfigurasi broker OAuth per provider.
    oauth: Arc<std::collections::BTreeMap<String, tui_config::OAuthBrokerEntry>>,
    /// State PKCE yang menunggu callback.
    oauth_pending: OAuthPending,
    /// Token hasil broker (hanya di memori, tidak ditulis ke disk).
    oauth_tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    /// Hook eksternal yang dipasang pada setiap sesi serve.
    hooks: tui_config::HooksConfig,
}

fn layani(mut stream: TcpStream, token: &str, ctx: &ServeCtx) -> ServeResult {
    let mut buffer = [0u8; 16384];
    let mut request = Vec::new();
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                request.extend_from_slice(&buffer[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if request.len() > 1024 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let teks = String::from_utf8_lossy(&request);
    let mut bagian = teks.split("\r\n\r\n");
    let head = bagian.next().unwrap_or_default();
    let mut baris = head.split_whitespace();
    let metode = baris.next().unwrap_or_default().to_string();
    let path = baris.next().unwrap_or_default().to_string();
    // Body untuk POST (panjang dari Content-Length; tanpa chunked).
    let mut body = bagian.next().unwrap_or_default().to_string();
    if let Some(cl) = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|l| l.trim().parse::<usize>().ok())
    {
        while body.len() < cl {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.push_str(&String::from_utf8_lossy(&buffer[..n])),
            }
        }
    }
    // Auth.
    let auth_ok = token.is_empty()
        || head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
            .map(|l| l.trim_end() == format!("Authorization: Bearer {token}"))
            .unwrap_or(false);
    if !auth_ok {
        return balas(stream, 401, r#"{"error":"token serve tidak valid"}"#);
    }
    let (path_tanpa_query, _) = path.split_once('?').unwrap_or((&path, ""));
    match (metode.as_str(), path_tanpa_query) {
        ("GET", "/api/oauth/start") => {
            let Some(provider) = query_param(&path, "provider") else {
                return balas(stream, 400, r#"{"error":"provider wajib"}"#);
            };
            let Some(entry) = ctx.oauth.get(&provider) else {
                return balas(stream, 404, r#"{"error":"provider OAuth tidak terdaftar"}"#);
            };
            if entry.authorize_url.is_empty()
                || entry.token_url.is_empty()
                || entry.client_id.is_empty()
                || entry.redirect_uri.is_empty()
            {
                return balas(
                    stream,
                    400,
                    r#"{"error":"konfigurasi OAuth provider tidak lengkap"}"#,
                );
            }
            let state = acak();
            let verifier = format!("{}{}", acak(), acak());
            let challenge = pkce_challenge(&verifier);
            let redirect =
                query_param(&path, "redirect_uri").unwrap_or_else(|| entry.redirect_uri.clone());
            if let Ok(mut pending) = ctx.oauth_pending.lock() {
                pending.insert(
                    state.clone(),
                    (provider.clone(), verifier, redirect.clone()),
                );
            }
            let scopes = entry.scopes.join(" ");
            let url = tambah_query(
                &entry.authorize_url,
                &[
                    ("response_type", "code"),
                    ("client_id", &entry.client_id),
                    ("redirect_uri", &redirect),
                    ("scope", &scopes),
                    ("state", &state),
                    ("code_challenge", &challenge),
                    ("code_challenge_method", "S256"),
                ],
            );
            balas(
                stream,
                200,
                &serde_json::json!({"authorization_url": url, "state": state}).to_string(),
            )
        }
        ("GET", "/api/oauth/callback") => {
            let Some(code) = query_param(&path, "code") else {
                return balas(stream, 400, r#"{"error":"code wajib"}"#);
            };
            let Some(state) = query_param(&path, "state") else {
                return balas(stream, 400, r#"{"error":"state wajib"}"#);
            };
            let pending = ctx
                .oauth_pending
                .lock()
                .ok()
                .and_then(|mut g| g.remove(&state));
            let Some((provider, verifier, redirect)) = pending else {
                return balas(stream, 400, r#"{"error":"state tidak dikenal"}"#);
            };
            let Some(entry) = ctx.oauth.get(&provider).cloned() else {
                return balas(stream, 404, r#"{"error":"provider tidak terdaftar"}"#);
            };
            let rahasia = std::env::var(&entry.client_secret_env).unwrap_or_default();
            let mut form = vec![
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", redirect.as_str()),
                ("client_id", entry.client_id.as_str()),
                ("code_verifier", verifier.as_str()),
            ];
            if !rahasia.trim().is_empty() {
                form.push(("client_secret", rahasia.as_str()));
            }
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let Ok(rt) = rt else {
                return balas(stream, 500, r#"{"error":"runtime gagal"}"#);
            };
            let tukar = rt.block_on(async {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(20))
                    .build()
                    .map_err(|e| e.to_string())?;
                let respon = client
                    .post(&entry.token_url)
                    .form(&form)
                    .send()
                    .await
                    .map_err(|e| e.to_string())?
                    .error_for_status()
                    .map_err(|e| e.to_string())?;
                let nilai: serde_json::Value = respon.json().await.map_err(|e| e.to_string())?;
                nilai
                    .get("access_token")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| "token tidak dikembalikan server".to_string())
            });
            match tukar {
                Ok(token) => {
                    if let Ok(mut semua) = ctx.oauth_tokens.lock() {
                        semua.insert(provider.clone(), token);
                    }
                    balas(
                        stream,
                        200,
                        &serde_json::json!({"provider": provider, "ok": true}).to_string(),
                    )
                }
                Err(err) => balas(stream, 502, &serde_json::json!({"error": err}).to_string()),
            }
        }
        ("GET", "/api/oauth/status") => {
            let provider = query_param(&path, "provider").unwrap_or_default();
            let configured = ctx.oauth.contains_key(&provider);
            let has_token = ctx
                .oauth_tokens
                .lock()
                .map(|g| g.contains_key(&provider))
                .unwrap_or(false);
            balas(
                stream,
                200,
                &serde_json::json!({"provider": provider, "configured": configured, "has_token": has_token}).to_string(),
            )
        }
        ("GET", "/api/health") => balas(
            stream,
            200,
            &format!(
                r#"{{"ok":true,"model":"{}","mode":"serve","live_sessions":{}}}"#,
                ctx.model,
                ctx.sessions.sesi.lock().map(|g| g.len()).unwrap_or(0)
            ),
        ),
        ("GET", "/api/sessions") => {
            let daftar: Vec<String> = tui_session::list(&sessions_dir(&ctx.cwd))
                .into_iter()
                .map(|s| {
                    format!(
                        r#"{{"id":"{}","messages":{},"title":{}}}"#,
                        s.id,
                        s.messages,
                        serde_json::to_string(&s.title).unwrap_or_default()
                    )
                })
                .collect();
            balas(
                stream,
                200,
                &format!(
                    "{{\"sessions\":[{}],\"live\":{}}}",
                    daftar.join(","),
                    serde_json::to_string(&ctx.sessions.ringkasan()).unwrap_or_default()
                ),
            )
        }
        ("GET", "/api/session") => {
            // Metadata sesi panjang (model, jumlah pesan, usage, sibuk).
            let id = body_session_id(&body);
            let Ok((id, state)) = ctx.sessions.ambil_atau_buat(id.as_deref()) else {
                return balas(stream, 409, r#"{"error":"batas sesi live tercapai"}"#);
            };
            let jumlah = state.pesan.lock().map(|g| g.len()).unwrap_or(0);
            let (input, output) = state
                .usage
                .lock()
                .map(|g| (g.input_tokens, g.output_tokens))
                .unwrap_or((0, 0));
            let sibuk = state.aktif.lock().map(|g| g.is_some()).unwrap_or(false);
            balas(
                stream,
                200,
                &serde_json::json!({
                    "session_id": id,
                    "model": ctx.model,
                    "messages": jumlah,
                    "input_tokens": input,
                    "output_tokens": output,
                    "busy": sibuk,
                })
                .to_string(),
            )
        }
        ("POST", "/api/abort") => {
            // Batalkan turn yang sedang berjalan (token sesi aktif).
            let id = body_session_id(&body);
            let token_batal = id
                .as_deref()
                .and_then(|id| {
                    ctx.sessions
                        .sesi
                        .lock()
                        .ok()
                        .and_then(|g| g.get(id).cloned())
                })
                .or_else(|| {
                    ctx.sessions
                        .sesi
                        .lock()
                        .ok()
                        .and_then(|g| g.get("default").cloned())
                })
                .and_then(|state| state.aktif.lock().ok().and_then(|g| g.clone()));
            match token_batal {
                Some(token) => {
                    token.cancel();
                    balas(stream, 200, r#"{"aborted":true}"#)
                }
                None => balas(stream, 409, r#"{"error":"tidak ada turn yang berjalan"}"#),
            }
        }
        ("POST", "/api/session/delete") => {
            let id = body_session_id(&body);
            match id.as_deref().filter(|id| ctx.sessions.hapus(id)) {
                Some(id) => balas(stream, 200, &serde_json::json!({"deleted": id}).to_string()),
                None => balas(stream, 404, r#"{"error":"sesi tidak ditemukan"}"#),
            }
        }
        ("GET", "/api/events") => balas(stream, 200, &ctx.riwayat.json()),
        ("GET", "/api/subagents") => {
            // Daftar anak subagent (bila fitur aktif di sesi serve).
            let agents: Vec<serde_json::Value> = ctx
                .subagents
                .as_ref()
                .map(|manager| {
                    manager
                        .list()
                        .iter()
                        .map(|h| {
                            let hasil = h.result();
                            serde_json::json!({
                                "id": h.id,
                                "status": h.status().label(),
                                "turns": hasil.as_ref().map(|r| r.turns).unwrap_or(0),
                                "tool_calls": hasil.as_ref().map(|r| r.tool_calls).unwrap_or(0),
                                "is_error": hasil.as_ref().is_some_and(|r| r.is_error),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            balas(
                stream,
                200,
                &serde_json::json!({ "agents": agents }).to_string(),
            )
        }
        ("GET", "/api/events/stream") => {
            // SSE push: mulai dari Last-Event-ID (0 = seluruh riwayat); koneksi
            // ditahan sampai klien putus (poll 300 ms).
            let mulai = head
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("last-event-id:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|l| l.trim().parse::<usize>().ok())
                .unwrap_or(0);
            ctx.riwayat.stream_sse(stream, mulai);
            // Stream menahan koneksi sampai klien putus; tidak ada balasan JSON.
            ServeResult {
                status: 200,
                body: String::new(),
            }
        }
        ("POST", "/api/prompt") => {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(&body);
            let Ok(parsed) = parsed else {
                return balas(stream, 400, r#"{"error":"body bukan JSON"}"#);
            };
            let Some(pesan) = parsed.get("message").and_then(|m| m.as_str()) else {
                return balas(stream, 400, r#"{"error":"field 'message' wajib"}"#);
            };
            if pesan.trim().is_empty() {
                return balas(stream, 400, r#"{"error":"message kosong"}"#);
            }
            // Sesi panjang = konten (transkrip+usage) di state; objek Session
            // dibangun ulang per request (tidak lintas-thread).
            let id_cekdir = body_session_id(&body);
            let sibuk = id_cekdir
                .as_deref()
                .and_then(|id| {
                    ctx.sessions
                        .sesi
                        .lock()
                        .ok()
                        .and_then(|g| g.get(id).cloned())
                })
                .and_then(|state| state.aktif.lock().ok().map(|g| g.is_some()))
                .unwrap_or(false);
            if sibuk {
                return balas(
                    stream,
                    409,
                    r#"{"error":"turn sesi ini sedang berjalan; POST /api/abort dulu"}"#,
                );
            }
            let id_diminta = body_session_id(&body);
            let (session_id, state) = match ctx.sessions.ambil_atau_buat(id_diminta.as_deref()) {
                Ok(pair) => pair,
                Err(err) => {
                    return balas(stream, 409, &serde_json::json!({"error": err}).to_string());
                }
            };
            let (transkrip, usage_awal) = {
                let (pesan_state, usage_state) = (state.pesan.lock(), state.usage.lock());
                match (pesan_state, usage_state) {
                    (Ok(p), Ok(u)) => (p.clone(), *u),
                    _ => (Vec::new(), tui_wire::Usage::default()),
                }
            };
            let builder = SessionBuilder::new(ctx.cwd.clone())
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
            let (mut sesi, mut stream_event) = match builder.build() {
                Ok((sesi, events)) => (sesi, events),
                Err(err) => {
                    return balas(
                        stream,
                        500,
                        &format!(
                            r#"{{"error":{}}}"#,
                            serde_json::to_string(&err.message).unwrap_or_default()
                        ),
                    );
                }
            };
            crate::hooks::pasang_sdk(&mut sesi, &ctx.hooks);
            sesi.push_messages(transkrip);
            sesi.seed_usage(usage_awal);
            let token_batal = sesi.cancel_token();
            if let Ok(mut g) = state.aktif.lock() {
                *g = Some(token_batal);
            }
            let riwayat = &ctx.riwayat;
            // Kita di thread OS; buat runtime kecil untuk turn.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let rt = match rt {
                Ok(rt) => rt,
                Err(err) => {
                    return balas(
                        stream,
                        500,
                        &format!(r#"{{"error":"runtime gagal: {err}"}}"#),
                    );
                }
            };
            let hasil = rt.block_on(async {
                let mulai = std::time::Instant::now();
                riwayat.push(format!(
                    r#"{{"event":"prompt","message":{}}}"#,
                    serde_json::to_string(pesan).unwrap_or_default()
                ));
                match sesi.prompt(pesan).await {
                    Ok(outcome) => {
                        // Kumpulkan event sesi (turn/tool) ke riwayat collab.
                        while let Some(event) = stream_event.try_next()
                            && let Some((nama, detail)) = ringkas_event(&event)
                        {
                            riwayat.push(format!(r#"{{"event":"{nama}","detail":{detail}}}"#));
                        }
                        riwayat.push(format!(
                            r#"{{"event":"jawab","detail":{{"status":"{:?}","turns":{}}}}}"#,
                            outcome.stop_reason, outcome.turns
                        ));
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
                        // Simpan konten sesi panjang + bersihkan penanda aktif.
                        if let Ok(mut p) = state.pesan.lock() {
                            *p = sesi.messages().to_vec();
                        }
                        if let Ok(mut u) = state.usage.lock() {
                            *u = sesi.usage();
                        }
                        if let Ok(mut g) = state.aktif.lock() {
                            *g = None;
                        }
                        serde_json::json!({
                            "session_id": session_id,
                            "status": format!("{:?}", outcome.stop_reason),
                            "turns": outcome.turns,
                            "text": teks,
                            "elapsed_ms": mulai.elapsed().as_millis() as u64,
                        })
                        .to_string()
                    }
                    Err(err) => {
                        // Error juga: simpan transkrip, bersihkan penanda aktif.
                        if let Ok(mut p) = state.pesan.lock() {
                            *p = sesi.messages().to_vec();
                        }
                        if let Ok(mut u) = state.usage.lock() {
                            *u = sesi.usage();
                        }
                        if let Ok(mut g) = state.aktif.lock() {
                            *g = None;
                        }
                        serde_json::json!({
                            "error": err.message,
                            "code": err.code.to_string(),
                        })
                        .to_string()
                    }
                }
            });
            let status = if hasil.contains("\"error\"") {
                500
            } else {
                200
            };
            balas(stream, status, &hasil)
        }
        _ => balas(stream, 404, r#"{"error":"path tidak dikenal"}"#),
    }
}

/// Ambil `session_id` dari body JSON (opsional; `None` = sesi default).
fn body_session_id(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("session_id")?.as_str().map(str::to_string))
}

fn sessions_dir(_cwd: &PathBuf) -> PathBuf {
    crate::config_root().join("sessions")
}

/// Ringkasan event sesi untuk riwayat collab (payload JSON).
fn ringkas_event(event: &tui_wire::AgentEvent) -> Option<(&'static str, String)> {
    match event {
        tui_wire::AgentEvent::AgentStart => Some(("turn.start", "{}".to_string())),
        tui_wire::AgentEvent::AgentEnd { turns } => {
            Some(("turn.end", format!(r#"{{"turns":{turns}}}"#)))
        }
        tui_wire::AgentEvent::ToolExecutionStart {
            tool_name, preview, ..
        } => Some((
            "tool.start",
            format!(
                r#"{{"tool":{},"preview":{}}}"#,
                serde_json::to_string(tool_name).unwrap_or_default(),
                serde_json::to_string(preview).unwrap_or_default()
            ),
        )),
        tui_wire::AgentEvent::ToolExecutionEnd {
            tool_name,
            is_error,
            ..
        } => Some((
            "tool.end",
            format!(
                r#"{{"tool":{},"is_error":{is_error}}}"#,
                serde_json::to_string(tool_name).unwrap_or_default()
            ),
        )),
        tui_wire::AgentEvent::Notice { text } => {
            Some(("notice", serde_json::to_string(text).unwrap_or_default()))
        }
        _ => None,
    }
}

fn balas(mut stream: TcpStream, status: u16, body: &str) -> ServeResult {
    let alasan = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {alasan}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
    ServeResult {
        status,
        body: body.to_string(),
    }
}

#[cfg(test)]
mod registry_tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn registry_buat_dan_hapus_sesi() {
        let reg = SessionRegistry::new();
        assert_eq!(reg.ringkasan().len(), 1); // sesi default selalu ada
        let (id, state) = reg.ambil_atau_buat(Some("alpha")).unwrap();
        assert_eq!(id, "alpha");
        state.pesan.lock().unwrap().push(tui_wire::Message::System {
            content: "halo".to_string(),
        });
        assert!(
            reg.ringkasan()
                .iter()
                .any(|s| s["id"] == "alpha" && s["messages"] == 1)
        );
        assert!(reg.hapus("alpha"));
        assert!(!reg.hapus("default"));
        assert!(!reg.hapus("alpha"));
    }

    #[test]
    fn registry_batasi_jumlah_sesi() {
        let reg = SessionRegistry::new();
        for i in 0..15 {
            reg.ambil_atau_buat(Some(&format!("s-{i}"))).unwrap();
        }
        assert!(reg.ambil_atau_buat(None).is_err());
    }

    #[test]
    fn body_session_id_opsional() {
        assert_eq!(body_session_id(r#"{"message":"hai"}"#), None);
        assert_eq!(
            body_session_id(r#"{"session_id":"beta","message":"hai"}"#),
            Some("beta".to_string())
        );
    }
}

#[cfg(test)]
mod oauth_tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn query_param_dan_tambah_query() {
        let path = "/api/oauth/callback?code=a%20b&state=xyz";
        assert_eq!(query_param(path, "code"), Some("a b".to_string()));
        assert_eq!(query_param(path, "state"), Some("xyz".to_string()));
        let url = tambah_query("https://x.test/auth", &[("scope", "a b")]);
        assert_eq!(url, "https://x.test/auth?scope=a%20b");
    }

    #[test]
    fn pkce_challenge_stabil() {
        assert_eq!(pkce_challenge("abc"), pkce_challenge("abc"));
        assert_ne!(pkce_challenge("abc"), pkce_challenge("abd"));
    }
}
