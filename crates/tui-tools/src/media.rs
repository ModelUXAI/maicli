//! Tool `media`: jalur generik TTS / image generation lewat perintah eksternal.
//!
//! Penyedia tidak dibundling: config `[media]` menentukan argv perintah. Tool
//! mengirim prompt lewat stdin, menyimpan stdout ke berkas keluaran, membuang
//! env kredensial, dan membatasi waktu 30 dtk.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::{MxError, Tool, ToolContext};

/// Tool media dengan dua perintah opsional.
#[derive(Clone)]
pub struct MediaTool {
    tts: Option<(String, Vec<String>)>,
    image: Option<(String, Vec<String>)>,
}

impl MediaTool {
    /// `None` bila dua-duanya tidak dikonfigurasi.
    pub fn baru(tts: (String, Vec<String>), image: (String, Vec<String>)) -> Option<Self> {
        let tts = (!tts.0.trim().is_empty()).then_some(tts);
        let image = (!image.0.trim().is_empty()).then_some(image);
        if tts.is_none() && image.is_none() {
            None
        } else {
            Some(Self { tts, image })
        }
    }

    /// Bangun dari argv config (elemen pertama = program).
    pub fn dari_config(tts: Vec<String>, image: Vec<String>) -> Option<Self> {
        let pecah = |argv: Vec<String>| -> (String, Vec<String>) {
            let mut iter = argv.into_iter();
            match iter.next() {
                Some(program) => (program, iter.collect()),
                None => (String::new(), Vec::new()),
            }
        };
        Self::baru(pecah(tts), pecah(image))
    }

    /// Jalankan satu perintah; stdin=prompt, stdout=berkas hasil.
    pub async fn jalankan(
        perintah: &(String, Vec<String>),
        prompt: &str,
        keluaran: &Path,
    ) -> Result<String, String> {
        let mut command = tokio::process::Command::new(&perintah.0);
        command.args(&perintah.1).arg(keluaran);
        command
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // Kredensial tidak diwariskan ke penyedia pihak ketiga.
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
        let mut child = command
            .spawn()
            .map_err(|e| format!("gagal menjalankan {}: {e}", perintah.0))?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(prompt.as_bytes()).await;
        }
        let hasil = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
            .await
            .map_err(|_| "perintah media melewati batas 30 dtk".to_string())?
            .map_err(|e| e.to_string())?;
        if !hasil.status.success() {
            return Err(String::from_utf8_lossy(&hasil.stderr).trim().to_string());
        }
        let isi = std::fs::read(keluaran).map_err(|e| format!("hasil tidak ditulis: {e}"))?;
        Ok(format!(
            "{} byte ditulis ke {}",
            isi.len(),
            keluaran.display()
        ))
    }
}

#[async_trait]
impl Tool for MediaTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "media".to_string(),
            description: "Buat audio/gambar lewat perintah eksternal dari config [media]."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["tts", "image"] },
                    "text": { "type": "string" },
                    "out": { "type": "string" }
                },
                "required": ["action", "text", "out"]
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
        let Some(teks) = args.get("text").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen text wajib"));
        };
        let Some(out) = args.get("out").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen out wajib"));
        };
        let perintah = match aksi {
            "tts" => self.tts.as_ref(),
            "image" => self.image.as_ref(),
            _ => return Ok(ToolOutput::err("action harus tts atau image")),
        };
        let Some(perintah) = perintah else {
            return Ok(ToolOutput::err(format!(
                "penyedia {aksi} tidak dikonfigurasi"
            )));
        };
        let keluaran: PathBuf = if Path::new(out).is_absolute() {
            PathBuf::from(out)
        } else {
            ctx.cwd.join(out)
        };
        match Self::jalankan(perintah, teks, &keluaran).await {
            Ok(pesan) => Ok(ToolOutput::ok(pesan)),
            Err(err) => Ok(ToolOutput::err(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    #[cfg(unix)]
    #[tokio::test]
    async fn tulis_hasil_dari_child() {
        use super::*;
        let root = std::env::temp_dir().join(format!(
            "maicli-media-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let perintah = (
            "python3".to_string(),
            vec![
                "-c".to_string(),
                "import sys; open(sys.argv[1], 'wb').write(sys.stdin.buffer.read())".to_string(),
            ],
        );
        let tool = MediaTool::baru(perintah.clone(), perintah).expect("tool valid");
        let ctx = ToolContext::new(&root);
        let out = tool
            .execute(
                &json!({"action":"tts","text":"halo","out":"hasil.wav"}),
                &ctx,
            )
            .await
            .expect("tool jalan");
        assert!(!out.is_error, "{}", out.content);
        assert_eq!(std::fs::read(root.join("hasil.wav")).unwrap(), b"halo");
        std::fs::remove_dir_all(root).ok();
    }
}
