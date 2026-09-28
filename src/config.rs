//! The `[athena]` section of `autumn.toml`.
//!
//! # Contract
//!
//! Each layer overrides the layers before it:
//!
//! 1. The defaults.
//! 2. `[athena]` in `autumn.toml`.
//! 3. `[profile.<name>.athena]` in `autumn.toml`.
//! 4. `[athena]` in `autumn-<name>.toml`.
//! 5. `AUTUMN_ATHENA__<KEY>` variables. `AUTUMN_ATHENA__POLL__MAX_MS` sets `poll.max_ms`.
//!
//! The result must pass [`AthenaConfig::validate`]. Unknown keys are errors.
//!
//! ```toml
//! [athena]
//! region = "eu-west-1"
//! workgroup = "primary"
//! database = "sales"
//! output_location = "s3://my-bucket/athena/"
//! timeout_ms = 300000
//! max_rows = 10000
//! ```

use std::path::{Path, PathBuf};
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
        Self {
            initial_ms: 200,
            max_ms: 2000,
            multiplier: 2.0,
        }
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
    /// The most bytes of values that one query can return.
    pub max_result_bytes: usize,
    /// The most queries that can run at the same time in this process. `0` removes the limit.
    pub max_concurrent_queries: usize,
    /// The AWS account ID that must own the result bucket.
    pub expected_bucket_owner: Option<String>,
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
        Self {
            region: None,
            endpoint_url: None,
            workgroup: "primary".to_owned(),
            catalog: None,
            database: None,
            output_location: None,
            timeout_ms: 300_000,
            max_rows: 10_000,
            max_result_bytes: 64 * 1024 * 1024,
            max_concurrent_queries: 16,
            expected_bucket_owner: None,
            page_size: 1000,
            reuse_max_age_minutes: 0,
            health_check: true,
            cancel_on_drop: true,
            poll: PollConfig::default(),
        }
    }
}

/// The type of a configuration leaf, for environment values.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Integer,
    Float,
    Bool,
}

/// Each leaf key and its type.
const LEAVES: &[(&str, Kind)] = &[
    ("region", Kind::Text),
    ("endpoint_url", Kind::Text),
    ("workgroup", Kind::Text),
    ("catalog", Kind::Text),
    ("database", Kind::Text),
    ("output_location", Kind::Text),
    ("timeout_ms", Kind::Integer),
    ("max_rows", Kind::Integer),
    ("max_result_bytes", Kind::Integer),
    ("max_concurrent_queries", Kind::Integer),
    ("expected_bucket_owner", Kind::Text),
    ("page_size", Kind::Integer),
    ("reuse_max_age_minutes", Kind::Integer),
    ("health_check", Kind::Bool),
    ("cancel_on_drop", Kind::Bool),
    ("poll.initial_ms", Kind::Integer),
    ("poll.max_ms", Kind::Integer),
    ("poll.multiplier", Kind::Float),
];

/// The longest reuse age that Athena accepts: seven days.
pub(crate) const MAX_REUSE_MINUTES: u32 = 10_080;

impl AthenaConfig {
    /// Reads `[section]` from the app files and the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a file is not valid TOML or a value is not valid.
    pub fn resolve(section: &str) -> Result<Self, ConfigError> {
        autumn_web::dotenv::os_env_with_dotenv().map_or_else(
            |_| Self::resolve_with_env(section, &autumn_web::config::OsEnv),
            |env| Self::resolve_with_env(section, &env),
        )
    }

    /// Reads `[section]` with `env` as the environment.
    ///
    /// # Errors
    ///
    /// See [`resolve`](Self::resolve).
    pub fn resolve_with_env(section: &str, env: &dyn Env) -> Result<Self, ConfigError> {
        let (selected, profile) = active_profile(env);
        let mut merged = toml::Table::new();
        if let Some(base) = read_toml(&config_file("autumn.toml", env))? {
            merge_section(&mut merged, base.get(section), section)?;
            for name in inline_profile_names(&profile) {
                let inline = base
                    .get("profile")
                    .and_then(|p| p.get(name))
                    .and_then(|p| p.get(section));
                merge_section(&mut merged, inline, section)?;
            }
        }
        for name in autumn_web::config::profile_override_file_lookup_names(&profile, &selected) {
            if let Some(file) = read_toml(&config_file(&format!("autumn-{name}.toml"), env))? {
                merge_section(&mut merged, file.get(section), section)?;
                break;
            }
        }
        apply_env(&mut merged, section, env)?;
        let config: Self = toml::Value::Table(merged)
            .try_into()
            .map_err(|err| ConfigError(format!("[{section}]: {err}")))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks each value.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] that names the first key that is not valid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let fail = |key: &str, rule: &str| Err(ConfigError(format!("athena.{key} {rule}")));
        let wg = &self.workgroup;
        let wg_chars = wg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if wg.is_empty() || wg.len() > 128 || !wg_chars {
            return fail("workgroup", "must be 1 to 128 of `A-Z a-z 0-9 . _ -`");
        }
        if let Some(region) = &self.region
            && (region.is_empty()
                || !region
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        {
            return fail("region", "must be a region code, for example `eu-west-1`");
        }
        if let Some(url) = &self.endpoint_url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            return fail("endpoint_url", "must start with `http://` or `https://`");
        }
        if let Some(location) = &self.output_location
            && location
                .strip_prefix("s3://")
                .is_none_or(|rest| rest.split('/').next().is_none_or(str::is_empty))
        {
            return fail("output_location", "must be `s3://<bucket>/<prefix>`");
        }
        for (key, value) in [("catalog", &self.catalog), ("database", &self.database)] {
            if value.as_deref().is_some_and(|v| v.trim().is_empty()) {
                return fail(key, "must not be empty");
            }
        }
        if self.timeout_ms == 0 {
            return fail("timeout_ms", "must be 1 or more");
        }
        if self.max_rows == 0 {
            return fail("max_rows", "must be 1 or more");
        }
        if !(1..=1000).contains(&self.page_size) {
            return fail("page_size", "must be from 1 to 1000");
        }
        if self.reuse_max_age_minutes > MAX_REUSE_MINUTES {
            return fail("reuse_max_age_minutes", "must be 10080 (7 days) or less");
        }
        if self.poll.initial_ms == 0 {
            return fail("poll.initial_ms", "must be 1 or more");
        }
        if self.poll.max_ms < self.poll.initial_ms {
            return fail("poll.max_ms", "must be poll.initial_ms or more");
        }
        if !self.poll.multiplier.is_finite() || self.poll.multiplier < 1.0 {
            return fail("poll.multiplier", "must be a finite number, 1.0 or more");
        }
        Ok(())
    }

    /// The query timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    pub(crate) const fn backoff(&self) -> Backoff {
        Backoff::new(
            Duration::from_millis(self.poll.initial_ms),
            Duration::from_millis(self.poll.max_ms),
            self.poll.multiplier,
        )
    }
}

/// Gives the selected profile text and the normalized profile, as Autumn does.
fn active_profile(env: &dyn Env) -> (String, String) {
    let selected = ["AUTUMN_ENV", "AUTUMN_PROFILE"]
        .iter()
        .filter_map(|key| env.var(key).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let release = env.var("AUTUMN_IS_DEBUG").is_ok_and(|v| v == "0");
            if release { "prod" } else { "dev" }.to_owned()
        });
    let profile =
        autumn_web::config::normalize_profile_name(&selected).unwrap_or_else(|| "dev".to_owned());
    (selected, profile)
}

/// The inline profile names to read, in order. The canonical name is last.
fn inline_profile_names(profile: &str) -> Vec<&str> {
    match profile {
        "prod" => vec!["production", "prod"],
        "dev" => vec!["development", "dev"],
        other => vec![other],
    }
}

/// Finds a config file in `AUTUMN_MANIFEST_DIR`, or else in the working directory.
fn config_file(name: &str, env: &dyn Env) -> PathBuf {
    env.var("AUTUMN_MANIFEST_DIR")
        .ok()
        .map(|dir| Path::new(&dir).join(name))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

fn read_toml(path: &Path) -> Result<Option<toml::Table>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map(Some)
            .map_err(|err| ConfigError(format!("{}: {err}", path.display()))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ConfigError(format!("{}: {err}", path.display()))),
    }
}

fn merge_section(
    into: &mut toml::Table,
    layer: Option<&toml::Value>,
    section: &str,
) -> Result<(), ConfigError> {
    match layer {
        None => Ok(()),
        Some(toml::Value::Table(table)) => {
            deep_merge(into, table);
            Ok(())
        }
        Some(_) => Err(ConfigError(format!("[{section}] must be a table"))),
    }
}

fn deep_merge(into: &mut toml::Table, layer: &toml::Table) {
    for (key, value) in layer {
        match (into.get_mut(key), value) {
            (Some(toml::Value::Table(old)), toml::Value::Table(new)) => deep_merge(old, new),
            _ => {
                into.insert(key.clone(), value.clone());
            }
        }
    }
}

fn apply_env(into: &mut toml::Table, section: &str, env: &dyn Env) -> Result<(), ConfigError> {
    let prefix = format!("AUTUMN_{}__", section.to_ascii_uppercase());
    for (path, kind) in LEAVES {
        let name = format!("{prefix}{}", path.replace('.', "__").to_ascii_uppercase());
        let Ok(raw) = env.var(&name) else {
            continue;
        };
        let bad = || ConfigError(format!("{name}: can not read {raw:?}"));
        let value = match kind {
            Kind::Text => toml::Value::String(raw.clone()),
            Kind::Integer => toml::Value::Integer(raw.trim().parse().map_err(|_| bad())?),
            Kind::Float => toml::Value::Float(raw.trim().parse().map_err(|_| bad())?),
            Kind::Bool => match raw.trim() {
                "true" | "1" => toml::Value::Boolean(true),
                "false" | "0" => toml::Value::Boolean(false),
                _ => return Err(bad()),
            },
        };
        let mut table = &mut *into;
        let mut keys = path.split('.').peekable();
        while let Some(key) = keys.next() {
            if keys.peek().is_none() {
                table.insert(key.to_owned(), value);
                break;
            }
            let entry = table
                .entry(key.to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            let Some(inner) = entry.as_table_mut() else {
                return Err(ConfigError(format!("[{section}.{key}] must be a table")));
            };
            table = inner;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
