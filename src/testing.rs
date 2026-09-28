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
    status_error: Option<String>,
    results_error: Option<String>,
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

    /// Makes each status call fail with `message`.
    pub fn status_error(mut self, message: impl Into<String>) -> Self {
        self.status_error = Some(message.into());
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
    stopped: bool,
}

#[derive(Debug, Default)]
struct State {
    scripts: VecDeque<FakeQuery>,
    queries: HashMap<String, Running>,
    started: Vec<StartRequest>,
    stopped: Vec<String>,
    page_requests: Vec<i32>,
    workgroup_error: Option<String>,
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

    fn status_of(&self, query_id: &str) -> Result<Status, ApiError> {
        let mut state = self.lock();
        let query = state.queries.get_mut(query_id).ok_or_else(|| {
            ApiError::new("GetQueryExecution", format!("unknown query {query_id}"))
        })?;
        if let Some(message) = &query.script.status_error {
            return Err(ApiError::new("GetQueryExecution", message.clone()));
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
            let mut state = self.lock();
            state.started.push(request);
            let script = state
                .scripts
                .pop_front()
                .ok_or_else(|| ApiError::new("StartQueryExecution", "FakeAthena has no script"))?;
            if let Some(message) = &script.start_error {
                return Err(ApiError::new("StartQueryExecution", message.clone()));
            }
            state.next_id += 1;
            let id = format!("fake-{}", state.next_id);
            state.queries.insert(
                id.clone(),
                Running {
                    script,
                    polls: 0,
                    stopped: false,
                },
            );
            Ok(id)
        })
    }

    fn status<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<Status, ApiError>> {
        Box::pin(async move { self.status_of(query_id) })
    }

    fn results<'a>(
        &'a self,
        query_id: &'a str,
        next_token: Option<String>,
        max_results: i32,
    ) -> BoxFuture<'a, Result<Page, ApiError>> {
        Box::pin(async move { self.page_of(query_id, next_token.as_deref(), max_results) })
    }

    fn stop<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            let mut state = self.lock();
            state.stopped.push(query_id.to_owned());
            if let Some(query) = state.queries.get_mut(query_id) {
                query.stopped = true;
            }
            Ok(())
        })
    }

    fn check_workgroup<'a>(&'a self, workgroup: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            let _ = workgroup;
            self.lock()
                .workgroup_error
                .clone()
                .map_or(Ok(()), |message| {
                    Err(ApiError::new("GetWorkGroup", message))
                })
        })
    }
}
