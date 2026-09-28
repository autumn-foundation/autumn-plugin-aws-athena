//! The `[athena]` section of `autumn.toml`.

use std::time::Duration;

use autumn_web::config::Env;
use serde::{Deserialize, Serialize};

use crate::backoff::Backoff;

/// The default section name.
pub const DEFAULT_SECTION: &str = "athena";

/// A configuration that is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub(crate) String);

/// The status poll settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct PollConfig {
    /// The first delay in milliseconds.
    pub initial_ms: u64,
    /// The largest delay in milliseconds.
    pub max_ms: u64,
    /// The growth factor for each poll.
    pub multiplier: f64,
}

impl Default for PollConfig {
    fn default() -> Self {
        todo!()
    }
}

/// The plugin settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[non_exhaustive]
pub struct AthenaConfig {
    /// The AWS region. `None` uses the AWS default chain.
    pub region: Option<String>,
    /// A custom endpoint URL, for example for a local emulator.
    pub endpoint_url: Option<String>,
    /// The workgroup.
    pub workgroup: String,
    /// The data catalog. `None` uses the Athena default.
    pub catalog: Option<String>,
    /// The database. `None` uses the Athena default.
    pub database: Option<String>,
    /// The S3 result location, for example `s3://bucket/prefix/`.
    pub output_location: Option<String>,
    /// The query timeout in milliseconds.
    pub timeout_ms: u64,
    /// The most rows that one query can return.
    pub max_rows: usize,
    /// The rows in each results page, from 1 to 1000.
    pub page_size: i32,
    /// The maximum age in minutes of a reused result. `0` disables reuse.
    pub reuse_max_age_minutes: u32,
    /// Add a readiness check that reads the workgroup.
    pub health_check: bool,
    /// Stop a query in Athena when the caller drops it.
    pub cancel_on_drop: bool,
    /// The status poll settings.
    pub poll: PollConfig,
}

impl Default for AthenaConfig {
    fn default() -> Self {
        todo!()
    }
}

impl AthenaConfig {
    /// Reads `[section]` from the app files and the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a file is not valid TOML or a value is not valid.
    pub fn resolve(section: &str) -> Result<Self, ConfigError> {
        let _ = section;
        todo!()
    }

    /// Reads `[section]` with `env` as the environment.
    ///
    /// # Errors
    ///
    /// See [`resolve`](Self::resolve).
    pub fn resolve_with_env(section: &str, env: &dyn Env) -> Result<Self, ConfigError> {
        let _ = (section, env);
        todo!()
    }

    /// Checks each value.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] that names the first key that is not valid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        todo!()
    }

    /// The query timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    pub(crate) fn backoff(&self) -> Backoff {
        todo!()
    }
}

#[cfg(test)]
mod tests;
