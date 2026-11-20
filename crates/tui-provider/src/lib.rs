//! Klien gateway OpenAI-compatible: streaming SSE, perakitan tool call,
//! pemetaan finish_reason, dan retry.
//!
//! Modul ini tidak pernah menyentuh kredensial di luar header request; kunci
//! hanya hidup di memori klien dan tidak pernah ditulis ke log atau sesi.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod client;
pub mod error;
pub mod models;
pub mod sse;
pub mod wire;

pub use client::{ChatRequest, ModelInfo, OpenAiCompatClient, Provider, RetryPolicy};
pub use error::GatewayError;
pub use models::{DiscoveryCache, match_model};
