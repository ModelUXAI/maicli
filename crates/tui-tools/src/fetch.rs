//! Tool `fetch`: ambil URL HTTP(S) dan ubah respons teks/HTML jadi markdown.
//!
//! Dipakai juga oleh `read` bila path berupa URL. Batas ukuran dan timeout
//! mencegah respons besar menghabiskan konteks; hanya skema http/https yang
//! diterima (skema host lain tetap lewat `UriResolver` di `read`).

use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

/// Timeout permintaan HTTP.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Ambil satu URL; kembalikan `(content_type, isi_teks)`.
pub async fn ambil(url: &str, max_bytes: usize) -> Result<(String, String), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("fetch hanya menerima URL http:// atau https://".to_string());
    }
    if max_bytes == 0 {
        return Err("batas ukuran harus > 0".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| format!("gagal membuat klien HTTP: {e}"))?;
    let respon = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("gagal mengambil {url}: {e}"))?
        .error_for_status()
        .map_err(|e| format!("HTTP error untuk {url}: {e}"))?;
    let tipe = respon
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/plain")
        .split(';')
        .next()
        .unwrap_or("text/plain")
        .trim()
        .to_ascii_lowercase();
    let penuh = respon
        .bytes()
        .await
        .map_err(|e| format!("gagal membaca body {url}: {e}"))?;
    // Potong di batas karakter UTF-8 supaya tidak merusak teks multibyte.
    let potong = penuh.len().min(max_bytes);
    let mut akhir = potong;
    while akhir > 0 && std::str::from_utf8(&penuh[..akhir]).is_err() {
        akhir -= 1;
    }
    let teks = String::from_utf8_lossy(&penuh[..akhir]).into_owned();
    let isi = if tipe.contains("html") || tipe.contains("xml") {
        html_ke_markdown(&teks)
    } else {
        teks
    };
    Ok((tipe, isi))
}

/// Konversi HTML sederhana ke markdown (tanpa crate parser tambahan).
///
/// Fokus pada konten artikel: hapus script/style, ubah heading, link, paragraf,
/// list, dan entity umum. Tag tak dikenal dibuang, bukan dibocorkan ke model.
pub fn html_ke_markdown(html: &str) -> String {
    let mut out = html.to_string();
    // Regex Rust tidak punya backreference: buang tiap tag blok secara eksplisit.
    for tag in ["script", "style", "noscript"] {
        let pola = format!(r"(?is)<{tag}\b[^>]*>.*?</{tag}>");
        if let Ok(re) = Regex::new(&pola) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    for (pola, ganti) in [
        (r"(?i)<br\s*/?>", "\n"),
        (r"(?i)</(p|div|section|article|li|tr|h[1-6])>", "\n\n"),
    ] {
        if let Ok(re) = Regex::new(pola) {
            out = re.replace_all(&out, ganti).into_owned();
        }
    }
    if let Ok(re) = Regex::new(r"(?i)<h([1-6])[^>]*>") {
        out = re
            .replace_all(&out, |c: &regex::Captures| {
                let level = c
                    .get(1)
                    .and_then(|m| m.as_str().parse::<usize>().ok())
                    .unwrap_or(1);
                format!("\n\n{} ", "#".repeat(level))
            })
            .into_owned();
    }
    // Link: simpan teks + href; anchor tanpa href dibuang tagnya saja.
    if let Ok(re) = Regex::new(r#"(?is)<a\b[^>]*href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#) {
        out = re
            .replace_all(&out, |c: &regex::Captures| {
                let href = c.get(1).map(|m| m.as_str()).unwrap_or("");
                let teks = c.get(2).map(|m| m.as_str()).unwrap_or("");
                format!("[{teks}]({href})")
            })
            .into_owned();
    }
    if let Ok(re) = Regex::new(r"(?s)<[^>]+>") {
        out = re.replace_all(&out, "").into_owned();
    }
    for (dari, ke) in [
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&nbsp;", " "),
    ] {
        out = out.replace(dari, ke);
    }
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tool `fetch` untuk registry.
pub struct FetchTool;

#[async_trait]
impl Tool for FetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "fetch".to_string(),
            description: "Ambil halaman http(s) dan kembalikan teks/markdown (dibatasi ukuran)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL http(s)" },
                    "max_bytes": { "type": "integer", "description": "Batas ukuran body (default 64 KiB)" }
                },
                "required": ["url"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(url) = args.get("url").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen url wajib"));
        };
        let batas = args
            .get("max_bytes")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(ctx.output_cap)
            .min(ctx.output_cap);
        match ambil(url, batas).await {
            Ok((tipe, isi)) => Ok(ToolOutput::ok(format!("[{tipe}]\n{isi}"))),
            Err(pesan) => Ok(ToolOutput::err(pesan)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn html_jadi_markdown() {
        let html = "<html><head><style>xxx</style></head><body><h1>Judul</h1><p>Halo \
            <a href='https://example.com'>dunia</a> &amp; semua</p><script>y</script></body></html>";
        let md = html_ke_markdown(html);
        assert!(md.contains("# Judul"), "{md}");
        assert!(md.contains("[dunia](https://example.com)"), "{md}");
        assert!(md.contains("& semua"), "{md}");
        assert!(!md.contains("xxx"), "{md}");
    }

    #[test]
    fn skema_non_http_ditolak() {
        let err = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(ambil("file:///etc/passwd", 1024))
            .unwrap_err();
        assert!(err.contains("http"), "{err}");
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
                let body = "<h1>Halo</h1><p>dari lokal</p>";
                let respon = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(respon.as_bytes()).await;
            }
        });
        let (tipe, isi) = ambil(&format!("http://{addr}/"), 4096).await.unwrap();
        assert!(tipe.contains("html"), "{tipe}");
        assert!(isi.contains("# Halo"), "{isi}");
        assert!(isi.contains("dari lokal"), "{isi}");
    }
}
