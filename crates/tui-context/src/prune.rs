//! Pruning hasil tool lama.
//!
//! Hasil tool (output `bash`, isi berkas) adalah bagian konteks yang paling
//! cepat membesar dan paling cepat basi. Yang dipertahankan hanya `keep_last`
//! hasil terakhir; sisanya diganti placeholder sehingga pasangan
//! `tool_use`/`tool_result` tetap utuh dan model tahu isinya pernah ada.

use tui_wire::Message;

/// Pengganti isi hasil tool yang dipangkas.
pub const PRUNED_TOOL_RESULT: &str = "[hasil tool lama dihapus untuk menghemat konteks]";

/// Pangkas isi `ToolResult` lama; mengembalikan jumlah yang benar-benar diubah.
pub fn prune_tool_results(messages: &mut [Message], keep_last: usize) -> usize {
    let positions: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| matches!(message, Message::ToolResult { .. }))
        .map(|(index, _)| index)
        .collect();
    let cut = positions.len().saturating_sub(keep_last);
    let mut pruned = 0;
    for index in positions.into_iter().take(cut) {
        if let Some(Message::ToolResult { content, .. }) = messages.get_mut(index)
            && content.as_str() != PRUNED_TOOL_RESULT
            && content.chars().count() > PRUNED_TOOL_RESULT.chars().count()
        {
            *content = PRUNED_TOOL_RESULT.to_string();
            pruned += 1;
        }
    }
    pruned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(content: &str) -> Message {
        Message::tool_result("call_1", "bash", content, false, false)
    }

    #[test]
    fn hanya_hasil_terakhir_yang_dipertahankan() {
        let mut messages = vec![
            Message::user("mulai"),
            result(&"a".repeat(200)),
            Message::assistant(&tui_wire::AssistantMessage::text(
                "lanjut",
                tui_wire::StopReason::Stop,
            )),
            result(&"b".repeat(200)),
            Message::user("lagi"),
            result(&"c".repeat(200)),
        ];
        let pruned = prune_tool_results(&mut messages, 1);
        assert_eq!(pruned, 2);
        match (&messages[1], &messages[3], &messages[5]) {
            (
                Message::ToolResult { content: first, .. },
                Message::ToolResult {
                    content: second, ..
                },
                Message::ToolResult { content: third, .. },
            ) => {
                assert_eq!(first, PRUNED_TOOL_RESULT);
                assert_eq!(second, PRUNED_TOOL_RESULT);
                assert!(third.starts_with('c'));
            }
            other => panic!("bentuk tak terduga: {other:?}"),
        }
    }

    #[test]
    fn keep_last_nol_memangkas_semua() {
        let mut messages = vec![result(&"x".repeat(100)), result(&"y".repeat(100))];
        assert_eq!(prune_tool_results(&mut messages, 0), 2);
    }

    #[test]
    fn hasil_pendek_dan_sudah_dipangkas_tidak_dihitung() {
        let mut messages = vec![result("pendek"), result(PRUNED_TOOL_RESULT)];
        assert_eq!(prune_tool_results(&mut messages, 0), 0);
    }

    #[test]
    fn pesan_non_tool_tidak_disentuh() {
        let mut messages = vec![Message::user("halo")];
        assert_eq!(prune_tool_results(&mut messages, 0), 0);
        assert!(matches!(&messages[0], Message::User { content } if content == "halo"));
    }
}
