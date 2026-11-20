//! Taksonomi error stabil (RENCANA.md §7.3).
//!
//! Kode ini muncul di UI, log, dan file sesi. Mengubah namanya = mengubah
//! kontrak yang dibaca manusia dan alat lain, jadi nama dipertahankan.

use std::fmt;

/// Kode error yang stabil. Nilai string-nya adalah bagian dari kontrak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ErrorCode {
    /// Gateway tidak terjangkau atau timeout saat connect.
    #[serde(rename = "E_GATEWAY_UNREACHABLE")]
    GatewayUnreachable,
    /// Gateway menolak kredensial (401/403).
    #[serde(rename = "E_GATEWAY_AUTH")]
    GatewayAuth,
    /// Gateway membatasi laju (429).
    #[serde(rename = "E_GATEWAY_RATE_LIMIT")]
    GatewayRateLimit,
    /// Gateway gagal di sisi server (5xx).
    #[serde(rename = "E_GATEWAY_5XX")]
    GatewayServer,
    /// Gateway menolak payload, memblokir konten, atau konteks melebihi window.
    #[serde(rename = "E_GATEWAY_REJECTED")]
    GatewayRejected,
    /// Model tidak ada pada hasil discovery.
    #[serde(rename = "E_MODEL_UNKNOWN")]
    ModelUnknown,
    /// Approval ditolak atau timeout.
    #[serde(rename = "E_TOOL_DENIED")]
    ToolDenied,
    /// Tool gagal menjalankan aksinya.
    #[serde(rename = "E_TOOL_FAILED")]
    ToolFailed,
    /// Tool melewati batas waktu.
    #[serde(rename = "E_TOOL_TIMEOUT")]
    ToolTimeout,
    /// File sesi tidak bisa diparse.
    #[serde(rename = "E_SESSION_CORRUPT")]
    SessionCorrupt,
    /// Config tidak valid.
    #[serde(rename = "E_CONFIG")]
    Config,
    /// Tidak ada kunci API di env maupun keyring.
    #[serde(rename = "E_KEY_MISSING")]
    KeyMissing,
    /// Sesi sedang sibuk dengan turn lain.
    #[serde(rename = "E_AGENT_BUSY")]
    AgentBusy,
    /// Permintaan masuk (RPC) tidak bisa diparse.
    #[serde(rename = "E_PROTOCOL")]
    Protocol,
    /// Sesi yang diminta untuk dilanjutkan tidak ditemukan.
    #[serde(rename = "E_SESSION_NOT_FOUND")]
    SessionNotFound,
    /// File sesi sedang dipakai proses lain.
    #[serde(rename = "E_SESSION_LOCKED")]
    SessionLocked,
    /// Sesi sedang dijeda.
    #[serde(rename = "E_AGENT_PAUSED")]
    AgentPaused,
}

impl ErrorCode {
    /// Kode seperti yang ditulis di log dan UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GatewayUnreachable => "E_GATEWAY_UNREACHABLE",
            Self::GatewayAuth => "E_GATEWAY_AUTH",
            Self::GatewayRateLimit => "E_GATEWAY_RATE_LIMIT",
            Self::GatewayServer => "E_GATEWAY_5XX",
            Self::GatewayRejected => "E_GATEWAY_REJECTED",
            Self::ModelUnknown => "E_MODEL_UNKNOWN",
            Self::ToolDenied => "E_TOOL_DENIED",
            Self::ToolFailed => "E_TOOL_FAILED",
            Self::ToolTimeout => "E_TOOL_TIMEOUT",
            Self::SessionCorrupt => "E_SESSION_CORRUPT",
            Self::Config => "E_CONFIG",
            Self::KeyMissing => "E_KEY_MISSING",
            Self::AgentBusy => "E_AGENT_BUSY",
            Self::Protocol => "E_PROTOCOL",
            Self::SessionNotFound => "E_SESSION_NOT_FOUND",
            Self::SessionLocked => "E_SESSION_LOCKED",
            Self::AgentPaused => "E_AGENT_PAUSED",
        }
    }

    /// Kalimat untuk pengguna: apa yang terjadi dan apa yang bisa dilakukan.
    pub const fn user_message(self) -> &'static str {
        match self {
            Self::GatewayUnreachable => {
                "Gateway tidak terjangkau. Periksa koneksi dan alamat gateway di config."
            }
            Self::GatewayAuth => {
                "Kunci API ditolak gateway. Periksa MODELUXAI_API_KEY atau keyring."
            }
            Self::GatewayRateLimit => {
                "Gateway membatasi permintaan. Tunggu sebentar lalu coba lagi."
            }
            Self::GatewayServer => "Gateway sedang bermasalah. Coba lagi beberapa saat lagi.",
            Self::GatewayRejected => {
                "Permintaan ditolak gateway. Pendekkan prompt atau jalankan /compact."
            }
            Self::ModelUnknown => "Model tidak ada di gateway. Pilih model dari /model.",
            Self::ToolDenied => "Tool tidak dijalankan karena persetujuan ditolak.",
            Self::ToolFailed => "Tool gagal. Periksa path atau argumen yang dipakai.",
            Self::ToolTimeout => "Tool melewati batas waktu dan dihentikan.",
            Self::SessionCorrupt => "File sesi rusak. Pulihkan dari backup atau mulai sesi baru.",
            Self::Config => "Config tidak valid. Perbaiki file yang disebut pada pesan.",
            Self::KeyMissing => {
                "Kunci API belum ada. Isi environment MODELUXAI_API_KEY atau keyring Windows."
            }
            Self::AgentBusy => "Sesi masih memproses turn sebelumnya. Tunggu sampai selesai.",
            Self::Protocol => "Permintaan tidak dikenali. Periksa bentuk JSON dan field type.",
            Self::SessionNotFound => {
                "Sesi tidak ditemukan. Jalankan `mx --list-sessions` untuk melihat id."
            }
            Self::SessionLocked => {
                "File sesi sedang dipakai proses lain. Tutup proses itu, atau hapus file .lock bila prosesnya sudah mati."
            }
            Self::AgentPaused => "Sesi sedang dijeda. Jalankan /resume untuk melanjutkan.",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error yang membawa kode stabil plus pesan teknis.
///
/// `source` menyimpan penyebab asli (mis. error transport) tanpa pernah ikut
/// dicetak ke pengguna akhir.
#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct MxError {
    /// Kode stabil.
    pub code: ErrorCode,
    /// Pesan teknis singkat (boleh berbahasa Inggris untuk detail library).
    pub message: String,
    /// Penyebab asli, tidak untuk ditampilkan ke pengguna.
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl MxError {
    /// Bangun error dengan kode dan pesan.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
        }
    }

    /// Bangun error yang membawa penyebab asli.
    pub fn with_source(
        code: ErrorCode,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Kalimat siap tampil untuk pengguna.
    pub fn user_message(&self) -> &'static str {
        self.code.user_message()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kode_serialisasi_stabil() {
        let json = serde_json::to_string(&ErrorCode::GatewayUnreachable).unwrap();
        assert_eq!(json, "\"E_GATEWAY_UNREACHABLE\"");
        let back: ErrorCode = serde_json::from_str("\"E_TOOL_TIMEOUT\"").unwrap();
        assert_eq!(back, ErrorCode::ToolTimeout);
    }

    #[test]
    fn setiap_kode_punya_pesan_pengguna() {
        for code in [
            ErrorCode::GatewayUnreachable,
            ErrorCode::GatewayAuth,
            ErrorCode::GatewayRateLimit,
            ErrorCode::GatewayServer,
            ErrorCode::GatewayRejected,
            ErrorCode::ModelUnknown,
            ErrorCode::ToolDenied,
            ErrorCode::ToolFailed,
            ErrorCode::ToolTimeout,
            ErrorCode::SessionCorrupt,
            ErrorCode::Config,
            ErrorCode::KeyMissing,
            ErrorCode::AgentBusy,
        ] {
            assert!(!code.user_message().is_empty(), "kode {code} tanpa pesan");
        }
    }

    #[test]
    fn display_memuat_kode_dan_pesan() {
        let err = MxError::new(ErrorCode::Config, "baris 3: field tidak dikenal");
        let text = err.to_string();
        assert!(text.contains("E_CONFIG"));
        assert!(text.contains("baris 3"));
    }
}
