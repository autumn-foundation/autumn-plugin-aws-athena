//! A scripted fake of Athena for tests.
//!
//! [`FakeAthena`] runs no SQL. Each started query takes the next [`FakeQuery`] script.
//!
//! ```rust
//! use autumn_plugin_aws_athena::testing::{FakeAthena, FakeQuery};
//!
//! let fake = FakeAthena::new();
//! fake.push(
//!     FakeQuery::succeeded()
//!         .columns(&[("id", "bigint"), ("name", "varchar")])
//!         .row(&[Some("1"), Some("Ada")]),
//! );
//! ```

// Each call holds the lock for the full call. This keeps the fake simple.
#![allow(clippy::significant_drop_tightening)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::api::{
    ApiError, AthenaApi, BoxFuture, Column, FailureInfo, Page, QueryState, StartRequest,
    StatementType, Statistics, Status,
};

/// The script for one query.
#[derive(Debug, Clone)]
#[must_use]
pub struct FakeQuery {
    /// The states to report, in order. The last state repeats.
    states: Vec<QueryState>,
    reason: Option<String>,
    failure: Option<FailureInfo>,
    statement_type: StatementType,
    statistics: Statistics,
    columns: Vec<Column>,
    rows: Vec<Vec<Option<String>>>,
    start_error: Option<String>,
    status_error: Option<ApiError>,
    results_error: Option<String>,
    failing_polls: usize,
    start_delay: Duration,
    status_delay: Duration,
    results_delay: Duration,
}

impl FakeQuery {
    fn with_states(states: Vec<QueryState>) -> Self {
        Self {
            states,
            reason: None,
            failure: None,
            statement_type: StatementType::Dml,
            statistics: Statistics::default(),
            columns: Vec::new(),
            rows: Vec::new(),
            start_error: None,
            status_error: None,
            results_error: None,
            failing_polls: 0,
            start_delay: Duration::ZERO,
            status_delay: Duration::ZERO,
            results_delay: Duration::ZERO,
        }
    }

    /// A query that runs for one poll and then succeeds.
    pub fn succeeded() -> Self {
        Self::with_states(vec![QueryState::Running, QueryState::Succeeded])
    }

    /// A query that runs for one poll and then fails with `reason`.
    pub fn failed(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let mut query = Self::with_states(vec![QueryState::Running, QueryState::Failed]);
        query.failure = Some(FailureInfo {
            category: Some(2),
            error_type: None,
            retryable: false,
            message: Some(reason.clone()),
        });
        query.reason = Some(reason);
        query
    }

    /// A query that someone else stops.
    pub fn cancelled() -> Self {
        Self::with_states(vec![QueryState::Queued, QueryState::Cancelled])
    }

    /// A query that runs until a call to `stop`.
    pub fn pending() -> Self {
        Self::with_states(vec![QueryState::Running])
    }

    /// A start call that fails with `message`.
    pub fn start_error(message: impl Into<String>) -> Self {
        let mut query = Self::succeeded();
        query.start_error = Some(message.into());
        query
    }

    /// Makes each status call fail with `message`. A retry can clear the error.
    pub fn status_error(mut self, message: impl Into<String>) -> Self {
        self.status_error = Some(ApiError::new("GetQueryExecution", message));
        self
    }

    /// Makes each status call fail with `message`. A retry does not clear the error.
    pub fn status_error_permanent(mut self, message: impl Into<String>) -> Self {
        self.status_error = Some(ApiError::permanent("GetQueryExecution", message));
        self
    }

    /// Makes the first `polls` status calls fail. A retry clears the error.
    pub const fn failing_polls(mut self, polls: usize) -> Self {
        self.failing_polls = polls;
        self
    }

    /// Makes each start call wait for `delay`. Athena gets the query before the wait.
    pub const fn start_delay(mut self, delay: Duration) -> Self {
        self.start_delay = delay;
        self
    }

    /// Makes each status call wait for `delay`.
    pub const fn status_delay(mut self, delay: Duration) -> Self {
        self.status_delay = delay;
        self
    }

    /// Makes each results call wait for `delay`.
    pub const fn results_delay(mut self, delay: Duration) -> Self {
        self.results_delay = delay;
        self
    }

    /// Makes each results call fail with `message`.
    pub fn results_error(mut self, message: impl Into<String>) -> Self {
        self.results_error = Some(message.into());
        self
    }

    /// Sets the failure details of a failed query.
    pub fn failure(mut self, failure: FailureInfo) -> Self {
        self.failure = Some(failure);
        self
    }

    /// Sets the states to report. The last state repeats.
    pub fn states(mut self, states: impl IntoIterator<Item = QueryState>) -> Self {
        self.states = states.into_iter().collect();
        self
    }

    /// Sets the statement type. The default is DML.
    pub const fn statement_type(mut self, statement_type: StatementType) -> Self {
        self.statement_type = statement_type;
        self
    }

    /// Sets the bytes scanned.
    pub const fn data_scanned(mut self, bytes: u64) -> Self {
        self.statistics.data_scanned_bytes = bytes;
        self
    }

    /// Sets the columns as `(label, type)` pairs.
    pub fn columns(mut self, columns: &[(&str, &str)]) -> Self {
        self.columns = columns
            .iter()
            .map(|(name, t)| Column::new(*name, *t))
            .collect();
        self
    }

    /// Adds a data row. `None` is SQL `NULL`.
    pub fn row(mut self, values: &[Option<&str>]) -> Self {
        self.rows
            .push(values.iter().map(|v| v.map(str::to_owned)).collect());
        self
    }

    /// The raw result rows, with the header row that Athena adds for DML.
    fn raw_rows(&self) -> Vec<Vec<Option<String>>> {
        let header = (self.statement_type == StatementType::Dml && !self.columns.is_empty())
            .then(|| self.columns.iter().map(|c| Some(c.name.clone())).collect());
        header
            .into_iter()
            .chain(self.rows.iter().cloned())
            .collect()
    }
}

#[derive(Debug)]
struct Running {
    script: FakeQuery,
    polls: usize,
    failed_polls: usize,
    stopped: bool,
}

#[derive(Debug, Default)]
struct State {
    scripts: VecDeque<FakeQuery>,
    queries: HashMap<String, Running>,
    started: Vec<StartRequest>,
    stopped: Vec<String>,
    page_requests: Vec<i32>,
    checked_workgroups: Vec<String>,
    workgroup_error: Option<String>,
    tokens: HashMap<String, String>,
    next_id: u64,
}

/// A scripted [`AthenaApi`]. Clones share one state.
#[derive(Debug, Clone, Default)]
pub struct FakeAthena {
    state: Arc<Mutex<State>>,
}

impl FakeAthena {
    /// Makes a fake with no scripts.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds the script for the next query.
    pub fn push(&self, query: FakeQuery) {
        self.lock().scripts.push_back(query);
    }

    /// Makes each workgroup check fail with `message`.
    pub fn fail_workgroup_check(&self, message: impl Into<String>) {
        self.lock().workgroup_error = Some(message.into());
    }

    /// The start requests, in order.
    #[must_use]
    pub fn started(&self) -> Vec<StartRequest> {
        self.lock().started.clone()
    }

    /// The IDs of the stopped queries, in order.
    #[must_use]
    pub fn stopped(&self) -> Vec<String> {
        self.lock().stopped.clone()
    }

    /// The `max_results` value of each results call, in order.
    #[must_use]
    pub fn page_requests(&self) -> Vec<i32> {
        self.lock().page_requests.clone()
    }

    /// The workgroup of each workgroup check, in order.
    #[must_use]
    pub fn checked_workgroups(&self) -> Vec<String> {
        self.lock().checked_workgroups.clone()
    }

    /// The scripted delay of a call for `query_id`.
    fn delay(&self, query_id: &str, pick: fn(&FakeQuery) -> Duration) -> Duration {
        self.lock()
            .queries
            .get(query_id)
            .map_or(Duration::ZERO, |query| pick(&query.script))
    }

    /// Records a start. Gives the query ID and the delay before the response.
    fn begin(&self, request: StartRequest) -> Result<(String, Duration), ApiError> {
        let mut state = self.lock();
        let token = request.client_request_token.clone();
        state.started.push(request);
        // Athena gives the same query for a repeated token.
        if let Some(id) = token.as_ref().and_then(|t| state.tokens.get(t)).cloned() {
            let delay = state
                .queries
                .get(&id)
                .map_or(Duration::ZERO, |q| q.script.start_delay);
            return Ok((id, delay));
        }
        let script = state.scripts.pop_front().ok_or_else(|| {
            ApiError::permanent("StartQueryExecution", "FakeAthena has no script")
        })?;
        if let Some(message) = &script.start_error {
            return Err(ApiError::permanent("StartQueryExecution", message.clone()));
        }
        state.next_id += 1;
        let id = format!("fake-{}", state.next_id);
        let delay = script.start_delay;
        let running = Running {
            script,
            polls: 0,
            failed_polls: 0,
            stopped: false,
        };
        state.queries.insert(id.clone(), running);
        if let Some(token) = token {
            state.tokens.insert(token, id.clone());
        }
        Ok((id, delay))
    }

    fn status_of(&self, query_id: &str) -> Result<Status, ApiError> {
        let mut state = self.lock();
        let query = state.queries.get_mut(query_id).ok_or_else(|| {
            ApiError::permanent("GetQueryExecution", format!("unknown query {query_id}"))
        })?;
        if let Some(err) = &query.script.status_error {
            return Err(err.clone());
        }
        if query.failed_polls < query.script.failing_polls {
            query.failed_polls += 1;
            return Err(ApiError::new("GetQueryExecution", "Rate exceeded"));
        }
        let scripted = query
            .script
            .states
            .get(query.polls)
            .or_else(|| query.script.states.last())
            .cloned()
            .unwrap_or(QueryState::Succeeded);
        query.polls += 1;
        let state = if query.stopped && !scripted.is_terminal() {
            QueryState::Cancelled
        } else {
            scripted
        };
        let mut status = Status::new(state.clone());
        status.statement_type = query.script.statement_type;
        status.statistics = query.script.statistics;
        if state == QueryState::Failed {
            status.state_change_reason.clone_from(&query.script.reason);
            status.failure.clone_from(&query.script.failure);
        }
        Ok(status)
    }

    fn page_of(&self, query_id: &str, token: Option<&str>, max: i32) -> Result<Page, ApiError> {
        let mut state = self.lock();
        state.page_requests.push(max);
        if !(1..=1000).contains(&max) {
            return Err(ApiError::permanent(
                "GetQueryResults",
                "MaxResults must be 1 to 1000",
            ));
        }
        let query = state
            .queries
            .get(query_id)
            .ok_or_else(|| ApiError::new("GetQueryResults", format!("unknown query {query_id}")))?;
        if let Some(message) = &query.script.results_error {
            return Err(ApiError::new("GetQueryResults", message.clone()));
        }
        let rows = query.script.raw_rows();
        let start = token
            .map_or(Ok(0), str::parse::<usize>)
            .map_err(|_| ApiError::new("GetQueryResults", "the next token is not valid"))?;
        let end = start
            .saturating_add(usize::try_from(max).unwrap_or(0))
            .min(rows.len());
        Ok(Page {
            columns: query.script.columns.clone(),
            rows: rows.get(start..end).map(<[_]>::to_vec).unwrap_or_default(),
            next_token: (end < rows.len()).then(|| end.to_string()),
        })
    }
}

impl AthenaApi for FakeAthena {
    fn start(&self, request: StartRequest) -> BoxFuture<'_, Result<String, ApiError>> {
        Box::pin(async move {
            let (id, delay) = self.begin(request)?;
            tokio::time::sleep(delay).await;
            Ok(id)
        })
    }

    fn status<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<Status, ApiError>> {
        Box::pin(async move {
            tokio::time::sleep(self.delay(query_id, |q| q.status_delay)).await;
            self.status_of(query_id)
        })
    }

    fn results<'a>(
        &'a self,
        query_id: &'a str,
        next_token: Option<String>,
        max_results: i32,
    ) -> BoxFuture<'a, Result<Page, ApiError>> {
        Box::pin(async move {
            tokio::time::sleep(self.delay(query_id, |q| q.results_delay)).await;
            self.page_of(query_id, next_token.as_deref(), max_results)
        })
    }

    fn stop<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            let mut state = self.lock();
            let query = state.queries.get_mut(query_id).ok_or_else(|| {
                ApiError::permanent("StopQueryExecution", format!("unknown query {query_id}"))
            })?;
            query.stopped = true;
            state.stopped.push(query_id.to_owned());
            Ok(())
        })
    }

    fn check_workgroup<'a>(&'a self, workgroup: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            let mut state = self.lock();
            state.checked_workgroups.push(workgroup.to_owned());
            state.workgroup_error.clone().map_or(Ok(()), |message| {
                Err(ApiError::new("GetWorkGroup", message))
            })
        })
    }
}
