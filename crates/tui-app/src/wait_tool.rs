//! Tool `wait`: tunggu job latar (subagent) selesai atau dibatalkan.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tui_subagent::manager::{SubagentManager, SubagentState};
use tui_tools::{Tool, ToolContext};
use tui_wire::{Concurrency, MxError, ToolDefinition, ToolOutput, ToolTier};

/// Ringkasan hasil penungguan (dipakai test & output tool).
pub fn ringkas_wait(state: SubagentState, teks: Option<&str>) -> String {
    match teks {
        Some(t) if !t.trim().is_empty() => format!("{}: {}", state.label(), t),
        _ => format!("{}: tidak ada teks hasil", state.label()),
    }
}

/// Tool penunggu subagent.
pub struct WaitTool {
    manager: Arc<SubagentManager>,
}

impl WaitTool {
    /// Bangun tool dari manager yang sama dengan panel TUI/RPC.
    pub fn baru(manager: Arc<SubagentManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for WaitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "wait".to_string(),
            description: "Tunggu subagent berdasarkan id sampai selesai/dibatalkan.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "timeout_ms": { "type": "integer", "description": "100..60000, default 5000" }
                },
                "required": ["id"]
            }),
            concurrency: Concurrency::Shared,
        }
    }

    fn tier(&self) -> ToolTier {
        ToolTier::Read
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<ToolOutput, MxError> {
        let Some(id) = args.get("id").and_then(Value::as_str) else {
            return Ok(ToolOutput::err("argumen id wajib"));
        };
        let batas = Duration::from_millis(
            args.get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(5_000)
                .clamp(100, 60_000),
        );
        let operasi = async {
            loop {
                if let Some(handle) = self.manager.get(id) {
                    let state = handle.status();
                    if state != SubagentState::Running {
                        let hasil = handle.result();
                        return ringkas_wait(state, hasil.as_ref().map(|r| r.text.as_str()));
                    }
                } else {
                    return format!("subagent '{id}' tidak ditemukan");
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        match tokio::time::timeout(batas, operasi).await {
            Ok(teks) => Ok(ToolOutput::ok(teks)),
            Err(_) => Ok(ToolOutput::err(format!(
                "subagent '{id}' masih berjalan setelah {} ms",
                batas.as_millis()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn ringkas_status_anak() {
        assert!(ringkas_wait(SubagentState::Idle, Some("selesai")).contains("selesai"));
        assert!(ringkas_wait(SubagentState::Killed, None).contains("dihentikan"));
    }

    #[tokio::test]
    async fn id_tidak_ada_dilaporkan() {
        let tool = WaitTool::baru(Arc::new(SubagentManager::new()));
        let ctx = ToolContext::new(std::env::temp_dir());
        let out = tool
            .execute(&json!({"id":"tidak-ada","timeout_ms":200}), &ctx)
            .await
            .expect("tool jalan");
        assert!(out.content.contains("tidak ditemukan"), "{}", out.content);
    }
}
