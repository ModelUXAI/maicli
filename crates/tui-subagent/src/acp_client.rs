//! Tool `acp_subagent`: memakai **agen ACP eksternal** sebagai subagent.
//!
//! Protokol: JSON-RPC 2.0 line-delimited di stdio (sama dengan mode server ACP
//! kita): `initialize` → `session/new` → `session/prompt`, notifikasi
//! `session/update` dikumpulkan, balasan id prompt dijadikan hasil.
//!
//! Kredensial tidak diwariskan; timeout + cancel membatalkan child. Tool bertier
//! Exec sehingga tetap melewati approval gate induk.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tui_tools::{Tool, ToolContext};
use tui_wire::{Concurrency, ErrorCode, MxError, ToolDefinition, ToolOutput, ToolTier};

/// Satu agen ACP eksternal.
#[derive(Clone)]
pub struct AcpSubagentTool {
    command: String,
    args: Vec<String>,
    timeout: Duration,
}

impl AcpSubagentTool {
    /// Bangun tool dari config; `None` bila tidak diaktifkan/perintah kosong.
    pub fn from_config(config: &tui_config::AcpClientConfig) -> Option<Self> {
        if !config.enabled || config.command.trim().is_empty() {
            return None;
        }
        Some(Self::new(
            &config.command,
            config.args.clone(),
            config.timeout_secs,
        ))
    }

    /// Konstruktor eksplisit (dipakai test & wiring manual).
    pub fn new(command: &str, args: Vec<String>, timeout_secs: u64) -> Self {
        Self {
            command: command.to_string(),
            args,
            timeout: Duration::from_secs(timeout_secs.max(1)),
        }
    }

    /// Ambil teks dari satu notifikasi `session/update` ACP.
    pub fn teks_update(pesan: &Value) -> Option<String> {
        let update = pesan.get("params")?.get("update")?;
        if update.get("sessionUpdate")?.as_str()? != "agent_message_chunk" {
            return None;
        }
        update
            .get("content")?
            .get("text")?
            .as_str()
            .map(str::to_string)
    }
}

#[async_trait]
impl Tool for AcpSubagentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "acp_subagent".to_string(),
            description: "Jalankan prompt di agen ACP eksternal (subagent).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": { "prompt": { "type": "string" } },
                "required": ["prompt"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(prompt) = args.get("prompt").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("field prompt wajib diisi"));
        };
        let mut command = tokio::process::Command::new(&self.command);
        command.args(&self.args);
        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        command.kill_on_drop(true);
        // Kredensial host tidak boleh masuk ke agen pihak ketiga.
        for (nama, _) in std::env::vars() {
            let upper = nama.to_ascii_uppercase();
            if upper.contains("KEY")
                || upper.contains("TOKEN")
                || upper.contains("SECRET")
                || upper.contains("PASSWORD")
            {
                command.env_remove(&nama);
            }
        }
        let mut child = command.spawn().map_err(|e| {
            MxError::new(
                ErrorCode::ToolFailed,
                format!("agen ACP '{}' tidak bisa dijalankan: {e}", self.command),
            )
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| MxError::new(ErrorCode::ToolFailed, "stdin agen ACP tidak tersedia"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MxError::new(ErrorCode::ToolFailed, "stdout agen ACP tidak tersedia"))?;
        let mut reader = BufReader::new(stdout).lines();
        async fn kirim(
            stdin: &mut tokio::process::ChildStdin,
            value: Value,
        ) -> std::io::Result<()> {
            let mut baris = serde_json::to_string(&value).unwrap_or_default();
            baris.push('\n');
            stdin.write_all(baris.as_bytes()).await
        }
        let galat = |pesan: String| Ok(ToolOutput::err(pesan));
        if kirim(
            &mut stdin,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}),
        )
        .await
        .is_err()
        {
            return galat("gagal menulis initialize ke agen ACP".to_string());
        }
        let mut teks = String::new();
        let mut session_id = String::from("acp-1");
        let mut selesai = false;
        loop {
            let baris = tokio::select! {
                () = ctx.cancel.cancelled() => return galat("[dibatalkan] agen ACP dihentikan".to_string()),
                hasil = tokio::time::timeout(self.timeout, reader.next_line()) => match hasil {
                    Ok(Ok(Some(baris))) => baris,
                    Ok(Ok(None)) => break,
                    Ok(Err(err)) => return galat(format!("baca agen ACP gagal: {err}")),
                    Err(_) => return galat(format!("agen ACP melewati batas {:?}", self.timeout)),
                },
            };
            let Ok(pesan) = serde_json::from_str::<Value>(&baris) else {
                continue;
            };
            if let Some(partial) = Self::teks_update(&pesan) {
                teks.push_str(&partial);
                continue;
            }
            if pesan.get("method").and_then(Value::as_str) == Some("session/request_permission") {
                // Agen eksternal tidak punya UI approval di sini → tolak bernama.
                let _ = kirim(
                    &mut stdin,
                    json!({
                        "jsonrpc":"2.0",
                        "id": pesan.get("id").cloned().unwrap_or(Value::Null),
                        "result": {"outcome": {"outcome": "reject"}}
                    }),
                )
                .await;
                continue;
            }
            if pesan.get("id").and_then(Value::as_u64) == Some(1) {
                if kirim(
                    &mut stdin,
                    json!({
                        "jsonrpc":"2.0","id":2,"method":"session/new",
                        "params":{"cwd":ctx.cwd.display().to_string(),"mcpServers":[]}
                    }),
                )
                .await
                .is_err()
                {
                    return galat("gagal session/new ke agen ACP".to_string());
                }
            } else if pesan.get("id").and_then(Value::as_u64) == Some(2) {
                // Agen boleh memilih id sesi sendiri; pakai id dari result.
                if let Some(id) = pesan
                    .get("result")
                    .and_then(|r| r.get("sessionId"))
                    .and_then(Value::as_str)
                    && !id.is_empty()
                {
                    session_id = id.to_string();
                }
                if kirim(
                    &mut stdin,
                    json!({
                        "jsonrpc":"2.0","id":3,"method":"session/prompt",
                        "params":{"sessionId":session_id,"prompt":[{"type":"text","text":prompt}]}
                    }),
                )
                .await
                .is_err()
                {
                    return galat("gagal session/prompt ke agen ACP".to_string());
                }
            } else if pesan.get("id").and_then(Value::as_u64) == Some(3) {
                selesai = true;
                break;
            }
        }
        if !selesai {
            return galat("agen ACP menutup koneksi sebelum prompt selesai".to_string());
        }
        Ok(ToolOutput::ok(if teks.trim().is_empty() {
            "(agen ACP selesai tanpa teks)".to_string()
        } else {
            teks
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn teks_update_hanya_chunk() {
        let pesan = json!({
            "jsonrpc":"2.0","method":"session/update",
            "params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"halo"}}}
        });
        assert_eq!(
            AcpSubagentTool::teks_update(&pesan),
            Some("halo".to_string())
        );
        let lain = json!({"params":{"update":{"sessionUpdate":"tool_call"}}});
        assert_eq!(AcpSubagentTool::teks_update(&lain), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn jalankan_agen_acp_mock() {
        let skrip = r#"while IFS= read -r l; do
  case "$l" in
    *'"method":"initialize"'*) printf '%s
' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}' ;;
    *'"method":"session/new"'*) printf '%s
' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1"}}' ;;
    *'"method":"session/prompt"'*)
      printf '%s
' '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"halo dari mock"}}}}'
      printf '%s
' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
      ;;
  esac
done"#;
        let tool = AcpSubagentTool::new("/bin/sh", vec!["-c".to_string(), skrip.to_string()], 5);
        let ctx = ToolContext::new(std::env::temp_dir());
        let hasil = tool
            .execute(&json!({"prompt": "hai"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(!hasil.is_error, "{}", hasil.content);
        assert!(
            hasil.content.contains("halo dari mock"),
            "{}",
            hasil.content
        );
    }

    #[test]
    fn config_menolak_perintah_kosong() {
        let config = tui_config::AcpClientConfig::default();
        assert!(AcpSubagentTool::from_config(&config).is_none());
        let config = tui_config::AcpClientConfig {
            enabled: true,
            command: "acme-agent".to_string(),
            args: vec!["--stdio".to_string()],
            timeout_secs: 5,
        };
        assert!(AcpSubagentTool::from_config(&config).is_some());
    }
}
