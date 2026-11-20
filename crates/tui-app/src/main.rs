//! Titik masuk binary `mx`.

mod acp_mode;
mod approvals;
mod banner;
mod cli;
mod event_log;
mod logging;
mod mcp_boot;
mod models_cache;
mod parity_report;
mod relay_mode;
mod rpc_mode;
mod serve_mode;
mod session_open;
mod text_mode;
mod tui_mode;
mod usage;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use tui_approval::ApprovalPolicy;
use tui_config::{LimitsConfig, LoadedConfig, UiConfig};
use tui_provider::OpenAiCompatClient;

/// Data yang dibutuhkan satu sesi setelah config dibaca.
pub struct Boot {
    /// Klien gateway.
    pub client: OpenAiCompatClient,
    /// Model default sesi.
    pub model: String,
    /// Direktori kerja sesi.
    pub cwd: PathBuf,
    /// Direktori penyimpanan sesi.
    pub sessions_dir: PathBuf,
    /// Batas waktu dan ukuran.
    pub limits: LimitsConfig,
    /// Kebijakan approval untuk mode tanpa UI (RPC).
    pub approval: ApprovalPolicy,
    /// Path sesi yang dilanjutkan, bila ada.
    pub resume_path: Option<PathBuf>,
    /// Pesan dari sesi yang dilanjutkan.
    pub resume_messages: Vec<tui_wire::Message>,
    /// Pemakaian token yang sudah tercatat di sesi yang dilanjutkan.
    pub resume_usage: tui_wire::Usage,
    /// Path file grant approval persisten.
    pub grants_path: PathBuf,
    /// Masa berlaku grant `always` (None = tidak kedaluwarsa).
    pub grant_ttl: Option<std::time::Duration>,
    /// Warna tampilan TUI.
    pub ui: UiConfig,
    /// Id model yang diketahui dari discovery gateway (kosong bila belum pernah).
    pub models: Vec<String>,
    /// Provider aktif (id registry) dan env var kuncinya.
    pub provider: (String, String),
    /// Berkas rekam jejak event agen (JSONL) bila `--event-log` dipakai.
    pub event_log: Option<std::path::PathBuf>,
    /// Tool dari server MCP yang berhasil terhubung (didahului boot MCP).
    pub mcp_tools: Vec<Arc<dyn tui_tools::Tool>>,
    /// Registry subagent untuk mode TUI (panel F3); `None` = fitur mati.
    pub subagents: Option<Arc<tui_subagent::manager::SubagentManager>>,
    /// Penerima notifikasi server MCP (diteruskan ke timeline oleh mode).
    pub notif_rx: Option<tokio::sync::mpsc::UnboundedReceiver<(String, tui_mcp::McpNotification)>>,
    /// Skill ditemukan saat boot (folder skills/ + MX_SKILLS_DIR).
    pub skills: Vec<tui_tools::skills::Skill>,
    /// Katalog omp dimuat (flag `--catalog` / config `[catalog] path`).
    pub katalog: Option<tui_katalog::Katalog>,
    /// Catatan status MCP untuk ditampilkan ke pengguna.
    pub mcp_notes: Vec<String>,
}

impl Boot {
    /// Registry tool satu sesi: 8 tool v1 + skill + tool MCP yang terhubung.
    pub fn tool_registry(&self) -> tui_tools::ToolRegistry {
        let mut registry = tui_tools::ToolRegistry::v1();
        if !self.skills.is_empty() {
            registry.register(Arc::new(tui_tools::skills::SkillTool::baru(
                self.skills.clone(),
            )));
        }
        for tool in &self.mcp_tools {
            registry.register(tool.clone());
        }
        registry
    }
}

/// Sesi yang dilanjutkan: path, pesan, dan model terakhir yang tercatat.
struct ResumedSession {
    path: PathBuf,
    messages: Vec<tui_wire::Message>,
    model: Option<String>,
    usage: tui_wire::Usage,
}

/// Cari sesi yang diminta lalu muat pesannya.
async fn resolve_resume(
    resume: Option<String>,
    sessions_dir: std::path::PathBuf,
) -> Result<Option<ResumedSession>, tui_wire::MxError> {
    let Some(requested) = resume else {
        return Ok(None);
    };
    let (path, loaded) = tokio::task::spawn_blocking(move || {
        let summary = tui_session::resolve_target(&sessions_dir, &requested)?;
        let loaded = tui_session::SessionStore::load(&summary.path)?;
        Ok::<_, tui_wire::MxError>((summary.path, loaded))
    })
    .await
    .map_err(|_| {
        tui_wire::MxError::new(
            tui_wire::ErrorCode::SessionCorrupt,
            "gagal membaca sesi (task blocking batal)".to_string(),
        )
    })??;
    let (input_tokens, output_tokens) = loaded.usage();
    Ok(Some(ResumedSession {
        path,
        messages: loaded.messages(),
        model: loaded.model().map(str::to_string),
        usage: tui_wire::Usage {
            input_tokens,
            output_tokens,
            cached_tokens: 0,
        },
    }))
}

/// Buat salinan sesi lalu siapkan sesi itu untuk dilanjutkan.
async fn fork_session(
    requested: String,
    sessions_dir: std::path::PathBuf,
) -> Result<ResumedSession, tui_wire::MxError> {
    let forked = tokio::task::spawn_blocking(move || {
        let summary = tui_session::resolve_target(&sessions_dir, &requested)?;
        let title = format!("{} (fork)", summary.title);
        let (store, _id) = tui_session::SessionStore::fork(&summary.path, &sessions_dir, &title)?;
        let path = store.path().to_path_buf();
        let loaded = tui_session::SessionStore::load(&path)?;
        Ok::<_, tui_wire::MxError>((path, summary.id, loaded))
    })
    .await
    .map_err(|_| {
        tui_wire::MxError::new(
            tui_wire::ErrorCode::SessionCorrupt,
            "gagal membuat fork sesi (task blocking batal)".to_string(),
        )
    })??;
    let (path, parent, loaded) = forked;
    let (input_tokens, output_tokens) = loaded.usage();
    println!("fork sesi: {} (dari {parent})", path.display());
    Ok(ResumedSession {
        path,
        messages: loaded.messages(),
        model: loaded.model().map(str::to_string),
        usage: tui_wire::Usage {
            input_tokens,
            output_tokens,
            cached_tokens: 0,
        },
    })
}

/// Direktori data aplikasi (config, sesi, log).
///
/// `MX_DATA_DIR` menimpa lokasi default agar instalasi portabel dan test E2E
/// bisa memakai direktori terisolasi tanpa menyentuh profil pengguna. Variabel
/// ini tidak pernah mengubah isi peka; ia hanya memindahkan lokasi berkas.
pub(crate) fn config_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("MX_DATA_DIR") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.config_dir().join("ModeluXAI").join("TUI"))
        .unwrap_or_else(|| PathBuf::from(".mx"))
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = cli::parse();
    let cwd = args
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let root = config_root();
    let global = tui_config::default_global_path();
    let project = tui_config::default_project_path(&cwd);
    let load_global = global.clone();
    let load_project = project.clone();
    let loaded: LoadedConfig = match tokio::task::spawn_blocking(move || {
        tui_config::load(load_global.as_deref(), Some(&load_project))
    })
    .await
    {
        Ok(Ok(loaded)) => loaded,
        Ok(Err(err)) => {
            eprintln!("{}: {}", err.code, err.message);
            eprintln!("{}", err.code.user_message());
            return ExitCode::from(2);
        }
        Err(_) => {
            eprintln!("gagal memuat config: task blocking dibatalkan");
            return ExitCode::from(2);
        }
    };
    let mut loaded = loaded;
    for item in args.set.clone() {
        let Some((key, value)) = item.split_once('=') else {
            eprintln!("--set harus berbentuk bagian.kunci=nilai: {item}");
            return ExitCode::from(2);
        };
        if let Err(err) = tui_config::apply_override(&mut loaded, key.trim(), value) {
            eprintln!("{}: {}", err.code, err.message);
            return ExitCode::from(2);
        }
    }
    let _guard = logging::init(&root.join("logs"));
    let sessions_dir = root.join("sessions");

    if let (Some(expected), Some(actual)) =
        (args.parity_expected.clone(), args.parity_actual.clone())
    {
        match parity_report::compare_files("transkrip", &expected, &actual) {
            Ok(diff) => {
                println!("{}", diff.report());
                if !diff.is_clean() {
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                return ExitCode::from(1);
            }
        }
    }
    if let Some(requested) = args.drop_session.clone() {
        let summary = match tui_session::resolve_target(&sessions_dir, &requested) {
            Ok(summary) => summary,
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                return ExitCode::from(1);
            }
        };
        match tui_session::remove(&summary.path) {
            Ok(()) => println!("sesi dihapus: {} ({})", summary.id, summary.path.display()),
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                return ExitCode::from(1);
            }
        }
        return ExitCode::SUCCESS;
    }
    if let Some(query) = args.list_sessions.clone() {
        let sessions = if query.trim().is_empty() {
            tui_session::list(&sessions_dir)
        } else {
            tui_session::search(&sessions_dir, &query)
        };
        if sessions.is_empty() {
            println!("belum ada sesi di {}", sessions_dir.display());
        } else {
            for session in sessions {
                println!("{}", session.display_line());
            }
        }
        return ExitCode::SUCCESS;
    }

    if let Some(cli::Command::Config { action, key, value }) = args.command {
        match action {
            cli::ConfigAction::Show => {
                for (key, value, source) in loaded.describe() {
                    println!("{key} = {value}  [{source}]");
                }
            }
            cli::ConfigAction::Set => {
                let (Some(key), Some(value)) = (key, value) else {
                    eprintln!("pakai: mx config set <key> <value>");
                    return ExitCode::from(2);
                };
                let Some(global) = global.as_deref() else {
                    eprintln!("tidak ada direktori config pengguna");
                    return ExitCode::from(2);
                };
                if let Err(err) = tui_config::set_global_value(global, &key, &value) {
                    eprintln!("{}: {}", err.code, err.message);
                    return ExitCode::from(2);
                }
                println!("diset: {key} = {value}  [{}]", global.display());
            }
            cli::ConfigAction::KeyImport => {
                let name = loaded.config.gateway.api_key_env.clone();
                let Some(value) = std::env::var(&name).ok().filter(|v| !v.trim().is_empty()) else {
                    eprintln!(
                        "env var {name} belum diisi; set dulu lalu jalankan ulang perintah ini"
                    );
                    return ExitCode::from(1);
                };
                let mut backend = tui_config::secret::KeyringBackend;
                match tui_config::secret::store_key(&mut backend, &name, &value) {
                    Ok(()) => {
                        println!(
                            "kunci API disimpan di keyring untuk '{name}' (nilai tidak ditampilkan)"
                        )
                    }
                    Err(err) => {
                        eprintln!("{}: {}", err.code, err.message);
                        return ExitCode::from(1);
                    }
                }
            }
            cli::ConfigAction::Path => {
                println!(
                    "global: {}",
                    global
                        .as_deref()
                        .unwrap_or(std::path::Path::new("-"))
                        .display()
                );
                println!("proyek: {}", project.display());
            }
        }
        return ExitCode::SUCCESS;
    }

    let Some(api_key) = loaded.api_key() else {
        let code = tui_wire::ErrorCode::KeyMissing;
        eprintln!("{code}: {}", code.user_message());
        eprintln!(
            "Contoh: setx {} \"<kunci>\"",
            loaded.config.gateway.api_key_env
        );
        return ExitCode::from(1);
    };

    let client =
        match OpenAiCompatClient::new(&loaded.config.gateway.base_url, Some(api_key.clone())) {
            Ok(client) => client,
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                return ExitCode::from(1);
            }
        };
    if args.resume.is_some() && args.fork.is_some() {
        eprintln!("pakai salah satu: --resume atau --fork");
        return ExitCode::from(2);
    }
    // Daftar model untuk `/model` (best-effort): pakai cache file bila masih
    // segar, selain itu ambil dari gateway lalu simpan cache (C-02).
    let cache_path = root.join("models-cache.json");
    let known_models = match models_cache::load(&cache_path) {
        Some(cached) => cached,
        None => {
            match tokio::time::timeout(std::time::Duration::from_secs(3), client.list_models())
                .await
            {
                Ok(Ok(models)) => {
                    let ids: Vec<String> = models.into_iter().map(|model| model.id).collect();
                    if let Err(err) = models_cache::save(&cache_path, &ids) {
                        tracing::warn!(
                            target: "mx::models",
                            "cache model tidak bisa ditulis: {err}"
                        );
                    }
                    ids
                }
                _ => Vec::new(),
            }
        }
    };
    let resume = if let Some(requested) = args.fork.clone() {
        match fork_session(requested, sessions_dir.clone()).await {
            Ok(session) => Some(session),
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                eprintln!("{}", err.code.user_message());
                return ExitCode::from(1);
            }
        }
    } else {
        match resolve_resume(args.resume.clone(), sessions_dir.clone()).await {
            Ok(resume) => resume,
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                eprintln!("{}", err.code.user_message());
                return ExitCode::from(1);
            }
        }
    };
    let (resume_path, resume_messages, resume_model, resume_usage) = match resume {
        Some(session) => (
            Some(session.path),
            session.messages,
            session.model,
            session.usage,
        ),
        None => (None, Vec::new(), None, tui_wire::Usage::default()),
    };
    let mcp_boot = mcp_boot::connect_all(&loaded.config.mcp).await;
    // Extensions: folder <config_root>/extensions (manifest ketat + env
    // dibersihkan + approval Exec); flag --no-extensions / config menonaktifkan.
    let mut extension_tools: Vec<Arc<dyn tui_tools::Tool>> = Vec::new();
    let mut extension_notes: Vec<String> = Vec::new();
    if loaded.config.extensions.enabled && !args.no_extensions {
        extension_tools = tui_tools::extensions::muat_extensions(
            &config_root().join("extensions"),
            &mut extension_notes,
        );
        for note in &extension_notes {
            eprintln!("[extensions] {note}");
        }
    }
    // Skills: folder proyek + MX_SKILLS_DIR (nama di-dedup); dimuat sebelum
    // cwd dipindah ke Boot.
    let mut skills_dimuat = tui_tools::skills::temukan(&cwd.join("skills"));
    if let Ok(custom) = std::env::var("MX_SKILLS_DIR")
        && !custom.is_empty()
    {
        let path = std::path::PathBuf::from(custom);
        for skill in tui_tools::skills::temukan(&path) {
            if !skills_dimuat.iter().any(|s| s.nama == skill.nama) {
                skills_dimuat.push(skill);
            }
        }
    }
    if args.mode == cli::Mode::Acp {
        let hasil = acp_mode::run(loaded.config.clone(), cwd, api_key).await;
        return match hasil {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                ExitCode::FAILURE
            }
        };
    }
    if args.mode == cli::Mode::Relay {
        let hasil = relay_mode::run(
            loaded.config.clone(),
            args.port,
            cwd,
            api_key,
            skills_dimuat,
        )
        .await;
        return match hasil {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                ExitCode::FAILURE
            }
        };
    }
    if args.mode == cli::Mode::Serve {
        // Mode serve tak perlu Boot/sesi; kunci dikirim ke server.
        let hasil = serve_mode::run(
            loaded.config.clone(),
            args.port,
            cwd,
            api_key,
            skills_dimuat,
        )
        .await;
        return match hasil {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{}: {}", err.code, err.message);
                ExitCode::from(1)
            }
        };
    }
    let boot = Boot {
        client,
        // Precedensi model: --model eksplisit > model terakhir di sesi > config.
        model: args
            .model
            .clone()
            .or(resume_model)
            .unwrap_or_else(|| loaded.config.models.default.clone()),
        cwd,
        sessions_dir,
        limits: loaded.config.limits.clone(),
        approval: ApprovalPolicy::from(loaded.config.approval.clone()),
        resume_path,
        resume_messages,
        resume_usage,
        grants_path: root.join("grants.json"),
        grant_ttl: (loaded.config.approval.grant_ttl_hours > 0)
            .then(|| std::time::Duration::from_secs(loaded.config.approval.grant_ttl_hours * 3600)),
        ui: loaded.config.ui.clone(),
        models: known_models,
        provider: (
            loaded.config.gateway.provider.clone(),
            loaded.config.gateway.api_key_env.clone(),
        ),
        event_log: args.event_log.clone(),
        mcp_tools: {
            let mut semua = mcp_boot.tools;
            semua.extend(extension_tools);
            semua
        },
        mcp_notes: {
            let mut notes = mcp_boot.notes;
            notes.extend(extension_notes);
            notes
        },
        subagents: (args.mode == cli::Mode::Tui)
            .then(|| Arc::new(tui_subagent::manager::SubagentManager::new())),
        notif_rx: mcp_boot.notif_rx,
        skills: skills_dimuat,
        // Katalog omp: flag --catalog menang atas config catalog.path.
        katalog: {
            let dir_katalog = args
                .catalog
                .clone()
                .filter(|p| !p.as_os_str().is_empty())
                .or_else(|| {
                    let dari_config = loaded.config.catalog.path.clone();
                    (!dari_config.is_empty()).then(|| PathBuf::from(dari_config))
                });
            match dir_katalog {
                Some(dir) if dir.join("providers").is_dir() => {
                    match tokio::task::spawn_blocking(move || tui_katalog::susun(&dir)).await {
                        Ok(Ok(katalog)) => {
                            eprintln!(
                                "[katalog] dimuat: {} provider, {} class, {} aturan cascade",
                                katalog.jumlah_provider(),
                                katalog.jumlah_class(),
                                katalog.kaskade.len()
                            );
                            Some(katalog)
                        }
                        Ok(Err(err)) => {
                            eprintln!("[katalog] gagal dimuat: {err}");
                            None
                        }
                        Err(err) => {
                            eprintln!("[katalog] task muat gagal: {err}");
                            None
                        }
                    }
                }
                Some(dir) => {
                    eprintln!(
                        "[katalog] {} bukan direktori rules (tidak ada providers/) — dilewati",
                        dir.display()
                    );
                    None
                }
                None => None,
            }
        },
    };

    let result = if let Some(prompt) = args.prompt.clone() {
        text_mode::run_once(boot, prompt).await
    } else {
        match args.mode {
            cli::Mode::Text => text_mode::run(boot).await,
            cli::Mode::Tui => tui_mode::run(boot).await,
            cli::Mode::Rpc => rpc_mode::run(boot).await,
            cli::Mode::Acp | cli::Mode::Relay | cli::Mode::Serve => {
                unreachable!("ditangani sebelum boot")
            }
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {}", err.code, err.message);
            eprintln!("{}", err.code.user_message());
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mx-main-{name}-{}", tui_session::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn fork_membuat_salinan_dengan_pesan_dan_model() {
        let dir = temp_dir("fork");
        let mut store = tui_session::SessionStore::create(&dir, "asli", ".").unwrap();
        store
            .append(&tui_session::SessionEntry::message(
                None,
                tui_wire::Message::user("halo"),
            ))
            .unwrap();
        store
            .append(&tui_session::SessionEntry::model_change("model-x"))
            .unwrap();
        drop(store);
        let forked = fork_session("latest".to_string(), dir.clone())
            .await
            .expect("fork berhasil");
        assert_ne!(forked.path, dir.join("tidak-ada"), "path fork baru");
        assert_eq!(forked.messages.len(), 1, "pesan ikut tersalin");
        assert_eq!(forked.model.as_deref(), Some("model-x"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fork_sesi_tidak_ada_dilaporkan() {
        let dir = temp_dir("fork-kosong");
        let err = match fork_session("latest".to_string(), dir.clone()).await {
            Err(err) => err,
            Ok(_) => panic!("harus gagal tanpa sesi"),
        };
        assert_eq!(err.code, tui_wire::ErrorCode::SessionNotFound);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
