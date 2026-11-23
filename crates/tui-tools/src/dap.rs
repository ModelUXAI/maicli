//! Tool `debug`: klien Debug Adapter Protocol (DAP) generik lewat stdio.
//!
//! Server DAP ditentukan config `[debug]` (mis. `debugpy --stdio`, `dlv dap`,
//! `lldb-dap`). Tool hanya mengirim permintaan standar: initialize →
//! attach/launch → aksi (breakpoints/stack/evaluate/continue/disconnect).

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

/// Klien DAP satu adapter.
#[derive(Clone)]
pub struct DapTool {
    command: String,
    args: Vec<String>,
    timeout: Duration,
}

impl DapTool {
    /// `None` bila perintah kosong.
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
}

/// Tulis pesan DAP (framing identik dengan LSP).
async fn tulis(stdin: &mut tokio::process::ChildStdin, value: &Value) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let isi = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let frame = format!("Content-Length: {}\r\n\r\n{isi}", isi.len());
    stdin
        .write_all(frame.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

/// Bangun argumen untuk satu aksi DAP.
pub fn args_aksi(args: &Value, aksi: &str) -> Value {
    match aksi {
        "breakpoints" => {
            let source = args.get("source").and_then(Value::as_str).unwrap_or("");
            let lines: Vec<u64> = args
                .get("lines")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            json!({
                "source": {"path": source},
                "breakpoints": lines.iter().map(|n| json!({"line": n})).collect::<Vec<_>>()
            })
        }
        "stack" => json!({
            "threadId": args.get("thread_id").and_then(Value::as_u64).unwrap_or(1),
            "startFrame": 0,
            "levels": 20
        }),
        "evaluate" => json!({
            "expression": args.get("expression").and_then(Value::as_str).unwrap_or(""),
            "frameId": args.get("frame_id").and_then(Value::as_u64).unwrap_or(0),
            "context": "repl"
        }),
        "continue" => json!({
            "threadId": args.get("thread_id").and_then(Value::as_u64).unwrap_or(1)
        }),
        _ => json!({}),
    }
}

/// Metode DAP untuk satu aksi.
fn metode(aksi: &str) -> &'static str {
    match aksi {
        "breakpoints" => "setBreakpoints",
        "stack" => "stackTrace",
        "evaluate" => "evaluate",
        "continue" => "continue",
        "disconnect" => "disconnect",
        _ => "",
    }
}

#[async_trait]
impl Tool for DapTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "debug".to_string(),
            description: "Kontrol debugger lewat DAP: breakpoints/stack/evaluate/continue."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["breakpoints", "stack", "evaluate", "continue", "disconnect"]
                    },
                    "mode": { "type": "string", "enum": ["attach", "launch"], "description": "Default attach" },
                    "program": { "type": "string" },
                    "source": { "type": "string" },
                    "lines": { "type": "array", "items": { "type": "integer" } },
                    "thread_id": { "type": "integer" },
                    "frame_id": { "type": "integer" },
                    "expression": { "type": "string" }
                },
                "required": ["action"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(aksi) = args.get("action").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen action wajib"));
        };
        if metode(aksi).is_empty() {
            return Ok(ToolOutput::err(format!("aksi DAP '{aksi}' tidak dikenal")));
        }
        let mode = args.get("mode").and_then(Value::as_str).unwrap_or("attach");
        if !matches!(mode, "attach" | "launch") {
            return Ok(ToolOutput::err("mode harus attach atau launch"));
        }
        let mut child = match tokio::process::Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(v) => v,
            Err(err) => {
                return Ok(ToolOutput::err(format!(
                    "adapter DAP '{}' tidak bisa dijalankan: {err}",
                    self.command
                )));
            }
        };
        use tokio::io::BufReader;
        let mut stdin = match child.stdin.take() {
            Some(v) => v,
            None => {
                return Ok(ToolOutput::err(
                    "stdin adapter DAP tidak tersedia".to_string(),
                ));
            }
        };
        let stdout = match child.stdout.take() {
            Some(v) => v,
            None => {
                return Ok(ToolOutput::err(
                    "stdout adapter DAP tidak tersedia".to_string(),
                ));
            }
        };
        let mut reader = BufReader::new(stdout);
        let galat = |pesan: String| Ok(ToolOutput::err(pesan));
        if tulis(
            &mut stdin,
            &json!({
                "seq":1,"type":"request","command":"initialize",
                "arguments":{
                    "clientID":"maicli","adapterID":"debug",
                    "linesStartAt1":true,"columnsStartAt1":true,"pathFormat":"path"
                }
            }),
        )
        .await
        .is_err()
        {
            return galat("gagal initialize DAP".to_string());
        }
        let mut hasil: Option<Value> = None;
        let mut diberhentikan: Vec<Value> = Vec::new();
        loop {
            let pesan = tokio::select! {
                () = ctx.cancel.cancelled() => return galat("[dibatalkan] adapter DAP dihentikan".to_string()),
                h = tokio::time::timeout(self.timeout, crate::lsp::baca(&mut reader)) => match h {
                    Ok(Ok(Some(v))) => v,
                    Ok(Ok(None)) => break,
                    Ok(Err(err)) => return galat(err),
                    Err(_) => return galat(format!("adapter DAP melewati batas {:?}", self.timeout)),
                },
            };
            if pesan.get("event").and_then(Value::as_str) == Some("stopped") {
                diberhentikan.push(pesan);
                continue;
            }
            if pesan.get("type").and_then(Value::as_str) == Some("response")
                && pesan.get("request_seq").and_then(Value::as_u64) == Some(1)
            {
                let mut argumen = json!({
                    "program": args.get("program").and_then(Value::as_str).unwrap_or(""),
                    "cwd": ctx.cwd.display().to_string(),
                    "stopOnEntry": false
                });
                if mode == "launch" {
                    argumen["console"] = json!("internalConsole");
                }
                if tulis(
                    &mut stdin,
                    &json!({"seq":2,"type":"request","command":mode,"arguments":argumen}),
                )
                .await
                .is_err()
                {
                    return galat("gagal mengirim attach/launch DAP".to_string());
                }
                continue;
            }
            if pesan.get("type").and_then(Value::as_str) == Some("response")
                && pesan.get("request_seq").and_then(Value::as_u64) == Some(2)
            {
                if pesan.get("success") == Some(&Value::Bool(false)) {
                    return galat(format!(
                        "DAP attach/launch gagal: {}",
                        pesan.get("message").and_then(Value::as_str).unwrap_or("")
                    ));
                }
                if tulis(
                    &mut stdin,
                    &json!({
                        "seq":3,"type":"request","command":metode(aksi),
                        "arguments":args_aksi(args, aksi)
                    }),
                )
                .await
                .is_err()
                {
                    return galat("gagal mengirim aksi DAP".to_string());
                }
                continue;
            }
            if pesan.get("type").and_then(Value::as_str) == Some("response")
                && pesan.get("request_seq").and_then(Value::as_u64) == Some(3)
            {
                hasil = Some(pesan);
                break;
            }
        }
        let Some(pesan) = hasil else {
            return galat("adapter DAP menutup koneksi sebelum menjawab".to_string());
        };
        if pesan.get("success") == Some(&Value::Bool(false)) {
            return galat(format!(
                "DAP error: {}",
                pesan.get("message").and_then(Value::as_str).unwrap_or("")
            ));
        }
        let isi = json!({
            "result": pesan.get("body").cloned().unwrap_or(Value::Null),
            "stopped": diberhentikan,
        });
        Ok(ToolOutput::ok(
            serde_json::to_string_pretty(&isi).unwrap_or_default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn argumen_breakpoints_dan_evaluate() {
        let bp = args_aksi(&json!({"source":"a.rs","lines":[3,7]}), "breakpoints");
        assert_eq!(bp["breakpoints"][1]["line"], 7);
        let ev = args_aksi(&json!({"expression":"x + 1","frame_id":2}), "evaluate");
        assert_eq!(ev["expression"], "x + 1");
        assert_eq!(ev["frameId"], 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn session_dap_mock() {
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

seq = 0
while True:
    pesan = baca()
    if pesan is None:
        break
    seq += 1
    respon = {"seq": seq, "type": "response", "request_seq": pesan["seq"],
              "command": pesan["command"], "success": True, "body": {}}
    if pesan["command"] == "setBreakpoints":
        respon["body"] = {"breakpoints": [{"verified": True, "line": 3}]}
    kirim(respon)
"#;
        let tool = DapTool::new("python3", vec!["-c".to_string(), skrip.to_string()], 5)
            .expect("tool valid");
        let ctx = ToolContext::new(std::env::temp_dir());
        let hasil = tool
            .execute(
                &json!({"action":"breakpoints","source":"a.rs","lines":[3]}),
                &ctx,
            )
            .await
            .expect("tool jalan");
        assert!(!hasil.is_error, "{}", hasil.content);
        assert!(hasil.content.contains("verified"), "{}", hasil.content);
    }
}
