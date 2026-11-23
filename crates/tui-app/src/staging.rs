//! Staging atomik untuk perubahan berkas (`stage_write` → `/accept`).
//!
//! Konten disimpan di `<data>/staging/<id>.json`; belum menyentuh disk tujuan.
//! `/accept <id>` menulis ke path tujuan hanya bila berada di dalam cwd, lalu
//! menghapus staging.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Satu perubahan yang menunggu persetujuan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub target: String,
    pub content: String,
    pub created_ms: u64,
}

/// Folder staging.
pub fn dir(root: &Path) -> PathBuf {
    root.join("staging")
}

/// Buat staging baru; `target` absolut.
pub fn buat(root: &Path, target: &Path, content: &str) -> Result<Stage, String> {
    let id = format!(
        "stage-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let item = Stage {
        id: id.clone(),
        target: target.display().to_string(),
        content: content.to_string(),
        created_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    let folder = dir(root);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(
        folder.join(format!("{id}.json")),
        serde_json::to_string_pretty(&item).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(item)
}

/// Daftar staging yang ada.
pub fn daftar(root: &Path) -> Vec<Stage> {
    let Ok(entri) = std::fs::read_dir(dir(root)) else {
        return Vec::new();
    };
    let mut hasil: Vec<Stage> = entri
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|isi| serde_json::from_str(&isi).ok())
        .collect();
    hasil.sort_by_key(|s| s.created_ms);
    hasil
}

/// Terima satu staging: tulis ke target (harus di dalam `cwd`) lalu hapus.
pub fn terima(root: &Path, cwd: &Path, id: &str) -> Result<PathBuf, String> {
    let folder = dir(root);
    let path = folder.join(format!("{id}.json"));
    let isi = std::fs::read_to_string(&path).map_err(|e| format!("staging {id} tidak ada: {e}"))?;
    let stage: Stage = serde_json::from_str(&isi).map_err(|e| format!("staging rusak: {e}"))?;
    let target = PathBuf::from(&stage.target);
    let target_nyata = std::fs::canonicalize(target.parent().unwrap_or(cwd))
        .map(|p| p.join(target.file_name().unwrap_or_default()))
        .unwrap_or(target.clone());
    let cwd_nyata = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    if !target_nyata.starts_with(&cwd_nyata) {
        return Err(format!("target di luar cwd: {}", target_nyata.display()));
    }
    if let Some(induk) = target_nyata.parent() {
        std::fs::create_dir_all(induk).map_err(|e| e.to_string())?;
    }
    let tmp = target_nyata.with_extension("maicli-stage.tmp");
    std::fs::write(&tmp, &stage.content).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target_nyata).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(path);
    Ok(target_nyata)
}

/// Tool `stage_write`: tulis ke staging, bukan disk tujuan.
pub struct StageWriteTool {
    root: PathBuf,
}

impl StageWriteTool {
    pub fn baru(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait::async_trait]
impl tui_tools::Tool for StageWriteTool {
    fn definition(&self) -> tui_wire::ToolDefinition {
        tui_wire::ToolDefinition {
            name: "stage_write".to_string(),
            description: "Usulkan isi berkas (staging); terima dengan /accept <id>.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            concurrency: tui_wire::Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> tui_wire::ToolTier {
        tui_wire::ToolTier::Write
    }

    async fn execute(
        &self,
        args: &serde_json::Value,
        ctx: &tui_tools::ToolContext,
    ) -> Result<tui_wire::ToolOutput, tui_wire::MxError> {
        let Some(rel) = args.get("path").and_then(serde_json::Value::as_str) else {
            return Ok(tui_wire::ToolOutput::err("argumen path wajib"));
        };
        let Some(content) = args.get("content").and_then(serde_json::Value::as_str) else {
            return Ok(tui_wire::ToolOutput::err("argumen content wajib"));
        };
        let target = if Path::new(rel).is_absolute() {
            PathBuf::from(rel)
        } else {
            ctx.cwd.join(rel)
        };
        match buat(&self.root, &target, content) {
            Ok(stage) => Ok(tui_wire::ToolOutput::ok(format!(
                "staging {} untuk {} ({} byte); terima: /accept {}",
                stage.id,
                stage.target,
                content.len(),
                stage.id
            ))),
            Err(err) => Ok(tui_wire::ToolOutput::err(err)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn stage_lalu_accept_menulis() {
        let root = std::env::temp_dir().join(format!(
            "maicli-stage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = root.join("kerja");
        std::fs::create_dir_all(&cwd).unwrap();
        let target = cwd.join("a.txt");
        let stage = buat(&root, &target, "isi baru").unwrap();
        assert!(!target.exists());
        let ditulis = terima(&root, &cwd, &stage.id).unwrap();
        assert_eq!(std::fs::read_to_string(ditulis).unwrap(), "isi baru");
        assert!(daftar(&root).is_empty());
        std::fs::remove_dir_all(root).ok();
    }
}
