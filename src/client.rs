//! The query client.
//!
//! # Contract
//!
//! - A query checks the placeholder count before it calls Athena.
//! - A query polls with backoff until a terminal state or the deadline.
//! - At the deadline, the query stops in Athena and gives [`AthenaError::Timeout`].
//! - The deadline also bounds each call and the result read.
//! - A dropped query stops in Athena if `cancel_on_drop` is on.
//! - A result with more rows than the limit gives [`AthenaError::TooManyRows`].
//! - Logs have the query ID. Logs never have the SQL text or the parameters.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::time::{Instant, sleep_until, timeout_at};

use crate::api::{
    ApiError, AthenaApi, Column, QueryState, StartRequest, StatementType, Statistics,
};
use crate::config::{AthenaConfig, ConfigError, MAX_REUSE_MINUTES};
use crate::error::AthenaError;
use crate::literal::Param;
use crate::metrics::{Metrics, Outcome};
use crate::placeholder;
use crate::result::{decode_rows, is_header};
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
    open: Mutex<HashSet<String>>,
}

impl Inner {
    fn open(&self) -> MutexGuard<'_, HashSet<String>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl std::fmt::Debug for Athena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Athena")
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
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
        config.validate()?;
        Ok(Self {
            inner: Arc::new(Inner {
                api,
                config,
                metrics,
                open: Mutex::default(),
            }),
        })
    }

    /// The configuration.
    #[must_use]
    pub fn config(&self) -> &AthenaConfig {
        &self.inner.config
    }

    pub(crate) fn metrics(&self) -> &Arc<Metrics> {
        &self.inner.metrics
    }

    pub(crate) fn api(&self) -> &Arc<dyn AthenaApi> {
        &self.inner.api
    }

    /// Starts a query builder for `sql`. Use `?` for each parameter.
    pub fn query(&self, sql: impl Into<String>) -> Query {
        Query {
            athena: self.clone(),
            sql: sql.into(),
            params: Vec::new(),
            database: None,
            catalog: None,
            workgroup: None,
            timeout: None,
            max_rows: None,
            reuse_minutes: None,
        }
    }

    /// The number of queries that did not end yet.
    #[must_use]
    pub fn open_queries(&self) -> usize {
        self.inner.open().len()
    }

    /// Stops each open query in Athena. The waiting callers get [`AthenaError::Cancelled`].
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self.inner.open().iter().cloned().collect();
        let mut tasks = tokio::task::JoinSet::new();
        for id in ids {
            let api = Arc::clone(&self.inner.api);
            tasks.spawn(async move { stop_logged(api.as_ref(), &id).await });
        }
        while tasks.join_next().await.is_some() {}
    }
}

async fn stop_logged(api: &dyn AthenaApi, query_id: &str) {
    match api.stop(query_id).await {
        Ok(()) => tracing::info!(query_id, "stopped the Athena query"),
        Err(err) => tracing::warn!(query_id, error = %err, "can not stop the Athena query"),
    }
}

/// Tracks one started query. A drop before `finish` or `stop` counts as a cancel.
struct OpenQuery {
    inner: Arc<Inner>,
    id: String,
    done: bool,
}

impl OpenQuery {
    fn new(inner: &Arc<Inner>, id: String) -> Self {
        inner.open().insert(id.clone());
        inner.metrics.started();
        Self {
            inner: Arc::clone(inner),
            id,
            done: false,
        }
    }

    fn end(&mut self, outcome: Outcome, scanned_bytes: u64) {
        self.done = true;
        self.inner.open().remove(&self.id);
        self.inner.metrics.ended(outcome, scanned_bytes);
    }

    fn finish(mut self, outcome: Outcome, statistics: &Statistics) {
        self.end(outcome, statistics.data_scanned_bytes);
    }

    async fn stop(mut self, outcome: Outcome) {
        self.end(outcome, 0);
        stop_logged(self.inner.api.as_ref(), &self.id).await;
    }
}

impl Drop for OpenQuery {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.end(Outcome::Cancelled, 0);
        if !self.inner.config.cancel_on_drop {
            return;
        }
        let id = std::mem::take(&mut self.id);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let api = Arc::clone(&self.inner.api);
                runtime.spawn(async move { stop_logged(api.as_ref(), &id).await });
            }
            Err(_) => {
                tracing::warn!(query_id = %id, "no runtime: can not stop the dropped Athena query");
            }
        }
    }
}

/// A query to run. Make one with [`Athena::query`].
#[must_use]
pub struct Query {
    athena: Athena,
    sql: String,
    params: Vec<Param>,
    database: Option<String>,
    catalog: Option<String>,
    workgroup: Option<String>,
    timeout: Option<Duration>,
    max_rows: Option<usize>,
    reuse_minutes: Option<u32>,
}

/// Shows the parameter count only. Parameter values can be personal data.
impl std::fmt::Debug for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Query")
            .field("sql", &self.sql)
            .field("params", &self.params.len())
            .finish_non_exhaustive()
    }
}

impl Query {
    /// Binds the next `?` placeholder to `value`.
    pub fn bind(mut self, value: impl Into<Param>) -> Self {
        self.params.push(value.into());
        self
    }

    /// Uses `database` for this query.
    pub fn database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Uses `catalog` for this query.
    pub fn catalog(mut self, catalog: impl Into<String>) -> Self {
        self.catalog = Some(catalog.into());
        self
    }

    /// Uses `workgroup` for this query.
    pub fn workgroup(mut self, workgroup: impl Into<String>) -> Self {
        self.workgroup = Some(workgroup.into());
        self
    }

    /// Uses `timeout` for this query. The timeout includes the result read.
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Uses `max_rows` as the row limit for this query.
    pub const fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    /// Lets Athena reuse a result that is `max_age_minutes` old or newer. `0` disables reuse.
    pub const fn reuse_results(mut self, max_age_minutes: u32) -> Self {
        self.reuse_minutes = Some(max_age_minutes);
        self
    }

    fn config(&self) -> &AthenaConfig {
        &self.athena.inner.config
    }

    fn timeout_value(&self) -> Duration {
        self.timeout.unwrap_or_else(|| self.config().timeout())
    }

    fn request(&self) -> Result<StartRequest, AthenaError> {
        let placeholders = placeholder::count(&self.sql);
        if placeholders != self.params.len() {
            return Err(AthenaError::ParameterCount {
                placeholders,
                parameters: self.params.len(),
            });
        }
        if self.timeout_value().is_zero() {
            return Err(ConfigError("the query timeout must be more than zero".to_owned()).into());
        }
        let reuse = self
            .reuse_minutes
            .unwrap_or_else(|| self.config().reuse_max_age_minutes);
        if reuse > MAX_REUSE_MINUTES {
            return Err(ConfigError(
                "reuse_results must be 10080 minutes (7 days) or less".to_owned(),
            )
            .into());
        }
        let config = self.config();
        Ok(StartRequest {
            sql: self.sql.clone(),
            parameters: self.params.iter().map(Param::to_sql).collect(),
            workgroup: self
                .workgroup
                .clone()
                .unwrap_or_else(|| config.workgroup.clone()),
            catalog: self.catalog.clone().or_else(|| config.catalog.clone()),
            database: self.database.clone().or_else(|| config.database.clone()),
            output_location: config.output_location.clone(),
            reuse_max_age_minutes: (reuse > 0).then(|| i32::try_from(reuse).unwrap_or(i32::MAX)),
        })
    }

    /// Starts the query and polls until it ends.
    async fn wait(&self, deadline: Instant) -> Result<Execution, AthenaError> {
        let request = self.request()?;
        let inner = &self.athena.inner;
        let id = timeout_at(deadline, inner.api.start(request))
            .await
            .map_err(|_| {
                ApiError::new(
                    "StartQueryExecution",
                    "no response before the query timeout",
                )
            })??;
        tracing::debug!(query_id = %id, "started the Athena query");
        let open = OpenQuery::new(inner, id.clone());
        let backoff = inner.config.backoff();
        let mut attempt = 0_u32;
        loop {
            if Instant::now() >= deadline {
                open.stop(Outcome::TimedOut).await;
                return Err(AthenaError::Timeout {
                    query_id: id,
                    timeout: self.timeout_value(),
                });
            }
            sleep_until((Instant::now() + backoff.delay(attempt)).min(deadline)).await;
            attempt = attempt.saturating_add(1);
            let status = match timeout_at(deadline, inner.api.status(&id)).await {
                Err(_) => continue,
                Ok(Err(err)) => {
                    open.stop(Outcome::Failed).await;
                    return Err(err.into());
                }
                Ok(Ok(status)) => status,
            };
            let outcome = match status.state {
                QueryState::Succeeded => Outcome::Succeeded,
                QueryState::Failed => Outcome::Failed,
                QueryState::Cancelled => Outcome::Cancelled,
                QueryState::Queued | QueryState::Running | QueryState::Unknown(_) => continue,
            };
            open.finish(outcome, &status.statistics);
            tracing::debug!(query_id = %id, state = ?status.state, "the Athena query ended");
            return match outcome {
                Outcome::Succeeded => Ok(Execution {
                    query_id: id,
                    statement_type: status.statement_type,
                    statistics: status.statistics,
                }),
                Outcome::Failed => Err(AthenaError::Failed {
                    query_id: id,
                    reason: status.state_change_reason,
                    failure: status.failure,
                }),
                Outcome::Cancelled | Outcome::TimedOut => {
                    Err(AthenaError::Cancelled { query_id: id })
                }
            };
        }
    }

    /// Reads all result pages of a query that succeeded.
    async fn read(
        &self,
        execution: Execution,
        deadline: Instant,
    ) -> Result<QueryOutput, AthenaError> {
        let api = &self.athena.inner.api;
        let limit = self.max_rows.unwrap_or_else(|| self.config().max_rows);
        let page_size = self.config().page_size;
        let id = execution.query_id.clone();
        let timed_out = || AthenaError::Timeout {
            query_id: id.clone(),
            timeout: self.timeout_value(),
        };
        let mut columns: Option<Arc<[Column]>> = None;
        let mut rows = Vec::new();
        let mut token = None;
        loop {
            let page = timeout_at(deadline, api.results(&id, token.take(), page_size))
                .await
                .map_err(|_| timed_out())??;
            let first_page = columns.is_none();
            let columns = Arc::clone(columns.get_or_insert_with(|| page.columns.into()));
            let mut raw = page.rows;
            if first_page
                && raw
                    .first()
                    .is_some_and(|row| is_header(execution.statement_type, &columns, row))
            {
                raw.remove(0);
            }
            if rows.len().saturating_add(raw.len()) > limit {
                return Err(AthenaError::TooManyRows {
                    query_id: id,
                    limit,
                });
            }
            rows.extend(decode_rows(&columns, raw)?);
            match page.next_token {
                Some(next) if Instant::now() < deadline => token = Some(next),
                Some(_) => return Err(timed_out()),
                None => {
                    return Ok(QueryOutput {
                        execution,
                        columns,
                        rows,
                    });
                }
            }
        }
    }

    fn deadline(&self) -> Instant {
        Instant::now() + self.timeout_value()
    }

    /// Runs the query and waits for it. Reads no rows.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError`] if the query does not succeed in time.
    pub async fn execute(self) -> Result<Execution, AthenaError> {
        self.wait(self.deadline()).await
    }

    /// Runs the query and reads all rows.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError`] if the query does not succeed in time or has too many rows.
    pub async fn fetch(self) -> Result<QueryOutput, AthenaError> {
        let deadline = self.deadline();
        let execution = self.wait(deadline).await?;
        self.read(execution, deadline).await
    }

    /// Runs the query and decodes each row into `T`.
    ///
    /// # Errors
    ///
    /// See [`fetch`](Self::fetch). Also returns [`AthenaError::Decode`] if a row does not fit `T`.
    pub async fn fetch_as<T: DeserializeOwned>(self) -> Result<Vec<T>, AthenaError> {
        let output = self.fetch().await?;
        output
            .rows
            .iter()
            .map(|row| row.deserialize().map_err(AthenaError::from))
            .collect()
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
