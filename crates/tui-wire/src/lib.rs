//! Tipe bersama untuk seluruh workspace: pesan, event dua tingkat, kontrak tool,
//! dan taksonomi error.
//!
//! Nama tipe dan varian sengaja dibuat identik dengan oh-my-pi (omp) supaya
//! differential testing (`docs/paritas.md`) bisa membandingkan urutan event
//! tanpa tabel penerjemahan tambahan.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod error;
pub mod event;
pub mod message;
pub mod parity;
pub mod providers;
pub mod rpc;
pub mod secure_file;
pub mod tool;

pub use error::{ErrorCode, MxError};
pub use event::{AgentEvent, ProviderEvent, SessionChoice};
pub use message::{AssistantMessage, ContentBlock, Message, StopReason, Usage};
pub use tool::{
    AgentToolMeta, Concurrency, LoadMode, RenderHint, ToolCall, ToolDefinition, ToolOutput,
    ToolTier,
};
