//! The seam between the plugin and Athena.
//!
//! [`AthenaApi`] has one method for each Athena call that the plugin uses.
//! The plugin uses the AWS SDK in production. Tests use a fake.

use std::future::Future;
use std::pin::Pin;

/// A boxed future that is `Send`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The calls to Athena that the plugin uses.
pub trait AthenaApi: Send + Sync + 'static {
    /// Starts a query and gives its ID (`StartQueryExecution`).
    fn start(&self, request: StartRequest) -> BoxFuture<'_, Result<String, ApiError>>;

    /// Gives the status of a query (`GetQueryExecution`).
    fn status<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<Status, ApiError>>;

    /// Gives one page of results (`GetQueryResults`).
    fn results<'a>(
        &'a self,
        query_id: &'a str,
        next_token: Option<String>,
        max_results: i32,
    ) -> BoxFuture<'a, Result<Page, ApiError>>;

    /// Stops a query (`StopQueryExecution`).
    fn stop<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<(), ApiError>>;

    /// Reads a workgroup to check access (`GetWorkGroup`).
    fn check_workgroup<'a>(&'a self, workgroup: &'a str) -> BoxFuture<'a, Result<(), ApiError>>;
}

/// The input of [`AthenaApi::start`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct StartRequest {
    /// The SQL text.
    pub sql: String,
    /// The encoded execution parameters, in placeholder order.
    pub parameters: Vec<String>,
    /// The workgroup.
    pub workgroup: String,
    /// The data catalog.
    pub catalog: Option<String>,
    /// The database.
    pub database: Option<String>,
    /// The S3 location for the results.
    pub output_location: Option<String>,
    /// The maximum age in minutes of a reused result. `None` disables reuse.
    pub reuse_max_age_minutes: Option<i32>,
}

/// The state of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum QueryState {
    /// Athena accepted the query.
    Queued,
    /// The query runs.
    Running,
    /// The query completed.
    Succeeded,
    /// The query failed.
    Failed,
    /// Someone stopped the query.
    Cancelled,
    /// A state that this version does not know.
    Unknown(String),
}

impl QueryState {
    /// Parses an Athena state name.
    #[must_use]
    pub fn parse(name: &str) -> Self {
        let _ = name;
        todo!()
    }

    /// Returns `true` if the query can not change state again.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        todo!()
    }
}

/// The kind of statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StatementType {
    /// Data definition, for example `CREATE TABLE`.
    Ddl,
    /// Data manipulation, for example `SELECT`.
    Dml,
    /// Utility, for example `SHOW TABLES`.
    Utility,
    /// Athena did not say.
    #[default]
    Unknown,
}

/// The error details of a failed query.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FailureInfo {
    /// `1` is a system error, `2` is a user error, `3` is other.
    pub category: Option<i32>,
    /// The Athena error type code.
    pub error_type: Option<i32>,
    /// Athena thinks that a retry can succeed.
    pub retryable: bool,
    /// The Athena error message.
    pub message: Option<String>,
}

/// The statistics of a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Statistics {
    /// The bytes that the query scanned.
    pub data_scanned_bytes: u64,
    /// The engine time in milliseconds.
    pub engine_execution_ms: u64,
    /// The total time in milliseconds, with queue time.
    pub total_execution_ms: u64,
    /// Athena reused an earlier result.
    pub reused_result: bool,
}

/// The output of [`AthenaApi::status`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Status {
    /// The state.
    pub state: QueryState,
    /// The reason for the last state change.
    pub state_change_reason: Option<String>,
    /// The failure details, for a failed query.
    pub failure: Option<FailureInfo>,
    /// The kind of statement.
    pub statement_type: StatementType,
    /// The statistics.
    pub statistics: Statistics,
}

impl Status {
    /// Makes a status with no details.
    #[must_use]
    pub fn new(state: QueryState) -> Self {
        Self {
            state,
            state_change_reason: None,
            failure: None,
            statement_type: StatementType::Unknown,
            statistics: Statistics::default(),
        }
    }
}

/// A result column.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Column {
    /// The column label.
    pub name: String,
    /// The Athena type name, for example `varchar` or `bigint`.
    pub type_name: String,
}

impl Column {
    /// Makes a column.
    #[must_use]
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self { name: name.into(), type_name: type_name.into() }
    }
}

/// The output of [`AthenaApi::results`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Page {
    /// The columns.
    pub columns: Vec<Column>,
    /// The rows. `None` is SQL `NULL`.
    pub rows: Vec<Vec<Option<String>>>,
    /// The token for the next page.
    pub next_token: Option<String>,
}

/// An error from a call to Athena.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Athena {operation} call failed: {message}")]
#[non_exhaustive]
pub struct ApiError {
    /// The operation, for example `StartQueryExecution`.
    pub operation: &'static str,
    /// The error message.
    pub message: String,
}

impl ApiError {
    /// Makes an error.
    #[must_use]
    pub fn new(operation: &'static str, message: impl Into<String>) -> Self {
        Self { operation, message: message.into() }
    }
}

#[cfg(test)]
mod tests;
