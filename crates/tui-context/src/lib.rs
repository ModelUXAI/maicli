//! Konteks: estimasi token, anggaran window, pruning hasil tool lama, dan
//! perencanaan compaction.
//!
//! Modul ini murni (tanpa I/O dan tanpa async) supaya ambang batas bisa diuji
//! deterministik. Pemanggilan model untuk ringkasan ada di `tui-core`.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod budget;
pub mod plan;
pub mod prune;
pub mod shake;
pub mod tokens;

pub use budget::{
    Budget, COMPACTION_RECOVERY_BAND, DEFAULT_KEEP_RECENT_TOKENS, DEFAULT_RESERVE_TOKENS,
};
pub use plan::{CompactionPlan, apply_compaction, plan_compaction};
pub use prune::{PRUNED_TOOL_RESULT, prune_tool_results};
pub use shake::{SHAKE_KEEP_LAST, ShakePlan, plan_shake};
pub use tokens::{estimate_message, estimate_messages, estimate_tokens};
