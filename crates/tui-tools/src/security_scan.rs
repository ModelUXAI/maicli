//! Tool `security_scan`: pemindaian pola kredensial di workspace.
//!
//! Hanya mencocokkan pola teks—bukan validator kunci live—dan **tidak pernah**
//! mengembalikan nilai rahasia: temuan selalu diredaksi. Berguna sebagai gate
//! cepat sebelum commit, bukan pengganti scanner khusus.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

/// Satu temuan (nilai sudah diredaksi).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Temuan {
    pub path: String,
    pub baris: usize,
    pub label: &'static str,
    pub severity: &'static str,
}

/// Pola yang dikenali; tuple = (label, severity, regex).
const POLA: [(&str, &str, &str); 8] = [
    ("aws-access-key", "tinggi", r"AKIA[0-9A-Z]{16}"),
    (
        "github-token",
        "tinggi",
        r"gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}",
    ),
    ("openai-key", "tinggi", r"sk-[A-Za-z0-9_-]{20,}"),
    ("slack-token", "tinggi", r"xox[baprs]-[A-Za-z0-9-]{10,}"),
    (
        "private-key",
        "tinggi",
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    ),
    (
        "jwt",
        "sedang",
        r"eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
    ),
    (
        "password-assignment",
        "sedang",
        r#"(?i)(password|passwd|api[_-]?key|secret|token)\s*[:=]\s*["'][^\s"']{8,}["']"#,
    ),
    (
        "bearer-header",
        "rendah",
        r#"(?i)authorization\s*[:=]\s*["']?Bearer\s+[A-Za-z0-9._-]{16,}"#,
    ),
];

/// Redaksi nilai sensitive sebelum ditampilkan.
pub fn redaksi(teks: &str) -> String {
    let mut out = teks.to_string();
    for (_, _, pola) in POLA {
        if let Ok(re) = Regex::new(pola) {
            out = re.replace_all(&out, "[REDACTED]").into_owned();
        }
    }
    out
}

/// Pindai satu potong teks; `path` dipakai untuk pelaporan.
pub fn pindai_teks(path: &str, teks: &str) -> Vec<Temuan> {
    let kompilasi: Vec<(&str, &str, Regex)> = POLA
        .iter()
        .filter_map(|(label, severity, pola)| {
            Regex::new(pola).ok().map(|re| (*label, *severity, re))
        })
        .collect();
    let mut hasil = Vec::new();
    for (idx, baris) in teks.lines().enumerate() {
        for (label, severity, re) in &kompilasi {
            if re.is_match(baris) {
                hasil.push(Temuan {
                    path: path.to_string(),
                    baris: idx + 1,
                    label,
                    severity,
                });
            }
        }
    }
    hasil
}

/// Kumpulkan berkas teks yang akan dipindai.
fn berkas(root: &Path, maks: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(root).into_iter().flatten() {
        if out.len() >= maks {
            break;
        }
        let path = e.path();
        if !e.file_type().is_file()
            || path.components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some(".git" | "target" | "node_modules" | "dist")
                )
            })
        {
            continue;
        }
        let Ok(isi) = std::fs::read(path) else {
            continue;
        };
        if isi.iter().take(4096).any(|b| *b == 0) {
            continue;
        }
        out.push(path.to_path_buf());
    }
    out.sort();
    out
}

/// Pindai root direktori; hasil dibatasi 200 baris.
pub fn pindai(root: &Path, maks_file: usize) -> Vec<Temuan> {
    let mut hasil = Vec::new();
    for file in berkas(root, maks_file) {
        let Ok(isi) = std::fs::read_to_string(&file) else {
            continue;
        };
        let relatif = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        hasil.extend(pindai_teks(&relatif, &isi));
        if hasil.len() >= 200 {
            hasil.truncate(200);
            break;
        }
    }
    hasil
}

/// Tool `security_scan`.
pub struct SecurityScanTool;

#[async_trait]
impl Tool for SecurityScanTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "security_scan".to_string(),
            description: "Pindai workspace untuk pola kredensial; nilai temuan selalu diredaksi."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Subdireksi; kosong = workspace" },
                    "max_files": { "type": "integer", "description": "Default 500" }
                },
                "required": []
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let maks = args
            .get("max_files")
            .and_then(Value::as_u64)
            .unwrap_or(500)
            .clamp(1, 2000) as usize;
        let root = args
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(|s| ctx.cwd.join(s))
            .unwrap_or_else(|| ctx.cwd.clone());
        if !root.is_dir() {
            return Ok(ToolOutput::err(format!(
                "bukan direktori: {}",
                root.display()
            )));
        }
        let hasil = pindai(&root, maks);
        if hasil.is_empty() {
            return Ok(ToolOutput::ok(
                "tidak ada pola kredensial terdeteksi".to_string(),
            ));
        }
        let baris: Vec<String> = hasil
            .iter()
            .map(|t| {
                format!(
                    "{}:{} [{}] {} (nilai diredaksi)",
                    t.path, t.baris, t.severity, t.label
                )
            })
            .collect();
        Ok(ToolOutput::ok(format!(
            "{} temuan:\n{}",
            hasil.len(),
            baris.join("\n")
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn deteksi_dan_redaksi_kunci() {
        let teks = "key = \"sk-abcdefghijklmnopqrstuvwx\"\npassword = 'rahasia123'\n";
        let hasil = pindai_teks("a.rs", teks);
        assert!(hasil.iter().any(|t| t.label == "openai-key"), "{hasil:?}");
        assert!(hasil.iter().any(|t| t.label == "password-assignment"));
        assert!(!redaksi(teks).contains("sk-abcdef"), "{}", redaksi(teks));
    }

    #[test]
    fn pindai_direktori() {
        let root = std::env::temp_dir().join(format!(
            "maicli-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "AKIAABCDEFGHIJKLMNOP\n").unwrap();
        let hasil = pindai(&root, 10);
        assert!(
            hasil.iter().any(|t| t.label == "aws-access-key"),
            "{hasil:?}"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
