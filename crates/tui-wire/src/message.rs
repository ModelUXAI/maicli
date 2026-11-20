//! Bentuk pesan antara pengguna, model, dan tool.

use serde::{Deserialize, Serialize};

/// Alasan model berhenti (cermin omp `assistant-message.ts`).
///
/// `Stop` dan `ToolUse` sama-sama menjalankan tool; `Length` tidak boleh,
/// karena argumen tool bisa terpotong. Aturan pakainya ada di `tui-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Model selesai normal.
    Stop,
    /// Model meminta pemanggilan tool.
    ToolUse,
    /// Turn terpotong karena `max_tokens`.
    Length,
    /// Turn gagal karena error provider.
    Error,
    /// Turn dibatalkan pengguna.
    Aborted,
}

impl StopReason {
    /// Apakah turn ini boleh mengeksekusi tool yang dibawanya.
    pub const fn is_runnable(self) -> bool {
        matches!(self, Self::Stop | Self::ToolUse)
    }
}

/// Satu blok isi pesan assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Teks biasa.
    Text {
        /// Isi teks.
        text: String,
    },
    /// Blok penalaran model (bila provider mengirimkannya).
    Thinking {
        /// Isi penalaran.
        text: String,
    },
    /// Permintaan memanggil tool. `arguments` adalah JSON mentah seperti
    /// diterima dari provider (belum diparse, karena bisa terpotong).
    ToolUse {
        /// Id panggilan dari provider.
        id: String,
        /// Nama tool.
        name: String,
        /// Argumen JSON mentah.
        arguments: String,
    },
}

impl ContentBlock {
    /// Teks bila blok ini blok teks.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::Thinking { .. } | Self::ToolUse { .. } => None,
        }
    }

    /// Id panggilan bila blok ini permintaan tool.
    pub fn tool_use_id(&self) -> Option<&str> {
        match self {
            Self::ToolUse { id, .. } => Some(id),
            Self::Text { .. } | Self::Thinking { .. } => None,
        }
    }
}

/// Pemakaian token yang dilaporkan provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Token input yang ditagih.
    #[serde(default)]
    pub input_tokens: u64,
    /// Token output.
    #[serde(default)]
    pub output_tokens: u64,
    /// Token input yang dilayani dari cache (bila dilaporkan).
    #[serde(default)]
    pub cached_tokens: u64,
}

impl Usage {
    /// Jumlah token yang menempati window konteks.
    pub const fn context_tokens(self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Tambahkan pemakaian lain (mis. akumulasi antar-turn).
    pub fn accumulate(&mut self, other: Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(other.cached_tokens);
    }

    /// Selisih pemakaian dibanding `previous` (tidak pernah negatif).
    pub fn delta_since(self, previous: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_sub(previous.input_tokens),
            output_tokens: self.output_tokens.saturating_sub(previous.output_tokens),
            cached_tokens: self.cached_tokens.saturating_sub(previous.cached_tokens),
        }
    }

    /// Apakah tidak ada token sama sekali.
    pub fn is_zero(self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.cached_tokens == 0
    }
}

/// Pesan akhir satu turn assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMessage {
    /// Blok isi berurutan.
    pub content: Vec<ContentBlock>,
    /// Alasan berhenti.
    pub stop_reason: StopReason,
    /// Pemakaian token.
    #[serde(default)]
    pub usage: Usage,
    /// Pesan error provider bila `stop_reason` = `Error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AssistantMessage {
    /// Pesan dengan satu blok teks.
    pub fn text(text: impl Into<String>, stop_reason: StopReason) -> Self {
        Self {
            content: vec![ContentBlock::Text { text: text.into() }],
            stop_reason,
            usage: Usage::default(),
            error: None,
        }
    }

    /// Semua blok teks digabung.
    pub fn joined_text(&self) -> String {
        let mut out = String::new();
        for block in &self.content {
            if let ContentBlock::Text { text } = block {
                out.push_str(text);
            }
        }
        out
    }

    /// Semua permintaan tool di pesan ini, berurutan.
    pub fn tool_calls(&self) -> Vec<(&str, &str, &str)> {
        let mut out = Vec::new();
        for block in &self.content {
            if let ContentBlock::ToolUse {
                id,
                name,
                arguments,
            } = block
            {
                out.push((id.as_str(), name.as_str(), arguments.as_str()));
            }
        }
        out
    }

    /// Apakah turn ini membawa permintaan tool yang boleh dijalankan.
    pub fn has_runnable_tool_calls(&self) -> bool {
        self.stop_reason.is_runnable() && self.content.iter().any(|b| b.tool_use_id().is_some())
    }
}

/// Satu pesan dalam konteks percakapan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    /// Instruksi sistem.
    System {
        /// Isi instruksi.
        content: String,
    },
    /// Pesan pengguna.
    User {
        /// Isi pesan.
        content: String,
    },
    /// Balasan assistant, mungkin membawa permintaan tool.
    Assistant {
        /// Blok isi.
        content: Vec<ContentBlock>,
        /// Alasan berhenti.
        stop_reason: StopReason,
    },
    /// Hasil eksekusi tool yang dipasangkan ke satu `tool_use`.
    ToolResult {
        /// Id panggilan yang dijawab.
        tool_call_id: String,
        /// Nama tool.
        tool_name: String,
        /// Isi hasil (sudah dipotong bila melebihi cap).
        content: String,
        /// Apakah hasil ini error.
        is_error: bool,
        /// Apakah isi dipotong.
        truncated: bool,
    },
}

impl Message {
    /// Pesan pengguna.
    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
        }
    }

    /// Instruksi sistem.
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    /// Pesan assistant dari hasil satu turn.
    pub fn assistant(message: &AssistantMessage) -> Self {
        Self::Assistant {
            content: message.content.clone(),
            stop_reason: message.stop_reason,
        }
    }

    /// Hasil tool.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
        truncated: bool,
    ) -> Self {
        Self::ToolResult {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            content: content.into(),
            is_error,
            truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_dan_nol_pada_usage() {
        let before = Usage {
            input_tokens: 10,
            output_tokens: 4,
            cached_tokens: 2,
        };
        let after = Usage {
            input_tokens: 25,
            output_tokens: 9,
            cached_tokens: 2,
        };
        let delta = after.delta_since(before);
        assert_eq!(delta.input_tokens, 15);
        assert_eq!(delta.output_tokens, 5);
        assert_eq!(delta.cached_tokens, 0);
        assert!(!delta.is_zero());
        assert!(Usage::default().is_zero());
        assert!(
            before.delta_since(after).is_zero(),
            "selisih negatif dijepit ke nol"
        );
    }

    #[test]
    fn usage_terakumulasi_dengan_aman() {
        let mut total = Usage {
            input_tokens: 10,
            output_tokens: 5,
            cached_tokens: 2,
        };
        total.accumulate(Usage {
            input_tokens: 3,
            output_tokens: 7,
            cached_tokens: 1,
        });
        assert_eq!(
            total,
            Usage {
                input_tokens: 13,
                output_tokens: 12,
                cached_tokens: 3
            }
        );
        assert_eq!(total.context_tokens(), 25);
        total.accumulate(Usage {
            input_tokens: u64::MAX,
            output_tokens: 0,
            cached_tokens: 0,
        });
        assert_eq!(total.input_tokens, u64::MAX, "tidak boleh overflow");
    }

    #[test]
    fn stop_reason_hanya_stop_dan_tooluse_yang_runnable() {
        assert!(StopReason::Stop.is_runnable());
        assert!(StopReason::ToolUse.is_runnable());
        assert!(!StopReason::Length.is_runnable());
        assert!(!StopReason::Error.is_runnable());
        assert!(!StopReason::Aborted.is_runnable());
    }

    #[test]
    fn pesan_round_trip_lewat_json() {
        let message = Message::tool_result("call_1", "read", "isi berkas", false, false);
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("\"role\":\"tool_result\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn assistant_dengan_tool_call_terdeteksi() {
        let message = AssistantMessage {
            content: vec![
                ContentBlock::Text {
                    text: "membaca berkas".into(),
                },
                ContentBlock::ToolUse {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: "{\"path\":\"a.txt\"}".into(),
                },
            ],
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
            error: None,
        };
        assert!(message.has_runnable_tool_calls());
        assert_eq!(message.tool_calls().len(), 1);
        assert_eq!(message.joined_text(), "membaca berkas");
    }

    #[test]
    fn turn_length_tidak_boleh_menjalankan_tool() {
        let message = AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: "{\"command\":\"dir\"}".into(),
            }],
            stop_reason: StopReason::Length,
            usage: Usage::default(),
            error: None,
        };
        assert!(!message.has_runnable_tool_calls());
        assert_eq!(message.tool_calls().len(), 1);
    }
}
