//! The plugin in an Autumn test app.

use autumn_plugin_aws_athena::config::AthenaConfig;
use autumn_plugin_aws_athena::testing::{FakeAthena, FakeQuery};
use autumn_plugin_aws_athena::{Athena, AthenaError, AthenaPlugin};
use autumn_web::prelude::*;
use autumn_web::test::{TestApp, TestClient};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Order {
    id: i64,
    customer: String,
}

#[get("/orders")]
async fn orders(athena: Athena) -> AutumnResult<Json<Vec<Order>>> {
    let rows = athena
        .query("SELECT id, customer FROM orders WHERE customer <> ?")
        .bind("nobody")
        .fetch_as::<Order>()
        .await
        .map_err(AthenaError::into_autumn)?;
    Ok(Json(rows))
}

fn config() -> AthenaConfig {
    let mut config = AthenaConfig::default();
    config.database = Some("sales".into());
    config
}

fn app(fake: &FakeAthena, config: AthenaConfig) -> TestClient {
    TestApp::new()
        .routes(routes![orders])
        .plugin(AthenaPlugin::new().config(config).api(fake.clone()))
        .build()
}

#[tokio::test]
async fn a_handler_runs_a_query_with_the_extractor() {
    let fake = FakeAthena::new();
    fake.push(
        FakeQuery::succeeded()
            .columns(&[("id", "bigint"), ("customer", "varchar")])
            .row(&[Some("1"), Some("ada")]),
    );
    let client = app(&fake, config());
    let response = client.get("/orders").send().await;
    response.assert_ok();
    assert_eq!(
        response.json::<Vec<Order>>(),
        vec![Order {
            id: 1,
            customer: "ada".into()
        }]
    );
    let started = fake.started();
    assert_eq!(started[0].parameters, vec!["'nobody'".to_owned()]);
    assert_eq!(started[0].database.as_deref(), Some("sales"));
}

#[tokio::test]
async fn configure_changes_the_config() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::succeeded().columns(&[("id", "bigint"), ("customer", "varchar")]));
    let client = TestApp::new()
        .routes(routes![orders])
        .plugin(
            AthenaPlugin::new()
                .config(config())
                .configure(|c| c.workgroup = "reports".into())
                .api(fake.clone()),
        )
        .build();
    client.get("/orders").send().await.assert_ok();
    assert_eq!(fake.started()[0].workgroup, "reports");
}

#[tokio::test]
async fn a_failed_query_gives_a_server_error() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::failed("TABLE_NOT_FOUND"));
    let client = app(&fake, config());
    client.get("/orders").send().await.assert_status(500);
}

#[tokio::test]
async fn a_timeout_gives_a_gateway_timeout() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    let mut config = config();
    config.timeout_ms = 50;
    config.poll.initial_ms = 10;
    config.poll.max_ms = 10;
    let client = app(&fake, config);
    client.get("/orders").send().await.assert_status(504);
    // The plugin stops the query in a background task.
    for _ in 0..100 {
        if !fake.stopped().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(fake.stopped(), vec!["fake-1".to_owned()]);
}

#[tokio::test]
async fn the_extractor_fails_without_the_plugin() {
    let client = TestApp::new().routes(routes![orders]).build();
    client.get("/orders").send().await.assert_status(500);
}

#[tokio::test]
async fn from_state_gives_the_handle() {
    let fake = FakeAthena::new();
    let client = app(&fake, config());
    let athena = Athena::from_state(client.state()).expect("the plugin installs the handle");
    assert_eq!(athena.config().database.as_deref(), Some("sales"));
}

#[tokio::test]
async fn the_readiness_check_reads_the_workgroup() {
    let fake = FakeAthena::new();
    let client = app(&fake, config());
    let body = client.get("/actuator/health").send().await.text();
    assert!(body.contains("athena"), "{body}");
    assert!(!body.contains("DOWN"), "{body}");
}

#[tokio::test]
async fn a_failed_readiness_check_hides_the_aws_error() {
    let fake = FakeAthena::new();
    fake.fail_workgroup_check("AccessDenied: arn:aws:iam::123456789012:user/secret");
    let client = app(&fake, config());
    let body = client.get("/actuator/health").send().await.text();
    assert!(body.contains("DOWN"), "{body}");
    // The health output must not show AWS error details.
    assert!(!body.contains("123456789012"), "{body}");
}

#[tokio::test]
async fn no_readiness_check_when_the_config_says_no() {
    let fake = FakeAthena::new();
    fake.fail_workgroup_check("down");
    let mut config = config();
    config.health_check = false;
    let client = app(&fake, config);
    let body = client.get("/actuator/health").send().await.text();
    assert!(!body.contains("athena"), "{body}");
}

#[tokio::test]
async fn metrics_are_on_the_prometheus_endpoint() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::succeeded().columns(&[("id", "bigint"), ("customer", "varchar")]));
    let client = app(&fake, config());
    client.get("/orders").send().await.assert_ok();
    let body = client.get("/actuator/prometheus").send().await.text();
    assert!(
        body.contains("athena_queries_total{outcome=\"succeeded\"} 1"),
        "{body}"
    );
}

#[test]
#[should_panic(expected = "page_size")]
fn an_invalid_config_stops_the_boot() {
    let mut config = config();
    config.page_size = 5000;
    let _ = app(&FakeAthena::new(), config);
}

#[tokio::test]
async fn the_start_of_shutdown_stops_open_queries() {
    let fake = FakeAthena::new();
    fake.push(FakeQuery::pending());
    let client = app(&fake, config());
    let athena = Athena::from_state(client.state()).unwrap();
    let task = tokio::spawn(async move { athena.query("SELECT 1").execute().await });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    // Autumn marks the shutdown before it drains the requests.
    client.state().begin_shutdown_for_test();
    let err = task.await.unwrap().unwrap_err();
    assert!(
        matches!(
            err,
            AthenaError::ShuttingDown | AthenaError::Cancelled { .. }
        ),
        "{err:?}"
    );
    for _ in 0..100 {
        if !fake.stopped().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(fake.stopped()[0], "fake-1");
}
