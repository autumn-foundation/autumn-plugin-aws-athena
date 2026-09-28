//! [`AthenaApi`] on the AWS SDK client.
//!
//! # Contract
//!
//! - Each method makes one SDK call. The SDK retries throttles and transient errors.
//! - The SDK sets `ClientRequestToken`, so a retried start does not start a second query.
//! - An empty optional setting is not sent.
//! - The plugin sends its own `ClientRequestToken`, so it can find a query whose start timed out.
//! - An SDK error gives an [`ApiError`] with the full error context.
//! - A denied or bad request is permanent. A throttle, a server error or a network error is retryable.
//! - A column uses its label, or its name if it has no label.
//! - A disabled workgroup fails the workgroup check.

use aws_sdk_athena::Client;
use aws_sdk_athena::config::{BehaviorVersion, Region};
use aws_sdk_athena::error::{DisplayErrorContext, ProvideErrorMetadata, SdkError};
use aws_sdk_athena::types::{
    QueryExecution, QueryExecutionContext, QueryExecutionState, ResultConfiguration,
    ResultReuseByAgeConfiguration, ResultReuseConfiguration, StatementType as SdkStatementType,
    WorkGroupState,
};

use crate::api::{
    ApiError, AthenaApi, BoxFuture, Column, FailureInfo, Page, QueryState, StartRequest,
    StatementType, Statistics, Status,
};
use crate::config::AthenaConfig;

/// The production [`AthenaApi`]. It uses `aws-sdk-athena`.
#[derive(Debug, Clone)]
pub struct SdkAthena {
    client: Client,
}

impl SdkAthena {
    /// Uses an SDK client that the app made.
    #[must_use]
    pub const fn new(client: Client) -> Self {
        Self { client }
    }

    /// Makes an SDK client from the AWS default chain and `config`.
    ///
    /// `region` and `endpoint_url` in `config` override the AWS defaults.
    pub async fn from_config(config: &AthenaConfig) -> Self {
        let mut loader = aws_config::defaults(BehaviorVersion::latest());
        if let Some(region) = &config.region {
            loader = loader.region(Region::new(region.clone()));
        }
        if let Some(endpoint) = &config.endpoint_url {
            loader = loader.endpoint_url(endpoint);
        }
        Self::new(Client::new(&loader.load().await))
    }

    /// The SDK client.
    #[must_use]
    pub const fn client(&self) -> &Client {
        &self.client
    }
}

/// Service error codes that a retry does not clear.
const PERMANENT_CODES: &[&str] = &[
    "InvalidRequestException",
    "AccessDeniedException",
    "ResourceNotFoundException",
    "MetadataException",
    "UnrecognizedClientException",
];

/// Makes an [`ApiError`] from an SDK error.
fn sdk_error<E, R>(operation: &'static str, err: SdkError<E, R>) -> ApiError
where
    E: ProvideErrorMetadata + std::error::Error + std::fmt::Debug + 'static,
    R: std::fmt::Debug,
{
    let permanent = match &err {
        SdkError::ServiceError(context) => {
            // The error metadata has the code. The variant name is the fallback.
            let variant = format!("{:?}", context.err());
            let code = context.err().code().unwrap_or(&variant);
            PERMANENT_CODES
                .iter()
                .any(|permanent| code.starts_with(permanent))
        }
        SdkError::ConstructionFailure(_) => true,
        _ => false,
    };
    let message = DisplayErrorContext(err).to_string();
    if permanent {
        ApiError::permanent(operation, message)
    } else {
        ApiError::new(operation, message)
    }
}

fn non_negative(value: Option<i64>) -> u64 {
    value.and_then(|v| u64::try_from(v).ok()).unwrap_or(0)
}

fn map_state(state: &QueryExecutionState) -> QueryState {
    match state {
        QueryExecutionState::Queued => QueryState::Queued,
        QueryExecutionState::Running => QueryState::Running,
        QueryExecutionState::Succeeded => QueryState::Succeeded,
        QueryExecutionState::Failed => QueryState::Failed,
        QueryExecutionState::Cancelled => QueryState::Cancelled,
        other => QueryState::Unknown(other.as_str().to_owned()),
    }
}

const fn map_statement(statement: Option<&SdkStatementType>) -> StatementType {
    match statement {
        Some(SdkStatementType::Ddl) => StatementType::Ddl,
        Some(SdkStatementType::Dml) => StatementType::Dml,
        Some(SdkStatementType::Utility) => StatementType::Utility,
        _ => StatementType::Unknown,
    }
}

fn map_execution(execution: &QueryExecution) -> Result<Status, ApiError> {
    let status = execution.status();
    let state = status
        .and_then(|s| s.state())
        .ok_or_else(|| ApiError::new("GetQueryExecution", "the response has no query state"))?;
    let mut mapped = Status::new(map_state(state));
    mapped.state_change_reason = status
        .and_then(|s| s.state_change_reason())
        .map(str::to_owned);
    mapped.failure = status.and_then(|s| s.athena_error()).map(|e| FailureInfo {
        category: e.error_category(),
        error_type: e.error_type(),
        retryable: e.retryable(),
        message: e.error_message().map(str::to_owned),
    });
    mapped.statement_type = map_statement(execution.statement_type());
    if let Some(stats) = execution.statistics() {
        mapped.statistics = Statistics {
            data_scanned_bytes: non_negative(stats.data_scanned_in_bytes()),
            engine_execution_ms: non_negative(stats.engine_execution_time_in_millis()),
            total_execution_ms: non_negative(stats.total_execution_time_in_millis()),
            reused_result: stats
                .result_reuse_information()
                .is_some_and(aws_sdk_athena::types::ResultReuseInformation::reused_previous_result),
        };
    }
    Ok(mapped)
}

impl AthenaApi for SdkAthena {
    fn start(&self, request: StartRequest) -> BoxFuture<'_, Result<String, ApiError>> {
        const OP: &str = "StartQueryExecution";
        Box::pin(async move {
            let mut call = self
                .client
                .start_query_execution()
                .query_string(request.sql)
                .work_group(request.workgroup)
                .set_client_request_token(request.client_request_token);
            if !request.parameters.is_empty() {
                call = call.set_execution_parameters(Some(request.parameters));
            }
            if request.catalog.is_some() || request.database.is_some() {
                call = call.query_execution_context(
                    QueryExecutionContext::builder()
                        .set_catalog(request.catalog)
                        .set_database(request.database)
                        .build(),
                );
            }
            if request.output_location.is_some() || request.expected_bucket_owner.is_some() {
                call = call.result_configuration(
                    ResultConfiguration::builder()
                        .set_output_location(request.output_location)
                        .set_expected_bucket_owner(request.expected_bucket_owner)
                        .build(),
                );
            }
            if let Some(minutes) = request.reuse_max_age_minutes {
                call = call.result_reuse_configuration(
                    ResultReuseConfiguration::builder()
                        .result_reuse_by_age_configuration(
                            ResultReuseByAgeConfiguration::builder()
                                .enabled(true)
                                .max_age_in_minutes(minutes)
                                .build(),
                        )
                        .build(),
                );
            }
            let output = call.send().await.map_err(|err| sdk_error(OP, err))?;
            output
                .query_execution_id()
                .map(str::to_owned)
                .ok_or_else(|| ApiError::new(OP, "the response has no query ID"))
        })
    }

    fn status<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<Status, ApiError>> {
        const OP: &str = "GetQueryExecution";
        Box::pin(async move {
            let output = self
                .client
                .get_query_execution()
                .query_execution_id(query_id)
                .send()
                .await
                .map_err(|err| sdk_error(OP, err))?;
            let execution = output
                .query_execution()
                .ok_or_else(|| ApiError::new(OP, "the response has no query execution"))?;
            map_execution(execution)
        })
    }

    fn results<'a>(
        &'a self,
        query_id: &'a str,
        next_token: Option<String>,
        max_results: i32,
    ) -> BoxFuture<'a, Result<Page, ApiError>> {
        Box::pin(async move {
            let output = self
                .client
                .get_query_results()
                .query_execution_id(query_id)
                .set_next_token(next_token)
                .max_results(max_results)
                .send()
                .await
                .map_err(|err| sdk_error("GetQueryResults", err))?;
            let Some(set) = output.result_set() else {
                return Ok(Page {
                    next_token: output.next_token().map(str::to_owned),
                    ..Page::default()
                });
            };
            let columns = set
                .result_set_metadata()
                .map(aws_sdk_athena::types::ResultSetMetadata::column_info)
                .unwrap_or_default()
                .iter()
                .map(|c| Column::new(c.label().unwrap_or_else(|| c.name()), c.r#type()))
                .collect();
            let rows = set
                .rows()
                .iter()
                .map(|row| {
                    row.data()
                        .iter()
                        .map(|d| d.var_char_value().map(str::to_owned))
                        .collect()
                })
                .collect();
            Ok(Page {
                columns,
                rows,
                next_token: output.next_token().map(str::to_owned),
            })
        })
    }

    fn stop<'a>(&'a self, query_id: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            self.client
                .stop_query_execution()
                .query_execution_id(query_id)
                .send()
                .await
                .map(|_| ())
                .map_err(|err| sdk_error("StopQueryExecution", err))
        })
    }

    fn check_workgroup<'a>(&'a self, workgroup: &'a str) -> BoxFuture<'a, Result<(), ApiError>> {
        Box::pin(async move {
            const OP: &str = "GetWorkGroup";
            let output = self
                .client
                .get_work_group()
                .work_group(workgroup)
                .send()
                .await
                .map_err(|err| sdk_error(OP, err))?;
            let state = output.work_group().and_then(|w| w.state());
            if state == Some(&WorkGroupState::Disabled) {
                return Err(ApiError::permanent(OP, "the workgroup is disabled"));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
