//! CLI `maicli`: antarmuka baris perintah untuk sesi agent.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Mode UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// TUI penuh (ratatui).
    Tui,
    /// REPL teks untuk lingkungan tanpa terminal interaktif.
    Text,
    /// Server RPC JSONL di stdin/stdout (untuk otomasi dan harness).
    Rpc,
    /// API server HTTP lokal (loopback; `--port`).
    Serve,
    /// Relay WebSocket collab (room view/control; `--port`).
    Relay,
    /// Agen ACP di stdin/stdout (host eksternal/lokal mengendalikan).
    Acp,
}

/// Aksi `maicli config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConfigAction {
    /// Tampilkan config efektif beserta asal nilainya.
    Show,
    /// Tampilkan path config global dan proyek.
    Path,
    /// Set satu nilai di config global.
    Set,
    /// Simpan kunci API dari env var ke keyring OS.
    KeyImport,
}

/// Argumen baris perintah.
#[derive(Debug, Parser)]
#[command(name = "maicli", version, about = "TUI agentic ModeluXAI")]
pub struct Cli {
    /// Mode UI.
    #[arg(long, value_enum, default_value_t = Mode::Tui)]
    pub mode: Mode,
    /// Model yang dipakai untuk sesi ini (menimpa config).
    #[arg(long)]
    pub model: Option<String>,
    /// Direktori kerja sesi.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Jalankan satu prompt lalu keluar (tanpa UI).
    #[arg(long)]
    pub prompt: Option<String>,
    /// Lanjutkan sesi terakhir (`--resume`) atau sesi dengan id tertentu.
    #[arg(long, num_args = 0..=1, default_missing_value = "latest", value_name = "ID")]
    pub resume: Option<String>,
    /// Buat salinan sesi (fork) lalu lanjutkan salinannya.
    #[arg(long, num_args = 0..=1, default_missing_value = "latest", value_name = "ID")]
    pub fork: Option<String>,
    /// Override config sekali jalan (`bagian.kunci=nilai`); boleh diulang.
    #[arg(long = "set", value_name = "KEY=VALUE", action = clap::ArgAction::Append)]
    pub set: Vec<String>,
    /// Rekam setiap event agen ke berkas JSONL (untuk parity/harness).
    #[arg(long, value_name = "FILE")]
    pub event_log: Option<PathBuf>,
    /// Port API server (`--mode serve`; default 8787).
    #[arg(long, default_value_t = 8787)]
    pub port: u16,
    /// Berkas transkrip acuan (JSONL event agen) untuk parity.
    #[arg(long, value_name = "FILE", requires = "parity_actual")]
    pub parity_expected: Option<PathBuf>,
    /// Berkas transkrip hasil yang dibandingkan dengan acuan.
    #[arg(long, value_name = "FILE", requires = "parity_expected")]
    pub parity_actual: Option<PathBuf>,
    /// Hapus sesi (file + lock) lalu keluar.
    #[arg(long, value_name = "ID")]
    pub drop_session: Option<String>,
    /// Muat katalog omp dari direktori `rules/` (menimpa config catalog.path).
    #[arg(long, value_name = "DIR")]
    pub catalog: Option<PathBuf>,
    /// Jangan muat extension eksternal (menimpa config extensions.enabled).
    #[arg(long)]
    pub no_extensions: bool,
    /// Jalankan wizard setup (pilih provider, kunci, model) lalu keluar.
    #[arg(long, default_value_t = false)]
    pub setup: bool,
    /// Tampilkan daftar sesi (opsional difilter judul) lalu keluar.
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "JUDUL")]
    pub list_sessions: Option<String>,
    /// Salin semua transkrip yang terdaftar di indeks SQLite ke direktori ini.
    #[arg(long, value_name = "DIR")]
    pub backup_sessions: Option<PathBuf>,
    /// Perintah tambahan.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Subperintah.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Berurusan dengan config.
    Config {
        /// Aksi yang dijalankan.
        #[arg(value_enum, default_value_t = ConfigAction::Show)]
        action: ConfigAction,
        /// Key untuk `set`, mis. `approval.mode`.
        key: Option<String>,
        /// Nilai untuk `set`.
        value: Option<String>,
    },
}

/// Parse argumen proses saat ini.
pub fn parse() -> Cli {
    Cli::parse()
}
