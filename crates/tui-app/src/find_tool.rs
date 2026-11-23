//! Tool `find`: cari kode dari deskripsi bahasa alami lewat model judge.
//!
//! Model hanya memilih dari daftar path nyata yang dikirim tool; jawaban tetap
//! divalidasi terhadap kandidat, sehingga model tidak bisa mengarang berkas.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_provider::Provider;
use tui_tools::{Tool, ToolContext};
use tui_wire::{
    Concurrency, Message, MxError, ProviderEvent, ToolDefinition, ToolOutput, ToolTier,
};

/// Tool pencarian semantik ringan.
pub struct FindTool {
    provider: Arc<dyn Provider>,
    model: String,
}

impl FindTool {
    /// Bangun tool dengan provider/model yang dipakai untuk memilih path.
    pub fn baru(provider: Arc<dyn Provider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

/// Kumpulkan kandidat berkas teks (deterministik, terurut).
pub fn kandidat(root: &Path, maks: usize) -> Vec<String> {
    let mut antre = VecDeque::from([root.to_path_buf()]);
    let mut hasil = Vec::new();
    while let Some(dir) = antre.pop_front() {
        let Ok(entri) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut anak: Vec<PathBuf> = entri.flatten().map(|e| e.path()).collect();
        anak.sort();
        for path in anak {
            if hasil.len() >= maks {
                return hasil;
            }
            if path.is_dir() {
                let nama = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(nama, ".git" | "target" | "node_modules" | "dist") {
                    antre.push_back(path);
                }
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e,
                    "rs" | "ts"
                        | "tsx"
                        | "js"
                        | "jsx"
                        | "py"
                        | "go"
                        | "md"
                        | "toml"
                        | "json"
                        | "yaml"
                        | "yml"
                )
            }) {
                hasil.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    hasil
}

/// Validasi jawaban model terhadap daftar kandidat.
pub fn parse_pilihan(jawaban: &str, kandidat: &[String], maks: usize) -> Vec<String> {
    let mut hasil = Vec::new();
    for baris in jawaban.lines() {
        let calon = baris
            .trim()
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == ' ')
            .trim()
            .trim_matches('`');
        if calon.is_empty() {
            continue;
        }
        if let Some(cocok) = kandidat.iter().find(|p| p.as_str() == calon)
            && !hasil.contains(cocok)
        {
            hasil.push(cocok.clone());
            if hasil.len() >= maks {
                break;
            }
        }
    }
    hasil
}

#[async_trait]
impl Tool for FindTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "find".to_string(),
            description:
                "Cari berkas dari deskripsi bahasa alami (model memilih dari daftar nyata)."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "description": { "type": "string" },
                    "limit": { "type": "integer", "description": "1..20, default 5" }
                },
                "required": ["description"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(deskripsi) = args.get("description").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen description wajib"));
        };
        let batas = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 20) as usize;
        let daftar = kandidat(&ctx.cwd, 300);
        if daftar.is_empty() {
            return Ok(ToolOutput::ok("tidak ada berkas kandidat".to_string()));
        }
        let prompt = format!(
            "Pilih maksimal {batas} path yang paling relevan untuk: {deskripsi}\n\
             Balas satu path per baris, persis seperti daftar. Jangan menambah penjelasan.\n\n{}",
            daftar
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}. {p}", i + 1))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let request = tui_provider::ChatRequest {
            model: self.model.clone(),
            messages: vec![
                Message::system("Kamu asisten pencarian berkas. Hanya balas daftar path."),
                Message::user(prompt),
            ],
            tools: Vec::new(),
            max_tokens: Some(500),
            reasoning_effort: None,
            service_tier: None,
        };
        let mut penerima = match Provider::stream_chat(self.provider.as_ref(), request).await {
            Ok(p) => p,
            Err(err) => return Ok(ToolOutput::err(format!("find gagal: {}", err.message))),
        };
        let mut jawaban = String::new();
        while let Some(event) = penerima.recv().await {
            if let ProviderEvent::Done { message } = event {
                jawaban = message.joined_text();
            }
        }
        let pilihan = parse_pilihan(&jawaban, &daftar, batas);
        if pilihan.is_empty() {
            Ok(ToolOutput::ok(
                "model tidak memilih path yang valid".to_string(),
            ))
        } else {
            Ok(ToolOutput::ok(pilihan.join("\n")))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn kandidat_dan_parse_valid() {
        let root = std::env::temp_dir().join(format!(
            "maicli-find-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "x").unwrap();
        std::fs::write(root.join("b.md"), "y").unwrap();
        let daftar = kandidat(&root, 10);
        assert!(daftar.contains(&"src/a.rs".to_string()), "{daftar:?}");
        let pilih = parse_pilihan("1. src/a.rs\n- b.md\ncarangan.rs", &daftar, 5);
        assert_eq!(pilih, vec!["src/a.rs".to_string(), "b.md".to_string()]);
        std::fs::remove_dir_all(root).ok();
    }
}
