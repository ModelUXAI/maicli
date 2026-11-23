//! Tool `web_search`: pencarian web tanpa dependensi tambahan.
//!
//! Backend default = DuckDuckGo HTML (tanpa kunci). Backend `brave` memakai
//! `BRAVE_SEARCH_API_KEY` dari env (nilai kunci tidak pernah dicatat/ditampilkan).
//! Test memakai server HTTP loopback dan parser murni, bukan jaringan publik.

use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Satu hasil pencarian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hasil {
    /// Judul halaman.
    pub judul: String,
    /// URL hasil.
    pub url: String,
    /// Ringkasan singkat.
    pub cuplikan: String,
}

/// Ambil hasil dari URL yang diberikan (dipakai test & backend HTML).
pub async fn ambil_html(url: &str, query: &str, batas: usize) -> Result<Vec<Hasil>, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent("maicli-web-search/1.0")
        .build()
        .map_err(|e| format!("gagal membuat klien HTTP: {e}"))?;
    let respon = client
        .get(url)
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("gagal mencari: {e}"))?
        .error_for_status()
        .map_err(|e| format!("search HTTP error: {e}"))?;
    let html = respon
        .text()
        .await
        .map_err(|e| format!("gagal membaca hasil: {e}"))?;
    Ok(parse_ddg(&html, batas))
}

/// Parse blok hasil DuckDuckGo HTML.
pub fn parse_ddg(html: &str, batas: usize) -> Vec<Hasil> {
    let link =
        Regex::new(r#"(?is)<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#);
    let cuplikan = Regex::new(r#"(?is)<a[^>]*class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>"#);
    let (Ok(link), Ok(cuplikan)) = (link, cuplikan) else {
        return Vec::new();
    };
    link.captures_iter(html)
        .take(batas)
        .enumerate()
        .map(|(i, caps)| {
            let bersih = |s: &str| {
                Regex::new(r"(?s)<[^>]+>")
                    .map(|re| re.replace_all(s, "").into_owned())
                    .unwrap_or_else(|_| s.to_string())
                    .replace("&amp;", "&")
                    .replace("&quot;", "\"")
                    .replace("&#x27;", "'")
                    .trim()
                    .to_string()
            };
            Hasil {
                judul: caps.get(2).map(|m| bersih(m.as_str())).unwrap_or_default(),
                url: caps
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                cuplikan: cuplikan
                    .captures_iter(html)
                    .nth(i)
                    .and_then(|c| c.get(1))
                    .map(|m| bersih(m.as_str()))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// Parse respons JSON Brave Search.
pub fn parse_brave(body: &str, batas: usize) -> Vec<Hasil> {
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    root.get("web")
        .and_then(|w| w.get("results"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(batas)
                .map(|v| Hasil {
                    judul: v
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    url: v
                        .get("url")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    cuplikan: v
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Cari lewat backend pilihan.
pub async fn cari(query: &str, batas: usize, backend: &str) -> Result<Vec<Hasil>, String> {
    if query.trim().is_empty() {
        return Err("query tidak boleh kosong".to_string());
    }
    match backend {
        "duckduckgo" | "" => ambil_html("https://html.duckduckgo.com/html/", query, batas).await,
        "brave" => {
            let kunci = std::env::var("BRAVE_SEARCH_API_KEY").unwrap_or_default();
            if kunci.trim().is_empty() {
                return Err("backend brave butuh env BRAVE_SEARCH_API_KEY".to_string());
            }
            let client = reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .map_err(|e| e.to_string())?;
            let respon = client
                .get("https://api.search.brave.com/res/v1/web/search")
                .query(&[("q", query), ("count", &batas.to_string())])
                .header("X-Subscription-Token", kunci)
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| format!("brave HTTP error: {e}"))?;
            Ok(parse_brave(
                &respon.text().await.map_err(|e| e.to_string())?,
                batas,
            ))
        }
        lain => Err(format!("backend '{lain}' tidak dikenal (duckduckgo|brave)")),
    }
}

/// Tool `web_search`.
pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "Cari web via DuckDuckGo HTML atau Brave (env BRAVE_SEARCH_API_KEY)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "description": "1..10, default 5" },
                    "backend": { "type": "string", "enum": ["duckduckgo", "brave"] }
                },
                "required": ["query"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(query) = args.get("query").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen query wajib"));
        };
        let batas = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 10) as usize;
        let backend = args
            .get("backend")
            .and_then(Value::as_str)
            .unwrap_or("duckduckgo");
        let hasil = {
            let _ = ctx; // cap keluaran mengikuti hasil pencarian (maks 10)
            cari(query, batas, backend).await
        };
        match hasil {
            Ok(items) if items.is_empty() => Ok(ToolOutput::ok("tidak ada hasil")),
            Ok(items) => {
                let baris: Vec<String> = items
                    .iter()
                    .map(|h| {
                        format!(
                            "- {} — {}\n  {}{}",
                            h.judul,
                            h.url,
                            h.cuplikan,
                            if h.cuplikan.is_empty() { "" } else { "\n" }
                        )
                    })
                    .collect();
                Ok(ToolOutput::ok(baris.join("\n")))
            }
            Err(err) => Ok(ToolOutput::err(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn parse_hasil_ddg() {
        let html = r#"
        <a class="result__a" href="https://example.com/a">Judul &amp; A</a>
        <a class="result__snippet">Cuplikan <b>satu</b></a>
        <a class="result__a" href="https://example.com/b">Judul B</a>
        <a class="result__snippet">Cuplikan dua</a>
        "#;
        let hasil = parse_ddg(html, 5);
        assert_eq!(hasil.len(), 2);
        assert_eq!(hasil[0].judul, "Judul & A");
        assert_eq!(hasil[0].cuplikan, "Cuplikan satu");
        assert_eq!(hasil[1].url, "https://example.com/b");
    }

    #[test]
    fn parse_hasil_brave() {
        let body =
            r#"{"web":{"results":[{"title":"T","url":"https://x.test","description":"D"}]}}"#;
        let hasil = parse_brave(body, 5);
        assert_eq!(hasil.len(), 1);
        assert_eq!(hasil[0].judul, "T");
    }

    #[tokio::test]
    async fn ambil_html_dari_server_lokal() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 512];
                let _ = sock.read(&mut buf).await;
                let body = r#"<a class="result__a" href="https://lokal.test">Lokal</a>"#;
                let respon = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(respon.as_bytes()).await;
            }
        });
        let hasil = ambil_html(&format!("http://{addr}/"), "maicli", 5)
            .await
            .unwrap();
        assert_eq!(hasil.len(), 1);
        assert_eq!(hasil[0].judul, "Lokal");
    }
}
