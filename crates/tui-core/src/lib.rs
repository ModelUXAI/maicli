//! Turn loop agent: antrean steering/follow-up, aturan stopReason, gerbang
//! approval, dan eksekusi tool.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod agent;
pub mod commands;
pub mod status;

pub use agent::{
    AfterToolHook, AgentSession, BeforeToolHook, ContextSyncFn, TurnOutcome, TurnSettings,
};
pub use commands::{COMMANDS, CommandContext, CommandResult, CommandSpec, dispatch, help_text};
pub use status::TurnStatus;
