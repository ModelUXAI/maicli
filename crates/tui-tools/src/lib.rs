//! Registry tool dan implementasi tool v1.
//!
//! Setiap tool menyatakan tier-nya (read/write/exec) sehingga gerbang approval
//! bisa memutuskan tanpa mengenal nama tool. Tool tidak pernah memutuskan
//! sendiri apakah boleh berjalan.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tui_wire::{Concurrency, ErrorCode, MxError, ToolDefinition, ToolOutput, ToolTier};

pub mod cancel;
pub mod extensions;
pub mod skills;
pub mod tool_meta;

pub use cancel::CancelToken;
pub use tool_meta::agent_tool_meta;

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("cwd", &self.cwd)
            .field("output_cap", &self.output_cap)
            .field("interactive", &self.interactive)
            .field("bash_timeout", &self.bash_timeout)
            .field("bash_max_timeout", &self.bash_max_timeout)
            .field("cancel", &self.cancel)
            .field("ask", &self.ask.is_some())
            .field("uri_bridge", &self.uri_bridge.is_some())
            .finish()
    }
}

/// Konteks eksekusi satu tool.
#[derive(Clone)]
pub struct ToolContext {
    /// Direktori kerja sesi; semua path relatif dihitung dari sini.
    pub cwd: PathBuf,
    /// Cap byte untuk output tool.
    pub output_cap: usize,
    /// Apakah sesi punya antarmuka untuk bertanya ke pengguna.
    pub interactive: bool,
    /// Timeout default perintah shell.
    pub bash_timeout: Duration,
    /// Batas atas timeout perintah shell.
    pub bash_max_timeout: Duration,
    /// Token pembatalan yang dipakai tool berjalan lama.
    pub cancel: CancelToken,
    /// Kanal pertanyaan untuk tool `ask`; `None` = tidak ada UI pertanyaan.
    pub ask: Option<mpsc::UnboundedSender<AskRequest>>,
    /// Jembatan URI host (v2 RPC): `read`/`write` URL skema host dipantulkan ke
    /// host alih-alih filesystem; `None` = tidak ada skema host.
    pub uri_bridge: Option<std::sync::Arc<dyn UriResolver>>,
}

/// Resolver skema URI milik host (dipasang mode RPC v2).
pub trait UriResolver: Send + Sync {
    /// Baca satu URL skema host; `Ok(isi)` atau `Err(pesan)`.
    fn read(
        &self,
        url: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;
    /// Tulis satu URL; `Ok(konfirmasi)` atau `Err(pesan)`.
    fn write(
        &self,
        url: String,
        content: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;
    /// Apakah skema ini milik host.
    fn owns(&self, url: &str) -> bool;
}

/// Satu pertanyaan dari tool `ask` ke UI.
#[derive(Debug)]
pub struct AskRequest {
    /// Pertanyaan yang ditampilkan.
    pub question: String,
    /// Kanal jawaban; kirim teks jawaban, atau drop untuk membatalkan.
    pub reply: oneshot::Sender<String>,
}

/// Batas waktu menunggu jawaban pengguna.
pub const ASK_TIMEOUT_SECS: u64 = 600;

impl ToolContext {
    /// Konteks dengan nilai default RENCANA.md §3.4.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            output_cap: 65_536,
            interactive: false,
            bash_timeout: Duration::from_secs(120),
            bash_max_timeout: Duration::from_secs(600),
            cancel: CancelToken::new(),
            ask: None,
            uri_bridge: None,
        }
    }
}

/// Kontrak satu tool.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Definisi yang dikirim ke model.
    fn definition(&self) -> ToolDefinition;
    /// Tier untuk kebijakan approval.
    fn tier(&self) -> ToolTier;
    /// Aturan konkurensi.
    fn concurrency(&self) -> Concurrency {
        Concurrency::Shared
    }
    /// Jalankan tool. Error yang bisa dijelaskan ke model dikembalikan sebagai
    /// `ToolOutput` error; `MxError` hanya untuk kegagalan infrastruktur.
    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError>;
}

/// Kumpulan tool yang tersedia untuk satu sesi.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
    todos: Arc<Mutex<Vec<String>>>,
}

impl ToolRegistry {
    /// Daftarkan satu tool tambahan (dipakai extension dan test).
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Registry tool v1.
    pub fn v1() -> Self {
        let todos = Arc::new(Mutex::new(Vec::new()));
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(ReadTool),
            Arc::new(WriteTool),
            Arc::new(EditTool),
            Arc::new(GlobTool),
            Arc::new(GrepTool),
            Arc::new(BashTool),
            Arc::new(TodoTool::new(Arc::clone(&todos))),
            Arc::new(AskTool),
        ];
        Self { tools, todos }
    }

    /// Daftar todo saat ini (salinan; kosong bila belum pernah diisi).
    pub fn todos(&self) -> Vec<String> {
        match self.todos.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => Vec::new(),
        }
    }

    /// Definisi semua tool, berurutan.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|tool| tool.definition()).collect()
    }

    /// Registry dari kumpulan tool tertentu (dipakai test dan komposisi kustom).
    pub fn from_tools(tools: Vec<Arc<dyn Tool>>) -> Self {
        Self {
            tools,
            todos: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Cari tool berdasarkan nama.
    /// Ganti tool dengan nama sama, atau daftarkan bila belum ada.
    ///
    /// Dipakai `set_host_tools` RPC: mengirim ulang definisi host mengganti
    /// tool host lama tanpa menyentuh tool lain.
    pub fn replace_named(&mut self, tool: Arc<dyn Tool>) -> bool {
        let nama = tool.definition().name;
        if let Some(slot) = self
            .tools
            .iter_mut()
            .find(|ada| ada.definition().name == nama)
        {
            *slot = tool;
            true
        } else {
            self.tools.push(tool);
            false
        }
    }

    /// Hapus tool dengan nama tertentu; `true` bila ada yang dihapus.
    pub fn remove_named(&mut self, name: &str) -> bool {
        let sebelum = self.tools.len();
        self.tools.retain(|tool| tool.definition().name != name);
        self.tools.len() != sebelum
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .iter()
            .find(|tool| tool.definition().name == name)
            .cloned()
    }

    /// Nama semua tool yang terdaftar.
    pub fn names(&self) -> Vec<String> {
        self.tools
            .iter()
            .map(|tool| tool.definition().name)
            .collect()
    }
}

/// Tool yang tidak dikenal.
pub fn unknown_tool(name: &str) -> MxError {
    MxError::new(ErrorCode::ToolFailed, format!("tool tidak dikenal: {name}"))
}

fn tool_error(message: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::ToolFailed, message.into())
}

/// Selesaikan path relatif terhadap cwd dan tolak keluar dari cwd.
pub fn resolve_in_cwd(cwd: &Path, raw: &str) -> Result<PathBuf, MxError> {
    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        if candidate.starts_with(cwd) {
            pastikan_tidak_keluar(cwd, candidate, raw)?;
            return Ok(candidate.to_path_buf());
        }
        return Err(tool_error(format!("path di luar direktori kerja: {raw}")));
    }
    for component in candidate.components() {
        if matches!(component, Component::ParentDir) {
            return Err(tool_error(format!("path traversal ditolak: {raw}")));
        }
    }
    let joined = cwd.join(candidate);
    pastikan_tidak_keluar(cwd, &joined, raw)?;
    Ok(joined)
}

/// Pastikan path nyata (setelah symlink/junction) tetap di dalam cwd.
///
/// Pemeriksaan komponen saja tidak cukup: symlink/junction di dalam cwd bisa
/// menunjuk ke luar. Bagian yang belum ada (mis. berkas baru) diperiksa lewat
/// leluhur terdekat yang ada lalu sisanya ditempelkan tanpa resolusi.
fn pastikan_tidak_keluar(cwd: &Path, joined: &Path, raw: &str) -> Result<(), MxError> {
    let cwd_nyata = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut leluhur = joined.to_path_buf();
    let mut sisa: Vec<std::ffi::OsString> = Vec::new();
    while !leluhur.exists() {
        let Some(nama) = leluhur.file_name().map(|n| n.to_os_string()) else {
            return Err(tool_error(format!("path traversal ditolak: {raw}")));
        };
        sisa.push(nama);
        let Some(atas) = leluhur.parent() else {
            return Err(tool_error(format!("path traversal ditolak: {raw}")));
        };
        leluhur = atas.to_path_buf();
    }
    let mut nyata = leluhur
        .canonicalize()
        .map_err(|e| tool_error(format!("path tidak bisa dipastikan: {e}")))?;
    for bagian in sisa.iter().rev() {
        nyata.push(bagian);
    }
    if !nyata.starts_with(&cwd_nyata) {
        return Err(tool_error(format!(
            "path traversal ditolak (keluar direktori kerja): {raw}"
        )));
    }
    Ok(())
}

fn truncate_bytes(text: &str, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text.to_string(), false);
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text.get(..end).unwrap_or_default().to_string();
    out.push_str("\n…[dipotong]");
    (out, true)
}

// ---------------------------------------------------------------- read

struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read".into(),
            description: "Membaca isi berkas teks dengan nomor baris; bisa dibatasi offset/limit."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relatif terhadap direktori kerja" },
                    "offset": { "type": "integer", "description": "Baris awal (1-based), default 1" },
                    "limit": { "type": "integer", "description": "Jumlah baris maksimum, default semua" }
                },
                "required": ["path"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let raw = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen path wajib"))?;
        // URL skema host (mis. db://users/42) dipantulkan ke host (RPC v2).
        if let Some(bridge) = &ctx.uri_bridge
            && bridge.owns(raw)
        {
            let isi = bridge.read(raw.to_string()).await;
            return Ok(match isi {
                Ok(isi) => ToolOutput::ok(isi),
                Err(pesan) => ToolOutput::err(pesan),
            });
        }
        let path = resolve_in_cwd(&ctx.cwd, raw)?;
        let bytes = std::fs::read(&path)
            .map_err(|e| tool_error(format!("gagal membaca {}: {e}", path.display())))?;
        if bytes.iter().take(8_192).any(|byte| *byte == 0) {
            return Ok(ToolOutput::err(format!(
                "{} tampak biner (mengandung byte NUL); tool read hanya untuk berkas teks",
                path.display()
            )));
        }
        let text = String::from_utf8_lossy(&bytes);
        let offset = args
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as usize;
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|v| v.max(1) as usize);
        let mut numbered = String::new();
        let mut shown = 0usize;
        let total = text.lines().count();
        for (index, line) in text.lines().enumerate() {
            let line_number = index + 1;
            if line_number < offset {
                continue;
            }
            if let Some(limit) = limit
                && shown >= limit
            {
                break;
            }
            numbered.push_str(&format!("{line_number}\t{line}\n"));
            shown += 1;
        }
        if shown == 0 {
            return Ok(ToolOutput::ok(format!(
                "[tidak ada baris pada offset {offset}; berkas punya {total} baris]"
            )));
        }
        let (content, truncated) = truncate_bytes(&numbered, ctx.output_cap);
        Ok(ToolOutput::ok(content).truncated(truncated))
    }
}

// ---------------------------------------------------------------- write

struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write".into(),
            description: "Menulis berkas baru atau mengganti isi berkas.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let raw = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen path wajib"))?;
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen content wajib"))?;
        // URL skema host writable dipantulkan ke host (RPC v2).
        if let Some(bridge) = &ctx.uri_bridge
            && bridge.owns(raw)
        {
            let hasil = bridge.write(raw.to_string(), content.to_string()).await;
            return Ok(match hasil {
                Ok(konfirmasi) => ToolOutput::ok(konfirmasi),
                Err(pesan) => ToolOutput::err(pesan),
            });
        }
        let path = resolve_in_cwd(&ctx.cwd, raw)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                tool_error(format!("gagal membuat direktori {}: {e}", parent.display()))
            })?;
        }
        std::fs::write(&path, content.as_bytes())
            .map_err(|e| tool_error(format!("gagal menulis {}: {e}", path.display())))?;
        Ok(ToolOutput::ok(format!(
            "menulis {} byte ke {}",
            content.len(),
            path.display()
        )))
    }
}

// ---------------------------------------------------------------- edit

struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "edit".into(),
            description: "Mengganti teks tertentu di satu berkas; teks lama harus cocok persis dan unik kecuali replace_all.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_string": { "type": "string", "description": "Teks yang dicari, harus unik kecuali replace_all" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean", "description": "Ganti semua kemunculan" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let raw = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen path wajib"))?;
        let old = args
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen old_string wajib"))?;
        let new = args
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen new_string wajib"))?;
        let replace_all = args
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if old.is_empty() {
            return Ok(ToolOutput::err("old_string tidak boleh kosong"));
        }
        let path = resolve_in_cwd(&ctx.cwd, raw)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| tool_error(format!("gagal membaca {}: {e}", path.display())))?;
        let matches = text.matches(old).count();
        if matches == 0 {
            return Ok(ToolOutput::err(format!(
                "teks lama tidak ditemukan di {} (0 kecocokan)",
                path.display()
            )));
        }
        if matches > 1 && !replace_all {
            return Ok(ToolOutput::err(format!(
                "teks lama ditemukan {matches} kali di {}; perjelas old_string atau pakai replace_all=true",
                path.display()
            )));
        }
        let updated = if replace_all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        std::fs::write(&path, updated.as_bytes())
            .map_err(|e| tool_error(format!("gagal menulis {}: {e}", path.display())))?;
        Ok(ToolOutput::ok(format!(
            "mengganti {matches} kemunculan di {} ({} byte)",
            path.display(),
            updated.len()
        )))
    }
}

// ---------------------------------------------------------------- glob

struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "glob".into(),
            description: "Mencari berkas berdasarkan pola glob.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "pattern": { "type": "string", "description": "Contoh: **/*.rs" } },
                "required": ["pattern"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let pattern = args
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen pattern wajib"))?;
        let matcher = globset::Glob::new(pattern)
            .map_err(|e| tool_error(format!("pola tidak valid: {e}")))?
            .compile_matcher();
        let mut found = Vec::new();
        for entry in walkdir::WalkDir::new(&ctx.cwd)
            .max_depth(12)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(&ctx.cwd) else {
                continue;
            };
            let display = relative.to_string_lossy().replace('\\', "/");
            if matcher.is_match(&display) {
                found.push(display);
                if found.len() >= 200 {
                    break;
                }
            }
        }
        found.sort();
        Ok(ToolOutput::ok(found.join("\n")))
    }
}

// ---------------------------------------------------------------- grep

struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".into(),
            description: "Mencari pola regex di isi berkas.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string", "description": "Subdirektori, default direktori kerja" },
                    "max_results": { "type": "integer", "description": "Batas hasil, default 200, maksimum 1000" }
                },
                "required": ["pattern"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let pattern = args
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen pattern wajib"))?;
        let regex = regex::Regex::new(pattern)
            .map_err(|e| tool_error(format!("regex tidak valid: {e}")))?;
        let root = match args.get("path").and_then(Value::as_str) {
            Some(sub) => resolve_in_cwd(&ctx.cwd, sub)?,
            None => ctx.cwd.clone(),
        };
        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|value| value.clamp(1, 1_000) as usize)
            .unwrap_or(200);
        let mut hits = Vec::new();
        'outer: for entry in walkdir::WalkDir::new(&root)
            .max_depth(12)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes.len() > 1_048_576 || bytes.contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            let Ok(relative) = entry.path().strip_prefix(&ctx.cwd) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if regex.is_match(line) {
                    hits.push(format!(
                        "{}:{}:{}",
                        relative.display(),
                        index + 1,
                        line.trim()
                    ));
                    if hits.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }
        if hits.len() >= max_results {
            hits.push(format!(
                "[dipotong pada {max_results} hasil; persempit pola atau path]"
            ));
        }
        let (content, truncated) = truncate_bytes(&hits.join("\n"), ctx.output_cap);
        Ok(ToolOutput::ok(content).truncated(truncated))
    }
}

// ---------------------------------------------------------------- bash

struct BashTool;

enum WaitOutcome<T> {
    Done(T),
    Cancelled,
}

#[async_trait]
impl Tool for BashTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".into(),
            description: "Menjalankan perintah shell dengan timeout dan cap output.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout_ms": { "type": "integer", "description": "Opsional, detik dikali 1000" }
                },
                "required": ["command"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen command wajib"))?;
        let requested = args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map(Duration::from_millis)
            .unwrap_or(ctx.bash_timeout)
            .min(ctx.bash_max_timeout);
        let mut process = if cfg!(windows) {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/C").arg(command);
            cmd
        } else {
            let mut cmd = tokio::process::Command::new("/bin/sh");
            cmd.arg("-c").arg(command);
            cmd
        };
        // POSIX: child jadi leader proses group → kill_tree(-pgid) menghentikan
        // seluruh pohon (sub-shell + utilitas), bukan hanya shell-nya.
        #[cfg(unix)]
        {
            process.process_group(0);
        }
        process
            .current_dir(&ctx.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // Jangan wariskan env yang tampak seperti kredensial ke proses anak:
        // output tool masuk transkrip, jadi kunci yang terbaca perintah akan
        // bocor ke sesi/log.
        for (nama, _) in std::env::vars() {
            if nama_env_rahasia(&nama) {
                process.env_remove(&nama);
            }
        }
        let mut child = process
            .spawn()
            .map_err(|e| tool_error(format!("gagal menjalankan perintah: {e}")))?;
        let pid = child.id();
        let stdout_task = tokio::spawn(read_capped(child.stdout.take(), ctx.output_cap));
        let stderr_task = tokio::spawn(read_capped(child.stderr.take(), ctx.output_cap));
        let cancel = ctx.cancel.clone();
        let waited = tokio::select! {
            outcome = tokio::time::timeout(requested, child.wait()) => WaitOutcome::Done(outcome),
            () = cancel.cancelled() => WaitOutcome::Cancelled,
        };
        let status = match waited {
            WaitOutcome::Cancelled => {
                // Pembatalan dari UI: bunuh pohon, jangan menunggu sisa output.
                kill_tree(pid).await;
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                let (partial, truncated) = collect_partial(stdout_task, stderr_task).await;
                return Ok(ToolOutput::err(format!(
                    "[dibatalkan] perintah dan anak prosesnya dihentikan\n[output parsial]\n{partial}"
                ))
                .truncated(truncated || !partial.is_empty()));
            }
            WaitOutcome::Done(Ok(Ok(status))) => status,
            WaitOutcome::Done(Ok(Err(err))) => {
                return Err(tool_error(format!("gagal menunggu perintah: {err}")));
            }
            WaitOutcome::Done(Err(_)) => {
                // Bunuh pohon SELAGI induk masih hidup: `taskkill /T` menemukan
                // anak proses lewat induknya. Setelah itu kill langsung dan
                // tunggu sebentar agar handle pipe dilepas.
                kill_tree(pid).await;
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                let (partial, truncated) = collect_partial(stdout_task, stderr_task).await;
                return Ok(ToolOutput::err(format!(
                    "[timeout {} detik] perintah dan anak prosesnya dihentikan\n[output parsial]\n{partial}",
                    requested.as_secs()
                ))
                .truncated(truncated || !partial.is_empty()));
            }
        };
        let (stdout, stdout_truncated) = stdout_task.await.unwrap_or_default();
        let (stderr, stderr_truncated) = stderr_task.await.unwrap_or_default();
        let mut text = String::from_utf8_lossy(&stdout).into_owned();
        if !stderr.is_empty() {
            text.push_str("\n[stderr]\n");
            text.push_str(&String::from_utf8_lossy(&stderr));
        }
        let code = status.code().unwrap_or(-1);
        let (body, truncated) = truncate_bytes(&text, ctx.output_cap);
        let truncated = truncated || stdout_truncated || stderr_truncated;
        if status.success() {
            Ok(ToolOutput::ok(body).truncated(truncated))
        } else {
            Ok(ToolOutput::err(format!("exit {code}\n{body}")).truncated(truncated))
        }
    }
}

/// Kumpulkan output parsial setelah proses dibunuh (maks 2 detik).
///
/// Pipe ditutup ketika proses mati, jadi reader berhenti dengan sendirinya.
/// Bila belum selesai dalam batas waktu, kembalikan penanda kosong — jangan
/// menggantung turn.
async fn collect_partial(
    stdout_task: tokio::task::JoinHandle<(Vec<u8>, bool)>,
    stderr_task: tokio::task::JoinHandle<(Vec<u8>, bool)>,
) -> (String, bool) {
    let joined = tokio::time::timeout(Duration::from_secs(2), async move {
        (stdout_task.await, stderr_task.await)
    })
    .await;
    match joined {
        Ok((Ok((stdout, stdout_truncated)), Ok((stderr, stderr_truncated)))) => {
            let mut text = String::from_utf8_lossy(&stdout).into_owned();
            if !stderr.is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&String::from_utf8_lossy(&stderr));
            }
            if text.trim().is_empty() {
                text.push_str("(tidak ada output parsial)");
            }
            (text, stdout_truncated || stderr_truncated)
        }
        _ => ("(output parsial tidak terbaca)".to_string(), false),
    }
}

/// Apakah nama variabel environment tampak seperti kredensial.
fn nama_env_rahasia(nama: &str) -> bool {
    let atas = nama.to_ascii_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD"]
        .iter()
        .any(|pola| atas.contains(pola))
}

/// Baca pipe sampai cap, kembalikan byte dan penanda terpotong.
async fn read_capped<R>(reader: Option<R>, cap: usize) -> (Vec<u8>, bool)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let Some(reader) = reader else {
        return (Vec::new(), false);
    };
    let mut limited = reader.take(cap.saturating_add(1) as u64);
    let mut buffer = Vec::new();
    if limited.read_to_end(&mut buffer).await.is_err() {
        return (Vec::new(), false);
    }
    let truncated = buffer.len() > cap;
    buffer.truncate(cap);
    (buffer, truncated)
}

/// Hentikan pohon proses. Windows: `taskkill /T` membunuh anak proses yang
/// diturunkan `cmd /C`; POSIX: child di-spawn sebagai **leader proses group**
/// (`process_group(0)`) → `kill(-pgid, SIGKILL)` menghentikan seluruh pohon.
pub(crate) async fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    if cfg!(windows) {
        let mut command = tokio::process::Command::new("taskkill");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), command.status()).await;
        return;
    }
    // POSIX: sinyal via utilitas sistem (tanpa FFI unsafe — gate melarang
    // `unsafe_code`; persetujuan iqbal 2026-11-20 = "lanjutkan kill-tree POSIX",
    // jalurnya: child di-spawn sebagai leader group → /usr/bin/kill -9 -<pgid>).
    let mut command = tokio::process::Command::new("kill");
    command
        .args(["-9", "--", &format!("-{pid}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let _ = tokio::time::timeout(Duration::from_secs(5), command.status()).await;
}

// ---------------------------------------------------------------- todo

struct TodoTool {
    items: Arc<Mutex<Vec<String>>>,
}

impl TodoTool {
    fn new(items: Arc<Mutex<Vec<String>>>) -> Self {
        Self { items }
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn tier(&self) -> ToolTier {
        // RENCANA C-04: `todo` bertier read (mengubah daftar kerja bukan
        // menyentuh berkas), jadi tidak memicu approval tulis.
        ToolTier::Read
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "todo".into(),
            description: "Mengganti atau menampilkan daftar pekerjaan sesi.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Daftar baru; kosongkan untuk melihat daftar saat ini"
                    }
                }
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        if let Some(items) = args.get("items").and_then(Value::as_array) {
            let list: Vec<String> = items
                .iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect();
            let mut guard = self
                .items
                .lock()
                .map_err(|_| tool_error("daftar todo rusak"))?;
            *guard = list;
        }
        let guard = self
            .items
            .lock()
            .map_err(|_| tool_error("daftar todo rusak"))?;
        let body = if guard.is_empty() {
            "(kosong)".to_string()
        } else {
            guard
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. {item}", i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let (content, truncated) = truncate_bytes(&body, ctx.output_cap);
        Ok(ToolOutput::ok(content).truncated(truncated))
    }
}

// ---------------------------------------------------------------- ask

struct AskTool;

#[async_trait]
impl Tool for AskTool {
    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ask".into(),
            description: "Mengajukan pertanyaan ke pengguna pada sesi interaktif.".into(),
            parameters: json!({
                "type": "object",
                "properties": { "question": { "type": "string" } },
                "required": ["question"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let question = args
            .get("question")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("argumen question wajib"))?;
        if !ctx.interactive {
            return Ok(ToolOutput::err("ask hanya tersedia di mode interaktif"));
        }
        let Some(sender) = ctx.ask.as_ref() else {
            return Ok(ToolOutput::err(format!(
                "mode ini belum punya UI pertanyaan; ajukan langsung di chat: {question}"
            )));
        };
        let (reply, answer) = oneshot::channel();
        if sender
            .send(AskRequest {
                question: question.to_string(),
                reply,
            })
            .is_err()
        {
            return Ok(ToolOutput::err("UI pertanyaan tidak aktif"));
        }
        let cancel = ctx.cancel.clone();
        let answered = tokio::select! {
            received = tokio::time::timeout(Duration::from_secs(ASK_TIMEOUT_SECS), answer) => {
                match received {
                    Ok(Ok(text)) => text,
                    Ok(Err(_)) => return Ok(ToolOutput::err("UI menutup tanpa jawaban")),
                    Err(_) => {
                        return Ok(ToolOutput::err(format!(
                            "tidak ada jawaban dalam {ASK_TIMEOUT_SECS} detik"
                        )));
                    }
                }
            }
            () = cancel.cancelled() => return Ok(ToolOutput::err("[dibatalkan] pertanyaan tidak dijawab")),
        };
        if answered.trim().is_empty() {
            return Ok(ToolOutput::err("pengguna melewati pertanyaan"));
        }
        Ok(ToolOutput::ok(format!("jawaban pengguna: {answered}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mx-tools-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Kriteria §2.2 #2: kill-tree POSIX menghentikan SELURUH pohon
    // (shell + utilitas anak). Jalan di Unix; dibuktikan di server Linux.
    #[cfg(unix)]
    #[tokio::test]
    async fn kill_tree_posix_menghentikan_pohon() {
        use std::process::Stdio;
        let dir = temp_dir("kill-tree-posix");
        let ctx = ToolContext::new(&dir);
        // sh -c: spawn utilitas anak (sleep) lalu tunggu → anak = pohon 2 level.
        let mut command = tokio::process::Command::new("/bin/sh");
        command.arg("-c").arg("sleep 30 & wait");
        command
            .current_dir(&ctx.cwd)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        command.process_group(0);
        let mut child = command.spawn().expect("spawn sh");
        let pid = child.id().expect("pid");
        // Tunggu utilitas sleep benar-benar hidup.
        tokio::time::sleep(Duration::from_millis(300)).await;
        eprintln!(
            "[debug] pid={pid} stat={}",
            std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default()
        );
        let anak_hidup = |pid: u32| -> bool {
            // Anggota group tersisa? Zombie (Z) = sudah mati, menunggu reap —
            // dianggap mati (temuan debug di server Linux 2026-11-20).
            std::fs::read_dir("/proc")
                .map(|entries| {
                    entries.flatten().any(|e| {
                        e.file_name()
                            .to_str()
                            .and_then(|s| s.parse::<u32>().ok())
                            .map(|p| {
                                std::fs::read_to_string(format!("/proc/{p}/stat"))
                                    .ok()
                                    .and_then(|s| {
                                        s.rsplit_once(')').map(|(_, sisa)| sisa.to_string())
                                    })
                                    .map(|sisa| {
                                        let fields: Vec<&str> = sisa.split_whitespace().collect();
                                        fields.len() > 2
                                            && fields[0] != "Z"
                                            && fields[2] == pid.to_string()
                                    })
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        };
        assert!(anak_hidup(pid), "pohon harus hidup sebelum kill");
        kill_tree(Some(pid)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!anak_hidup(pid), "pohon harus mati setelah kill_tree");
        let _ = child.start_kill();
        let _ = child.wait().await;
        let _ = fs::remove_dir_all(&dir);
    }

    /// Perintah "tunggu lama" portabel (untuk uji cancel/timeout):
    /// Windows = ping 6x, POSIX = sleep 5 detik.
    fn perintah_tunggu() -> String {
        if cfg!(windows) {
            "ping -n 6 127.0.0.1 > nul".to_string()
        } else {
            "sleep 5".to_string()
        }
    }

    #[tokio::test]
    async fn extension_loader_dan_eksekusi_json() {
        // Folder extension: dua tool — satu jawaban statis, satu meneruskan
        // stdin ke stdout (bukti argumen dikirim). Command per platform.
        let induk = temp_dir("ext-induk");
        let folder = induk.join("sapa");
        fs::create_dir_all(&folder).unwrap();
        let tool_statis: String = if cfg!(windows) {
            r#"["cmd", "/C", "echo {\"content\":\"hai dunia\",\"is_error\":false}"]"#.to_string()
        } else {
            r#"["sh", "-c", "echo {\"content\":\"hai dunia\",\"is_error\":false}"]"#.to_string()
        };
        let tool_stdin: String = if cfg!(windows) {
            r#"["cmd", "/C", "more > stdin_tangkap.txt & type stdin_tangkap.txt"]"#.to_string()
        } else {
            r#"["sh", "-c", "cat > stdin_tangkap.txt; cat stdin_tangkap.txt"]"#.to_string()
        };
        fs::write(
            folder.join("extension.json"),
            format!(
                r#"{{
  "name": "sapa",
  "version": "1.0.0",
  "tools": [
    {{"name": "sapa_orang", "description": "Menyapa", "command": {tool_statis}}},
    {{"name": "lengking", "description": "Meneruskan stdin", "command": {tool_stdin}}}
  ]
}}"#
            ),
        )
        .unwrap();
        let mut catatan = Vec::new();
        let tools = extensions::muat_extensions(&induk, &mut catatan);
        assert_eq!(tools.len(), 2, "catatan: {catatan:?}");
        assert_eq!(tools[0].definition().name, "ext.sapa.sapa_orang");
        assert_eq!(tools[0].tier(), ToolTier::Exec);
        // Eksekusi nyata (cwd = folder extension; stdin dikirim).
        let ctx = ToolContext::new(temp_dir("ext-cwd"));
        let out = tools[0]
            .execute(&json!({"nama": "iqbal"}), &ctx)
            .await
            .expect("execute");
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("hai dunia"), "{}", out.content);
        // Tool kedua: stdout = payload stdin (argumen JSON asli).
        let out2 = tools[1]
            .execute(&json!({"nama": "iqbal", "n": 7}), &ctx)
            .await
            .expect("execute 2");
        assert!(
            out2.content.contains("\"nama\":\"iqbal\""),
            "stdin harus sampai: {}",
            out2.content
        );
        assert!(out2.content.contains("\"n\":7"), "{}", out2.content);
        let _ = fs::remove_dir_all(&induk);
    }

    #[tokio::test]
    async fn extension_loader_menolak_manifest_rusak_dan_traversal() {
        let induk = temp_dir("ext-bad");
        // 1) Nama ≠ folder.
        let folder1 = induk.join("a");
        fs::create_dir_all(&folder1).unwrap();
        fs::write(
            folder1.join("extension.json"),
            r#"{"name":"beda","version":"1","tools":[]}"#,
        )
        .unwrap();
        // 2) Traversal di **program** (argv[0]) — jalur eksekusi luar ditolak.
        let folder2 = induk.join("b");
        fs::create_dir_all(&folder2).unwrap();
        fs::write(
            folder2.join("extension.json"),
            r#"{"name":"b","version":"1","tools":[{"name":"t","description":"x","command":["..\\evil.exe","--flag"]}]}"#,
        )
        .unwrap();
        // 3) Manifest rusak (field asing).
        let folder3 = induk.join("c");
        fs::create_dir_all(&folder3).unwrap();
        fs::write(
            folder3.join("extension.json"),
            r#"{"name":"c","version":"1","tools":[],"extra":1}"#,
        )
        .unwrap();
        let mut catatan = Vec::new();
        let tools = extensions::muat_extensions(&induk, &mut catatan);
        assert!(tools.is_empty(), "semua harus ditolak: {}", tools.len());
        assert!(cataten_len(&catatan) >= 3, "catatan: {catatan:?}");
        let _ = fs::remove_dir_all(&induk);
    }

    fn cataten_len(v: &[String]) -> usize {
        v.len()
    }

    #[tokio::test]
    async fn read_memberi_nomor_baris() {
        let dir = temp_dir("read");
        fs::write(dir.join("a.txt"), "satu\ndua\n").unwrap();
        let out = ReadTool
            .execute(&json!({"path": "a.txt"}), &ToolContext::new(&dir))
            .await
            .unwrap();
        assert!(out.content.contains("1\tsatu"));
        assert!(out.content.contains("2\tdua"));
        assert!(!out.is_error);
    }

    #[tokio::test]
    async fn read_menolak_traversal() {
        let dir = temp_dir("traversal");
        let err = ReadTool
            .execute(&json!({"path": "..\\rahasia.txt"}), &ToolContext::new(&dir))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ToolFailed);
        assert!(err.message.contains("traversal"));
    }

    #[tokio::test]
    async fn write_membuat_direktori_induk() {
        let dir = temp_dir("write");
        let out = WriteTool
            .execute(
                &json!({"path": "sub/dir/b.txt", "content": "isi"}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(out.content.contains("3 byte"));
        assert_eq!(
            fs::read_to_string(dir.join("sub/dir/b.txt")).unwrap(),
            "isi"
        );
    }

    #[tokio::test]
    async fn output_dipotong_sesuai_cap() {
        let dir = temp_dir("cap");
        fs::write(dir.join("besar.txt"), "x".repeat(500)).unwrap();
        let mut ctx = ToolContext::new(&dir);
        ctx.output_cap = 100;
        let out = ReadTool
            .execute(&json!({"path": "besar.txt"}), &ctx)
            .await
            .unwrap();
        assert!(out.truncated);
        assert!(out.content.contains("[dipotong]"));
    }

    #[tokio::test]
    async fn glob_menemukan_berkas() {
        let dir = temp_dir("glob");
        fs::write(dir.join("a.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("b.txt"), "x").unwrap();
        let out = GlobTool
            .execute(&json!({"pattern": "**/*.rs"}), &ToolContext::new(&dir))
            .await
            .unwrap();
        assert!(out.content.contains("a.rs"));
        assert!(!out.content.contains("b.txt"));
    }

    #[tokio::test]
    async fn grep_menemukan_pola() {
        let dir = temp_dir("grep");
        fs::write(dir.join("a.txt"), "halo dunia\nbaris lain\n").unwrap();
        let out = GrepTool
            .execute(&json!({"pattern": "dunia"}), &ToolContext::new(&dir))
            .await
            .unwrap();
        assert!(out.content.contains("a.txt:1:"));
    }

    #[tokio::test]
    async fn read_menghormati_offset_dan_limit() {
        let dir = temp_dir("read-slice");
        fs::write(dir.join("a.txt"), "satu\ndua\ntiga\nempat\n").unwrap();
        let out = ReadTool
            .execute(
                &json!({"path": "a.txt", "offset": 2, "limit": 2}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(out.content.contains("2\tdua"), "{}", out.content);
        assert!(out.content.contains("3\ttiga"), "{}", out.content);
        assert!(!out.content.contains("1\tsatu"));
        assert!(!out.content.contains("4\tempat"));
    }

    #[tokio::test]
    async fn read_menolak_berkas_biner() {
        let dir = temp_dir("read-biner");
        fs::write(dir.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();
        let out = ReadTool
            .execute(&json!({"path": "blob.bin"}), &ToolContext::new(&dir))
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("biner"));
    }

    #[tokio::test]
    async fn read_offset_di_luar_isi_memberi_keterangan() {
        let dir = temp_dir("read-eof");
        fs::write(dir.join("a.txt"), "satu\n").unwrap();
        let out = ReadTool
            .execute(
                &json!({"path": "a.txt", "offset": 10}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert!(out.content.contains("tidak ada baris"), "{}", out.content);
        assert!(out.content.contains("1 baris"), "{}", out.content);
    }

    #[tokio::test]
    async fn grep_menghormati_max_results() {
        let dir = temp_dir("grep-cap");
        fs::write(dir.join("a.txt"), "cocok satu\ncocok dua\ncocok tiga\n").unwrap();
        let out = GrepTool
            .execute(
                &json!({"pattern": "cocok", "max_results": 1}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert_eq!(out.content.lines().count(), 2, "{}", out.content);
        assert!(out.content.contains("[dipotong pada 1 hasil"));
    }

    #[tokio::test]
    async fn ask_interaktif_tetap_jujur_belum_ada_ui() {
        let dir = temp_dir("ask-interaktif");
        let mut ctx = ToolContext::new(&dir);
        ctx.interactive = true;
        let out = AskTool
            .execute(&json!({"question": "lanjut?"}), &ctx)
            .await
            .unwrap();
        assert!(out.is_error, "tidak boleh mengaku menunggu jawaban");
        assert!(
            out.content.contains("belum punya UI pertanyaan"),
            "{}",
            out.content
        );
    }

    #[tokio::test]
    async fn ask_mengirim_pertanyaan_dan_menerima_jawaban() {
        let dir = temp_dir("ask-kanal");
        let (tx, mut rx) = mpsc::unbounded_channel::<AskRequest>();
        let mut ctx = ToolContext::new(&dir);
        ctx.interactive = true;
        ctx.ask = Some(tx);
        tokio::spawn(async move {
            if let Some(request) = rx.recv().await {
                let _ = request.reply.send("ya, lanjut".to_string());
            }
        });
        let out = AskTool
            .execute(&json!({"question": "lanjut?"}), &ctx)
            .await
            .unwrap();
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("jawaban pengguna: ya, lanjut"));
    }

    #[tokio::test]
    async fn ask_jawaban_kosong_dianggap_dilewati() {
        let dir = temp_dir("ask-lewati");
        let (tx, mut rx) = mpsc::unbounded_channel::<AskRequest>();
        let mut ctx = ToolContext::new(&dir);
        ctx.interactive = true;
        ctx.ask = Some(tx);
        tokio::spawn(async move {
            if let Some(request) = rx.recv().await {
                let _ = request.reply.send("   ".to_string());
            }
        });
        let out = AskTool
            .execute(&json!({"question": "lanjut?"}), &ctx)
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("melewati"));
    }

    #[test]
    fn replace_named_mengganti_dan_mendaftar() {
        struct Penanda {
            nama: &'static str,
        }
        #[async_trait::async_trait]
        impl Tool for Penanda {
            fn tier(&self) -> ToolTier {
                ToolTier::Read
            }
            fn definition(&self) -> ToolDefinition {
                ToolDefinition {
                    name: self.nama.to_string(),
                    description: "uji".into(),
                    parameters: serde_json::json!({"type": "object"}),
                    concurrency: tui_wire::Concurrency::Shared,
                }
            }
            async fn execute(
                &self,
                _args: &serde_json::Value,
                _ctx: &ToolContext,
            ) -> Result<ToolOutput, MxError> {
                Ok(ToolOutput::ok("x"))
            }
        }
        let mut registry = ToolRegistry::from_tools(Vec::new());
        assert!(
            !registry.replace_named(Arc::new(Penanda { nama: "a" })),
            "baru = daftar"
        );
        assert_eq!(registry.names(), vec!["a".to_string()]);
        assert!(
            registry.replace_named(Arc::new(Penanda { nama: "a" })),
            "ada = ganti"
        );
        assert_eq!(registry.names().len(), 1, "tidak diduplikasi");
        assert!(registry.remove_named("a"));
        assert!(!registry.remove_named("a"), "hapus dua kali = tidak ada");
        assert!(registry.names().is_empty());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn junction_keluar_cwd_ditolak() {
        let dir = temp_dir("junction-cwd");
        let luar = temp_dir("junction-luar");
        std::fs::write(luar.join("rahasia.txt"), "isi rahasia").unwrap();
        let tautan = dir.join("tautan");
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                tautan.to_str().unwrap(),
                luar.to_str().unwrap(),
            ])
            .status()
            .expect("jalankan mklink");
        if !status.success() {
            // Junction tidak tersedia di lingkungan ini; jangan gagal palsu.
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_dir_all(&luar);
            return;
        }
        let ctx = ToolContext::new(&dir);
        let out = ReadTool
            .execute(&json!({"path": "tautan/rahasia.txt"}), &ctx)
            .await;
        match out {
            Err(err) => assert!(
                err.message.to_lowercase().contains("traversal"),
                "{}",
                err.message
            ),
            Ok(out) => assert!(
                out.is_error,
                "membaca lewat junction harus ditolak: {}",
                out.content
            ),
        }
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&luar);
    }

    #[test]
    fn env_kredensial_dikenali_untuk_dibuang() {
        for nama in [
            "MODELUXAI_API_KEY",
            "GITHUB_TOKEN",
            "DB_PASSWORD",
            "my_secret_value",
        ] {
            assert!(nama_env_rahasia(nama), "{nama} harus dikenali");
        }
        for nama in ["PATH", "USERNAME", "TEMP", "MX_DATA_DIR"] {
            assert!(!nama_env_rahasia(nama), "{nama} bukan kredensial");
        }
    }

    struct UjiBridge {
        skema: String,
    }

    impl UriResolver for UjiBridge {
        fn owns(&self, url: &str) -> bool {
            url.starts_with(&format!("{}://", self.skema))
        }
        fn read(
            &self,
            _url: String,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        {
            Box::pin(async { Ok("isi dari host".to_string()) })
        }
        fn write(
            &self,
            _url: String,
            isi: String,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        {
            Box::pin(async move { Ok(format!("tertulis: {isi}")) })
        }
    }

    #[tokio::test]
    async fn read_dan_write_url_skema_host_dipantulkan_ke_bridge() {
        let dir = temp_dir("uri-bridge");
        let mut ctx = ToolContext::new(&dir);
        ctx.uri_bridge = Some(Arc::new(UjiBridge {
            skema: "db".to_string(),
        }));
        // read
        let out = ReadTool
            .execute(&json!({"path": "db://users/42"}), &ctx)
            .await
            .expect("read via bridge");
        assert!(!out.is_error);
        assert_eq!(out.content, "isi dari host");
        // write
        let out = WriteTool
            .execute(
                &json!({"path": "db://users/42", "content": "isi baru"}),
                &ctx,
            )
            .await
            .expect("write via bridge");
        assert!(!out.is_error);
        assert_eq!(out.content, "tertulis: isi baru");
        // URL skema asing tidak dipantulkan; jatuh ke jalur path (gagal membaca
        // karena karakter URL tidak sah sebagai nama berkas) — diuji sebagai error.
        let out = ReadTool
            .execute(&json!({"path": "lain://users/42"}), &ctx)
            .await
            .expect_err("skema asing = error infrastruktur");
        assert_eq!(out.code, tui_wire::ErrorCode::ToolFailed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_menolak_traversal_gaya_windows() {
        let dir = temp_dir("traversal-win");
        let ctx = ToolContext::new(&dir);
        for raw in [
            "..\\rahasia.txt",
            "sub/../../rahasia.txt",
            "C:\\Windows\\win.ini",
        ] {
            match ReadTool.execute(&json!({"path": raw}), &ctx).await {
                Err(err) => {
                    let pesan = err.message.to_lowercase();
                    assert!(
                        pesan.contains("traversal")
                            || pesan.contains("ditolak")
                            || pesan.contains("di luar"),
                        "pesan harus jelas untuk {raw}: {}",
                        err.message
                    );
                }
                Ok(out) => assert!(
                    out.is_error,
                    "path {raw} harus ditolak, bukan dibaca: {}",
                    out.content
                ),
            }
        }
    }

    #[tokio::test]
    async fn edit_pencocokan_persis_termasuk_akhir_baris() {
        let dir = temp_dir("edit-crlf");
        let berkas = dir.join("crlf.txt");
        std::fs::write(&berkas, "baris satu\r\nbaris dua\r\n").unwrap();
        let ctx = ToolContext::new(&dir);
        // LF saja tidak cocok dengan CRLF: kontraknya pencocokan persis.
        let out = EditTool
            .execute(
                &json!({"path": "crlf.txt", "old_string": "baris satu\nbaris dua", "new_string": "x"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.is_error, "LF tidak boleh cocok dengan CRLF");
        // Dengan CRLF persis, penggantian berhasil.
        let out = EditTool
            .execute(
                &json!({"path": "crlf.txt", "old_string": "baris satu\r\nbaris dua", "new_string": "baris baru"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(
            !out.is_error,
            "pencocokan persis CRLF harus berhasil: {}",
            out.content
        );
        let isi = std::fs::read_to_string(&berkas).unwrap();
        assert_eq!(isi, "baris baru\r\n");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn timeout_mengembalikan_output_parsial_bertanda() {
        let dir = temp_dir("timeout-parsial");
        let mut ctx = ToolContext::new(&dir);
        ctx.bash_timeout = Duration::from_millis(400);
        ctx.bash_max_timeout = Duration::from_secs(30);
        let out = BashTool
            .execute(
                &json!({"command": "echo mulai & ping -n 10 127.0.0.1 >NUL"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.is_error, "timeout harus error hasil: {}", out.content);
        assert!(
            out.content.contains("[timeout"),
            "harus menyebut timeout: {}",
            out.content
        );
        assert!(
            out.content.contains("mulai"),
            "output parsial harus disertakan: {}",
            out.content
        );
        assert!(out.truncated, "output parsial harus ditandai terpotong");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn timeout_membunuh_anak_proses() {
        let dir = temp_dir("kill-tree");
        let marker = dir.join("child-marker.txt");
        let mut ctx = ToolContext::new(&dir);
        ctx.bash_timeout = Duration::from_millis(1_000);
        ctx.bash_max_timeout = Duration::from_secs(30);
        // Anak proses menyentuh marker setelah 2,5 detik; induk menahan ~9 detik.
        let perintah = format!(
            "start \"\" /B powershell -NoProfile -Command \"Start-Sleep -Milliseconds 2500; \
             Set-Content -Path '{}' -Value 1\" & ping -n 10 127.0.0.1 >NUL",
            marker.display()
        );
        let out = BashTool
            .execute(&json!({ "command": perintah }), &ctx)
            .await
            .unwrap();
        assert!(out.is_error, "timeout harus dilaporkan sebagai error hasil");
        assert!(out.content.contains("[timeout"), "{}", out.content);
        std::thread::sleep(Duration::from_millis(3_500));
        assert!(
            !marker.exists(),
            "anak proses harus ikut dibunuh, bukan hidup sampai menulis marker"
        );
    }

    #[tokio::test]
    async fn bash_mengembalikan_kode_keluar() {
        let dir = temp_dir("bash");
        let out = BashTool
            .execute(&json!({"command": "echo halo"}), &ToolContext::new(&dir))
            .await
            .unwrap();
        assert!(out.content.contains("halo"), "keluaran: {}", out.content);
    }

    #[tokio::test]
    async fn bash_berhenti_saat_dibatalkan() {
        let dir = temp_dir("bash-cancel");
        let mut ctx = ToolContext::new(&dir);
        ctx.bash_timeout = Duration::from_secs(30);
        let cancel = ctx.cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancel.cancel();
        });
        let started = std::time::Instant::now();
        let out = BashTool
            .execute(&json!({"command": perintah_tunggu()}), &ctx)
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(
            out.content.contains("dibatalkan"),
            "keluaran: {}",
            out.content
        );
        assert!(
            // Batas 8 dtk: pembatalan nyata berjalan <1 dtk, tetapi saat test
            // paralel (beban CPU) spawn proses bisa telat; yang diuji adalah
            // "jangan menunggu timeout penuh 30 dtk". Tinjau ulang 2027-01-19.
            started.elapsed() < Duration::from_secs(8),
            "pembatalan harus cepat, bukan menunggu 30 detik"
        );
    }
    #[tokio::test]
    async fn bash_timeout_menghentikan_perintah() {
        let dir = temp_dir("bash-timeout");
        let mut ctx = ToolContext::new(&dir);
        ctx.bash_timeout = Duration::from_millis(300);
        let out = BashTool
            .execute(&json!({"command": perintah_tunggu()}), &ctx)
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("timeout"));
    }

    #[test]
    fn todo_bertier_read_sesuai_rencana() {
        let registry = ToolRegistry::v1();
        let tool = registry.get("todo").expect("tool todo");
        assert_eq!(tool.tier(), ToolTier::Read);
        let bash = registry.get("bash").expect("tool bash");
        assert_eq!(bash.tier(), ToolTier::Exec);
    }

    #[test]
    fn registry_membagikan_daftar_todo() {
        let registry = ToolRegistry::v1();
        assert!(registry.todos().is_empty());
        let tool = registry.get("todo").expect("tool todo");
        let dir = temp_dir("todo-registry");
        let ctx = ToolContext::new(&dir);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime
            .block_on(tool.execute(&json!({"items": ["kerja"]}), &ctx))
            .expect("tulis todo");
        assert_eq!(registry.todos(), vec!["kerja".to_string()]);
    }

    #[tokio::test]
    async fn todo_menyimpan_dan_menampilkan() {
        let dir = temp_dir("todo");
        let tool = TodoTool::new(Arc::new(Mutex::new(Vec::new())));
        let ctx = ToolContext::new(&dir);
        tool.execute(&json!({"items": ["satu", "dua"]}), &ctx)
            .await
            .unwrap();
        let out = tool.execute(&json!({}), &ctx).await.unwrap();
        assert!(out.content.contains("1. satu"));
        assert!(out.content.contains("2. dua"));
    }

    #[tokio::test]
    async fn ask_di_luar_mode_interaktif_menolak() {
        let dir = temp_dir("ask");
        let out = AskTool
            .execute(&json!({"question": "lanjut?"}), &ToolContext::new(&dir))
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("interaktif"));
    }

    #[tokio::test]
    async fn edit_mengganti_satu_kemunculan() {
        let dir = temp_dir("edit");
        fs::write(dir.join("a.txt"), "satu dua tiga").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.txt", "old_string": "dua", "new_string": "DUA"}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).unwrap(),
            "satu DUA tiga"
        );
    }

    #[tokio::test]
    async fn edit_menolak_teks_ambigu() {
        let dir = temp_dir("edit-ambiguous");
        fs::write(dir.join("a.txt"), "x x").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.txt", "old_string": "x", "new_string": "y"}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("2 kali"));
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "x x");
    }

    #[tokio::test]
    async fn edit_teks_tidak_ditemukan_adalah_error_hasil() {
        let dir = temp_dir("edit-missing");
        fs::write(dir.join("a.txt"), "isi").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.txt", "old_string": "tidak-ada", "new_string": "y"}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("0 kecocokan"));
    }

    #[tokio::test]
    async fn edit_replace_all_mengganti_semua() {
        let dir = temp_dir("edit-all");
        fs::write(dir.join("a.txt"), "a a a").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        assert_eq!(fs::read_to_string(dir.join("a.txt")).unwrap(), "b b b");
    }

    #[tokio::test]
    async fn edit_old_string_kosong_ditolak() {
        let dir = temp_dir("edit-empty");
        fs::write(dir.join("a.txt"), "isi").unwrap();
        let out = EditTool
            .execute(
                &json!({"path": "a.txt", "old_string": "", "new_string": "y"}),
                &ToolContext::new(&dir),
            )
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("tidak boleh kosong"));
    }

    #[test]
    fn registry_v1_memuat_delapan_tool() {
        let registry = ToolRegistry::v1();
        let names = registry.names();
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"bash".to_string()));
        assert!(names.contains(&"edit".to_string()));
        assert!(registry.get("tidak-ada").is_none());
    }
}
