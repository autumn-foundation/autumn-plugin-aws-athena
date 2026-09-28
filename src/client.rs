//! The query client.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::api::{AthenaApi, Column, StatementType, Statistics};
use crate::config::AthenaConfig;
use crate::error::AthenaError;
use crate::literal::Param;
use crate::metrics::Metrics;
use crate::value::Row;

/// A handle to Athena. Clones share one connection and one set of open queries.
#[derive(Clone)]
pub struct Athena {
    inner: Arc<Inner>,
}

struct Inner {
    api: Arc<dyn AthenaApi>,
    config: AthenaConfig,
    metrics: Arc<Metrics>,
}

impl std::fmt::Debug for Athena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Athena").field("config", &self.inner.config).finish_non_exhaustive()
    }
}

impl Athena {
    /// Makes a handle on `api` with `config`.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError::Config`] if `config` is not valid.
    pub fn new(api: impl AthenaApi, config: AthenaConfig) -> Result<Self, AthenaError> {
        Self::with_parts(Arc::new(api), config, Arc::default())
    }

    pub(crate) fn with_parts(
        api: Arc<dyn AthenaApi>,
        config: AthenaConfig,
        metrics: Arc<Metrics>,
    ) -> Result<Self, AthenaError> {
        let _ = (api, config, metrics);
        todo!()
    }

    /// The configuration.
    #[must_use]
    pub fn config(&self) -> &AthenaConfig {
        &self.inner.config
    }

    pub(crate) fn metrics(&self) -> &Arc<Metrics> {
        &self.inner.metrics
    }

    /// Starts a query builder for `sql`. Use `?` for each parameter.
    #[must_use]
    pub fn query(&self, sql: impl Into<String>) -> Query {
        let _ = sql;
        todo!()
    }

    /// The number of queries that did not end yet.
    #[must_use]
    pub fn open_queries(&self) -> usize {
        todo!()
    }

    /// Stops each open query in Athena.
    pub async fn stop_all(&self) {
        todo!()
    }
}

/// A query to run. Make one with [`Athena::query`].
#[derive(Debug)]
#[must_use]
pub struct Query {
    athena: Athena,
}

impl Query {
    /// Binds the next `?` placeholder to `value`.
    pub fn bind(self, value: impl Into<Param>) -> Self {
        let _ = value.into();
        todo!()
    }

    /// Uses `database` for this query.
    pub fn database(self, database: impl Into<String>) -> Self {
        let _ = database.into();
        todo!()
    }

    /// Uses `catalog` for this query.
    pub fn catalog(self, catalog: impl Into<String>) -> Self {
        let _ = catalog.into();
        todo!()
    }

    /// Uses `workgroup` for this query.
    pub fn workgroup(self, workgroup: impl Into<String>) -> Self {
        let _ = workgroup.into();
        todo!()
    }

    /// Uses `timeout` for this query.
    pub fn timeout(self, timeout: Duration) -> Self {
        let _ = timeout;
        todo!()
    }

    /// Uses `max_rows` as the row limit for this query.
    pub fn max_rows(self, max_rows: usize) -> Self {
        let _ = max_rows;
        todo!()
    }

    /// Lets Athena reuse a result that is `max_age_minutes` old or newer. `0` disables reuse.
    pub fn reuse_results(self, max_age_minutes: u32) -> Self {
        let _ = max_age_minutes;
        todo!()
    }

    /// Runs the query and waits for it. Reads no rows.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError`] if the query does not succeed in time.
    pub async fn execute(self) -> Result<Execution, AthenaError> {
        todo!()
    }

    /// Runs the query and reads all rows.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError`] if the query does not succeed in time or has too many rows.
    pub async fn fetch(self) -> Result<QueryOutput, AthenaError> {
        todo!()
    }

    /// Runs the query and decodes each row into `T`.
    ///
    /// # Errors
    ///
    /// See [`fetch`](Self::fetch). Also returns [`AthenaError::Decode`] if a row does not fit `T`.
    pub async fn fetch_as<T: DeserializeOwned>(self) -> Result<Vec<T>, AthenaError> {
        todo!()
    }
}

/// A query that succeeded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Execution {
    /// The query ID.
    pub query_id: String,
    /// The kind of statement.
    pub statement_type: StatementType,
    /// The statistics.
    pub statistics: Statistics,
}

/// The rows of a query that succeeded.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct QueryOutput {
    /// The query that made the rows.
    pub execution: Execution,
    /// The columns.
    pub columns: Arc<[Column]>,
    /// The rows, without the header row.
    pub rows: Vec<Row>,
}

#[cfg(test)]
mod tests;
