//! Loader extension eksternal (adopsi omp extensions, versi v1 aman).
//!
//! Setiap extension = folder di `<config_root>/extensions/<nama>/` dengan
//! manifest `extension.json` (parsing **ketat**, `deny_unknown_fields`) yang
//! mendeklarasikan **tool proses eksternal**. Setiap pemanggilan tool
//! men-spawn perintah manifest, mengirim argumen sebagai JSON di stdin, dan
//! membaca **satu baris** jawaban JSON di stdout.
//!
//! Sandboxing v1 (terdokumentasi, sesuai AGENTS §5):
//! - cwd proses = folder extension;
//! - **env dibersihkan** (tidak mewarisi API key/kredensial) — hanya PATH,
//!   SystemRoot, TEMP/TMP, LANG;
//! - timeout + cap output + pembatalan via token (kill tree);
//! - nama tool di registry berprefix `ext.<extension>.<tool>`;
//! - tier Exec: selalu lewat approval gate, tanpa jalur pintas.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tui_wire::{Concurrency, MxError, ToolDefinition, ToolTier};

use crate::{Tool, ToolContext, ToolOutput};

/// Manifest extension (`extension.json`) — parsing ketat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    /// Nama extension (harus sama dengan nama folder).
    pub name: String,
    /// Versi bebas (ditampilkan).
    pub version: String,
    /// Daftar tool yang disediakan.
    pub tools: Vec<ExtToolSpec>,
}

/// Deklarasi satu tool di manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtToolSpec {
    /// Nama tool (tanpa prefix; alfanumerik + garis bawah).
    pub name: String,
    /// Deskripsi untuk model.
    pub description: String,
    /// Perintah yang mengeksekusi tool (argv; elemen boleh path relatif ke
    /// folder extension — traversal `..` ditolak).
    pub command: Vec<String>,
}

/// Validasi nama bebas-aman (alfanumerik, `-`, `_`).
fn nama_sah(nama: &str) -> bool {
    !nama.is_empty()
        && nama
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Path dalam folder extension: tolak traversal dan absolut.
fn path_dalam_folder(folder: &Path, elemen: &str) -> Option<PathBuf> {
    if elemen.contains("..") || Path::new(elemen).is_absolute() {
        return None;
    }
    Some(folder.join(elemen))
}

/// Muat semua extension dari folder induk; extension rusak dilaporkan di
/// `catatan` (bukan membuat boot gagal).
pub fn muat_extensions(folder_induk: &Path, catatan: &mut Vec<String>) -> Vec<Arc<dyn Tool>> {
    let mut hasil: Vec<Arc<dyn Tool>> = Vec::new();
    let Ok(entries) = std::fs::read_dir(folder_induk) else {
        return hasil;
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    for folder in folders {
        let nama_folder = folder
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let manifest_path = folder.join("extension.json");
        let Ok(isi) = std::fs::read_to_string(&manifest_path) else {
            catatan.push(format!(
                "extensions/{nama_folder}: extension.json tidak ada — dilewati"
            ));
            continue;
        };
        let manifest: ExtensionManifest = match serde_json::from_str(&isi) {
            Ok(m) => m,
            Err(err) => {
                catatan.push(format!(
                    "extensions/{nama_folder}: manifest tidak valid ({err}) — dilewati"
                ));
                continue;
            }
        };
        if manifest.name != nama_folder || !nama_sah(&manifest.name) {
            catatan.push(format!(
                "extensions/{nama_folder}: nama manifest '{}' ≠ nama folder / tidak sah — dilewati",
                manifest.name
            ));
            continue;
        }
        // Nama tool unik dalam extension.
        let mut terlihat: HashMap<String, ()> = HashMap::new();
        for spec in &manifest.tools {
            if !nama_sah(&spec.name)
                || terlihat.insert(spec.name.clone(), ()).is_some()
                || spec.command.is_empty()
            {
                catatan.push(format!(
                    "extensions/{}: tool '{}' tidak sah/dobel/tanpa command — dilewati",
                    manifest.name, spec.name
                ));
                continue;
            }
            // Program (argv[0]): **file di folder extension** bila ada (jalur
            // relatif divalidasi — traversal/absolut ditolak keras), selain itu
            // nama program di PATH. **Argumen tetap literal** (bukan mapping).
            let program = &spec.command[0];
            if program.contains("..") || Path::new(program).is_absolute() {
                // Bila bukan file di folder ext, hanya nama program PATH yang
                // sah — traversal di program = jalur eksekusi luar, tolak.
                let sah = path_dalam_folder(&folder, program).is_some_and(|p| p.exists());
                if !sah {
                    catatan.push(format!(
                        "extensions/{}: program '{}' keluar folder — tool ditolak",
                        manifest.name, program
                    ));
                    continue;
                }
            }
            let program_path = path_dalam_folder(&folder, program)
                .filter(|p| p.exists())
                .unwrap_or_else(|| PathBuf::from(program));
            let mut argv_str: Vec<String> = spec.command.clone();
            argv_str[0] = program_path.to_string_lossy().to_string();
            let nama_tool = format!("ext.{}.{}", manifest.name, spec.name);
            hasil.push(Arc::new(ExtensionTool {
                nama_tool,
                deskripsi: spec.description.clone(),
                folder: folder.clone(),
                argv: argv_str,
            }));
        }
        catatan.push(format!(
            "extensions/{}: dimuat ({} tool)",
            manifest.name,
            manifest.tools.len()
        ));
    }
    hasil
}

/// Satu tool extension: spawn proses, kirim argumen JSON di stdin, baca satu
/// baris jawaban JSON.
struct ExtensionTool {
    nama_tool: String,
    deskripsi: String,
    folder: PathBuf,
    argv: Vec<String>,
}

impl ExtensionTool {
    /// Env minimal untuk proses extension (tanpa kredensial).
    fn env_bersih() -> HashMap<String, String> {
        let mut env = HashMap::new();
        for kunci in ["PATH", "SystemRoot", "TEMP", "TMP", "LANG", "WINDIR"] {
            if let Ok(nilai) = std::env::var(kunci) {
                env.insert(kunci.to_string(), nilai);
            }
        }
        env
    }
}

#[async_trait::async_trait]
impl Tool for ExtensionTool {
    fn tier(&self) -> ToolTier {
        // Kode pihak ketiga — exec, selalu approval.
        ToolTier::Exec
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.nama_tool.clone(),
            description: format!("(extension) {}", self.deskripsi),
            parameters: json!({
                "type": "object",
                "additionalProperties": true
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let mut command = tokio::process::Command::new(&self.argv[0]);
        command
            .args(&self.argv[1..])
            .current_dir(&self.folder)
            .env_clear()
            .envs(Self::env_bersih())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // POSIX: leader proses group → kill_tree menghentikan pohon extension.
        #[cfg(unix)]
        {
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|err| {
            MxError::new(
                tui_wire::ErrorCode::ToolFailed,
                format!("extension {} gagal dijalankan: {err}", self.nama_tool),
            )
        })?;
        let pid = child.id();
        // Kirim argumen sebagai satu baris JSON di stdin.
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let payload = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
            let _ = stdin.write_all(payload.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.shutdown().await;
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        // Pipe dibaca di task terpisah (tidak memblokir wait; tanpa deadlock).
        let stdout_task = tokio::spawn(super::read_capped(stdout, ctx.output_cap));
        let stderr_task = tokio::spawn(super::read_capped(stderr, 4_096));
        enum Tunggu {
            Selesai(std::io::Result<std::process::ExitStatus>),
            Timeout,
            Dibatalkan,
        }
        let hasil_tunggu = tokio::select! {
            outcome = tokio::time::timeout(ctx.bash_timeout, child.wait()) => match outcome {
                Ok(status) => Tunggu::Selesai(status),
                Err(_) => Tunggu::Timeout,
            },
            () = ctx.cancel.cancelled() => Tunggu::Dibatalkan,
        };
        match hasil_tunggu {
            Tunggu::Selesai(Ok(status)) => {
                let ((out_bytes, out_trunc), (err_bytes, _)) = (
                    stdout_task.await.unwrap_or_default(),
                    stderr_task.await.unwrap_or_default(),
                );
                let mut isi = String::from_utf8_lossy(&out_bytes).to_string();
                if out_trunc {
                    isi.push_str("… [terpotong]");
                }
                if !err_bytes.is_empty() {
                    isi.push_str(&format!(
                        "\n[stderr] {}",
                        String::from_utf8_lossy(&err_bytes)
                            .chars()
                            .take(200)
                            .collect::<String>()
                    ));
                }
                // Jawaban extension: baris pertama JSON valid.
                if let Some(baris) = isi.lines().find(|l| l.trim_start().starts_with('{'))
                    && let Ok(jawaban) = serde_json::from_str::<Value>(baris.trim())
                {
                    let content = jawaban
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or(&isi)
                        .to_string();
                    let is_error = jawaban
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(!status.success());
                    return Ok(if is_error {
                        ToolOutput::err(content)
                    } else {
                        ToolOutput::ok(content)
                    });
                }
                if status.success() {
                    Ok(ToolOutput::ok(isi))
                } else {
                    Ok(ToolOutput::err(isi))
                }
            }
            Tunggu::Selesai(Err(err)) => Ok(ToolOutput::err(format!(
                "extension {}: tunggu proses gagal: {err}",
                self.nama_tool
            ))),
            Tunggu::Timeout | Tunggu::Dibatalkan => {
                super::kill_tree(pid).await;
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                let penyebab = if matches!(hasil_tunggu, Tunggu::Dibatalkan) {
                    "dibatalkan"
                } else {
                    "timeout"
                };
                Ok(ToolOutput::err(format!(
                    "[{penyebab}] extension {} dihentikan ({} dtk)",
                    self.nama_tool,
                    ctx.bash_timeout.as_secs()
                )))
            }
        }
    }
}
