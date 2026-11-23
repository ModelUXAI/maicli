//! Tool `memory`: model boleh menambah/mencari/melihara memori proyek.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_tools::{Tool, ToolContext};
use tui_wire::{Concurrency, ToolDefinition, ToolOutput, ToolTier};

use crate::MemoryHandle;

/// Tool memori dengan tier Write (selalu lewat approval).
pub struct MemoryTool {
    memory: Arc<MemoryHandle>,
}

impl MemoryTool {
    /// Bangun tool dari pegangan memori.
    pub fn new(memory: Arc<MemoryHandle>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "memory".to_string(),
            description: "Memori proyek persisten lintas sesi. action=add menyimpan catatan, \
                          search mencari yang relevan, view menampilkan terbaru, forget menghapus \
                          berdasarkan id, stats menampilkan ringkasan."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["add", "search", "view", "forget", "stats", "retain", "recall", "reflect", "learn", "edit"]
                    },
                    "text": { "type": "string" },
                    "query": { "type": "string" },
                    "id": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 }
                },
                "required": ["action"]
            }),
            concurrency: Concurrency::Exclusive,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Write
    }

    async fn execute(
        &self,
        args: &Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, tui_wire::MxError> {
        let action = args.get("action").and_then(Value::as_str).unwrap_or("view");
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, 50) as usize)
            .unwrap_or(10);
        let hasil = match action {
            "add" => {
                let Some(text) = args.get("text").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action=add butuh field text"));
                };
                let tags: Vec<String> = args
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                self.memory
                    .add(text, &tags)
                    .map(|entry| format!("memori disimpan: {} — {}", entry.id, entry.text))
            }
            "search" => {
                let Some(query) = args.get("query").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action=search butuh field query"));
                };
                self.memory.search(query, limit).map(|hits| {
                    if hits.is_empty() {
                        return "tidak ada memori yang cocok".to_string();
                    }
                    hits.iter()
                        .map(|hit| {
                            format!(
                                "- [{}] {} (skor {:.2})",
                                hit.entry.id, hit.entry.text, hit.score
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
            "view" => self.memory.recent(limit).map(|entries| {
                if entries.is_empty() {
                    return "memori masih kosong".to_string();
                }
                entries
                    .iter()
                    .map(|entry| format!("- [{}] {}", entry.id, entry.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
            "forget" => {
                let Some(id) = args.get("id").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action=forget butuh field id"));
                };
                self.memory.forget(id).map(|removed| {
                    if removed {
                        "dihapus".to_string()
                    } else {
                        "id tidak ditemukan".to_string()
                    }
                })
            }
            "retain" | "learn" => {
                let Some(text) = args.get("text").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err(format!("action={action} butuh field text")));
                };
                let tags = if action == "learn" {
                    vec!["pelajaran".to_string()]
                } else {
                    Vec::new()
                };
                self.memory
                    .add(text, &tags)
                    .map(|entry| format!("{}: {} — {}", action, entry.id, entry.text))
            }
            "recall" => {
                let Some(query) = args.get("query").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action=recall butuh field query"));
                };
                self.memory.search(query, limit).map(|hits| {
                    if hits.is_empty() {
                        return "tidak ada memori yang cocok".to_string();
                    }
                    let mut out = String::from("Recall:\n");
                    for hit in hits {
                        out.push_str(&format!("- [{}] {}\n", hit.entry.id, hit.entry.text));
                    }
                    out
                })
            }
            "reflect" => {
                let query = args.get("query").and_then(Value::as_str).unwrap_or("");
                self.memory.search(query, limit).map(|hits| {
                    if hits.is_empty() {
                        return "tidak ada bahan untuk refleksi".to_string();
                    }
                    let gabung = hits
                        .iter()
                        .map(|h| h.entry.text.clone())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    format!("Refleksi ({} memori):\n{}", hits.len(), gabung)
                })
            }
            "edit" => {
                let Some(id) = args.get("id").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action=edit butuh field id"));
                };
                let Some(text) = args.get("text").and_then(Value::as_str) else {
                    return Ok(ToolOutput::err("action=edit butuh field text"));
                };
                let Some(lama) = self
                    .memory
                    .all()
                    .ok()
                    .and_then(|all| all.into_iter().find(|e| e.id == id))
                else {
                    return Ok(ToolOutput::err("id memori tidak ditemukan"));
                };
                self.memory.forget(id).map(|_| ())?;
                self.memory
                    .add(text, &lama.tags)
                    .map(|entry| format!("memori diperbarui: {} (lama {})", entry.id, id))
            }
            "stats" => self.memory.stats().map(|stats| {
                format!(
                    "backend={} entries={} embedding={} dir={}",
                    stats.backend,
                    stats.entries,
                    stats.embedding,
                    stats.dir.display()
                )
            }),
            lain => {
                return Ok(ToolOutput::err(format!("action '{lain}' tidak dikenal")));
            }
        };
        Ok(match hasil {
            Ok(text) => ToolOutput::ok(text),
            Err(err) => ToolOutput::err(format!("{}: {}", err.code, err.message)),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use serde_json::json;

    fn tool() -> (MemoryTool, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("maicli-mtool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let memory =
            MemoryHandle::open(&dir, "jsonl", false, 20, 500, false).expect("memori dibuka");
        (MemoryTool::new(Arc::new(memory)), dir)
    }

    #[tokio::test]
    async fn add_lalu_search() {
        let (tool, dir) = tool();
        let ctx = ToolContext::new(&dir);
        let add = tool
            .execute(&json!({"action":"add","text":"gateway rustls"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(!add.is_error, "{}", add.content);
        let search = tool
            .execute(&json!({"action":"search","query":"rustls"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(
            search.content.contains("gateway rustls"),
            "{}",
            search.content
        );
    }

    #[tokio::test]
    async fn aksi_asing_dilaporkan_ke_model() {
        let (tool, dir) = tool();
        let ctx = ToolContext::new(&dir);
        let hasil = tool
            .execute(&json!({"action":"hapus-semua"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(hasil.is_error);
        assert!(hasil.content.contains("tidak dikenal"));
    }
    #[tokio::test]
    async fn retain_recall_reflect_learn() {
        let (tool, dir) = tool();
        let ctx = ToolContext::new(&dir);
        for (action, text) in [
            ("retain", "gateway memakai rustls"),
            ("learn", "jangan hardcode kunci"),
        ] {
            let hasil = tool
                .execute(&json!({"action":action,"text":text}), &ctx)
                .await
                .expect("tool jalan");
            assert!(!hasil.is_error, "{}", hasil.content);
        }
        let recall = tool
            .execute(&json!({"action":"recall","query":"rustls"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(recall.content.contains("rustls"), "{}", recall.content);
        let reflect = tool
            .execute(&json!({"action":"reflect","query":"kunci"}), &ctx)
            .await
            .expect("tool jalan");
        assert!(reflect.content.contains("kunci"), "{}", reflect.content);
    }
}
