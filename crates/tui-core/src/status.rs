//! Status satu turn beserta transisi yang sah (RENCANA §7.2).
//!
//! Status ini menggambarkan **turn**, bukan sesi: `Aborted` dan `Error` adalah
//! terminal untuk turn itu, sementara sesi tetap hidup dan turn berikutnya mulai
//! dari `Idle`. Modul ini murni supaya transisinya bisa diuji deterministik.

/// Status satu turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    /// Tidak ada panggilan model yang berjalan.
    Idle,
    /// Menunggu/menerima delta streaming dari provider.
    Streaming,
    /// Menjalankan tool call hasil turn sebelumnya.
    ExecutingTools,
    /// Tertunda di gate approval (menunggu keputusan pengguna).
    AwaitingApproval,
    /// Turn dihentikan (pembatalan atau gate).
    Aborted,
    /// Turn berakhir karena error provider atau kegagalan lain.
    Error,
}

impl TurnStatus {
    /// Apakah transisi `self -> next` sah menurut diagram §7.2.
    ///
    /// Aturan: `Idle` hanya bisa masuk ke `Streaming` (atau tetap `Idle`);
    /// `Streaming` boleh berakhir (`Idle`/`Aborted`/`Error`) atau lanjut ke tool;
    /// `ExecutingTools` boleh meminta approval, memanggil model lagi, atau
    /// berakhir; `AwaitingApproval` kembali ke eksekusi atau berakhir;
    /// status terminal turn (`Aborted`/`Error`) hanya boleh kembali ke `Idle`
    /// untuk turn berikutnya.
    pub fn can_transition_to(self, next: TurnStatus) -> bool {
        use TurnStatus::{Aborted, AwaitingApproval, Error, ExecutingTools, Idle, Streaming};
        match self {
            Idle => matches!(next, Idle | Streaming),
            Streaming => matches!(next, Streaming | ExecutingTools | Idle | Aborted | Error),
            ExecutingTools => matches!(
                next,
                ExecutingTools | Streaming | AwaitingApproval | Idle | Aborted | Error
            ),
            AwaitingApproval => matches!(next, AwaitingApproval | ExecutingTools | Aborted | Error),
            Aborted | Error => matches!(next, Idle),
        }
    }

    /// Status terminal untuk satu turn.
    pub fn is_turn_terminal(self) -> bool {
        matches!(self, TurnStatus::Aborted | TurnStatus::Error)
    }

    /// Label stabil untuk log/UI.
    pub fn label(self) -> &'static str {
        match self {
            TurnStatus::Idle => "idle",
            TurnStatus::Streaming => "streaming",
            TurnStatus::ExecutingTools => "executing_tools",
            TurnStatus::AwaitingApproval => "awaiting_approval",
            TurnStatus::Aborted => "aborted",
            TurnStatus::Error => "error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transisi_sah_mengikuti_diagram() {
        use TurnStatus::*;
        // Jalur normal: idle → streaming → executing_tools → streaming → idle.
        assert!(Idle.can_transition_to(Streaming));
        assert!(Streaming.can_transition_to(ExecutingTools));
        assert!(ExecutingTools.can_transition_to(Streaming));
        assert!(Streaming.can_transition_to(Idle));
        // Approval tertunda dan kembali ke eksekusi.
        assert!(ExecutingTools.can_transition_to(AwaitingApproval));
        assert!(AwaitingApproval.can_transition_to(ExecutingTools));
        // Turn berakhir karena batal/error.
        assert!(Streaming.can_transition_to(Aborted));
        assert!(AwaitingApproval.can_transition_to(Error));
        // Turn berikutnya mulai dari idle.
        assert!(Aborted.can_transition_to(Idle));
        assert!(Error.can_transition_to(Idle));
    }

    #[test]
    fn transisi_tidak_sah_ditolak() {
        use TurnStatus::*;
        assert!(
            !Idle.can_transition_to(ExecutingTools),
            "tool tanpa streaming"
        );
        assert!(!Idle.can_transition_to(Aborted), "belum ada turn berjalan");
        assert!(
            !AwaitingApproval.can_transition_to(Streaming),
            "approval bukan streaming"
        );
        assert!(
            !AwaitingApproval.can_transition_to(Idle),
            "harus lewat eksekusi/batal"
        );
        assert!(
            !Aborted.can_transition_to(Streaming),
            "turn batal harus idle dulu"
        );
        assert!(!Error.can_transition_to(ExecutingTools));
    }

    #[test]
    fn label_dan_terminal_stabil() {
        assert_eq!(TurnStatus::AwaitingApproval.label(), "awaiting_approval");
        assert!(TurnStatus::Aborted.is_turn_terminal());
        assert!(TurnStatus::Error.is_turn_terminal());
        assert!(!TurnStatus::Idle.is_turn_terminal());
        assert!(!TurnStatus::ExecutingTools.is_turn_terminal());
    }
}
