//! Tool `browser`: kendali minimal Chromium/Electron lewat CDP HTTP.
//!
//! Mendukung daftar tab, buka tab, dan tutup tab melalui endpoint DevTools
//! lokal (default `http://127.0.0.1:9222`). Evaluasi JS lewat WebSocket belum
//! termasuk di sini.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:9222";

/// Ringkas daftar tab CDP menjadi baris teks.
pub fn ringkas_tabs(body: &str) -> Result<String, String> {
    let nilai: Value = serde_json::from_str(body).map_err(|e| format!("JSON CDP invalid: {e}"))?;
    let Some(item) = nilai.as_array() else {
        return Err("respons /json/list bukan array".to_string());
    };
    let baris: Vec<String> = item
        .iter()
        .map(|t| {
            format!(
                "{} · {} · {}",
                t.get("id").and_then(Value::as_str).unwrap_or("?"),
                t.get("title").and_then(Value::as_str).unwrap_or(""),
                t.get("url").and_then(Value::as_str).unwrap_or("")
            )
        })
        .collect();
    Ok(if baris.is_empty() {
        "tidak ada tab".to_string()
    } else {
        baris.join("\n")
    })
}

/// Ambil alamat WebSocket target pertama bertipe `page`.
pub fn ws_target(body: &str) -> Option<String> {
    let nilai: Value = serde_json::from_str(body).ok()?;
    nilai
        .as_array()?
        .iter()
        .find(|t| t.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|t| t.get("webSocketDebuggerUrl")?.as_str().map(str::to_string))
}

/// Parse balasan `Runtime.evaluate` menjadi nilai teks/error.
pub fn parse_eval(frame: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(frame).map_err(|e| e.to_string())?;
    if let Some(err) = v.get("result").and_then(|r| r.get("exceptionDetails")) {
        return Err(err
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("exception")
            .to_string());
    }
    let hasil = v
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.get("value"));
    match hasil {
        Some(Value::String(t)) => Ok(t.clone()),
        Some(v) => Ok(v.to_string()),
        None => Ok("(tanpa nilai)".to_string()),
    }
}

/// Tool `browser` (HTTP CDP).
pub struct BrowserTool;

#[async_trait]
impl Tool for BrowserTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser".to_string(),
            description: "Daftar/buka/tutup tab browser via CDP HTTP lokal.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["tabs", "new", "close", "evaluate"] },
                    "endpoint": { "type": "string", "description": "Default http://127.0.0.1:9222" },
                    "url": { "type": "string" },
                    "id": { "type": "string" },
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

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(aksi) = args.get("action").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen action wajib"));
        };
        let endpoint = args
            .get("endpoint")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_ENDPOINT)
            .trim_end_matches('/');
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(err) => return Ok(ToolOutput::err(err.to_string())),
        };
        let hasil = match aksi {
            "tabs" => match client.get(format!("{endpoint}/json/list")).send().await {
                Ok(r) => match r.text().await {
                    Ok(t) => ringkas_tabs(&t),
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(format!("CDP tidak tersedia: {e}")),
            },
            "new" => {
                let Some(url) = args.get("url").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action new butuh url"));
                };
                match client
                    .put(format!("{endpoint}/json/new"))
                    .query(&[("url", url)])
                    .send()
                    .await
                {
                    Ok(r) => match r.text().await {
                        Ok(t) => Ok(t),
                        Err(e) => Err(e.to_string()),
                    },
                    Err(e) => Err(format!("CDP tidak tersedia: {e}")),
                }
            }
            "evaluate" => {
                let Some(expr) = args.get("expression").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action evaluate butuh expression"));
                };
                let daftar = match client.get(format!("{endpoint}/json/list")).send().await {
                    Ok(r) => r.text().await.unwrap_or_default(),
                    Err(e) => return Ok(ToolOutput::err(format!("CDP tidak tersedia: {e}"))),
                };
                let Some(ws_url) = ws_target(&daftar) else {
                    return Ok(ToolOutput::err(
                        "tidak ada target page untuk evaluasi".to_string(),
                    ));
                };
                let (mut ws, _) = match tokio_tungstenite::connect_async(ws_url).await {
                    Ok(v) => v,
                    Err(e) => return Ok(ToolOutput::err(format!("CDP WS gagal: {e}"))),
                };
                let permintaan = json!({
                    "id": 1,
                    "method": "Runtime.evaluate",
                    "params": {"expression": expr, "returnByValue": true}
                });
                if ws
                    .send(Message::Text(permintaan.to_string().into()))
                    .await
                    .is_err()
                {
                    return Ok(ToolOutput::err(
                        "gagal mengirim Runtime.evaluate".to_string(),
                    ));
                }
                let mut jawaban = Err("CDP WS ditutup sebelum jawaban".to_string());
                while let Some(Ok(pesan)) = ws.next().await {
                    if let Message::Text(teks) = pesan
                        && teks.contains("\"id\":1")
                    {
                        jawaban = parse_eval(teks.as_str());
                        break;
                    }
                }
                jawaban
            }
            "close" => {
                let Some(id) = args.get("id").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action close butuh id"));
                };
                match client
                    .get(format!("{endpoint}/json/close/{id}"))
                    .send()
                    .await
                {
                    Ok(_) => Ok(format!("tab '{id}' ditutup")),
                    Err(e) => Err(format!("CDP tidak tersedia: {e}")),
                }
            }
            _ => Err(format!("aksi browser '{aksi}' tidak dikenal")),
        };
        match hasil {
            Ok(teks) => {
                let (isi, _) = crate::truncate_bytes(&teks, 16_384);
                Ok(ToolOutput::ok(isi))
            }
            Err(err) => Ok(ToolOutput::err(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn ringkas_daftar_tab() {
        let body = r#"[{"id":"a","title":"Judul","url":"https://x.test"}]"#;
        let teks = ringkas_tabs(body).unwrap();
        assert!(teks.contains("a · Judul · https://x.test"), "{teks}");
    }
    #[tokio::test]
    async fn tabs_dari_server_cdp_lokal() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf).await;
                let body = r#"[{"id":"x","title":"Lokal","url":"http://lokal"}]"#;
                let respon = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(respon.as_bytes()).await;
            }
        });
        let tool = BrowserTool;
        let ctx = ToolContext::new(std::env::temp_dir());
        let out = tool
            .execute(
                &json!({"action":"tabs","endpoint":format!("http://{addr}")}),
                &ctx,
            )
            .await
            .expect("tool jalan");
        assert!(out.content.contains("Lokal"), "{}", out.content);
    }
    #[test]
    fn parse_eval_dan_ws_target() {
        let frame = r#"{"id":1,"result":{"result":{"type":"string","value":"42"}}}"#;
        assert_eq!(parse_eval(frame).unwrap(), "42");
        let daftar = r#"[{"type":"page","webSocketDebuggerUrl":"ws://x"}]"#;
        assert_eq!(ws_target(daftar), Some("ws://x".to_string()));
        assert!(parse_eval(r#"{"id":1,"result":{"exceptionDetails":{"text":"boom"}}}"#).is_err());
    }
}
