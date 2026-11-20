//! Estimasi token tanpa tokenizer.
//!
//! Gateway tidak memberi tokenizer yang bisa dipakai lokal, jadi estimasi
//! memakai pendekatan karakter (≈4 karakter per token) ditambah framing per
//! pesan. Estimasi boleh meleset; yang penting **arah** dan konsistensi, karena
//! ambang compaction butuh angka yang selalu tersedia sebelum request dikirim.

use tui_wire::{ContentBlock, Message};

/// Framing per pesan (role, pemisah, penutup) dalam token.
pub const MESSAGE_FRAMING_TOKENS: u64 = 16;

/// Estimasi token untuk satu potong teks.
pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(4)
}

/// Estimasi token untuk satu pesan lengkap.
pub fn estimate_message(message: &Message) -> u64 {
    let body = match message {
        Message::System { content } | Message::User { content } => estimate_tokens(content),
        Message::Assistant { content, .. } => content.iter().map(estimate_block).sum(),
        Message::ToolResult {
            tool_name, content, ..
        } => estimate_tokens(tool_name).saturating_add(estimate_tokens(content)),
    };
    body.saturating_add(MESSAGE_FRAMING_TOKENS)
}

/// Estimasi token untuk seluruh konteks.
pub fn estimate_messages(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message).sum()
}

fn estimate_block(block: &ContentBlock) -> u64 {
    match block {
        ContentBlock::Text { text } | ContentBlock::Thinking { text } => estimate_tokens(text),
        ContentBlock::ToolUse {
            name, arguments, ..
        } => estimate_tokens(name).saturating_add(estimate_tokens(arguments)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_wire::{AssistantMessage, StopReason, Usage};

    #[test]
    fn teks_pendek_dibulatkan_ke_atas() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn pesan_pengguna_menambah_framing() {
        let message = Message::user("abcd");
        assert_eq!(estimate_message(&message), 1 + MESSAGE_FRAMING_TOKENS);
    }

    #[test]
    fn blok_tool_use_dihitung_dengan_argumen() {
        let message = Message::assistant(&AssistantMessage {
            content: vec![
                ContentBlock::Text {
                    text: "a".repeat(40),
                },
                ContentBlock::ToolUse {
                    id: "c1".into(),
                    name: "read".into(),
                    arguments: "x".repeat(40),
                },
            ],
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
            error: None,
        });
        // 10 (teks) + 1 (nama) + 10 (argumen) + 16 (framing)
        assert_eq!(estimate_message(&message), 37);
    }

    #[test]
    fn total_konteks_menjumlahkan_semua_pesan() {
        let messages = vec![Message::user("abcd"), Message::user("abcd")];
        assert_eq!(
            estimate_messages(&messages),
            2 * (1 + MESSAGE_FRAMING_TOKENS)
        );
    }
}
