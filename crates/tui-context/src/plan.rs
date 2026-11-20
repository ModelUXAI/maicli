//! Perencanaan compaction: memisahkan bagian yang diringkas dari bagian yang
//! dipertahankan.
//!
//! Batas pemisahan tidak boleh menghasilkan `tool_result` tanpa pasangan
//! `tool_use` di depannya, jadi batas digeser maju sampai aman.

use tui_wire::Message;

use crate::tokens::estimate_message;

/// Hasil pemisahan pesan untuk compaction.
#[derive(Debug, Clone, PartialEq)]
pub struct CompactionPlan {
    /// Pesan yang akan digantikan ringkasan.
    pub to_summarize: Vec<Message>,
    /// Pesan yang dipertahankan apa adanya.
    pub keep: Vec<Message>,
}

/// Susun rencana compaction; `None` bila tidak ada yang bisa diringkas.
pub fn plan_compaction(messages: &[Message], keep_recent_tokens: u64) -> Option<CompactionPlan> {
    if messages.is_empty() {
        return None;
    }
    let mut split = messages.len();
    if keep_recent_tokens > 0 {
        let mut accumulated = 0u64;
        for (index, message) in messages.iter().enumerate().rev() {
            if accumulated >= keep_recent_tokens {
                break;
            }
            accumulated = accumulated.saturating_add(estimate_message(message));
            split = index;
        }
    }
    while split < messages.len() && matches!(messages.get(split), Some(Message::ToolResult { .. }))
    {
        split += 1;
    }
    if split == 0 {
        return None;
    }
    Some(CompactionPlan {
        to_summarize: messages.get(..split)?.to_vec(),
        keep: messages.get(split..)?.to_vec(),
    })
}

/// Terapkan ringkasan: satu pesan ringkasan di depan, lalu pesan yang dipertahankan.
pub fn apply_compaction(summary: &str, plan: &CompactionPlan) -> Vec<Message> {
    let mut out = Vec::with_capacity(plan.keep.len() + 1);
    out.push(Message::user(format!(
        "[ringkasan konteks sebelumnya]\n{}",
        summary.trim()
    )));
    out.extend(plan.keep.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_wire::{AssistantMessage, StopReason};

    fn long_user(chars: usize) -> Message {
        Message::user("a".repeat(chars))
    }

    #[test]
    fn mempertahankan_ekor_sesuai_anggaran() {
        let messages = vec![long_user(400), long_user(400), long_user(400)];
        // tiap pesan ≈100 token + 16 framing; anggaran 120 memaksa dua pesan terakhir
        // dipertahankan agar ekor benar-benar menutup anggaran.
        let plan = plan_compaction(&messages, 120).expect("harus ada yang diringkas");
        assert_eq!(plan.keep.len(), 2);
        assert_eq!(plan.to_summarize.len(), 1);
    }

    #[test]
    fn batas_tidak_boleh_mulai_dengan_tool_result() {
        let messages = vec![
            long_user(400),
            Message::assistant(&AssistantMessage::text("pakai tool", StopReason::ToolUse)),
            Message::tool_result("call_1", "bash", "x".repeat(400), false, false),
            long_user(400),
        ];
        let plan = plan_compaction(&messages, 120).expect("harus ada yang diringkas");
        assert!(matches!(
            plan.keep.first(),
            Some(Message::User { .. }) | None
        ));
        assert_eq!(plan.keep.len(), 1);
    }

    #[test]
    fn semua_muatan_tidak_ada_yang_diringkas() {
        let messages = vec![long_user(20), long_user(20)];
        assert!(plan_compaction(&messages, 10_000).is_none());
    }

    #[test]
    fn daftar_kosong_tidak_menghasilkan_rencana() {
        assert!(plan_compaction(&[], 100).is_none());
    }

    #[test]
    fn apply_menaruh_ringkasan_di_depan() {
        let plan = CompactionPlan {
            to_summarize: vec![long_user(400)],
            keep: vec![long_user(20)],
        };
        let result = apply_compaction("  isi ringkasan  ", &plan);
        assert_eq!(result.len(), 2);
        match &result[0] {
            Message::User { content } => {
                assert!(content.starts_with("[ringkasan konteks sebelumnya]"));
                assert!(content.contains("isi ringkasan"));
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn anggaran_nol_mempertahankan_hanya_ekor_yang_aman() {
        let plan = plan_compaction(
            &[
                long_user(10),
                Message::tool_result("c1", "read", "isi", false, false),
                long_user(10),
            ],
            0,
        )
        .expect("harus ada yang diringkas");
        assert!(plan.keep.is_empty());
        assert_eq!(plan.to_summarize.len(), 3);
    }
}
