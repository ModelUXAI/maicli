//! Wizard **initial setup** (first-run): bila kunci API belum ada, TUI/teks
//! tidak mati dengan `E_KEY_MISSING` — menampilkan wizard interaktif:
//! pilih provider → masukkan kunci (tersembunyi) → simpan ke **keyring** →
//! discovery model → pilih → tulis config global → selesai (jalankan ulang).

use std::io::Write as _;
use std::path::PathBuf;

use tui_wire::MxError;

/// Baca satu baris dengan **echo disembunyikan** (crossterm raw mode);
/// fallback: baris biasa bila terminal tidak bisa raw (mis. pipe).
fn baca_baris_tersembunyi() -> String {
    use crossterm::terminal;
    if terminal::enable_raw_mode().is_err() {
        // Pipe / non-TTY: baca biasa (tidak ada yang bisa disembunyikan).
        let mut baris = String::new();
        let _ = std::io::stdin().read_line(&mut baris);
        return baris.trim().to_string();
    }
    let mut nilai = String::new();
    let mut stdout = std::io::stdout();
    loop {
        use crossterm::event::{self, Event, KeyCode, KeyModifiers};
        match event::read() {
            Ok(Event::Key(k)) if k.kind == crossterm::event::KeyEventKind::Press => match k.code {
                KeyCode::Enter => break,
                KeyCode::Char(c) if k.modifiers == KeyModifiers::CONTROL && c == 'c' => {
                    nilai.clear();
                    break;
                }
                KeyCode::Backspace => {
                    nilai.pop();
                }
                KeyCode::Char(c) => {
                    nilai.push(c);
                    let _ = write!(stdout, "*");
                    let _ = stdout.flush();
                }
                _ => {}
            },
            _ => break,
        }
    }
    let _ = terminal::disable_raw_mode();
    println!();
    nilai
}

/// Daftar kandidat provider untuk wizard: registry bawaan + penanda env terisi.
fn daftar_provider() -> Vec<(String, String, String)> {
    // (id, label, env pertama)
    let mut daftar: Vec<(String, String, String)> = Vec::new();
    for id in [
        "modeluxai",
        "openai",
        "groq",
        "deepseek",
        "openrouter",
        "ollama",
        "lm-studio",
    ] {
        if let Some(entry) = tui_wire::providers::by_id(id) {
            let env = entry
                .env_keys
                .first()
                .cloned()
                .unwrap_or_else(|| "<env>".to_string());
            let siap = entry
                .resolve_env_key(|nama| std::env::var(nama).ok().filter(|v| !v.trim().is_empty()))
                .is_some();
            daftar.push((
                entry.id.clone(),
                format!(
                    "{} — base: {} [{}]",
                    entry.label,
                    if entry.base_url.is_empty() {
                        "?"
                    } else {
                        entry.base_url.as_str()
                    },
                    if siap {
                        "✓ env terisi"
                    } else {
                        "perlu kunci"
                    }
                ),
                env,
            ));
        }
    }
    daftar
}

/// Jalankan wizard; `Ok(())` = config terisi (minta restart), `Err` = dibatalkan.
pub async fn jalankan() -> Result<(), MxError> {
    println!("╭──────────────────────────────────────────────╮");
    println!("│  Setup awal ModeluXAI (mx) — pertama kali    │");
    println!("╰──────────────────────────────────────────────╯");
    println!();
    // 1) Pilih provider.
    let daftar = daftar_provider();
    println!("Pilih provider:");
    for (i, (_id, label, _)) in daftar.iter().enumerate() {
        println!("  {}. {}", i + 1, label);
    }
    print!("nomor: ");
    let _ = std::io::stdout().flush();
    let mut baris = String::new();
    let _ = std::io::stdin().read_line(&mut baris);
    let nomor: usize = baris
        .trim()
        .parse()
        .map_err(|_| MxError::new(tui_wire::ErrorCode::Config, "nomor tidak valid".to_string()))?;
    let (id_provider, _, env_nama) = daftar
        .get(nomor.checked_sub(1).ok_or_else(|| {
            MxError::new(
                tui_wire::ErrorCode::Config,
                "nomor di luar daftar".to_string(),
            )
        })?)
        .ok_or_else(|| {
            MxError::new(
                tui_wire::ErrorCode::Config,
                "nomor di luar daftar".to_string(),
            )
        })?
        .clone();
    // Base URL dari registry (untuk provider yang perlu ditulis config).
    let base_url = tui_wire::providers::by_id(&id_provider)
        .map(|e| e.base_url.clone())
        .unwrap_or_default();
    // 2) Kunci (tersembunyi).
    println!(
        "Masukkan API key untuk {id_provider} (disimpan di **keyring** OS, tidak ditulis ke file; Enter bila env {env_nama} sudah diisi):"
    );
    let kunci = baca_baris_tersembunyi();
    if !kunci.trim().is_empty() {
        let mut backend = tui_config::secret::KeyringBackend;
        match tui_config::secret::store_key(&mut backend, &env_nama, kunci.trim()) {
            Ok(()) => println!("✓ kunci disimpan di keyring ({env_nama})"),
            Err(err) => {
                println!(
                    "✗ keyring tidak tersedia ({}) — set env {env_nama} secara manual lalu jalankan ulang",
                    err.message
                );
                return Ok(());
            }
        }
    } else {
        println!("(lewati — memakai env {env_nama} bila terisi)");
    }
    // 3) Tulis config global (provider + env key), kecuali bawaan yang sudah benar.
    let akar = config_root_global().join("config.toml");
    let set = |kunci_toml: &str, nilai: &str| -> Result<(), MxError> {
        tui_config::set_global_value(&akar, kunci_toml, nilai)
    };
    set("gateway.provider", &id_provider)?;
    if !base_url.is_empty() && id_provider != "modeluxai" {
        set("gateway.base_url", &base_url)?;
    }
    set("gateway.api_key_env", &env_nama)?;
    // 4) Discovery model → pilih.
    println!("Mengambil daftar model dari gateway…");
    let kunci_baru = tui_config::secret::resolve_api_key(&env_nama);
    match tui_provider::OpenAiCompatClient::new(&base_url, kunci_baru) {
        Ok(client) => match client.list_models().await {
            Ok(models) if !models.is_empty() => {
                println!("Pilih model default:");
                for (i, m) in models.iter().take(20).enumerate() {
                    println!("  {}. {}", i + 1, m.id);
                }
                print!("nomor (Enter = {}: ", models[0].id);
                let _ = std::io::stdout().flush();
                let mut pilih = String::new();
                let _ = std::io::stdin().read_line(&mut pilih);
                let model = pilih
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| models.get(n.saturating_sub(1)))
                    .map(|m| m.id.clone())
                    .unwrap_or_else(|| models[0].id.clone());
                match set("models.default", &model) {
                    Ok(()) => println!("✓ model default: {model}"),
                    Err(err) => println!("✗ gagal simpan model: {}", err.message),
                }
            }
            _ => println!("(discovery kosong/gagal — set models.default via /model nanti)"),
        },
        Err(_) => println!("✗ gateway tidak bisa dibuat — set gateway.base_url secara manual"),
    }
    println!();
    println!("Setup selesai ✓ — jalankan ulang: mx");
    Ok(())
}

/// Direktori data global (sama dengan `crate::config_root`).
fn config_root_global() -> PathBuf {
    crate::config_root()
}
