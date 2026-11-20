//! Event dua tingkat: tingkat provider (aliran mentah) dan tingkat agen
//! (peristiwa turn). Nama varian mengikuti omp agar urutan event bisa
//! dibandingkan langsung oleh harness differential.

use crate::error::ErrorCode;
use crate::message::AssistantMessage;

/// Event tingkat provider: satu aliran dari satu panggilan model.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum ProviderEvent {
    /// Panggilan model dimulai.
    Start,
    /// Awal satu blok teks.
    TextStart,
    /// Potongan teks.
    TextDelta {
        /// Potongan yang baru tiba.
        text: String,
    },
    /// Akhir blok teks.
    TextEnd,
    /// Awal satu blok penalaran.
    ThinkingStart,
    /// Potongan penalaran.
    ThinkingDelta {
        /// Potongan yang baru tiba.
        text: String,
    },
    /// Akhir blok penalaran.
    ThinkingEnd,
    /// Awal satu permintaan tool (id dan nama biasanya tiba di sini).
    ToolCallStart {
        /// Indeks panggilan dalam pesan ini.
        index: u32,
        /// Id panggilan dari provider.
        id: String,
        /// Nama tool.
        name: String,
    },
    /// Potongan argumen JSON untuk satu panggilan tool.
    ToolCallDelta {
        /// Indeks panggilan.
        index: u32,
        /// Potongan argumen.
        arguments: String,
    },
    /// Akhir satu permintaan tool.
    ToolCallEnd {
        /// Indeks panggilan.
        index: u32,
    },
    /// Panggilan selesai; membawa pesan yang sudah dirakit.
    Done {
        /// Pesan akhir.
        message: AssistantMessage,
    },
    /// Panggilan gagal.
    Error {
        /// Kode stabil untuk UI.
        code: ErrorCode,
        /// Pesan teknis.
        message: String,
    },
}

/// Satu sesi yang bisa dipilih dari daftar di UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionChoice {
    /// Id sesi.
    pub id: String,
    /// Judul sesi.
    pub title: String,
    /// Jumlah pesan.
    pub messages: usize,
}

/// Event tingkat agen: yang dikonsumsi TUI dan store sesi.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum AgentEvent {
    /// Turn pertama dimulai.
    AgentStart,
    /// Bersihkan timeline UI (balasan `/clear`).
    ClearTimeline,
    /// Satu turn dimulai.
    TurnStart {
        /// Nomor turn, mulai 1.
        index: u32,
    },
    /// Pesan assistant mulai mengalir.
    MessageStart,
    /// Potongan dari provider diteruskan apa adanya.
    MessageUpdate(ProviderEvent),
    /// Pesan assistant selesai dirakit.
    MessageEnd {
        /// Pesan akhir turn ini.
        message: AssistantMessage,
    },
    /// Tool mulai dieksekusi (setelah approval).
    ToolExecutionStart {
        /// Id panggilan.
        tool_call_id: String,
        /// Nama tool.
        tool_name: String,
        /// Cuplikan argumen untuk ditampilkan.
        preview: String,
    },
    /// Tool selesai dieksekusi.
    ToolExecutionEnd {
        /// Id panggilan.
        tool_call_id: String,
        /// Nama tool.
        tool_name: String,
        /// Apakah hasilnya error.
        is_error: bool,
        /// Cuplikan hasil untuk ditampilkan.
        preview: String,
    },
    /// Satu turn selesai.
    TurnEnd {
        /// Nomor turn.
        index: u32,
    },
    /// Seluruh turn (termasuk follow-up) selesai.
    AgentEnd {
        /// Jumlah turn yang dijalankan.
        turns: u32,
    },
    /// Pemakaian konteks terbaru untuk indikator di UI.
    ContextUsage {
        /// Perkiraan token konteks saat ini.
        tokens: u64,
        /// Batas window konteks.
        window: u64,
    },
    /// Daftar sesi tersimpan untuk pemilih di UI.
    SessionsListed {
        /// Sesi, terbaru lebih dulu.
        sessions: Vec<SessionChoice>,
    },
    /// Sesi aktif berganti (dipakai mode TUI yang mengganti sesi tanpa keluar).
    SessionSwitched {
        /// Path file sesi yang baru.
        path: String,
        /// Jumlah pesan yang dimuat.
        messages: usize,
    },
    /// Pesan informasi untuk UI (bukan bagian protokol provider).
    Notice {
        /// Teks yang ditampilkan apa adanya.
        text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ContentBlock, StopReason, Usage};

    #[test]
    fn event_provider_bisa_dibandingkan() {
        let a = ProviderEvent::TextDelta {
            text: "halo".into(),
        };
        let b = ProviderEvent::TextDelta {
            text: "halo".into(),
        };
        let c = ProviderEvent::TextDelta { text: "hai".into() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn event_done_membawa_pesan() {
        let message = AssistantMessage {
            content: vec![ContentBlock::Text { text: "ok".into() }],
            stop_reason: StopReason::Stop,
            usage: Usage::default(),
            error: None,
        };
        let event = AgentEvent::MessageEnd {
            message: message.clone(),
        };
        match event {
            AgentEvent::MessageEnd { message: ended } => assert_eq!(ended, message),
            _ => unreachable!(),
        }
    }
}
