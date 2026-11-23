//! Tool `lsp`: klien LSP generik lewat stdio untuk diagnostics/references/
//! definition/rename. Server ditentukan config (`[lsp]`) bukan oleh model.
//!
//! Protokol LSP memakai header `Content-Length` seperti HTTP; respon yang
//! relevan dicocokkan oleh id, notifikasi `publishDiagnostics` dikumpulkan
//! sebagai cadangan bila server tidak mendukung pull diagnostics.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

/// Klien satu server LSP.
#[derive(Clone)]
pub struct LspTool {
    command: String,
    args: Vec<String>,
    timeout: Duration,
}

impl LspTool {
    /// Bangun tool; `None` bila perintah kosong.
    pub fn new(command: &str, args: Vec<String>, timeout_secs: u64) -> Option<Self> {
        if command.trim().is_empty() {
            return None;
        }
        Some(Self {
            command: command.to_string(),
            args,
            timeout: Duration::from_secs(timeout_secs.max(1)),
        })
    }

    /// Konversi path menjadi `file://` URI (persen-encode karakter khusus).
    pub fn file_uri(root: &Path, relatif: &str) -> String {
        let mut path = PathBuf::from(relatif);
        if path.is_relative() {
            path = root.join(path);
        }
        let mut teks = path.to_string_lossy().replace('\\', "/");
        if !teks.starts_with('/') {
            teks.insert(0, '/');
        }
        let encoded = teks
            .replace('%', "%25")
            .replace(' ', "%20")
            .replace('#', "%23")
            .replace('?', "%3F");
        format!("file://{encoded}")
    }

    /// Ringkas array diagnostic LSP menjadi baris yang mudah dibaca model.
    pub fn ringkas_diagnostic(items: &[Value]) -> Vec<String> {
        items
            .iter()
            .map(|d| {
                let baris = d
                    .get("range")
                    .and_then(|r| r.get("start"))
                    .and_then(|s| s.get("line"))
                    .and_then(Value::as_u64)
                    .map(|n| n + 1)
                    .unwrap_or(0);
                let severity = match d.get("severity").and_then(Value::as_u64) {
                    Some(1) => "error",
                    Some(2) => "warning",
                    Some(3) => "info",
                    _ => "hint",
                };
                format!(
                    "{}:{} {} — {}",
                    severity,
                    baris,
                    d.get("code").and_then(Value::as_str).unwrap_or(""),
                    d.get("message").and_then(Value::as_str).unwrap_or("")
                )
            })
            .collect()
    }
}

/// Baca satu pesan LSP dari reader; `None` bila stream ditutup.
pub(crate) async fn baca<R: tokio::io::AsyncBufRead + tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Value>, String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    let mut panjang: Option<usize> = None;
    loop {
        let mut baris = String::new();
        let n = reader
            .read_line(&mut baris)
            .await
            .map_err(|e| format!("baca header LSP gagal: {e}"))?;
        if n == 0 {
            return Ok(None);
        }
        let baris = baris.trim_end_matches(['\r', '\n']);
        if baris.is_empty() {
            break;
        }
        if let Some((kunci, nilai)) = baris.split_once(':')
            && kunci.eq_ignore_ascii_case("content-length")
        {
            panjang = nilai.trim().parse::<usize>().ok();
        }
    }
    let Some(n) = panjang else {
        return Err("header Content-Length tidak ada".to_string());
    };
    if n > 8 * 1024 * 1024 {
        return Err(format!("pesan LSP terlalu besar ({n} byte)"));
    }
    let mut buf = vec![0u8; n];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("baca body LSP gagal: {e}"))?;
    serde_json::from_slice(&buf)
        .map(Some)
        .map_err(|e| format!("JSON LSP invalid: {e}"))
}

#[async_trait]
impl Tool for LspTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "lsp".to_string(),
            description: "Diagnostics/references/definition/rename lewat server LSP eksternal."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["diagnostics", "references", "definition", "rename", "hover"]
                    },
                    "path": { "type": "string" },
                    "line": { "type": "integer", "description": "Baris 0-based" },
                    "character": { "type": "integer", "description": "Kolom 0-based" },
                    "new_name": { "type": "string", "description": "Nama baru untuk rename" }
                },
                "required": ["action", "path", "line", "character"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(action) = args.get("action").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen action wajib"));
        };
        if !matches!(
            action,
            "diagnostics" | "references" | "definition" | "rename" | "hover"
        ) {
            return Ok(ToolOutput::err(format!(
                "action LSP '{action}' tidak dikenal"
            )));
        }
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen path wajib"));
        };
        let baris = args.get("line").and_then(Value::as_u64).unwrap_or(0);
        let kolom = args.get("character").and_then(Value::as_u64).unwrap_or(0);
        let uri = Self::file_uri(&ctx.cwd, path);
        let isi = std::fs::read_to_string(ctx.cwd.join(path)).unwrap_or_default();
        let (metode, params): (&str, Value) = match action {
            "diagnostics" => (
                "textDocument/diagnostic",
                json!({"textDocument": {"uri": uri}}),
            ),
            "references" => (
                "textDocument/references",
                json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": baris, "character": kolom},
                    "context": {"includeDeclaration": true}
                }),
            ),
            "definition" | "hover" => (
                if action == "definition" {
                    "textDocument/definition"
                } else {
                    "textDocument/hover"
                },
                json!({"textDocument": {"uri": uri}, "position": {"line": baris, "character": kolom}}),
            ),
            "rename" => {
                let Some(nama) = args.get("new_name").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("rename butuh argumen new_name"));
                };
                (
                    "textDocument/rename",
                    json!({
                        "textDocument": {"uri": uri},
                        "position": {"line": baris, "character": kolom},
                        "newName": nama
                    }),
                )
            }
            _ => return Ok(ToolOutput::err("action LSP tidak didukung")),
        };

        let mut child = match tokio::process::Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                return Ok(ToolOutput::err(format!(
                    "server LSP '{}' tidak bisa dijalankan: {err}",
                    self.command
                )));
            }
        };
        use tokio::io::{AsyncWriteExt, BufReader};
        let mut stdin = match child.stdin.take() {
            Some(v) => v,
            None => {
                return Ok(ToolOutput::err(
                    "stdin server LSP tidak tersedia".to_string(),
                ));
            }
        };
        let stdout = match child.stdout.take() {
            Some(v) => v,
            None => {
                return Ok(ToolOutput::err(
                    "stdout server LSP tidak tersedia".to_string(),
                ));
            }
        };
        let mut reader = BufReader::new(stdout);

        async fn tulis(
            stdin: &mut tokio::process::ChildStdin,
            value: &Value,
        ) -> Result<(), String> {
            let isi = serde_json::to_string(value).map_err(|e| e.to_string())?;
            let frame = format!("Content-Length: {}\r\n\r\n{isi}", isi.len());
            stdin
                .write_all(frame.as_bytes())
                .await
                .map_err(|e| e.to_string())
        }
        let galat = |pesan: String| Ok(ToolOutput::err(pesan));
        if tulis(
            &mut stdin,
            &json!({
                "jsonrpc":"2.0","id":1,"method":"initialize",
                "params":{
                    "processId": std::process::id(),
                    "rootUri": Self::file_uri(&ctx.cwd, "."),
                    "capabilities": {"textDocument": {"diagnostic": {}}}
                }
            }),
        )
        .await
        .is_err()
        {
            return galat("gagal menulis initialize LSP".to_string());
        }
        let mut hasil: Option<Value> = None;
        let mut diagnostics_notif: Vec<Value> = Vec::new();
        loop {
            let pesan = tokio::select! {
                () = ctx.cancel.cancelled() => return galat("[dibatalkan] server LSP dihentikan".to_string()),
                hasil_baca = tokio::time::timeout(self.timeout, baca(&mut reader)) => match hasil_baca {
                    Ok(Ok(Some(v))) => v,
                    Ok(Ok(None)) => break,
                    Ok(Err(err)) => return galat(err),
                    Err(_) => return galat(format!("server LSP melewati batas {:?}", self.timeout)),
                },
            };
            if pesan.get("id").and_then(Value::as_u64) == Some(1) {
                if tulis(
                    &mut stdin,
                    &json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
                )
                .await
                .is_err()
                {
                    return galat("gagal mengirim initialized".to_string());
                }
                if tulis(
                    &mut stdin,
                    &json!({
                        "jsonrpc":"2.0","method":"textDocument/didOpen",
                        "params":{"textDocument":{"uri":uri,"languageId":"rust","version":1,"text":isi}}
                    }),
                )
                .await
                .is_err()
                {
                    return galat("gagal mengirim didOpen".to_string());
                }
                if tulis(
                    &mut stdin,
                    &json!({"jsonrpc":"2.0","id":2,"method":metode,"params":params}),
                )
                .await
                .is_err()
                {
                    return galat("gagal mengirim permintaan LSP".to_string());
                }
                continue;
            }
            if pesan.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
            {
                if let Some(items) = pesan
                    .get("params")
                    .and_then(|p| p.get("diagnostics"))
                    .and_then(Value::as_array)
                {
                    diagnostics_notif = items.clone();
                }
                continue;
            }
            if pesan.get("id").and_then(Value::as_u64) == Some(2) {
                hasil = Some(pesan);
                break;
            }
        }
        let Some(pesan) = hasil else {
            return galat("server LSP menutup koneksi sebelum menjawab".to_string());
        };
        if let Some(err) = pesan.get("error") {
            return galat(format!("LSP error: {err}"));
        }
        let result = pesan.get("result").cloned().unwrap_or(Value::Null);
        if action == "diagnostics" {
            let items = result
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_else(|| diagnostics_notif.clone());
            let baris = Self::ringkas_diagnostic(&items);
            return Ok(ToolOutput::ok(if baris.is_empty() {
                "tidak ada diagnostic".to_string()
            } else {
                baris.join("\n")
            }));
        }
        Ok(ToolOutput::ok(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn uri_file_persen_encode() {
        let root = PathBuf::from("/proyek");
        assert_eq!(
            LspTool::file_uri(&root, "src/a b.rs"),
            "file:///proyek/src/a%20b.rs"
        );
    }

    #[test]
    fn ringkas_diagnostic_memakai_baris_1_based() {
        let items = vec![json!({
            "range": {"start": {"line": 4, "character": 2}},
            "severity": 1,
            "code": "E0001",
            "message": "salah"
        })];
        assert_eq!(
            LspTool::ringkas_diagnostic(&items),
            vec!["error:5 E0001 — salah".to_string()]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn diagnostik_dari_server_mock() {
        // Mock LSP ditulis di Python: `read` shell bisa membaca melewati batas
        // frame sehingga balasan kedua hilang saat piped.
        let skrip = r#"
import sys, json

def baca():
    header = {}
    while True:
        baris = sys.stdin.buffer.readline()
        if not baris:
            return None
        if baris in (b"\r\n", b"\n"):
            break
        k, v = baris.decode().split(":", 1)
        header[k.strip().lower()] = int(v.strip())
    return json.loads(sys.stdin.buffer.read(header["content-length"]))

def kirim(obj):
    isi = json.dumps(obj).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(isi)}\r\n\r\n".encode() + isi)
    sys.stdout.buffer.flush()

while True:
    pesan = baca()
    if pesan is None:
        break
    metode = pesan.get("method")
    if metode == "initialize":
        kirim({"jsonrpc": "2.0", "id": 1, "result": {"capabilities": {}}})
    elif metode == "textDocument/diagnostic":
        kirim({"jsonrpc": "2.0", "id": 2, "result": {"items": [
            {"range": {"start": {"line": 0, "character": 0}},
             "severity": 1, "message": "mock error"}]}})
"#;

        let tool = LspTool::new("python3", vec!["-c".to_string(), skrip.to_string()], 5)
            .expect("tool valid");
        let ctx = ToolContext::new(std::env::temp_dir());
        let out = tool
            .execute(
                &json!({"action":"diagnostics","path":"x.rs","line":0,"character":0}),
                &ctx,
            )
            .await
            .expect("tool jalan");
        assert!(!out.is_error, "{}", out.content);
        assert!(out.content.contains("mock error"), "{}", out.content);
    }
}
