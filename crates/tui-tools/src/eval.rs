//! Tool `eval`: kernel Python persisten per `kernel_id` untuk `run_code`.
//!
//! Satu proses REPL (`python3 -u -i` atau `bun repl`) dipertahankan antar-call, sehingga variabel /
//! import bertahan. Sentinel baris tunggal menandai akhir eksekusi; timeout
//! mencegah kode menggantung tool.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

const SENTINEL: &str = "__MAICLI_EVAL_DONE__";

struct Kernel {
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

static KERNELS: OnceLock<Mutex<HashMap<String, Kernel>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Kernel>> {
    KERNELS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Bersihkan prompt REPL dan banner dari keluaran.
pub fn bersihkan_dengan(mentah: &str, lang: &str) -> String {
    let (prompt, banner) = if lang == "js" {
        ("> ", "Welcome to Bun")
    } else {
        (">>> ", "Python 3.")
    };
    mentah
        .lines()
        .filter(|l| !l.contains(banner) && !l.contains("Type \"help\""))
        .flat_map(|l| l.split(prompt))
        .map(str::trim_end)
        .filter(|l| !l.is_empty() && !(lang == "js" && *l == "undefined"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Bersihkan keluaran Python (kompatibilitas test lama).
pub fn bersihkan(mentah: &str) -> String {
    bersihkan_dengan(mentah, "python")
}

/// Tool `eval`.
pub struct EvalTool;

#[async_trait]
impl Tool for EvalTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "eval".to_string(),
            description: "Jalankan kode Python di kernel persisten (state antar-call).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "lang": { "type": "string", "enum": ["python", "js"], "description": "Default python" },
                    "kernel_id": { "type": "string", "description": "Default: default" },
                    "timeout_ms": { "type": "integer", "description": "100..30000, default 5000" }
                },
                "required": ["code"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(code) = args.get("code").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen code wajib"));
        };
        if code.trim().is_empty() {
            return Ok(ToolOutput::err("code tidak boleh kosong"));
        }
        let lang = args.get("lang").and_then(Value::as_str).unwrap_or("python");
        if !matches!(lang, "python" | "js") {
            return Ok(ToolOutput::err("lang harus python atau js"));
        }
        let id = format!(
            "{}:{}",
            lang,
            args.get("kernel_id")
                .and_then(Value::as_str)
                .unwrap_or("default")
        );
        let batas = Duration::from_millis(
            args.get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(5_000)
                .clamp(100, 30_000),
        );
        let mut kernels = registry().lock().await;
        if !kernels.contains_key(&id) {
            let (program, argv): (&str, &[&str]) = if lang == "js" {
                ("bun", &["repl"][..])
            } else {
                ("python3", &["-u", "-i"][..])
            };
            let mut child = match tokio::process::Command::new(program)
                .args(argv)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => c,
                Err(err) => return Ok(ToolOutput::err(format!("python3 tidak tersedia: {err}"))),
            };
            let stdin = match child.stdin.take() {
                Some(v) => v,
                None => return Ok(ToolOutput::err("stdin kernel tidak tersedia".to_string())),
            };
            let stdout = match child.stdout.take() {
                Some(v) => v,
                None => return Ok(ToolOutput::err("stdout kernel tidak tersedia".to_string())),
            };
            kernels.insert(
                id.clone(),
                Kernel {
                    _child: child,
                    stdin,
                    stdout: BufReader::new(stdout),
                },
            );
        }
        let Some(kernel) = kernels.get_mut(&id) else {
            return Ok(ToolOutput::err("kernel tidak tersedia".to_string()));
        };
        let penanda = if lang == "js" {
            format!("console.log({SENTINEL:?})")
        } else {
            format!("print({SENTINEL:?})")
        };
        let kirim = format!("{code}\n{penanda}\n");
        if kernel.stdin.write_all(kirim.as_bytes()).await.is_err() {
            return Ok(ToolOutput::err("gagal menulis ke kernel".to_string()));
        }
        let mut mentah = String::new();
        loop {
            let mut baris = String::new();
            let hasil = tokio::time::timeout(batas, kernel.stdout.read_line(&mut baris)).await;
            match hasil {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(_)) => {
                    if baris.contains(SENTINEL) {
                        break;
                    }
                    mentah.push_str(&baris);
                }
                Ok(Err(err)) => return Ok(ToolOutput::err(format!("baca kernel gagal: {err}"))),
            }
        }
        let hasil = bersihkan_dengan(&mentah, lang);
        Ok(ToolOutput::ok(if hasil.is_empty() {
            "(tanpa keluaran)".to_string()
        } else {
            hasil
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn bersihkan_prompt_repl() {
        assert_eq!(bersihkan(">>> >>> 42\n>>> "), "42");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn state_kernel_bertahan() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let tool = EvalTool;
        let ctx = ToolContext::new(std::env::temp_dir());
        let id = format!("uji-{}", std::process::id());
        let _ = tool
            .execute(&json!({"code":"x = 41","kernel_id":id}), &ctx)
            .await
            .expect("kernel jalan");
        let out = tool
            .execute(&json!({"code":"print(x + 1)","kernel_id":id}), &ctx)
            .await
            .expect("kernel jalan");
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("42"), "{}", out.content);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn state_kernel_bun_bertahan() {
        if std::process::Command::new("bun")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let tool = EvalTool;
        let ctx = ToolContext::new(std::env::temp_dir());
        let id = format!("uji-js-{}", std::process::id());
        let _ = tool
            .execute(
                &json!({"code":"const y = 41","lang":"js","kernel_id":id}),
                &ctx,
            )
            .await
            .expect("kernel bun jalan");
        let out = tool
            .execute(
                &json!({"code":"console.log(y + 1)","lang":"js","kernel_id":id}),
                &ctx,
            )
            .await
            .expect("kernel bun jalan");
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("42"), "{}", out.content);
    }
}
