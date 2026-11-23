//! Registry slash command v1.
//!
//! Command tidak pernah memanggil model; ia mengembalikan hasil yang dipakai
//! UI (teks balasan, ganti model, keluar). Menambah command = menambah baris di
//! `COMMANDS` + satu cabang di `dispatch` + test.

use std::path::Path;

/// Konteks yang dibutuhkan command.
pub struct CommandContext<'a> {
    /// Model aktif sesi.
    pub model: &'a str,
    /// Provider aktif (id registry) dan env var kuncinya.
    pub provider: (&'a str, &'a str),
    /// Nama tool yang terdaftar.
    pub tools: &'a [String],
    /// Path file sesi, bila sudah dibuat.
    pub session_path: Option<&'a Path>,
    /// Id model yang diketahui dari discovery gateway (boleh kosong).
    pub known_models: &'a [String],
    /// Direktori sesi (untuk `/tree`); None = tidak tersedia.
    pub sessions_dir: Option<&'a Path>,
    /// Ringkasan skill aktif (untuk `/skills`); kosong = tidak ada skill.
    pub skills_ringkas: &'a str,
    /// Window konteks dari config (untuk `/extended-context`).
    pub context_window: Option<u64>,
    /// Tingkat penalaran aktif (untuk `/effort` tanpa argumen).
    pub effort: Option<&'a str>,
}

/// Hasil eksekusi satu baris input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// Tampilkan teks ini ke pengguna.
    Reply(String),
    /// Ganti model sesi ke id ini.
    SetModel(String),
    /// Ringkas konteks sekarang.
    Compact,
    /// Jeda sesi.
    Pause,
    /// Lanjutkan sesi yang dijeda.
    Resume,
    /// Tampilkan pemakaian token.
    Usage,
    /// Tampilkan daftar sesi tersimpan (opsional difilter judul).
    Sessions(Option<String>),
    /// Pindah ke sesi lain (`None` = `latest`).
    SwitchSession(Option<String>),
    /// Ganti judul sesi.
    SetTitle(String),
    /// Tampilkan pemakaian konteks terhadap anggaran.
    Context,
    /// Tampilkan daftar todo sesi.
    Todo,
    /// Mulai sesi baru (mengosongkan konteks).
    NewSession,
    /// Duplikasi sesi sekarang lalu lanjutkan salinannya.
    ForkSession,
    /// Tampilkan pohon sesi (fork; kedalaman = cabang).
    Tree,
    /// Hapus semua grant approval yang diingat.
    ForgetApprovals,
    /// Bersihkan layar timeline (UI).
    ClearScreen,
    /// Set tingkat penalaran (`low`/`medium`/`high`); `None` = default gateway.
    SetEffort(Option<String>),
    /// Set window konteks token (Some = extended; None = kembali config).
    SetContextWindow(Option<u64>),
    /// Aktifkan yolo hanya untuk turn berikutnya.
    ForceOnce,
    /// Tampilkan CHANGELOG (bagian teratas).
    Changelog,
    /// Tampilkan statistik sesi.
    Stats,
    /// Jalankan ulang wizard setup.
    Setup,
    /// Buka config global di $EDITOR.
    Settings,
    /// Hapus kunci provider dari keyring.
    Logout,
    /// Daftar server MCP (`remove <id>` dipegang mode karena mengubah config).
    McpList,
    /// Hapus server MCP dari config global.
    McpRemove(String),
    /// Tulis transkrip sesi ke berkas Markdown (default: maicli-export.md).
    Export(Option<String>),
    /// Ringkas sesi sekarang lalu mulai sesi baru berisi ringkasannya.
    Handoff,
    /// Minta agen membuat commit git (tetap lewat approval seperti tool lain).
    Commit,
    /// Subcommand `/todo add|rm|clear` (penggantian daftar didelegasi mode).
    TodoSub(char, Option<String>),
    /// Command fitur yang dieksekusi mode UI (nama tanpa garis miring).
    Feature {
        /// Nama command, mis. `memory` atau `mcp-reauth`.
        name: String,
        /// Argumen mentah setelah nama.
        argument: Option<String>,
    },
    /// Tampilkan daftar provider (tanpa argumen) atau ganti provider aktif.
    SetProvider(Option<String>),
    /// Keluar dari sesi.
    Quit,
    /// Perintah tidak dikenal.
    Unknown(String),
    /// Bukan command (kirim ke model).
    NotACommand,
}

/// Command yang dieksekusi mode UI (bukan logika murni core).
pub const FEATURE_NAMES: &[&str] = &[
    "fresh",
    "pin",
    "unpin",
    "jobs",
    "git",
    "local",
    "browser",
    "copy",
    "open",
    "trace",
    "dump",
    "debug",
    "elide",
    "shake",
    "modelpreset",
    "preset",
    "skillful",
    "collab",
    "memory",
    "mm",
    "marketplace",
    "plugins",
    "ssh",
    "advisor",
    "computer",
    "branch",
    "fast",
    "slow",
    "live",
    "record",
    "delete",
    "btw",
    "tan",
    "omfg",
    "cleanse",
    "mcp-add",
    "mcp-test",
    "mcp-enable",
    "mcp-disable",
    "export-anon",
    "checkpoint",
    "rewind",
    "hook-test",
    "artifact",
    "conflict",
    "xd",
    "tool-issue",
    "tool-issues",
    "tool-issue-resolve",
    "task-worktree",
    "skill-new",
    "skill-rm",
    "skill-list",
    "diagnose",
    "compare",
    "watchdog",
    "prewalk",
    "research",
    "guided-goal",
    "think",
    "yield",
    "goal",
    "loop",
    "ratchet",
    "accept",
    "staging",
    "system-prompt",
    "plan",
    "plan-review",
    "install-id",
    "prompts",
    "notifications",
    "budget",
    "edit",
    "add-dir",
    "dirs",
    "review",
    "annotate",
    "context-note",
    "new-context",
    "todo-export",
    "todo-import",
    "todo-copy",
];

/// Daftar command v1 beserta deskripsi satu baris.
/// Satu command builtin beserta atributnya (C-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// Nama tanpa garis miring depan.
    pub name: &'static str,
    /// Pemakaian argumen, mis. `<>` atau `[id]`; kosong berarti tanpa argumen.
    pub usage: &'static str,
    /// Nama alternatif yang setara.
    pub aliases: &'static [&'static str],
    /// Deskripsi satu baris untuk `/help`.
    pub description: &'static str,
    /// Apakah command menerima argumen.
    pub allow_args: bool,
}

/// Registry command builtin v1 (satu-satunya precedence di v1).
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "help",
        usage: "",
        aliases: &["?"],
        description: "daftar perintah",
        allow_args: false,
    },
    CommandSpec {
        name: "model",
        usage: "[id]",
        aliases: &[],
        description: "tampilkan atau ganti model sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "compact",
        usage: "",
        aliases: &[],
        description: "ringkas konteks sekarang",
        allow_args: false,
    },
    CommandSpec {
        name: "pause",
        usage: "",
        aliases: &[],
        description: "jeda sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "resume",
        usage: "",
        aliases: &[],
        description: "lanjutkan sesi yang dijeda",
        allow_args: false,
    },
    CommandSpec {
        name: "usage",
        usage: "",
        aliases: &[],
        description: "pemakaian token sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "sessions",
        usage: "[judul]",
        aliases: &[],
        description: "daftar sesi tersimpan",
        allow_args: true,
    },
    CommandSpec {
        name: "tree",
        usage: "",
        aliases: &[],
        description: "pohon sesi (fork)",
        allow_args: false,
    },
    CommandSpec {
        name: "switch",
        usage: "[id]",
        aliases: &[],
        description: "pindah sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "title",
        usage: "<teks>",
        aliases: &[],
        description: "ganti judul sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "context",
        usage: "",
        aliases: &[],
        description: "pemakaian konteks vs anggaran",
        allow_args: false,
    },
    CommandSpec {
        name: "todo",
        usage: "[add|append|start|done|drop|rm|clear|copy|export|import]",
        aliases: &[],
        description: "daftar dan kelola todo sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "new",
        usage: "",
        aliases: &[],
        description: "mulai sesi baru",
        allow_args: false,
    },
    CommandSpec {
        name: "fork",
        usage: "[pane|window|tab]",
        aliases: &[],
        description: "duplikasi sesi lalu lanjutkan salinannya (argumen = buka di pane/window)",
        allow_args: true,
    },
    CommandSpec {
        name: "tools",
        usage: "",
        aliases: &[],
        description: "daftar tool aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "session",
        usage: "",
        aliases: &[],
        description: "path file sesi",
        allow_args: false,
    },
    CommandSpec {
        name: "provider",
        usage: "[id]",
        aliases: &[],
        description: "daftar atau ganti provider gateway",
        allow_args: true,
    },
    CommandSpec {
        name: "clear",
        usage: "",
        aliases: &["cls"],
        description: "bersihkan layar timeline",
        allow_args: false,
    },
    CommandSpec {
        name: "skills",
        usage: "",
        aliases: &[],
        description: "daftar skill aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "effort",
        usage: "[low|medium|high]",
        aliases: &[],
        description: "tingkat penalaran model (tanpa argumen = lihat sekarang)",
        allow_args: true,
    },
    CommandSpec {
        name: "extended-context",
        usage: "[on|off]",
        aliases: &["extctx"],
        description: "pakai window konteks besar (on) atau kembali ke config (off)",
        allow_args: true,
    },
    CommandSpec {
        name: "force",
        usage: "",
        aliases: &["yolo"],
        description: "turn berikutnya berjalan tanpa approval (sekali)",
        allow_args: false,
    },
    CommandSpec {
        name: "stats",
        usage: "",
        aliases: &[],
        description: "statistik sesi (pesan, token)",
        allow_args: false,
    },
    CommandSpec {
        name: "changelog",
        usage: "",
        aliases: &[],
        description: "CHANGELOG bagian teratas",
        allow_args: false,
    },
    CommandSpec {
        name: "setup",
        usage: "",
        aliases: &[],
        description: "wizard setup ulang (provider, kunci, model)",
        allow_args: false,
    },
    CommandSpec {
        name: "settings",
        usage: "",
        aliases: &[],
        description: "buka config global di editor",
        allow_args: false,
    },
    CommandSpec {
        name: "logout",
        usage: "",
        aliases: &[],
        description: "hapus kunci provider dari keyring",
        allow_args: false,
    },
    CommandSpec {
        name: "mcp",
        usage: "[list|add <id> <url>|remove <id>|enable/disable <id>|test <id>|reauth <id>]",
        aliases: &[],
        description: "kelola server MCP (tanpa argumen = list)",
        allow_args: true,
    },
    CommandSpec {
        name: "export",
        usage: "[berkas]",
        aliases: &[],
        description: "tulis transkrip sesi ke berkas Markdown",
        allow_args: true,
    },
    CommandSpec {
        name: "handoff",
        usage: "",
        aliases: &[],
        description: "ringkas sesi lalu mulai sesi baru berisi ringkasannya",
        allow_args: false,
    },
    CommandSpec {
        name: "commit",
        usage: "",
        aliases: &[],
        description: "minta agen membuat commit git dari perubahan saat ini",
        allow_args: false,
    },
    CommandSpec {
        name: "forget-approvals",
        usage: "",
        aliases: &[],
        description: "hapus semua grant approval",
        allow_args: false,
    },
    CommandSpec {
        name: "fresh",
        usage: "",
        aliases: &[],
        description: "sesi baru tanpa membawa grant approval sesi lama",
        allow_args: false,
    },
    CommandSpec {
        name: "pin",
        usage: "[no]",
        aliases: &[],
        description: "sematkan pesan ke-N (default: pesan terakhir)",
        allow_args: true,
    },
    CommandSpec {
        name: "unpin",
        usage: "[no]",
        aliases: &[],
        description: "lepas sematan pesan ke-N (default: semua)",
        allow_args: true,
    },
    CommandSpec {
        name: "jobs",
        usage: "",
        aliases: &[],
        description: "daftar job/subagent latar",
        allow_args: false,
    },
    CommandSpec {
        name: "git",
        usage: "",
        aliases: &[],
        description: "status git repo aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "local",
        usage: "",
        aliases: &[],
        description: "tampilkan setting sesi aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "browser",
        usage: "<url>",
        aliases: &[],
        description: "buka URL di browser default",
        allow_args: true,
    },
    CommandSpec {
        name: "copy",
        usage: "<teks>",
        aliases: &[],
        description: "salin teks ke clipboard",
        allow_args: true,
    },
    CommandSpec {
        name: "open",
        usage: "<path|url>",
        aliases: &[],
        description: "buka path/URL dengan aplikasi default",
        allow_args: true,
    },
    CommandSpec {
        name: "trace",
        usage: "",
        aliases: &[],
        description: "tulis permintaan model terakhir ke berkas JSON",
        allow_args: false,
    },
    CommandSpec {
        name: "dump",
        usage: "",
        aliases: &[],
        description: "tulis snapshot state sesi ke berkas JSON",
        allow_args: false,
    },
    CommandSpec {
        name: "debug",
        usage: "",
        aliases: &[],
        description: "tulis dump debug lengkap (state + permintaan terakhir)",
        allow_args: false,
    },
    CommandSpec {
        name: "elide",
        usage: "[keep]",
        aliases: &[],
        description: "pangkas hasil tool lama (default semua)",
        allow_args: true,
    },
    CommandSpec {
        name: "shake",
        usage: "[semua|thinking|images]",
        aliases: &[],
        description: "pangkas konteks tanpa memanggil model",
        allow_args: true,
    },
    CommandSpec {
        name: "modelpreset",
        usage: "[save|use|rm|list] ...",
        aliases: &["preset"],
        description: "kelola preset model/effort",
        allow_args: true,
    },
    CommandSpec {
        name: "skillful",
        usage: "[on|off]",
        aliases: &[],
        description: "sertakan daftar skill di system prompt",
        allow_args: true,
    },
    CommandSpec {
        name: "collab",
        usage: "[start|join <url> <room> <kunci>|status|stop]",
        aliases: &[],
        description: "relay collab WebSocket in-TUI",
        allow_args: true,
    },
    CommandSpec {
        name: "memory",
        usage: "[view|add|forget|stats|backend] ...",
        aliases: &[],
        description: "memori persisten lintas sesi",
        allow_args: true,
    },
    CommandSpec {
        name: "mm",
        usage: "[list|show|add|refresh] ...",
        aliases: &[],
        description: "mental model dari memori",
        allow_args: true,
    },
    CommandSpec {
        name: "marketplace",
        usage: "[list|search|install|remove|update] ...",
        aliases: &[],
        description: "marketplace extension/plugin",
        allow_args: true,
    },
    CommandSpec {
        name: "plugins",
        usage: "[list|reload]",
        aliases: &[],
        description: "daftar/muat ulang plugin",
        allow_args: true,
    },
    CommandSpec {
        name: "ssh",
        usage: "[add|list|remove|test] ...",
        aliases: &[],
        description: "kelola host SSH untuk exec remote",
        allow_args: true,
    },
    CommandSpec {
        name: "advisor",
        usage: "[on|off|model <id>]",
        aliases: &[],
        description: "model penasihat kedua",
        allow_args: true,
    },
    CommandSpec {
        name: "hook-test",
        usage: "<event>",
        aliases: &[],
        description: "jalankan hook eksternal dari config [hooks]",
        allow_args: true,
    },
    CommandSpec {
        name: "artifact",
        usage: "[add <path>|list|get <id>]",
        aliases: &[],
        description: "kelola lampiran/biner sebagai artefak",
        allow_args: true,
    },
    CommandSpec {
        name: "conflict",
        usage: "[N|conflict://N] [ours|theirs|base]",
        aliases: &[],
        description: "daftar/selesaikan konflik merge git",
        allow_args: true,
    },
    CommandSpec {
        name: "xd",
        usage: "<uri>",
        aliases: &[],
        description: "resolusi URI internal (xd://issue/<id>)",
        allow_args: true,
    },
    CommandSpec {
        name: "tool-issue",
        usage: "<tool> <severity> <pesan>",
        aliases: &[],
        description: "laporkan masalah tool",
        allow_args: true,
    },
    CommandSpec {
        name: "tool-issues",
        usage: "",
        aliases: &[],
        description: "daftar isu tool",
        allow_args: false,
    },
    CommandSpec {
        name: "tool-issue-resolve",
        usage: "<id>",
        aliases: &[],
        description: "tandai isu tool selesai",
        allow_args: true,
    },
    CommandSpec {
        name: "task-worktree",
        usage: "[keep] <prompt>",
        aliases: &[],
        description: "jalankan prompt di worktree git terisolasi",
        allow_args: true,
    },
    CommandSpec {
        name: "skill-new",
        usage: "<nama> <deskripsi>",
        aliases: &[],
        description: "buat skill terkelola di <cwd>/skills/",
        allow_args: true,
    },
    CommandSpec {
        name: "skill-rm",
        usage: "<nama>",
        aliases: &[],
        description: "hapus skill terkelola",
        allow_args: true,
    },
    CommandSpec {
        name: "skill-list",
        usage: "",
        aliases: &[],
        description: "daftar skill workspace",
        allow_args: false,
    },
    CommandSpec {
        name: "diagnose",
        usage: "",
        aliases: &[],
        description: "ringkasan kondisi git + anggaran konteks",
        allow_args: false,
    },
    CommandSpec {
        name: "compare",
        usage: "[ref]",
        aliases: &[],
        description: "bandingkan working tree dengan ref git (default HEAD)",
        allow_args: true,
    },
    CommandSpec {
        name: "watchdog",
        usage: "<pola1,pola2>",
        aliases: &[],
        description: "atur pola larangan TTSR/watchdog",
        allow_args: true,
    },
    CommandSpec {
        name: "prewalk",
        usage: "[langkah]",
        aliases: &[],
        description: "telusuri rencana read-only sebelum eksekusi",
        allow_args: true,
    },
    CommandSpec {
        name: "research",
        usage: "<pertanyaan> [putaran 2..5]",
        aliases: &[],
        description: "riset terarah read-only beberapa putaran",
        allow_args: true,
    },
    CommandSpec {
        name: "guided-goal",
        usage: "<tujuan>",
        aliases: &[],
        description: "goal + langkah pertama terarah",
        allow_args: true,
    },
    CommandSpec {
        name: "think",
        usage: "<teks>",
        aliases: &[],
        description: "catatan penalaran internal",
        allow_args: true,
    },
    CommandSpec {
        name: "yield",
        usage: "[teks]",
        aliases: &[],
        description: "tandai menunggu masukan",
        allow_args: true,
    },
    CommandSpec {
        name: "goal",
        usage: "[<teks>|clear]",
        aliases: &[],
        description: "tujuan persisten untuk ratchet/loop",
        allow_args: true,
    },
    CommandSpec {
        name: "loop",
        usage: "<n> <prompt>",
        aliases: &[],
        description: "antrekan prompt n kali (1..20)",
        allow_args: true,
    },
    CommandSpec {
        name: "ratchet",
        usage: "[prompt]",
        aliases: &[],
        description: "satu langkah lanjutan dari goal",
        allow_args: true,
    },
    CommandSpec {
        name: "accept",
        usage: "<id>",
        aliases: &[],
        description: "terima perubahan staging secara atomik",
        allow_args: true,
    },
    CommandSpec {
        name: "staging",
        usage: "",
        aliases: &[],
        description: "daftar perubahan staging",
        allow_args: false,
    },
    CommandSpec {
        name: "system-prompt",
        usage: "[<teks>|clear]",
        aliases: &[],
        description: "override system prompt pengguna",
        allow_args: true,
    },
    CommandSpec {
        name: "plan",
        usage: "[<teks>|append <teks>|clear]",
        aliases: &[],
        description: "kelola rencana kerja persisten",
        allow_args: true,
    },
    CommandSpec {
        name: "plan-review",
        usage: "",
        aliases: &[],
        description: "tampilkan rencana kerja aktif",
        allow_args: false,
    },
    CommandSpec {
        name: "install-id",
        usage: "",
        aliases: &[],
        description: "tampilkan id instalasi lokal (tanpa telemetri)",
        allow_args: false,
    },
    CommandSpec {
        name: "prompts",
        usage: "[list|use <nama>|clear]",
        aliases: &[],
        description: "kelola prompt files dari <cwd>/prompts/*.md",
        allow_args: true,
    },
    CommandSpec {
        name: "notifications",
        usage: "[add <teks>|list|clear]",
        aliases: &[],
        description: "kelola notifikasi aktif untuk system prompt",
        allow_args: true,
    },
    CommandSpec {
        name: "budget",
        usage: "",
        aliases: &[],
        description: "ringkasan pemakaian token dan sisa konteks",
        allow_args: false,
    },
    CommandSpec {
        name: "edit",
        usage: "[berkas]",
        aliases: &[],
        description: "buka berkas di editor eksternal (VISUAL/EDITOR)",
        allow_args: true,
    },
    CommandSpec {
        name: "add-dir",
        usage: "<direktori>",
        aliases: &[],
        description: "tambah root pencarian workspace",
        allow_args: true,
    },
    CommandSpec {
        name: "dirs",
        usage: "[clear]",
        aliases: &[],
        description: "daftar root workspace aktif",
        allow_args: true,
    },
    CommandSpec {
        name: "review",
        usage: "[save]",
        aliases: &[],
        description: "review model kedua terhadap diff git (save = simpan anotasi)",
        allow_args: true,
    },
    CommandSpec {
        name: "annotate",
        usage: "[add <teks>|list|clear]",
        aliases: &[],
        description: "kelola anotasi review pada system prompt",
        allow_args: true,
    },
    CommandSpec {
        name: "checkpoint",
        usage: "[label|list]",
        aliases: &[],
        description: "simpan patch git + posisi pesan sebagai checkpoint",
        allow_args: true,
    },
    CommandSpec {
        name: "rewind",
        usage: "[id|latest]",
        aliases: &[],
        description: "kembalikan patch git dan potong sesi ke checkpoint",
        allow_args: true,
    },
    CommandSpec {
        name: "context-note",
        usage: "[add <teks>|list|clear]",
        aliases: &["note"],
        description: "catatan konteks persisten untuk system prompt",
        allow_args: true,
    },
    CommandSpec {
        name: "new-context",
        usage: "",
        aliases: &[],
        description: "sesi baru dengan catatan konteks tetap dimuat",
        allow_args: false,
    },
    CommandSpec {
        name: "computer",
        usage: "[status]",
        aliases: &[],
        description: "status tool kontrol desktop",
        allow_args: true,
    },
    CommandSpec {
        name: "branch",
        usage: "[judul]",
        aliases: &[],
        description: "fork sesi lalu buka di pane/window terminal",
        allow_args: true,
    },
    CommandSpec {
        name: "fast",
        usage: "",
        aliases: &[],
        description: "tier layanan prioritas (service_tier=priority)",
        allow_args: false,
    },
    CommandSpec {
        name: "slow",
        usage: "",
        aliases: &[],
        description: "kembali ke tier layanan default",
        allow_args: false,
    },
    CommandSpec {
        name: "live",
        usage: "[start|join <url> <room> <kunci>|status|stop]",
        aliases: &[],
        description: "relay live sesi (alias collab)",
        allow_args: true,
    },
    CommandSpec {
        name: "record",
        usage: "[on|off|status]",
        aliases: &[],
        description: "rekam event sesi ke JSONL",
        allow_args: true,
    },
    CommandSpec {
        name: "delete",
        usage: "<id>",
        aliases: &[],
        description: "hapus sesi tersimpan (file + lock)",
        allow_args: true,
    },
    CommandSpec {
        name: "btw",
        usage: "",
        aliases: &[],
        description: "pangkas ringan hasil tool lama (keep 16)",
        allow_args: false,
    },
    CommandSpec {
        name: "tan",
        usage: "",
        aliases: &[],
        description: "pangkas hasil tool lama (keep 4)",
        allow_args: false,
    },
    CommandSpec {
        name: "omfg",
        usage: "",
        aliases: &[],
        description: "pangkas semua hasil tool lama",
        allow_args: false,
    },
    CommandSpec {
        name: "cleanse",
        usage: "",
        aliases: &[],
        description: "buang thinking + semua hasil tool lama",
        allow_args: false,
    },
    CommandSpec {
        name: "quit",
        usage: "",
        aliases: &["exit"],
        description: "keluar dari sesi",
        allow_args: false,
    },
];

impl CommandSpec {
    /// Teks pemakaian untuk `/help`, mis. `/title <teks>`.
    pub fn display(&self) -> String {
        if self.usage.is_empty() {
            format!("/{}", self.name)
        } else {
            format!("/{} {}", self.name, self.usage)
        }
    }
}

/// Cari command berdasarkan nama atau alias (`help` / `?`).
pub fn find_command(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))
}

/// Daftar command dalam bentuk teks untuk `/help`.
pub fn help_text() -> String {
    let mut out = String::from("perintah yang tersedia:\n");
    for spec in COMMANDS {
        out.push_str(&format!("  {:<14} {}\n", spec.display(), spec.description));
    }
    out.push_str("selain itu, teks apa pun dikirim ke model.");
    out
}

/// Bagian teratas CHANGELOG (dipotong sebelum rilis kedua) untuk `/changelog`.
pub fn changelog_text() -> String {
    const SEMUA: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../CHANGELOG.md"));
    let bagian: Vec<String> = SEMUA
        .split("## [")
        .skip(1)
        .take(2)
        .map(|s| format!("## [{s}"))
        .collect();
    if bagian.is_empty() {
        return "(CHANGELOG kosong)".to_string();
    }
    bagian.join("\n")
}

/// Teks statistik sesi untuk `/stats`.
pub fn stats_text(jumlah_pesan: usize, usage: tui_wire::Usage) -> String {
    format!(
        "statistik sesi:\n  pesan: {jumlah_pesan}\n  token masuk: {}\n  token keluar: {}",
        usage.input_tokens, usage.output_tokens
    )
}

/// Prompt yang dikirim ke agen untuk `/commit` (approval tetap berjalan).
pub fn commit_prompt() -> String {
    "Buat commit git untuk perubahan yang ada sekarang: jalankan `git add -A` dan \
     `git commit` dengan pesan commit satu baris berbahasa Indonesia yang \
     merangkum perubahan. Jangan mendorong (push) ke remote."
        .to_string()
}
/// Teks pemakaian token untuk UI.
pub fn usage_text(
    usage: tui_wire::Usage,
    context_tokens: u64,
    by_model: &[(String, tui_wire::Usage)],
) -> String {
    let mut text = format!(
        "token: input {} · output {} · cache {} · konteks ~{} (perkiraan)",
        usage.input_tokens, usage.output_tokens, usage.cached_tokens, context_tokens
    );
    let models: Vec<&(String, tui_wire::Usage)> = by_model
        .iter()
        .filter(|(_, model_usage)| !model_usage.is_zero())
        .collect();
    if models.len() > 1 {
        text.push_str("\nper model:");
        for (name, model_usage) in models.iter().take(5) {
            text.push_str(&format!(
                "\n  {name}: input {} · output {}",
                model_usage.input_tokens, model_usage.output_tokens
            ));
        }
    }
    text
}

/// Teks hasil compaction manual untuk UI.
///
/// Ditulis ASCII (`\u{2192}`) supaya berkas tetap aman dari alat yang menulis
/// ulang dengan encoding sistem.
pub fn compaction_text(report: crate::agent::CompactionReport) -> String {
    format!(
        "konteks diringkas: ~{} \u{2192} ~{} token ({} pesan diganti ringkasan)",
        report.before_tokens, report.after_tokens, report.summarized
    )
}

/// Teks pemakaian konteks terhadap anggaran.
pub fn context_text(used: u64, budget: tui_context::Budget) -> String {
    let usable = budget.usable().max(1);
    let percent = used.saturating_mul(100) / usable;
    format!(
        "konteks ~{} dari {} token ({percent}%) · reserve {} · target pemulihan {} (perkiraan)",
        used,
        usable,
        budget.reserve_tokens,
        budget.recovery_target()
    )
}

/// Teks daftar todo sesi.
pub fn todo_text(items: &[String]) -> String {
    if items.is_empty() {
        return "belum ada todo".to_string();
    }
    let mut text = String::from("todo:");
    for (index, item) in items.iter().enumerate() {
        text.push_str(&format!("\n  {}. {item}", index + 1));
    }
    text
}

/// Cocokkan argumen `/model` dengan daftar model yang diketahui.
///
/// Bila daftar kosong (discovery belum pernah berhasil), argumen diterima apa
/// adanya karena tidak ada dasar untuk menolaknya. Bila daftar ada, pencocokan
/// persis (tanpa peduli huruf besar) atau awalan unik yang dipakai; selain itu
/// galat jelas berisi pilihan yang tersedia.
pub fn resolve_model(argument: &str, known: &[String]) -> Result<String, String> {
    if known.is_empty() {
        return Ok(argument.to_string());
    }
    let lowered = argument.to_ascii_lowercase();
    if let Some(exact) = known
        .iter()
        .find(|model| model.to_ascii_lowercase() == lowered)
    {
        return Ok(exact.clone());
    }
    let matches: Vec<&String> = known
        .iter()
        .filter(|model| model.to_ascii_lowercase().contains(&lowered))
        .collect();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(format!(
            "model '{argument}' tidak ada di daftar gateway; pilihan: {}",
            known.join(", ")
        )),
        several => Err(format!(
            "'{argument}' cocok dengan beberapa model ({}); pakai id yang lebih panjang",
            several
                .iter()
                .take(5)
                .map(|model| model.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Jalankan satu baris input.
pub fn dispatch(input: &str, ctx: &CommandContext<'_>) -> CommandResult {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return CommandResult::NotACommand;
    };
    let mut parts = rest.splitn(2, char::is_whitespace);
    let raw_name = parts.next().unwrap_or_default().to_ascii_lowercase();
    let argument = parts.next().unwrap_or_default().trim().to_string();
    let spec = find_command(&raw_name);
    if let Some(spec) = spec
        && !spec.allow_args
        && !argument.is_empty()
    {
        return CommandResult::Reply(format!("{} tidak menerima argumen", spec.display()));
    }
    // Alias dipetakan ke nama kanonik supaya perilakunya benar-benar satu.
    let name = spec.map_or(raw_name.clone(), |spec| spec.name.to_string());
    match name.as_str() {
        "provider" => {
            if argument.is_empty() {
                let mut text = format!(
                    "provider aktif: {} (kunci: {})",
                    ctx.provider.0, ctx.provider.1
                );
                text.push_str("\ntersedia:");
                for (id, label, bawaan) in tui_wire::providers::listing() {
                    let tanda = if bawaan { " [bawaan]" } else { "" };
                    text.push_str(&format!("\n  {id} — {label}{tanda}"));
                }
                CommandResult::Reply(text)
            } else {
                match tui_wire::providers::by_id(&argument) {
                    Some(entry) => CommandResult::SetProvider(Some(entry.id)),
                    None => {
                        let tersedia: Vec<String> = tui_wire::providers::all()
                            .iter()
                            .map(|p| p.id.clone())
                            .collect();
                        CommandResult::Reply(format!(
                            "provider '{argument}' tidak ada di registry; pilihan: {}",
                            tersedia.join(", ")
                        ))
                    }
                }
            }
        }
        "help" => CommandResult::Reply(help_text()),
        "clear" | "cls" => CommandResult::ClearScreen,
        "skills" => CommandResult::Reply(if ctx.skills_ringkas.is_empty() {
            "tidak ada skill aktif (buat folder skills/<nama>/SKILL.md)".to_string()
        } else {
            ctx.skills_ringkas.to_string()
        }),
        "effort" => match argument.as_str() {
            "" => CommandResult::Reply(match ctx.effort {
                Some(e) => format!("effort aktif: {e}"),
                None => "effort: default gateway".to_string(),
            }),
            "low" | "medium" | "high" => CommandResult::SetEffort(Some(argument)),
            lain => CommandResult::Reply(format!(
                "effort '{lain}' tidak dikenal; pilihan: low, medium, high (atau kosong = default)"
            )),
        },
        "extended-context" | "extctx" => match argument.as_str() {
            "on" => CommandResult::SetContextWindow(Some(400_000)),
            "off" => CommandResult::SetContextWindow(ctx.context_window),
            "" => CommandResult::Reply(match ctx.context_window {
                Some(w) => format!(
                    "window konteks config: {w} token (/extended-context on = pakai 400.000)"
                ),
                None => "window konteks tidak dibatasi config".to_string(),
            }),
            lain => CommandResult::Reply(format!("argumen '{lain}' tidak dikenal; pakai on / off")),
        },
        "force" | "yolo" => CommandResult::ForceOnce,
        "stats" => CommandResult::Stats,
        "changelog" => CommandResult::Changelog,
        "setup" => CommandResult::Setup,
        "settings" => CommandResult::Settings,
        "logout" => CommandResult::Logout,
        "mcp" => match argument.split_once(char::is_whitespace) {
            Some(("remove", id)) if !id.trim().is_empty() => {
                CommandResult::McpRemove(id.trim().to_string())
            }
            Some(("reauth", id)) if !id.trim().is_empty() => CommandResult::Feature {
                name: "mcp-reauth".to_string(),
                argument: Some(id.trim().to_string()),
            },
            Some(("add", sisa)) if !sisa.trim().is_empty() => CommandResult::Feature {
                name: "mcp-add".to_string(),
                argument: Some(sisa.trim().to_string()),
            },
            Some(("test", id)) if !id.trim().is_empty() => CommandResult::Feature {
                name: "mcp-test".to_string(),
                argument: Some(id.trim().to_string()),
            },
            Some(("enable", id)) if !id.trim().is_empty() => CommandResult::Feature {
                name: "mcp-enable".to_string(),
                argument: Some(id.trim().to_string()),
            },
            Some(("disable", id)) if !id.trim().is_empty() => CommandResult::Feature {
                name: "mcp-disable".to_string(),
                argument: Some(id.trim().to_string()),
            },
            Some(("list", _)) | None => CommandResult::McpList,
            _ => CommandResult::Reply(
                "pakai: /mcp list|add|remove|enable|disable|test|reauth <id>".to_string(),
            ),
        },
        "export" => {
            if argument == "anon" {
                CommandResult::Feature {
                    name: "export-anon".to_string(),
                    argument: None,
                }
            } else if argument == "zip" {
                CommandResult::Feature {
                    name: "export-zip".to_string(),
                    argument: None,
                }
            } else {
                CommandResult::Export(if argument.is_empty() {
                    None
                } else {
                    Some(argument)
                })
            }
        }
        "handoff" => CommandResult::Handoff,
        "commit" => CommandResult::Commit,
        "model" => {
            if argument.is_empty() {
                let mut text = format!("model aktif: {}", ctx.model);
                if ctx.known_models.is_empty() {
                    text.push_str("\n(daftar model belum diambil dari gateway)");
                } else {
                    text.push_str("\ntersedia: ");
                    text.push_str(&ctx.known_models.join(", "));
                }
                CommandResult::Reply(text)
            } else {
                match resolve_model(&argument, ctx.known_models) {
                    Ok(model) => CommandResult::SetModel(model),
                    Err(message) => CommandResult::Reply(message),
                }
            }
        }
        "compact" => CommandResult::Compact,
        "usage" => CommandResult::Usage,
        "sessions" => CommandResult::Sessions(if argument.is_empty() {
            None
        } else {
            Some(argument)
        }),
        "tree" => CommandResult::Tree,
        "new" => CommandResult::NewSession,
        "fork" => match argument.as_str() {
            "" => CommandResult::ForkSession,
            "pane" | "window" | "tab" => CommandResult::Feature {
                name: "branch".to_string(),
                argument: Some(argument),
            },
            lain => CommandResult::Reply(format!(
                "argumen fork '{lain}' tidak dikenal; pakai pane, window, atau tab"
            )),
        },
        "context" => CommandResult::Context,
        "todo" => {
            if argument.is_empty() {
                CommandResult::Todo
            } else {
                let (sub, sisa) = argument
                    .split_once(char::is_whitespace)
                    .unwrap_or((argument.as_str(), ""));
                match sub {
                    "add" if !sisa.trim().is_empty() => {
                        CommandResult::TodoSub('a', Some(sisa.trim().to_string()))
                    }
                    "start" | "drop" => match sisa.trim().parse::<usize>() {
                        Ok(n) if n >= 1 => CommandResult::TodoSub(
                            if sub == "start" { 's' } else { 'x' },
                            Some(n.to_string()),
                        ),
                        _ => CommandResult::Reply(format!("pakai /todo {sub} <nomor-dari-daftar>")),
                    },
                    "append" => match sisa.trim().split_once(char::is_whitespace) {
                        Some((nomor, teks)) if !teks.trim().is_empty() => {
                            match nomor.parse::<usize>() {
                                Ok(n) if n >= 1 => CommandResult::TodoSub(
                                    'p',
                                    Some(format!("{n} {}", teks.trim())),
                                ),
                                _ => CommandResult::Reply(
                                    "pakai /todo append <nomor> <teks>".to_string(),
                                ),
                            }
                        }
                        _ => CommandResult::Reply("pakai /todo append <nomor> <teks>".to_string()),
                    },
                    "rm" | "done" => match sisa.trim().parse::<usize>() {
                        Ok(n) if n >= 1 => CommandResult::TodoSub(
                            if sub == "rm" { 'r' } else { 'd' },
                            Some(n.to_string()),
                        ),
                        _ => CommandResult::Reply(format!("pakai /todo {sub} <nomor-dari-daftar>")),
                    },
                    "clear" => CommandResult::TodoSub('c', None),
                    "export" => CommandResult::Feature {
                        name: "todo-export".to_string(),
                        argument: (!sisa.trim().is_empty()).then(|| sisa.trim().to_string()),
                    },
                    "import" => CommandResult::Feature {
                        name: "todo-import".to_string(),
                        argument: (!sisa.trim().is_empty()).then(|| sisa.trim().to_string()),
                    },
                    "copy" => CommandResult::Feature {
                        name: "todo-copy".to_string(),
                        argument: None,
                    },
                    lain => CommandResult::Reply(format!(
                        "subcommand todo '{lain}' tidak dikenal; pakai: add/append/start/done/drop/rm/clear (atau /todo saja)"
                    )),
                }
            }
        }
        "title" => {
            if argument.is_empty() {
                CommandResult::Reply("pakai /title <teks>".to_string())
            } else {
                CommandResult::SetTitle(argument)
            }
        }
        "switch" => CommandResult::SwitchSession(if argument.is_empty() {
            None
        } else {
            Some(argument)
        }),
        "forget-approvals" => CommandResult::ForgetApprovals,
        "pause" => CommandResult::Pause,
        "resume" => CommandResult::Resume,
        "tools" => {
            if ctx.tools.is_empty() {
                CommandResult::Reply("belum ada tool terdaftar".to_string())
            } else {
                CommandResult::Reply(ctx.tools.join(", "))
            }
        }
        "session" => CommandResult::Reply(match ctx.session_path {
            Some(path) => path.display().to_string(),
            None => "belum ada file sesi".to_string(),
        }),
        "quit" => CommandResult::Quit,
        "notifications" => {
            let arg = argument.as_str();
            let (sub, isi) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
            let isi = isi.trim();
            match sub {
                "list" => CommandResult::Feature {
                    name: "notifications-list".to_string(),
                    argument: None,
                },
                "clear" => CommandResult::Feature {
                    name: "notifications-clear".to_string(),
                    argument: None,
                },
                "add" if !isi.is_empty() => CommandResult::Feature {
                    name: "notifications-add".to_string(),
                    argument: Some(isi.to_string()),
                },
                _ => CommandResult::Reply("pakai /notifications add <teks>|list|clear".to_string()),
            }
        }
        "annotate" => {
            let arg = argument.as_str();
            let (sub, isi) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
            let isi = isi.trim();
            match sub {
                "list" => CommandResult::Feature {
                    name: "annotate-list".to_string(),
                    argument: None,
                },
                "clear" => CommandResult::Feature {
                    name: "annotate-clear".to_string(),
                    argument: None,
                },
                "add" if !isi.is_empty() => CommandResult::Feature {
                    name: "annotate-add".to_string(),
                    argument: Some(isi.to_string()),
                },
                _ => CommandResult::Reply("pakai /annotate add <teks>|list|clear".to_string()),
            }
        }
        "checkpoint" if argument.trim() == "list" => CommandResult::Feature {
            name: "checkpoint-list".to_string(),
            argument: None,
        },
        "context-note" => {
            let arg = argument.as_str();
            let (sub, isi) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
            let isi = isi.trim();
            match sub {
                "list" => CommandResult::Feature {
                    name: "context-note-list".to_string(),
                    argument: None,
                },
                "clear" => CommandResult::Feature {
                    name: "context-note-clear".to_string(),
                    argument: None,
                },
                "add" if !isi.is_empty() => CommandResult::Feature {
                    name: "context-note-add".to_string(),
                    argument: Some(isi.to_string()),
                },
                _ => CommandResult::Reply("pakai /context-note add <teks>|list|clear".to_string()),
            }
        }
        other if FEATURE_NAMES.contains(&other) => CommandResult::Feature {
            name: other.to_string(),
            argument: (!argument.is_empty()).then_some(argument),
        },
        other => {
            let names: Vec<String> = COMMANDS.iter().map(CommandSpec::display).collect();
            CommandResult::Unknown(format!(
                "perintah tidak dikenal: /{other}. Tersedia: {}",
                names.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx<'a>(
        model: &'a str,
        tools: &'a [String],
        session: Option<&'a Path>,
    ) -> CommandContext<'a> {
        CommandContext {
            model,
            provider: ("modeluxai", "MODELUXAI_API_KEY"),
            tools,
            session_path: session,
            known_models: &[],
            sessions_dir: None,
            skills_ringkas: "",
            context_window: None,
            effort: None,
        }
    }

    #[test]
    fn teks_biasa_bukan_command() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("halo dunia", &ctx("m", &tools, None)),
            CommandResult::NotACommand
        );
    }

    #[test]
    fn help_menyebut_semua_command() {
        let tools = Vec::new();
        match dispatch("/help", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                for spec in COMMANDS {
                    let display = spec.display();
                    assert!(text.contains(&display), "help tidak menyebut {display}");
                }
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn model_tanpa_argumen_menampilkan_yang_aktif() {
        let tools = Vec::new();
        match dispatch("/model", &ctx("deepseek-v4.1-flash", &tools, None)) {
            CommandResult::Reply(text) => assert!(text.contains("deepseek-v4.1-flash")),
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn model_dengan_argumen_mengganti() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("  /model  glm-5-air ", &ctx("m", &tools, None)),
            CommandResult::SetModel("glm-5-air".to_string())
        );
    }

    #[test]
    fn tools_dan_session_membaca_konteks() {
        let tools = vec!["read".to_string(), "bash".to_string()];
        match dispatch("/tools", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => assert_eq!(text, "read, bash"),
            other => panic!("dapat {other:?}"),
        }
        let path = PathBuf::from("sesi.jsonl");
        match dispatch("/session", &ctx("m", &tools, Some(&path))) {
            CommandResult::Reply(text) => assert!(text.ends_with("sesi.jsonl")),
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn compact_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/compact", &ctx("m", &tools, None)),
            CommandResult::Compact
        );
    }

    #[test]
    fn compaction_menyebut_token_dan_jumlah_pesan() {
        let text = compaction_text(crate::agent::CompactionReport {
            before_tokens: 12_000,
            after_tokens: 3_500,
            summarized: 8,
        });
        assert!(text.contains("12000"), "{text}");
        assert!(text.contains("3500"), "{text}");
        assert!(text.contains("8 pesan"), "{text}");
        assert!(
            text.contains("\u{2192}"),
            "panah ditulis sebagai escape: {text}"
        );
    }

    #[test]
    fn usage_dikenali_dan_diformat() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/usage", &ctx("m", &tools, None)),
            CommandResult::Usage
        );
        let usage = tui_wire::Usage {
            input_tokens: 120,
            output_tokens: 30,
            cached_tokens: 40,
        };
        let text = usage_text(usage, 500, &[]);
        assert!(text.contains("input 120"), "{text}");
        assert!(text.contains("output 30"), "{text}");
        assert!(text.contains("cache 40"), "{text}");
        assert!(text.contains("~500"), "{text}");
        assert!(
            !text.contains("per model"),
            "satu model tidak perlu rincian"
        );
    }

    #[test]
    fn usage_merinci_per_model_bila_lebih_dari_satu() {
        let total = tui_wire::Usage {
            input_tokens: 15,
            output_tokens: 9,
            cached_tokens: 0,
        };
        let by_model = vec![
            (
                "model-a".to_string(),
                tui_wire::Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cached_tokens: 0,
                },
            ),
            (
                "model-b".to_string(),
                tui_wire::Usage {
                    input_tokens: 5,
                    output_tokens: 4,
                    cached_tokens: 0,
                },
            ),
        ];
        let text = usage_text(total, 900, &by_model);
        assert!(text.contains("per model"), "{text}");
        assert!(text.contains("model-a: input 10"), "{text}");
        assert!(text.contains("model-b: input 5"), "{text}");
    }

    fn ctx_models<'a>(known: &'a [String]) -> CommandContext<'a> {
        CommandContext {
            model: "aktif",
            provider: ("modeluxai", "MODELUXAI_API_KEY"),
            tools: &[],
            session_path: None,
            known_models: known,
            sessions_dir: None,
            skills_ringkas: "",
            context_window: None,
            effort: None,
        }
    }

    #[test]
    fn model_tanpa_arg_menampilkan_daftar_bila_ada() {
        let known = vec!["model-a".to_string(), "model-b".to_string()];
        match dispatch("/model", &ctx_models(&known)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("model aktif: aktif"), "{text}");
                assert!(text.contains("model-a"), "{text}");
                assert!(text.contains("model-b"), "{text}");
            }
            other => panic!("harus Reply, dapat {other:?}"),
        }
        match dispatch("/model", &ctx_models(&[])) {
            CommandResult::Reply(text) => {
                assert!(text.contains("belum diambil dari gateway"), "{text}");
            }
            other => panic!("harus Reply, dapat {other:?}"),
        }
    }

    #[test]
    fn model_tidak_dikenal_ditolak_tanpa_menebak() {
        let known = vec!["deepseek-v4.1-flash".to_string()];
        assert_eq!(
            resolve_model("deepseek-v4.1-flash", &known),
            Ok("deepseek-v4.1-flash".to_string())
        );
        assert_eq!(
            resolve_model("DEEPSEEK-v4.1-FLASH", &known),
            Ok("deepseek-v4.1-flash".to_string())
        );
        assert!(
            resolve_model("v4.1", &known).is_ok(),
            "awalan unik diterima"
        );
        let err = resolve_model("model-tak-ada", &known).expect_err("harus ditolak");
        assert!(err.contains("tidak ada di daftar gateway"), "{err}");
        assert!(err.contains("deepseek-v4.1-flash"), "{err}");
        assert_eq!(resolve_model("apa-saja", &[]), Ok("apa-saja".to_string()));
    }

    #[test]
    fn model_tidak_dikenal_lewat_dispatch_tidak_mengganti() {
        let known = vec!["deepseek-v4.1-flash".to_string()];
        match dispatch("/model salah", &ctx_models(&known)) {
            CommandResult::Reply(text) => assert!(text.contains("tidak ada"), "{text}"),
            other => panic!("tidak boleh SetModel: {other:?}"),
        }
        assert_eq!(
            dispatch("/model deepseek-v4.1-flash", &ctx_models(&known)),
            CommandResult::SetModel("deepseek-v4.1-flash".to_string())
        );
    }

    #[test]
    fn registry_memiliki_atribut_dan_alias() {
        let help = find_command("help").expect("help ada");
        assert!(help.aliases.contains(&"?"));
        let quit = find_command("exit").expect("alias exit");
        assert_eq!(quit.name, "quit");
        assert!(quit.aliases.contains(&"exit"));
        let model = find_command("model").expect("model ada");
        assert!(model.allow_args);
        assert_eq!(model.display(), "/model [id]");
        let compact = find_command("compact").expect("compact ada");
        assert!(!compact.allow_args);
        assert_eq!(COMMANDS.len(), 107, "jumlah command harus cocok dokumen");
        for spec in COMMANDS {
            assert!(
                !spec.description.is_empty(),
                "{} tanpa deskripsi",
                spec.name
            );
        }
    }

    #[test]
    fn command_tanpa_argumen_menolak_argumen() {
        let tools = Vec::new();
        match dispatch("/compact sekarang", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("tidak menerima argumen"), "{text}")
            }
            other => panic!("tidak boleh eksekusi: {other:?}"),
        }
        match dispatch("/exit", &ctx("m", &tools, None)) {
            CommandResult::Quit => {}
            other => panic!("alias exit harus keluar: {other:?}"),
        }
        match dispatch("/?", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("perintah yang tersedia"), "{text}")
            }
            other => panic!("alias ? harus menampilkan bantuan: {other:?}"),
        }
    }

    #[test]
    fn provider_tanpa_argumen_menampilkan_daftar() {
        let tools = Vec::new();
        match dispatch("/provider", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("provider aktif: modeluxai"), "{text}");
                assert!(text.contains("MODELUXAI_API_KEY"), "{text}");
                assert!(text.contains("openrouter"), "{text}");
                assert!(text.contains("[bawaan]"), "{text}");
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn provider_dengan_argumen_dikenal_menghasilkan_set() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/provider groq", &ctx("m", &tools, None)),
            CommandResult::SetProvider(Some("groq".to_string()))
        );
        match dispatch("/provider bukan-ada", &ctx("m", &tools, None)) {
            CommandResult::Reply(text) => {
                assert!(text.contains("tidak ada di registry"), "{text}");
                assert!(text.contains("modeluxai"), "{text}");
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn forget_approvals_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/forget-approvals", &ctx("m", &tools, None)),
            CommandResult::ForgetApprovals
        );
    }

    #[test]
    fn new_dan_fork_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/new", &ctx("m", &tools, None)),
            CommandResult::NewSession
        );
        assert_eq!(
            dispatch("/fork", &ctx("m", &tools, None)),
            CommandResult::ForkSession
        );
    }

    #[test]
    fn context_dan_todo_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/context", &ctx("m", &tools, None)),
            CommandResult::Context
        );
        assert_eq!(
            dispatch("/todo", &ctx("m", &tools, None)),
            CommandResult::Todo
        );
    }

    #[test]
    fn context_dan_todo_diformat() {
        let budget = tui_context::Budget::new(100_000, 16_000, 20_000);
        let text = context_text(45_000, budget);
        assert!(text.contains("45000"), "{text}");
        assert!(text.contains("84000"), "{text}");
        assert!(text.contains("53%"), "{text}");
        assert!(text.contains("target pemulihan 67200"), "{text}");

        assert_eq!(todo_text(&[]), "belum ada todo");
        let text = todo_text(&["satu".to_string(), "dua".to_string()]);
        assert!(text.contains("1. satu"), "{text}");
        assert!(text.contains("2. dua"), "{text}");
    }

    #[test]
    fn title_sesi_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/title kerja malam", &ctx("m", &tools, None)),
            CommandResult::SetTitle("kerja malam".to_string())
        );
        assert_eq!(
            dispatch("/title", &ctx("m", &tools, None)),
            CommandResult::Reply("pakai /title <teks>".to_string())
        );
    }

    #[test]
    fn switch_sesi_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/switch", &ctx("m", &tools, None)),
            CommandResult::SwitchSession(None)
        );
        assert_eq!(
            dispatch("/switch abc123", &ctx("m", &tools, None)),
            CommandResult::SwitchSession(Some("abc123".to_string()))
        );
        assert_eq!(
            dispatch("/resume", &ctx("m", &tools, None)),
            CommandResult::Resume,
            "/resume tetap untuk melanjutkan sesi yang dijeda"
        );
    }

    #[test]
    fn sessions_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/sessions", &ctx("m", &tools, None)),
            CommandResult::Sessions(None)
        );
        assert_eq!(
            dispatch("/sessions catatan", &ctx("m", &tools, None)),
            CommandResult::Sessions(Some("catatan".to_string()))
        );
    }

    #[test]
    fn pause_dan_resume_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/pause", &ctx("m", &tools, None)),
            CommandResult::Pause
        );
        assert_eq!(
            dispatch("/resume", &ctx("m", &tools, None)),
            CommandResult::Resume
        );
    }

    #[test]
    fn clear_skills_effort_extctx_force_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/clear", &ctx("m", &tools, None)),
            CommandResult::ClearScreen
        );
        assert_eq!(
            dispatch("/cls", &ctx("m", &tools, None)),
            CommandResult::ClearScreen
        );
        assert_eq!(
            dispatch("/skills", &ctx("m", &tools, None)),
            CommandResult::Reply(
                "tidak ada skill aktif (buat folder skills/<nama>/SKILL.md)".to_string()
            )
        );
        assert_eq!(
            dispatch("/effort", &ctx("m", &tools, None)),
            CommandResult::Reply("effort: default gateway".to_string())
        );
        assert_eq!(
            dispatch("/effort high", &ctx("m", &tools, None)),
            CommandResult::SetEffort(Some("high".to_string()))
        );
        assert!(
            matches!(
                dispatch("/effort gila", &ctx("m", &tools, None)),
                CommandResult::Reply(_)
            ),
            "effort tak dikenal = pesan, bukan set"
        );
        assert_eq!(
            dispatch("/extended-context on", &ctx("m", &tools, None)),
            CommandResult::SetContextWindow(Some(400_000))
        );
        assert_eq!(
            dispatch("/extctx off", &ctx("m", &tools, None)),
            CommandResult::SetContextWindow(None)
        );
        assert_eq!(
            dispatch("/force", &ctx("m", &tools, None)),
            CommandResult::ForceOnce
        );
        assert_eq!(
            dispatch("/yolo", &ctx("m", &tools, None)),
            CommandResult::ForceOnce
        );
    }

    #[test]
    fn kelompok2_command_dikenali() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/stats", &ctx("m", &tools, None)),
            CommandResult::Stats
        );
        assert_eq!(
            dispatch("/changelog", &ctx("m", &tools, None)),
            CommandResult::Changelog
        );
        assert_eq!(
            dispatch("/setup", &ctx("m", &tools, None)),
            CommandResult::Setup
        );
        assert_eq!(
            dispatch("/settings", &ctx("m", &tools, None)),
            CommandResult::Settings
        );
        assert_eq!(
            dispatch("/logout", &ctx("m", &tools, None)),
            CommandResult::Logout
        );
        assert_eq!(
            dispatch("/mcp", &ctx("m", &tools, None)),
            CommandResult::McpList
        );
        assert_eq!(
            dispatch("/mcp list", &ctx("m", &tools, None)),
            CommandResult::McpList
        );
        assert_eq!(
            dispatch("/mcp remove klien-a", &ctx("m", &tools, None)),
            CommandResult::McpRemove("klien-a".to_string())
        );
        assert!(matches!(
            dispatch("/mcp hapus klien-a", &ctx("m", &tools, None)),
            CommandResult::Reply(_)
        ));
        assert_eq!(
            dispatch("/export", &ctx("m", &tools, None)),
            CommandResult::Export(None)
        );
        assert_eq!(
            dispatch("/export catatan.md", &ctx("m", &tools, None)),
            CommandResult::Export(Some("catatan.md".to_string()))
        );
        assert_eq!(
            dispatch("/handoff", &ctx("m", &tools, None)),
            CommandResult::Handoff
        );
        assert_eq!(
            dispatch("/commit", &ctx("m", &tools, None)),
            CommandResult::Commit
        );
    }

    #[test]
    fn todo_subcommand_dipetakan() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/todo", &ctx("m", &tools, None)),
            CommandResult::Todo
        );
        assert_eq!(
            dispatch("/todo add kerja baru", &ctx("m", &tools, None)),
            CommandResult::TodoSub('a', Some("kerja baru".to_string()))
        );
        assert_eq!(
            dispatch("/todo rm 2", &ctx("m", &tools, None)),
            CommandResult::TodoSub('r', Some("2".to_string()))
        );
        assert_eq!(
            dispatch("/todo done 1", &ctx("m", &tools, None)),
            CommandResult::TodoSub('d', Some("1".to_string()))
        );
        assert_eq!(
            dispatch("/todo clear", &ctx("m", &tools, None)),
            CommandResult::TodoSub('c', None)
        );
        assert!(matches!(
            dispatch("/todo gila", &ctx("m", &tools, None)),
            CommandResult::Reply(_)
        ));
    }

    #[test]
    fn feature_commands_menjadi_feature() {
        let tools = Vec::new();
        for name in [
            "fresh",
            "pin",
            "unpin",
            "jobs",
            "git",
            "local",
            "browser",
            "copy",
            "open",
            "trace",
            "dump",
            "debug",
            "elide",
            "shake",
            "modelpreset",
            "skillful",
            "collab",
            "memory",
            "mm",
            "marketplace",
            "plugins",
            "ssh",
            "advisor",
            "computer",
            "branch",
            "fast",
            "slow",
            "live",
            "record",
        ] {
            match dispatch(&format!("/{name}"), &ctx("m", &tools, None)) {
                CommandResult::Feature { name: hasil, .. } => {
                    assert_eq!(hasil, name, "fitur {name}");
                }
                other => panic!("{name} menghasilkan {other:?}"),
            }
        }
        assert!(matches!(
            dispatch("/mcp reauth server", &ctx("m", &tools, None)),
            CommandResult::Feature { name, argument }
                if name == "mcp-reauth" && argument.as_deref() == Some("server")
        ));
        assert!(matches!(
            dispatch("/fork pane", &ctx("m", &tools, None)),
            CommandResult::Feature { name, argument }
                if name == "branch" && argument.as_deref() == Some("pane")
        ));
    }

    #[test]
    fn quit_dan_unknown() {
        let tools = Vec::new();
        assert_eq!(
            dispatch("/quit", &ctx("m", &tools, None)),
            CommandResult::Quit
        );
        assert_eq!(
            dispatch("/exit", &ctx("m", &tools, None)),
            CommandResult::Quit
        );
        match dispatch("/tidak-ada", &ctx("m", &tools, None)) {
            CommandResult::Unknown(text) => assert!(text.contains("/help")),
            other => panic!("dapat {other:?}"),
        }
    }
}
