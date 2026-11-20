//! Metadata tampilan tool v1 (RENCANA C-04 `AgentTool`).
//!
//! Dipisah dari `ToolDefinition` karena definisi tool adalah bentuk wire yang
//! dikirim ke gateway; label/render/loadMode hanya urusan UI dan tidak boleh
//! ikut ke payload API.

use tui_wire::{AgentToolMeta, LoadMode, RenderHint};

/// Metadata tampilan untuk tool v1.
///
/// Label dipakai timeline, `render` memilih gaya keluaran; `load_mode` tetap
/// `Eager` untuk semua tool v1. Nama tak dikenal mendapat metadata default
/// (label = nama) supaya tool asing tetap tampil jelas, bukan panic.
pub fn agent_tool_meta(name: &str) -> AgentToolMeta {
    let (label, render) = match name {
        "read" => ("baca", RenderHint::Plain),
        "write" => ("tulis", RenderHint::Plain),
        "edit" => ("edit", RenderHint::Diff),
        "glob" => ("glob", RenderHint::Plain),
        "grep" => ("grep", RenderHint::Plain),
        "bash" => ("perintah", RenderHint::Command),
        "todo" => ("todo", RenderHint::Plain),
        "ask" => ("tanya", RenderHint::Plain),
        nama if nama.starts_with("mcp.") => ("mcp", RenderHint::Plain),
        _ => (name, RenderHint::Plain),
    };
    AgentToolMeta {
        label: label.to_string(),
        load_mode: LoadMode::Eager,
        render,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semua_tool_v1_punya_label_dan_render_tepat() {
        assert_eq!(agent_tool_meta("read").label, "baca");
        assert_eq!(agent_tool_meta("bash").render, RenderHint::Command);
        assert_eq!(agent_tool_meta("edit").render, RenderHint::Diff);
        for tool in [
            "read", "write", "edit", "glob", "grep", "bash", "todo", "ask",
        ] {
            let meta = agent_tool_meta(tool);
            assert_eq!(meta.load_mode, LoadMode::Eager, "{tool}");
            assert!(!meta.label.is_empty(), "{tool} tanpa label");
        }
    }

    #[test]
    fn tool_mcp_mendapat_label_mcp() {
        let meta = agent_tool_meta("mcp.mock.echo");
        assert_eq!(meta.label, "mcp");
        assert_eq!(meta.render, RenderHint::Plain);
        assert_eq!(meta.load_mode, LoadMode::Eager);
    }

    #[test]
    fn tool_asing_mendapat_label_namanya() {
        let meta = agent_tool_meta("tidak_ada");
        assert_eq!(meta.label, "tidak_ada");
        assert_eq!(meta.render, RenderHint::Plain);
    }
}
