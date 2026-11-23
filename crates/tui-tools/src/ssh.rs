//! Tool `ssh`: eksekusi perintah di host yang terdaftar di config.
//!
//! Hanya host bernama (bukan target bebas dari model), `BatchMode=yes` supaya
//! tidak pernah menunggu password, timeout + cap output, dan tier Exec sehingga
//! selalu lewat approval.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ErrorCode, MxError, ToolDefinition, ToolOutput, ToolTier};

use crate::{Tool, ToolContext};

/// Satu host SSH yang diizinkan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshHost {
    /// Target `user@host` atau `host`.
    pub target: String,
    /// Port SSH.
    pub port: u16,
    /// Path kunci privat (kosong = default ssh/agent).
    pub identity: String,
    /// Batas waktu koneksi/perintah (detik).
    pub timeout_secs: u64,
}

fn galat(pesan: impl Into<String>) -> MxError {
    MxError::new(ErrorCode::Config, pesan.into())
}

fn batas_teks(teks: &str, cap: usize) -> String {
    if teks.len() <= cap {
        return teks.to_string();
    }
    let mut end = cap;
    while end > 0 && !teks.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…(dipotong, {} byte)", &teks[..end], teks.len())
}

/// Tool SSH.
pub struct SshTool {
    hosts: BTreeMap<String, SshHost>,
}

impl SshTool {
    /// Bangun tool dari daftar host.
    pub fn new(hosts: BTreeMap<String, SshHost>) -> Self {
        Self { hosts }
    }

    /// Daftar nama host.
    pub fn names(&self) -> Vec<String> {
        self.hosts.keys().cloned().collect()
    }

    fn host(&self, name: &str) -> Result<&SshHost, MxError> {
        self.hosts.get(name).ok_or_else(|| {
            let tersedia = self.names().join(", ");
            galat(format!(
                "host SSH '{name}' tidak terdaftar; pilihan: {tersedia}"
            ))
        })
    }
}

/// Argumen `ssh` untuk satu host dan perintah (murni, mudah diuji).
pub fn build_args(host: &SshHost, command: &str) -> Vec<String> {
    let mut args = vec![
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        format!("ConnectTimeout={}", host.timeout_secs.clamp(1, 60)),
        "-p".to_string(),
        host.port.to_string(),
    ];
    if !host.identity.is_empty() {
        args.push("-i".to_string());
        args.push(host.identity.clone());
    }
    args.push(host.target.clone());
    args.push(command.to_string());
    args
}

fn env_kredensial(nama: &str) -> bool {
    let upper = nama.to_ascii_uppercase();
    upper.contains("KEY")
        || upper.contains("TOKEN")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
}

#[async_trait]
impl Tool for SshTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ssh".to_string(),
            description:
                "Jalankan satu perintah di host SSH yang terdaftar di config [ssh.hosts]. \
                          Host harus bernama; password interaktif tidak didukung (BatchMode)."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "Nama host di config" },
                    "command": { "type": "string", "description": "Perintah shell di host" },
                    "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 600 }
                },
                "required": ["host", "command"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Exec
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(name) = args.get("host").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("field host wajib diisi"));
        };
        let Some(command) = args.get("command").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("field command wajib diisi"));
        };
        if command.trim().is_empty() {
            return Ok(ToolOutput::err("command tidak boleh kosong"));
        }
        if command.contains('\0') {
            return Ok(ToolOutput::err("command mengandung NUL"));
        }
        let host = match self.host(name) {
            Ok(host) => host,
            Err(err) => return Ok(ToolOutput::err(format!("{}: {}", err.code, err.message))),
        };
        let timeout = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(host.timeout_secs)
            .clamp(1, 600);
        let mut process = tokio::process::Command::new("ssh");
        process
            .args(build_args(host, command))
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null());
        for (nama, _) in std::env::vars() {
            if env_kredensial(&nama) {
                process.env_remove(&nama);
            }
        }
        let hasil = tokio::select! {
            () = ctx.cancel.cancelled() => {
                return Ok(ToolOutput::err("[dibatalkan] ssh dihentikan"));
            }
            hasil = tokio::time::timeout(Duration::from_secs(timeout), process.output()) => hasil,
        };
        let output = match hasil {
            Err(_) => {
                return Ok(ToolOutput::err(format!(
                    "ssh melewati batas {timeout} detik"
                )));
            }
            Ok(Err(err)) => {
                return Ok(ToolOutput::err(format!("ssh tidak bisa dijalankan: {err}")));
            }
            Ok(Ok(output)) => output,
        };
        let mut teks = String::from_utf8_lossy(&output.stdout).to_string();
        let err = String::from_utf8_lossy(&output.stderr);
        if !err.trim().is_empty() {
            if !teks.is_empty() {
                teks.push('\n');
            }
            teks.push_str(&err);
        }
        let cap = ctx.output_cap.max(1_024);
        let teks = batas_teks(teks.trim_end(), cap);
        if output.status.success() {
            Ok(ToolOutput::ok(if teks.is_empty() {
                "(perintah selesai tanpa output)".to_string()
            } else {
                teks
            }))
        } else {
            Ok(ToolOutput::err(format!(
                "ssh keluar {}: {teks}",
                output.status
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn host() -> SshHost {
        SshHost {
            target: "deploy@contoh".to_string(),
            port: 2222,
            identity: "/home/u/.ssh/id".to_string(),
            timeout_secs: 30,
        }
    }

    #[test]
    fn argumen_memaksa_batchmode_dan_port() {
        let args = build_args(&host(), "uptime");
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-o" && w[1] == "BatchMode=yes")
        );
        assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-i" && w[1] == "/home/u/.ssh/id")
        );
        assert_eq!(args[args.len() - 2], "deploy@contoh");
        assert_eq!(args[args.len() - 1], "uptime");
    }

    #[tokio::test]
    async fn host_tidak_terdaftar_dilaporkan() {
        let tool = SshTool::new(BTreeMap::new());
        let ctx = ToolContext::new(std::env::temp_dir());
        let hasil = tool
            .execute(&json!({"host":"x","command":"true"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(hasil.is_error);
        assert!(hasil.content.contains("tidak terdaftar"));
    }

    #[test]
    fn batas_teks_tidak_memotong_utf8() {
        let teks = "é".repeat(100);
        let hasil = batas_teks(&teks, 6);
        assert!(hasil.starts_with("ééé"), "{hasil}");
    }
}
