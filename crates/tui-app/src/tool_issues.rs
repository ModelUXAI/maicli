//! Laporan masalah tool (`report_tool_issue` / resolve).
//!
//! Isu disimpan sebagai JSONL di `<data>/tool-issues.jsonl`. Isu terbuka
//! disuntikkan ke system prompt supaya agen menyadari bug tool yang sedang
//! diikuti; `/tool-issue-resolve` menandai selesai tanpa menghapus riwayat.

use serde::{Deserialize, Serialize};

/// Satu laporan isu tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub tool: String,
    pub pesan: String,
    pub severity: String,
    pub status: String,
    #[serde(default)]
    pub ts: u64,
}

/// Path penyimpanan isu.
pub fn path() -> std::path::PathBuf {
    crate::config_root().join("tool-issues.jsonl")
}

fn sekarang_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Muat seluruh isu; baris rusak diabaikan.
pub fn muat() -> Vec<Issue> {
    let Ok(isi) = std::fs::read_to_string(path()) else {
        return Vec::new();
    };
    isi.lines()
        .filter_map(|l| serde_json::from_str::<Issue>(l).ok())
        .collect()
}

/// Simpan ulang seluruh isu secara atomik.
fn simpan(semua: &[Issue]) -> Result<(), String> {
    let target = path();
    let tmp = target.with_extension("jsonl.tmp");
    let mut isi = String::new();
    for item in semua {
        isi.push_str(&serde_json::to_string(item).map_err(|e| e.to_string())?);
        isi.push('\n');
    }
    std::fs::write(&tmp, isi).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())
}

/// Tambah laporan baru; mengembalikan id.
pub fn tambah(tool: &str, pesan: &str, severity: &str) -> Result<String, String> {
    let tool = tool.trim();
    let pesan = pesan.trim();
    if tool.is_empty() || pesan.is_empty() {
        return Err("tool dan pesan wajib".to_string());
    }
    let severity = if severity.is_empty() {
        "sedang"
    } else {
        severity
    };
    let id = format!("issue-{:x}", sekarang_ms());
    let mut semua = muat();
    semua.push(Issue {
        id: id.clone(),
        tool: tool.to_string(),
        pesan: pesan.to_string(),
        severity: severity.to_string(),
        status: "terbuka".to_string(),
        ts: sekarang_ms(),
    });
    simpan(&semua)?;
    Ok(id)
}

/// Tandai satu isu sebagai selesai.
pub fn selesaikan(id: &str) -> Result<bool, String> {
    let mut semua = muat();
    let Some(item) = semua.iter_mut().find(|i| i.id == id) else {
        return Ok(false);
    };
    item.status = "selesai".to_string();
    simpan(&semua)?;
    Ok(true)
}

/// Hanya isu terbuka untuk system prompt.
pub fn terbuka(semua: &[Issue]) -> Vec<&Issue> {
    semua.iter().filter(|i| i.status == "terbuka").collect()
}

/// Render blok system prompt.
pub fn blok(semua: &[Issue]) -> String {
    let buka = terbuka(semua);
    if buka.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n## Isu tool terbuka (/tool-issues)\n");
    for i in buka {
        out.push_str(&format!(
            "- [{}] {} ({}): {}\n",
            i.id, i.tool, i.severity, i.pesan
        ));
    }
    out
}

/// Tool `report_tool_issue`: laporan isu dari dalam turn.
pub struct ReportToolIssueTool {
    root: std::path::PathBuf,
}

impl ReportToolIssueTool {
    /// Bangun tool dengan root data (bisa diarahkan ke temp pada test).
    pub fn baru(root: std::path::PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait::async_trait]
impl tui_tools::Tool for ReportToolIssueTool {
    fn definition(&self) -> tui_wire::ToolDefinition {
        tui_wire::ToolDefinition {
            name: "report_tool_issue".to_string(),
            description: "Laporkan masalah tool agar tercatat dan muncul ke pengguna.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tool": { "type": "string" },
                    "message": { "type": "string" },
                    "severity": { "type": "string", "enum": ["rendah", "sedang", "tinggi"] }
                },
                "required": ["tool", "message"]
            }),
            concurrency: tui_wire::Concurrency::Shared,
        }
    }

    fn tier(&self) -> tui_wire::ToolTier {
        tui_wire::ToolTier::Read
    }

    async fn execute(
        &self,
        args: &serde_json::Value,
        _ctx: &tui_tools::ToolContext,
    ) -> Result<tui_wire::ToolOutput, tui_wire::MxError> {
        let Some(tool) = args.get("tool").and_then(serde_json::Value::as_str) else {
            return Ok(tui_wire::ToolOutput::err("field tool wajib"));
        };
        let Some(pesan) = args.get("message").and_then(serde_json::Value::as_str) else {
            return Ok(tui_wire::ToolOutput::err("field message wajib"));
        };
        let severity = args
            .get("severity")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("sedang");
        match tambah_di(&self.root, tool, pesan, severity) {
            Ok(id) => Ok(tui_wire::ToolOutput::ok(format!("isu dicatat: {id}"))),
            Err(err) => Ok(tui_wire::ToolOutput::err(err)),
        }
    }
}

/// Tambah isu dengan root eksplisit (dipakai tool & test).
pub fn tambah_di(
    root: &std::path::Path,
    tool: &str,
    pesan: &str,
    severity: &str,
) -> Result<String, String> {
    let tool = tool.trim();
    let pesan = pesan.trim();
    if tool.is_empty() || pesan.is_empty() {
        return Err("tool dan pesan wajib".to_string());
    }
    let severity = if severity.is_empty() {
        "sedang"
    } else {
        severity
    };
    let id = format!("issue-{:x}", sekarang_ms());
    let mut semua = muat_di(root);
    semua.push(Issue {
        id: id.clone(),
        tool: tool.to_string(),
        pesan: pesan.to_string(),
        severity: severity.to_string(),
        status: "terbuka".to_string(),
        ts: sekarang_ms(),
    });
    simpan_di(root, &semua)?;
    Ok(id)
}

fn muat_di(root: &std::path::Path) -> Vec<Issue> {
    let Ok(isi) = std::fs::read_to_string(root.join("tool-issues.jsonl")) else {
        return Vec::new();
    };
    isi.lines()
        .filter_map(|l| serde_json::from_str::<Issue>(l).ok())
        .collect()
}

fn simpan_di(root: &std::path::Path, semua: &[Issue]) -> Result<(), String> {
    let target = root.join("tool-issues.jsonl");
    let mut isi = String::new();
    for item in semua {
        isi.push_str(&serde_json::to_string(item).map_err(|e| e.to_string())?);
        isi.push('\n');
    }
    std::fs::write(&target, isi).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn blok_hanya_isu_terbuka() {
        let buka = Issue {
            id: "issue-1".to_string(),
            tool: "bash".to_string(),
            pesan: "timeout".to_string(),
            severity: "tinggi".to_string(),
            status: "terbuka".to_string(),
            ts: 1,
        };
        let mut tutup = buka.clone();
        tutup.id = "issue-2".to_string();
        tutup.status = "selesai".to_string();
        let teks = blok(&[buka, tutup]);
        assert!(teks.contains("issue-1"));
        assert!(!teks.contains("issue-2"));
    }
    #[tokio::test]
    async fn tool_report_menulis_isu() {
        let root = std::env::temp_dir().join(format!("maicli-issue-tool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let tool = ReportToolIssueTool::baru(root.clone());
        let ctx = tui_tools::ToolContext::new(&root);
        let out = tui_tools::Tool::execute(
            &tool,
            &serde_json::json!({"tool":"bash","message":"timeout","severity":"tinggi"}),
            &ctx,
        )
        .await
        .expect("tool jalan");
        assert!(!out.is_error, "{}", out.content);
        assert!(
            std::fs::read_to_string(root.join("tool-issues.jsonl"))
                .unwrap()
                .contains("timeout")
        );
        std::fs::remove_dir_all(root).ok();
    }
}
