//! Autumn plugin for Amazon Athena.

pub mod api;
mod backoff;
mod client;
pub mod config;
mod error;
pub mod literal;
mod metrics;
mod placeholder;
mod result;
pub mod sdk;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod value;

pub use client::{Athena, Execution, Query, QueryOutput};
pub use error::AthenaError;
