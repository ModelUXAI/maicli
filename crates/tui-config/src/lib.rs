//! Lapisan config: default → global → proyek → override runtime, dengan
//! provenance per kunci supaya `config show` bisa menjelaskan asal nilai.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tui_approval::{ApprovalMode, ApprovalPolicy, ToolPolicy};
use tui_wire::{ErrorCode, MxError};

/// Konfigurasi efektif.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Ujung AI.
    pub gateway: GatewayConfig,
    /// Model default.
    pub models: ModelsConfig,
    /// Kebijakan approval.
    pub approval: ApprovalConfig,
    /// Batas waktu dan ukuran.
    pub limits: LimitsConfig,
    /// Tampilan TUI.
    pub ui: UiConfig,
    /// Server MCP.
    pub mcp: McpConfig,
    /// Katalog omp (pemilihan provider/model).
    pub catalog: CatalogConfig,
    /// Extension eksternal (loader folder).
    pub extensions: ExtensionsConfig,
    /// Provider tambahan yang ditambahkan pengguna (via `/provider add`).
    pub providers_eksternal: HashMap<String, ProviderEksternal>,
}

/// Satu provider tambahan milik pengguna (bukan registry bawaan).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEksternal {
    /// Ujung OpenAI-compatible (`http(s)://…/v1`).
    pub base_url: String,
    /// Nama env var kunci API (kredensial tetap tidak pernah ditulis).
    pub env_key: String,
    /// Model bawaan (opsional; kosong = pakai discovery).
    pub model: String,
}

/// Bagian `[extensions]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExtensionsConfig {
    /// Muat extension dari `<config_root>/extensions/` saat boot.
    pub enabled: bool,
}

impl Default for ExtensionsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Bagian `[catalog]`: direktori rules omp untuk integrasi katalog runtime.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CatalogConfig {
    /// Path direktori `rules/` omp (berisi providers/, auth/, dst.).
    /// Kosong = katalog tidak dimuat (flag `--catalog` menang).
    pub path: String,
}

/// Bagian `[ui]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// Warna aksen (`#rrggbb`) untuk status dan bingkai aktif.
    pub accent: String,
    /// Warna redup (`#rrggbb`) untuk hint dan teks sekunder.
    pub dim: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            accent: "#d7a05b".to_string(),
            dim: "#8a8f98".to_string(),
        }
    }
}

/// Bagian `[mcp]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    /// Timeout default satu request MCP (detik).
    pub timeout_secs: u64,
    /// Server MCP; kunci map = nama logis server.
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            servers: BTreeMap::new(),
        }
    }
}

impl McpConfig {
    /// Validasi ringan: nama tidak kosong, command ada, timeout > 0.
    pub fn validate(&self) -> Result<(), String> {
        if self.timeout_secs == 0 {
            return Err("mcp.timeout_secs harus > 0".to_string());
        }
        for (nama, server) in &self.servers {
            if nama.trim().is_empty() {
                return Err("nama server MCP tidak boleh kosong".to_string());
            }
            if server.command.trim().is_empty() && server.url.is_none() {
                return Err(format!(
                    "mcp.servers.{nama}: command wajib diisi, atau url untuk server HTTP/SSE"
                ));
            }
            if server.command.trim().is_empty() && server.url.is_some() {
                match server.transport.as_deref().unwrap_or("http") {
                    "http" | "sse" => {}
                    lain => {
                        return Err(format!(
                            "mcp.servers.{nama}.transport '{lain}' tidak dikenal (http|sse)"
                        ));
                    }
                }
            }
            if server.command.trim().is_empty()
                && server.url.as_deref().is_none_or(|u| u.trim().is_empty())
            {
                return Err(format!("mcp.servers.{nama}.url tidak boleh kosong"));
            }
            if server.timeout_secs == Some(0) {
                return Err(format!("mcp.servers.{nama}.timeout_secs harus > 0"));
            }
        }
        Ok(())
    }
}

/// Satu server MCP di `[mcp.servers.<nama>]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpServerConfig {
    /// Perintah yang dijalankan (stdio). Kosong bila `url` diisi.
    pub command: String,
    /// URL server (HTTP/SSE); mengisi ini mengganti jalur stdio.
    #[serde(default)]
    pub url: Option<String>,
    /// Transport untuk `url`: `http` (default) atau `sse` (legacy 2024-11-05).
    #[serde(default)]
    pub transport: Option<String>,
    /// Header kustom per request; nilai `env:NAMA` diambil dari environment
    /// saat koneksi (mis. `Authorization = "env:TOKEN_MCP"`).
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Argumen perintah.
    pub args: Vec<String>,
    /// Environment tambahan/penimpa.
    pub env: BTreeMap<String, String>,
    /// Timeout per request; `None` = `mcp.timeout_secs`.
    pub timeout_secs: Option<u64>,
    /// Apakah server dijalankan (default `true`).
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl GatewayConfig {
    /// Sinkronkan base_url/api_key_env dengan registry bila provider dikenal
    /// dan field itu masih default/berubah; dipanggil saat memuat config.
    pub fn sinkron_provider(&mut self) -> Result<(), String> {
        if self.provider.trim().is_empty() {
            return Err("gateway.provider tidak boleh kosong".to_string());
        }
        let Some(entry) = tui_wire::providers::by_id(&self.provider) else {
            let tersedia: Vec<String> = tui_wire::providers::all()
                .iter()
                .map(|p| p.id.clone())
                .collect();
            return Err(format!(
                "provider '{}' tidak ada di registry; pilihan: {}",
                self.provider,
                tersedia.join(", ")
            ));
        };
        // Entri dengan endpoint registry: pakai endpoint registry bila base_url
        // kosong, persis bawaan provider itu, ATAU persis bawaan provider lain
        // (artinya pengguna tidak menulis base_url sendiri — ia ikut provider).
        let base_url_bawaan_lain = tui_wire::providers::all()
            .iter()
            .any(|p| p.id != entry.id && !p.base_url.is_empty() && p.base_url == self.base_url);
        if !entry.base_url.is_empty()
            && (self.base_url.is_empty()
                || self.base_url == default_base_for(&entry.id)
                || base_url_bawaan_lain)
        {
            self.base_url = entry.base_url.clone();
        }
        let env_bawaan_lain = tui_wire::providers::all()
            .iter()
            .any(|p| p.id != entry.id && p.env_keys.first() == Some(&self.api_key_env));
        if self.api_key_env.is_empty()
            || self.api_key_env == default_env_for(&entry.id)
            || env_bawaan_lain
        {
            if let Some(env) = entry.env_keys.first() {
                self.api_key_env = env.clone();
            }
        }
        Ok(())
    }
}

/// Base URL bawaan provider (mengikuti tabel registry).
fn default_base_for(id: &str) -> String {
    tui_wire::providers::by_id(id)
        .map(|p| p.base_url.clone())
        .unwrap_or_default()
}

/// Env var bawaan provider.
fn default_env_for(id: &str) -> String {
    tui_wire::providers::by_id(id)
        .and_then(|p| p.env_keys.first().cloned())
        .unwrap_or_default()
}

impl UiConfig {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [("ui.accent", &self.accent), ("ui.dim", &self.dim)] {
            if !is_hex_color(value) {
                return Err(format!("{name} harus warna hex #rrggbb, bukan '{value}'"));
            }
        }
        Ok(())
    }
}

/// Apakah teks berbentuk `#rrggbb` (atau `#rgb`).
fn is_hex_color(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('#') else {
        return false;
    };
    matches!(rest.len(), 3 | 6) && rest.chars().all(|c| c.is_ascii_hexdigit())
}

pub mod secret;

/// Bagian [gateway].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GatewayConfig {
    /// Provider aktif (`modeluxai` bawaan, atau id dari registry).
    pub provider: String,
    /// Basis URL OpenAI-compatible (diakhiri `/v1`).
    pub base_url: String,
    /// Nama environment variable yang menyimpan kunci API.
    pub api_key_env: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        let bawaan = &tui_wire::providers::tabel()[0];
        Self {
            provider: bawaan.id.clone(),
            base_url: bawaan.base_url.clone(),
            api_key_env: bawaan.env_keys[0].clone(),
        }
    }
}

/// Bagian `[models]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelsConfig {
    /// Model untuk sesi baru.
    pub default: String,
    /// Model untuk compaction.
    pub compact: String,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            default: "deepseek-v4.1-flash".to_string(),
            compact: "deepseek-v4.1-flash".to_string(),
        }
    }
}

/// Bagian `[approval]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ApprovalConfig {
    /// Mode approval.
    pub mode: ApprovalMode,
    /// Folder yang selalu meminta persetujuan.
    pub restricted_folders: Vec<PathBuf>,
    /// Kebijakan per tool (`allow`/`deny`/`prompt`).
    pub tools: BTreeMap<String, ToolPolicy>,
    /// Masa berlaku grant `always` dalam jam; 0 = tidak kedaluwarsa.
    #[serde(default)]
    pub grant_ttl_hours: u64,
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            mode: ApprovalMode::AlwaysAsk,
            restricted_folders: Vec::new(),
            tools: BTreeMap::new(),
            grant_ttl_hours: 0,
        }
    }
}

impl From<ApprovalConfig> for ApprovalPolicy {
    fn from(config: ApprovalConfig) -> Self {
        Self {
            mode: config.mode,
            tools: config.tools,
            restricted_folders: config.restricted_folders,
        }
    }
}
/// Bagian `[limits]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsConfig {
    /// Deadline satu turn (detik).
    pub turn_deadline_secs: u64,
    /// Timeout approval (detik).
    pub approval_timeout_secs: u64,
    /// Timeout bash default (detik).
    pub bash_timeout_secs: u64,
    /// Batas atas timeout bash (detik).
    pub bash_max_timeout_secs: u64,
    /// Cap output tool (byte).
    pub tool_output_cap_bytes: usize,
    /// Ukuran window konteks model (token).
    pub context_window_tokens: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            turn_deadline_secs: 900,
            approval_timeout_secs: 600,
            bash_timeout_secs: 120,
            bash_max_timeout_secs: 600,
            tool_output_cap_bytes: 65536,
            context_window_tokens: 128_000,
        }
    }
}

/// Asal nilai satu lapisan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    /// Path file (atau label untuk lapisan default).
    pub path: PathBuf,
    /// Apakah file ini ada saat dibaca.
    pub exists: bool,
    /// Kunci tingkat atas yang diberikan file ini.
    pub keys: Vec<String>,
}

/// Hasil memuat config.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    /// Config efektif.
    pub config: Config,
    /// Lapisan yang dibaca, berurutan dari prioritas terendah.
    pub sources: Vec<SourceInfo>,
}

impl LoadedConfig {
    /// Kunci API dari environment, bila ada.
    pub fn api_key(&self) -> Option<String> {
        self.api_key_from(crate::secret::resolve_api_key)
    }

    /// Sumber kunci API: `env`, `keyring`, atau `tidak ada` (tanpa nilainya).
    pub fn api_key_source(&self) -> &'static str {
        crate::secret::source(&self.config.gateway.api_key_env)
    }

    /// Kunci API dengan lookup yang bisa diganti (dipakai test tanpa mengubah
    /// environment proses).
    pub fn api_key_from(&self, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
        let name = &self.config.gateway.api_key_env;
        match lookup(name) {
            Some(value) if !value.trim().is_empty() => Some(value),
            _ => None,
        }
    }

    /// Baris `kunci = nilai (sumber)` untuk `config show`; kunci API tidak
    /// pernah ditampilkan, hanya status keberadaannya.
    pub fn describe(&self) -> Vec<(String, String, String)> {
        let config = &self.config;
        let mut rows = vec![
            (
                "gateway.provider".to_string(),
                config.gateway.provider.clone(),
                self.source_of("gateway"),
            ),
            (
                "gateway.base_url".to_string(),
                config.gateway.base_url.clone(),
                self.source_of("gateway"),
            ),
            (
                "ui.accent".to_string(),
                config.ui.accent.clone(),
                self.source_of("ui"),
            ),
            (
                "ui.dim".to_string(),
                config.ui.dim.clone(),
                self.source_of("ui"),
            ),
            (
                "models.default".to_string(),
                config.models.default.clone(),
                self.source_of("models"),
            ),
            (
                "models.compact".to_string(),
                config.models.compact.clone(),
                self.source_of("models"),
            ),
            (
                "approval.mode".to_string(),
                format!("{:?}", config.approval.mode),
                self.source_of("approval"),
            ),
            (
                "approval.grant_ttl_hours".to_string(),
                config.approval.grant_ttl_hours.to_string(),
                self.source_of("approval"),
            ),
            (
                "limits.turn_deadline_secs".to_string(),
                config.limits.turn_deadline_secs.to_string(),
                self.source_of("limits"),
            ),
            (
                "limits.context_window_tokens".to_string(),
                config.limits.context_window_tokens.to_string(),
                self.source_of("limits"),
            ),
            (
                "limits.tool_output_cap_bytes".to_string(),
                config.limits.tool_output_cap_bytes.to_string(),
                self.source_of("limits"),
            ),
            (
                "mcp.timeout_secs".to_string(),
                config.mcp.timeout_secs.to_string(),
                self.source_of("mcp"),
            ),
            (
                "mcp.servers".to_string(),
                if config.mcp.servers.is_empty() {
                    "<kosong>".to_string()
                } else {
                    config
                        .mcp
                        .servers
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                },
                self.source_of("mcp"),
            ),
            (
                format!("gateway.api_key ({})", config.gateway.api_key_env),
                match self.api_key() {
                    Some(_) => "<ada, tidak ditampilkan>".to_string(),
                    None => "<belum diisi>".to_string(),
                },
                self.api_key_source().to_string(),
            ),
        ];
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    fn source_of(&self, key: &str) -> String {
        for source in self.sources.iter().rev() {
            if source.keys.iter().any(|k| k == key) {
                return source.path.display().to_string();
            }
        }
        "default".to_string()
    }
}

/// Path config global: `%APPDATA%\ModeluXAI\TUI\config.toml` di Windows.
pub fn default_global_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| {
        dirs.config_dir()
            .join("ModeluXAI")
            .join("TUI")
            .join("config.toml")
    })
}

/// Path config proyek: `<cwd>/.mx/config.toml`.
pub fn default_project_path(cwd: &Path) -> PathBuf {
    cwd.join(".mx").join("config.toml")
}

/// Ubah satu nilai config global (`a.b.c = value`) tanpa menghapus key lain.
///
/// Nilai diketik: `true`/`false` menjadi boolean, angka menjadi integer,
/// sisanya string. Hasilnya divalidasi dengan memuat ulang seluruh config,
/// jadi kesalahan nama key atau nilai tidak pernah tersimpan.
pub fn set_global_value(path: &Path, key: &str, raw: &str) -> Result<(), MxError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(config_error(path, &format!("tidak bisa dibaca: {err}"))),
    };
    let mut table: toml::Value = if existing.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&existing).map_err(|e| config_error(path, &e.to_string()))?
    };
    let root = table
        .as_table_mut()
        .ok_or_else(|| config_error(path, "isi config bukan tabel TOML"))?;
    let segments: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
    let (Some((last, parents)), true) = (segments.split_last(), segments.len() >= 2) else {
        return Err(MxError::new(
            ErrorCode::Config,
            format!("key '{key}' harus berbentuk bagian.nama"),
        ));
    };
    let mut node = root;
    for segment in parents {
        node = node
            .entry((*segment).to_string())
            .or_insert(toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                MxError::new(
                    ErrorCode::Config,
                    format!("bagian config '{segment}' bukan tabel"),
                )
            })?;
    }
    node.insert((*last).to_string(), typed_value(raw));
    let text = toml::to_string_pretty(&table)
        .map_err(|e| config_error(path, &format!("tidak bisa diserialisasi: {e}")))?;
    // Validasi hasil sebelum menyimpan: key asing dan nilai salah ditolak di sini.
    // Kunci asing bukan "TOML parse error" — pesannya harus menyebut apa yang salah.
    let parsed: Config = toml::from_str(&text).map_err(|e| {
        if e.message().starts_with("unknown field") {
            MxError::new(
                ErrorCode::Config,
                format!("kunci config tidak dikenal: {}", e.message()),
            )
        } else {
            config_error(path, &e.to_string())
        }
    })?;
    parsed
        .ui
        .validate()
        .map_err(|detail| MxError::new(ErrorCode::Config, detail))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| config_error(path, &e.to_string()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| config_error(path, &e.to_string()))?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path).map_err(|e| config_error(path, &e.to_string()))?;
    // Config memuat pengaturan gateway: batasi ACL ke pengguna saja.
    tui_wire::secure_file::restrict_to_current_user(path)
        .map_err(|e| config_error(path, &format!("gagal membatasi izin: {e}")))
}

fn typed_value(raw: &str) -> toml::Value {
    if let Ok(flag) = raw.parse::<bool>() {
        return toml::Value::Boolean(flag);
    }
    if let Ok(number) = raw.parse::<i64>() {
        return toml::Value::Integer(number);
    }
    toml::Value::String(raw.to_string())
}

/// Terapkan satu override runtime (`--set key=value`) pada config yang sudah dimuat.
///
/// Override tidak menulis file dan punya prioritas tertinggi; validasi sama
/// dengan `set_global_value` sehingga nilai salah ditolak sebelum dipakai.
pub fn apply_override(loaded: &mut LoadedConfig, key: &str, raw: &str) -> Result<(), MxError> {
    let mut table = toml::Value::try_from(&loaded.config)
        .map_err(|e| MxError::new(ErrorCode::Config, e.to_string()))?;
    set_value_in_table(&mut table, key, raw)?;
    let text =
        toml::to_string(&table).map_err(|e| MxError::new(ErrorCode::Config, e.to_string()))?;
    let parsed: Config =
        toml::from_str(&text).map_err(|e| MxError::new(ErrorCode::Config, e.to_string()))?;
    parsed
        .ui
        .validate()
        .map_err(|detail| MxError::new(ErrorCode::Config, detail))?;
    loaded.config = parsed;
    // `describe` mencari sumber per bagian tingkat atas, jadi simpan bagiannya.
    let section = key.split('.').next().unwrap_or(key).to_string();
    loaded.sources.push(SourceInfo {
        path: PathBuf::from("<runtime --set>"),
        exists: true,
        keys: vec![section],
    });
    Ok(())
}

fn set_value_in_table(table: &mut toml::Value, key: &str, raw: &str) -> Result<(), MxError> {
    let root = table
        .as_table_mut()
        .ok_or_else(|| MxError::new(ErrorCode::Config, "config bukan tabel TOML".to_string()))?;
    let segments: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return Err(MxError::new(
            ErrorCode::Config,
            format!("key '{key}' harus berbentuk bagian.nama"),
        ));
    }
    let (last, parents) = segments
        .split_last()
        .ok_or_else(|| MxError::new(ErrorCode::Config, format!("key '{key}' kosong")))?;
    let mut node = root;
    for segment in parents {
        node = node
            .entry((*segment).to_string())
            .or_insert(toml::Value::Table(toml::map::Map::new()))
            .as_table_mut()
            .ok_or_else(|| {
                MxError::new(
                    ErrorCode::Config,
                    format!("bagian config '{segment}' bukan tabel"),
                )
            })?;
    }
    node.insert((*last).to_string(), typed_value(raw));
    Ok(())
}

/// Muat config dari lapisan global lalu proyek.
pub fn load(global: Option<&Path>, project: Option<&Path>) -> Result<LoadedConfig, MxError> {
    let mut merged = toml::Value::Table(toml::map::Map::new());
    let mut sources = Vec::new();
    for path in [global, project].into_iter().flatten() {
        let exists = path.exists();
        let mut keys = Vec::new();
        if exists {
            let text = std::fs::read_to_string(path)
                .map_err(|e| config_error(path, &format!("tidak bisa dibaca: {e}")))?;
            // Validasi per file supaya pesan error membawa nomor baris dari TOML.
            let _validated: Config =
                toml::from_str(&text).map_err(|e| config_error(path, &e.to_string()))?;
            let value: toml::Value =
                toml::from_str(&text).map_err(|e| config_error(path, &e.to_string()))?;
            if let Some(table) = value.as_table() {
                keys.extend(table.keys().cloned());
            }
            merge_values(&mut merged, value);
        }
        sources.push(SourceInfo {
            path: path.to_path_buf(),
            exists,
            keys,
        });
    }
    let mut config: Config = merged.try_into().map_err(|e: toml::de::Error| {
        MxError::new(ErrorCode::Config, format!("config tidak valid: {e}"))
    })?;
    config
        .ui
        .validate()
        .map_err(|detail| MxError::new(ErrorCode::Config, detail))?;
    config
        .mcp
        .validate()
        .map_err(|detail| MxError::new(ErrorCode::Config, detail))?;
    config
        .gateway
        .sinkron_provider()
        .map_err(|detail| MxError::new(ErrorCode::Config, detail))?;
    Ok(LoadedConfig { config, sources })
}

fn merge_values(base: &mut toml::Value, overlay: toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                match base_table.get_mut(&key) {
                    Some(existing) => merge_values(existing, value),
                    None => {
                        base_table.insert(key, value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value;
        }
    }
}

fn config_error(path: &Path, detail: &str) -> MxError {
    MxError::new(ErrorCode::Config, format!("{}: {detail}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mx-config-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn set_kunci_asing_pesan_bukan_toml_parse_error() {
        let dir = temp_dir("acl-kunci");
        let path = dir.join("config.toml");
        let err = set_global_value(&path, "tidak.ada", "1").expect_err("harus ditolak");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(
            err.message.contains("kunci config tidak dikenal"),
            "pesan harus menyebut kunci asing: {}",
            err.message
        );
        assert!(
            err.message.contains("tidak"),
            "pesan harus menyebut kunci yang salah: {}",
            err.message
        );
        assert!(
            !err.message.to_lowercase().contains("parse error"),
            "bukan kesalahan sintaks TOML: {}",
            err.message
        );
        assert!(!path.exists(), "file tidak boleh ditulis saat ditolak");
    }

    #[test]
    fn provider_dikenal_sinkron_base_url_dan_env() {
        let dir = temp_dir("prov-sync");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[gateway]\nprovider = \"openrouter\"\n").unwrap();
        let loaded = load(Some(&path), None).expect("load");
        assert_eq!(loaded.config.gateway.provider, "openrouter");
        assert_eq!(
            loaded.config.gateway.base_url,
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(loaded.config.gateway.api_key_env, "OPENROUTER_API_KEY");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_default_modeluxai() {
        let dir = temp_dir("prov-default");
        let path = dir.join("config.toml");
        std::fs::write(&path, "").unwrap();
        let loaded = load(Some(&path), None).expect("load");
        assert_eq!(loaded.config.gateway.provider, "modeluxai");
        assert_eq!(
            loaded.config.gateway.base_url,
            "https://api.modeluxai.com/v1"
        );
        assert_eq!(loaded.config.gateway.api_key_env, "MODELUXAI_API_KEY");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provider_tak_kenal_ditolak_dengan_daftar() {
        let dir = temp_dir("prov-asing");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[gateway]\nprovider = \"bukan-ada\"\n").unwrap();
        let err = load(Some(&path), None).expect_err("harus ditolak");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(
            err.message.contains("tidak ada di registry"),
            "{}",
            err.message
        );
        assert!(err.message.contains("modeluxai"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn describe_menyebut_provider() {
        let dir = temp_dir("prov-describe");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[gateway]\nprovider = \"groq\"\n").unwrap();
        let loaded = load(Some(&path), None).expect("load");
        let rows = loaded.describe();
        assert!(
            rows.iter()
                .any(|(k, v, _)| k == "gateway.provider" && v == "groq")
        );
        assert!(
            rows.iter()
                .any(|(k, v, _)| k == "gateway.base_url" && v.contains("groq.com"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn provider_entry_serialisasi_bulat() {
        let entry = tui_wire::providers::by_id("openrouter").expect("openrouter");
        let teks = toml::to_string(&entry).expect("serialisasi");
        let kembali: tui_wire::providers::ProviderEntry =
            toml::from_str(&teks).expect("deserialisasi");
        assert_eq!(kembali, entry);
    }

    #[test]
    fn mcp_server_dibaca_dengan_default_benar() {
        let dir = temp_dir("mcp-baca");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[mcp]\ntimeout_secs = 12\n\n[mcp.servers.echo]\ncommand = \"powershell\"\nargs = [\"-File\", \"s.ps1\"]\nenv = { FOO = \"bar\" }\n\n[mcp.servers.mati]\ncommand = \"x\"\nenabled = false\n",
        )
        .unwrap();
        let loaded = load(Some(&path), None).expect("load");
        assert_eq!(loaded.config.mcp.timeout_secs, 12);
        assert_eq!(loaded.config.mcp.servers.len(), 2);
        let echo = loaded.config.mcp.servers.get("echo").expect("echo");
        assert_eq!(echo.command, "powershell");
        assert_eq!(echo.args.len(), 2);
        assert_eq!(echo.env.get("FOO").map(String::as_str), Some("bar"));
        assert!(echo.enabled);
        assert_eq!(echo.timeout_secs, None);
        let mati = loaded.config.mcp.servers.get("mati").expect("mati");
        assert!(!mati.enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_command_kosong_ditolak_dengan_pesan_jelas() {
        let dir = temp_dir("mcp-kosong");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[mcp.servers.rusak]\ncommand = \"\"\n").unwrap();
        let err = load(Some(&path), None).expect_err("harus ditolak");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("command wajib"), "{}", err.message);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mcp_field_asing_ditolak() {
        let dir = temp_dir("mcp-asing");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[mcp.servers.a]\ncommand = \"x\"\nliar = 1\n").unwrap();
        let err = load(Some(&path), None).expect_err("harus ditolak");
        assert!(
            err.message.contains("kunci config tidak dikenal")
                || err.message.contains("unknown field"),
            "{}",
            err.message
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn describe_menyebut_mcp() {
        let dir = temp_dir("mcp-describe");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[mcp]\ntimeout_secs = 15\n[mcp.servers.alpha]\ncommand = \"x\"\n",
        )
        .unwrap();
        let loaded = load(Some(&path), None).expect("load");
        let rows = loaded.describe();
        assert!(
            rows.iter()
                .any(|(k, v, _)| k == "mcp.timeout_secs" && v == "15")
        );
        assert!(
            rows.iter()
                .any(|(k, v, _)| k == "mcp.servers" && v == "alpha")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn grant_ttl_default_nol_dan_bisa_diubah() {
        assert_eq!(ApprovalConfig::default().grant_ttl_hours, 0);
        let dir = temp_dir("grant-ttl");
        let path = dir.join("config.toml");
        set_global_value(&path, "approval.grant_ttl_hours", "24").expect("set");
        let loaded = load(Some(&path), None).expect("load");
        assert_eq!(loaded.config.approval.grant_ttl_hours, 24);
        let row = loaded
            .describe()
            .into_iter()
            .find(|(key, _, _)| key == "approval.grant_ttl_hours")
            .expect("baris grant_ttl_hours");
        assert_eq!(row.1, "24");
        let mut loaded = load(None, None).unwrap();
        apply_override(&mut loaded, "approval.grant_ttl_hours", "6").expect("override");
        assert_eq!(loaded.config.approval.grant_ttl_hours, 6);
    }

    #[test]
    fn config_global_dibatasi_ke_pengguna_saja() {
        let dir = temp_dir("acl");
        let path = dir.join("config.toml");
        set_global_value(&path, "ui.dim", "#aabbcc").expect("set");
        assert!(
            tui_wire::secure_file::acl_restricted(&path).unwrap(),
            "file config harus 1 ACE dan terlindungi"
        );
    }

    #[test]
    fn ui_hex_tidak_valid_ditolak() {
        let dir = temp_dir("ui-invalid");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[ui]\naccent = \"biru\"\n").unwrap();
        let err = load(Some(&path), None).expect_err("harus error");
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("ui.accent"), "{}", err.message);
        assert!(err.message.contains("#rrggbb"), "{}", err.message);
    }

    #[test]
    fn ui_default_tampil_di_describe() {
        let loaded = load(None, None).expect("config default");
        let rows = loaded.describe();
        let accent = rows
            .iter()
            .find(|(key, _, _)| key == "ui.accent")
            .expect("baris ui.accent");
        assert_eq!(accent.1, "#d7a05b");
        assert_eq!(accent.2, "default");
    }

    #[test]
    fn set_nilai_tersimpan_dan_terbaca() {
        let dir = temp_dir("set-basic");
        let path = dir.join("config.toml");
        set_global_value(&path, "approval.mode", "yolo").expect("set");
        let loaded = load(Some(&path), None).expect("load");
        let row = loaded
            .describe()
            .into_iter()
            .find(|(key, _, _)| key == "approval.mode")
            .expect("baris approval.mode");
        assert!(row.1.to_lowercase().contains("yolo"), "{}", row.1);
    }

    #[test]
    fn set_mempertahankan_key_lain() {
        let dir = temp_dir("set-preserve");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[models]\ndefault = \"m-awal\"\n").unwrap();
        set_global_value(&path, "ui.dim", "#aabbcc").expect("set");
        let loaded = load(Some(&path), None).expect("load");
        assert_eq!(loaded.config.models.default, "m-awal");
        assert_eq!(loaded.config.ui.dim, "#aabbcc");
    }

    #[test]
    fn set_menolak_key_asing_dan_nilai_salah() {
        let dir = temp_dir("set-invalid");
        let path = dir.join("config.toml");
        let err = set_global_value(&path, "approval.modee", "yolo").expect_err("key asing");
        assert_eq!(err.code, ErrorCode::Config);
        let err = set_global_value(&path, "ui.accent", "biru").expect_err("nilai salah");
        assert!(err.message.contains("ui.accent"), "{}", err.message);
        let err = set_global_value(&path, "tanpa-titik", "x").expect_err("bentuk key");
        assert!(err.message.contains("bagian.nama"), "{}", err.message);
        assert!(!path.exists(), "file tidak ditulis saat validasi gagal");
    }

    #[test]
    fn set_angka_menjadi_integer() {
        let dir = temp_dir("set-angka");
        let path = dir.join("config.toml");
        set_global_value(&path, "limits.turn_deadline_secs", "42").expect("set");
        let loaded = load(Some(&path), None).expect("load");
        assert_eq!(loaded.config.limits.turn_deadline_secs, 42);
    }

    #[test]
    fn proyek_menimpa_global() {
        let dir = temp_dir("precedence");
        let global = dir.join("global.toml");
        let project = dir.join("project.toml");
        std::fs::write(&global, "[models]\ndefault = \"global-model\"\n").unwrap();
        std::fs::write(&project, "[models]\ndefault = \"proyek-model\"\n").unwrap();
        let loaded = load(Some(&global), Some(&project)).unwrap();
        assert_eq!(loaded.config.models.default, "proyek-model");
        assert_eq!(
            loaded.config.gateway.base_url,
            "https://api.modeluxai.com/v1"
        );
        let source = loaded
            .describe()
            .into_iter()
            .find(|(key, _, _)| key == "models.default")
            .map(|(_, _, source)| source)
            .unwrap();
        assert!(source.ends_with("project.toml"));
    }

    #[test]
    fn override_runtime_menimpa_proyek_dan_tidak_menulis_file() {
        let dir = temp_dir("override");
        let global = dir.join("global.toml");
        let project = dir.join("project.toml");
        std::fs::write(&global, "[models]\ndefault = \"dari-global\"\n").unwrap();
        std::fs::write(&project, "[models]\ndefault = \"dari-proyek\"\n").unwrap();
        let mut loaded = load(Some(&global), Some(&project)).unwrap();
        assert_eq!(loaded.config.models.default, "dari-proyek");
        apply_override(&mut loaded, "models.default", "dari-runtime").unwrap();
        assert_eq!(loaded.config.models.default, "dari-runtime");
        let file_after = std::fs::read_to_string(&project).unwrap();
        assert!(
            file_after.contains("dari-proyek"),
            "file tidak boleh berubah"
        );
        assert_eq!(
            loaded.sources.last().map(|s| s.path.clone()),
            Some(PathBuf::from("<runtime --set>"))
        );
    }

    #[test]
    fn override_runtime_menolak_nilai_tidak_valid() {
        let dir = temp_dir("override-invalid");
        let path = dir.join("c.toml");
        std::fs::write(&path, "[ui]\naccent = \"#112233\"\n").unwrap();
        let mut loaded = load(Some(&path), None).unwrap();
        let err = apply_override(&mut loaded, "ui.accent", "biru").unwrap_err();
        assert_eq!(err.code, ErrorCode::Config);
        assert_eq!(
            loaded.config.ui.accent, "#112233",
            "nilai lama dipertahankan"
        );
        let err = apply_override(&mut loaded, "tanpa-titik", "x").unwrap_err();
        assert!(err.message.contains("bagian.nama"), "{}", err.message);
    }

    #[test]
    fn config_rusak_menyebut_nama_kunci() {
        let dir = temp_dir("invalid-key");
        let path = dir.join("rusak.toml");
        // Typo pada nama kunci harus dilaporkan menyebut kuncinya.
        std::fs::write(&path, "[gateway]\nbase_urll = \"x\"\n").unwrap();
        let err = load(None, Some(&path)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("rusak.toml"), "{}", err.message);
        assert!(
            err.message.contains("base_urll"),
            "pesan harus menyebut kunci yang salah: {}",
            err.message
        );
    }

    #[test]
    fn config_rusak_menyebut_file_dan_baris() {
        let dir = temp_dir("invalid");
        let path = dir.join("rusak.toml");
        std::fs::write(&path, "[gateway]\nbase_url = 42\n").unwrap();
        let err = load(None, Some(&path)).unwrap_err();
        assert_eq!(err.code, ErrorCode::Config);
        assert!(err.message.contains("rusak.toml"));
        assert!(
            err.message.contains("line"),
            "pesan harus menyebut baris: {}",
            err.message
        );
    }

    #[test]
    fn sumber_kunci_dilaporkan_tanpa_nilai() {
        assert_eq!(
            secret::source_from(|_| Some("x".to_string()), |_| Some("y".to_string()), "K"),
            "env"
        );
        assert_eq!(
            secret::source_from(|_| Some("  ".to_string()), |_| Some("y".to_string()), "K"),
            "keyring"
        );
        assert_eq!(secret::source_from(|_| None, |_| None, "K"), "tidak ada");
        let dir = temp_dir("sumber-kunci");
        let path = dir.join("c.toml");
        let loaded = load(None, Some(&path)).unwrap();
        let rows = loaded.describe();
        let (_, _, source) = rows
            .iter()
            .find(|(key, _, _)| key.starts_with("gateway.api_key"))
            .expect("baris kunci ada");
        assert!(
            matches!(source.as_str(), "env" | "keyring" | "tidak ada"),
            "sumber tidak dikenal: {source}"
        );
    }

    #[test]
    fn kunci_tidak_akan_ditampilkan() {
        let dir = temp_dir("redaction");
        let path = dir.join("c.toml");
        std::fs::write(&path, "[gateway]\napi_key_env = \"MX_TEST_KEY\"\n").unwrap();
        let loaded = load(None, Some(&path)).unwrap();
        assert_eq!(loaded.config.gateway.api_key_env, "MX_TEST_KEY");
        assert_eq!(
            loaded.api_key_from(|_| Some("kunci-rahasia-123".to_string())),
            Some("kunci-rahasia-123".to_string())
        );
        let rows = loaded.describe();
        assert!(
            rows.iter()
                .all(|(_, value, _)| !value.contains("kunci-rahasia-123"))
        );
        let key_row = rows
            .iter()
            .find(|(key, _, _)| key.contains("api_key"))
            .unwrap();
        assert!(key_row.1.contains("belum diisi"));
    }

    #[test]
    fn approval_config_dipetakan_ke_policy() {
        let mut config = ApprovalConfig {
            mode: ApprovalMode::Write,
            ..ApprovalConfig::default()
        };
        config.tools.insert("bash".to_string(), ToolPolicy::Deny);
        let policy = ApprovalPolicy::from(config);
        assert_eq!(policy.mode, ApprovalMode::Write);
        assert_eq!(policy.tools.get("bash"), Some(&ToolPolicy::Deny));
        assert!(policy.restricted_folders.is_empty());
    }
    #[test]
    fn file_tidak_ada_bukan_error() {
        let dir = temp_dir("missing");
        let loaded = load(Some(&dir.join("tidak-ada.toml")), None).unwrap();
        assert_eq!(loaded.sources.len(), 1);
        assert!(!loaded.sources[0].exists);
    }
}
