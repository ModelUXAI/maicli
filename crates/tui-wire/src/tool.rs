//! Kontrak tool: tier, definisi untuk model, panggilan, dan hasil.

use serde::{Deserialize, Serialize};

/// Kapan skema tool dimuat ke permintaan model (RENCANA C-04 `loadMode`).
///
/// v1 selalu `Eager` (semua skema dikirim setiap panggilan); `Deferred` disiapkan
/// sebagai kontrak untuk tool yang skemanya dimuat saat dipakai.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoadMode {
    /// Skema selalu dikirim bersama definisi tool lain.
    #[default]
    Eager,
    /// Skema baru dimuat ketika tool dipakai.
    Deferred,
}

/// Petunjuk gaya render hasil tool di UI (RENCANA C-04 `render`).
///
/// Ini bukan fungsi render (UI yang memutuskan tampilan), hanya kategori yang
/// stabil agar TUI/teks bisa memilih format: teks biasa, keluaran perintah,
/// atau diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderHint {
    /// Teks biasa (read/grep/glob/todo/ask/write).
    #[default]
    Plain,
    /// Keluaran perintah shell (bash).
    Command,
    /// Perubahan berkas (edit).
    Diff,
}

/// Metadata tampilan untuk satu tool (RENCANA C-04 `AgentTool`).
///
/// Dipisah dari [`ToolDefinition`] karena definisi adalah bentuk wire yang
/// dikirim ke gateway (tidak boleh membawa label/loadMode/render ke payload API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentToolMeta {
    /// Nama tampilan singkat di timeline.
    pub label: String,
    /// Kapan skema dimuat.
    pub load_mode: LoadMode,
    /// Gaya render hasil.
    pub render: RenderHint,
}

impl AgentToolMeta {
    /// Metadata default: label = nama tool, eager, teks biasa.
    pub fn plain(name: &str) -> Self {
        Self {
            label: name.to_string(),
            load_mode: LoadMode::Eager,
            render: RenderHint::Plain,
        }
    }
}

/// Klasifikasi tool untuk kebijakan approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolTier {
    /// Hanya membaca (read, glob, grep, todo, ask).
    Read,
    /// Mengubah berkas (write, edit).
    Write,
    /// Menjalankan proses (bash, task).
    Exec,
}

impl ToolTier {
    /// Peringkat untuk perbandingan mode approval (cermin omp `TIER_RANK`).
    pub const fn rank(self) -> u8 {
        match self {
            Self::Read => 0,
            Self::Write => 1,
            Self::Exec => 2,
        }
    }
}

/// Bagaimana tool boleh berjalan bersamaan dengan tool lain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Concurrency {
    /// Boleh berjalan bersamaan dengan tool `Shared` lain.
    #[default]
    Shared,
    /// Harus sendirian.
    Exclusive,
}

/// Definisi tool seperti yang dikirim ke model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Nama unik (dipakai model).
    pub name: String,
    /// Deskripsi 1–3 kalimat untuk model.
    pub description: String,
    /// Skema JSON parameter.
    pub parameters: serde_json::Value,
    /// Konkurensi eksekusi: `shared` boleh berjalan paralel, `exclusive` sendiri.
    #[serde(default)]
    pub concurrency: Concurrency,
}

/// Satu panggilan tool dari model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// Id panggilan.
    pub id: String,
    /// Nama tool.
    pub name: String,
    /// Argumen JSON mentah.
    pub arguments: String,
}

impl ToolCall {
    /// Bangun panggilan dari bagian-bagian mentah.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

/// Hasil eksekusi tool yang siap menjadi `tool_result`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolOutput {
    /// Isi hasil untuk model.
    pub content: String,
    /// Apakah hasil ini error.
    pub is_error: bool,
    /// Apakah isi dipotong karena cap output.
    pub truncated: bool,
}

impl ToolOutput {
    /// Hasil sukses.
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            truncated: false,
        }
    }

    /// Hasil error.
    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            truncated: false,
        }
    }

    /// Tandai hasil sebagai terpotong.
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peringkat_tier_berurutan() {
        assert!(ToolTier::Read.rank() < ToolTier::Write.rank());
        assert!(ToolTier::Write.rank() < ToolTier::Exec.rank());
    }

    #[test]
    fn tier_serialisasi_huruf_kecil() {
        assert_eq!(serde_json::to_string(&ToolTier::Exec).unwrap(), "\"exec\"");
    }
}
