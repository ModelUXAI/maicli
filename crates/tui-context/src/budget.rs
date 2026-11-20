//! Anggaran konteks dan ambang compaction.

/// Token yang disisakan untuk jawaban model (cermin omp `DEFAULT_RESERVE_TOKENS`).
pub const DEFAULT_RESERVE_TOKENS: u64 = 16_384;

/// Token terakhir yang selalu dipertahankan saat compaction.
pub const DEFAULT_KEEP_RECENT_TOKENS: u64 = 20_000;

/// Setelah compaction, konteks harus turun ke bawah fraksi ini dari window
/// terpakai. Nilai 0,8 mengikuti `COMPACTION_RECOVERY_BAND` omp.
pub const COMPACTION_RECOVERY_BAND: f64 = 0.8;

/// Anggaran konteks satu sesi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Ukuran window model dalam token.
    pub context_window: u64,
    /// Token yang tidak boleh dipakai konteks (untuk output).
    pub reserve_tokens: u64,
    /// Token terakhir yang dipertahankan saat compaction.
    pub keep_recent_tokens: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            reserve_tokens: DEFAULT_RESERVE_TOKENS,
            keep_recent_tokens: DEFAULT_KEEP_RECENT_TOKENS,
        }
    }
}

impl Budget {
    /// Anggaran baru; `reserve` dan `keep` di-clamp agar tidak melebihi window.
    pub fn new(context_window: u64, reserve_tokens: u64, keep_recent_tokens: u64) -> Self {
        let reserve = reserve_tokens.min(context_window);
        Self {
            context_window,
            reserve_tokens: reserve,
            keep_recent_tokens: keep_recent_tokens.min(context_window.saturating_sub(reserve)),
        }
    }

    /// Token yang boleh dipakai konteks.
    pub fn usable(&self) -> u64 {
        self.context_window.saturating_sub(self.reserve_tokens)
    }

    /// Apakah konteks harus diringkas sebelum panggilan berikutnya.
    pub fn needs_compaction(&self, used_tokens: u64) -> bool {
        used_tokens > self.usable()
    }

    /// Ambang "pulih" setelah compaction (0,8 × usable).
    pub fn recovery_target(&self) -> u64 {
        (self.usable() as f64 * COMPACTION_RECOVERY_BAND) as u64
    }

    /// Apakah hasil compaction sudah cukup kecil.
    pub fn within_recovery(&self, used_tokens: u64) -> bool {
        used_tokens <= self.recovery_target()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sesuai_konstanta_omp() {
        let budget = Budget::default();
        assert_eq!(budget.reserve_tokens, 16_384);
        assert_eq!(budget.keep_recent_tokens, 20_000);
        assert_eq!(budget.usable(), 128_000 - 16_384);
    }

    #[test]
    fn ambang_compaction_di_batas_usable() {
        let budget = Budget::new(10_000, 4_000, 2_000);
        assert!(!budget.needs_compaction(6_000));
        assert!(budget.needs_compaction(6_001));
    }

    #[test]
    fn recovery_target_80_persen() {
        let budget = Budget::new(10_000, 4_000, 2_000);
        assert_eq!(budget.recovery_target(), 4_800);
        assert!(budget.within_recovery(4_800));
        assert!(!budget.within_recovery(4_801));
    }

    #[test]
    fn nilai_aneh_di_clamp() {
        let budget = Budget::new(1_000, 5_000, 5_000);
        assert_eq!(budget.reserve_tokens, 1_000);
        assert_eq!(budget.usable(), 0);
        assert_eq!(budget.keep_recent_tokens, 0);
    }
}
