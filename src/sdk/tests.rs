use aws_sdk_athena::Client;
use aws_sdk_athena::operation::get_query_execution::GetQueryExecutionOutput;
use aws_sdk_athena::operation::get_query_results::GetQueryResultsOutput;
use aws_sdk_athena::operation::get_work_group::{GetWorkGroupError, GetWorkGroupOutput};
use aws_sdk_athena::operation::start_query_execution::{
    StartQueryExecutionError, StartQueryExecutionOutput,
};
use aws_sdk_athena::operation::stop_query_execution::StopQueryExecutionOutput;
use aws_sdk_athena::types::error::{InvalidRequestException, TooManyRequestsException};
use aws_sdk_athena::types::{
    AthenaError as SdkFailure, ColumnInfo, Datum, QueryExecution, QueryExecutionState,
    QueryExecutionStatistics, QueryExecutionStatus, ResultReuseInformation, ResultSet,
    ResultSetMetadata, Row as SdkRow, StatementType as SdkStatementType, WorkGroup, WorkGroupState,
};
use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};

use super::*;
use crate::api::{Column, QueryState, StatementType};

fn athena(rules: &[&Rule]) -> SdkAthena {
    SdkAthena::new(mock_client!(aws_sdk_athena, RuleMode::MatchAny, rules))
}

#[tokio::test]
async fn start_sends_each_setting() {
    let rule = mock!(Client::start_query_execution)
        .match_requests(|input| {
            let context = input.query_execution_context().unwrap();
            let reuse = input
                .result_reuse_configuration()
                .and_then(|r| r.result_reuse_by_age_configuration())
                .unwrap();
            input.query_string() == Some("SELECT ?")
                && input.execution_parameters() == ["'a'".to_owned()]
                && input.work_group() == Some("reports")
                && context.database() == Some("sales")
                && context.catalog() == Some("lake")
                && input
                    .result_configuration()
                    .and_then(|r| r.output_location())
                    == Some("s3://out/")
                && input
                    .result_configuration()
                    .and_then(|r| r.expected_bucket_owner())
                    == Some("123456789012")
                && reuse.enabled()
                && reuse.max_age_in_minutes() == Some(30)
                && input.client_request_token() == Some("0123456789abcdef0123456789abcdef")
        })
        .then_output(|| {
            StartQueryExecutionOutput::builder()
                .query_execution_id("q-1")
                .build()
        });
    let request = StartRequest {
        sql: "SELECT ?".into(),
        parameters: vec!["'a'".into()],
        workgroup: "reports".into(),
        catalog: Some("lake".into()),
        database: Some("sales".into()),
        output_location: Some("s3://out/".into()),
        expected_bucket_owner: Some("123456789012".into()),
        reuse_max_age_minutes: Some(30),
        client_request_token: Some("0123456789abcdef0123456789abcdef".into()),
    };
    assert_eq!(athena(&[&rule]).start(request).await.unwrap(), "q-1");
    assert_eq!(rule.num_calls(), 1);
}

#[tokio::test]
async fn start_leaves_out_empty_settings() {
    let rule = mock!(Client::start_query_execution)
        .match_requests(|input| {
            input.execution_parameters().is_empty()
                && input.query_execution_context().is_none()
                && input.result_configuration().is_none()
                && input.result_reuse_configuration().is_none()
        })
        .then_output(|| {
            StartQueryExecutionOutput::builder()
                .query_execution_id("q-2")
                .build()
        });
    let request = StartRequest {
        sql: "SELECT 1".into(),
        workgroup: "primary".into(),
        ..StartRequest::default()
    };
    assert_eq!(athena(&[&rule]).start(request).await.unwrap(), "q-2");
}

#[tokio::test]
async fn start_without_an_id_is_an_error() {
    let rule = mock!(Client::start_query_execution)
        .then_output(|| StartQueryExecutionOutput::builder().build());
    let err = athena(&[&rule])
        .start(StartRequest::default())
        .await
        .unwrap_err();
    assert_eq!(err.operation, "StartQueryExecution");
}

#[tokio::test]
async fn start_errors_keep_the_service_message() {
    let rule = mock!(Client::start_query_execution).then_error(|| {
        StartQueryExecutionError::InvalidRequestException(
            InvalidRequestException::builder()
                .message("line 1: bad SQL")
                .build(),
        )
    });
    let err = athena(&[&rule])
        .start(StartRequest::default())
        .await
        .unwrap_err();
    assert_eq!(err.operation, "StartQueryExecution");
    assert!(err.message.contains("line 1: bad SQL"), "{}", err.message);
}

#[tokio::test]
async fn status_maps_the_execution() {
    let rule = mock!(Client::get_query_execution)
        .match_requests(|input| input.query_execution_id() == Some("q-1"))
        .then_output(|| {
            GetQueryExecutionOutput::builder()
                .query_execution(
                    QueryExecution::builder()
                        .statement_type(SdkStatementType::Dml)
                        .status(
                            QueryExecutionStatus::builder()
                                .state(QueryExecutionState::Failed)
                                .state_change_reason("SYNTAX_ERROR")
                                .athena_error(
                                    SdkFailure::builder()
                                        .error_category(2)
                                        .error_type(1000)
                                        .retryable(false)
                                        .error_message("bad")
                                        .build(),
                                )
                                .build(),
                        )
                        .statistics(
                            QueryExecutionStatistics::builder()
                                .data_scanned_in_bytes(2048)
                                .engine_execution_time_in_millis(15)
                                .total_execution_time_in_millis(40)
                                .result_reuse_information(
                                    ResultReuseInformation::builder()
                                        .reused_previous_result(true)
                                        .build(),
                                )
                                .build(),
                        )
                        .build(),
                )
                .build()
        });
    let status = athena(&[&rule]).status("q-1").await.unwrap();
    assert_eq!(status.state, QueryState::Failed);
    assert_eq!(status.state_change_reason.as_deref(), Some("SYNTAX_ERROR"));
    assert_eq!(status.statement_type, StatementType::Dml);
    let failure = status.failure.unwrap();
    assert_eq!(failure.category, Some(2));
    assert_eq!(failure.error_type, Some(1000));
    assert!(!failure.retryable);
    assert_eq!(failure.message.as_deref(), Some("bad"));
    assert_eq!(status.statistics.data_scanned_bytes, 2048);
    assert_eq!(status.statistics.engine_execution_ms, 15);
    assert_eq!(status.statistics.total_execution_ms, 40);
    assert!(status.statistics.reused_result);
}

#[tokio::test]
async fn status_maps_each_state_and_statement_type() {
    for (state, expected) in [
        (QueryExecutionState::Queued, QueryState::Queued),
        (QueryExecutionState::Running, QueryState::Running),
        (QueryExecutionState::Succeeded, QueryState::Succeeded),
        (QueryExecutionState::Cancelled, QueryState::Cancelled),
    ] {
        let rule = mock!(Client::get_query_execution).then_output(move || {
            GetQueryExecutionOutput::builder()
                .query_execution(
                    QueryExecution::builder()
                        .statement_type(SdkStatementType::Utility)
                        .status(QueryExecutionStatus::builder().state(state.clone()).build())
                        .build(),
                )
                .build()
        });
        let status = athena(&[&rule]).status("q").await.unwrap();
        assert_eq!(status.state, expected);
        assert_eq!(status.statement_type, StatementType::Utility);
        assert_eq!(status.failure, None);
    }
}

#[tokio::test]
async fn status_without_an_execution_is_an_error() {
    let rule = mock!(Client::get_query_execution)
        .then_output(|| GetQueryExecutionOutput::builder().build());
    let err = athena(&[&rule]).status("q").await.unwrap_err();
    assert_eq!(err.operation, "GetQueryExecution");
}

#[tokio::test]
async fn status_without_a_state_is_an_error() {
    let rule = mock!(Client::get_query_execution).then_output(|| {
        GetQueryExecutionOutput::builder()
            .query_execution(QueryExecution::builder().build())
            .build()
    });
    let err = athena(&[&rule]).status("q").await.unwrap_err();
    assert!(err.message.contains("no query state"), "{}", err.message);
}

#[tokio::test]
async fn status_maps_unknown_values_and_missing_statistics() {
    let rule = mock!(Client::get_query_execution).then_output(|| {
        GetQueryExecutionOutput::builder()
            .query_execution(
                QueryExecution::builder()
                    .status(
                        QueryExecutionStatus::builder()
                            .state(QueryExecutionState::from("PAUSED"))
                            .build(),
                    )
                    .statistics(
                        QueryExecutionStatistics::builder()
                            .data_scanned_in_bytes(-5)
                            .build(),
                    )
                    .build(),
            )
            .build()
    });
    let status = athena(&[&rule]).status("q").await.unwrap();
    assert_eq!(status.state, QueryState::Unknown("PAUSED".into()));
    assert_eq!(status.statement_type, StatementType::Unknown);
    assert_eq!(status.statistics.data_scanned_bytes, 0);
    assert_eq!(status.statistics.total_execution_ms, 0);
}

#[tokio::test]
async fn status_maps_ddl() {
    let rule = mock!(Client::get_query_execution).then_output(|| {
        GetQueryExecutionOutput::builder()
            .query_execution(
                QueryExecution::builder()
                    .statement_type(SdkStatementType::Ddl)
                    .status(
                        QueryExecutionStatus::builder()
                            .state(QueryExecutionState::Succeeded)
                            .build(),
                    )
                    .build(),
            )
            .build()
    });
    let status = athena(&[&rule]).status("q").await.unwrap();
    assert_eq!(status.statement_type, StatementType::Ddl);
}

#[tokio::test]
async fn invalid_requests_are_permanent_and_throttles_are_retryable() {
    let bad = mock!(Client::start_query_execution).then_error(|| {
        StartQueryExecutionError::InvalidRequestException(
            InvalidRequestException::builder().message("bad").build(),
        )
    });
    let err = athena(&[&bad])
        .start(StartRequest::default())
        .await
        .unwrap_err();
    assert!(!err.retryable);
    let busy = mock!(Client::start_query_execution).then_error(|| {
        StartQueryExecutionError::TooManyRequestsException(
            TooManyRequestsException::builder()
                .message("slow down")
                .build(),
        )
    });
    let err = athena(&[&busy])
        .start(StartRequest::default())
        .await
        .unwrap_err();
    assert!(err.retryable);
}

#[tokio::test]
async fn columns_use_the_label_when_it_is_set() {
    let rule = mock!(Client::get_query_results).then_output(|| {
        GetQueryResultsOutput::builder()
            .result_set(
                ResultSet::builder()
                    .result_set_metadata(
                        ResultSetMetadata::builder()
                            .column_info(
                                ColumnInfo::builder()
                                    .name("_col0")
                                    .label("total")
                                    .r#type("bigint")
                                    .build()
                                    .unwrap(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .build()
    });
    let page = athena(&[&rule]).results("q", None, 10).await.unwrap();
    assert_eq!(page.columns, vec![Column::new("total", "bigint")]);
}

#[tokio::test]
async fn results_keep_the_token_without_a_result_set() {
    let rule = mock!(Client::get_query_results)
        .then_output(|| GetQueryResultsOutput::builder().next_token("t-9").build());
    let page = athena(&[&rule]).results("q", None, 10).await.unwrap();
    assert_eq!(page.next_token.as_deref(), Some("t-9"));
    assert!(page.rows.is_empty());
}

#[tokio::test]
async fn a_disabled_workgroup_fails_the_check() {
    let rule = mock!(Client::get_work_group).then_output(|| {
        GetWorkGroupOutput::builder()
            .work_group(
                WorkGroup::builder()
                    .name("primary")
                    .state(WorkGroupState::Disabled)
                    .build()
                    .unwrap(),
            )
            .build()
    });
    let err = athena(&[&rule])
        .check_workgroup("primary")
        .await
        .unwrap_err();
    assert!(err.message.contains("disabled"), "{}", err.message);
}

#[tokio::test]
async fn each_call_names_its_operation_in_errors() {
    use aws_sdk_athena::operation::get_query_execution::GetQueryExecutionError;
    use aws_sdk_athena::operation::get_query_results::GetQueryResultsError;
    use aws_sdk_athena::operation::stop_query_execution::StopQueryExecutionError;
    let invalid = || InvalidRequestException::builder().message("x").build();
    let status = mock!(Client::get_query_execution)
        .then_error(move || GetQueryExecutionError::InvalidRequestException(invalid()));
    let results = mock!(Client::get_query_results)
        .then_error(move || GetQueryResultsError::InvalidRequestException(invalid()));
    let stop = mock!(Client::stop_query_execution)
        .then_error(move || StopQueryExecutionError::InvalidRequestException(invalid()));
    let athena = athena(&[&status, &results, &stop]);
    assert_eq!(
        athena.status("q").await.unwrap_err().operation,
        "GetQueryExecution"
    );
    assert_eq!(
        athena.results("q", None, 1).await.unwrap_err().operation,
        "GetQueryResults"
    );
    assert_eq!(
        athena.stop("q").await.unwrap_err().operation,
        "StopQueryExecution"
    );
}

fn datum(value: Option<&str>) -> Datum {
    Datum::builder()
        .set_var_char_value(value.map(str::to_owned))
        .build()
}

#[tokio::test]
async fn results_map_columns_rows_and_nulls() {
    let rule = mock!(Client::get_query_results)
        .match_requests(|input| {
            input.query_execution_id() == Some("q-1")
                && input.next_token() == Some("t-1")
                && input.max_results() == Some(500)
        })
        .then_output(|| {
            GetQueryResultsOutput::builder()
                .result_set(
                    ResultSet::builder()
                        .result_set_metadata(
                            ResultSetMetadata::builder()
                                .column_info(
                                    ColumnInfo::builder()
                                        .name("id")
                                        .r#type("bigint")
                                        .build()
                                        .unwrap(),
                                )
                                .column_info(
                                    ColumnInfo::builder()
                                        .name("note")
                                        .r#type("varchar")
                                        .build()
                                        .unwrap(),
                                )
                                .build(),
                        )
                        .rows(
                            SdkRow::builder()
                                .data(datum(Some("1")))
                                .data(datum(None))
                                .build(),
                        )
                        .rows(
                            SdkRow::builder()
                                .data(datum(Some("2")))
                                .data(datum(Some("")))
                                .build(),
                        )
                        .build(),
                )
                .next_token("t-2")
                .build()
        });
    let page = athena(&[&rule])
        .results("q-1", Some("t-1".into()), 500)
        .await
        .unwrap();
    assert_eq!(
        page.columns,
        vec![Column::new("id", "bigint"), Column::new("note", "varchar")]
    );
    assert_eq!(
        page.rows,
        vec![
            vec![Some("1".to_owned()), None],
            vec![Some("2".to_owned()), Some(String::new())]
        ]
    );
    assert_eq!(page.next_token.as_deref(), Some("t-2"));
}

#[tokio::test]
async fn results_without_a_result_set_are_empty() {
    let rule =
        mock!(Client::get_query_results).then_output(|| GetQueryResultsOutput::builder().build());
    let page = athena(&[&rule]).results("q", None, 10).await.unwrap();
    assert_eq!(page, Page::default());
}

#[tokio::test]
async fn stop_sends_the_query_id() {
    let rule = mock!(Client::stop_query_execution)
        .match_requests(|input| input.query_execution_id() == Some("q-9"))
        .then_output(|| StopQueryExecutionOutput::builder().build());
    athena(&[&rule]).stop("q-9").await.unwrap();
    assert_eq!(rule.num_calls(), 1);
}

#[tokio::test]
async fn check_workgroup_reads_the_workgroup() {
    let ok = mock!(Client::get_work_group)
        .match_requests(|input| input.work_group() == Some("primary"))
        .then_output(|| GetWorkGroupOutput::builder().build());
    athena(&[&ok]).check_workgroup("primary").await.unwrap();
    let fail = mock!(Client::get_work_group).then_error(|| {
        GetWorkGroupError::InvalidRequestException(
            InvalidRequestException::builder()
                .message("no such workgroup")
                .build(),
        )
    });
    let err = athena(&[&fail])
        .check_workgroup("missing")
        .await
        .unwrap_err();
    assert_eq!(err.operation, "GetWorkGroup");
    assert!(err.message.contains("no such workgroup"), "{}", err.message);
}

#[tokio::test]
async fn from_config_uses_the_region_and_the_endpoint() {
    let config = AthenaConfig {
        region: Some("eu-west-1".into()),
        endpoint_url: Some("http://localhost:4566".into()),
        ..AthenaConfig::default()
    };
    let athena = SdkAthena::from_config(&config).await;
    let sdk_config = athena.client().config();
    assert_eq!(
        sdk_config.region().map(ToString::to_string).as_deref(),
        Some("eu-west-1")
    );
    let debug = format!("{sdk_config:?}");
    assert!(debug.contains("localhost:4566"), "{debug}");
}
