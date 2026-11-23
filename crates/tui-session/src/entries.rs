//! Bentuk entry transkrip sesi v1.
//!
//! Skema mengikuti `docs/skema-sesi-v1.md`: satu file JSONL, baris pertama
//! header `type:"session"`, lalu entry yang menunjuk `parent_id` sehingga
//! percabangan (fork) bisa ditambahkan tanpa mengubah format.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tui_wire::Message;

/// Versi format yang ditulis biner ini.
pub const SESSION_VERSION: u32 = 1;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Id unik yang stabil dalam satu proses (waktu + urutan).
///
/// Bukan UUID: cukup unik untuk satu mesin dan satu proses, tanpa dependensi
/// tambahan. Bentrok antar-mesin tidak relevan karena sesi tidak disinkronkan.
pub fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{seq:x}")
}

/// Waktu sekarang dalam milidetik sejak epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Satu baris transkrip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionEntry {
    /// Header sesi; selalu baris pertama.
    Session {
        /// Versi format.
        version: u32,
        /// Id sesi.
        id: String,
        /// Judul sesi.
        title: String,
        /// Waktu dibuat (ms sejak epoch).
        created_at: u64,
        /// Direktori kerja saat sesi dibuat.
        cwd: String,
        /// Id sesi induk bila ini hasil fork.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_session: Option<String>,
    },
    /// Satu pesan percakapan.
    Message {
        /// Id entry.
        id: String,
        /// Id entry induk (untuk percabangan).
        parent_id: Option<String>,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Isi pesan.
        message: Message,
    },
    /// Pemakaian token satu turn.
    ModelUsage {
        /// Id entry.
        id: String,
        /// Id entry induk.
        parent_id: Option<String>,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Model yang dipakai.
        model: String,
        /// Token input.
        input_tokens: u64,
        /// Token output.
        output_tokens: u64,
    },
    /// Pergantian model.
    ModelChange {
        /// Id entry.
        id: String,
        /// Id entry induk.
        parent_id: Option<String>,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Model baru.
        model: String,
    },
    /// Hasil compaction (ringkasan menggantikan sebagian konteks).
    ///
    /// Saat sesi dilanjutkan, semua pesan **sebelum** entry ini digantikan oleh
    /// satu pesan ringkasan.
    Compaction {
        /// Id entry.
        id: String,
        /// Id entry induk.
        parent_id: Option<String>,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Ringkasan yang menggantikan konteks lama.
        summary: String,
        /// Jumlah pesan yang digantikan.
        replaced_messages: u64,
    },
    /// Perubahan judul sesi.
    TitleChange {
        /// Id entry.
        id: String,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Judul baru.
        title: String,
    },
    /// Keputusan approval yang perlu diaudit.
    ApprovalDecision {
        /// Id entry.
        id: String,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Nama tool.
        tool: String,
        /// Subjek (perintah atau path).
        subject: String,
        /// Tier tool (`read`/`write`/`exec`).
        #[serde(default)]
        tier: String,
        /// Keputusan yang diambil.
        decision: String,
    },
    /// Penanda sematan pesan di timeline.
    ///
    /// `index` = nomor pesan 1-based pada transkrip sesi; `pinned=false`
    /// melepas sematan tanpa menghapus riwayat.
    Pin {
        /// Id entry.
        id: String,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Nomor pesan (1-based).
        index: u64,
        /// Apakah pesan disematkan.
        pinned: bool,
        /// Cuplikan isi pesan saat disematkan.
        preview: String,
    },
    /// Penanda awal sesi (cwd, versi binary).
    SessionInit {
        /// Id entry.
        id: String,
        /// Waktu entry ditulis.
        timestamp: u64,
        /// Direktori kerja.
        cwd: String,
        /// Versi biner yang menulis sesi.
        app_version: String,
    },
}

impl SessionEntry {
    /// Entry pesan baru.
    pub fn message(parent_id: Option<String>, message: Message) -> Self {
        Self::Message {
            id: new_id(),
            parent_id,
            timestamp: now_ms(),
            message,
        }
    }

    /// Entry sematan pesan.
    pub fn pin(index: u64, pinned: bool, preview: impl Into<String>) -> Self {
        Self::Pin {
            id: new_id(),
            timestamp: now_ms(),
            index,
            pinned,
            preview: preview.into(),
        }
    }

    /// Entry pemakaian token satu turn.
    pub fn model_usage(model: impl Into<String>, input_tokens: u64, output_tokens: u64) -> Self {
        Self::ModelUsage {
            id: new_id(),
            parent_id: None,
            timestamp: now_ms(),
            model: model.into(),
            input_tokens,
            output_tokens,
        }
    }

    /// Entry perubahan model aktif.
    pub fn model_change(model: impl Into<String>) -> Self {
        Self::ModelChange {
            id: new_id(),
            parent_id: None,
            timestamp: now_ms(),
            model: model.into(),
        }
    }

    /// Entry perubahan judul sesi.
    pub fn title_change(title: impl Into<String>) -> Self {
        Self::TitleChange {
            id: new_id(),
            timestamp: now_ms(),
            title: title.into(),
        }
    }

    /// Entry audit keputusan approval.
    pub fn approval(
        tool: impl Into<String>,
        tier: impl Into<String>,
        subject: impl Into<String>,
        decision: impl Into<String>,
    ) -> Self {
        Self::ApprovalDecision {
            id: new_id(),
            timestamp: now_ms(),
            tool: tool.into(),
            subject: subject.into(),
            tier: tier.into(),
            decision: decision.into(),
        }
    }

    /// Entry compaction baru.
    pub fn compaction(
        parent_id: Option<String>,
        summary: impl Into<String>,
        replaced: u64,
    ) -> Self {
        Self::Compaction {
            id: new_id(),
            parent_id,
            timestamp: now_ms(),
            summary: summary.into(),
            replaced_messages: replaced,
        }
    }

    /// Id entry (header sesi memakai id sesi).
    pub fn id(&self) -> &str {
        match self {
            Self::Session { id, .. }
            | Self::Message { id, .. }
            | Self::ModelUsage { id, .. }
            | Self::ModelChange { id, .. }
            | Self::Compaction { id, .. }
            | Self::TitleChange { id, .. }
            | Self::ApprovalDecision { id, .. }
            | Self::Pin { id, .. }
            | Self::SessionInit { id, .. } => id,
        }
    }

    /// Apakah entry ini ikut ke konteks model.
    ///
    /// Entry metadata (judul, approval, init) tidak pernah dikirim ke model.
    pub fn is_context_message(&self) -> bool {
        matches!(self, Self::Message { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_unik_dalam_proses() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }

    #[test]
    fn approval_serialisasi_dengan_tier() {
        let entry = SessionEntry::approval("bash", "exec", "dir", "Once");
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""type":"approval_decision""#), "{json}");
        assert!(json.contains(r#""tier":"exec""#), "{json}");
        assert!(json.contains(r#""decision":"Once""#), "{json}");
        // File lama tanpa `tier` tetap bisa dibaca.
        let old = r#"{"type":"approval_decision","id":"x","timestamp":1,"tool":"read","subject":"a","decision":"Deny"}"#;
        let parsed: SessionEntry = serde_json::from_str(old).unwrap();
        assert!(matches!(parsed, SessionEntry::ApprovalDecision { tier, .. } if tier.is_empty()));
    }

    #[test]
    fn header_serialisasi_dengan_tag_type() {
        let header = SessionEntry::Session {
            version: SESSION_VERSION,
            id: "s1".into(),
            title: "uji".into(),
            created_at: 1,
            cwd: "C:\\kerja".into(),
            parent_session: None,
        };
        let json = serde_json::to_string(&header).unwrap();
        assert!(json.starts_with("{\"type\":\"session\""));
        assert!(!json.contains("parent_session"));
        let back: SessionEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, header);
    }

    #[test]
    fn hanya_entry_pesan_masuk_konteks() {
        let message = SessionEntry::message(None, Message::user("halo"));
        assert!(message.is_context_message());
        let title = SessionEntry::TitleChange {
            id: "x".into(),
            timestamp: 1,
            title: "t".into(),
        };
        assert!(!title.is_context_message());
    }
}
