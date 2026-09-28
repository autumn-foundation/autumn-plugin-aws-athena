//! Autumn plugin for Amazon Athena.

pub mod api;
mod backoff;
mod client;
pub mod config;
mod error;
mod health;
pub mod literal;
mod metrics;
mod placeholder;
mod plugin;
mod result;
pub mod sdk;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod value;

pub use client::{Athena, Execution, Query, QueryOutput};
pub use error::AthenaError;
pub use plugin::AthenaPlugin;
