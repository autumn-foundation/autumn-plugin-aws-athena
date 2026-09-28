use std::time::Duration;

use serde::Deserialize;

use super::*;
use crate::api::QueryState;
use crate::testing::{FakeAthena, FakeQuery};
use crate::value::Value;

fn config() -> AthenaConfig {
    AthenaConfig {
        database: Some("sales".into()),
        output_location: Some("s3://results/".into()),
        ..AthenaConfig::default()
    }
}

fn athena(fake: &FakeAthena) -> Athena {
    Athena::new(fake.clone(), config()).unwrap()
}

fn orders() -> FakeQuery {
    FakeQuery::succeeded()
        .columns(&[("id", "bigint"), ("customer", "varchar")])
        .row(&[Some("1"), Some("ada")])
        .row(&[Some("2"), None])
        .data_scanned(1024)
}

#[tokio::test(start_paused = true)]
async fn fetch_returns_the_rows_without_the_header() {
    let fake = FakeAthena::new();
    fake.push(orders());
    let output = athena(&fake)
        .query("SELECT id, customer FROM orders")
        .fetch()
        .await
        .unwrap();
    assert_eq!(output.columns.len(), 2);
    assert_eq!(output.rows.len(), 2);
    assert_eq!(
        output.rows[0].values(),
        &[Value::Int(1), Value::Text("ada".into())]
    );
    assert_eq!(output.rows[1].values(), &[Value::Int(2), Value::Null]);
    assert_eq!(output.execution.query_id, "fake-1");
    assert_eq!(output.execution.statistics.data_scanned_bytes, 1024);
}

#[tokio::test(start_paused = true)]
async fn fetch_as_decodes_each_row() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Order {
        id: i64,
        customer: Option<String>,
    }
    let fake = FakeAthena::new();
    fake.push(orders());
    let rows: Vec<Order> = athena(&fake).query("SELECT 1").fetch_as().await.unwrap();
    assert_eq!(
        rows,
        vec![
            Order {
                id: 1,
                customer: Some("ada".into())
            },
            Order {
                id: 2,
                customer: None
            }
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn the_start_request_has_the_config_and_the_parameters() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::succeeded());
    athena(&fake)
        .query("SELECT * FROM t WHERE a = ? AND b = ?")
        .bind("it's")
        .bind(7)
        .execute()
        .await
        .unwrap();
    let request = &fake.started()[0];
    assert_eq!(request.sql, "SELECT * FROM t WHERE a = ? AND b = ?");
    assert_eq!(
        request.parameters,
        vec!["'it''s'".to_owned(), "7".to_owned()]
    );
    assert_eq!(request.workgroup, "primary");
    assert_eq!(request.database.as_deref(), Some("sales"));
    assert_eq!(request.catalog, None);
    assert_eq!(request.output_location.as_deref(), Some("s3://results/"));
    assert_eq!(request.reuse_max_age_minutes, None);
}

#[tokio::test(start_paused = true)]
async fn query_settings_override_the_config() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::succeeded());
    athena(&fake)
        .query("SELECT 1")
        .database("other")
        .catalog("lake")
        .workgroup("reports")
        .reuse_results(60)
        .execute()
        .await
        .unwrap();
    let request = &fake.started()[0];
    assert_eq!(request.database.as_deref(), Some("other"));
    assert_eq!(request.catalog.as_deref(), Some("lake"));
    assert_eq!(request.workgroup, "reports");
    assert_eq!(request.reuse_max_age_minutes, Some(60));
}

#[tokio::test(start_paused = true)]
async fn config_reuse_applies_to_each_query() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::succeeded());
    fake.push(FakeQuery::succeeded());
    let mut config = config();
    config.reuse_max_age_minutes = 15;
    let athena = Athena::new(fake.clone(), config).unwrap();
    athena.query("SELECT 1").execute().await.unwrap();
    athena
        .query("SELECT 1")
        .reuse_results(0)
        .execute()
        .await
        .unwrap();
    assert_eq!(fake.started()[0].reuse_max_age_minutes, Some(15));
    assert_eq!(fake.started()[1].reuse_max_age_minutes, None);
}

#[tokio::test(start_paused = true)]
async fn a_parameter_count_mismatch_fails_before_the_start() {
    let fake = FakeAthena::new();
    let err = athena(&fake).query("SELECT ?").execute().await.unwrap_err();
    assert_eq!(
        err,
        AthenaError::ParameterCount {
            placeholders: 1,
            parameters: 0
        }
    );
    assert!(fake.started().is_empty());
}

#[tokio::test(start_paused = true)]
async fn reads_all_pages_and_skips_only_the_first_header() {
    let fake = FakeAthena::new();
    let mut query = FakeQuery::succeeded().columns(&[("n", "integer")]);
    for n in 0..25 {
        query = query.row(&[Some(&n.to_string())]);
    }
    fake.push(query);
    let mut config = config();
    config.page_size = 10;
    let athena = Athena::new(fake.clone(), config).unwrap();
    let output = athena.query("SELECT n").fetch().await.unwrap();
    let values: Vec<_> = output.rows.iter().map(|r| r.values()[0].clone()).collect();
    assert_eq!(values, (0..25).map(Value::Int).collect::<Vec<_>>());
    // The header row and 25 data rows make three pages of 10.
    assert_eq!(fake.page_requests(), vec![10, 10, 10]);
}

#[tokio::test(start_paused = true)]
async fn a_utility_result_has_no_header_to_skip() {
    let fake = FakeAthena::new();
    fake.push(
        FakeQuery::succeeded()
            .statement_type(StatementType::Utility)
            .columns(&[("tab_name", "string")])
            .row(&[Some("orders")]),
    );
    let output = athena(&fake).query("SHOW TABLES").fetch().await.unwrap();
    assert_eq!(output.rows.len(), 1);
    assert_eq!(
        output.rows[0].get("tab_name"),
        Some(&Value::Text("orders".into()))
    );
}

#[tokio::test(start_paused = true)]
async fn too_many_rows_fails() {
    let fake = FakeAthena::new();
    fake.push(orders());
    let err = athena(&fake)
        .query("SELECT 1")
        .max_rows(1)
        .fetch()
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AthenaError::TooManyRows {
            query_id: "fake-1".into(),
            limit: 1
        }
    );
}

#[tokio::test(start_paused = true)]
async fn exactly_the_row_limit_passes() {
    let fake = FakeAthena::new();
    fake.push(orders());
    let output = athena(&fake)
        .query("SELECT 1")
        .max_rows(2)
        .fetch()
        .await
        .unwrap();
    assert_eq!(output.rows.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_failed_query_gives_the_reason() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::failed("SYNTAX_ERROR: line 1:8"));
    let athena = athena(&fake);
    let err = athena.query("SELEC 1").fetch().await.unwrap_err();
    let AthenaError::Failed {
        query_id,
        reason,
        failure,
    } = err
    else {
        panic!("expected a failure, got {err:?}");
    };
    assert_eq!(query_id, "fake-1");
    assert_eq!(reason.as_deref(), Some("SYNTAX_ERROR: line 1:8"));
    assert_eq!(failure.unwrap().category, Some(2));
    assert!(fake.stopped().is_empty());
    assert_eq!(athena.open_queries(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_cancelled_query_is_an_error() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::cancelled());
    let err = athena(&fake).query("SELECT 1").execute().await.unwrap_err();
    assert_eq!(
        err,
        AthenaError::Cancelled {
            query_id: "fake-1".into()
        }
    );
}

#[tokio::test(start_paused = true)]
async fn unknown_states_keep_the_poll_going() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::succeeded().states([
        QueryState::Queued,
        QueryState::Unknown("PAUSED".into()),
        QueryState::Running,
        QueryState::Succeeded,
    ]));
    athena(&fake).query("SELECT 1").execute().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_timeout_stops_the_query() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    let athena = athena(&fake);
    let started = tokio::time::Instant::now();
    let err = athena
        .query("SELECT 1")
        .timeout(Duration::from_secs(5))
        .execute()
        .await
        .unwrap_err();
    assert_eq!(
        err,
        AthenaError::Timeout {
            query_id: "fake-1".into(),
            timeout: Duration::from_secs(5)
        }
    );
    assert_eq!(fake.stopped(), vec!["fake-1".to_owned()]);
    assert!(started.elapsed() <= Duration::from_secs(5) + Duration::from_millis(10));
    assert_eq!(athena.open_queries(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_start_error_is_an_api_error() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::start_error("access denied"));
    let err = athena(&fake).query("SELECT 1").execute().await.unwrap_err();
    assert!(
        matches!(err, AthenaError::Api(ref e) if e.operation == "StartQueryExecution"),
        "{err:?}"
    );
    assert!(fake.stopped().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_status_error_stops_the_query() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending().status_error("network"));
    let athena = athena(&fake);
    let err = athena.query("SELECT 1").execute().await.unwrap_err();
    assert!(matches!(err, AthenaError::Api(_)), "{err:?}");
    settle().await;
    assert_eq!(fake.stopped(), vec!["fake-1".to_owned()]);
    assert_eq!(athena.open_queries(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_results_error_is_an_api_error() {
    let fake = FakeAthena::new();
    fake.push(orders().results_error("throttled"));
    let err = athena(&fake).query("SELECT 1").fetch().await.unwrap_err();
    assert!(
        matches!(err, AthenaError::Api(ref e) if e.operation == "GetQueryResults"),
        "{err:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn execute_reads_no_results() {
    let fake = FakeAthena::new();
    fake.push(orders());
    let execution = athena(&fake).query("SELECT 1").execute().await.unwrap();
    assert_eq!(execution.query_id, "fake-1");
    assert_eq!(execution.statement_type, StatementType::Dml);
    assert!(fake.page_requests().is_empty());
}

/// Lets spawned tasks run.
async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn dropping_a_running_query_stops_it() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    let athena = athena(&fake);
    let task = tokio::spawn({
        let athena = athena.clone();
        async move { athena.query("SELECT 1").execute().await }
    });
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(athena.open_queries(), 1);
    task.abort();
    let _ = task.await;
    settle().await;
    assert_eq!(fake.stopped(), vec!["fake-1".to_owned()]);
    assert_eq!(athena.open_queries(), 0);
}

#[tokio::test(start_paused = true)]
async fn drop_does_not_stop_when_the_config_says_no() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    let mut config = config();
    config.cancel_on_drop = false;
    let athena = Athena::new(fake.clone(), config).unwrap();
    let task = tokio::spawn({
        let athena = athena.clone();
        async move { athena.query("SELECT 1").execute().await }
    });
    tokio::time::sleep(Duration::from_secs(1)).await;
    task.abort();
    let _ = task.await;
    settle().await;
    assert!(fake.stopped().is_empty());
    assert_eq!(athena.open_queries(), 0);
}

#[tokio::test(start_paused = true)]
async fn stop_all_stops_each_open_query() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    fake.push(FakeQuery::pending());
    let athena = athena(&fake);
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let athena = athena.clone();
            tokio::spawn(async move { athena.query("SELECT 1").execute().await })
        })
        .collect();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(athena.open_queries(), 2);
    athena.stop_all().await;
    let mut stopped = fake.stopped();
    stopped.sort();
    assert_eq!(stopped, vec!["fake-1".to_owned(), "fake-2".to_owned()]);
    for task in tasks {
        let err = task.await.unwrap().unwrap_err();
        assert!(matches!(err, AthenaError::Cancelled { .. }), "{err:?}");
    }
}

#[tokio::test(start_paused = true)]
async fn metrics_count_the_outcomes() {
    use autumn_web::actuator::MetricsSource;
    let fake = FakeAthena::new();
    fake.push(orders());
    fake.push(FakeQuery::failed("x"));
    let athena = athena(&fake);
    athena.query("SELECT 1").execute().await.unwrap();
    athena.query("SELECT 1").execute().await.unwrap_err();
    let families = athena.metrics().collect();
    let total = families
        .iter()
        .find(|f| f.name == "athena_queries_total")
        .unwrap();
    let count = |outcome: &str| {
        total
            .samples
            .iter()
            .find(|s| s.labels[0].1 == outcome)
            .unwrap()
            .value
    };
    assert!((count("succeeded") - 1.0).abs() < f64::EPSILON);
    assert!((count("failed") - 1.0).abs() < f64::EPSILON);
    let scanned = families
        .iter()
        .find(|f| f.name == "athena_data_scanned_bytes_total")
        .unwrap();
    assert!((scanned.samples[0].value - 1024.0).abs() < f64::EPSILON);
}

#[test]
fn new_rejects_an_invalid_config() {
    let mut config = config();
    config.page_size = 0;
    let err = Athena::new(FakeAthena::new(), config).unwrap_err();
    assert!(matches!(err, AthenaError::Config(_)), "{err:?}");
}
