//! Store sesi JSONL v1: header, entry berversi, tulis atomik, resume, dan fork.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod entries;
pub mod store;

pub use entries::{SESSION_VERSION, SessionEntry, new_id};
pub use store::{
    LOCK_STALE_AFTER_MS, LoadedSession, SessionLock, SessionStore, SessionSummary, SimpulPohon,
    acquire_lock, latest, list, pohon, remove, resolve_target, resume_messages, search,
};
