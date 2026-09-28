//! The query client.
//!
//! # Contract
//!
//! - A query checks its settings and the placeholder count before it calls Athena.
//! - A query polls with backoff until a terminal state or the deadline.
//! - At the deadline, the query stops in Athena and gives [`AthenaError::Timeout`].
//! - The deadline bounds each call. The last status call can pass the deadline by one second.
//! - A start that does not complete in time is sent again with the same token, then stopped.
//! - A dropped query stops in Athena if `cancel_on_drop` is on.
//! - Each stop call has a time limit.
//! - A result over the row or byte limit gives an error, not a partial result.
//! - After [`Athena::shutdown`], new queries give [`AthenaError::ShuttingDown`].
//! - Logs have the query ID. Logs never have the SQL text or the parameters.

use std::collections::HashSet;
use std::hash::BuildHasher as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, sleep_until, timeout, timeout_at};

use crate::api::{AthenaApi, Column, QueryState, StartRequest, StatementType, Statistics};
use crate::config::{AthenaConfig, ConfigError, MAX_REUSE_MINUTES};
use crate::error::AthenaError;
use crate::literal::{Param, ParamError};
use crate::metrics::{Metrics, Outcome};
use crate::placeholder;
use crate::result::{decode_rows, is_header};
use crate::value::Row;

/// The time limit of one stop call.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// The time limit to find and stop a query whose start did not complete.
const RECOVER_TIMEOUT: Duration = Duration::from_secs(120);

/// The minimum time that a status call gets, also at the deadline.
const CALL_GRACE: Duration = Duration::from_secs(1);

/// Status errors in a row that stop the query. The SDK retries each call first.
const MAX_STATUS_ERRORS: u32 = 3;

/// The longest execution parameter that Athena accepts.
const MAX_PARAMETER_CHARS: usize = 1024;

/// The deadline for a timeout that is too large for the clock.
const FAR_FUTURE: Duration = Duration::from_secs(30 * 365 * 24 * 60 * 60);

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
    closed: AtomicBool,
    permits: Option<Arc<Semaphore>>,
}

impl Inner {
    fn open(&self) -> MutexGuard<'_, HashSet<String>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
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
        let permits = (config.max_concurrent_queries > 0)
            .then(|| Arc::new(Semaphore::new(config.max_concurrent_queries)));
        let inner = Inner {
            api,
            config,
            metrics,
            open: Mutex::default(),
            closed: AtomicBool::new(false),
            permits,
        };
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// The configuration.
    #[must_use]
    pub fn config(&self) -> &AthenaConfig {
        &self.inner.config
    }

    #[cfg(test)]
    pub(crate) fn metrics(&self) -> &Arc<Metrics> {
        &self.inner.metrics
    }

    pub(crate) fn api(&self) -> &Arc<dyn AthenaApi> {
        &self.inner.api
    }

    /// Starts a query builder for `sql`. Use `?` for each parameter.
    pub fn query(&self, sql: impl Into<String>) -> AthenaQuery {
        AthenaQuery {
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

    /// Stops each open query and refuses new queries.
    pub async fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::Release);
        self.stop_all().await;
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
    match timeout(STOP_TIMEOUT, api.stop(query_id)).await {
        Ok(Ok(())) => tracing::info!(query_id, "stopped the Athena query"),
        Ok(Err(err)) => tracing::warn!(query_id, error = %err, "can not stop the Athena query"),
        Err(_) => tracing::warn!(query_id, "the stop call did not complete in time"),
    }
}

/// Stops `query_id` in a new task, if a runtime is available.
fn spawn_stop(api: &Arc<dyn AthenaApi>, query_id: String) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let api = Arc::clone(api);
        runtime.spawn(async move { stop_logged(api.as_ref(), &query_id).await });
    } else {
        tracing::warn!(%query_id, "no runtime: can not stop the Athena query");
    }
}

/// Sends `request` again with its token to find the query, then stops the query.
fn spawn_recover(api: &Arc<dyn AthenaApi>, request: StartRequest) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("no runtime: can not stop an Athena query with an open start");
        return;
    };
    let api = Arc::clone(api);
    runtime.spawn(async move {
        match timeout(RECOVER_TIMEOUT, api.start(request)).await {
            Ok(Ok(id)) => stop_logged(api.as_ref(), &id).await,
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "can not find the Athena query to stop it");
            }
            Err(_) => tracing::warn!("can not find the Athena query to stop it in time"),
        }
    });
}

/// A new idempotency token: 32 hex characters.
fn request_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let high = std::collections::hash_map::RandomState::new().hash_one(count);
    let low = std::collections::hash_map::RandomState::new().hash_one((count, high));
    format!("{high:016x}{low:016x}")
}

/// A start that did not give a query ID yet.
struct PendingStart {
    inner: Arc<Inner>,
    request: Option<StartRequest>,
}

impl PendingStart {
    fn done(mut self) {
        self.request = None;
    }

    fn recover(mut self) {
        if let Some(request) = self.request.take() {
            spawn_recover(&self.inner.api, request);
        }
    }
}

impl Drop for PendingStart {
    fn drop(&mut self) {
        if let Some(request) = self.request.take()
            && self.inner.config.cancel_on_drop
        {
            spawn_recover(&self.inner.api, request);
        }
    }
}

/// Tracks one started query. A drop before `finish` or `stop` counts as a cancel.
struct OpenQuery {
    inner: Arc<Inner>,
    id: String,
    statistics: Statistics,
    done: bool,
}

impl OpenQuery {
    fn new(inner: &Arc<Inner>, id: String) -> Self {
        inner.open().insert(id.clone());
        inner.metrics.started();
        Self {
            inner: Arc::clone(inner),
            id,
            statistics: Statistics::default(),
            done: false,
        }
    }

    fn end(&mut self, outcome: Outcome) {
        self.done = true;
        self.inner.open().remove(&self.id);
        self.inner
            .metrics
            .ended(outcome, self.statistics.data_scanned_bytes);
    }

    fn finish(mut self, outcome: Outcome) {
        self.end(outcome);
    }

    /// Ends the query and stops it in Athena in a new task.
    fn stop(mut self, outcome: Outcome) {
        self.end(outcome);
        spawn_stop(&self.inner.api, std::mem::take(&mut self.id));
    }
}

impl Drop for OpenQuery {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.end(Outcome::Cancelled);
        let id = std::mem::take(&mut self.id);
        if self.inner.config.cancel_on_drop {
            spawn_stop(&self.inner.api, id);
        } else {
            tracing::warn!(query_id = %id, "the dropped Athena query continues in Athena");
        }
    }
}

/// A query to run. Make one with [`Athena::query`].
#[must_use]
pub struct AthenaQuery {
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

/// Shows the SQL length and the parameter count only. The values can be personal data.
impl std::fmt::Debug for AthenaQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AthenaQuery")
            .field("sql_len", &self.sql.len())
            .field("params", &self.params.len())
            .finish_non_exhaustive()
    }
}

impl AthenaQuery {
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

    /// Uses `timeout` for this query. The timeout includes the wait for a slot and the result read.
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Uses `max_rows` as the row limit for this query.
    pub const fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    /// Lets Athena reuse a result that is not older than `max_age_minutes`. `0` disables reuse.
    ///
    /// A query with parameters never uses reuse. Athena can match a result for other values.
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

    fn timed_out(&self, query_id: Option<String>) -> AthenaError {
        AthenaError::Timeout {
            query_id,
            timeout: self.timeout_value(),
        }
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
        let parameters: Vec<String> = self.params.iter().map(Param::to_sql).collect();
        if parameters
            .iter()
            .any(|p| p.chars().count() > MAX_PARAMETER_CHARS)
        {
            return Err(ParamError::new("encoded", "longer than 1024 characters").into());
        }
        let reuse = if parameters.is_empty() { reuse } else { 0 };
        let config = self.config();
        Ok(StartRequest {
            sql: self.sql.clone(),
            parameters,
            workgroup: self
                .workgroup
                .clone()
                .unwrap_or_else(|| config.workgroup.clone()),
            catalog: self.catalog.clone().or_else(|| config.catalog.clone()),
            database: self.database.clone().or_else(|| config.database.clone()),
            output_location: config.output_location.clone(),
            expected_bucket_owner: config.expected_bucket_owner.clone(),
            reuse_max_age_minutes: (reuse > 0).then(|| i32::try_from(reuse).unwrap_or(i32::MAX)),
            client_request_token: Some(request_token()),
        })
    }

    /// Waits for a slot if the config limits the concurrent queries.
    async fn slot(&self, deadline: Instant) -> Result<Option<OwnedSemaphorePermit>, AthenaError> {
        let Some(permits) = &self.athena.inner.permits else {
            return Ok(None);
        };
        match timeout_at(deadline, Arc::clone(permits).acquire_owned()).await {
            Ok(Ok(permit)) => Ok(Some(permit)),
            // The plugin never closes the semaphore.
            Ok(Err(_)) => Err(AthenaError::ShuttingDown),
            Err(_) => Err(self.timed_out(None)),
        }
    }

    /// Starts the query and gives its ID.
    async fn start(&self, deadline: Instant) -> Result<String, AthenaError> {
        let inner = &self.athena.inner;
        if inner.is_closed() {
            return Err(AthenaError::ShuttingDown);
        }
        let request = self.request()?;
        let pending = PendingStart {
            inner: Arc::clone(inner),
            request: Some(request.clone()),
        };
        match timeout_at(deadline, inner.api.start(request)).await {
            Ok(Ok(id)) => {
                pending.done();
                Ok(id)
            }
            Ok(Err(err)) => {
                pending.done();
                Err(err.into())
            }
            Err(_) => {
                pending.recover();
                Err(self.timed_out(None))
            }
        }
    }

    /// Starts the query and polls until it ends.
    async fn wait(&self, deadline: Instant) -> Result<Execution, AthenaError> {
        let id = self.start(deadline).await?;
        let inner = &self.athena.inner;
        tracing::debug!(query_id = %id, "started the Athena query");
        let mut open = OpenQuery::new(inner, id.clone());
        let backoff = inner.config.backoff();
        let mut attempt = 0_u32;
        let mut errors = 0_u32;
        loop {
            if inner.is_closed() {
                open.stop(Outcome::Cancelled);
                return Err(AthenaError::ShuttingDown);
            }
            if Instant::now() >= deadline {
                open.stop(Outcome::TimedOut);
                return Err(self.timed_out(Some(id)));
            }
            sleep_until((Instant::now() + backoff.delay(attempt)).min(deadline)).await;
            attempt = attempt.saturating_add(1);
            let call_deadline = deadline.max(Instant::now() + CALL_GRACE);
            let status = match timeout_at(call_deadline, inner.api.status(&id)).await {
                Err(_) => continue,
                Ok(Err(err)) => {
                    errors += 1;
                    if err.retryable && errors < MAX_STATUS_ERRORS {
                        tracing::debug!(query_id = %id, error = %err, "the status call failed");
                        continue;
                    }
                    open.stop(Outcome::Failed);
                    return Err(err.into());
                }
                Ok(Ok(status)) => status,
            };
            errors = 0;
            open.statistics = status.statistics;
            let outcome = match status.state {
                QueryState::Succeeded => Outcome::Succeeded,
                QueryState::Failed => Outcome::Failed,
                QueryState::Cancelled => Outcome::Cancelled,
                QueryState::Queued | QueryState::Running | QueryState::Unknown(_) => continue,
            };
            open.finish(outcome);
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
        let row_limit = self.max_rows.unwrap_or_else(|| self.config().max_rows);
        let byte_limit = self.config().max_result_bytes;
        let page_size = self.config().page_size;
        let id = execution.query_id.clone();
        let mut columns: Option<Arc<[Column]>> = None;
        let mut rows = Vec::new();
        let mut bytes = 0_usize;
        let mut token = None;
        loop {
            let page = timeout_at(deadline, api.results(&id, token.take(), page_size))
                .await
                .map_err(|_| self.timed_out(Some(id.clone())))??;
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
            if rows.len().saturating_add(raw.len()) > row_limit {
                return Err(AthenaError::TooManyRows {
                    query_id: id,
                    limit: row_limit,
                });
            }
            bytes = raw
                .iter()
                .flatten()
                .flatten()
                .fold(bytes, |total, value| total.saturating_add(value.len()));
            if bytes > byte_limit {
                return Err(AthenaError::ResultTooLarge {
                    query_id: id,
                    limit_bytes: byte_limit,
                });
            }
            rows.extend(decode_rows(&columns, raw)?);
            match page.next_token {
                Some(next) if Instant::now() < deadline => token = Some(next),
                Some(_) => return Err(self.timed_out(Some(id))),
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
        let now = Instant::now();
        now.checked_add(self.timeout_value())
            .unwrap_or_else(|| now + FAR_FUTURE)
    }

    /// Runs the query and waits for it. Reads no rows.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError`] if the query does not succeed in time.
    pub async fn execute(self) -> Result<Execution, AthenaError> {
        let deadline = self.deadline();
        let _slot = self.slot(deadline).await?;
        self.wait(deadline).await
    }

    /// Runs the query and reads all rows.
    ///
    /// # Errors
    ///
    /// Returns [`AthenaError`] if the query does not succeed in time or has too many rows.
    pub async fn fetch(self) -> Result<QueryOutput, AthenaError> {
        let deadline = self.deadline();
        let _slot = self.slot(deadline).await?;
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
