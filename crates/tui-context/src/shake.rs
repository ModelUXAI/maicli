//! Metode compaction `shake`: buang konten besar yang bisa dipulihkan tanpa
//! memanggil model.
//!
//! Yang dibuang hanya isi hasil tool lama (lihat [`crate::prune`]); hasil tool
//! terbaru dipertahankan karena bisa jadi masih dipakai iterasi turn berjalan.
//! Tidak ada panggilan model, jadi metode ini murah dan aman dipakai lebih dulu.

use tui_wire::Message;

use crate::prune::prune_tool_results;
use crate::tokens::estimate_messages;

/// Jumlah minimal hasil tool terbaru yang selalu dipertahankan `shake`.
///
/// Delapan dipilih supaya beberapa iterasi tool terakhir dalam satu turn tetap
/// utuh; isi yang dipangkas bisa dibaca ulang dari disk bila benar-benar perlu.
pub const SHAKE_KEEP_LAST: usize = 8;

/// Hasil rencana `shake`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShakePlan {
    /// Pesan setelah isi hasil tool lama dipangkas.
    pub messages: Vec<Message>,
    /// Jumlah hasil tool yang isinya diganti placeholder.
    pub pruned: usize,
    /// Perkiraan token sebelum dipangkas.
    pub before_tokens: u64,
    /// Perkiraan token sesudah dipangkas.
    pub after_tokens: u64,
}

/// Buat rencana `shake` bila konteks melewati `target_tokens`.
///
/// Mengembalikan `None` bila konteks sudah cukup kecil atau tidak ada hasil tool
/// yang bisa dipangkas, sehingga pemanggil tidak mengganti konteks tanpa manfaat.
pub fn plan_shake(messages: &[Message], target_tokens: u64, keep_last: usize) -> Option<ShakePlan> {
    let before_tokens = estimate_messages(messages);
    if before_tokens <= target_tokens {
        return None;
    }
    let mut candidate = messages.to_vec();
    let pruned = prune_tool_results(&mut candidate, keep_last.max(SHAKE_KEEP_LAST));
    if pruned == 0 {
        return None;
    }
    let after_tokens = estimate_messages(&candidate);
    if after_tokens >= before_tokens {
        return None;
    }
    Some(ShakePlan {
        messages: candidate,
        pruned,
        before_tokens,
        after_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(content: &str) -> Message {
        Message::tool_result("call", "bash", content, false, false)
    }

    #[test]
    fn shake_memangkas_hasil_lama_dan_mempertahankan_terbaru() {
        let mut messages = vec![Message::user("mulai")];
        for _ in 0..20 {
            messages.push(result(&"x".repeat(2_000)));
            messages.push(Message::assistant(&tui_wire::AssistantMessage::text(
                "ok",
                tui_wire::StopReason::Stop,
            )));
        }
        let before = estimate_messages(&messages);
        let plan = plan_shake(&messages, 100, 1).expect("ada yang bisa dipangkas");
        assert!(plan.after_tokens < plan.before_tokens, "{plan:?}");
        assert_eq!(plan.before_tokens, before);
        let remaining = plan
            .messages
            .iter()
            .filter(|message| {
                matches!(message, Message::ToolResult { content, .. }
                    if content.as_str() != crate::PRUNED_TOOL_RESULT)
            })
            .count();
        assert_eq!(remaining, SHAKE_KEEP_LAST, "hanya hasil terbaru yang utuh");
    }

    #[test]
    fn shake_tidak_menyentuh_pesan_biasa() {
        let messages = vec![Message::user("y".repeat(10_000))];
        assert!(plan_shake(&messages, 10, 1).is_none());
    }

    #[test]
    fn shake_tidak_berjalan_bila_sudah_kecil() {
        let messages = vec![Message::user("halo"), result("kecil")];
        assert!(plan_shake(&messages, 10_000, 1).is_none());
    }
}
