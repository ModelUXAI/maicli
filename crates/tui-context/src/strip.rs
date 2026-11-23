//! Pembersihan blok konteks yang tidak perlu dikirim ulang ke model.
//!
//! `/shake thinking` membuang blok penalaran lama; teks jawaban dan permintaan
//! tool tetap utuh supaya pasangan `tool_use`/`tool_result` tidak rusak.

use tui_wire::{ContentBlock, Message};

/// Buang semua blok `Thinking` dari pesan assistant; mengembalikan jumlah blok.
pub fn strip_thinking(messages: &mut [Message]) -> usize {
    let mut removed = 0;
    for message in messages {
        if let Message::Assistant { content, .. } = message {
            let before = content.len();
            content.retain(|block| !matches!(block, ContentBlock::Thinking { .. }));
            removed += before - content.len();
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_wire::{AssistantMessage, StopReason};

    fn assistant_with_thinking() -> Message {
        let mut message = AssistantMessage::text("jawaban", StopReason::Stop);
        message.content.insert(
            0,
            ContentBlock::Thinking {
                text: "penalaran panjang".to_string(),
            },
        );
        Message::assistant(&message)
    }

    #[test]
    fn thinking_dibuang_teks_tetap() {
        let mut messages = vec![Message::user("tanya"), assistant_with_thinking()];
        assert_eq!(strip_thinking(&mut messages), 1);
        match &messages[1] {
            Message::Assistant { content, .. } => {
                assert_eq!(content.len(), 1);
                assert!(content[0].as_text() == Some("jawaban"));
            }
            other => panic!("bentuk tak terduga: {other:?}"),
        }
    }

    #[test]
    fn tanpa_blok_thinking_tidak_berubah() {
        let mut messages = vec![Message::user("halo")];
        assert_eq!(strip_thinking(&mut messages), 0);
    }
}
