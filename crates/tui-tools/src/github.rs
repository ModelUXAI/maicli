//! Tool `github` + skema `pr://` / `issue://` untuk satu antarmuka `read`.
//!
//! Memakai GitHub REST API v3. Token opsional dari env `GITHUB_TOKEN` (nilai tidak
//! pernah ditampilkan); tanpa token hanya data publik yang bisa diambil.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

const API: &str = "https://api.github.com";
const TIMEOUT: Duration = Duration::from_secs(20);

/// Target `pr://owner/repo/12` atau `issue://owner/repo/12`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `pulls` atau `issues`.
    pub kind: &'static str,
    pub owner: String,
    pub repo: String,
    pub number: String,
}

/// Parse URI `pr://…` / `issue://…`.
pub fn parse_uri(uri: &str) -> Option<Target> {
    let (skema, sisa) = uri.split_once("://")?;
    let kind = match skema {
        "pr" => "pulls",
        "issue" => "issues",
        _ => return None,
    };
    let mut bagian = sisa.trim_end_matches('/').split('/');
    let owner = bagian.next()?.to_string();
    let repo = bagian.next()?.to_string();
    let number = bagian.next()?.to_string();
    if owner.is_empty()
        || repo.is_empty()
        || number.is_empty()
        || !number.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(Target {
        kind,
        owner,
        repo,
        number,
    })
}

/// Ringkas respons GitHub menjadi teks ringkas untuk model.
pub fn ringkas(kind: &str, body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("JSON GitHub invalid: {e}"))?;
    if let Some(pesan) = v.get("message").and_then(Value::as_str)
        && !pesan.is_empty()
    {
        return Err(format!("GitHub: {pesan}"));
    }
    let judul = v.get("title").and_then(Value::as_str).unwrap_or("");
    let state = v.get("state").and_then(Value::as_str).unwrap_or("");
    let url = v.get("html_url").and_then(Value::as_str).unwrap_or("");
    let badan = v.get("body").and_then(Value::as_str).unwrap_or("");
    let potong: String = badan.chars().take(4000).collect();
    Ok(format!(
        "{kind} #{n} — {judul} [{state}]\n{url}\n\n{potong}",
        n = v
            .get("number")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .unwrap_or_default()
    ))
}

/// Ambil satu PR/issue dari base URL tertentu (base dapat diganti test).
pub async fn ambil(base: &str, target: &Target) -> Result<String, String> {
    let mut req = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent("maicli-github/1.0")
        .build()
        .map_err(|e| e.to_string())?
        .get(format!(
            "{}/repos/{}/{}/{}/{}",
            base.trim_end_matches('/'),
            target.owner,
            target.repo,
            target.kind,
            target.number
        ));
    if let Ok(token) = std::env::var("GITHUB_TOKEN")
        && !token.trim().is_empty()
    {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    let respon = req
        .send()
        .await
        .map_err(|e| format!("gagal menghubungi GitHub: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GitHub HTTP error: {e}"))?;
    ringkas(
        target.kind,
        &respon.text().await.map_err(|e| e.to_string())?,
    )
}

/// Baca `pr://` / `issue://` dari tool `read` (satu antarmuka).
pub async fn baca_uri(uri: &str) -> Result<String, String> {
    let target = parse_uri(uri).ok_or_else(|| format!("URI GitHub tidak valid: {uri}"))?;
    ambil(API, &target).await
}

/// Tool `github` (action pr/issue).
pub struct GithubTool;

#[async_trait]
impl Tool for GithubTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "github".to_string(),
            description: "Baca PR/issue GitHub (token opsional dari env GITHUB_TOKEN).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["pr", "issue"] },
                    "owner": { "type": "string" },
                    "repo": { "type": "string" },
                    "number": { "type": "string" }
                },
                "required": ["action", "owner", "repo", "number"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let _ = ctx;
        let action = args.get("action").and_then(Value::as_str).unwrap_or("");
        let owner = args.get("owner").and_then(Value::as_str).unwrap_or("");
        let repo = args.get("repo").and_then(Value::as_str).unwrap_or("");
        let number = args
            .get("number")
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
            .unwrap_or_default();
        let uri = format!("{action}://{owner}/{repo}/{number}");
        match parse_uri(&uri) {
            Some(target) => match ambil(API, &target).await {
                Ok(teks) => Ok(ToolOutput::ok(teks)),
                Err(err) => Ok(ToolOutput::err(err)),
            },
            None => Ok(ToolOutput::err(format!("target GitHub tidak valid: {uri}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn parse_uri_pr_dan_issue() {
        assert_eq!(
            parse_uri("pr://modeluxai/maicli/12"),
            Some(Target {
                kind: "pulls",
                owner: "modeluxai".to_string(),
                repo: "maicli".to_string(),
                number: "12".to_string()
            })
        );
        assert_eq!(parse_uri("issue://o/r/7").map(|t| t.kind), Some("issues"));
        assert!(parse_uri("pr://o/r/x").is_none());
    }

    #[test]
    fn ringkas_respons_github() {
        let body = r#"{"number":12,"title":"Perbaikan","state":"open","html_url":"https://github.com/o/r/pull/12","body":"detail"}"#;
        let teks = ringkas("pulls", body).unwrap();
        assert!(teks.contains("Perbaikan"));
        assert!(teks.contains("https://github.com/o/r/pull/12"));
    }

    #[tokio::test]
    async fn ambil_dari_server_lokal() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf).await;
                let body = r#"{"number":7,"title":"Lokal","state":"closed","body":"x"}"#;
                let respon = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(respon.as_bytes()).await;
            }
        });
        let target = Target {
            kind: "issues",
            owner: "o".to_string(),
            repo: "r".to_string(),
            number: "7".to_string(),
        };
        let teks = ambil(&format!("http://{addr}"), &target).await.unwrap();
        assert!(teks.contains("Lokal"), "{teks}");
    }
}
